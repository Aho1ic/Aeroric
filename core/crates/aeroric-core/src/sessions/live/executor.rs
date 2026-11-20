//! [`StructuredExecutor`]: the [`TaskExecutor`] the task service talks to. Runs
//! that a structured driver can take go to one (N2: Codex in a local
//! workspace, chosen by the host's [`Routing`]); everything else is delegated
//! unchanged to the PTY executor it wraps (N2 D2), so the task service and its
//! reducer see the same port as before.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use super::acp::driver::{AcpLauncher, AcpSession, AcpSessionSpec, TranscriptLocator};
use super::claude::driver::{ClaudeLauncher, ClaudeSession, ClaudeSessionSpec};
use super::codex::driver::{CodexSession, CodexSessionSpec};
use super::codex::server::{CodexServer, Launcher};
use super::omp::driver::{OmpLauncher, OmpSession, OmpSessionSpec};
use super::{now_ms, DriverRegistry, LiveStatusSink, SessionDriver, SharedLog};
use crate::tasks::executor::{
    CancelTarget, Execution, ExecutorFailure, LaunchRequest, ResetOutcome, ResumeRequest,
    TaskExecutor,
};
use crate::tasks::{ProtocolFamily, WorkingDirectory};

/// A session idle this long releases its process; the next prompt resumes it.
/// A session with background work never counts as idle (2026-11-07: 10 minutes
/// ended dev servers and long builds an agent had left running).
pub const SESSION_IDLE_MS: u64 = 60 * 60_000;

/// Whether idle sessions are released at all (设置 → 通用 → Agent 进程; on
/// unless the user turned it off).
pub trait IdleReleasePolicy: Send + Sync {
    fn enabled(&self) -> bool;
}
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// Which shared app-server a structured Codex run talks to: the built-in
/// agent's own, or one per agent configuration (N4b: each configuration has
/// its own managed home and gateway credentials).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ServerKey {
    Agent(String),
    Config(String),
    /// One remote `codex app-server` per (SSH connection, agent, configuration).
    Remote(RemoteKey),
}

/// A session on another machine over SSH (2026-10-27).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteKey {
    pub connection_id: String,
    pub agent: String,
    /// A pinned Agent configuration: the remote agent talks to this Mac's
    /// gateway through an SSH reverse forward (G1–G6, 2026-10-28).
    pub config: Option<String>,
}

/// Which Claude launch a run uses (N5): one process per session, so the key
/// only picks the program and environment — the built-in or custom agent, or
/// an agent configuration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClaudeKey {
    pub agent: String,
    pub config: Option<String>,
}

/// Where a structured run goes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OmpKey {
    pub agent: String,
    /// A pinned Agent configuration: omp talks to the local gateway.
    pub config: Option<String>,
}

/// An agent that speaks ACP (Q-R5): its family picks the dialect.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AcpKey {
    pub family: ProtocolFamily,
    pub agent: String,
    /// A pinned Agent configuration: the agent talks to the local gateway.
    pub config: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Codex(ServerKey),
    Claude(ClaudeKey),
    Omp(OmpKey),
    Acp(AcpKey),
    RemoteClaude(RemoteKey),
    RemoteOmp(RemoteKey),
}

impl Route {
    /// Whether the agent runs on this Mac and can start the local
    /// `aeroric-cli` context server. A remote `codex app-server` cannot.
    /// H2 (2026-11-20): OMP on this Mac can (its managed home's `mcp.json`).
    pub fn runs_locally(&self) -> bool {
        match self {
            Route::Codex(ServerKey::Remote(_)) => false,
            Route::Codex(_) | Route::Claude(_) | Route::Acp(_) | Route::Omp(_) => true,
            Route::RemoteClaude(_) | Route::RemoteOmp(_) => false,
        }
    }

    /// C8: the agent runs on another machine.
    pub fn is_remote(&self) -> bool {
        matches!(
            self,
            Route::Codex(ServerKey::Remote(_)) | Route::RemoteClaude(_) | Route::RemoteOmp(_)
        )
    }

    /// H1: the family of the harness the route starts.
    pub fn family(&self) -> ProtocolFamily {
        match self {
            Route::Codex(_) => ProtocolFamily::Codex,
            Route::Claude(_) | Route::RemoteClaude(_) => ProtocolFamily::Claude,
            Route::Omp(_) | Route::RemoteOmp(_) => ProtocolFamily::Omp,
            Route::Acp(key) => key.family,
        }
    }

    /// C8: the harness the route starts.
    pub fn harness(&self) -> &'static str {
        match self.family() {
            ProtocolFamily::Codex => "codex",
            ProtocolFamily::Claude => "claude",
            ProtocolFamily::Omp => "omp",
            ProtocolFamily::Dsh => "dsh",
        }
    }
}

/// What the host decides per run: whether `(family, agent, agent configuration)`
/// goes structured — and how — and how to start the agent.
pub trait Routing: Send + Sync {
    /// `None` keeps the run on the PTY executor.
    fn route(
        &self,
        family: ProtocolFamily,
        agent: &str,
        agent_config_id: Option<&str>,
    ) -> Option<Route>;
    fn codex_launcher(&self, key: &ServerKey) -> Launcher;
    fn claude_launcher(&self, key: &ClaudeKey) -> ClaudeLauncher;
    /// Runs before a Codex session opens its thread in `cwd` (B2 built-in skills).
    fn before_codex_session(&self, _key: &ServerKey, _cwd: &str) {}
    /// The `CLAUDE_CONFIG_DIR` this key's process writes transcripts under
    /// (`<dir>/projects/<encoded cwd>/<session id>.jsonl`); `None` records no path.
    fn claude_config_dir(&self, _key: &ClaudeKey) -> Option<std::path::PathBuf> {
        None
    }
    fn omp_launcher(&self, _key: &OmpKey) -> OmpLauncher {
        std::sync::Arc::new(|_| Err("这个宿主不支持 omp 原生会话".to_string()))
    }
    fn acp_launcher(&self, _key: &AcpKey) -> AcpLauncher {
        std::sync::Arc::new(|_| Err("这个宿主不支持 ACP 会话".to_string()))
    }
    /// Where an ACP agent started for `key` in `cwd` keeps its transcripts.
    fn acp_transcript(&self, _key: &AcpKey, _cwd: &str) -> Option<TranscriptLocator> {
        None
    }
    /// A session on an SSH workspace; `None` = the PTY executor.
    fn route_remote(
        &self,
        _family: ProtocolFamily,
        _agent: &str,
        _agent_config_id: Option<&str>,
        _connection_id: &str,
    ) -> Option<Route> {
        None
    }
    fn remote_claude_launcher(&self, _key: &RemoteKey) -> ClaudeLauncher {
        std::sync::Arc::new(|_| Err("这个宿主不支持远程原生会话".to_string()))
    }
    fn remote_omp_launcher(&self, _key: &RemoteKey) -> OmpLauncher {
        std::sync::Arc::new(|_| Err("这个宿主不支持远程原生会话".to_string()))
    }
    /// Where a remote Claude writes transcripts (`~` expands there).
    fn remote_claude_config_dir(&self, _key: &RemoteKey) -> std::path::PathBuf {
        std::path::PathBuf::from("~/.claude")
    }
}

/// B4′: a resumed session gets a brief only after this long without a turn.
const RESUME_BRIEF_AFTER_MS: u64 = 60 * 60 * 1000;

pub struct StructuredExecutor {
    inner: Arc<dyn TaskExecutor>,
    routing: Arc<dyn Routing>,
    sink: Arc<dyn LiveStatusSink>,
    /// A05: snapshots each session's repository at turn boundaries.
    turn_changes: Arc<super::turn_changes::TurnChangeTracker>,
    /// B4′: every task's checkpoint, derived from its updates.
    checkpoints: Arc<super::checkpoint::CheckpointSink>,
    /// B4-1: the review loops, between the delegations and the checkpoints.
    reviews: Arc<super::review::ReviewLoop>,
    /// B4-2: delegations, between the delivery queue and the review loops.
    delegations: Arc<super::delegation::Delegations>,
    /// R1-2: the delivery queue, right above the host's sink.
    deliveries: Arc<super::delivery::Deliveries>,
    /// 2026-11-12: discussions, above delegations.
    discussions: Arc<super::discussion::Discussions>,
    logins: Arc<super::login::Logins>,
    change_review: Arc<super::change_review::ChangeReview>,
    /// B4-3: the shared boards.
    boards: Arc<super::board::Boards>,
    /// B5-1: long-term memory (the profile a starting session is handed).
    memory: Arc<crate::memory::Memory>,
    /// B5.5-1: project Wikis.
    wikis: Arc<crate::knowledge::Wikis>,
    /// Q-R6: the reference library (one for every project).
    references: Arc<crate::knowledge::references::References>,
    /// C7 (2026-11-20): the skills each launch of a session mounted.
    skill_mounts: Arc<crate::extensions::catalog::MountLog>,
    /// C8 (2026-11-20): the tools each launch of a session had.
    tool_log: Arc<super::tool_catalog::ToolLog>,
    surface_log: Arc<super::surfaces::SurfaceLog>,
    /// B4′: how to start the context server (`program`, `data_dir`); unset
    /// gives sessions none (tests, and hosts that opt out).
    context_program: Mutex<Option<(String, String)>>,
    /// H6: the data folder whose `packs/` each launch is given from.
    packs_dir: Mutex<Option<std::path::PathBuf>>,
    context_tokens: Arc<super::context_server::ContextTokens>,
    runtime: tokio::runtime::Handle,
    registry: Arc<DriverRegistry>,
    /// CU 2026-10-31: each native session's (project, generation), so a
    /// cancel can settle the task even when no turn end will arrive.
    owners: Mutex<std::collections::HashMap<String, (String, u64)>>,
    servers: Mutex<HashMap<ServerKey, Arc<CodexServer>>>,
    claude_launchers: Mutex<HashMap<ClaudeKey, ClaudeLauncher>>,
    /// Unset releases idle sessions (tests, and hosts without the setting).
    idle_release: Mutex<Option<Arc<dyn IdleReleasePolicy>>>,
}

impl StructuredExecutor {
    /// Also starts the idle sweep on `runtime`; it stops when the executor drops.
    pub fn new(
        inner: Arc<dyn TaskExecutor>,
        routing: Arc<dyn Routing>,
        sink: Arc<dyn LiveStatusSink>,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let registry: Arc<DriverRegistry> = Arc::default();
        // Beneath the turn-change tracker, so it also sees the updates that
        // tracker appends after a turn (turnChanges, autoCheck).
        // R1-3a: below the queue, so it sees the queue's failures too.
        let logins = super::login::Logins::new(sink);
        let deliveries =
            super::delivery::Deliveries::new(Arc::clone(&logins) as Arc<dyn LiveStatusSink>);
        logins.set_deliveries(Arc::downgrade(&deliveries));
        deliveries.set_registry(Arc::clone(&registry));
        let sink: Arc<dyn LiveStatusSink> = Arc::clone(&deliveries) as Arc<dyn LiveStatusSink>;
        let delegations = super::delegation::Delegations::new(sink);
        delegations.set_registry(Arc::clone(&registry));
        // A1: result cards go through the queue and follow their delivery.
        delegations.follow(&deliveries);
        let sink: Arc<dyn LiveStatusSink> = Arc::clone(&delegations) as Arc<dyn LiveStatusSink>;
        let discussions = super::discussion::Discussions::new(sink);
        let sink: Arc<dyn LiveStatusSink> = Arc::clone(&discussions) as Arc<dyn LiveStatusSink>;
        let reviews = super::review::ReviewLoop::new(sink);
        reviews.set_registry(Arc::clone(&registry));
        let sink: Arc<dyn LiveStatusSink> = Arc::clone(&reviews) as Arc<dyn LiveStatusSink>;
        let checkpoints = super::checkpoint::CheckpointSink::new(sink);
        reviews.set_checkpoints(Arc::downgrade(&checkpoints));
        delegations.set_checkpoints(Arc::downgrade(&checkpoints));
        let sink: Arc<dyn LiveStatusSink> = Arc::clone(&checkpoints) as Arc<dyn LiveStatusSink>;
        let turn_changes = super::turn_changes::TurnChangeTracker::new(sink, Arc::clone(&registry));
        delegations.set_ledger(Arc::clone(&turn_changes.ledger));
        delegations.set_artifacts(Arc::clone(&turn_changes.artifacts));
        checkpoints.set_registry(Arc::clone(&registry));
        let weak = Arc::downgrade(&checkpoints);
        turn_changes.checks.set_extra(Arc::new(move |task: &str| {
            weak.upgrade()
                .map(|cp| cp.acceptance_checks(task))
                .unwrap_or_default()
        }));
        let weak = Arc::downgrade(&reviews);
        let weak_delegations = Arc::downgrade(&delegations);
        let memory: Arc<crate::memory::Memory> = Arc::default();
        let weak_memory = Arc::downgrade(&memory);
        let weak_tracker = Arc::downgrade(&turn_changes);
        turn_changes.set_hook(Arc::new(move |settled| {
            if let Some(delegations) = weak_delegations.upgrade() {
                delegations.turn_settled(settled.clone());
            }
            // B6 代码版本链: a turn that changed files, in a project that
            // uses memory, is split into blocks.
            if let (true, Some(memory), Some(tracker)) = (
                settled.diff.is_some(),
                weak_memory.upgrade(),
                weak_tracker.upgrade(),
            ) {
                if let (Some(project), Some(record)) = (
                    tracker.project_of(&settled.task_id),
                    tracker.ledger.get(&settled.task_id, &settled.turn_id),
                ) {
                    crate::memory::chain::record_turn(&memory, &project, &settled.task_id, &record);
                }
            }
            if let Some(reviews) = weak.upgrade() {
                reviews.turn_settled(settled);
            }
        }));
        let change_review = Arc::new(super::change_review::ChangeReview::new(Arc::clone(
            &turn_changes.ledger,
        )));
        let sink: Arc<dyn LiveStatusSink> =
            Arc::new(super::turn_changes::TrackingSink(Arc::clone(&turn_changes)));
        // B1-1: on top, so every read-only session's request is denied
        // whichever role (discussion, review, delegate) started it.
        let sink: Arc<dyn LiveStatusSink> = Arc::new(super::read_only::ReadOnlyGuard::new(
            sink,
            Arc::clone(&registry),
        ));
        let executor = Arc::new(Self {
            inner,
            routing,
            sink,
            turn_changes,
            checkpoints,
            reviews,
            delegations,
            deliveries,
            discussions,
            logins,
            change_review,
            boards: Arc::default(),
            memory,
            wikis: Arc::default(),
            references: Arc::default(),
            skill_mounts: Arc::default(),
            tool_log: Arc::default(),
            surface_log: Arc::default(),
            context_program: Mutex::new(None),
            packs_dir: Mutex::new(None),
            context_tokens: Arc::default(),
            runtime: runtime.clone(),
            registry,
            owners: Mutex::default(),
            servers: Mutex::default(),
            claude_launchers: Mutex::default(),
            idle_release: Mutex::default(),
        });
        let weak: Weak<Self> = Arc::downgrade(&executor);
        runtime.spawn(async move {
            let mut ticks = tokio::time::interval(SWEEP_EVERY);
            loop {
                ticks.tick().await;
                let Some(executor) = weak.upgrade() else {
                    return;
                };
                executor.sweep(now_ms()).await;
            }
        });
        executor
    }

    /// Live drivers, for the host's session methods (N3).
    /// A05: which projects run their tests / diagnostics after a turn.
    pub fn set_auto_check_policy(&self, policy: Arc<dyn super::auto_check::AutoCheckPolicy>) {
        self.turn_changes.checks.set_policy(policy);
    }

    /// Whether the sweep releases idle sessions.
    pub fn set_idle_release_policy(&self, policy: Arc<dyn IdleReleasePolicy>) {
        *self.idle_release.lock() = Some(policy);
    }

    /// B4′: give a session that is about to start the brief armed for its
    /// task (a source move), or — resuming after a long pause with something
    /// left open — a resume brief. Harnesses with a system layer take it
    /// there; otherwise it goes in front of `prompt` (a resume without a
    /// prompt then carries none). Returns the prompt to send.
    ///
    /// B5-1: the user's memory profile goes in front of the brief, but only
    /// into a system layer — never into the user's message. H3: the profile
    /// is one part of the opening (`opening.rs`), with what Aeroric offers
    /// and where to look.
    fn hand_brief(
        &self,
        task_id: &str,
        project_id: &str,
        session: &Arc<dyn SessionDriver>,
        prompt: Option<String>,
        resuming: bool,
        injection: super::tool_catalog::Injection,
    ) -> (Option<String>, super::surfaces::Opening) {
        let opening = self.opening(task_id, project_id, injection);
        self.hand_handoff(task_id, session, prompt, resuming, Some(&opening))
    }

    /// H3: the opening for one launch. The pointers name only what the
    /// session's tools can open, so a session without them gets none.
    fn opening(
        &self,
        task_id: &str,
        project_id: &str,
        injection: super::tool_catalog::Injection,
    ) -> String {
        use super::opening;
        let with_tools = injection == super::tool_catalog::Injection::Injected;
        let offered = self.offered_tools(project_id);
        let profile = self
            .memory
            .profile_for_session(Some(project_id), Some(task_id));
        let pointers = with_tools
            .then(|| {
                let wiki: Vec<String> = if offered.wiki {
                    self.wikis
                        .pages(project_id)
                        .map(|pages| pages.into_iter().map(|p| p.title).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let roles: Vec<String> = self
                    .delegations
                    .roles()
                    .into_iter()
                    .map(|r| r.name)
                    .collect();
                opening::pointers(&wiki, &roles)
            })
            .flatten();
        opening::compose(
            &opening::purpose(with_tools, offered),
            profile.as_deref(),
            pointers.as_deref(),
        )
    }

    fn hand_handoff(
        &self,
        task_id: &str,
        session: &Arc<dyn SessionDriver>,
        prompt: Option<String>,
        resuming: bool,
        opening: Option<&str>,
    ) -> (Option<String>, super::surfaces::Opening) {
        use super::surfaces::Opening;
        use aeroric_protocol::checkpoint::HandoffReason;
        let reason = self.checkpoints.take_armed(task_id).or_else(|| {
            (resuming
                && self
                    .checkpoints
                    .stale_with_open_work(task_id, RESUME_BRIEF_AFTER_MS))
            .then_some(HandoffReason::Resume)
        });
        let checkpoint =
            reason.and_then(|_| self.checkpoints.get(task_id).filter(|cp| !cp.is_empty()));
        let (Some(reason), Some(checkpoint)) = (reason, checkpoint) else {
            // H1: what became of the opening is kept with the launch.
            return match opening {
                None => (prompt, Opening::Nothing),
                Some(text) if session.set_instructions(text.to_string()) => {
                    (prompt, Opening::Taken(text.chars().count()))
                }
                // H3: no system layer: in front of the first message, in an
                // envelope; a resume with no message carries none.
                Some(text) => {
                    let opening = Opening::Refused {
                        brief: false,
                        in_message: prompt.is_some(),
                    };
                    (prompt.map(|p| super::opening::attach(text, &p)), opening)
                }
            };
        };
        let brief = super::checkpoint::brief::handoff_brief(&checkpoint, reason);
        if let Some(log) = self.registry.log(task_id) {
            let update = log.push(
                aeroric_protocol::session_live::SessionUpdateKind::Handoff {
                    reason,
                    checkpoint: Box::new(checkpoint),
                },
                now_ms(),
            );
            self.sink.update(&update);
        }
        let opening_text = opening;
        let instructions = match opening_text {
            Some(opening) => format!("{opening}\n\n---\n\n{brief}"),
            None => brief.clone(),
        };
        let chars = instructions.chars().count();
        if session.set_instructions(instructions) {
            return (prompt, Opening::Taken(chars));
        }
        let opening = Opening::Refused {
            brief: true,
            in_message: prompt.is_some(),
        };
        // H3: the opening and the brief together, in the same envelope.
        let context = match opening_text {
            Some(text) => format!("{text}\n\n---\n\n{brief}"),
            None => brief,
        };
        (
            prompt.map(|p| super::opening::attach(&context, &p)),
            opening,
        )
    }

    /// B4′: give every session the context server, started as
    /// `program --data-dir data_dir mcp-context`.
    pub fn set_context_program(&self, program: String, data_dir: String) {
        *self.context_program.lock() = Some((program, data_dir));
    }

    /// H6: give every launch on this Mac the capability packs under
    /// `<data_dir>/packs/`.
    pub fn set_packs_dir(&self, data_dir: std::path::PathBuf) {
        *self.packs_dir.lock() = Some(data_dir);
    }

    /// H6: plan the packs for this launch and hand the driver its servers;
    /// every item, given or not, with why.
    fn give_packs(
        &self,
        route: &Route,
        session: &Arc<dyn SessionDriver>,
    ) -> Vec<aeroric_protocol::packs::PackItem> {
        use crate::extensions::packs;
        let Some(data_dir) = self.packs_dir.lock().clone() else {
            return Vec::new();
        };
        let (found, broken) = packs::load(&data_dir);
        for (folder, why) in broken {
            eprintln!("[aeroric] capability pack {folder} skipped: {why}");
        }
        let mut plan = packs::plan(
            &found,
            packs::Target {
                family: route.family(),
                remote: !route.runs_locally(),
            },
        );
        if !plan.servers.is_empty() && !session.set_pack_servers(plan.servers) {
            packs::refused(&mut plan.items, route.family());
        }
        plan.items
    }

    pub fn context_tokens(&self) -> &Arc<super::context_server::ContextTokens> {
        &self.context_tokens
    }

    /// Hand a starting session its context server and a fresh token; C8:
    /// whether it took it.
    fn give_context(
        &self,
        task_id: &str,
        project_id: &str,
        session: &Arc<dyn SessionDriver>,
    ) -> super::tool_catalog::Injection {
        use super::tool_catalog::Injection;
        let Some((program, data_dir)) = self.context_program.lock().clone() else {
            return Injection::NoServer;
        };
        let token = self.context_tokens.issue(task_id, project_id);
        let server = super::context_server::ContextServer {
            command: program,
            args: vec!["--data-dir".into(), data_dir, "mcp-context".into()],
            env: vec![(super::context_server::TOKEN_ENV.into(), token)],
        };
        if !session.set_context_server(server) {
            self.context_tokens.revoke(task_id);
            return Injection::Unsupported;
        }
        Injection::Injected
    }

    /// B4′: the tasks' checkpoints (the host sets where they are kept).
    pub fn checkpoints(&self) -> &Arc<super::checkpoint::CheckpointSink> {
        &self.checkpoints
    }

    /// B4-1: the review loops (the host sets their host and directory).
    pub fn reviews(&self) -> &Arc<super::review::ReviewLoop> {
        &self.reviews
    }

    pub fn deliveries(&self) -> &Arc<super::delivery::Deliveries> {
        &self.deliveries
    }

    pub fn logins(&self) -> &Arc<super::login::Logins> {
        &self.logins
    }

    /// R2-1: the turns that changed files.
    pub fn turn_ledger(&self) -> &Arc<super::turn_ledger::TurnLedger> {
        &self.turn_changes.ledger
    }

    /// B6: the files each task delivered.
    pub fn artifacts(&self) -> &Arc<super::artifacts::ArtifactRegistry> {
        &self.turn_changes.artifacts
    }

    /// B6: where `task_id`'s agent works, once its session started.
    pub fn task_cwd(&self, task_id: &str) -> Option<std::path::PathBuf> {
        self.turn_changes.cwd_of(task_id)
    }

    /// R2-2: hunk-by-hunk review of those changes.
    pub fn change_review(&self) -> &Arc<super::change_review::ChangeReview> {
        &self.change_review
    }

    pub fn discussions(&self) -> &Arc<super::discussion::Discussions> {
        &self.discussions
    }

    pub fn delegations(&self) -> &Arc<super::delegation::Delegations> {
        &self.delegations
    }

    pub fn boards(&self) -> &Arc<super::board::Boards> {
        &self.boards
    }

    pub fn memory(&self) -> &Arc<crate::memory::Memory> {
        &self.memory
    }

    pub fn wikis(&self) -> &Arc<crate::knowledge::Wikis> {
        &self.wikis
    }

    pub fn skill_mounts(&self) -> &Arc<crate::extensions::catalog::MountLog> {
        &self.skill_mounts
    }

    pub fn tool_log(&self) -> &Arc<super::tool_catalog::ToolLog> {
        &self.tool_log
    }

    /// H1: each launch's surfaces.
    pub fn surface_log(&self) -> &Arc<super::surfaces::SurfaceLog> {
        &self.surface_log
    }

    /// C8: what the context server's conditional families are offered on,
    /// asked as `aeroric-cli mcp-context` asks the host.
    fn offered_tools(&self, project_id: &str) -> super::tool_catalog::Offered {
        super::tool_catalog::Offered {
            memory: self.memory.project_enabled(project_id),
            wiki: self.wikis.has_pages(project_id) && self.wikis.agent_access(project_id),
            references: !self.references.list().is_empty(),
            expert: self.delegations.expert().is_some(),
        }
    }

    /// C7: a launch on this Mac keeps what skills it is given (a remote
    /// one is given none by Aeroric). C8: OMP on this Mac is mounted too.
    /// H1: how many of Aeroric's own skills it was given (`None`: not kept).
    fn note_skills(
        &self,
        task_id: &str,
        family: ProtocolFamily,
        route: &Route,
        cwd: &str,
    ) -> Option<usize> {
        if route.is_remote() {
            return None;
        }
        let harness = match family {
            ProtocolFamily::Codex => "codex",
            ProtocolFamily::Claude => "claude",
            ProtocolFamily::Omp => "omp",
            ProtocolFamily::Dsh => "dsh",
        };
        let home = crate::settings::home_dir();
        self.skill_mounts.note(
            task_id,
            harness,
            Some(std::path::Path::new(cwd)),
            home.as_deref(),
            crate::clock::now_ms_u64(),
        )
    }

    /// H1: keep what this launch had on each surface.
    fn note_surfaces(&self, task_id: &str, route: &Route, observed: super::surfaces::Observed) {
        let place = if route.is_remote() {
            aeroric_protocol::live_capability::Place::Remote
        } else {
            aeroric_protocol::live_capability::Place::Local
        };
        self.surface_log.note(
            task_id,
            super::surfaces::launch(
                route.family(),
                place,
                route.harness(),
                &observed,
                crate::clock::now_ms_u64(),
            ),
        );
    }

    pub fn references(&self) -> &Arc<crate::knowledge::references::References> {
        &self.references
    }

    /// Tasks whose latest check run failed (the attention queue).
    pub fn check_failures(&self) -> Vec<super::auto_check::CheckFailure> {
        self.turn_changes.checks.failures()
    }

    pub fn registry(&self) -> &Arc<DriverRegistry> {
        &self.registry
    }

    /// Suspend sessions idle for [`SESSION_IDLE_MS`] (unless the user turned
    /// that off), then stop servers nobody uses.
    pub async fn sweep(&self, now: u64) {
        let policy = self.idle_release.lock().clone();
        let release = policy.is_none_or(|policy| policy.enabled());
        for (_, driver) in self.registry.all() {
            if release
                && driver
                    .idle_since_ms()
                    .is_some_and(|since| now.saturating_sub(since) >= SESSION_IDLE_MS)
            {
                driver.suspend();
            }
        }
        self.registry.prune_logs(now);
        let servers: Vec<_> = self.servers.lock().values().cloned().collect();
        for server in servers {
            server.stop_if_unused(now).await;
        }
    }

    fn server(&self, key: &ServerKey) -> Arc<CodexServer> {
        let mut servers = self.servers.lock();
        Arc::clone(
            servers
                .entry(key.clone())
                .or_insert_with(|| CodexServer::new(self.routing.codex_launcher(key))),
        )
    }

    /// Where a run with these facts goes; `None` = the PTY executor.
    fn route_task(
        &self,
        family: ProtocolFamily,
        agent: &str,
        config: Option<&str>,
        cwd: &WorkingDirectory,
    ) -> Option<Route> {
        if !matches!(
            family,
            ProtocolFamily::Codex
                | ProtocolFamily::Claude
                | ProtocolFamily::Omp
                | ProtocolFamily::Dsh
        ) {
            return None;
        }
        let route = match cwd {
            WorkingDirectory::Local { .. } => self.routing.route(family, agent, config),
            WorkingDirectory::Ssh { connection_id, .. } => {
                self.routing
                    .route_remote(family, agent, config, connection_id)
            }
            WorkingDirectory::Wsl { .. } => None,
        };
        // An ACP route needs its family's dialect; without one it stays on the PTY.
        route.filter(|route| match route {
            Route::Acp(key) => super::acp::dialect::for_family(key.family).is_some(),
            _ => true,
        })
    }

    fn claude_launcher(&self, key: &ClaudeKey) -> ClaudeLauncher {
        let mut launchers = self.claude_launchers.lock();
        Arc::clone(
            launchers
                .entry(key.clone())
                .or_insert_with(|| self.routing.claude_launcher(key)),
        )
    }

    /// Whether a run with these facts goes to a structured driver.
    pub fn takes_task(
        &self,
        family: ProtocolFamily,
        agent: &str,
        config: Option<&str>,
        cwd: &WorkingDirectory,
    ) -> bool {
        self.route_task(family, agent, config, cwd).is_some()
    }

    fn start_session(
        &self,
        spec: CodexSessionSpec,
        route: &Route,
    ) -> (Arc<dyn SessionDriver>, super::tool_catalog::Injection) {
        if let Some(old) = self.registry.remove(&spec.task_id) {
            let _ = old.close();
        }
        self.owners.lock().insert(
            spec.task_id.clone(),
            (spec.project_id.clone(), spec.generation),
        );
        let log = self
            .registry
            .log(&spec.task_id)
            .unwrap_or_else(|| SharedLog::new(&spec.task_id));
        // A05 snapshots the working tree with local git; a remote one is not here.
        let remote = matches!(
            route,
            Route::RemoteClaude(_) | Route::RemoteOmp(_) | Route::Codex(ServerKey::Remote(_))
        );
        if !remote {
            self.turn_changes
                .watch(&spec.task_id, &spec.project_id, &spec.cwd);
        }
        let session: Arc<dyn SessionDriver> = match route {
            Route::Codex(key) => {
                // B2: a direct Codex session gets the built-in skills in its project.
                self.routing.before_codex_session(key, &spec.cwd);
                CodexSession::start(
                    spec.clone(),
                    self.server(key),
                    Arc::clone(&self.sink),
                    log.clone(),
                    &self.runtime,
                )
            }
            Route::Claude(key) => ClaudeSession::start(
                ClaudeSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session_id: spec.thread_id.clone(),
                    config_dir: self.routing.claude_config_dir(key),
                },
                self.claude_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
            Route::RemoteClaude(key) => ClaudeSession::start(
                ClaudeSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session_id: spec.thread_id.clone(),
                    // Transcript paths are the remote machine's (`~` expands there).
                    config_dir: Some(self.routing.remote_claude_config_dir(key)),
                },
                self.routing.remote_claude_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
            Route::RemoteOmp(key) => OmpSession::start(
                OmpSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session: spec.thread_id.clone(),
                    remote: true,
                },
                self.routing.remote_omp_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
            Route::Acp(key) => match super::acp::dialect::for_family(key.family) {
                Some(dialect) => AcpSession::start(
                    AcpSessionSpec {
                        task_id: spec.task_id.clone(),
                        project_id: spec.project_id.clone(),
                        generation: spec.generation,
                        agent: spec.agent.clone(),
                        cwd: spec.cwd.clone(),
                        permission: spec.permission.clone(),
                        model: spec.model.clone(),
                        effort: spec.effort.clone(),
                        session: spec.thread_id.clone(),
                        dialect,
                        transcript: self.routing.acp_transcript(key, &spec.cwd),
                    },
                    self.routing.acp_launcher(key),
                    Arc::clone(&self.sink),
                    log.clone(),
                    &self.runtime,
                ),
                None => unreachable!("an ACP route is only made for a family with a dialect"),
            },
            Route::Omp(key) => OmpSession::start(
                OmpSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session: spec.thread_id.clone(),
                    remote: false,
                },
                self.routing.omp_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
        };
        // C8: what tools this launch has is kept beside it.
        let injection = if route.is_remote() {
            super::tool_catalog::Injection::Remote
        } else if route.runs_locally() {
            self.give_context(&spec.task_id, &spec.project_id, &session)
        } else {
            super::tool_catalog::Injection::Unsupported
        };
        let mut tools = super::tool_catalog::catalog(
            route.harness(),
            injection,
            self.offered_tools(&spec.project_id),
            crate::clock::now_ms_u64(),
        );
        tools.packs = self.give_packs(&route, &session);
        self.tool_log.note(&spec.task_id, tools);
        self.registry
            .insert(&spec.task_id, Arc::clone(&session), log);
        (session, injection)
    }

    /// Close and drop a task's driver; false when it has none.
    pub fn close_session(&self, task_id: &str) -> bool {
        self.context_tokens.revoke(task_id);
        match self.registry.remove(task_id) {
            Some(driver) => {
                let _ = driver.close();
                true
            }
            None => false,
        }
    }
}

/// Let the settings catalog ask the installed Codex for its models (N4)
/// through this executor's shared app-server. Process-wide, so only the
/// product wiring calls it. A catalog read happens on host worker threads, so
/// the request runs on its own thread and is bounded.
pub fn install_model_fetcher(executor: &Arc<StructuredExecutor>) {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(15);
    let weak = Arc::downgrade(executor);
    super::codex::models::install_fetcher(Arc::new(move |agent: &str| {
        let executor = weak.upgrade().ok_or("Aeroric 的会话服务已停止")?;
        let server = executor.server(&ServerKey::Agent(agent.to_string()));
        let runtime = executor.runtime.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(runtime.block_on(super::codex::models::list_models(&server)));
        });
        rx.recv_timeout(LIMIT)
            .unwrap_or_else(|_| Err("读取 Codex 模型目录超时；请确认 Codex 已安装并登录。".into()))
    }));
}

fn local_path(cwd: &WorkingDirectory) -> String {
    match cwd {
        WorkingDirectory::Local { path } => path.clone(),
        WorkingDirectory::Ssh { remote_path, .. } => remote_path.clone(),
        WorkingDirectory::Wsl { path, .. } => path.clone(),
    }
}

impl TaskExecutor for StructuredExecutor {
    fn launch(&self, request: &LaunchRequest) -> Execution<()> {
        let config = request.agent_config_id.as_deref();
        let Some(route) = self.route_task(
            request.family,
            &request.agent,
            config,
            &request.working_directory,
        ) else {
            return self.inner.launch(request);
        };
        let (session, injection) = self.start_session(
            CodexSessionSpec {
                task_id: request.task_id.clone(),
                project_id: request.project_id.clone(),
                generation: request.generation,
                agent: request.agent.clone(),
                cwd: local_path(&request.working_directory),
                extra_dirs: request.linked_folders.clone(),
                permission: request.permission_mode_wire.clone(),
                model: super::codex::models::chosen_model(request.selected_model.as_ref()),
                effort: request.reasoning_effort.clone(),
                thread_id: request.session_id.clone(),
            },
            &route,
        );
        let skills = self.note_skills(
            &request.task_id,
            request.family,
            &route,
            &local_path(&request.working_directory),
        );
        self.checkpoints
            .note_prompt(&request.task_id, &request.prompt);
        let prompt = request
            .prompt_override
            .clone()
            .unwrap_or_else(|| request.prompt.clone());
        let (prompt, opening) = self.hand_brief(
            &request.task_id,
            &request.project_id,
            &session,
            Some(prompt),
            false,
            injection,
        );
        self.note_surfaces(
            &request.task_id,
            &route,
            super::surfaces::Observed {
                opening,
                injection,
                skills,
            },
        );
        let prompt = prompt.unwrap_or_default();
        let started = if prompt.trim().is_empty() {
            session.attach()
        } else {
            session.prompt(prompt)
        };
        started.map_err(|error| ExecutorFailure::Spawn {
            reason: error.to_string(),
        })
    }

    fn resume(&self, request: &ResumeRequest) -> Execution<()> {
        let config = request.agent_config_id.as_deref();
        let Some(route) = self.route_task(
            request.family,
            &request.agent,
            config,
            &request.working_directory,
        ) else {
            return self.inner.resume(request);
        };
        // Codex app-server resumes `history_mode = "paginated"` threads — 0.163
        // writes every thread it starts that way — so they are not refused here
        // (N6a, reversing N6 D3 after the real smoke).
        if let Some(driver) = self.registry.get(&request.task_id) {
            // Still open here: the same session continues under the new generation.
            driver.rebind(request.generation);
            if let Some(owner) = self.owners.lock().get_mut(&request.task_id) {
                owner.1 = request.generation;
            }
            return driver.attach().map_err(|error| ExecutorFailure::Spawn {
                reason: error.to_string(),
            });
        }
        let (session, injection) = self.start_session(
            CodexSessionSpec {
                task_id: request.task_id.clone(),
                project_id: request.project_id.clone(),
                generation: request.generation,
                agent: request.agent.clone(),
                cwd: local_path(&request.working_directory),
                extra_dirs: request.linked_folders.clone(),
                permission: request.permission_mode_wire.clone(),
                model: super::codex::models::chosen_model(request.selected_model.as_ref()),
                effort: request.reasoning_effort.clone(),
                thread_id: Some(request.session_id.clone()),
            },
            &route,
        );
        let skills = self.note_skills(
            &request.task_id,
            request.family,
            &route,
            &local_path(&request.working_directory),
        );
        let (_, opening) = self.hand_brief(
            &request.task_id,
            &request.project_id,
            &session,
            None,
            true,
            injection,
        );
        self.note_surfaces(
            &request.task_id,
            &route,
            super::surfaces::Observed {
                opening,
                injection,
                skills,
            },
        );
        session.attach().map_err(|error| ExecutorFailure::Spawn {
            reason: error.to_string(),
        })
    }

    fn cancel(&self, task_id: &str, target: &CancelTarget) -> Execution<()> {
        // CU 2026-10-31: closing a native session sends no turn end (it
        // unsubscribes first), so the task stayed `running` after 停止 —
        // also when its process had already gone. Settle it here.
        // R1-2 stop barrier (CU 2026-11-02, F2): the toolbar's 停止 comes
        // here, not through `session.interrupt`.
        self.deliveries.pause(task_id);
        let owner = self.owners.lock().remove(task_id);
        let closed = self.close_session(task_id);
        if let Some((project_id, generation)) = owner.clone() {
            // Off this call: the task service holds its lock while it cancels,
            // and recording the status takes the same lock.
            let sink = Arc::clone(&self.sink);
            let task = task_id.to_string();
            self.runtime.spawn_blocking(move || {
                sink.status(
                    &project_id,
                    &task,
                    generation,
                    aeroric_protocol::task::TaskStatus::Cancelled,
                    None,
                );
            });
        }
        if closed || owner.is_some() {
            return Ok(());
        }
        self.inner.cancel(task_id, target)
    }

    fn complete(&self, task_id: &str, working_directory: &str) -> Execution<()> {
        if self.close_session(task_id) {
            return Ok(());
        }
        self.inner.complete(task_id, working_directory)
    }

    fn reset(&self, task_id: &str) -> Execution<ResetOutcome> {
        if self.close_session(task_id) {
            return Ok(ResetOutcome::default());
        }
        self.inner.reset(task_id)
    }
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;
