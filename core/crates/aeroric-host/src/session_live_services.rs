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
use aeroric_core::tasks::{AgentFamilies, ProtocolFamily, TaskService};
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
    // B4′: checkpoints live beside the other per-task files.
    live.executor
        .checkpoints()
        .set_dir(live.context.paths.data_dir().join("checkpoints"));
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
    set.register(MOVE_SOURCE_METHOD, blocking(&live, move_source));
    set.register(PLAN_SWITCH_METHOD, blocking(&live, plan_switch));
    set.register(AUTO_CHECK_GET_METHOD, blocking(&live, auto_check_get));
    set.register(AUTO_CHECK_SET_METHOD, blocking(&live, auto_check_set));
    set.register(
        AUTO_CHECK_FAILURES_METHOD,
        blocking(&live, auto_check_failures),
    );
    set.register(cp_wire::GET_METHOD, blocking(&live, checkpoint_get));
    set.register(
        cp_wire::ADD_CONSTRAINT_METHOD,
        blocking(&live, checkpoint_add_constraint),
    );
    set.register(
        cp_wire::SET_OBLIGATION_METHOD,
        blocking(&live, checkpoint_set_obligation),
    );
    set.register(
        cp_wire::CLARIFY_ACCEPT_METHOD,
        blocking(&live, clarify_accept),
    );
    set.register(cp_wire::BRANCH_START_METHOD, blocking(&live, branch_start));
    set.register(
        cp_wire::BRANCH_RETURN_METHOD,
        blocking(&live, branch_return),
    );
    set.register(CONTEXT_SEARCH_METHOD, blocking(&live, context_search));
    set.register(CONTEXT_READ_METHOD, blocking(&live, context_read));
    set.register(CONTEXT_HANDOFF_METHOD, blocking(&live, context_handoff));
    // B4′: sessions get the context server only from the product binary
    // (a test harness is not an `aeroric-cli` that can serve it).
    if let Ok(exe) = std::env::current_exe() {
        let is_cli = exe
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "aeroric-cli");
        if is_cli {
            live.executor.set_context_program(
                exe.to_string_lossy().into_owned(),
                live.context.paths.data_dir().to_string_lossy().into_owned(),
            );
        }
    }
}

// MARK: B4′ side branches (2026-10-30)

/// The record field naming a side branch's parent task.
const BRANCH_OF_FIELD: &str = "branchOf";

/// `branch.start {taskId}`: a side branch — a fork of the task's session,
/// marked with its parent so its conclusion can come back. `{taskId}` of the branch.
fn branch_start(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((project, _)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let branch = match crate::workspace_meta_services::fork_task(
        &live.context,
        &live.service,
        &p.task_id,
        "侧支",
    ) {
        Ok(id) => id,
        Err(crate::workspace_meta_services::ForkError::NotFound(message)) => {
            return failure(request.id.clone(), ErrorCode::NotFound, message)
        }
        Err(crate::workspace_meta_services::ForkError::Invalid(message)) => {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &format!("{message}（侧支目前只支持 Claude Code 与 Codex）"),
            )
        }
    };
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let parent = p.task_id.clone();
    let id = branch.clone();
    let written = live.service.mutate(&project_id, |records, _now| {
        let next = records
            .iter()
            .map(|record| {
                let mut record = record.clone();
                if record.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                    record
                        .0
                        .insert(BRANCH_OF_FIELD.into(), Value::String(parent.clone()));
                }
                record
            })
            .collect();
        (next, true)
    });
    if let Err(error) = written {
        return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
    }
    ok(request, json!({"taskId": branch}))
}

/// `branch.return {taskId}` (the branch): its last answer goes back to the
/// parent as a result card and onto the parent's checkpoint; the branch's
/// process stays in the branch.
fn branch_return(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((_, record)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let Some(parent) = record
        .get(BRANCH_OF_FIELD)
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "这个会话不是侧支",
        );
    };
    let Some(answer) = context_messages(live, &record)
        .into_iter()
        .rev()
        .find(|(role, _)| role == "assistant")
        .map(|(_, text)| text)
    else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "侧支还没有回答可以带回",
        );
    };
    let title = context_title(&record);
    let title = title.trim_end_matches("（侧支）").to_string();
    let result = cp_wire::BranchResult {
        task_id: p.task_id.clone(),
        title,
        summary: answer.chars().take(2000).collect(),
        files: 0,
        additions: 0,
        deletions: 0,
        returned_ms: now_ms(),
    };
    live.executor
        .checkpoints()
        .add_branch(&parent, result.clone());
    if let Some(log) = live.executor.registry().log(&parent) {
        log.push(
            aeroric_protocol::session_live::SessionUpdateKind::BranchReturned {
                result: result.clone(),
            },
            now_ms(),
        );
    }
    ok(request, json!({"parentTaskId": parent, "result": result}))
}

// MARK: B4′ context server (2026-10-30)

/// Called by `aeroric-cli mcp-context` on behalf of an agent; every call
/// carries the token its session was started with.
pub const CONTEXT_SEARCH_METHOD: &str = "context.searchSessions";
pub const CONTEXT_READ_METHOD: &str = "context.readSession";
pub const CONTEXT_HANDOFF_METHOD: &str = "context.readHandoff";
/// A transcript bigger than this is skipped by the search (not truncated).
const CONTEXT_MAX_FILE: u64 = 32 * 1024 * 1024;
const CONTEXT_SNIPPET: usize = 80;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextParams {
    token: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    all_projects: bool,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    from: Option<usize>,
}

/// `(task_id, project_id)` the token was issued for.
fn context_scope(
    live: &Live,
    request: &Request,
) -> Result<(ContextParams, String, String), Box<Frame>> {
    let p: ContextParams = params(request)?;
    match live.executor.context_tokens().resolve(&p.token) {
        Some((task, project)) => Ok((p, task, project)),
        None => Err(Box::new(failure(
            request.id.clone(),
            ErrorCode::Unauthenticated,
            "上下文令牌无效或会话已结束",
        ))),
    }
}

/// The text blocks of a task's current session, in order (role, text).
fn context_messages(live: &Live, record: &Record) -> Vec<(String, String)> {
    let (_, path) = aeroric_core::tasks::session_owner::current_session(record);
    let Some(path) = path else {
        return Vec::new();
    };
    let too_big = std::fs::metadata(&path)
        .map(|m| m.len() > CONTEXT_MAX_FILE)
        .unwrap_or(true);
    if too_big {
        return Vec::new();
    }
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let family = launch_family_of(record, live.families.as_ref());
    crate::services::parse_messages(&lines, family)
        .into_iter()
        .filter_map(|m| {
            let text: Vec<String> = m
                .content
                .iter()
                .filter_map(|c| match c {
                    aeroric_core::sessions::SessionContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            (!text.is_empty()).then(|| (m.role.clone(), text.join("\n")))
        })
        .collect()
}

fn context_title(record: &Record) -> String {
    let text = |k: &str| {
        record
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    text("name")
        .or_else(|| text("prompt"))
        .map(|s| s.chars().take(60).collect())
        .unwrap_or_else(|| "未命名会话".into())
}

/// Around the first match: `CONTEXT_SNIPPET` chars each side, on char bounds.
fn context_snippet(text: &str, at: usize, len: usize) -> String {
    let start = text[..at]
        .char_indices()
        .rev()
        .nth(CONTEXT_SNIPPET)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let end = text[at + len..]
        .char_indices()
        .nth(CONTEXT_SNIPPET)
        .map(|(i, _)| at + len + i)
        .unwrap_or(text.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(text[start..end].trim());
    if end < text.len() {
        out.push('…');
    }
    out
}

/// `context.searchSessions {token, query, limit?, allProjects?}`: sessions of
/// the token's project whose text contains `query` (case-insensitive). Other
/// projects are not searched in this version (Q-F18: needs the user's opt-in,
/// which has no switch yet).
fn context_search(live: &Live, request: &Request) -> Frame {
    let (p, task_id, project_id) = match context_scope(live, request) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    if p.all_projects {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "跨项目检索需要用户在 Aeroric 里授权，当前未开启；只能检索本项目的会话",
        );
    }
    let query = p.query.unwrap_or_default();
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "query 不能为空",
        );
    }
    let limit = p.limit.unwrap_or(10).clamp(1, 50);
    let records = match live.service.load(&project_id) {
        Ok(snapshot) => snapshot.records,
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    };
    let mut hits = Vec::new();
    'records: for record in &records {
        let Some(id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        for (index, (role, text)) in context_messages(live, record).iter().enumerate() {
            let lower = text.to_lowercase();
            // Lowercasing can move byte offsets; match on the original when it does.
            let found = if lower.len() == text.len() {
                lower.find(&needle).map(|at| (at, needle.len()))
            } else {
                text.find(query.trim()).map(|at| (at, query.trim().len()))
            };
            if let Some((at, len)) = found {
                hits.push(json!({
                    "taskId": id,
                    "title": context_title(record),
                    "current": id == task_id,
                    "messageIndex": index,
                    "role": role,
                    "snippet": context_snippet(text, at, len),
                }));
                if hits.len() >= limit {
                    break 'records;
                }
            }
        }
    }
    ok(request, json!({"query": query, "hits": hits}))
}

/// `context.readSession {token, taskId?, from?, limit?}`: a session's text
/// messages in this project (the token's own task by default).
fn context_read(live: &Live, request: &Request) -> Frame {
    let (p, task_id, project_id) = match context_scope(live, request) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let wanted = p.task_id.unwrap_or(task_id);
    let records = match live.service.load(&project_id) {
        Ok(snapshot) => snapshot.records,
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    };
    let Some(record) = records
        .iter()
        .find(|r| r.get("id").and_then(Value::as_str) == Some(wanted.as_str()))
    else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "本项目没有这个会话",
        );
    };
    let messages = context_messages(live, record);
    let limit = p.limit.unwrap_or(40).clamp(1, 200);
    let from = p
        .from
        .unwrap_or_else(|| messages.len().saturating_sub(limit));
    let page: Vec<Value> = messages
        .iter()
        .enumerate()
        .skip(from)
        .take(limit)
        .map(|(i, (role, text))| {
            let text: String = text.chars().take(4000).collect();
            json!({"index": i, "role": role, "text": text})
        })
        .collect();
    ok(
        request,
        json!({"taskId": wanted, "title": context_title(record), "total": messages.len(), "messages": page}),
    )
}

/// `context.readHandoff {token}`: the token's task checkpoint, as the brief.
fn context_handoff(live: &Live, request: &Request) -> Frame {
    let (_, task_id, _) = match context_scope(live, request) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let checkpoint = live
        .executor
        .checkpoints()
        .get(&task_id)
        .unwrap_or_else(|| cp_wire::Checkpoint::new(&task_id));
    let brief = aeroric_core::sessions::live::checkpoint::brief::handoff_brief(
        &checkpoint,
        cp_wire::HandoffReason::Resume,
    );
    ok(request, json!({"brief": brief, "checkpoint": checkpoint}))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClarifyAcceptParams {
    task_id: String,
    #[serde(default)]
    acceptance: Vec<cp_wire::Acceptance>,
    #[serde(default)]
    constraints: Vec<String>,
}

/// `clarify.accept {taskId, acceptance, constraints}`: the user ticked the
/// criteria. They become the task's obligations, the permission chosen at
/// submission comes back, and the work starts with them in the message.
fn clarify_accept(live: &Live, request: &Request) -> Frame {
    use aeroric_core::sessions::live::checkpoint::clarify;
    let p: ClarifyAcceptParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((project, record)) = live.record(&p.task_id) else {
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
    let restore = record
        .get(clarify::RESTORE_FIELD)
        .and_then(Value::as_str)
        .unwrap_or("ask")
        .to_string();
    let checkpoints = live.executor.checkpoints();
    checkpoints.add_obligations(
        &p.task_id,
        p.acceptance
            .iter()
            .enumerate()
            .map(|(i, a)| cp_wire::Obligation {
                id: format!("a{}", i + 1),
                text: a.text.clone(),
                check: a.check.clone(),
                state: cp_wire::ObligationState::Open,
                source: cp_wire::ObligationSource::Acceptance,
            })
            .collect(),
    );
    for constraint in &p.constraints {
        checkpoints.add_constraint(&p.task_id, constraint);
    }
    let task_id = p.task_id.clone();
    let restored = restore.clone();
    let written = live.service.mutate(&project_id, |records, _now| {
        let next = records
            .iter()
            .map(|record| {
                let mut record = record.clone();
                if record.get("id").and_then(Value::as_str) == Some(task_id.as_str()) {
                    record.0.remove(clarify::RESTORE_FIELD);
                    record
                        .0
                        .insert("permissionMode".into(), Value::String(restored.clone()));
                }
                record
            })
            .collect();
        (next, true)
    });
    if let Err(error) = written {
        return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
    }
    let driver = match live.driver(&p.task_id) {
        Some(driver) => driver,
        None => match live.reopen(request, &p.task_id) {
            Ok(driver) => driver,
            Err(frame) => return *frame,
        },
    };
    if let Err(error) = driver.configure(ConfigChange {
        permission: Some(restore),
        ..ConfigChange::default()
    }) {
        return driver_failure(request, error);
    }
    match driver.prompt(clarify::start_message(&p.acceptance, &p.constraints)) {
        Ok(()) => ok(request, json!({"accepted": true})),
        Err(error) => driver_failure(request, error),
    }
}

// MARK: B4′ checkpoints (2026-10-30)

use aeroric_protocol::checkpoint as cp_wire;

/// `checkpoint.get {taskId}` → `{checkpoint}` (an empty one when the task has
/// none yet, so the progress page always has something to draw).
fn checkpoint_get(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((_, record)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let checkpoint = live
        .executor
        .checkpoints()
        .get(&p.task_id)
        .unwrap_or_else(|| cp_wire::Checkpoint::new(&p.task_id));
    ok(
        request,
        json!({ "checkpoint": checkpoint, "branchOf": record.get(BRANCH_OF_FIELD) }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConstraintParams {
    task_id: String,
    text: String,
}

/// `checkpoint.addConstraint {taskId, text}`: a constraint the user states;
/// it travels with every later handoff.
fn checkpoint_add_constraint(live: &Live, request: &Request) -> Frame {
    let p: ConstraintParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if p.text.trim().is_empty() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "约束不能为空",
        );
    }
    if let Err(frame) = project_of(live, request, &p.task_id) {
        return *frame;
    }
    live.executor
        .checkpoints()
        .add_constraint(&p.task_id, &p.text);
    ok(request, json!({}))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObligationParams {
    task_id: String,
    id: String,
    state: cp_wire::ObligationState,
}

/// `checkpoint.setObligation {taskId, id, state}`: settle a criterion by hand.
fn checkpoint_set_obligation(live: &Live, request: &Request) -> Frame {
    let p: ObligationParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if let Err(frame) = project_of(live, request, &p.task_id) {
        return *frame;
    }
    if live
        .executor
        .checkpoints()
        .set_obligation(&p.task_id, &p.id, p.state)
    {
        ok(request, json!({}))
    } else {
        failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "这个任务没有这条义务",
        )
    }
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
    // A source move that could not carry the session over (2026-10-29) left a
    // handoff: the next message starts a fresh run with it in front.
    if live.driver(&p.task_id).is_none() {
        if let Some(frame) = start_with_handoff(live, request, &p.task_id, &p.text) {
            return frame;
        }
    }
    let driver = match live.driver(&p.task_id) {
        Some(driver) => driver,
        None => match live.reopen(request, &p.task_id) {
            Ok(driver) => driver,
            Err(frame) => return *frame,
        },
    };
    let checkpoints = live.executor.checkpoints();
    checkpoints.note_prompt(&p.task_id, &p.text);
    // B4′: same session, new model or compacted — remind it of what is open.
    let text = match checkpoints.take_reminder(&p.task_id) {
        Some(reminder) => format!("{reminder}{}", p.text),
        None => p.text,
    };
    match driver.prompt(text) {
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
    if change.model.is_some() {
        live.executor.checkpoints().arm_reminder(
            &p.task_id,
            aeroric_core::sessions::live::checkpoint::HandoffReason::ModelSwitch,
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
    if command == SlashCommand::Compact {
        live.executor.checkpoints().arm_reminder(
            &p.task_id,
            aeroric_core::sessions::live::checkpoint::HandoffReason::AfterCompact,
        );
    }
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

// MARK: move a session to another model source (2026-10-29)

/// `session.moveSource {taskId, configId?, model}`: the task continues under
/// another source (an Agent configuration, or the harness's own login when
/// `configId` is absent) with the conversation intact. When the session fits
/// the new model's window it is copied into the new source's home and forked
/// there (`native`); otherwise, or when this harness cannot be moved, the
/// whole transcript is written to a handoff file and the next message starts
/// a fresh session that is pointed at it (`handoff`). The old model is never
/// called, so this works when its upstream is down or out of quota.
pub const MOVE_SOURCE_METHOD: &str = "session.moveSource";
pub const PLAN_SWITCH_METHOD: &str = "session.planSwitch";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanParams {
    task_id: String,
    model: String,
    /// The source the picker offered the model under (legacy moves only).
    #[serde(default)]
    config_id: Option<String>,
}

/// How a model change happens (2026-10-30, D7/D9/D10). Under 「本机登录」 on
/// the gateway every model it routes switches in place, unless the context
/// would not fit the new window (then a handoff move). A session still on
/// the direct login, or on this harness's third-party sources, moves onto
/// it once. With the login off the 2026-10-29 moves between sources stay.
/// Answers `{mode: inPlace|move, configId, handoff, confirm}`; the App then
/// configures the model or calls `session.moveSource`.
fn plan_switch(live: &Live, request: &Request) -> Frame {
    use aeroric_core::agent_connections::{
        model::{ConnectionsDocument, Harness},
        routing, store,
    };
    let p: PlanParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((_, record)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let model = p.model.trim().to_string();
    if model.is_empty() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "缺少要换到的模型",
        );
    }
    let family = launch_family_of(&record, live.families.as_ref());
    let data = live.context.paths.data_dir().to_path_buf();
    let current = record
        .get("agentConfigId")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let harness = match family {
        ProtocolFamily::Codex => Some(Harness::Codex),
        ProtocolFamily::Claude => Some(Harness::Claude),
        _ => None,
    };
    let doc = store::load_effective(&store::document_path(&data)).ok();
    let login = harness.and_then(|h| {
        let id = ConnectionsDocument::via_login_id(h);
        let doc = doc.as_ref()?;
        let config = doc.config(&id)?;
        (!routing::candidates(doc, &config, &model).0.is_empty()).then_some((h, id))
    });
    let (in_place, config_id) = switch_route(
        login.as_ref().map(|(h, id)| (h.slug(), id.as_str())),
        current.as_deref(),
        p.config_id.as_deref(),
    );
    let fits = {
        let (_, path) = aeroric_core::tasks::session_owner::current_session(&record);
        let lines: Vec<String> = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|raw| raw.lines().map(str::to_string).collect())
            .unwrap_or_default();
        let used = context_used(&lines, family, path.as_deref());
        match (used, target_window(&data, config_id.as_deref(), &model)) {
            (Some(used), Some(window)) => used * 100 <= window * FIT_PERCENT,
            _ => true,
        }
    };
    let in_place = in_place && fits;
    let crossing = login.is_none() && current.is_none() != config_id.is_none();
    ok(
        request,
        json!({
            "mode": if in_place { "inPlace" } else { "move" },
            "configId": config_id,
            "handoff": !fits,
            "confirm": !fits || crossing,
        }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MoveParams {
    task_id: String,
    #[serde(default)]
    config_id: Option<String>,
    model: String,
}

/// The fraction of the new window a carried-over context may fill (the rest
/// is room for the next turns).
const FIT_PERCENT: u64 = 85;
/// The handoff's inline part may use this share of the new window.
const DIGEST_PERCENT: u64 = 40;
/// Characters per token for sizing text (mixed Chinese and English).
const CHARS_PER_TOKEN: u64 = 2;
const HANDOFF_KEY: &str = "pendingHandoff";

fn move_source(live: &Live, request: &Request) -> Frame {
    let p: MoveParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let invalid = |message: &str| failure(request.id.clone(), ErrorCode::InvalidArgument, message);
    let Some((project, record)) = live.record(&p.task_id) else {
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
    let text = |key: &str| {
        record
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let agent = text("agent").unwrap_or_default();
    let family = launch_family_of(&record, live.families.as_ref());
    let data = live.context.paths.data_dir().to_path_buf();
    let config_id = p.config_id.clone().filter(|c| !c.trim().is_empty());
    let target = match &config_id {
        Some(id) => match aeroric_core::agent_connections::launch::resolve_config(&data, id) {
            Ok(config) => Some(config),
            Err(reason) => return invalid(&reason),
        },
        None => None,
    };
    if let Some(config) = &target {
        if config.agent() != agent {
            return invalid("只能换到同一个 Harness 的模型来源");
        }
    }
    let model = p.model.trim().to_string();
    if model.is_empty() {
        return invalid("缺少要换到的模型");
    }
    let (session, path) = aeroric_core::tasks::session_owner::current_session(&record);
    let lines: Vec<String> = path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|raw| raw.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let used = context_used(&lines, family, path.as_deref());
    let window = target_window(&data, config_id.as_deref(), &model);
    let fits = match (used, window) {
        (Some(used), Some(window)) => used * 100 <= window * FIT_PERCENT,
        _ => true,
    };
    let source_home = home_of(&data, text("agentConfigId").as_deref(), family);
    let target_home = home_of(&data, config_id.as_deref(), family);
    let moved = match (fits, &session, &path, &source_home, &target_home) {
        (true, Some(session), Some(path), Some(from), Some(to))
            if matches!(family, ProtocolFamily::Claude | ProtocolFamily::Codex)
                && aeroric_core::sessions::live::fork_source(session).is_none() =>
        {
            copy_transcript(std::path::Path::new(path), from, to, family)
                .map(|copied| (session.clone(), copied))
        }
        _ => None,
    };
    // The task now runs under the new source; its process restarts on the next message.
    let mut fields: Vec<(&str, Option<String>)> = vec![
        ("agentConfigId", config_id.clone()),
        ("selectedModel", Some(model.clone())),
    ];
    let mode = match &moved {
        Some(_) => "native",
        None => "handoff",
    };
    if moved.is_none() {
        // B4′: the fresh session starts from the structured brief; the
        // transcript file stays as its appendix.
        live.executor.checkpoints().arm(
            &p.task_id,
            aeroric_core::sessions::live::checkpoint::HandoffReason::SourceMove,
        );
        let messages = crate::services::parse_messages(&lines, family);
        let file = match write_handoff_file(&data, &p.task_id, &messages) {
            Ok(file) => file,
            Err(reason) => return failure(request.id.clone(), ErrorCode::IoError, &reason),
        };
        let budget = window.unwrap_or(128_000) * DIGEST_PERCENT / 100 * CHARS_PER_TOKEN;
        fields.push((
            HANDOFF_KEY,
            Some(handoff_digest(&messages, &file, budget as usize)),
        ));
    }
    let task_id = p.task_id.clone();
    let written = live.service.mutate(&project_id, |records, _now| {
        let next = records
            .iter()
            .map(|record| {
                if record.get("id").and_then(Value::as_str) != Some(task_id.as_str()) {
                    return record.clone();
                }
                let mut record = record.clone();
                for (key, value) in &fields {
                    match value {
                        Some(value) => {
                            record
                                .0
                                .insert((*key).to_string(), Value::String(value.clone()));
                        }
                        None => {
                            record.0.remove(*key);
                        }
                    }
                }
                record
            })
            .collect();
        (next, true)
    });
    if let Err(error) = written {
        return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
    }
    live.executor.close_session(&p.task_id);
    let session_result = match &moved {
        Some((session, copied)) => live.service.attach_session(
            &project_id,
            &p.task_id,
            family,
            &agent,
            Some(&format!(
                "{}{session}",
                aeroric_core::sessions::live::FORK_PREFIX
            )),
            Some(&copied.to_string_lossy()),
        ),
        // HANDOFF-001: attach with no id and no path is a no-op by contract,
        // so the old session has to be cleared on purpose — otherwise the
        // next message resumes it instead of starting from the handoff.
        None => live.service.detach_session(&project_id, &p.task_id),
    };
    if let Err(error) = session_result {
        return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
    }
    ok(
        request,
        json!({ "mode": mode, "contextUsed": used, "targetWindow": window }),
    )
}

/// Which configuration a model change lands on, and whether in place
/// (`plan_switch`). `login` is the harness slug and its 「本机登录」 gateway
/// configuration when it is on and routes the model; `picked` is the source
/// the picker offered the model under.
fn switch_route(
    login: Option<(&str, &str)>,
    current: Option<&str>,
    picked: Option<&str>,
) -> (bool, Option<String>) {
    match login {
        Some((_, id)) if current == Some(id) => (true, Some(id.to_string())),
        Some((slug, id))
            if current.is_none()
                || current.is_some_and(|c| c.starts_with(&format!("via-{slug}-"))) =>
        {
            (false, Some(id.to_string()))
        }
        _ => (
            false,
            picked.filter(|c| !c.trim().is_empty()).map(str::to_string),
        ),
    }
}

#[cfg(test)]
mod switch_route_tests {
    use super::switch_route;

    const LOGIN: Option<(&str, &str)> = Some(("codex", "via-codex-login"));

    #[test]
    fn on_the_login_configuration_every_routed_model_switches_in_place() {
        assert_eq!(
            switch_route(LOGIN, Some("via-codex-login"), Some("via-codex-all")),
            (true, Some("via-codex-login".into()))
        );
    }

    #[test]
    fn a_direct_or_third_party_session_moves_onto_the_login_once() {
        assert_eq!(
            switch_route(LOGIN, None, None),
            (false, Some("via-codex-login".into()))
        );
        assert_eq!(
            switch_route(LOGIN, Some("via-codex-all"), None),
            (false, Some("via-codex-login".into()))
        );
    }

    #[test]
    fn with_the_login_off_the_picked_source_is_kept() {
        assert_eq!(
            switch_route(None, None, Some("via-codex-all")),
            (false, Some("via-codex-all".into()))
        );
        assert_eq!(
            switch_route(None, Some("via-codex-all"), Some("")),
            (false, None)
        );
        // A user's own configuration is never pulled onto the login.
        assert_eq!(
            switch_route(LOGIN, Some("my-pool"), Some("my-pool")),
            (false, Some("my-pool".into()))
        );
    }
}

/// The tokens the session's last turn sent (what the new model must hold).
/// Falls back to the transcript size when the transcript says nothing.
fn context_used(lines: &[String], family: ProtocolFamily, path: Option<&str>) -> Option<u64> {
    use aeroric_core::sessions::transcript_stats;
    let stats = match family {
        ProtocolFamily::Codex => transcript_stats::codex(lines),
        ProtocolFamily::Claude => transcript_stats::claude(lines),
        ProtocolFamily::Omp => transcript_stats::omp(lines),
        ProtocolFamily::Dsh => transcript_stats::dsh(lines),
    };
    stats.and_then(|s| s.context_used).or_else(|| {
        let bytes = std::fs::metadata(path?).ok()?.len();
        Some(bytes / 4)
    })
}

/// The model's context window as its connection declares it; unknown for a
/// built-in login (its own models are large and the CLI compacts itself).
fn target_window(data: &std::path::Path, config_id: Option<&str>, model: &str) -> Option<u64> {
    let config_id = config_id?;
    let doc = aeroric_core::agent_connections::store::load_effective(
        &aeroric_core::agent_connections::store::document_path(data),
    )
    .ok()?;
    let config = doc.config(config_id)?;
    let (candidates, _) =
        aeroric_core::agent_connections::routing::candidates(&doc, &config, model);
    candidates.iter().find_map(|c| {
        doc.connections
            .iter()
            .find(|connection| connection.id == c.connection_id)?
            .routable_models()
            .find(|m| m.routing_name() == model)?
            .context_window
    })
}

/// Where a source keeps this harness's sessions: a configuration's managed
/// home, or the CLI's own (`CLAUDE_CONFIG_DIR` / `CODEX_HOME`, else `~/.claude`
/// / `~/.codex`).
fn home_of(
    data: &std::path::Path,
    config_id: Option<&str>,
    family: ProtocolFamily,
) -> Option<std::path::PathBuf> {
    if let Some(id) = config_id {
        let config = aeroric_core::agent_connections::launch::resolve_config(data, id).ok()?;
        return Some(data.join("agent-homes").join(config.home_dir_name));
    }
    let (variable, folder) = match family {
        ProtocolFamily::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
        ProtocolFamily::Codex => ("CODEX_HOME", ".codex"),
        _ => return None,
    };
    std::env::var_os(variable)
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(folder))
        })
}

/// Copies the transcript into `to` where that CLI looks for it (Claude:
/// `projects/<cwd>/<id>.jsonl`; Codex: `sessions/<y>/<m>/<d>/rollout-….jsonl`).
/// The original stays where it is. `None` when the layout is not recognised.
fn copy_transcript(
    path: &std::path::Path,
    from: &std::path::Path,
    to: &std::path::Path,
    family: ProtocolFamily,
) -> Option<std::path::PathBuf> {
    let relative = match path.strip_prefix(from) {
        Ok(relative) => relative.to_path_buf(),
        Err(_) => {
            let parts: Vec<_> = path.components().rev().collect();
            let keep = match family {
                ProtocolFamily::Claude => 2,
                _ => 4,
            };
            let root = match family {
                ProtocolFamily::Claude => "projects",
                _ => "sessions",
            };
            let mut relative = std::path::PathBuf::from(root);
            for part in parts.iter().take(keep).rev() {
                relative.push(part.as_os_str());
            }
            relative
        }
    };
    let target = to.join(&relative);
    if target == path {
        return Some(target);
    }
    if !target.exists() {
        std::fs::create_dir_all(target.parent()?).ok()?;
        std::fs::copy(path, &target).ok()?;
    }
    Some(target)
}

/// The whole conversation as Markdown in Aeroric's data directory (never the
/// user's repository): every message, tool call and result, untrimmed.
fn write_handoff_file(
    data: &std::path::Path,
    task_id: &str,
    messages: &[aeroric_core::sessions::SessionMessage],
) -> Result<std::path::PathBuf, String> {
    let dir = data.join("handoffs");
    aeroric_core::settings::ensure_private_dir(&dir)?;
    let file = dir.join(format!("{task_id}-{}.md", now_ms()));
    aeroric_core::settings::atomic_write_private(&file, &render_transcript(messages))?;
    Ok(file)
}

fn render_transcript(messages: &[aeroric_core::sessions::SessionMessage]) -> String {
    use aeroric_core::sessions::SessionContent;
    let mut out = String::from("# 上一段会话的完整记录\n\n");
    for message in messages {
        let who = if message.role == "user" {
            "用户"
        } else {
            "Agent"
        };
        for block in &message.content {
            match block {
                SessionContent::Text { text } => out.push_str(&format!("## {who}\n\n{text}\n\n")),
                SessionContent::ToolUse { name, input, .. } => {
                    out.push_str(&format!("### 工具调用：{name}\n\n```\n{input}\n```\n\n"))
                }
                SessionContent::ToolResult { output, .. } => {
                    out.push_str(&format!("### 工具结果\n\n```\n{output}\n```\n\n"))
                }
                _ => {}
            }
        }
    }
    out
}

/// The handoff's inline part: where the full record is, then the most recent
/// conversation text (newest kept first) within `budget` characters.
fn handoff_digest(
    messages: &[aeroric_core::sessions::SessionMessage],
    file: &std::path::Path,
    budget: usize,
) -> String {
    use aeroric_core::sessions::SessionContent;
    let header = format!(
        "（Aeroric 交接）你接手的是另一个模型做到一半的任务。完整的对话记录（每条消息、工具调用与结果，未删减）在：\n{}\n需要细节时先读这个文件。下面是最近的对话：\n\n",
        file.display()
    );
    let mut recent: Vec<String> = Vec::new();
    let mut used = header.chars().count();
    for message in messages.iter().rev() {
        let who = if message.role == "user" {
            "用户"
        } else {
            "Agent"
        };
        let text: String = message
            .content
            .iter()
            .filter_map(|b| match b {
                SessionContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            continue;
        }
        let entry = format!("【{who}】{text}\n\n");
        let size = entry.chars().count();
        if used + size > budget {
            break;
        }
        used += size;
        recent.push(entry);
    }
    recent.reverse();
    format!(
        "{header}{}---\n接下来的消息是用户的新要求：\n\n",
        recent.concat()
    )
}

/// After a `handoff` move: the first message starts a fresh run with the
/// handoff in front, then the handoff is cleared. `None` when there is none.
fn start_with_handoff(live: &Live, request: &Request, task_id: &str, text: &str) -> Option<Frame> {
    let (project, record) = live.record(task_id)?;
    let handoff = record.get(HANDOFF_KEY).and_then(Value::as_str)?.to_string();
    let project_id = project.get("id").and_then(Value::as_str)?.to_string();
    let task = task_id.to_string();
    let _ = live.service.mutate(&project_id, |records, _now| {
        let next = records
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if r.get("id").and_then(Value::as_str) == Some(task.as_str()) {
                    r.0.remove(HANDOFF_KEY);
                }
                r
            })
            .collect();
        (next, true)
    });
    let operation_key = format!("session-handoff-{task_id}-{}", now_ms());
    Some(
        match live.service.start(
            &project,
            task_id,
            &operation_key,
            true,
            Some(format!("{handoff}{text}")),
            false,
            Default::default(),
            Default::default(),
        ) {
            Ok(_) => ok(request, json!({"accepted": true})),
            Err(error) => failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &error.to_string(),
            ),
        },
    )
}

#[cfg(test)]
mod move_tests {
    use super::*;
    use aeroric_core::sessions::{SessionContent, SessionMessage};

    fn message(role: &str, text: &str) -> SessionMessage {
        SessionMessage {
            role: role.into(),
            content: vec![SessionContent::Text { text: text.into() }],
            message_id: None,
        }
    }

    /// A window smaller than the conversation still gets a handoff that fits
    /// (1M → 200K, 2026-10-29): the newest turns inline, the rest in the file.
    #[test]
    fn the_digest_fits_a_small_window_and_keeps_the_newest_turns() {
        let mut messages = Vec::new();
        for i in 0..200 {
            messages.push(message("user", &format!("问题 {i} {}", "长".repeat(400))));
            messages.push(message(
                "assistant",
                &format!("回答 {i} {}", "长".repeat(400)),
            ));
        }
        let budget = 2_000;
        let digest = handoff_digest(&messages, std::path::Path::new("/tmp/h.md"), budget);
        assert!(
            digest.chars().count() <= budget + 40,
            "{}",
            digest.chars().count()
        );
        assert!(digest.contains("回答 199"), "the newest turn is kept");
        assert!(!digest.contains("问题 0 "), "the oldest falls to the file");
        assert!(digest.contains("/tmp/h.md"));
        let full = render_transcript(&messages);
        assert!(
            full.contains("问题 0 ") && full.contains("回答 199"),
            "the file has everything"
        );
    }

    /// Claude transcripts land under `projects/<cwd>/`, Codex rollouts keep
    /// their dated folders; the original is left in place.
    #[test]
    fn a_transcript_is_copied_where_the_target_cli_looks() {
        let root = std::env::temp_dir().join(format!("aeroric-move-{}", now_ms()));
        let from = root.join("a");
        let to = root.join("b");
        let source = from.join("projects/-w/abc.jsonl");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "{}\n").unwrap();
        let copied = copy_transcript(&source, &from, &to, ProtocolFamily::Claude).unwrap();
        assert_eq!(copied, to.join("projects/-w/abc.jsonl"));
        assert!(copied.exists() && source.exists());
        let rollout = root.join("x/sessions/2026/10/29/rollout-1.jsonl");
        std::fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        std::fs::write(&rollout, "{}\n").unwrap();
        let copied = copy_transcript(&rollout, &from, &to, ProtocolFamily::Codex).unwrap();
        assert_eq!(copied, to.join("sessions/2026/10/29/rollout-1.jsonl"));
        std::fs::remove_dir_all(root).ok();
    }
}
