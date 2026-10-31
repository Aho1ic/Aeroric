//! B5-1 long-term memory (2026-10-31), the host's side: the `memory.*`
//! methods the app calls and the `context.memory*` methods the context
//! server calls for an agent. The user's words take effect at once; an
//! agent only proposes, and only in a project that uses memory (Q-M5,
//! R2-1). The store is core's (`aeroric_core::memory`).

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::memory::{self, MemoryError};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::memory::{
    self as wire, MemoryCategory, MemoryDraft, MemoryItem, MemoryKind, MemorySettings,
};
use serde_json::{json, Value};

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const CONTEXT_MEMORY_RECALL_METHOD: &str = "context.memoryRecall";
pub const CONTEXT_MEMORY_PROPOSE_METHOD: &str = "context.memoryPropose";
pub const CONTEXT_MEMORY_ENABLED_METHOD: &str = "context.memoryEnabled";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    if let Err(error) = live
        .executor
        .memory()
        .set_dir(live.context.paths.data_dir().join("memory"))
    {
        eprintln!("[aeroric-host] memory store unavailable: {error}");
    }
    set.register(wire::LIST_METHOD, blocking(live, memory_list));
    set.register(wire::REMEMBER_METHOD, blocking(live, memory_remember));
    set.register(wire::CONFIRM_METHOD, blocking(live, memory_confirm));
    set.register(wire::ARCHIVE_METHOD, blocking(live, memory_archive));
    set.register(wire::PIN_METHOD, blocking(live, memory_pin));
    set.register(wire::MOVE_METHOD, blocking(live, memory_move));
    set.register(wire::UNDO_METHOD, blocking(live, memory_undo));
    set.register(wire::FORGET_METHOD, blocking(live, memory_forget));
    set.register(wire::ATTENTION_METHOD, blocking(live, memory_attention));
    set.register(wire::PROJECT_METHOD, blocking(live, memory_project));
    set.register(wire::NATIVE_METHOD, blocking(live, memory_native));
    set.register(wire::PROFILE_METHOD, blocking(live, memory_profile));
    set.register(CONTEXT_MEMORY_RECALL_METHOD, blocking(live, context_recall));
    set.register(
        CONTEXT_MEMORY_PROPOSE_METHOD,
        blocking(live, context_propose),
    );
    set.register(
        CONTEXT_MEMORY_ENABLED_METHOD,
        blocking(live, context_enabled),
    );
    set.register(wire::SETTINGS_METHOD, blocking(live, memory_settings));
    set.register(wire::RUNS_METHOD, blocking(live, memory_runs));
    set.register(wire::MODELS_METHOD, blocking(live, memory_models));
    set.register(wire::EXTRACT_METHOD, blocking(live, memory_extract));
    set.register(
        wire::IMPORT_NATIVE_METHOD,
        blocking(live, memory_import_native),
    );
    set.register("memory.sweep", blocking(live, memory_sweep));
    set.register(wire::FEEDBACK_METHOD, blocking(live, memory_feedback));
    set.register("context.memoryFeedback", blocking(live, context_feedback));
    start_sweeper(live);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn memory_failure(request: &Request, error: MemoryError) -> Frame {
    let code = match error {
        MemoryError::UnknownItem => ErrorCode::NotFound,
        MemoryError::Unavailable(_) | MemoryError::Storage(_) => ErrorCode::IoError,
        _ => ErrorCode::InvalidArgument,
    };
    failure(request.id.clone(), code, &error.to_string())
}

fn answer<T: serde::Serialize>(
    request: &Request,
    result: Result<T, MemoryError>,
    key: &str,
) -> Frame {
    match result {
        Ok(value) => ok(request, json!({ key: value })),
        Err(error) => memory_failure(request, error),
    }
}

/// Who said it, from the task's record: its harness and model.
fn source_of(live: &Live, task_id: Option<&str>) -> (Option<String>, Option<String>) {
    let Some((_, record)) = task_id.and_then(|id| live.record(id)) else {
        return (None, None);
    };
    let text = |key: &str| {
        record
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    (text("agent"), text("selectedModel"))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListParams {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

/// `memory.list {projectId?, query?}`: a scope's items (every state) and
/// whether the project uses memory.
fn memory_list(live: &Live, request: &Request) -> Frame {
    let p: ListParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    match memory.list(p.project_id.as_deref(), p.query.as_deref()) {
        Ok(items) => ok(
            request,
            json!({
                "items": items,
                "enabled": p.project_id.as_deref().map(|id| memory.project_enabled(id)),
            }),
        ),
        Err(error) => memory_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RememberParams {
    draft: MemoryDraft,
    /// The session it was said in (`/记住`).
    #[serde(default)]
    task_id: Option<String>,
    /// The words as typed, for the evidence (defaults to the draft's text).
    #[serde(default)]
    source: Option<String>,
}

/// `memory.remember {draft, taskId?, source?}`: the user keeps something.
/// Without a kind hint `/记住` sends `fact`; the host does not guess.
fn memory_remember(live: &Live, request: &Request) -> Frame {
    let p: RememberParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let source = p.source.clone().unwrap_or_else(|| p.draft.text.clone());
    let at = now_ms();
    let evidence = memory::evidence(p.task_id.as_deref(), Some("user"), None, &source, at);
    answer(
        request,
        live.executor.memory().remember(p.draft, evidence, at),
        "item",
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdParams {
    id: String,
    #[serde(default)]
    accept: Option<bool>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    project_id: Option<String>,
}

fn id_params(request: &Request) -> Result<IdParams, Box<Frame>> {
    params(request)
}

/// `memory.confirm {id, accept}`: accept a proposal, or reject (forget) it.
fn memory_confirm(live: &Live, request: &Request) -> Frame {
    let p = match id_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let result = live
        .executor
        .memory()
        .confirm(&p.id, p.accept.unwrap_or(true), now_ms());
    answer(request, result, "item")
}

/// `memory.archive {id, archived}`.
fn memory_archive(live: &Live, request: &Request) -> Frame {
    let p = match id_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let result = live
        .executor
        .memory()
        .archive(&p.id, p.archived.unwrap_or(true), now_ms());
    answer(request, result, "item")
}

/// `memory.pin {id, pinned}`.
fn memory_pin(live: &Live, request: &Request) -> Frame {
    let p = match id_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let result = live
        .executor
        .memory()
        .pin(&p.id, p.pinned.unwrap_or(true), now_ms());
    answer(request, result, "item")
}

/// `memory.move {id, projectId?}`: 改为全局 (no project) or into one.
fn memory_move(live: &Live, request: &Request) -> Frame {
    let p = match id_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let result = live
        .executor
        .memory()
        .move_to(&p.id, p.project_id.as_deref(), now_ms());
    answer(request, result, "item")
}

/// `memory.undo {id}`: take back a `/记住` (no forget signature).
fn memory_undo(live: &Live, request: &Request) -> Frame {
    let p = match id_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(request, live.executor.memory().undo(&p.id), "undone")
}

/// `memory.forget {id}`: 永久遗忘 the item and its older versions.
fn memory_forget(live: &Live, request: &Request) -> Frame {
    let p = match id_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor.memory().forget(&p.id, now_ms()),
        "removed",
    )
}

/// `memory.attention {}`: proposals waiting for the user.
fn memory_attention(live: &Live, request: &Request) -> Frame {
    answer(request, live.executor.memory().attention(), "items")
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectParams {
    project_id: String,
    #[serde(default)]
    enabled: Option<bool>,
}

/// `memory.project {projectId, enabled?}`: read or set "这个项目用记忆".
fn memory_project(live: &Live, request: &Request) -> Frame {
    let p: ProjectParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    if let Some(enabled) = p.enabled {
        if let Err(error) = memory.set_project_enabled(&p.project_id, enabled) {
            return memory_failure(request, error);
        }
    }
    ok(
        request,
        json!({ "enabled": memory.project_enabled(&p.project_id) }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScopeParams {
    #[serde(default)]
    project_id: Option<String>,
}

/// A local project's folder.
fn project_dir(live: &Live, project_id: &str) -> Option<PathBuf> {
    let projects = aeroric_core::projects::ProjectService::new(Arc::clone(&live.context))
        .list()
        .ok()?;
    let project = projects
        .iter()
        .find(|p| p.get("id").and_then(Value::as_str) == Some(project_id))?;
    let path = PathBuf::from(project.get("path").and_then(Value::as_str)?);
    path.is_dir().then_some(path)
}

/// The harness homes to look in: the CLIs' own and Aeroric's managed ones.
fn homes(live: &Live) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    for (harness, variable, folder) in [
        ("claude", "CLAUDE_CONFIG_DIR", ".claude"),
        ("codex", "CODEX_HOME", ".codex"),
    ] {
        if let Some(dir) = std::env::var_os(variable)
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(folder)))
        {
            out.push((harness.to_string(), dir));
        }
    }
    if let Ok(entries) = std::fs::read_dir(live.context.paths.data_dir().join("agent-homes")) {
        for entry in entries.flatten() {
            let dir = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if dir.join("projects").is_dir() {
                out.push((format!("claude（{name}）"), dir));
            } else if dir.join("memories").is_dir() || dir.join("AGENTS.md").is_file() {
                out.push((format!("codex（{name}）"), dir));
            }
        }
    }
    out
}

/// `memory.native {projectId?}`: the harnesses' own memory files, read-only.
fn memory_native(live: &Live, request: &Request) -> Frame {
    let p: ScopeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let cwd = p.project_id.as_deref().and_then(|id| project_dir(live, id));
    ok(
        request,
        json!({ "files": memory::native::discover(&homes(live), cwd.as_deref()) }),
    )
}

/// `memory.profile {projectId?}`: what a session starting there is handed.
fn memory_profile(live: &Live, request: &Request) -> Frame {
    let p: ScopeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    ok(
        request,
        json!({ "text": live.executor.memory().profile(p.project_id.as_deref()) }),
    )
}

// MARK: context server

/// The token's task and project, when that project uses memory.
fn token_scope(
    live: &Live,
    request: &Request,
    token: &str,
) -> Result<(String, String), Box<Frame>> {
    let (task, project) = live
        .executor
        .context_tokens()
        .resolve(token)
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::Unauthenticated,
                "上下文令牌无效或会话已结束",
            ))
        })?;
    if !live.executor.memory().project_enabled(&project) {
        return Err(Box::new(failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "这个项目没有开启记忆",
        )));
    }
    Ok((task, project))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TokenOnly {
    token: String,
}

/// `context.memoryEnabled {token}`: whether to offer the memory tools.
fn context_enabled(live: &Live, request: &Request) -> Frame {
    let p: TokenOnly = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let enabled = live
        .executor
        .context_tokens()
        .resolve(&p.token)
        .is_some_and(|(_, project)| live.executor.memory().project_enabled(&project));
    ok(request, json!({ "enabled": enabled }))
}

fn line(item: &MemoryItem) -> String {
    let scope = match (item.kind, item.project_id.is_some()) {
        (MemoryKind::SessionFact, _) => "会话线索",
        (_, true) => "本项目",
        _ => "全局",
    };
    let who = match item.assertion {
        wire::Assertion::UserExplicit => "用户说的",
        wire::Assertion::AgentConfirmed => "Agent 提议、用户确认",
        wire::Assertion::ToolVerified => "工具核实",
        wire::Assertion::ModelInferred => "模型推断",
    };
    let day = item.updated_ms / 86_400_000;
    let s = item.stats;
    let Some(detail) = &item.experience else {
        return format!(
            "- [{}] {}（{scope}，{who}，第 {day} 天，检索 {} 次，有用 {} 次）",
            item.id, item.text, s.retrieved, s.helpful
        );
    };
    // B5-3a: an experience shows its fix, where it holds, and its trust.
    let level = match item.confidence {
        Some(wire::Confidence::High) => "高",
        Some(wire::Confidence::Medium) => "中",
        _ => "低",
    };
    let mut out = format!(
        "- [{}] {}（经验·置信{level}，检索 {} 次，有用 {} 次，不对 {} 次）\n  解法：{}",
        item.id, item.text, s.retrieved, s.helpful, s.wrong, detail.solution
    );
    if !detail.envs.is_empty() {
        out.push_str(&format!("；适用：{}", detail.envs.join("、")));
    }
    out
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecallParams {
    token: String,
    query: String,
}

/// `context.memoryRecall {token, query}`: matching items with id, source
/// and date; says so plainly when nothing matches.
fn context_recall(live: &Live, request: &Request) -> Frame {
    let p: RecallParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (_, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    match live.executor.memory().recall(&project, &p.query) {
        Ok(items) => {
            let text = if items.is_empty() {
                "没有找到相关记忆。".to_string()
            } else {
                let lines: Vec<String> = items.iter().map(line).collect();
                format!(
                    "找到 {} 条记忆：\n{}\n用过之后可以用 memory_feedback 告诉 Aeroric 它有没有用。",
                    items.len(),
                    lines.join("\n")
                )
            };
            ok(request, json!({ "text": text, "items": items }))
        }
        Err(error) => memory_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposeParams {
    token: String,
    text: String,
    #[serde(default)]
    kind: Option<MemoryKind>,
    #[serde(default)]
    category: Option<MemoryCategory>,
    /// `false`: the user in general, not just this project.
    #[serde(default)]
    project: Option<bool>,
    /// What the user said that this rests on.
    #[serde(default)]
    evidence: Option<String>,
}

/// `context.memoryPropose {token, text, kind?, category?, project?, evidence?}`:
/// pending until the user confirms it.
fn context_propose(live: &Live, request: &Request) -> Frame {
    let p: ProposeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (task, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let kind = p.kind.unwrap_or(MemoryKind::Fact);
    let draft = MemoryDraft {
        project_id: p.project.unwrap_or(true).then_some(project),
        kind,
        category: p.category.filter(|_| kind == MemoryKind::Profile),
        text: p.text.clone(),
        supersedes: None,
    };
    let (harness, model) = source_of(live, Some(&task));
    let at = now_ms();
    let evidence = memory::evidence(
        Some(&task),
        harness.as_deref(),
        model.as_deref(),
        p.evidence.as_deref().unwrap_or(&p.text),
        at,
    );
    match live.executor.memory().propose(draft, evidence, at) {
        Ok(item) => {
            let text = if item.state == wire::MemoryState::Pending {
                format!("已提议，等用户确认：[{}] {}", item.id, item.text)
            } else {
                format!("这条已经在记忆里了：[{}] {}", item.id, item.text)
            };
            ok(request, json!({ "text": text, "item": item }))
        }
        Err(error) => memory_failure(request, error),
    }
}

// MARK: B5-2 extraction

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsParams {
    #[serde(default)]
    settings: Option<MemorySettings>,
}

/// `memory.settings {settings?}`: read, or save, the 整理设置. Turning
/// `auto` on needs a model and starts the clock: sessions finished before
/// it are left alone (no surprise bill for old history).
fn memory_settings(live: &Live, request: &Request) -> Frame {
    let p: SettingsParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    let old = memory.settings();
    if let Some(mut next) = p.settings {
        if next.auto && (next.connection_id.is_none() || next.model.is_none()) {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "先选一个分析模型，再打开自动整理",
            );
        }
        next.daily_limit = next.daily_limit.clamp(1, 500);
        next.enabled_ms = if next.auto && !old.auto {
            now_ms()
        } else {
            old.enabled_ms
        };
        if let Err(error) = memory.set_settings(&next) {
            return memory_failure(request, error);
        }
    }
    ok(request, json!({ "settings": memory.settings() }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunsParams {
    #[serde(default)]
    limit: Option<usize>,
}

/// `memory.runs {limit?}`: 整理记录, today's count and this month's total.
fn memory_runs(live: &Live, request: &Request) -> Frame {
    let p: RunsParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    let runs = match memory.runs(p.limit.unwrap_or(100).min(500)) {
        Ok(runs) => runs,
        Err(error) => return memory_failure(request, error),
    };
    let now = now_ms();
    let month_start = {
        use chrono::{Datelike, Local, TimeZone};
        Local
            .timestamp_millis_opt(now as i64)
            .single()
            .and_then(|t| {
                Local
                    .with_ymd_and_hms(t.year(), t.month(), 1, 0, 0, 0)
                    .earliest()
            })
            .map(|t| t.timestamp_millis() as u64)
            .unwrap_or(0)
    };
    let all = memory.runs(2000).unwrap_or_default();
    let month: Vec<_> = all
        .iter()
        .filter(|r| r.at_ms >= month_start && r.state != wire::MemoryRunState::Skipped)
        .collect();
    // `sum()` of no f64 is -0.0 (CU F3: shown as $-0.0000).
    let cost: f64 = month
        .iter()
        .filter_map(|r| r.cost_usd)
        .fold(0.0, |a, b| a + b);
    ok(
        request,
        json!({
            "runs": runs,
            "usedToday": memory.counted_since(memory::run::day_start_ms(now)),
            "dailyLimit": memory.settings().daily_limit,
            "monthCount": month.len(),
            "monthCostUsd": cost,
        }),
    )
}

/// `memory.models {}`: what can be the analysis model — enabled models of
/// enabled connections that speak Chat Completions.
fn memory_models(live: &Live, request: &Request) -> Frame {
    use aeroric_core::agent_connections::{model::WireProtocol, store};
    let doc = match store::load(&store::document_path(live.context.paths.data_dir())) {
        Ok(doc) => doc,
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
    };
    let mut models = Vec::new();
    for connection in doc.connections.iter().filter(|c| c.enabled) {
        if !connection
            .protocols
            .contains(&WireProtocol::ChatCompletions)
        {
            continue;
        }
        for entry in connection.routable_models() {
            models.push(json!({
                "connectionId": connection.id,
                "connection": connection.label,
                "model": entry.upstream_id,
            }));
        }
    }
    ok(request, json!({ "models": models }))
}

/// Extract one session: the conversation from its transcript, the call over
/// the chosen connection with its stored key.
fn extract_task(live: &Live, task_id: &str, forced: bool) -> Result<wire::MemoryRun, String> {
    use aeroric_core::agent_connections::store;
    let (project, record) = live.record(task_id).ok_or("没有这个会话")?;
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let title = super::context_title(&record);
    let messages = super::context_messages(live, &record);
    // B5-3a: its acceptance criteria all passed — experiences start at 中.
    let verified = live.executor.checkpoints().get(task_id).is_some_and(|cp| {
        !cp.obligations.is_empty()
            && cp
                .obligations
                .iter()
                .all(|o| o.state == aeroric_protocol::checkpoint::ObligationState::Passed)
    });
    let memory = live.executor.memory();
    let settings = memory.settings();
    let data = live.context.paths.data_dir().to_path_buf();
    let secrets = Arc::clone(&live.context.secrets);
    let call = move |system: &str, user: &str| {
        let id = settings.connection_id.clone().ok_or("没有选连接")?;
        let model = settings.model.clone().ok_or("没有选模型")?;
        let doc = store::load(&store::document_path(&data))?;
        let connection = doc
            .connections
            .into_iter()
            .find(|c| c.id == id && c.enabled)
            .ok_or("分析模型的连接不存在或已停用")?;
        let secret = store::get_credential(secrets.as_ref(), &id)?.ok_or("这个连接没有保存密钥")?;
        tokio::runtime::Handle::current().block_on(aeroric_core::settings::model_detect::chat_once(
            &connection.base_url,
            &secret,
            connection.use_global_proxy,
            &model,
            system,
            user,
            1200,
        ))
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    memory.extract_session(
        memory::run::Session {
            task_id,
            project_id: &project_id,
            title: &title,
            messages: &messages,
            forced,
            verified,
        },
        home.as_deref(),
        call,
        now_ms(),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExtractParams {
    task_id: String,
}

/// `memory.extract {taskId}`: 整理这次会话 / 重新整理 / 重试.
fn memory_extract(live: &Live, request: &Request) -> Frame {
    let p: ExtractParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match extract_task(live, &p.task_id, true) {
        Ok(run) => ok(request, json!({ "run": run })),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

/// The automatic pass (Q-M10, Q-M15): local native sessions of projects
/// that use memory, done and quiet for `SETTLED_AFTER_MS` or archived,
/// finished after auto was turned on, not extracted yet. Stops at the
/// day's limit.
fn sweep(live: &Live) -> usize {
    let memory = live.executor.memory();
    let settings = memory.settings();
    if !settings.auto || settings.model.is_none() {
        return 0;
    }
    let now = now_ms();
    let Ok(projects) =
        aeroric_core::projects::ProjectService::new(Arc::clone(&live.context)).list()
    else {
        return 0;
    };
    let mut done = 0;
    for project in projects {
        let Some(project_id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !memory.project_enabled(project_id) {
            continue;
        }
        let Ok(document) = live.service.load(project_id) else {
            continue;
        };
        for record in document.records {
            let Some(id) = record.get("id").and_then(Value::as_str) else {
                continue;
            };
            if memory.extracted(id).is_some() || !live.structured(&record, &project) {
                continue;
            }
            let number = |key: &str| record.get(key).and_then(Value::as_u64);
            let status = record.get("status").and_then(Value::as_str);
            let finished = match (status, number("completedAt"), number("archivedAt")) {
                (_, _, Some(archived)) => archived >= settings.enabled_ms,
                (Some("done"), Some(completed), None) => {
                    completed >= settings.enabled_ms
                        && now.saturating_sub(completed) >= wire::SETTLED_AFTER_MS
                }
                _ => false,
            };
            if !finished {
                continue;
            }
            match extract_task(live, id, false) {
                Ok(_) => done += 1,
                Err(error) if error.contains("到上限") || error.contains("还没有选") => {
                    return done
                }
                Err(_) => {}
            }
        }
    }
    done
}

/// `memory.sweep {}`: run the automatic pass now (the host runs it every
/// two minutes on its own).
fn memory_sweep(live: &Live, request: &Request) -> Frame {
    ok(request, json!({ "extracted": sweep(live) }))
}

pub(super) fn start_sweeper(live: &Arc<Live>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let weak = Arc::downgrade(live);
    handle.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            let Some(live) = weak.upgrade() else {
                break;
            };
            let _ = tokio::task::spawn_blocking(move || sweep(&live)).await;
        }
    });
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportParams {
    path: String,
    #[serde(default)]
    project_id: Option<String>,
}

/// `memory.importNative {path, projectId?}` (Q-M14): a file `memory.native`
/// lists, line by line into pending candidates — a project's file as its
/// facts, the user's own as their profile. No model.
fn memory_import_native(live: &Live, request: &Request) -> Frame {
    let p: ImportParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let cwd = p.project_id.as_deref().and_then(|id| project_dir(live, id));
    let files = memory::native::discover(&homes(live), cwd.as_deref());
    let Some(file) = files.iter().find(|f| f.path == p.path) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "只能导入记忆页列出的记忆文件",
        );
    };
    let Ok(text) = std::fs::read_to_string(&file.path) else {
        return failure(request.id.clone(), ErrorCode::IoError, "读不到这个文件");
    };
    let memory = live.executor.memory();
    let (mut imported, mut skipped) = (0, 0);
    let now = now_ms();
    for line in memory::native::import_lines(&text) {
        let draft = MemoryDraft {
            project_id: p.project_id.clone(),
            kind: if p.project_id.is_some() {
                MemoryKind::Fact
            } else {
                MemoryKind::Profile
            },
            category: None,
            text: line.clone(),
            supersedes: None,
        };
        let proof = memory::evidence(None, Some(&file.harness), None, &line, now);
        match memory.propose(draft, proof, now) {
            Ok(item) if item.state == wire::MemoryState::Pending && item.created_ms == now => {
                imported += 1
            }
            _ => skipped += 1,
        }
    }
    ok(request, json!({ "imported": imported, "skipped": skipped }))
}

// MARK: B5-3a feedback

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FeedbackParams {
    id: String,
    helpful: bool,
}

/// `memory.feedback {id, helpful}`: 有用 / 不对 from the memory page.
fn memory_feedback(live: &Live, request: &Request) -> Frame {
    let p: FeedbackParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor.memory().feedback(&p.id, p.helpful, now_ms()),
        "item",
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextFeedbackParams {
    token: String,
    id: String,
    helpful: bool,
}

/// `context.memoryFeedback {token, id, helpful}`: an agent says whether a
/// recalled item helped — only for this project's items or global ones.
fn context_feedback(live: &Live, request: &Request) -> Frame {
    let p: ContextFeedbackParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (_, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    match memory.item(&p.id) {
        Ok(Some(item)) if item.project_id.as_deref().is_none_or(|id| id == project) => {}
        Ok(_) => return memory_failure(request, MemoryError::UnknownItem),
        Err(error) => return memory_failure(request, error),
    }
    match memory.feedback(&p.id, p.helpful, now_ms()) {
        Ok(item) => ok(
            request,
            json!({
                "text": format!("已记下：[{}] {}", item.id, if p.helpful { "有用" } else { "不对" }),
                "item": item,
            }),
        ),
        Err(error) => memory_failure(request, error),
    }
}
