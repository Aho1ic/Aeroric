//! Which request a task's record turns into — the decisions from `taskLaunch.ts` and
//! `taskCompletion.ts`, without the `invoke` calls.
//!
//! Three things the client decided at this layer and nothing above it, so they are ported
//! here rather than left behind:
//!
//! - **dsh is a different channel**, not a variant of the PTY path: it runs over its own
//!   Web RPC, takes an `agentPreset`, sends the client time zone, and its *resume* sends an
//!   empty prompt on purpose. Replaying the original message into a session that already
//!   contains it would run the task twice.
//! - **`worktreePath` wins over the project path** for every local launch, and a task that
//!   has been resumed in a worktree keeps using it.
//! - **completing is three-way**, and the third branch never touches a process at all: a
//!   task that is not live has nothing to stop, so "mark done" is a pure state change.

use aeroric_protocol::task::{PermissionMode, ProtocolFamily, TaskStatus};
use aeroric_protocol::ProjectTarget;
use serde_json::Value;

use crate::projects::mutations::{is_addressable, resolve_location};
use crate::storage::compat::Record;
use crate::tasks::executor::{
    CancelTarget, DshWindowFacts, LaunchRequest, ResetOutcome, ResumeRequest, WorkingDirectory,
};
use crate::tasks::session_owner::{
    family_prefix, launch_family_of, resolve_session_owner, AgentFamilies,
};

/// How a task is meant to start. `Worktree` and `WebUi` are launch-time choices the client
/// carries; the persisted record only remembers the *result* (`worktreePath`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContinuationMode {
    /// A fresh process with the prompt.
    Start,
    /// A process reattaching to a session id.
    Resume,
}

fn number(record: &Record, key: &str) -> Option<u64> {
    record.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
    })
}

fn string(record: &Record, key: &str) -> Option<String> {
    record
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
}

fn permission_mode(record: &Record) -> PermissionMode {
    record
        .get("permissionMode")
        .and_then(|value| serde_json::from_value::<PermissionMode>(value.clone()).ok())
        .unwrap_or(PermissionMode::Ask)
}

/// The working directory a task runs in, resolved the way the client resolves it: a
/// worktree is a *local* path that overrides the project's, and a remote project has no
/// host-side path so the location decides the shape.
pub fn working_directory(record: &Record, project: &Record) -> Option<WorkingDirectory> {
    let worktree = string(record, "worktreePath");
    if !is_addressable(project) {
        // P2-03: an unaddressable location is listed and deleted but never run.
        return None;
    }
    match resolve_location(project) {
        ProjectTarget::Ssh {
            connection_id,
            remote_path,
        } => {
            let _ = worktree; // a remote project never has one; the client refuses that combination
            Some(WorkingDirectory::Ssh {
                connection_id,
                remote_path,
            })
        }
        ProjectTarget::Wsl {
            distribution,
            linux_path,
        } => Some(WorkingDirectory::Wsl {
            distribution,
            path: linux_path,
        }),
        ProjectTarget::Local { path } => Some(WorkingDirectory::Local {
            path: worktree.unwrap_or(path),
        }),
    }
}

/// The path a cancel/complete call carries. `task.worktreePath ?? project.path ?? ""` —
/// including the empty string, because the client's delete path calls it with a project it
/// could not resolve and the backend tolerates that.
pub fn process_path(record: &Record, project: &Record) -> String {
    match working_directory(record, project) {
        Some(WorkingDirectory::Local { path })
        | Some(WorkingDirectory::Ssh {
            remote_path: path, ..
        }) => path,
        Some(WorkingDirectory::Wsl { path, .. }) => path,
        None => string(record, "worktreePath").unwrap_or_default(),
    }
}

/// Which cancel channel a live task is on.
pub fn cancel_target(
    record: &Record,
    project: &Record,
    families: &dyn AgentFamilies,
) -> CancelTarget {
    // Which command stops a process is a question about how it was started, so this reads
    // the launch family, not the session one (ADR-028).
    if launch_family_of(record, families) == ProtocolFamily::Dsh {
        return CancelTarget::Dsh;
    }
    if !is_addressable(project) {
        return CancelTarget::Local {
            project_path: String::new(),
        };
    }
    match resolve_location(project) {
        ProjectTarget::Ssh { .. } => CancelTarget::Ssh,
        ProjectTarget::Wsl { .. } => CancelTarget::Wsl,
        ProjectTarget::Local { .. } => CancelTarget::Local {
            project_path: process_path(record, project),
        },
    }
}

/// `launchLocalTask`/`launchSshTask`/`launchWslTask`/`resumeDshTask`/… as one constructor.
pub fn launch_request(
    record: &Record,
    project: &Record,
    families: &dyn AgentFamilies,
    operation_key: &str,
    generation: u64,
    images: Vec<String>,
    texts: Vec<String>,
    inject_prompt_into_terminal: bool,
    prompt_override: Option<String>,
    dsh_window: Option<DshWindowFacts>,
) -> Option<LaunchRequest> {
    let working_directory = working_directory(record, project)?;
    let owner = resolve_session_owner(record, families);
    // Two different families in one request: which session fields to read is a session
    // question, which argument shape the CLI gets is a launch question. Collapsing them is
    // the bug ADR-028 exists to prevent.
    let session_prefix = family_prefix(owner.family);
    Some(LaunchRequest {
        task_id: string(record, "id")?,
        project_id: string(record, "projectId")?,
        operation_key: operation_key.to_string(),
        generation,
        agent: string(record, "agent").unwrap_or_default(),
        family: launch_family_of(record, families),
        permission_mode: permission_mode(record),
        permission_mode_wire: string(record, "permissionMode").unwrap_or_default(),
        prompt: string(record, "prompt").unwrap_or_default(),
        prompt_override,
        images,
        texts,
        inject_prompt_into_terminal,
        // A fresh start still hands the dsh channel the session it already has, so
        // reconnecting an existing session is not a second conversation.
        session_id: string(record, &format!("{session_prefix}SessionId")),
        selected_model: string(record, "selectedModel"),
        reasoning_effort: string(record, "reasoningEffort"),
        speed: string(record, "speed"),
        dsh_agent_preset: string(record, "dshAgentPreset"),
        created_at_ms: number(record, "createdAt").unwrap_or_default(),
        dsh_window,
        working_directory,
        // Gathered from the view at the call site; a core-side launch has none until the
        // host says so, and the executor must not invent a size.
        terminal_size: None,
    })
}

/// The resume counterpart. `prompt` is `None` for dsh and for a remote resume, which never
/// carried one, and the original message for a local PTY resume.
pub fn resume_request(
    record: &Record,
    project: &Record,
    families: &dyn AgentFamilies,
    operation_key: &str,
    generation: u64,
    session_id: &str,
) -> Option<ResumeRequest> {
    let working_directory = working_directory(record, project)?;
    let launch = launch_family_of(record, families);
    // Whether the CLI takes the original prompt as an argument is about the command line,
    // not about the transcript.
    let takes_prompt = launch != ProtocolFamily::Dsh
        && matches!(working_directory, WorkingDirectory::Local { .. });
    Some(ResumeRequest {
        task_id: string(record, "id")?,
        project_id: string(record, "projectId")?,
        operation_key: operation_key.to_string(),
        generation,
        family: launch,
        agent: string(record, "agent").unwrap_or_default(),
        session_id: session_id.to_string(),
        permission_mode: permission_mode(record),
        permission_mode_wire: string(record, "permissionMode").unwrap_or_default(),
        selected_model: string(record, "selectedModel"),
        reasoning_effort: string(record, "reasoningEffort"),
        speed: string(record, "speed"),
        dsh_agent_preset: string(record, "dshAgentPreset"),
        prompt: takes_prompt.then(|| string(record, "prompt").unwrap_or_default()),
        working_directory,
        terminal_size: None,
    })
}

/// The two things a completion can be, or neither. `taskCompletionCommand` verbatim: a dsh
/// task always goes through its own command, a live task of any other family gets stopped,
/// and anything else is already finished as far as a process is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Dsh,
    StopProcess,
    /// No process to stop: the caller only moves the state.
    Nothing,
}

pub fn completion_for(record: &Record, families: &dyn AgentFamilies) -> Completion {
    // "Which command finishes this task" is asked of the process, so it is a launch
    // question (ADR-028).
    if launch_family_of(record, families) == ProtocolFamily::Dsh {
        return Completion::Dsh;
    }
    match status_of(record) {
        Some(status) if is_live_for_completion(status) => Completion::StopProcess,
        _ => Completion::Nothing,
    }
}

/// `isLiveTerminalTaskStatus`: pending, running, input_required. Narrower than
/// [`TaskStatus::is_live`], which also counts `detached` — a detached task has no
/// reachable process to stop, so asking the backend to complete it would be a call that
/// cannot succeed.
pub fn is_live_for_completion(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Pending | TaskStatus::Running | TaskStatus::InputRequired
    )
}

fn status_of(record: &Record) -> Option<TaskStatus> {
    crate::tasks::reducer::status_of(record)
}

/// `mergeResetTaskSession` in `src/state/app/taskSwitch.ts:97-172`.
///
/// A reset reports whichever sessions the running process had written; every family's
/// fields are overlaid, and *only when exactly one* family produced a snapshot is the task
/// rebranded to it. Zero or two snapshots leave the attribution alone — guessing between two
/// is how a task ends up resuming the wrong transcript.
pub fn merge_reset_session(record: &Record, snapshot: &ResetOutcome) -> Record {
    let pairs = [
        (
            ProtocolFamily::Codex,
            &snapshot.codex_session_id,
            &snapshot.codex_session_path,
        ),
        (
            ProtocolFamily::Claude,
            &snapshot.claude_session_id,
            &snapshot.claude_session_path,
        ),
        (
            ProtocolFamily::Dsh,
            &snapshot.dsh_session_id,
            &snapshot.dsh_session_path,
        ),
        (
            ProtocolFamily::Omp,
            &snapshot.omp_session_id,
            &snapshot.omp_session_path,
        ),
    ];
    let mut next = record.clone();
    for (family, id, path) in &pairs {
        let prefix = family_prefix(*family);
        if let Some(value) = id {
            next.0
                .insert(format!("{prefix}SessionId"), Value::String(value.clone()));
        }
        if let Some(value) = path {
            next.0
                .insert(format!("{prefix}SessionPath"), Value::String(value.clone()));
        }
    }
    let present: Vec<ProtocolFamily> = pairs
        .iter()
        .filter(|(_, id, path)| {
            id.as_deref().is_some_and(|v| !v.is_empty())
                || path.as_deref().is_some_and(|v| !v.is_empty())
        })
        .map(|(family, _, _)| *family)
        .collect();
    if present.len() != 1 {
        return next;
    }
    let family = present[0];
    for other in [
        ProtocolFamily::Claude,
        ProtocolFamily::Codex,
        ProtocolFamily::Dsh,
        ProtocolFamily::Omp,
    ] {
        if other == family {
            continue;
        }
        let prefix = family_prefix(other);
        next.0.remove(&format!("{prefix}SessionId"));
        next.0.remove(&format!("{prefix}SessionPath"));
    }
    let agent = string(&next, "agent").unwrap_or_default();
    next.0
        .insert("sessionAgent".to_string(), Value::String(agent));
    next.0.insert(
        "sessionCodexLike".to_string(),
        Value::Bool(family == ProtocolFamily::Codex),
    );
    next.0.insert(
        "sessionFamily".to_string(),
        Value::String(
            serde_json::to_value(family)
                .expect("a family is a string")
                .as_str()
                .expect("a family is a string")
                .to_string(),
        ),
    );
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::session_owner::test_support::Table;
    use serde_json::json;

    fn record(value: Value) -> Record {
        Record(value.as_object().unwrap().clone())
    }

    fn local_project() -> Record {
        record(json!({"id": "p1", "path": "/repo"}))
    }

    fn ssh_project() -> Record {
        record(json!({
            "id": "p2", "path": "ssh:c1:/srv/app",
            "location": {"kind": "ssh", "connectionId": "c1", "remotePath": "/srv/app"}
        }))
    }

    /// ADR-028, both halves in one case. `claude_gpt55` is not a puzzle to be solved but a
    /// fact about the shipping client: it is *started* with codex arguments and its
    /// transcript is *read* as Claude. A test that only checked one of the two would pass
    /// while the other silently regressed — which is how the single field got here.
    #[test]
    fn one_agent_has_two_families_and_the_record_outlives_the_table() {
        let families = Table::new();
        let unpinned = record(json!({
            "id": "t", "projectId": "p1", "agent": "claude_gpt55", "status": "running",
            "claudeSessionId": "c-1"
        }));
        assert_eq!(
            launch_family_of(&unpinned, &families),
            ProtocolFamily::Codex,
            "the client runs build_codex_cmd for this agent (pty.rs selects on codex_like)"
        );
        assert_eq!(
            resolve_session_owner(&unpinned, &families).family,
            ProtocolFamily::Claude,
            "…and reads its history from ~/.claude/projects"
        );

        let request = launch_request(
            &unpinned,
            &local_project(),
            &families,
            "op-key",
            3,
            Vec::new(),
            Vec::new(),
            false,
            None,
            None,
        )
        .expect("a local project is launchable");
        assert_eq!(
            request.family,
            ProtocolFamily::Codex,
            "what the executor builds arguments from must be the launch family"
        );
        assert_eq!(
            request.session_id.as_deref(),
            Some("c-1"),
            "while the session coordinates still come from the claude-format fields"
        );

        let pinned = record(json!({
            "id": "t", "projectId": "p1", "agent": "claude_gpt55", "status": "running",
            "launchFamily": "claude", "claudeSessionId": "c-1"
        }));
        assert_eq!(
            launch_family_of(&pinned, &families),
            ProtocolFamily::Claude,
            "a task that recorded how it was started is not re-decided by a later profile edit"
        );

        let nonsense = record(json!({
            "id": "t", "projectId": "p1", "agent": "claude_gpt55", "status": "running",
            "launchFamily": "martian"
        }));
        assert_eq!(
            launch_family_of(&nonsense, &families),
            ProtocolFamily::Codex,
            "an unreadable value falls back to the same derivation, never to a guess"
        );
    }

    #[test]
    fn a_worktree_overrides_the_project_path_and_a_remote_project_has_neither() {
        let plain =
            record(json!({"id": "t", "projectId": "p1", "agent": "claude", "status": "running"}));
        assert_eq!(process_path(&plain, &local_project()), "/repo".to_string());
        let in_worktree = record(json!({
            "id": "t", "projectId": "p1", "agent": "claude", "status": "running",
            "worktreePath": "/repo/.wt/t"
        }));
        assert_eq!(
            process_path(&in_worktree, &local_project()),
            "/repo/.wt/t".to_string()
        );
        let remote = record(json!({"id": "t", "projectId": "p2", "agent": "claude"}));
        assert_eq!(
            working_directory(&remote, &ssh_project()),
            Some(WorkingDirectory::Ssh {
                connection_id: "c1".to_string(),
                remote_path: "/srv/app".to_string()
            }),
            "a worktree path from an older record must not be used as a remote cwd"
        );
    }

    #[test]
    fn the_cancel_channel_is_picked_by_family_first_then_by_location() {
        let families = Table::new();
        let dsh =
            record(json!({"id": "t", "projectId": "p1", "agent": "dsh", "status": "running"}));
        assert_eq!(
            cancel_target(&dsh, &local_project(), &families),
            CancelTarget::Dsh
        );
        let local =
            record(json!({"id": "t", "projectId": "p1", "agent": "claude", "status": "running"}));
        assert_eq!(
            cancel_target(&local, &local_project(), &families),
            CancelTarget::Local {
                project_path: "/repo".to_string()
            }
        );
        let remote = record(json!({"id": "t", "projectId": "p2", "agent": "claude"}));
        assert_eq!(
            cancel_target(&remote, &ssh_project(), &families),
            CancelTarget::Ssh
        );
        // dsh wins even on a remote project: the channel is where the process lives.
        let remote_dsh =
            record(json!({"id": "t", "projectId": "p2", "agent": "dsh", "sessionFamily": "dsh"}));
        assert_eq!(
            cancel_target(&remote_dsh, &ssh_project(), &families),
            CancelTarget::Dsh
        );
    }

    #[test]
    fn resuming_dsh_carries_no_prompt_while_a_local_pty_does() {
        let families = Table::new();
        let dsh = record(json!({
            "id": "t1", "projectId": "p1", "agent": "dsh", "prompt": "build it",
            "dshSessionId": "d-1", "sessionFamily": "dsh", "dshAgentPreset": "standard"
        }));
        let request =
            resume_request(&dsh, &local_project(), &families, "op-key", 2, "d-1").expect("planned");
        assert_eq!(
            request.prompt, None,
            "replaying the message would run the task twice"
        );
        assert_eq!(request.session_id, "d-1");
        assert_eq!(request.generation, 2);
        assert_eq!(request.dsh_agent_preset.as_deref(), Some("standard"));

        let claude = record(json!({
            "id": "t2", "projectId": "p1", "agent": "claude", "prompt": "build it",
            "claudeSessionId": "c-1", "sessionFamily": "claude", "permissionMode": "auto_edit"
        }));
        let request = resume_request(&claude, &local_project(), &families, "op-key", 1, "c-1")
            .expect("planned");
        assert_eq!(request.prompt.as_deref(), Some("build it"));
        assert_eq!(request.permission_mode, PermissionMode::AutoEdit);

        let remote = record(json!({
            "id": "t3", "projectId": "p2", "agent": "claude", "prompt": "x", "sessionFamily": "claude"
        }));
        let request =
            resume_request(&remote, &ssh_project(), &families, "op-key", 1, "s").expect("planned");
        assert_eq!(
            request.prompt, None,
            "the remote resume never sent a prompt, and this is not where that changes"
        );
    }

    #[test]
    fn a_launch_request_carries_the_family_it_will_actually_run() {
        let families = Table::new();
        let record = record(json!({
            "id": "t", "projectId": "p1", "agent": "codex", "prompt": "go",
            "claudeSessionId": "leftover", "sessionFamily": "codex", "codexSessionId": "x-1",
            "createdAt": 5i64, "selectedModel": "gpt-5", "reasoningEffort": "high"
        }));
        let request = launch_request(
            &record,
            &local_project(),
            &families,
            "op-key",
            7,
            vec!["img".to_string()],
            vec![],
            true,
            Some("handoff text".to_string()),
            None,
        )
        .expect("planned");
        assert_eq!(request.family, ProtocolFamily::Codex);
        assert_eq!(
            request.session_id.as_deref(),
            Some("x-1"),
            "not the stale claude id"
        );
        assert_eq!(request.generation, 7);
        assert_eq!(request.prompt_override.as_deref(), Some("handoff text"));
        assert_eq!(request.created_at_ms, 5);
        assert!(request.inject_prompt_into_terminal);
        assert_eq!(
            request.terminal_size, None,
            "the core does not guess a window size"
        );
    }

    #[test]
    fn completing_asks_the_backend_only_when_a_process_could_be_running() {
        let families = Table::new();
        let live = |status: &str| {
            record(json!({"id": "t", "projectId": "p1", "agent": "claude", "status": status}))
        };
        for status in ["pending", "running", "input_required"] {
            assert_eq!(
                completion_for(&live(status), &families),
                Completion::StopProcess,
                "{status}"
            );
        }
        for status in [
            "done",
            "failed",
            "cancelled",
            "todo",
            "interrupted",
            "detached",
        ] {
            assert_eq!(
                completion_for(&live(status), &families),
                Completion::Nothing,
                "{status} has nothing to stop"
            );
        }
        let dsh_detached = record(json!({
            "id": "t", "projectId": "p1", "agent": "dsh", "status": "detached", "sessionFamily": "dsh"
        }));
        assert_eq!(completion_for(&dsh_detached, &families), Completion::Dsh);
        assert!(!is_live_for_completion(TaskStatus::Detached));
        assert!(
            TaskStatus::Detached.is_live(),
            "and that is a deliberate difference from is_live"
        );
    }

    #[test]
    fn a_task_without_a_project_or_id_is_not_planned_rather_than_planned_wrong() {
        let families = Table::new();
        let orphan = record(json!({"id": "t", "agent": "claude", "prompt": "x"}));
        assert!(launch_request(
            &orphan,
            &local_project(),
            &families,
            "op-key",
            0,
            vec![],
            vec![],
            false,
            None,
            None,
        )
        .is_none());
        let no_id = record(json!({"projectId": "p1", "agent": "claude"}));
        assert!(resume_request(&no_id, &local_project(), &families, "op-key", 0, "s").is_none());
        // `task.worktreePath ?? project.path ?? ""`: the empty string only appears when the
        // project itself is gone, which is what the delete path does on purpose.
        assert_eq!(
            process_path(&no_id, &local_project()),
            "/repo".to_string(),
            "a project that resolves still wins"
        );
        assert_eq!(
            process_path(&no_id, &record(json!({"id": "p1"}))),
            String::new(),
            "and a project with no path yields the empty string, not a panic"
        );
    }

    /// §10 的 JS 台账量出来的一件事：dsh 的启动点今天**只发得出一个**每窗口事实（`clientTimeZone`）——
    /// `workspaceId` 一次都没发过，`promptMode` 只活在 composer 那条命令里。所以三个字段必须都是
    /// `Option`：把必填做进类型，宿主就只能替内核发明一个"缺省值"，而那是一条新行为不是迁移。
    /// 这条钉的是**形状可表达**，不是分发对不对（执行器仍整条拒绝 dsh）。
    #[test]
    fn a_dsh_launch_is_expressible_with_only_the_fact_the_webview_actually_sends() {
        let families = Table::new();
        let row = record(json!({
            "id": "t", "projectId": "p1", "agent": "dsh", "status": "running",
            "dshSessionId": "s-1", "dshAgentPreset": "standard"
        }));
        let request = launch_request(
            &row,
            &local_project(),
            &families,
            "op-dsh",
            1,
            Vec::new(),
            Vec::new(),
            false,
            None,
            Some(DshWindowFacts {
                client_time_zone: Some("Asia/Shanghai".to_string()),
                ..Default::default()
            }),
        )
        .expect("a dsh row on a local project is launchable");

        assert_eq!(
            request.family,
            ProtocolFamily::Dsh,
            "otherwise this case is about some other family"
        );
        let facts = request
            .dsh_window
            .expect("the webview's facts travel with the request");
        assert_eq!(facts.client_time_zone.as_deref(), Some("Asia/Shanghai"));
        assert!(
            facts.workspace_id.is_none() && facts.prompt_mode.is_none(),
            "absent stays absent — the kernel distinguishes it, so the host must not invent a default"
        );
        assert_eq!(
            request.session_id.as_deref(),
            Some("s-1"),
            "and the session the row already has still rides along"
        );
    }
}
