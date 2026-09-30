//! The host half of P2-05: core's `TaskExecutor` port, implemented against the launch bodies
//! the Tauri client already runs.
//!
//! Direction is the whole point of this file (plan Action 4):
//! `legacy command → TaskService → LegacyTaskExecutor → *_core launch body`. The executor must
//! never call a `#[tauri::command]`, because those commands are exactly what will call
//! `TaskService` once the writer role is handed over — that loop is why every body was split
//! into a wrapper plus a kernel first.
//!
//! The DSH branch dispatches `run_dsh_task_core` (wiring §12): the request carries the
//! three per-window facts (`DshWindowFacts`) and the session id comes from the record, so
//! the only remaining shape question is the working directory — DSH is local-only, and a
//! remote one is refused by name rather than guessed at.
//!
//! What is deliberately **not** here yet:
//!
//! - Run addressing. `cancel`/`complete` still speak the old task-id-only vocabulary; the
//!   `(task_id, generation)` rekey (ADR-020 + ADR-025) replaces these with "newest registered
//!   generation" for resource calls and "every live generation" for intents, and `RunRegistry`
//!   is what they will query.
//! - Post-spawn failures. Everything returned here is a *synchronous* refusal; once the kernel
//!   is on the async runtime its `Err` has no caller to return to. Under ADR-023 that outcome
//!   belongs on the observer as a `TaskEvent`, which is the wiring step, not this one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aeroric_core::tasks::{
    CancelTarget, Execution, ExecutorFailure, LaunchRequest, PermissionMode, ProtocolFamily,
    ResetOutcome, ResumeRequest, TaskExecutor, WorkingDirectory,
};
use tauri::{AppHandle, Manager};

use crate::dsh_webui::DshWebUiManager;
use crate::pty::{self, OutputSink};
use crate::run_registry::RunKey;
use crate::ssh::SshConnection;
use crate::TaskManager;

/// A terminal channel is a webview object: it cannot travel inside a `LaunchRequest`, and the
/// host cannot recreate one. So the command that starts an operation parks its channel here
/// under the caller's operation key (ADR-017 — that key is already the identity of "this
/// exact click, not a later one"), and the executor takes it out when core hands it the spawn.
///
/// Keying by operation rather than by task is the part that matters: ADR-017 allows two runs
/// of one task to be live at once, so a task-id mailbox would hand one window's bytes to
/// another window's process.
#[derive(Default)]
pub(crate) struct OutputSinkMailbox {
    sinks: Mutex<HashMap<String, OutputSink>>,
}

impl OutputSinkMailbox {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn park(&self, operation_key: &str, sink: OutputSink) {
        self.sinks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(operation_key.to_string(), sink);
    }

    /// Once only: a channel that could be taken twice would put one process's bytes into two
    /// terminals.
    pub(crate) fn take(&self, operation_key: &str) -> Option<OutputSink> {
        self.sinks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(operation_key)
    }

    /// How many channels are waiting. A caller that gave up before core was reached must
    /// `discard` its channel, and this count is how you notice that nobody did.
    pub(crate) fn parked(&self) -> usize {
        self.sinks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub(crate) fn discard(&self, operation_key: &str) -> bool {
        self.take(operation_key).is_some()
    }
}

/// Generic over the runtime on purpose (P2-05's acceptance evidence): the launch chain it
/// dispatches into (`run_task_core → spawn_pty_reader → send_pty_chunk`) has to be able to run
/// without a Wry webview, or "one start, one process" can only be asserted against a mock —
/// which is exactly what P2-04 could not close. Production instantiates it at `Wry`; the host
/// tests instantiate it at `MockRuntime`.
pub(crate) struct LegacyTaskExecutor<R: tauri::Runtime = tauri::Wry> {
    app: AppHandle<R>,
    sinks: Arc<OutputSinkMailbox>,
}

impl<R: tauri::Runtime> LegacyTaskExecutor<R> {
    pub(crate) fn new(app: AppHandle<R>, sinks: Arc<OutputSinkMailbox>) -> Self {
        Self { app, sinks }
    }

    fn channel_for(
        &self,
        operation_key: &str,
    ) -> Result<tauri::ipc::Channel<String>, ExecutorFailure> {
        let sink = self
            .sinks
            .take(operation_key)
            .ok_or_else(|| ExecutorFailure::Spawn {
                reason: missing_channel_reason(operation_key),
            })?;
        match sink {
            OutputSink::Channel(channel) => Ok(channel),
            // Parked by something that has no window to stream to. Only the channel arm can
            // back an agent launch; a shell-style event sink is a different destination.
            OutputSink::Event { .. } => Err(ExecutorFailure::Spawn {
                reason: format!(
                    "operation {operation_key} parked an event sink, and an agent process \
                     needs the terminal channel it was started with"
                ),
            }),
        }
    }

    /// The SSH connection is resolved synchronously, before anything is handed to the runtime:
    /// an unknown id is a refusal the caller can act on, and the alternative is a task that
    /// looks like it started and never produced a byte.
    fn ssh_connection(&self, connection_id: &str) -> Result<SshConnection, ExecutorFailure> {
        crate::ssh::ssh_connection_by_id(connection_id).map_err(|reason| ExecutorFailure::Spawn {
            reason: format!("ssh connection {connection_id} is unavailable: {reason}"),
        })
    }
}

/// dsh 只在本地工作区上运行：`run_dsh_task_core` 把 `project_path` 直接交给 webui
/// `create_session`，一个 SSH/WSL 工作区的 dsh 任务不是"换个路径"能表达的事，按名拒绝
/// 而不是猜。
fn dsh_refusal_reason() -> String {
    "a dsh task runs on a local workspace, and this launch request carried a remote one; \
     the webui launch body cannot address a remote path"
        .to_string()
}

fn missing_channel_reason(operation_key: &str) -> String {
    format!(
        "no terminal channel is parked for operation {operation_key}; a launch through this \
         executor must be started by the command that owns the window"
    )
}

/// The prompt a launch actually carries. `promptOverride` replaces the stored message rather
/// than appending to it — the same reading P2-04's parity harness uses for the client.
fn launch_prompt(request: &LaunchRequest) -> String {
    request
        .prompt_override
        .clone()
        .unwrap_or_else(|| request.prompt.clone())
}

fn terminal_size(size: Option<(u16, u16)>) -> (Option<u16>, Option<u16>) {
    size.map_or((None, None), |(cols, rows)| (Some(cols), Some(rows)))
}

fn optional_attachments(values: Vec<String>) -> Option<Vec<String>> {
    (!values.is_empty()).then_some(values)
}

/// Which stored permission spellings reach the CLI unchanged. `PermissionMode` cannot express
/// anything else, so the wire string is the only thing that keeps an unrecognised value from
/// growing a flag it never had.
fn permission_is_carried_verbatim(mode: PermissionMode, wire: &str) -> bool {
    matches!(
        (mode, wire),
        (PermissionMode::Ask, "ask")
            | (PermissionMode::AutoEdit, "auto_edit")
            | (PermissionMode::FullAccess, "full_access")
    )
}

impl<R: tauri::Runtime> TaskExecutor for LegacyTaskExecutor<R> {
    fn launch(&self, request: &LaunchRequest) -> Execution<()> {
        if request.family == ProtocolFamily::Dsh {
            let project_path = match request.working_directory.clone() {
                WorkingDirectory::Local { path } => path,
                _ => {
                    return Err(ExecutorFailure::Unsupported {
                        reason: dsh_refusal_reason(),
                    })
                }
            };
            let channel = self.channel_for(&request.operation_key)?;
            let app = self.app.clone();
            let task_id = request.task_id.clone();
            let agent = request.agent.clone();
            // Core minted this run's identity; the kernel registers the session and the
            // host's run registry under it (ADR-020/025), so this task's status reports
            // carry a generation core's staleness guard admits.
            let run = RunKey::minted(&task_id, request.generation);
            let prompt = launch_prompt(request);
            let session_id = request.session_id.clone();
            let (workspace_id, prompt_mode, client_time_zone) = match &request.dsh_window {
                Some(facts) => (
                    facts.workspace_id.clone(),
                    facts.prompt_mode.clone(),
                    facts.client_time_zone.clone(),
                ),
                None => (None, None, None),
            };
            let agent_preset = request.dsh_agent_preset.clone();
            let selected_model = request.selected_model.clone();
            let reasoning_effort = request.reasoning_effort.clone();
            // The dsh kernel takes the permission preset as `Option<String>`: `None` means
            // no `/permission` command at all, which is what an unrecognised stored spelling
            // must stay equal to.
            let permission_mode = (!request.permission_mode_wire.is_empty())
                .then(|| request.permission_mode_wire.clone());
            let images = optional_attachments(request.images.clone());
            // kernel 接线（补记二百六十八，D4 生效）：dsh 启动编排已上收 core
            // `dsh_host::executor::run_dsh_task`，本分支只剩「取 facts → 包 sink →
            // 分派」；`RunKey::minted` 的代经 `generation` 数字传给端口，宿主登记面
            // 在 adapter 的 `register_session`/`register_run` 里重建。
            let _ = run; // RunKey 的 (task, generation) 事实经 launch 字段下传。
            let sink: crate::dsh_webui::OutputSink = std::sync::Arc::new(move |text: String| {
                let _ = channel.send(text);
            });
            let launch = aeroric_core::agents::dsh_host::executor::DshLaunch {
                generation: request.generation,
                task_id,
                agent,
                project_path,
                prompt,
                session_id,
                workspace_id,
                agent_preset,
                prompt_mode,
                selected_model,
                reasoning_effort,
                permission_mode,
                images,
                client_time_zone,
            };
            tauri::async_runtime::spawn(async move {
                let ports = crate::dsh_webui::DshPortsAdapter::new(app.clone());
                let result =
                    aeroric_core::agents::dsh_host::executor::run_dsh_task(&ports, launch, sink)
                        .await;
                report_detached("launch", result);
            });
            return Ok(());
        }
        let channel = self.channel_for(&request.operation_key)?;
        let app = self.app.clone();
        let task_id = request.task_id.clone();
        let agent = request.agent.clone();
        let permission_mode = request.permission_mode_wire.clone();
        let prompt = launch_prompt(request);
        let selected_model = request.selected_model.clone();
        let reasoning_effort = request.reasoning_effort.clone();
        let speed = request.speed.clone();
        let images = optional_attachments(request.images.clone());
        let texts = optional_attachments(request.texts.clone());
        let force_prompt_injection = Some(request.inject_prompt_into_terminal);
        let (cols, rows) = terminal_size(request.terminal_size);
        let created_at = Some(request.created_at_ms as i64);
        // Core minted this run's identity, so the host's maps are keyed by it from the start
        // (ADR-020) and ADR-025's addressing can tell two runs of one task apart.
        let generation = request.generation;

        match request.working_directory.clone() {
            WorkingDirectory::Local { path } => {
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result = pty::run_task_core(
                        app.clone(),
                        &task_manager,
                        RunKey::minted(&task_id, generation),
                        task_id.clone(),
                        path,
                        prompt,
                        created_at,
                        agent,
                        permission_mode,
                        images,
                        texts,
                        selected_model,
                        reasoning_effort,
                        speed,
                        force_prompt_injection,
                        cols,
                        rows,
                        channel,
                    )
                    .await;
                    report_launch_detached(&app, &task_id, result);
                });
                Ok(())
            }
            WorkingDirectory::Ssh {
                connection_id,
                remote_path,
            } => {
                let connection = self.ssh_connection(&connection_id)?;
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result = crate::ssh::run_remote_task_core(
                        app.clone(),
                        &task_manager,
                        task_id.clone(),
                        connection,
                        remote_path,
                        prompt,
                        agent,
                        permission_mode,
                        selected_model,
                        reasoning_effort,
                        speed,
                        force_prompt_injection,
                        cols,
                        rows,
                        channel,
                    )
                    .await;
                    report_launch_detached(&app, &task_id, result);
                });
                Ok(())
            }
            WorkingDirectory::Wsl { distribution, path } => {
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result = crate::wsl::run_wsl_task_core(
                        app.clone(),
                        &task_manager,
                        task_id.clone(),
                        distribution,
                        path,
                        prompt,
                        agent,
                        permission_mode,
                        selected_model,
                        reasoning_effort,
                        speed,
                        force_prompt_injection,
                        cols,
                        rows,
                        channel,
                    )
                    .await;
                    report_launch_detached(&app, &task_id, result);
                });
                Ok(())
            }
        }
    }

    fn resume(&self, request: &ResumeRequest) -> Execution<()> {
        if request.family == ProtocolFamily::Dsh {
            return Err(ExecutorFailure::Unsupported {
                reason: "dsh has no native resume; the continuation is a handoff into a new \
                         session, which core decides (ADR-029) and `start` carries"
                    .to_string(),
            });
        }
        let channel = self.channel_for(&request.operation_key)?;
        let app = self.app.clone();
        let task_id = request.task_id.clone();
        let agent = request.agent.clone();
        let session_id = request.session_id.clone();
        let permission_mode = request.permission_mode_wire.clone();
        // A resume replays the original message only where the plan said the CLI can take it;
        // `resume_task_core`'s `_prompt` is that same value under the legacy name.
        let prompt = request.prompt.clone().unwrap_or_default();
        let selected_model = request.selected_model.clone();
        let reasoning_effort = request.reasoning_effort.clone();
        let speed = request.speed.clone();
        let (cols, rows) = terminal_size(request.terminal_size);
        let generation = request.generation;

        match request.working_directory.clone() {
            WorkingDirectory::Local { path } => {
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result = pty::resume_task_core(
                        app.clone(),
                        &task_manager,
                        RunKey::minted(&task_id, generation),
                        task_id.clone(),
                        path,
                        agent,
                        session_id,
                        prompt,
                        permission_mode,
                        selected_model,
                        reasoning_effort,
                        speed,
                        cols,
                        rows,
                        channel,
                    )
                    .await;
                    report_launch_detached(&app, &task_id, result);
                });
                Ok(())
            }
            WorkingDirectory::Ssh {
                connection_id,
                remote_path,
            } => {
                let connection = self.ssh_connection(&connection_id)?;
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result = crate::ssh::resume_remote_task_core(
                        app.clone(),
                        &task_manager,
                        task_id.clone(),
                        connection,
                        remote_path,
                        agent,
                        session_id,
                        permission_mode,
                        selected_model,
                        reasoning_effort,
                        speed,
                        cols,
                        rows,
                        channel,
                    )
                    .await;
                    report_launch_detached(&app, &task_id, result);
                });
                Ok(())
            }
            WorkingDirectory::Wsl { distribution, path } => {
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result = crate::wsl::resume_wsl_task_core(
                        app.clone(),
                        &task_manager,
                        task_id.clone(),
                        distribution,
                        path,
                        agent,
                        session_id,
                        permission_mode,
                        selected_model,
                        reasoning_effort,
                        speed,
                        cols,
                        rows,
                        channel,
                    )
                    .await;
                    report_launch_detached(&app, &task_id, result);
                });
                Ok(())
            }
        }
    }

    fn cancel(&self, task_id: &str, target: &CancelTarget) -> Execution<()> {
        let app = self.app.clone();
        let task_id = task_id.to_string();
        match target {
            CancelTarget::Local { project_path } => {
                let project_path = project_path.clone();
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result =
                        pty::cancel_task_core(&app, &task_manager, &task_id, &project_path);
                    report_detached("cancel", result);
                });
            }
            CancelTarget::Ssh => {
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result =
                        crate::ssh::cancel_remote_task_core(&app, &task_manager, &task_id).await;
                    report_detached("cancel", result);
                });
            }
            CancelTarget::Wsl => {
                tauri::async_runtime::spawn(async move {
                    let task_manager = app.state::<TaskManager>();
                    let result =
                        crate::wsl::cancel_wsl_task_core(&app, &task_manager, &task_id).await;
                    report_detached("cancel", result);
                });
            }
            CancelTarget::Dsh => {
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<DshWebUiManager>();
                    let result = crate::dsh_webui::cancel_dsh_task_core(&state, &task_id).await;
                    report_detached("cancel", result);
                });
            }
        }
        Ok(())
    }

    fn complete(&self, task_id: &str, working_directory: &str) -> Execution<()> {
        let app = self.app.clone();
        let task_id = task_id.to_string();
        let project_path = working_directory.to_string();
        tauri::async_runtime::spawn(async move {
            let task_manager = app.state::<TaskManager>();
            let result = pty::complete_task_core(&app, &task_manager, &task_id, &project_path);
            report_detached("complete", result);
        });
        Ok(())
    }

    /// Reset is the one port method whose result the caller acts on, so it cannot be detached
    /// the way a launch can — and the port is synchronous, so the wait has to happen off the
    /// calling thread. Running `block_on` here would panic whenever core reaches it from a
    /// Tauri command (a tokio worker), so the kernel runs on a thread of its own that owns the
    /// hand-off. Making this a returned `TaskEvent` instead is recorded as an open item for the
    /// wiring step in `docs/native/reviews/tauri-adapter.md`.
    fn reset(&self, task_id: &str) -> Result<ResetOutcome, ExecutorFailure> {
        let app = self.app.clone();
        let task_id = task_id.to_string();
        let handle = tauri::async_runtime::handle().clone();
        std::thread::spawn(move || {
            handle.block_on(async move {
                let task_manager = app.state::<TaskManager>();
                pty::reset_task_process_core(app.clone(), &task_manager, task_id)
                    .await
                    .map(pty::ResetTaskProcessResult::into_outcome)
                    .map_err(|reason| ExecutorFailure::Stop { reason })
            })
        })
        .join()
        .map_err(|_| ExecutorFailure::Stop {
            reason: "the reset thread panicked".to_string(),
        })?
    }
}

fn report_detached(stage: &str, result: Result<(), String>) {
    if let Err(reason) = result {
        eprintln!("[aeroric] legacy_task_executor {stage} failed after the handoff: {reason}");
    }
}

/// A launch/resume kernel that failed **after** the port returned `Ok` has no caller left to
/// hand the error to — the port is synchronous, the kernel is not (§4.8). Before the wiring,
/// the command awaited the kernel and its `Err` reached JS, whose `reportLaunchFailure` marked
/// the task failed; dropping that on the floor now would strand the row on `pending` with the
/// process never having existed. The observer owns exactly this: a failed *launch* is a failed
/// task, with the kernel's own words as the reason — the same shape `abandon_launch` gives the
/// synchronous refusals. The dsh kernel self-reports its launch-phase failures and must not be
/// double-reported here.
fn report_launch_detached<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => {}
        Err(reason) => {
            eprintln!("[aeroric] legacy_task_executor launch failed after the handoff: {reason}");
            crate::core_adapter::report_task_status_with_reason(
                app,
                task_id,
                "failed",
                Some(&reason),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(working_directory: WorkingDirectory) -> LaunchRequest {
        LaunchRequest {
            task_id: "t1".into(),
            project_id: "p1".into(),
            operation_key: "op-1".into(),
            generation: 3,
            family: ProtocolFamily::Claude,
            agent: "claude".into(),
            permission_mode: PermissionMode::Ask,
            permission_mode_wire: "default".into(),
            prompt: "original".into(),
            prompt_override: None,
            images: vec![],
            texts: vec![],
            inject_prompt_into_terminal: false,
            session_id: None,
            selected_model: None,
            reasoning_effort: None,
            speed: None,
            dsh_agent_preset: None,
            dsh_window: None,
            created_at_ms: 1_700_000_000_000,
            working_directory,
            terminal_size: Some((120, 40)),
        }
    }

    #[test]
    fn a_parked_channel_is_lent_exactly_once() {
        // The mailbox is the only place a window's bytes can be handed to a process; a second
        // take would mean two processes writing one terminal.
        let mailbox = OutputSinkMailbox::new();
        assert_eq!(mailbox.parked(), 0);
        mailbox.park(
            "op-1",
            OutputSink::Event {
                event_name: "x",
                id_key: "y",
            },
        );
        mailbox.park(
            "op-2",
            OutputSink::Event {
                event_name: "x",
                id_key: "y",
            },
        );
        assert_eq!(mailbox.parked(), 2);
        assert!(mailbox.take("op-1").is_some());
        assert!(
            mailbox.take("op-1").is_none(),
            "the same key cannot fund two runs"
        );
        assert!(mailbox.discard("op-2"));
        assert!(!mailbox.discard("op-2"));
        assert_eq!(mailbox.parked(), 0);
    }

    #[test]
    fn two_runs_of_one_task_do_not_share_a_mailbox_slot() {
        // ADR-017 permits this concurrency; a task-id-keyed mailbox would be the bug.
        let mailbox = OutputSinkMailbox::new();
        mailbox.park(
            "op-a",
            OutputSink::Event {
                event_name: "a",
                id_key: "y",
            },
        );
        mailbox.park(
            "op-b",
            OutputSink::Event {
                event_name: "b",
                id_key: "y",
            },
        );
        let first = match mailbox.take("op-a") {
            Some(OutputSink::Event { event_name, .. }) => event_name,
            _ => panic!("the first take must hand out op-a's sink"),
        };
        let second = match mailbox.take("op-b") {
            Some(OutputSink::Event { event_name, .. }) => event_name,
            _ => panic!("the second take must hand out op-b's sink"),
        };
        assert_eq!(first, "a");
        assert_eq!(second, "b");
    }

    #[test]
    fn a_handoff_prompt_replaces_the_message_it_does_not_append_to_it() {
        let mut request = request(WorkingDirectory::Local {
            path: "/repo".into(),
        });
        assert_eq!(launch_prompt(&request), "original");
        request.prompt_override = Some("carried over".into());
        assert_eq!(launch_prompt(&request), "carried over");
        request.prompt_override = Some(String::new());
        assert_eq!(
            launch_prompt(&request),
            "",
            "an explicit empty override is still an override — falling back to the stored \
             prompt would replay the original message"
        );
    }

    #[test]
    fn terminal_size_and_attachments_map_to_optional_arguments() {
        assert_eq!(terminal_size(None), (None, None));
        assert_eq!(terminal_size(Some((120, 40))), (Some(120), Some(40)));
        assert_eq!(optional_attachments(vec![]), None);
        assert_eq!(
            optional_attachments(vec!["png".into()]),
            Some(vec!["png".to_string()])
        );
    }

    #[test]
    fn the_wire_permission_mode_is_what_would_reach_the_cli() {
        // `default` is a value the record can hold that core's enum cannot express. Sending the
        // enum's spelling instead would add `--permission-mode default` to a command line that
        // today carries no permission flag at all.
        let request = request(WorkingDirectory::Local {
            path: "/repo".into(),
        });
        assert_eq!(request.permission_mode, PermissionMode::Ask);
        assert!(
            !permission_is_carried_verbatim(request.permission_mode, &request.permission_mode_wire),
            "Ask and \"default\" are different facts, which is why the executor uses the wire"
        );
        assert!(permission_is_carried_verbatim(
            PermissionMode::AutoEdit,
            "auto_edit"
        ));
        assert_eq!(
            crate::agent_family_maps::claude_permission_args(&request.permission_mode_wire),
            Vec::<&str>::new(),
            "the stored spelling decides the flags, and an unknown one decides none"
        );
        assert_eq!(
            crate::agent_family_maps::claude_permission_args("ask"),
            vec!["--permission-mode", "default"]
        );
    }

    #[test]
    fn the_dsh_refusal_is_about_the_workspace_not_the_facts() {
        // Since the dispatch exists, the only remaining refusal shape is a dsh task on a
        // remote working directory — and the reason has to say that, not gesture at facts
        // the request now carries.
        let reason = dsh_refusal_reason();
        assert!(reason.contains("local workspace"), "{reason}");
        assert!(reason.contains("remote"), "{reason}");
    }

    #[test]
    fn a_missing_channel_is_named_by_operation_not_by_task() {
        let reason = missing_channel_reason("op-42");
        assert!(reason.contains("op-42"), "{reason}");
        assert!(
            !reason.contains("t1"),
            "the task id is not what went wrong here"
        );
    }

    #[test]
    fn a_request_without_a_project_path_is_still_addressable_by_the_host() {
        // The working directory is a sum, so every arm names a real destination; an empty local
        // path is what the delete path tolerates and must not be turned into a guess.
        let request = request(WorkingDirectory::Local {
            path: String::new(),
        });
        assert!(matches!(
            request.working_directory,
            WorkingDirectory::Local { ref path } if path.is_empty()
        ));
    }
}
