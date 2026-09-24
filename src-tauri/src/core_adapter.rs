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

//! A module-level `dead_code` allowance is deliberate: nothing in the product path
//! constructs `CoreGateway` yet, because doing so would take the writer lease on macOS while
//! `storage.rs` is still the code that actually saves — which is the double-writer INV-03
//! forbids. The allowance is removed by the writer switch in this same task; `cargo test`
//! hides these warnings (tests use the items), so only `cargo check` shows them.
#![allow(
    dead_code,
    reason = "P2-05 in progress: consumed once the writer role switches over"
)]

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::{
    tasks::{AgentFamilies, ProtocolFamily, TaskEventSink, TaskNotification, TaskSessionFacts},
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

    pub(crate) fn context(&self) -> &Arc<CoreContext> {
        &self.context
    }

    pub(crate) fn mode(&self) -> CoreMode {
        self.mode
    }

    pub(crate) fn is_writer(&self) -> bool {
        self.mode.is_writer()
    }

    /// What the host owes the user at start-up: the reason we are not writing, and/or the
    /// cloud-sync caveat. `None` means there is genuinely nothing to report.
    pub(crate) fn notice(&self) -> Option<&str> {
        self.location_warning.as_deref()
    }

    /// The single gate every mutation in the adapter passes through.
    ///
    /// It exists so "read-only" is one decision rather than N: a command that forgets to
    /// check still cannot write, because `CoreContext::require_write` inside core refuses
    /// without a lease. This function only turns that into a message the UI can act on.
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
/// Deliberately thin, and deliberately without the legacy `is_task_active` suppression. That
/// gate is applied at *some* of the eight call sites and not others, so reproducing it needs
/// a per-site answer, not a policy here — and inventing one now would either drop events that
/// ship today or deliver ones today's code suppresses. Removing those eight sites is the next
/// step, and the gate is listed as an open item in `docs/native/reviews/tauri-adapter.md`.
pub(crate) struct TauriEventSink {
    app: tauri::AppHandle,
}

impl TauriEventSink {
    pub(crate) fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl TaskEventSink for TauriEventSink {
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

/// The frozen nine spellings, written out rather than round-tripped through serde: a payload
/// constructor should not need an `expect` to obtain a string that is total by construction.
/// `the_event_words_match_their_serialized_forms` pins this list against the contract's own
/// serialisation, so a new variant cannot drift silently.
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
