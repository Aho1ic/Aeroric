//! Live structured sessions over the local UDS (ADR-043, N3): `session.prompt`
//! … `session.updates`, capability `session.live`. Registered only when the
//! host was assembled with a [`StructuredExecutor`]; never forwarded by the
//! remote plane. Drivers come from the executor's registry; task records are
//! changed only through `TaskService`.

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::sessions::live::auto_check::{AutoCheckPolicy, AutoCheckSelection};
use aeroric_core::sessions::live::executor::StructuredExecutor;
use aeroric_core::sessions::live::update::Replay;
use aeroric_core::sessions::live::{
    Answer, ConfigChange, Decision, DriverError, SessionDriver, SlashCommand,
};
use aeroric_core::settings::preferences;
use aeroric_core::storage::Record;
use aeroric_core::tasks::launch_plan::working_directory;
use aeroric_core::tasks::session_owner::launch_family_of;
use aeroric_core::tasks::{AgentFamilies, TaskService};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::session_live::{
    self as wire, AnswerParams, CommandParams, ConfigureParams, LiveCommand, LiveDecision,
    PromptParams, RespondParams, RevokeGrantParams, SessionState, TaskParams, UpdatesBatch,
    UpdatesGap, UpdatesParams,
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

/// ADR-043: settle the native tasks a previous host process left active. Their
/// drivers died with it, so nothing would ever move them off `running`; a task
/// with a driver in `executor` is left alone. Returns the settled task ids.
pub fn settle_orphaned_sessions(
    executor: Arc<StructuredExecutor>,
    families: Arc<dyn AgentFamilies>,
    service: Arc<TaskService>,
    context: Arc<CoreContext>,
) -> Vec<String> {
    Live {
        executor,
        families,
        service,
        context,
    }
    .settle_orphaned()
}

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
    live.executor
        .set_auto_check_policy(Arc::new(SettingsAutoChecks(Arc::clone(&live.context))));
    // Before any session method can answer: the previous host's drivers are gone.
    live.settle_orphaned();
    let methods: [(&str, Run); 10] = [
        (wire::PROMPT_METHOD, prompt),
        (wire::INTERRUPT_METHOD, interrupt),
        (wire::RESPOND_METHOD, respond),
        (wire::ANSWER_METHOD, answer),
        (wire::REVOKE_GRANT_METHOD, revoke_grant),
        (wire::CONFIGURE_METHOD, configure),
        (wire::COMMAND_METHOD, command),
        (wire::CLOSE_METHOD, close),
        (wire::STATE_METHOD, state),
        (wire::UPDATES_METHOD, updates),
    ];
    for (name, run) in methods {
        set.register(name, blocking(&live, run));
    }
    set.register(AUTO_CHECK_GET_METHOD, blocking(&live, auto_check_get));
    set.register(AUTO_CHECK_SET_METHOD, blocking(&live, auto_check_set));
    set.register(
        AUTO_CHECK_FAILURES_METHOD,
        blocking(&live, auto_check_failures),
    );
}

/// A05: which checks the task's project runs after a turn; keyed by task so
/// the client never needs the project id.
pub const AUTO_CHECK_GET_METHOD: &str = "session.autoCheck.get";
pub const AUTO_CHECK_SET_METHOD: &str = "session.autoCheck.set";
/// Tasks whose latest check run failed, for the "需要我处理" queue.
pub const AUTO_CHECK_FAILURES_METHOD: &str = "session.autoCheck.failures";

/// Read on every turn end, so a toggle applies from the next turn on.
struct SettingsAutoChecks(Arc<CoreContext>);

impl AutoCheckPolicy for SettingsAutoChecks {
    fn selection(&self, project_id: &str) -> AutoCheckSelection {
        let fields = aeroric_core::storage::load_settings(&self.0)
            .ok()
            .and_then(|outcome| outcome.found());
        preferences::auto_check_selection(fields.as_ref(), project_id)
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutoCheckParams {
    task_id: String,
    tests: bool,
    diagnostics: bool,
}

fn project_of(live: &Live, request: &Request, task_id: &str) -> Result<String, Box<Frame>> {
    find_project_of_task(&live.context, task_id)
        .and_then(|project| {
            project
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no task with that id in any known project",
            ))
        })
}

fn selection_json(selection: AutoCheckSelection) -> Value {
    json!({"tests": selection.tests, "diagnostics": selection.diagnostics})
}

fn auto_check_get(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match project_of(live, request, &p.task_id) {
        Ok(project_id) => ok(
            request,
            selection_json(SettingsAutoChecks(Arc::clone(&live.context)).selection(&project_id)),
        ),
        Err(frame) => *frame,
    }
}

fn auto_check_failures(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "failures": live.executor.check_failures() }),
    )
}

fn auto_check_set(live: &Live, request: &Request) -> Frame {
    let p: AutoCheckParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let project_id = match project_of(live, request, &p.task_id) {
        Ok(project_id) => project_id,
        Err(frame) => return *frame,
    };
    let selection = AutoCheckSelection {
        tests: p.tests,
        diagnostics: p.diagnostics,
    };
    match preferences::update_preference(
        &live.context,
        preferences::PreferenceUpdate::AutoCheck {
            project_id,
            selection,
        },
    ) {
        Ok(_) => ok(request, selection_json(selection)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
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

    /// Whether this task's runs go to a structured driver.
    fn structured(&self, record: &Record, project: &Record) -> bool {
        working_directory(record, project).is_some_and(|cwd| {
            self.executor.takes_task(
                launch_family_of(record, self.families.as_ref()),
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
        })
    }

    fn settle_orphaned(&self) -> Vec<String> {
        let Ok(projects) =
            aeroric_core::projects::ProjectService::new(Arc::clone(&self.context)).list()
        else {
            return Vec::new();
        };
        let mut settled = Vec::new();
        for project in projects {
            let Some(project_id) = project.get("id").and_then(Value::as_str) else {
                continue;
            };
            let orphan = |record: &Record| {
                record
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| self.driver(id).is_none())
                    && self.structured(record, &project)
            };
            match self.service.settle_orphaned_sessions(project_id, &orphan) {
                Ok((ids, _)) => settled.extend(ids),
                Err(error) => eprintln!(
                    "[aeroric-host] cannot settle native sessions left by the previous host in {project_id}: {error}"
                ),
            }
        }
        settled
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

fn answer(live: &Live, request: &Request) -> Frame {
    let p: AnswerParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let answer = match (p.cancelled, p.value, p.confirmed) {
        (true, _, _) => Answer::Cancelled,
        (false, Some(value), None) => Answer::Value(value),
        (false, None, Some(confirmed)) => Answer::Confirmed(confirmed),
        _ => {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "session.answer takes exactly one of value, confirmed or cancelled",
            )
        }
    };
    with_driver(live, request, &p.task_id, |d| {
        d.answer(&p.question_id, answer)
    })
}

fn revoke_grant(live: &Live, request: &Request) -> Frame {
    let p: RevokeGrantParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    with_driver(live, request, &p.task_id, |d| d.revoke_grant(&p.subject))
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
            let session_id = aeroric_core::tasks::session_owner::current_session(&record).0;
            let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_string);
            let family = launch_family_of(&record, live.families.as_ref());
            let structured = live.structured(&record, &project);
            SessionState {
                task_id: p.task_id.clone(),
                live: false,
                structured,
                resume_blocked: None,
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
