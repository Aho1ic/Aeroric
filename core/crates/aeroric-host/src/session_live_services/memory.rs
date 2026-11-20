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
    set.register(
        wire::CONFIRM_MANY_METHOD,
        blocking(live, memory_confirm_many),
    );
    set.register(wire::ARCHIVE_METHOD, blocking(live, memory_archive));
    set.register(wire::PIN_METHOD, blocking(live, memory_pin));
    set.register(wire::MOVE_METHOD, blocking(live, memory_move));
    set.register(wire::UNDO_METHOD, blocking(live, memory_undo));
    set.register(wire::FORGET_METHOD, blocking(live, memory_forget));
    set.register(wire::ATTENTION_METHOD, blocking(live, memory_attention));
    set.register(wire::PROJECT_METHOD, blocking(live, memory_project));
    set.register(wire::NATIVE_METHOD, blocking(live, memory_native));
    set.register(
        wire::EVIDENCE_LOCATE_METHOD,
        blocking(live, memory_evidence_locate),
    );
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
    set.register(
        wire::HISTORY_PLAN_METHOD,
        blocking(live, memory_history_plan),
    );
    set.register(
        wire::EXTRACT_IMPORTED_METHOD,
        blocking(live, memory_extract_imported),
    );
    set.register(wire::EXTRACT_METHOD, blocking(live, memory_extract));
    set.register(wire::RUN_CANCEL_METHOD, blocking(live, memory_run_cancel));
    set.register(wire::RUN_RETRY_METHOD, blocking(live, memory_run_retry));
    set.register(wire::REINDEX_METHOD, blocking(live, memory_reindex));
    set.register(
        wire::IMPORT_NATIVE_METHOD,
        blocking(live, memory_import_native),
    );
    set.register("memory.sweep", blocking(live, memory_sweep));
    set.register(wire::FEEDBACK_METHOD, blocking(live, memory_feedback));
    set.register(wire::PROPOSALS_METHOD, blocking(live, skill_proposals));
    set.register(wire::PROPOSAL_EDIT_METHOD, blocking(live, skill_edit));
    set.register(wire::PROPOSAL_DECIDE_METHOD, blocking(live, skill_decide));
    set.register(wire::SKILL_VERSION_METHOD, blocking(live, skill_version));
    set.register(wire::SKILL_DISABLE_METHOD, blocking(live, skill_disable));
    set.register("context.memoryFeedback", blocking(live, context_feedback));
    set.register(wire::DREAM_METHOD, blocking(live, memory_dream));
    set.register(
        wire::DREAM_PROPOSALS_METHOD,
        blocking(live, memory_dream_proposals),
    );
    set.register(
        wire::DREAM_DECIDE_METHOD,
        blocking(live, memory_dream_decide),
    );
    set.register(wire::DREAM_UNDO_METHOD, blocking(live, memory_dream_undo));
    set.register(
        wire::DREAM_REVERT_METHOD,
        blocking(live, memory_dream_revert),
    );
    set.register(
        wire::DREAM_PENDING_METHOD,
        blocking(live, memory_dream_pending),
    );
    set.register(wire::CHAIN_METHOD, blocking(live, memory_chain));
    set.register(
        wire::SEMANTIC_STATUS_METHOD,
        blocking(live, memory_semantic_status),
    );
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManyParams {
    ids: Vec<String>,
    accept: bool,
}

/// `memory.confirmMany {ids, accept}`: 全部确认 / 全部拒绝.
fn memory_confirm_many(live: &Live, request: &Request) -> Frame {
    let p: ManyParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let result = live
        .executor
        .memory()
        .confirm_many(&p.ids, p.accept, now_ms());
    answer(request, result, "decided")
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
    // C2: what archiving touched beyond the item.
    match result {
        Ok((item, mut withdrawal)) => {
            reach_wiki(live, &mut withdrawal, false);
            ok(request, json!({ "item": item, "withdrawal": withdrawal }))
        }
        Err(error) => memory_failure(request, error),
    }
}

/// C2 second step: the Wiki pages of the withdrawn experiences' projects
/// (every project's, for a global one) lose their paragraphs now; a
/// forgotten one's words leave the history too. A project whose Wiki could
/// not be changed goes to the backlog (ids only), tried again on every
/// withdrawal and every 需要我处理 read, and is named in `failed`.
fn reach_wiki(live: &Live, withdrawal: &mut wire::Withdrawal, forgotten: bool) {
    let memory = live.executor.memory();
    let mut work: Vec<memory::WikiBacklog> = memory.wiki_backlog();
    let now = now_ms();
    if !withdrawal.experiences.is_empty() {
        let mut all: Option<Vec<String>> = None;
        let mut by_project: Vec<(String, Vec<String>)> = Vec::new();
        for (id, project) in &withdrawal.experiences {
            let projects = match project {
                Some(project) => vec![project.clone()],
                None => all.get_or_insert_with(|| project_ids(live)).clone(),
            };
            for project in projects {
                match by_project.iter_mut().find(|(p, _)| *p == project) {
                    Some((_, ids)) => ids.push(id.clone()),
                    None => by_project.push((project, vec![id.clone()])),
                }
            }
        }
        work.extend(
            by_project
                .into_iter()
                .map(|(project_id, ids)| memory::WikiBacklog {
                    project_id,
                    ids,
                    forgotten,
                    reason: String::new(),
                    at_ms: now,
                }),
        );
    }
    if work.is_empty() {
        return;
    }
    let (wiki_pages, wiki_flagged, failed, left) = run_backlog(live, work);
    withdrawal.wiki_pages += wiki_pages;
    withdrawal.wiki_flagged += wiki_flagged;
    withdrawal.failed.extend(failed);
    let _ = memory.set_wiki_backlog(&left);
}

/// Every project, by id.
fn project_ids(live: &Live) -> Vec<String> {
    aeroric_core::projects::ProjectService::new(Arc::clone(&live.context))
        .list()
        .unwrap_or_default()
        .iter()
        .filter_map(|p| p.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// Try each entry: `(pages, flagged, failures in words, entries left)`.
fn run_backlog(
    live: &Live,
    work: Vec<memory::WikiBacklog>,
) -> (u32, u32, Vec<String>, Vec<memory::WikiBacklog>) {
    let memory = live.executor.memory();
    let wikis = live.executor.wikis();
    let forgotten = |text: &str| memory.is_forgotten(text);
    let (mut pages, mut flagged, mut failed, mut left) = (0, 0, Vec::new(), Vec::new());
    for mut entry in work {
        let scrub: Option<&dyn Fn(&str) -> bool> = entry.forgotten.then_some(&forgotten);
        match wikis.withdraw_memory(&entry.project_id, &entry.ids, scrub, now_ms()) {
            Ok(report) => {
                pages += report.pages;
                flagged += report.flagged;
            }
            Err(error) => {
                let name = super::schedule::project(live, &entry.project_id)
                    .and_then(|p| p.get("name").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_else(|| entry.project_id.clone());
                failed.push(format!("项目「{name}」的 Wiki 没能更新：{error}"));
                entry.reason = error.to_string();
                left.push(entry);
            }
        }
    }
    (pages, flagged, failed, left)
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
    // `removed` stays the count older clients read; C2: `withdrawal` says
    // what else it touched.
    match live.executor.memory().forget(&p.id, now_ms()) {
        Ok(mut withdrawal) => {
            reach_wiki(live, &mut withdrawal, true);
            ok(
                request,
                json!({ "removed": withdrawal.items, "withdrawal": withdrawal }),
            )
        }
        Err(error) => memory_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocateParams {
    item_id: String,
    index: usize,
}

/// `memory.evidenceLocate {itemId, index}` (C1): what became of one piece
/// of an item's evidence — the message it was found in, read again from the
/// session (or the imported session) it came from.
fn memory_evidence_locate(live: &Live, request: &Request) -> Frame {
    let p: LocateParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let item = match live.executor.memory().item(&p.item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return failure(request.id.clone(), ErrorCode::NotFound, "没有这条记忆"),
        Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    };
    let Some(evidence) = item.evidence.get(p.index) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "这条记忆没有这份依据",
        );
    };
    let messages = match evidence.task_id.as_deref() {
        Some(task_id) if evidence.location.is_some() => match source_messages(live, task_id) {
            Ok(messages) => messages,
            Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
        },
        _ => None,
    };
    let found = memory::locate::lookup(evidence, messages.as_deref());
    ok(request, json!(found))
}

/// A session's text messages as extraction read them: a task's current
/// session, or an imported one; `None` when neither is there any more.
fn source_messages(live: &Live, task_id: &str) -> Result<Option<Vec<(String, String)>>, String> {
    if let Some((_, record)) = live.record(task_id) {
        return super::context_messages(live, &record, false).map(Some);
    }
    match imports(live).messages(task_id) {
        Ok((_, messages)) => Ok(Some(text_pairs(&messages))),
        Err(_) => Ok(None),
    }
}

/// `memory.attention {}`: proposals waiting for the user.
fn memory_attention(live: &Live, request: &Request) -> Frame {
    let memory = live.executor.memory();
    // C2: Wiki withdrawals that failed are tried again; those still
    // failing come along.
    let backlog = memory.wiki_backlog();
    let backlog = if backlog.is_empty() {
        backlog
    } else {
        let (_, _, _, left) = run_backlog(live, backlog);
        let _ = memory.set_wiki_backlog(&left);
        left
    };
    match memory.attention() {
        // B5-3b: skill proposals waiting for the user come along.
        Ok(items) => ok(
            request,
            json!({
                "items": items,
                "proposals": memory
                    .proposals()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|p| p.state == wire::SkillProposalState::Pending)
                    .collect::<Vec<_>>(),
                "wikiBacklog": backlog,
            }),
        ),
        Err(error) => memory_failure(request, error),
    }
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
        let was = memory.project_enabled(&p.project_id);
        if let Err(error) = memory.set_project_enabled(&p.project_id, enabled) {
            return memory_failure(request, error);
        }
        // B6: the ledger's turns of a project that starts using memory.
        if enabled && !was {
            backfill_chain(live, &p.project_id);
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

/// B6: a block's turn as the ledger has it now (accepted or undone since);
/// the stored state once the ledger let the turn go.
fn fresh(live: &Live, mut change: wire::CodeChange) -> wire::CodeChange {
    if let Some(record) = live
        .executor
        .turn_ledger()
        .get(&change.task_id, &change.turn_id)
    {
        change.state = record.state;
    }
    change
}

/// "  改动：src/ssh.rs::connect · main · +3 −1 · 已撤销".
fn change_line(change: &wire::CodeChange) -> String {
    let mut parts = vec![match &change.symbol {
        Some(symbol) => format!("{}::{symbol}", change.path),
        None => change.path.clone(),
    }];
    if let Some(branch) = &change.branch {
        parts.push(branch.clone());
    }
    parts.push(format!("+{} −{}", change.additions, change.deletions));
    parts.push(
        match change.state {
            aeroric_protocol::turn_ledger::TurnState::Pending => "待验收",
            aeroric_protocol::turn_ledger::TurnState::Accepted => "已接受",
            aeroric_protocol::turn_ledger::TurnState::Undone => "已撤销",
        }
        .into(),
    );
    format!("  改动：{}", parts.join(" · "))
}

/// B6: every ledger turn of the project's sessions not in the chain yet.
fn backfill_chain(live: &Live, project_id: &str) {
    let Ok(document) = live.service.load(project_id) else {
        return;
    };
    let memory = live.executor.memory();
    let ledger = live.executor.turn_ledger();
    for record in document.records {
        let Some(task_id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        for turn in ledger.list(task_id) {
            if !memory.has_changes(task_id, &turn.turn_id) {
                memory::chain::record_turn(memory, project_id, task_id, &turn);
            }
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChainParams {
    id: String,
}

/// `memory.chain {id}` (B6): the code changes of the sessions a memory came
/// from, newest first.
fn memory_chain(live: &Live, request: &Request) -> Frame {
    let p: ChainParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    let item = match memory.item(&p.id) {
        Ok(Some(item)) => item,
        Ok(None) => return memory_failure(request, MemoryError::UnknownItem),
        Err(error) => return memory_failure(request, error),
    };
    let changes: Vec<_> = memory
        .changes_for(&item, 20)
        .into_iter()
        .map(|c| fresh(live, c))
        .collect();
    ok(request, json!({ "changes": changes }))
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
    let (task, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let vector = query_vector(live, &p.query);
    match live
        .executor
        .memory()
        .recall_in_task(&project, &p.query, vector.as_deref(), Some(&task))
    {
        Ok(items) => {
            let text = if items.is_empty() {
                "没有找到相关记忆。".to_string()
            } else {
                let memory = live.executor.memory();
                let mut decorated = false;
                let lines: Vec<String> = items
                    .iter()
                    .map(|item| {
                        let mut out = line(item);
                        for change in memory.changes_for(item, wire::CHAIN_PER_ITEM) {
                            decorated = true;
                            out.push('\n');
                            out.push_str(&change_line(&fresh(live, change)));
                        }
                        out
                    })
                    .collect();
                let note = if decorated {
                    "\n“改动”是这条记忆的来源会话改过的代码块（文件::所在定义），从新到旧，已撤销的也列出；它说的是当时，不代表现在的代码。"
                } else {
                    ""
                };
                format!(
                    "找到 {} 条记忆：\n{}{note}\n用过之后可以用 memory_feedback 告诉 Aeroric 它有没有用。",
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
        // C3: a queued row has not asked the model yet.
        .filter(|r| {
            r.at_ms >= month_start
                && r.kind != wire::MemoryRunKind::Reindex
                && !matches!(
                    r.state,
                    wire::MemoryRunState::Skipped | wire::MemoryRunState::Queued
                )
        })
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
            // C3 second step: the App offers 重建索引 when it is not.
            "indexHealthy": memory.index_healthy(),
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
fn extract_task(
    live: &Live,
    task_id: &str,
    forced: bool,
    run_id: Option<&str>,
) -> Result<wire::MemoryRun, String> {
    let (project, record) = live.record(task_id).ok_or("没有这个会话")?;
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let title = super::context_title(&record);
    let messages = super::context_messages(live, &record, false)?;
    // B5-3a: its acceptance criteria all passed — experiences start at 中.
    let verified = live.executor.checkpoints().get(task_id).is_some_and(|cp| {
        !cp.obligations.is_empty()
            && cp
                .obligations
                .iter()
                .all(|o| o.state == aeroric_protocol::checkpoint::ObligationState::Passed)
    });
    let home = std::env::var_os("HOME").map(PathBuf::from);
    live.executor.memory().extract_session(
        memory::run::Session {
            task_id,
            project_id: &project_id,
            title: &title,
            messages: &messages,
            forced,
            verified,
            run_id,
        },
        home.as_deref(),
        analysis_call(live, 1200),
        now_ms(),
    )
}

/// The analysis model over the chosen connection with its stored key (also
/// the Wiki's module explanations, R4-2).
pub(super) fn analysis_call(
    live: &Live,
    max_tokens: u32,
) -> impl Fn(&str, &str) -> Result<aeroric_core::settings::model_detect::ChatOnce, String> {
    use aeroric_core::agent_connections::store;
    let settings = live.executor.memory().settings();
    let data = live.context.paths.data_dir().to_path_buf();
    let secrets = Arc::clone(&live.context.secrets);
    move |system: &str, user: &str| {
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
            max_tokens,
        ))
    }
}

fn imports(live: &Live) -> aeroric_core::sessions::import::ImportStore {
    aeroric_core::sessions::import::ImportStore::new(live.context.paths.data_dir())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HistoryPlanParams {
    project_id: String,
    #[serde(default)]
    since_ms: Option<u64>,
}

/// `memory.historyPlan {projectId, sinceMs?}` (Q4): the project's imported
/// sessions since then and what extracting them would cost. Reads only the
/// import index; no session text, no model call.
fn memory_history_plan(live: &Live, request: &Request) -> Frame {
    let p: HistoryPlanParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let sources: Vec<_> = imports(live)
        .visible()
        .into_iter()
        .filter(|s| s.project_id.as_deref() == Some(p.project_id.as_str()))
        .filter(|s| p.since_ms.is_none_or(|since| s.last_at_ms >= since))
        .map(|s| memory::run::HistorySource {
            agent: s.agent.as_str().to_string(),
            bytes: s.marker.size,
            id: s.id,
            title: s.title,
            last_at_ms: s.last_at_ms,
        })
        .collect();
    let plan = live
        .executor
        .memory()
        .history_plan(&p.project_id, &sources, now_ms());
    ok(request, json!(plan))
}

/// One imported session's text blocks, `(role, text)`.
fn text_pairs(messages: &[aeroric_core::sessions::SessionMessage]) -> Vec<(String, String)> {
    messages
        .iter()
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExtractImportedParams {
    id: String,
}

/// `memory.extractImported {id}` (Q4): extract one imported session. Not
/// forced: one already extracted, or where the user said one thing, is
/// skipped, and the day's limit holds.
fn memory_extract_imported(live: &Live, request: &Request) -> Frame {
    let p: ExtractImportedParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let result = imports(live)
        .messages(&p.id)
        .map_err(|error| error.to_string())
        .and_then(|(session, messages)| {
            let project_id = session.project_id.clone().ok_or("这个会话还没有归到项目")?;
            let pairs = text_pairs(&messages);
            let home = std::env::var_os("HOME").map(PathBuf::from);
            live.executor.memory().extract_session(
                memory::run::Session {
                    task_id: &session.id,
                    project_id: &project_id,
                    title: &session.title,
                    messages: &pairs,
                    forced: false,
                    verified: false,
                    run_id: None,
                },
                home.as_deref(),
                analysis_call(live, 1200),
                now_ms(),
            )
        });
    match result {
        Ok(run) => ok(request, json!({ "run": run })),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
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
    match extract_task(live, &p.task_id, true, None) {
        Ok(run) => ok(request, json!({ "run": run })),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunCancelParams {
    run_id: String,
}

/// `memory.runCancel {runId}` (C3): a queued 整理 is canceled at once; a
/// running one stops before its next write. The row as it is now (still
/// `running` until the run notices).
fn memory_run_cancel(live: &Live, request: &Request) -> Frame {
    let p: RunCancelParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor.memory().cancel_run(&p.run_id, now_ms()),
        "run",
    )
}

/// `memory.runRetry {runId}` (C3 second step): make the writes a run could
/// not, without the model; a run with none waiting (its call failed, or
/// they were kept past the retention) has its session extracted again.
fn memory_run_retry(live: &Live, request: &Request) -> Frame {
    let p: RunCancelParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    let result = match memory.retry_run(&p.run_id, now_ms()) {
        Ok(Some(run)) => Ok(run),
        Ok(None) => match memory.run(&p.run_id) {
            Some(run) if run.kind == wire::MemoryRunKind::Extract && !run.task_id.is_empty() => {
                extract_task(live, &run.task_id, true, None)
            }
            _ => Err("这次整理没有可以重试的内容".to_string()),
        },
        Err(error) => return memory_failure(request, error),
    };
    match result {
        Ok(run) => ok(request, json!({ "run": run })),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

/// `memory.reindex {}` (C3 second step): 重建索引, a row in 整理记录.
fn memory_reindex(live: &Live, request: &Request) -> Frame {
    answer(
        request,
        live.executor.memory().rebuild_index(now_ms()),
        "run",
    )
}

/// C3 second step: writes a run could not make are made again — no model,
/// so whether or not automatic extraction is on.
fn retry_writes(live: &Live) {
    let memory = live.executor.memory();
    let now = now_ms();
    for id in memory.retryable(now) {
        let _ = memory.retry_run(&id, now);
    }
}

/// C3: one automatic pass at a time, so a session is queued once.
static SWEEPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// B6: when a session last ran a turn, as the sweeps saw it (a dream waits
/// for `DREAM_IDLE_MS` of quiet).
static LAST_BUSY_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The project's name for 整理记录, or "全局".
fn scope_title(live: &Live, project_id: Option<&str>) -> String {
    let Some(id) = project_id else {
        return "全局".into();
    };
    aeroric_core::projects::ProjectService::new(Arc::clone(&live.context))
        .list()
        .ok()
        .and_then(|projects| {
            projects
                .into_iter()
                .find(|p| p.get("id").and_then(Value::as_str) == Some(id))
        })
        .and_then(|p| p.get("name").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| id.to_string())
}

/// Dream one scope; a local project's folder is also checked for files and
/// symbols gone (过时检测). `free_only`: only that check, no model.
fn dream_scope(
    live: &Live,
    project_id: Option<&str>,
    forced: bool,
    free_only: bool,
) -> Result<Option<wire::MemoryRun>, String> {
    let title = scope_title(live, project_id);
    let root = project_id.and_then(|id| project_dir(live, id));
    live.executor.memory().dream(
        memory::dream::DreamScope {
            project_id,
            title: &title,
            forced,
            root: root.as_deref(),
            free_only,
        },
        analysis_call(live, 1200),
        now_ms(),
    )
}

/// The automatic pass (Q-M10, Q-M15): local native sessions of projects
/// that use memory, done and quiet for `SETTLED_AFTER_MS` or archived,
/// finished after auto was turned on, not extracted yet. Stops at the
/// day's limit. B6: then every scope due a dream, once all sessions have
/// been quiet for `DREAM_IDLE_MS`.
fn sweep(live: &Live) -> usize {
    embed_missing(live);
    retry_writes(live);
    let done = extract_finished(live);
    dream_due_scopes(live);
    done
}

fn dream_due_scopes(live: &Live) {
    use std::sync::atomic::Ordering;
    let memory = live.executor.memory();
    let settings = memory.settings();
    let now = now_ms();
    if live
        .executor
        .registry()
        .all()
        .iter()
        .any(|(_, driver)| driver.snapshot().turn_active)
    {
        LAST_BUSY_MS.store(now, Ordering::Relaxed);
    }
    if !settings.auto
        || now.saturating_sub(LAST_BUSY_MS.load(Ordering::Relaxed)) < wire::DREAM_IDLE_MS
    {
        return;
    }
    // Second batch: with no dream due (or no model), a project still gets
    // its daily 过时检测, which needs no model.
    let mut dreaming = settings.model.is_some();
    let Ok(projects) =
        aeroric_core::projects::ProjectService::new(Arc::clone(&live.context)).list()
    else {
        return;
    };
    let mut scopes: Vec<Option<String>> = vec![None];
    scopes.extend(
        projects
            .iter()
            .filter_map(|p| p.get("id").and_then(Value::as_str))
            .filter(|id| memory.project_enabled(id))
            .map(|id| Some(id.to_string())),
    );
    for scope in scopes {
        if dreaming && memory.dream_due(scope.as_deref(), now) {
            match dream_scope(live, scope.as_deref(), false, false) {
                Err(error) if error.contains("到上限") || error.contains("还没有选") => {
                    dreaming = false;
                }
                _ => continue,
            }
        }
        if let Some(project) = scope.as_deref() {
            if memory.stale_due(project, now) {
                let _ = dream_scope(live, scope.as_deref(), false, true);
            }
        }
    }
}

fn extract_finished(live: &Live) -> usize {
    use std::sync::atomic::Ordering;
    if SWEEPING.swap(true, Ordering::AcqRel) {
        return 0;
    }
    let done = extract_due(live);
    SWEEPING.store(false, Ordering::Release);
    done
}

/// C3: every due session is queued first (整理记录 shows them waiting),
/// then extracted one by one; a row the user canceled meanwhile is left,
/// and those the pass does not get to leave the log.
fn extract_due(live: &Live) -> usize {
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
    let mut due: Vec<(String, String, String)> = Vec::new();
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
            let number = |key: &str| record.get(key).and_then(Value::as_u64);
            // B6: a turn that ended between two sweeps keeps a dream
            // waiting (a native session is `done` after every turn).
            if let Some(at) = number("completedAt") {
                LAST_BUSY_MS.fetch_max(at.min(now), std::sync::atomic::Ordering::Relaxed);
            }
            if memory.extracted(id).is_some() || !live.structured(&record, &project) {
                continue;
            }
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
            due.push((
                id.to_string(),
                project_id.to_string(),
                super::context_title(&record),
            ));
        }
    }
    let queued: Vec<(String, String)> = due
        .into_iter()
        .filter_map(|(task, project, title)| {
            memory
                .queue_run(&task, &project, &title, now)
                .ok()
                .map(|run| (task, run))
        })
        .collect();
    let mut done = 0;
    let mut stop = false;
    for (task, run) in &queued {
        if stop || !memory.queued(run) {
            memory.unqueue(run);
            continue;
        }
        match extract_task(live, task, false, Some(run)) {
            Ok(_) => done += 1,
            Err(error) => {
                memory.unqueue(run);
                stop = error.contains("到上限") || error.contains("还没有选");
            }
        }
    }
    done
}

/// `memory.dream {projectId?}` (B6): 现在整理 — dream one scope now, even
/// with fewer than `DREAM_AFTER_NEW` new items; the day's limit holds.
fn memory_dream(live: &Live, request: &Request) -> Frame {
    let p: ScopeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match dream_scope(live, p.project_id.as_deref(), true, false) {
        Ok(run) => ok(request, json!({ "run": run })),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

/// `memory.dreamProposals {projectId?}`: a scope's dream proposals.
fn memory_dream_proposals(live: &Live, request: &Request) -> Frame {
    let p: ScopeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor
            .memory()
            .dream_proposals(p.project_id.as_deref()),
        "proposals",
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DreamDecideParams {
    id: String,
    accept: bool,
    /// The item to keep, when the user picked another than the model.
    #[serde(default)]
    keep: Option<String>,
}

/// `memory.dreamDecide {id, accept, keep?}`.
fn memory_dream_decide(live: &Live, request: &Request) -> Frame {
    let p: DreamDecideParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor
            .memory()
            .decide_dream(&p.id, p.accept, p.keep.as_deref(), now_ms()),
        "proposal",
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DreamUndoParams {
    run_id: String,
}

/// `memory.dreamUndo {runId}`: 撤销这次整理.
fn memory_dream_undo(live: &Live, request: &Request) -> Frame {
    let p: DreamUndoParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor.memory().undo_dream(&p.run_id, now_ms()),
        "run",
    )
}

/// B6 可选 embedding: 随手记's embedding model, as a blocking function over
/// whole lists (split into the provider's batches). `None` while
/// 按意思找相近的记忆 is off.
fn embedder(live: &Live) -> Option<(String, impl Fn(&[String]) -> Result<Vec<Vec<f32>>, String>)> {
    if !live.executor.memory().settings().semantic {
        return None;
    }
    let config = crate::notebook_services::resolve_key_with_secrets(
        aeroric_core::settings::notebook_embed_config(),
        &*live.context.secrets,
    )
    .ok()?;
    let model = config.model.trim().to_string();
    let embed = move |inputs: &[String]| -> Result<Vec<Vec<f32>>, String> {
        use aeroric_core::notebook::rag::embed;
        let mut out = Vec::with_capacity(inputs.len());
        for range in embed::batches(config.provider, inputs.len()) {
            let part = tokio::runtime::Handle::current()
                .block_on(embed::embed_batch(&config, &inputs[range], &|| false))
                .map_err(|e| e.to_string())?;
            out.extend(part);
        }
        Ok(out)
    };
    Some((model, embed))
}

/// The sweep's part: vectors for items that have none yet.
fn embed_missing(live: &Live) {
    if let Some((model, embed)) = embedder(live) {
        let _ = live
            .executor
            .memory()
            .embed_missing(&model, embed, wire::SEMANTIC_PER_SWEEP);
    }
}

/// The recall query's unit vector; `None` when off, or when the model
/// cannot be reached (recall then uses words alone).
fn query_vector(live: &Live, query: &str) -> Option<Vec<f32>> {
    let (model, embed) = embedder(live)?;
    // The last try failed (the sweep retries every two minutes): do not
    // make every recall wait on a model that is not answering.
    let status = live.executor.memory().semantic_status().ok()?;
    if status.error.is_some() || status.model != model {
        return None;
    }
    let text = memory::semantic::for_query(&model, query);
    let vector = embed(&[text]).ok()?.into_iter().next()?;
    Some(memory::semantic::unit(vector))
}

/// `memory.semanticStatus {}` (B6): how far the vectors are, with the model
/// 随手记's settings name now.
fn memory_semantic_status(live: &Live, request: &Request) -> Frame {
    match live.executor.memory().semantic_status() {
        Ok(status) => ok(
            request,
            json!({
                "status": status,
                "model": aeroric_core::settings::notebook_embed_config().model,
            }),
        ),
        Err(error) => memory_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DreamRevertParams {
    id: String,
}

/// `memory.dreamRevert {id}` (B6 second batch): take back one accepted
/// proposal.
fn memory_dream_revert(live: &Live, request: &Request) -> Frame {
    let p: DreamRevertParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor.memory().revert_dream(&p.id, now_ms()),
        "proposal",
    )
}

/// `memory.dreamPending {}` (B6 second batch): the proposals waiting in
/// every scope, for 需要我处理.
fn memory_dream_pending(live: &Live, request: &Request) -> Frame {
    answer(
        request,
        live.executor.memory().pending_dreams(),
        "proposals",
    )
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
    // B6: turns recorded before the chain existed, or while the host was
    // down, once at start.
    let start = Arc::downgrade(live);
    handle.spawn(async move {
        let _ = tokio::task::spawn_blocking(move || {
            let Some(live) = start.upgrade() else {
                return;
            };
            let Ok(projects) =
                aeroric_core::projects::ProjectService::new(Arc::clone(&live.context)).list()
            else {
                return;
            };
            for project in projects {
                if let Some(id) = project.get("id").and_then(Value::as_str) {
                    if live.executor.memory().project_enabled(id) {
                        backfill_chain(&live, id);
                    }
                }
            }
        })
        .await;
    });
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
        // The file's name leads the excerpt so the page can group an import.
        let excerpt = format!("{} · {line}", file.title);
        let proof = memory::evidence(None, Some(&file.harness), None, &excerpt, now);
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
    let (task, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    match memory.item(&p.id) {
        Ok(Some(item)) if item.project_id.as_deref().is_none_or(|id| id == project) => {}
        Ok(_) => return memory_failure(request, MemoryError::UnknownItem),
        Err(error) => return memory_failure(request, error),
    }
    // C3: one vote per item per session.
    match memory.feedback_in_task(&p.id, p.helpful, &task, now_ms()) {
        Ok(Some(item)) => ok(
            request,
            json!({
                "text": format!("已记下：[{}] {}", item.id, if p.helpful { "有用" } else { "不对" }),
                "item": item,
            }),
        ),
        Ok(None) => ok(
            request,
            json!({
                "text": format!("这个会话已经反馈过 [{}]，不重复计数", p.id),
                "item": memory.item(&p.id).ok().flatten(),
            }),
        ),
        Err(error) => memory_failure(request, error),
    }
}

// MARK: B5-3b skill proposals

/// `memory.proposals {}`: every proposal, newest first.
fn skill_proposals(live: &Live, request: &Request) -> Frame {
    answer(request, live.executor.memory().proposals(), "proposals")
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillEditParams {
    id: String,
    edit: wire::SkillEdit,
}

/// `memory.proposalEdit {id, edit}`: a pending proposal changes; an accepted
/// skill gets a new version.
fn skill_edit(live: &Live, request: &Request) -> Frame {
    let p: SkillEditParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor
            .memory()
            .edit_proposal(&p.id, p.edit, now_ms()),
        "proposal",
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillDecideParams {
    id: String,
    accept: bool,
}

/// `memory.proposalDecide {id, accept}`: accepting pins the project's folder,
/// under which its sessions are offered the skill.
fn skill_decide(live: &Live, request: &Request) -> Frame {
    let p: SkillDecideParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let memory = live.executor.memory();
    let path = match memory.proposal(&p.id) {
        Ok(proposal) => proposal
            .project_id
            .as_deref()
            .and_then(|id| project_dir(live, id)),
        Err(error) => return memory_failure(request, error),
    };
    answer(
        request,
        memory.decide_proposal(&p.id, p.accept, path, now_ms()),
        "proposal",
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillVersionParams {
    id: String,
    version: u32,
}

/// `memory.skillVersion {id, version}`: 回到第 N 版.
fn skill_version(live: &Live, request: &Request) -> Frame {
    let p: SkillVersionParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor
            .memory()
            .use_version(&p.id, p.version, now_ms()),
        "proposal",
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillDisableParams {
    id: String,
    disabled: bool,
}

/// `memory.skillDisable {id, disabled}`: 停用 / 启用.
fn skill_disable(live: &Live, request: &Request) -> Frame {
    let p: SkillDisableParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor
            .memory()
            .disable_skill(&p.id, p.disabled, now_ms()),
        "proposal",
    )
}
