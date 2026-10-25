//! Live structured sessions over the local UDS (ADR-043, N3): `session.prompt`
//! … `session.updates`, capability `session.live`. Registered only when the
//! host was assembled with a [`StructuredExecutor`]; never forwarded by the
//! remote plane. Drivers come from the executor's registry; task records are
//! changed only through `TaskService`.

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::sessions::live::executor::StructuredExecutor;
use aeroric_core::sessions::live::update::Replay;
use aeroric_core::sessions::live::{
    ConfigChange, Decision, DriverError, SessionDriver, SlashCommand,
};
use aeroric_core::storage::Record;
use aeroric_core::tasks::launch_plan::working_directory;
use aeroric_core::tasks::session_owner::launch_family_of;
use aeroric_core::tasks::{AgentFamilies, TaskService};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::session_live::{
    self as wire, CommandParams, ConfigureParams, LiveCommand, LiveDecision, PromptParams,
    RespondParams, SessionState, TaskParams, UpdatesBatch, UpdatesGap, UpdatesParams,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::{find_project_of_task, ok};

pub use wire::CAPABILITY;

struct Live {
    executor: Arc<StructuredExecutor>,
    families: Arc<dyn AgentFamilies>,
    service: Arc<TaskService>,
    context: Arc<CoreContext>,
}

type Run = fn(&Live, &Request) -> Frame;

pub fn register_session_live_services(
    set: &mut ServiceSet,
    executor: Arc<StructuredExecutor>,
    families: Arc<dyn AgentFamilies>,
    service: Arc<TaskService>,
    context: Arc<CoreContext>,
) {
    let live = Arc::new(Live {
        executor,
        families,
        service,
        context,
    });
    let methods: [(&str, Run); 8] = [
        (wire::PROMPT_METHOD, prompt),
        (wire::INTERRUPT_METHOD, interrupt),
        (wire::RESPOND_METHOD, respond),
        (wire::CONFIGURE_METHOD, configure),
        (wire::COMMAND_METHOD, command),
        (wire::CLOSE_METHOD, close),
        (wire::STATE_METHOD, state),
        (wire::UPDATES_METHOD, updates),
    ];
    for (name, run) in methods {
        set.register(name, blocking(&live, run));
    }
}

fn blocking(live: &Arc<Live>, run: Run) -> Handler {
    let live = Arc::clone(live);
    Arc::new(move |request: Request, _call| {
        let live = Arc::clone(&live);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(&live, &request)).await {
                Ok(frame) => frame,
                Err(e) => failure(id, ErrorCode::IoError, &e.to_string()),
            }
        })
    })
}

fn params<T: DeserializeOwned>(request: &Request) -> Result<T, Box<Frame>> {
    serde_json::from_value(request.params.clone()).map_err(|error| {
        Box::new(failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &format!("{}: {error}", request.method),
        ))
    })
}

fn driver_failure(request: &Request, error: DriverError) -> Frame {
    let code = match &error {
        DriverError::Unsupported(_) => ErrorCode::InvalidArgument,
        DriverError::UnknownRequest(_) => ErrorCode::NotFound,
        DriverError::Agent(_) | DriverError::Transport(_) => ErrorCode::IoError,
    };
    failure(request.id.clone(), code, &error.to_string())
}

fn no_session(request: &Request) -> Frame {
    failure(
        request.id.clone(),
        ErrorCode::NotFound,
        "this task has no open native session",
    )
}

impl Live {
    fn driver(&self, task_id: &str) -> Option<Arc<dyn SessionDriver>> {
        self.executor.registry().get(task_id)
    }

    /// The task's record and its project.
    fn record(&self, task_id: &str) -> Option<(Record, Record)> {
        let project = find_project_of_task(&self.context, task_id)?;
        let project_id = project.get("id").and_then(Value::as_str)?.to_string();
        let record = self
            .service
            .load(&project_id)
            .ok()?
            .records
            .into_iter()
            .find(|record| record.get("id").and_then(Value::as_str) == Some(task_id))?;
        Some((project, record))
    }

    /// N3 D3: reopen a task's Codex session through the regular resume path
    /// (generations and guards apply), so a prompt after a host restart or a
    /// close continues the same conversation.
    fn reopen(
        &self,
        request: &Request,
        task_id: &str,
    ) -> Result<Arc<dyn SessionDriver>, Box<Frame>> {
        let Some((project, record)) = self.record(task_id) else {
            return Err(Box::new(failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no task with that id in any known project",
            )));
        };
        let Some(session_id) = aeroric_core::tasks::session_owner::current_session(&record).0
        else {
            return Err(Box::new(no_session(request)));
        };
        let operation_key = format!("session-prompt-{task_id}-{}", now_ms());
        if let Err(error) =
            self.service
                .resume(&project, task_id, &session_id, &operation_key, true, None)
        {
            return Err(Box::new(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &error.to_string(),
            )));
        }
        self.driver(task_id).ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::UnsupportedCapability,
                "this task does not run as a native session",
            ))
        })
    }
}

fn prompt(live: &Live, request: &Request) -> Frame {
    let p: PromptParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if p.text.trim().is_empty() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "session.prompt needs text",
        );
    }
    let driver = match live.driver(&p.task_id) {
        Some(driver) => driver,
        None => match live.reopen(request, &p.task_id) {
            Ok(driver) => driver,
            Err(frame) => return *frame,
        },
    };
    match driver.prompt(p.text) {
        Ok(()) => ok(request, json!({"accepted": true})),
        Err(error) => driver_failure(request, error),
    }
}

fn with_driver(
    live: &Live,
    request: &Request,
    task_id: &str,
    act: impl FnOnce(&dyn SessionDriver) -> Result<(), DriverError>,
) -> Frame {
    let Some(driver) = live.driver(task_id) else {
        return no_session(request);
    };
    match act(driver.as_ref()) {
        Ok(()) => ok(request, json!({})),
        Err(error) => driver_failure(request, error),
    }
}

fn interrupt(live: &Live, request: &Request) -> Frame {
    match params::<TaskParams>(request) {
        Ok(p) => with_driver(live, request, &p.task_id, |d| d.interrupt()),
        Err(frame) => *frame,
    }
}

fn respond(live: &Live, request: &Request) -> Frame {
    let p: RespondParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let decision = match p.decision {
        LiveDecision::AllowOnce => Decision::AllowOnce,
        LiveDecision::AllowForSession => Decision::AllowForSession,
        LiveDecision::Deny => Decision::Deny {
            reason: p.reason.filter(|r| !r.trim().is_empty()),
        },
    };
    with_driver(live, request, &p.task_id, |d| {
        d.respond(&p.request_id, decision)
    })
}

fn configure(live: &Live, request: &Request) -> Frame {
    let p: ConfigureParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let change = ConfigChange {
        model: p.model.clone().filter(|m| !m.trim().is_empty()),
        permission: p.permission.map(|permission| permission.wire().to_string()),
        effort: p.effort.clone().filter(|e| !e.trim().is_empty()),
    };
    if change == ConfigChange::default() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "session.configure changes nothing",
        );
    }
    // N3 D4: the record follows, so the board and a later resume agree.
    let Some((project, _)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let fields: Vec<(&str, String)> = [
        ("selectedModel", change.model.clone()),
        ("permissionMode", change.permission.clone()),
        ("reasoningEffort", change.effort.clone()),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|v| (key, v)))
    .collect();
    let task_id = p.task_id.clone();
    let written = live.service.mutate(&project_id, |records, _now| {
        let mut changed = false;
        let next = records
            .iter()
            .map(|record| {
                if record.get("id").and_then(Value::as_str) != Some(task_id.as_str()) {
                    return record.clone();
                }
                let mut record = record.clone();
                for (key, value) in &fields {
                    if record.get(key).and_then(Value::as_str) != Some(value.as_str()) {
                        record
                            .0
                            .insert((*key).to_string(), Value::String(value.clone()));
                        changed = true;
                    }
                }
                record
            })
            .collect();
        (next, changed)
    });
    if let Err(error) = written {
        return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
    }
    match live.driver(&p.task_id) {
        Some(driver) => match driver.configure(change) {
            Ok(()) => ok(request, json!({})),
            Err(error) => driver_failure(request, error),
        },
        // No open session: the record carries it into the next resume.
        None => ok(request, json!({})),
    }
}

fn command(live: &Live, request: &Request) -> Frame {
    let p: CommandParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let command = match p.command {
        LiveCommand::Compact => SlashCommand::Compact,
        LiveCommand::Review => SlashCommand::Review,
    };
    with_driver(live, request, &p.task_id, |d| d.command(command))
}

fn close(live: &Live, request: &Request) -> Frame {
    match params::<TaskParams>(request) {
        Ok(p) if live.executor.close_session(&p.task_id) => ok(request, json!({})),
        Ok(_) => no_session(request),
        Err(frame) => *frame,
    }
}

fn state(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let snapshot = match live.driver(&p.task_id) {
        Some(driver) => driver.snapshot(),
        None => {
            let Some((project, record)) = live.record(&p.task_id) else {
                return failure(
                    request.id.clone(),
                    ErrorCode::NotFound,
                    "no task with that id in any known project",
                );
            };
            let (session_id, session_path) =
                aeroric_core::tasks::session_owner::current_session(&record);
            let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_string);
            let family = launch_family_of(&record, live.families.as_ref());
            let structured = working_directory(&record, &project).is_some_and(|cwd| {
                live.executor.takes_task(
                    family,
                    record
                        .get("agent")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    record
                        .get("agentConfigId")
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty()),
                    &cwd,
                )
            });
            let resume_blocked = aeroric_core::sessions::live::executor::resume_blocked(
                family,
                session_path.as_deref(),
                session_id.as_deref().unwrap_or_default(),
            );
            SessionState {
                task_id: p.task_id.clone(),
                live: false,
                structured,
                resume_blocked,
                family: Some(family),
                session_id,
                model: text("selectedModel"),
                permission: text("permissionMode"),
                effort: text("reasoningEffort"),
                latest_seq: live
                    .executor
                    .registry()
                    .log(&p.task_id)
                    .map_or(0, |log| log.latest_seq()),
                ..SessionState::default()
            }
        }
    };
    match serde_json::to_value(snapshot) {
        Ok(value) => ok(request, value),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

fn updates(live: &Live, request: &Request) -> Frame {
    let p: UpdatesParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let limit = p
        .limit
        .unwrap_or(wire::DEFAULT_BATCH_LIMIT)
        .clamp(1, wire::MAX_BATCH_LIMIT);
    let batch = match live.executor.registry().log(&p.task_id) {
        Some(log) => {
            let latest_seq = log.latest_seq();
            if p.after > latest_seq {
                // The log restarted (a reopened session): the client's cursor
                // belongs to an earlier one and must re-read history.
                UpdatesBatch {
                    updates: vec![],
                    latest_seq,
                    gap: Some(UpdatesGap {
                        oldest_available: 1,
                    }),
                }
            } else {
                match log.page(p.after, limit) {
                    Replay::Updates(updates) => UpdatesBatch {
                        updates,
                        latest_seq,
                        gap: None,
                    },
                    Replay::Gap { oldest_available } => UpdatesBatch {
                        updates: vec![],
                        latest_seq,
                        gap: Some(UpdatesGap { oldest_available }),
                    },
                }
            }
        }
        None if p.after == 0 => UpdatesBatch {
            updates: vec![],
            latest_seq: 0,
            gap: None,
        },
        None => UpdatesBatch {
            updates: vec![],
            latest_seq: 0,
            gap: Some(UpdatesGap {
                oldest_available: 1,
            }),
        },
    };
    match serde_json::to_value(batch) {
        Ok(value) => ok(request, value),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
