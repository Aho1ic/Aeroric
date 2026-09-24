//! The Tauri host's entry into the native core (Task P2-05).
//!
//! This module owns exactly one decision at start-up — *may this process be the business
//! writer of this data directory* — and then hands the answer to everyone else. It is
//! deliberately not the place where task logic lives: `TaskService` decides what a write
//! means, `LegacyTaskExecutor` decides how a process starts, and this file only decides
//! whether we are allowed to be the one doing either.
//!
//! Two ADRs shape it:
//!
//! - **ADR-021**: the writer role is handed over on macOS only. Windows/Linux keep the
//!   shipping client as sole writer until P7, because that is where the product is
//!   actually tested. Crucially the *platform* judgement lives here, not in core —
//!   `StorageLocation::writer_eligible()` keeps answering only "is this location worthy",
//!   and core stays free of `#[cfg(target_os)]` politics.
//! - **ADR-027**: a second instance that cannot take the lease starts **read-only** and
//!   says so. It never steals the lease (a lease is released by its holder, and the plan
//!   forbids killing a foreign PID), and it never falls back to a temp directory — two
//!   divergent data sets that both look healthy is the one outcome worse than refusing.
//!
//! The mode decision is split out as [`decide_mode`] so the platform × lease-outcome
//! matrix is testable without touching a real filesystem.
//!
//! `install_core_services` runs from `lib.rs`'s setup hook, so the gateway is constructed on the
//! product path and the writer lease **is** taken on macOS from this point. That is deliberate and
//! it is not yet the writer switch: `storage.rs` still saves the task table, and nothing routes a
//! mutation through `TaskService`. What changes is that the refusal is now real — a second instance
//! starts read-only and says so — and that core's cover fingerprints start recording what the
//! legacy save path does to the same files, which is the counter-example ADR-011 and acceptance #3
//! ask for. Items still unused by the product path carry a targeted `dead_code` allowance at the
//! item, never a module-wide one: an allow whose stated reason has become false is worse than a
//! warning.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::{
    tasks::{
        AgentFamilies, LiveRuns, ProtocolFamily, TaskEvent, TaskEventSink, TaskNotification,
        TaskRuntimeObserver, TaskService, TaskSessionFacts, TaskStatus,
    },
    AppPaths, Clock, CoreContext, LeaseError, LeaseOwner, SystemClock,
};

use crate::startup_diagnostics;
use crate::storage_location::PlatformLocationProbe;

/// Why this process is or is not the writer. Every variant is a *statement*, and the
/// three non-`Writer` ones each carry a different user-visible consequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreMode {
    /// We hold the lease and may write.
    Writer,
    /// ADR-021: this platform has not been handed the role yet. Not a fault, and the
    /// shipping client is *still* writing — so unlike the variants below nothing is
    /// degraded, the core is simply a reader here.
    HandoverDeferred,
    /// ADR-027: another live Aeroric process holds the lease.
    ReadOnlyBusy,
    /// ADR-010: this location was refused (unprobeable volume, network mount, no
    /// permission receipt). Reported, because a silent refusal here would look like a
    /// working installation that never saves.
    ReadOnlyLocation,
}

impl CoreMode {
    #[allow(
        dead_code,
        reason = "读它的是 CoreGateway 上的同名转发，那条路要等第一个走 core 的写入口"
    )]
    pub(crate) fn is_writer(self) -> bool {
        self == CoreMode::Writer
    }

    /// The component name the frontend matches on to pick its wording.
    pub(crate) fn diagnostics_component(self) -> Option<&'static str> {
        match self {
            CoreMode::ReadOnlyBusy => Some("storage-lease"),
            CoreMode::ReadOnlyLocation => Some("storage-location"),
            _ => None,
        }
    }
}

/// The lease attempt, reduced to what the mode decision needs. Keeping this separate from
/// `LeaseError` is what lets the whole matrix be tested without creating a real directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Acquisition {
    #[allow(
        dead_code,
        reason = "生产路径在 Ok(acquisition) 分支里直接给 CoreMode::Writer；该变体只被 decide_mode 的四格矩阵用例构造"
    )]
    Acquired,
    HeldByAnother,
    Refused,
}

/// ADR-021: the one place that knows which platforms have been handed the writer role.
/// P7-01/P7-02 is where this stops being platform-conditional.
pub(crate) fn writer_handover_allowed() -> bool {
    cfg!(target_os = "macos")
}

/// Pure decision behind [`CoreGateway::startup`]: hand-over policy first, then whatever
/// the lease said. Order matters — on a deferred platform we never even attempt the
/// lease, so a Windows install cannot fail in a new way just because this code landed.
pub(crate) fn decide_mode(handover_allowed: bool, acquisition: Acquisition) -> CoreMode {
    if !handover_allowed {
        return CoreMode::HandoverDeferred;
    }
    match acquisition {
        Acquisition::Acquired => CoreMode::Writer,
        Acquisition::HeldByAnother => CoreMode::ReadOnlyBusy,
        Acquisition::Refused => CoreMode::ReadOnlyLocation,
    }
}

/// The host's single handle on the core. `None` context is impossible by construction: a
/// non-writer still gets a read-only `CoreContext`, so callers never branch on "did the
/// core come up" — only on `is_writer()`.
pub(crate) struct CoreGateway {
    context: Arc<CoreContext>,
    mode: CoreMode,
    /// Kept because `CoreContext` exposes no data-directory getter: the refusal messages
    /// must name the directory this instance will *not* write.
    data_dir: PathBuf,
    /// One start-up notice core itself produced (a cloud-synced root, ADR-010 decision 1).
    location_warning: Option<String>,
}

impl CoreGateway {
    /// Takes the writer role where ADR-021 allows it and the lease is free; otherwise
    /// starts as a reader and records why.
    pub(crate) fn startup(paths: AppPaths, version: &str) -> Self {
        let data_dir = paths.data_dir().to_path_buf();
        if !writer_handover_allowed() {
            // No lease attempt at all: `CoreContext::read_only` touches nothing.
            return Self {
                context: Arc::new(CoreContext::read_only(paths)),
                mode: CoreMode::HandoverDeferred,
                data_dir,
                location_warning: None,
            };
        }

        let owner = LeaseOwner {
            application: "aeroric".to_string(),
            version: version.to_string(),
        };
        let acquisition = CoreContext::acquire(
            paths.clone(),
            Arc::new(SystemClock) as Arc<dyn Clock>,
            &owner,
            &PlatformLocationProbe,
        );

        match acquisition {
            Ok(context) => {
                let location_warning = context.location_warning();
                Self {
                    context: Arc::new(context),
                    mode: CoreMode::Writer,
                    data_dir,
                    location_warning,
                }
            }
            Err(error) => {
                let mode = decide_mode(
                    true,
                    match error {
                        LeaseError::AlreadyInUse { .. } => Acquisition::HeldByAnother,
                        _ => Acquisition::Refused,
                    },
                );
                // core's own `Display` already names the directory and the holder, so the
                // record carries it verbatim rather than a paraphrase.
                let reason = error.to_string();
                if let Some(component) = mode.diagnostics_component() {
                    // `fallback` is the data directory: nothing was relocated, and the
                    // honest value here is *which* directory this instance will not write.
                    startup_diagnostics::record(
                        component,
                        reason.clone(),
                        data_dir.display().to_string(),
                    );
                }
                Self {
                    context: Arc::new(CoreContext::read_only(paths)),
                    mode,
                    data_dir,
                    location_warning: Some(reason),
                }
            }
        }
    }

    /// The start-up entry point: resolve the user home the way the rest of the host does, then
    /// hand the decision to [`CoreGateway::startup`].
    ///
    /// A missing home is a degradation, not a panic. `CoreContext::read_only` touches nothing on
    /// disk — no lease, no directory creation — so an unusable placeholder path is safe to carry
    /// purely for the message, and every later write still refuses through `require_write`.
    pub(crate) fn at_startup(version: &str) -> Self {
        let Some(home) = crate::platform::home_dir() else {
            let reason = "无法定位用户主目录，原生 core 本次启动只读".to_string();
            startup_diagnostics::record("storage-location", reason.clone(), String::new());
            return Self {
                context: Arc::new(CoreContext::read_only(AppPaths::from_user_home(""))),
                mode: CoreMode::ReadOnlyLocation,
                data_dir: PathBuf::from("~"),
                location_warning: Some(reason),
            };
        };
        Self::startup(AppPaths::from_user_home(home), version)
    }

    pub(crate) fn context(&self) -> &Arc<CoreContext> {
        &self.context
    }

    /// The directory this instance will or will not write. ADR-027's persistent read-only
    /// indicator has to name it, and a refusal that names nothing is not an explanation.
    #[allow(
        dead_code,
        reason = "只读的常驻横幅（ADR-027）与它的测试都要这个名字；今天只有 startup_wiring 用例在读"
    )]
    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    #[allow(
        dead_code,
        reason = "UI 侧的只读横幅要等 Action 3；ADR-027 要求的『持续可见』就是它，先留类型不留调用"
    )]
    pub(crate) fn mode(&self) -> CoreMode {
        self.mode
    }

    #[allow(
        dead_code,
        reason = "第一个经 core 的写入口就是它的调用方；今天 mutation 仍全走 storage.rs"
    )]
    pub(crate) fn is_writer(&self) -> bool {
        self.mode.is_writer()
    }

    /// What the host owes the user at start-up: the reason we are not writing, and/or the
    /// cloud-sync caveat. `None` means there is genuinely nothing to report.
    #[allow(
        dead_code,
        reason = "与 ADR-010 的云端目录告知同一个出口，接进 UI 时与 mode() 一起用"
    )]
    pub(crate) fn notice(&self) -> Option<&str> {
        self.location_warning.as_deref()
    }

    /// The single gate every mutation in the adapter passes through.
    ///
    /// It exists so "read-only" is one decision rather than N: a command that forgets to
    /// check still cannot write, because `CoreContext::require_write` inside core refuses
    /// without a lease. This function only turns that into a message the UI can act on.
    #[allow(
        dead_code,
        reason = "真正的兜底是 core 的 require_write，本函数只把理由说给人听；第一个经 core 的写入口即调用方"
    )]
    pub(crate) fn require_writer(&self) -> Result<(), String> {
        if self.is_writer() {
            return Ok(());
        }
        let dir = self.data_dir.display();
        Err(match self.mode {
            CoreMode::ReadOnlyBusy => {
                format!("another Aeroric instance is using {dir}; this window is read-only")
            }
            CoreMode::ReadOnlyLocation => {
                format!("{dir} is not a location Aeroric will write to; this window is read-only")
            }
            CoreMode::HandoverDeferred => {
                format!("the native core does not own writes on this platform yet; {dir}")
            }
            CoreMode::Writer => unreachable!("handled by is_writer()"),
        })
    }
}

/// The host's answer to "which family is this agent", asked twice (ADR-028).
///
/// **Launch** must not be a table here. `claude_gpt55` points at a shell script
/// (`app_settings/builtin.rs:196` → `~/.claude/start-gpt55.sh`), and
/// `app_settings/launch_spec.rs:400-406` infers codex-vs-claude from that script's
/// *contents* at run time — so any hardcoded answer here would override a real user's
/// configuration and change what gets executed. Delegating to the same function `pty.rs`
/// selects on is the only version of "equivalent to today" that is actually true.
///
/// **Session** is a table, because that side is not dynamic: `session.rs:557` states the
/// built-in `claude`/`claude_gpt55` keep Claude-format history, and a custom profile
/// carries an explicit `family` whose legacy fallback is `codex_like` — the same
/// precedence `config_bundles.rs:100-108` uses.
/// The host's answer to "which family is this agent", asked twice (ADR-028).
///
/// **Launch is delegated, never tabulated here.** `claude_gpt55` points at a shell script
/// (`app_settings/builtin.rs:196` → `~/.claude/start-gpt55.sh`), and
/// `app_settings/launch_spec.rs:400-406` infers codex-vs-claude from that script's
/// *contents* at run time. A hardcoded table would override a real user's configuration and
/// change which command line actually runs, so this asks the same function `pty.rs` selects
/// on — the only version of "equivalent to today" that is true.
///
/// **Session is a table**, because that half is not dynamic: `session.rs:557` says the
/// built-in `claude`/`claude_gpt55` keep Claude-format history, and a custom profile
/// carries an explicit `family` whose legacy fallback is `codex_like`.
pub(crate) struct SettingsFamilies;

impl SettingsFamilies {
    fn built_in_session_family(agent: &str) -> Option<ProtocolFamily> {
        match agent {
            "claude" | "claude_gpt55" => Some(ProtocolFamily::Claude),
            "codex" => Some(ProtocolFamily::Codex),
            "dsh" => Some(ProtocolFamily::Dsh),
            "omp" => Some(ProtocolFamily::Omp),
            _ => None,
        }
    }

    fn of(family: crate::app_settings::AgentFamily) -> ProtocolFamily {
        use crate::app_settings::AgentFamily as Host;
        match family {
            Host::Claude => ProtocolFamily::Claude,
            Host::Codex => ProtocolFamily::Codex,
            Host::Dsh => ProtocolFamily::Dsh,
            Host::Omp => ProtocolFamily::Omp,
        }
    }

    /// A custom profile's session family: an explicit `family`, else the legacy
    /// `codex_like` boolean. Same precedence as `config_bundles.rs:100-108`.
    fn custom_session_family(profile_family: &str, codex_like: bool) -> ProtocolFamily {
        Self::of(
            crate::app_settings::AgentFamily::parse(profile_family.trim())
                .unwrap_or_else(|| crate::app_settings::AgentFamily::from_codex_like(codex_like)),
        )
    }

    /// An agent neither side knows. `src/agents.ts:170-178` treats anything not literally
    /// `claude` as codex, so reproducing that is what "no behaviour change" means here.
    fn unknown_session_family(agent: &str) -> ProtocolFamily {
        if agent == "claude" {
            ProtocolFamily::Claude
        } else {
            ProtocolFamily::Codex
        }
    }
}

impl AgentFamilies for SettingsFamilies {
    fn launch_family(&self, agent: &str) -> ProtocolFamily {
        let spec = crate::app_settings::get_agent_launch_spec(agent);
        match Self::of(spec.family) {
            ProtocolFamily::Dsh => ProtocolFamily::Dsh,
            ProtocolFamily::Omp => ProtocolFamily::Omp,
            // `pty.rs:1625,1808` chooses `build_codex_cmd` vs `build_claude_cmd` on this
            // flag, so for the two CLI families the flag *is* the launch family.
            _ if spec.codex_like => ProtocolFamily::Codex,
            _ => ProtocolFamily::Claude,
        }
    }

    fn session_family(&self, agent: &str) -> ProtocolFamily {
        if let Some(family) = Self::built_in_session_family(agent) {
            return family;
        }
        match crate::app_settings::load_settings_internal()
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
        {
            Some(profile) => Self::custom_session_family(&profile.family, profile.codex_like),
            None => Self::unknown_session_family(agent),
        }
    }

    fn is_builtin(&self, agent: &str) -> bool {
        Self::built_in_session_family(agent).is_some()
    }

    /// ADR-029. `claude_gpt55` is Claude in *format* but codex in *capability*: the client
    /// compares `agentOption("claude_gpt55").family` (= codex) against the record's family, so
    /// a `claude` task switches to it by handoff rather than by `--resume`. Reproducing that
    /// conclusion is the point; "which folder holds the jsonl" is `session_family`'s question.
    fn native_resume_family(&self, agent: &str) -> ProtocolFamily {
        match agent {
            "claude_gpt55" => ProtocolFamily::Codex,
            "claude" => ProtocolFamily::Claude,
            "codex" => ProtocolFamily::Codex,
            "dsh" => ProtocolFamily::Dsh,
            "omp" => ProtocolFamily::Omp,
            // A custom profile carries one family, so capability and format agree for it —
            // that is what the client does today, and nothing here invents a third answer.
            _ => self.session_family(agent),
        }
    }
}

#[cfg(test)]
mod family_tests {
    use super::SettingsFamilies;
    use aeroric_core::tasks::{AgentFamilies, ProtocolFamily};

    /// The half of ADR-028 that lives in the host: one built-in, two answers. `launch` is
    /// deliberately not exercised here — resolving it loads the user's real settings and may
    /// read their start script from disk, which a unit test has no business doing. That
    /// path is covered by delegating to the very function `pty.rs` selects on.
    #[test]
    fn claude_gpt55_keeps_claude_sessions_even_though_it_is_not_launched_like_claude() {
        for (agent, expected) in [
            ("claude", ProtocolFamily::Claude),
            ("claude_gpt55", ProtocolFamily::Claude),
            ("codex", ProtocolFamily::Codex),
            ("dsh", ProtocolFamily::Dsh),
            ("omp", ProtocolFamily::Omp),
        ] {
            assert_eq!(
                SettingsFamilies.session_family(agent),
                expected,
                "built-in {agent}"
            );
            assert!(
                SettingsFamilies.is_builtin(agent),
                "{agent} must be built-in"
            );
        }
        assert!(!SettingsFamilies.is_builtin("my-codex"));
    }

    #[test]
    fn a_profile_without_an_explicit_family_falls_back_to_codex_like() {
        // The compatibility window ADR-028 keeps: `family` was added later, and an empty
        // one must not silently mean "claude" for a profile that says it is codex-like.
        assert_eq!(
            SettingsFamilies::custom_session_family("", true),
            ProtocolFamily::Codex
        );
        assert_eq!(
            SettingsFamilies::custom_session_family("", false),
            ProtocolFamily::Claude
        );
        // An explicit family wins over the boolean, including for the two odd cases.
        assert_eq!(
            SettingsFamilies::custom_session_family("dsh", true),
            ProtocolFamily::Dsh
        );
        assert_eq!(
            SettingsFamilies::custom_session_family("  omp  ", false),
            ProtocolFamily::Omp,
            "surrounding spaces are trimmed, matching how the client normalises it"
        );
        // An unrecognised spelling is not a licence to guess: it degrades through
        // codex_like, exactly like the legacy parse-or-derive chain.
        assert_eq!(
            SettingsFamilies::custom_session_family("martian", true),
            ProtocolFamily::Codex
        );
    }

    #[test]
    fn an_unknown_agent_is_codex_unless_it_is_literally_claude() {
        assert_eq!(
            SettingsFamilies::unknown_session_family("brand-new"),
            ProtocolFamily::Codex
        );
        assert_eq!(
            SettingsFamilies::unknown_session_family("claude"),
            ProtocolFamily::Claude
        );
    }
}

/// The adapter's announcement channel (ADR-023): core decides, this emits.
///
/// Deliberately thin. It carries **no** suppression gate: `is_task_active` was applied at some
/// of the 28 legacy sites and not others, and ADR-034 resolved that by moving the rule into the
/// observation channel (one core rule over "does this task still have a live run"), not into the
/// emitter. This type only knows how to spell a payload; `EventOutcome::notification_for` is the
/// only thing that decides whether to spell it.
pub(crate) struct TauriEventSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriEventSink<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: tauri::Runtime> TaskEventSink for TauriEventSink<R> {
    fn notify(&self, notification: TaskNotification) {
        use tauri::Emitter;
        match &notification {
            TaskNotification::Status {
                task_id,
                status,
                failure_reason,
            } => {
                let _ = self.app.emit(
                    "task-status",
                    task_status_payload(task_id, *status, failure_reason.as_deref()),
                );
            }
            TaskNotification::Session(facts) => {
                let _ = self.app.emit("task-session", task_session_payload(facts));
            }
        }
    }
}

/// The runtime -> core observation channel (ADR-033 §3): the single door an outside fact about
/// a task walks through on its way to the reducer, and the reason `TauriEventSink` can be the
/// only emitter (ADR-023).
///
/// Four things get decided here rather than at a call site, each with an ADR behind it:
///
/// * where the row lives - `TaskService::project_of`, because report sites carry a task id only;
/// * whether the observation is admitted at all - core's guards (ADR-034);
/// * what gets announced - `EventOutcome::notification_for` (ADR-035: admitted means emitted,
///   even when nothing changed; only a drop is silent);
/// * whether anything is written - never from here, see the note inside.
///
/// A task this process cannot route still announces itself through
/// `TaskEvent::notification_when_unrecorded`, because ADR-033 §4 forbids turning a handover into
/// silence; that path says so in the log instead of pretending the reducer saw it.
pub(crate) struct CoreRuntimeObserver<R: tauri::Runtime> {
    service: Arc<TaskService>,
    sink: Arc<TauriEventSink<R>>,
}

impl<R: tauri::Runtime> CoreRuntimeObserver<R> {
    pub(crate) fn new(service: Arc<TaskService>, sink: Arc<TauriEventSink<R>>) -> Self {
        Self { service, sink }
    }
}

impl<R: tauri::Runtime> TaskRuntimeObserver for CoreRuntimeObserver<R> {
    fn observe(&self, event: TaskEvent) {
        let task_id = event.task_id().to_string();
        let project = match self.service.project_of(&task_id) {
            Ok(project) => project,
            Err(error) => {
                // A corrupt or unreadable table is not an answer about *this* task, so it takes
                // the same no-row path as a missing row rather than silently swallowing a fact.
                eprintln!("[aeroric] core observer could not resolve {task_id}: {error}");
                None
            }
        };
        let notification = match project {
            Some(project_id) => {
                // `decide_observation`, not `observe`, and deliberately: on macOS the gateway
                // already *is* the writer while `storage.rs` is still the code that saves the
                // task table. Committing from here would put two writers on one `tasks.json`
                // with no reconciliation protocol in place (INV-03). Attaching the commit is
                // part of the store cutover (Action 3), and the gate case
                // `core-observer-does-not-write-yet` turns red if this starts calling `observe`
                // before that switch has been reviewed.
                match self.service.decide_observation(&project_id, event.clone()) {
                    Ok((outcome, _)) => outcome.notification_for(&task_id),
                    Err(error) => {
                        eprintln!(
                            "[aeroric] core observer refused an event for {task_id}: {error}"
                        );
                        None
                    }
                }
            }
            None => event.notification_when_unrecorded(),
        };
        if let Some(notification) = notification {
            self.sink.notify(notification);
        }
    }
}

/// The door every legacy report site walks through on its way to core (ADR-023, ADR-033).
///
/// Two decisions are made here rather than at the 22 call sites, and both have a reason:
///
/// * **Which generation reports.** The transcript watchers are task-scoped, not run-scoped —
///   they outlive the process whose transcript they read. Asking "which run is this line from?"
///   is unanswerable there, so the observation carries *the newest registered generation*
///   (ADR-025's addressing rule), which is what the site's own status word always described:
///   what this task is doing right now. Falling back to `UNMINTED` keeps a run core never
///   numbered observable instead of silently out of range.
/// * **What happens when core is not running.** `install_core_services` can fail (a corrupt
///   ledger) and the app still starts, by design. Then the announcement falls back to the same
///   `TauriEventSink`, so no report site ever goes quiet — ADR-033 §4 forbids the silence, and
///   keeping the emit inside the sink means "one emitter" stays literally true even in the
///   degraded mode.
pub(crate) fn report_task_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    status: &str,
) {
    report_task_status_with_reason(app, task_id, status, None);
}

/// The same report with the reason the host already had. Sites like `finalize_task_exit` word
/// the reason themselves ("Process exited with code 1", "WSL process exited with code 7"), and
/// that text is user-visible today — so it travels in the event and core passes it through
/// rather than synthesizing its own wording.
pub(crate) fn report_task_status_with_reason<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    status: &str,
    reason: Option<&str>,
) {
    use tauri::Manager;
    let Ok(parsed) = serde_json::from_value::<TaskStatus>(serde_json::json!(status)) else {
        // A status word outside the frozen nine is a bug in the caller, not a new spelling.
        eprintln!("[aeroric] report_task_status got unknown status {status:?} for {task_id}");
        return;
    };
    let generation = app
        .state::<crate::TaskManager>()
        .runs
        .resource_key(task_id)
        .map(|run| run.generation())
        .unwrap_or(crate::run_registry::RunKey::UNMINTED);
    let event = TaskEvent::Status {
        task_id: task_id.to_string(),
        generation,
        status: parsed,
        attention_requested_at_ms: None,
        failure_reason: reason.map(str::to_string),
    };
    match app.try_state::<Arc<dyn TaskRuntimeObserver>>() {
        Some(observer) => observer.observe(event),
        None => TauriEventSink::new(app.clone()).notify(match reason {
            Some(reason) => TaskNotification::status_with_reason(task_id, parsed, reason),
            None => TaskNotification::status(task_id, parsed),
        }),
    }
}

/// ADR-036: the host's answer to "is any run of this task still alive".
///
/// The answer is `is_task_active`'s *body*, not just the run registry: that gate also counts a
/// codex session entry, and a claude entry that is not a lazy-attach placeholder. Answering with
/// `TaskManager::has_live_run` alone would be narrower than the thing being replaced, and the
/// failure is the silent kind — the transcript watchers would start dropping statuses that ship
/// today. Nothing is copied into core: a mirror would fail by staying alive forever if a single
/// retire path were missed, and the failure would be silent (the gate stops suppressing).
///
/// It must not call back into core, and `has_live_run` only takes the registry's leaf lock, so
/// asking cannot extend a lock chain.
struct HostLiveness<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> LiveRuns for HostLiveness<R> {
    fn has_live_run(&self, task_id: &str) -> bool {
        crate::session::is_task_active(&self.app, task_id)
    }
}

/// Puts the adapter's shared state in front of the app, from `lib.rs`'s setup hook.
///
/// Three rules shape this function, and none of them are stylistic:
///
/// 1. **The gateway is built before anything is managed**, because the executor it hands the
///    service needs an `AppHandle` that only exists by then, and the service needs the *same*
///    `Arc<CoreContext>` the gateway resolved — a second context would be a second answer to
///    "may this process write" (ADR-021).
/// 2. **A failure here must not stop the app from starting.** If the ledger cannot be loaded,
///    the honest outcome is "no core service this run", recorded as a degradation — the same
///    posture `DbxState::new_blocking` takes. Refusing to boot over a corrupt
///    core 独占的账本文件坏了，就 strand 用户整个工作区，而那个文件之后是
///    release can repair.
/// 3. **Managing the service is not the writer switch.** `storage.rs` still saves the task
///    table; nothing routes a *mutation* through here yet. What becomes real is the lease, the
///    read-only refusal, and — since ADR-033/035 — the observation channel: the managed
///    `TaskRuntimeObserver` decides and announces without ever writing, which is exactly why the
///    announcement handover and the store cutover can land separately.
pub(crate) fn install_core_services<R: tauri::Runtime>(app: &tauri::AppHandle<R>, version: &str) {
    use aeroric_core::tasks::TaskExecutor;
    use tauri::Manager;

    let gateway = CoreGateway::at_startup(version);
    let context = gateway.context().clone();
    app.manage(gateway);

    let mailbox = Arc::new(crate::legacy_task_executor::OutputSinkMailbox::new());
    app.manage(Arc::clone(&mailbox));

    let executor: Arc<dyn TaskExecutor> = Arc::new(
        crate::legacy_task_executor::LegacyTaskExecutor::new(app.clone(), mailbox),
    );
    let live_runs: Arc<dyn LiveRuns> = Arc::new(HostLiveness { app: app.clone() });
    match TaskService::new(
        context,
        Arc::new(SettingsFamilies) as Arc<dyn AgentFamilies>,
        executor,
        live_runs,
    ) {
        Ok(service) => {
            // Arc, not the bare service: `CoreRuntimeObserver` has to share it, and a second
            // `TaskService` would be a second ledger over the same directory.
            let service = Arc::new(service);
            let observer: Arc<dyn TaskRuntimeObserver> = Arc::new(CoreRuntimeObserver::new(
                Arc::clone(&service),
                Arc::new(TauriEventSink::new(app.clone())),
            ));
            app.manage(service);
            app.manage(observer);
        }
        Err(error) => startup_diagnostics::record(
            "core-service",
            format!("原生 core 本次启动不可用：{error}"),
            String::new(),
        ),
    }
}

/// The frozen nine spellings, written out rather than round-tripped through serde: a payload
/// constructor should not need an `expect` to obtain a string that is total by construction.
/// `the_event_words_match_their_serialized_forms` pins this list against the contract's own
/// serialisation, so a new variant cannot drift silently.
#[allow(
    dead_code,
    reason = "载荷构造器随 TauriEventSink 一起接进产品路径；今天由三条形状用例与 TS 侧载荷契约测试守住"
)]
const fn status_word(status: aeroric_core::tasks::TaskStatus) -> &'static str {
    use aeroric_core::tasks::TaskStatus;
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::InputRequired => "input_required",
        TaskStatus::Detached => "detached",
        TaskStatus::Interrupted => "interrupted",
        TaskStatus::Done => "done",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

#[allow(dead_code, reason = "同上")]
const fn family_word(family: aeroric_core::tasks::ProtocolFamily) -> &'static str {
    use aeroric_core::tasks::ProtocolFamily;
    match family {
        ProtocolFamily::Claude => "claude",
        ProtocolFamily::Codex => "codex",
        ProtocolFamily::Dsh => "dsh",
        ProtocolFamily::Omp => "omp",
    }
}

/// Builds the `task-status` payload.
///
/// The key set and spellings are the shipping client's, not a redesign:
/// `src/state/app/useAppTauriEvents.ts` reads `task_id`, `status` and an *optional*
/// `failure_reason`, and the status strings are the frozen nine (`input_required`, not
/// `inputRequired`). `src/test/native-core-adapter.test.ts` re-derives all of it from both
/// sources at gate time, because ADR-023 promises this payload does not change.
///
/// `failure_reason` is added only when present, exactly as the shipping `failed` emits do: a
/// `running` event carries two keys today, so sending `"failure_reason": null` would be a new
/// key on a wire the frontend already parses — and omitting the key on the failed path would
/// take away the text the UI explains the failure with.
#[allow(
    dead_code,
    reason = "随 TauriEventSink 接线；键集与拼写由 core_adapter_tests 与 src/test/native-core-adapter.test.ts 双侧钉住"
)]
pub(crate) fn task_status_payload(
    task_id: &str,
    status: aeroric_core::tasks::TaskStatus,
    failure_reason: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "task_id": task_id,
        "status": status_word(status),
    });
    if let Some(reason) = failure_reason {
        payload["failure_reason"] = serde_json::json!(reason);
    }
    payload
}

/// Builds a `task-session` payload, adding the per-family extras only where the shipping
/// events add them. Emitting `"family": null` for a claude transcript would be a new key on
/// a wire the frontend already parses, which is the opposite of what ADR-023 permits.
#[allow(dead_code, reason = "同上")]
pub(crate) fn task_session_payload(facts: &TaskSessionFacts) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "task_id": facts.task_id,
        "session_id": facts.session_id,
        "session_path": facts.session_path,
        "codex_like": facts.codex_like,
    });
    if let Some(family) = facts.family {
        payload["family"] = serde_json::json!(family_word(family));
    }
    if let Some(preset) = &facts.agent_preset {
        payload["agent_preset"] = serde_json::json!(preset);
    }
    payload
}

#[cfg(test)]
mod sink_tests {
    use super::{family_word, status_word, task_session_payload, task_status_payload};
    use aeroric_core::tasks::{ProtocolFamily, TaskSessionFacts, TaskStatus};

    #[test]
    fn the_event_words_match_their_serialized_forms() {
        // `status_word`/`family_word` are hand-written totals, so this is the guard that they
        // stay the words the contract publishes. Without it a tenth status could be added on
        // one side only and every task event would carry a stale spelling.
        for status in TaskStatus::ALL {
            let serialized = serde_json::to_value(status).expect("a status is a string");
            assert_eq!(
                serialized.as_str(),
                Some(status_word(*status)),
                "{status:?}"
            );
        }
        for family in ProtocolFamily::ALL {
            let serialized = serde_json::to_value(family).expect("a family is a string");
            assert_eq!(
                serialized.as_str(),
                Some(family_word(*family)),
                "{family:?}"
            );
        }
    }
    use serde_json::json;

    #[test]
    fn the_status_payload_is_the_shape_the_frontend_already_reads() {
        assert_eq!(
            task_status_payload("t1", TaskStatus::InputRequired, None),
            json!({ "task_id": "t1", "status": "input_required" }),
            "snake_case, two keys, no extras"
        );
        assert_eq!(
            task_status_payload("t1", TaskStatus::Running, None),
            json!({ "task_id": "t1", "status": "running" })
        );
    }

    /// The reason is the third key the frontend destructures, and only the failed path has it.
    /// Both halves matter: without the `Some` arm the UI loses its failure text, and without the
    /// `None` arm every event grows a key the shipping client never sees today.
    #[test]
    fn a_failure_carries_its_reason_and_a_non_failure_does_not() {
        assert_eq!(
            task_status_payload(
                "t1",
                TaskStatus::Failed,
                Some("Remote process exited with code 1"),
            ),
            json!({
                "task_id": "t1",
                "status": "failed",
                "failure_reason": "Remote process exited with code 1",
            })
        );
        assert_eq!(
            task_status_payload("t1", TaskStatus::Failed, None),
            json!({ "task_id": "t1", "status": "failed" }),
            "a failure core has no text for sends no key, not an empty one"
        );
        assert!(
            !task_status_payload("t1", TaskStatus::Done, None)
                .as_object()
                .expect("an object")
                .contains_key("failure_reason"),
            "the key must be absent rather than null"
        );
    }

    #[test]
    fn claude_and_codex_payloads_carry_exactly_four_keys() {
        let facts = TaskSessionFacts {
            task_id: "t1".into(),
            session_id: "s1".into(),
            session_path: "/p.jsonl".into(),
            codex_like: true,
            family: None,
            agent_preset: None,
        };
        assert_eq!(
            task_session_payload(&facts),
            json!({
                "task_id": "t1", "session_id": "s1", "session_path": "/p.jsonl", "codex_like": true
            }),
            "adding a null `family` here would be a wire change, not a refactor"
        );
    }

    #[test]
    fn omp_adds_only_the_family_and_dsh_adds_the_preset_too() {
        let base = TaskSessionFacts {
            task_id: "t2".into(),
            session_id: "s2".into(),
            session_path: "/o.jsonl".into(),
            codex_like: false,
            family: Some(ProtocolFamily::Omp),
            agent_preset: None,
        };
        assert_eq!(
            task_session_payload(&base),
            json!({
                "task_id": "t2", "session_id": "s2", "session_path": "/o.jsonl",
                "codex_like": false, "family": "omp"
            })
        );
        let dsh = TaskSessionFacts {
            family: Some(ProtocolFamily::Dsh),
            agent_preset: Some("standard".into()),
            ..base
        };
        assert_eq!(
            task_session_payload(&dsh),
            json!({
                "task_id": "t2", "session_id": "s2", "session_path": "/o.jsonl",
                "codex_like": false, "family": "dsh", "agent_preset": "standard"
            })
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> AppPaths {
        let root = std::env::temp_dir().join(format!(
            "aeroric-gateway-{tag}-{}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        AppPaths::with_data_dir(&root, Some(&root.join("data")))
    }

    #[test]
    fn the_mode_matrix_is_decidable_without_a_filesystem() {
        // ADR-021 × ADR-027 in one table. The first two rows are the load-bearing pair:
        // a deferred platform must not report a fault, and a lease conflict must not be
        // mistaken for one.
        assert_eq!(
            decide_mode(false, Acquisition::Acquired),
            CoreMode::HandoverDeferred,
            "even a free lease must not make Windows a writer yet"
        );
        assert_eq!(
            decide_mode(false, Acquisition::HeldByAnother),
            CoreMode::HandoverDeferred,
            "and it must not look broken either"
        );
        assert_eq!(decide_mode(true, Acquisition::Acquired), CoreMode::Writer);
        assert_eq!(
            decide_mode(true, Acquisition::HeldByAnother),
            CoreMode::ReadOnlyBusy
        );
        assert_eq!(
            decide_mode(true, Acquisition::Refused),
            CoreMode::ReadOnlyLocation
        );
    }

    #[test]
    fn only_the_two_read_only_faults_are_reported_to_the_user() {
        assert_eq!(CoreMode::Writer.diagnostics_component(), None);
        assert_eq!(CoreMode::HandoverDeferred.diagnostics_component(), None);
        assert_eq!(
            CoreMode::ReadOnlyBusy.diagnostics_component(),
            Some("storage-lease")
        );
        assert_eq!(
            CoreMode::ReadOnlyLocation.diagnostics_component(),
            Some("storage-location")
        );
    }

    #[test]
    fn a_free_temp_directory_makes_this_process_the_writer() {
        // The point of prerequisite ①: the probe must be good enough that a normal local
        // directory actually gets a writer. If this ever goes red, the core has quietly
        // lost the ability to write anywhere.
        let (_root, gateway) = writer_on_temp("acquire");
        assert_eq!(
            gateway.mode(),
            CoreMode::Writer,
            "notice: {:?}",
            gateway.notice()
        );
        assert!(gateway.require_writer().is_ok());
    }

    #[test]
    fn a_second_instance_over_the_same_directory_starts_read_only_and_says_so() {
        // ADR-027 end to end, on a real lease. The first holder must stay writable: if
        // the *second* attempt damaged the first one, this is where it shows.
        let paths = temp_paths("double-open");
        let first = CoreGateway::startup(paths.clone(), "test");
        assert_eq!(first.mode(), CoreMode::Writer);

        let second = CoreGateway::startup(paths.clone(), "test");
        assert_eq!(second.mode(), CoreMode::ReadOnlyBusy);
        let error = second.require_writer().unwrap_err();
        assert!(
            error.contains("read-only"),
            "the refusal must name the mode, got: {error}"
        );
        assert!(
            error.contains(&paths.data_dir().display().to_string()),
            "and the directory it will not write: {error}"
        );
        assert!(second.notice().is_some(), "the reason must be reportable");

        let listed = startup_diagnostics::list_startup_degradations();
        assert!(
            listed.iter().any(|d| d.component == "storage-lease"),
            "the lease conflict must reach the start-up list the frontend reads"
        );

        assert!(
            first.require_writer().is_ok(),
            "losing a race for the lease must not degrade the holder"
        );

        // The lease is released by dropping the holder, not by any API here.
        drop(second);
        drop(first);
        let reopened = CoreGateway::startup(paths, "test");
        assert_eq!(
            reopened.mode(),
            CoreMode::Writer,
            "and a fresh start after both are gone must write again"
        );
    }

    #[test]
    fn read_only_mode_cannot_write_even_if_someone_asked_it_to() {
        // `CoreContext::read_only` has no lease, so core itself is the backstop; the
        // gateway must not claim writer-ness on top of it.
        let paths = temp_paths("read-only");
        let data_dir = paths.data_dir().to_path_buf();
        let gateway = CoreGateway {
            context: Arc::new(CoreContext::read_only(paths)),
            mode: CoreMode::ReadOnlyBusy,
            data_dir,
            location_warning: None,
        };
        assert!(!gateway.is_writer());
        assert!(gateway.require_writer().is_err());
        assert!(
            gateway.context().require_write().is_err(),
            "core must refuse independently of the gateway's own flag"
        );
    }

    /// Each test gets its own throwaway directory — nothing here may touch `~/.aeroric`,
    /// which the shipping app is writing on this very machine.
    fn writer_on_temp(tag: &str) -> (AppPaths, CoreGateway) {
        let paths = temp_paths(tag);
        let gateway = CoreGateway::startup(paths.clone(), "test");
        (paths, gateway)
    }

    #[test]
    fn handover_policy_is_one_function_and_is_not_a_stub() {
        // Guards the ADR-021 gate check: the platform judgement must live in exactly one
        // place. If it ever grows branches elsewhere, this still passes — so the real
        // protection is the `tauri-adapter` gate case, which greps for that.
        assert_eq!(writer_handover_allowed(), cfg!(target_os = "macos"));
    }
}
