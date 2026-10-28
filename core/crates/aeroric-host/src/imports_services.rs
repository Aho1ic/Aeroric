//! 已有会话导入的宿主面（UI 改版 M4）：`imports.*` 与 `agents.detect`。
//!
//! 只在本机 UDS 注册，不进 remote rpc（手机端不可达）。没有授权的 Agent，core 一个会话文件都不读；
//! `imports.estimate` 只做目录计数。扫描是按需增量的：授权后立即扫一次，之后 `imports.list` /
//! `imports.status` 在上次扫描超过 [`RESCAN_AFTER`] 时先补扫（marker 未变的文件不读）。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aeroric_core::agents::detect::detect_agents;
use aeroric_core::context::CoreContext;
use aeroric_core::projects::{mutations, ProjectService};
use aeroric_core::sessions::import::{
    assign_project, estimate, ImportAgent, ImportError, ImportStore, ScanReport,
};
use aeroric_core::storage::projects::load_projects;
use aeroric_core::tasks::ProtocolFamily;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::{ok, session_page};

pub const CAPABILITY: &str = "imports";
const RESCAN_AFTER: Duration = Duration::from_secs(15);

#[derive(Default)]
struct ScanState {
    last_at: Option<Instant>,
    last_at_ms: Option<u64>,
    last_report: Option<ScanReport>,
    last_error: Option<String>,
}

/// One lock for index writes and scan bookkeeping: scans are cheap with markers.
static STATE: Mutex<ScanState> = Mutex::new(ScanState {
    last_at: None,
    last_at_ms: None,
    last_report: None,
    last_error: None,
});

/// What the handlers read: the host context and the external user home the
/// session roots live under. The shared context's home is `/` on purpose (metrics
/// and discovery must not read outside history); a grant here is that consent,
/// so imports — like the settings services — get the real home from the host.
struct Env {
    context: Arc<CoreContext>,
    home: PathBuf,
}

pub fn register_import_services(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    external_home: Option<PathBuf>,
) {
    let home = external_home.unwrap_or_else(|| context.paths.user_home().to_path_buf());
    let env = Arc::new(Env { context, home });
    set.register("imports.status", blocking(&env, status));
    set.register("imports.estimate", blocking(&env, estimate_handler));
    set.register("imports.grant", blocking(&env, grant));
    set.register("imports.revoke", blocking(&env, revoke));
    set.register("imports.rescan", blocking(&env, rescan));
    set.register("imports.list", blocking(&env, list));
    set.register("imports.history", blocking(&env, history));
    set.register("imports.preview", blocking(&env, preview));
    set.register("imports.apply", blocking(&env, apply));
    set.register("imports.archive", blocking(&env, archive));
    set.register("imports.remove", blocking(&env, remove));
    set.register("agents.detect", blocking(&env, detect));
}

fn blocking(
    env: &Arc<Env>,
    run: impl Fn(Request, Arc<Env>) -> Frame + Send + Sync + 'static,
) -> Handler {
    let env = Arc::clone(env);
    let run = Arc::new(run);
    Arc::new(move |request: Request, _call| {
        let env = Arc::clone(&env);
        let run = Arc::clone(&run);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(request, env)).await {
                Ok(frame) => frame,
                Err(e) => failure(id, ErrorCode::IoError, &e.to_string()),
            }
        })
    })
}

fn store(context: &CoreContext) -> ImportStore {
    ImportStore::new(context.paths.data_dir())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn projects(context: &CoreContext) -> Vec<(String, String)> {
    load_projects(context)
        .unwrap_or_default()
        .iter()
        .filter_map(|record| {
            Some((
                mutations::text(record, "id")?.to_string(),
                mutations::text(record, "path")?.to_string(),
            ))
        })
        .collect()
}

fn agent_param(request: &Request) -> Option<ImportAgent> {
    request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .and_then(ImportAgent::parse)
}

fn bad_agent(request: &Request) -> Frame {
    failure(
        request.id.clone(),
        ErrorCode::InvalidArgument,
        "params.agent must be \"claude\" or \"codex\"",
    )
}

fn io_failure(request: &Request, error: impl std::fmt::Display) -> Frame {
    failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
}

/// Scan now (`force`) or only when the last scan is stale. Errors are kept for `status`.
fn scan(context: &CoreContext, force: bool) -> Result<Option<ScanReport>, ImportError> {
    let mut state = STATE.lock();
    if !force && state.last_at.is_some_and(|at| at.elapsed() < RESCAN_AFTER) {
        return Ok(None);
    }
    let result = store(context).scan(&projects(context));
    state.last_at = Some(Instant::now());
    state.last_at_ms = Some(now_ms());
    match &result {
        Ok(report) => {
            state.last_report = Some(report.clone());
            state.last_error = None;
        }
        Err(error) => state.last_error = Some(error.to_string()),
    }
    result.map(Some)
}

fn status(request: Request, env: Arc<Env>) -> Frame {
    let context = &env.context;
    let _ = scan(context, false);
    let store = store(context);
    let grants = store.grants();
    let index = store.index();
    let excluded = store.prefs().excluded;
    let state = STATE.lock();
    let agents: Vec<Value> = ImportAgent::ALL
        .iter()
        .map(|agent| {
            let grant = grants.iter().find(|g| g.agent == *agent);
            json!({
                "agent": agent.as_str(),
                "granted": grant.is_some(),
                "root": grant
                    .map(|g| g.root.clone())
                    .unwrap_or_else(|| agent.default_root(&env.home)),
                "grantedAtMs": grant.map(|g| g.granted_at_ms),
                "sessions": index.iter().filter(|s| s.agent == *agent).count(),
                // S8: sessions the user left out at import time, offered back.
                "excluded": index
                    .iter()
                    .filter(|s| s.agent == *agent && excluded.contains(&s.id))
                    .count(),
            })
        })
        .collect();
    ok(
        &request,
        json!({
            "agents": agents,
            "lastScanAtMs": state.last_at_ms,
            "lastReport": state.last_report,
            "lastError": state.last_error,
        }),
    )
}

fn estimate_handler(request: Request, env: Arc<Env>) -> Frame {
    let Some(agent) = agent_param(&request) else {
        return bad_agent(&request);
    };
    let root = agent.default_root(&env.home);
    let estimate = estimate(agent, &root);
    ok(
        &request,
        json!({ "agent": agent.as_str(), "root": root, "estimate": estimate }),
    )
}

fn grant(request: Request, env: Arc<Env>) -> Frame {
    let context = &env.context;
    let Some(agent) = agent_param(&request) else {
        return bad_agent(&request);
    };
    // The root is always the host-computed default; a client cannot point the
    // importer at an arbitrary directory.
    let root = agent.default_root(&env.home);
    if let Err(error) = store(context).grant(agent, root.clone(), now_ms()) {
        return io_failure(&request, error);
    }
    match scan(context, true) {
        Ok(report) => ok(
            &request,
            json!({ "agent": agent.as_str(), "root": root, "report": report }),
        ),
        Err(error) => io_failure(&request, error),
    }
}

fn revoke(request: Request, env: Arc<Env>) -> Frame {
    let context = &env.context;
    let Some(agent) = agent_param(&request) else {
        return bad_agent(&request);
    };
    let _guard = STATE.lock();
    match store(context).revoke(agent) {
        Ok(()) => ok(
            &request,
            json!({ "agent": agent.as_str(), "revoked": true }),
        ),
        Err(error) => io_failure(&request, error),
    }
}

fn rescan(request: Request, env: Arc<Env>) -> Frame {
    let context = &env.context;
    match scan(context, true) {
        Ok(report) => ok(&request, json!({ "report": report })),
        Err(error) => io_failure(&request, error),
    }
}

/// `params.projectId` filters to one project; `params.unassigned: true` to the 未归类 group.
fn list(request: Request, env: Arc<Env>) -> Frame {
    let context = &env.context;
    let _ = scan(context, false);
    let project = request.params.get("projectId").and_then(Value::as_str);
    let unassigned = request.params.get("unassigned").and_then(Value::as_bool) == Some(true);
    let sessions: Vec<_> = store(context)
        .visible()
        .into_iter()
        .filter(|s| project.is_none_or(|id| s.project_id.as_deref() == Some(id)))
        .filter(|s| !unassigned || s.project_id.is_none())
        .collect();
    ok(&request, json!({ "sessions": sessions }))
}

/// The agents whose session roots exist under the user's home.
fn import_roots(env: &Env) -> Vec<(ImportAgent, PathBuf)> {
    [ImportAgent::Claude, ImportAgent::Codex]
        .into_iter()
        .map(|agent| (agent, agent.default_root(&env.home)))
        .filter(|(_, root)| root.is_dir())
        .collect()
}

/// 2026-10-27 import picker: every session on disk, grouped later by its
/// working directory; nothing is saved. Opening the picker is the consent to
/// read the default roots (the same roots `imports.grant` reads).
fn preview(request: Request, env: Arc<Env>) -> Frame {
    let sessions = store(&env.context).preview(&import_roots(&env), &projects(&env.context));
    ok(&request, json!({ "sessions": sessions }))
}

/// The picker's answer (`params.selected`: session ids). Grants the agents
/// whose sessions were picked, adds a project for each picked working
/// directory that is not one yet (and still exists), remembers what was left
/// out, then rescans so the sessions land under their projects.
fn apply(request: Request, env: Arc<Env>) -> Frame {
    let context = &env.context;
    let selected: Vec<String> = request
        .params
        .get("selected")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let roots = import_roots(&env);
    let store = store(context);
    let offered = store.preview(&roots, &projects(context));
    let picked: Vec<_> = offered
        .iter()
        .filter(|p| selected.contains(&p.session.id))
        .collect();
    let offered_ids: Vec<String> = offered.iter().map(|p| p.session.id.clone()).collect();
    if let Err(error) = store.choose(&offered_ids, &selected) {
        return io_failure(&request, error);
    }
    for (agent, root) in &roots {
        if picked.iter().any(|p| p.session.agent == *agent) {
            if let Err(error) = store.grant(*agent, root.clone(), now_ms()) {
                return io_failure(&request, error);
            }
        }
    }
    let mut created = Vec::new();
    let mut failed = Vec::new();
    let mut known = projects(context);
    let mut dirs: Vec<String> = picked
        .iter()
        .filter_map(|p| p.session.cwd.clone())
        .collect();
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        if assign_project(Some(&dir), &known).is_some() || !std::path::Path::new(&dir).is_dir() {
            continue;
        }
        let new_id = uuid::Uuid::new_v4().to_string();
        match ProjectService::new(Arc::clone(context)).open_local(&dir, &new_id, now_ms() as i64) {
            Ok((upserted, _)) => {
                created.push(dir.clone());
                if let Some(id) = mutations::text(&upserted.project, "id") {
                    known.push((id.to_string(), dir));
                }
            }
            Err(error) => failed.push(format!("{dir}: {error}")),
        }
    }
    match scan(context, true) {
        Ok(report) => ok(
            &request,
            json!({
                "imported": picked.len(),
                "createdProjects": created,
                "failedProjects": failed,
                "report": report,
            }),
        ),
        Err(error) => io_failure(&request, error),
    }
}

fn session_id_param(request: &Request) -> Result<&str, Box<Frame>> {
    request
        .params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &format!("{} requires params.id", request.method),
            ))
        })
}

/// `params.archived` (default true): off the project list, under 已归档.
fn archive(request: Request, env: Arc<Env>) -> Frame {
    let id = match session_id_param(&request) {
        Ok(id) => id.to_string(),
        Err(frame) => return *frame,
    };
    let archived = request
        .params
        .get("archived")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let _guard = STATE.lock();
    match store(&env.context).set_archived(&id, archived) {
        Ok(()) => ok(&request, json!({ "id": id, "archived": archived })),
        Err(error) => io_failure(&request, error),
    }
}

/// Take a session out of Aeroric; the agent's own file is never touched.
fn remove(request: Request, env: Arc<Env>) -> Frame {
    let id = match session_id_param(&request) {
        Ok(id) => id.to_string(),
        Err(frame) => return *frame,
    };
    let _guard = STATE.lock();
    match store(&env.context).remove(&id) {
        Ok(()) => ok(&request, json!({ "id": id, "removed": true })),
        Err(error) => io_failure(&request, error),
    }
}

fn history(request: Request, env: Arc<Env>) -> Frame {
    let context = &env.context;
    let Some(id) = request.params.get("id").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "imports.history requires params.id",
        );
    };
    let before = request
        .params
        .get("before")
        .and_then(Value::as_str)
        .and_then(|cursor| cursor.parse::<usize>().ok());
    match store(context).messages(id) {
        Ok((session, messages)) => {
            let family = match session.agent {
                ImportAgent::Claude => ProtocolFamily::Claude,
                ImportAgent::Codex => ProtocolFamily::Codex,
            };
            let page = history_page(session_page(session.id.clone(), family, messages), before);
            let mut page = serde_json::to_value(page).unwrap_or_default();
            page["origin"] = json!("imported");
            page["title"] = json!(session.title);
            ok(&request, page)
        }
        Err(ImportError::UnknownSession(id)) => failure(
            request.id.clone(),
            ErrorCode::NotFound,
            &format!("no imported session with id {id}"),
        ),
        Err(error) => io_failure(&request, error),
    }
}

/// A history reply must fit one frame (`MAX_FRAME_BYTES`, 1 MiB): a whole
/// long session in one reply made the host drop the connection and the App
/// showed "读取历史失败：disconnected" (2026-10-27). So the reply is the
/// newest messages before `before` (a message index; default the end) that
/// fit `HISTORY_PAGE_BYTES`, with `nextCursor` pointing at the older rest.
const HISTORY_PAGE_BYTES: usize = 640 * 1024;
/// Longest string kept in one block (tool output, pasted files); the rest is
/// cut with a note. A message still over the page budget is cut harder.
const BLOCK_MAX_CHARS: usize = 24 * 1024;
const BLOCK_MIN_CHARS: usize = 1024;

pub(crate) fn history_page(
    mut page: aeroric_protocol::session::SessionPage,
    before: Option<usize>,
) -> aeroric_protocol::session::SessionPage {
    let mut messages = std::mem::take(&mut page.messages);
    let end = before.unwrap_or(messages.len()).min(messages.len());
    messages.truncate(end);
    let mut picked = Vec::new();
    let mut bytes = 0;
    while let Some(mut message) = messages.pop() {
        clip_message(&mut message, BLOCK_MAX_CHARS);
        let mut size = encoded_len(&message);
        if size > HISTORY_PAGE_BYTES {
            clip_message(&mut message, BLOCK_MIN_CHARS);
            size = encoded_len(&message);
        }
        if !picked.is_empty() && bytes + size > HISTORY_PAGE_BYTES {
            messages.push(message);
            break;
        }
        bytes += size;
        picked.push(message);
    }
    picked.reverse();
    page.has_more = !messages.is_empty();
    page.next_cursor = page.has_more.then(|| messages.len().to_string());
    page.messages = picked;
    page
}

fn encoded_len(message: &aeroric_protocol::session::SessionMessage) -> usize {
    serde_json::to_vec(message).map_or(0, |bytes| bytes.len())
}

fn clip_message(message: &mut aeroric_protocol::session::SessionMessage, max_chars: usize) {
    use aeroric_protocol::session::ContentBlock;
    for block in &mut message.blocks {
        match block {
            ContentBlock::Text { text, .. } => clip_text(text, max_chars),
            ContentBlock::Unknown(opaque) => clip_value(&mut opaque.payload, max_chars),
        }
    }
}

fn clip_value(value: &mut Value, max_chars: usize) {
    match value {
        Value::String(text) => clip_text(text, max_chars),
        Value::Array(items) => items.iter_mut().for_each(|v| clip_value(v, max_chars)),
        Value::Object(fields) => fields.values_mut().for_each(|v| clip_value(v, max_chars)),
        _ => {}
    }
}

fn clip_text(text: &mut String, max_chars: usize) {
    if text.len() <= max_chars {
        return;
    }
    let total = text.chars().count();
    if total <= max_chars {
        return;
    }
    let cut = text
        .char_indices()
        .nth(max_chars)
        .map_or(text.len(), |(i, _)| i);
    text.truncate(cut);
    text.push_str(&format!("\n…（内容过长，已截断；共 {total} 字）"));
}

fn detect(request: Request, env: Arc<Env>) -> Frame {
    let path = std::env::var("PATH").ok();
    let agents = detect_agents(path.as_deref(), &env.home);
    ok(&request, json!({ "agents": agents }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_history_is_paged_to_fit_one_frame() {
        use aeroric_protocol::session::{ContentBlock, MessageRole, SessionMessage, SessionPage};
        let big = "字".repeat(100_000);
        let messages: Vec<SessionMessage> = (0..60)
            .map(|i| SessionMessage {
                id: format!("m{i}"),
                role: MessageRole::Assistant,
                blocks: vec![ContentBlock::Text {
                    kind: "text".into(),
                    text: big.clone(),
                }],
                created_at: None,
            })
            .collect();
        let page = SessionPage {
            session_id: "s".into(),
            family: ProtocolFamily::Codex,
            messages,
            next_cursor: None,
            has_more: false,
        };
        let first = history_page(page.clone(), None);
        let bytes = serde_json::to_vec(&first).unwrap().len();
        assert!(
            bytes < aeroric_protocol::envelope::MAX_FRAME_BYTES,
            "{bytes}"
        );
        assert!(first.has_more);
        assert_eq!(
            first.messages.last().unwrap().id,
            "m59",
            "newest first page"
        );
        let cursor: usize = first.next_cursor.as_deref().unwrap().parse().unwrap();
        assert_eq!(cursor + first.messages.len(), 60);
        // Walking the cursor back reaches the first message exactly once.
        let mut seen = first.messages.len();
        let mut before = Some(cursor);
        while let Some(at) = before {
            let older = history_page(page.clone(), Some(at));
            seen += older.messages.len();
            before = older.next_cursor.as_deref().map(|c| c.parse().unwrap());
            if before.is_none() {
                assert_eq!(older.messages.first().unwrap().id, "m0");
            }
        }
        assert_eq!(seen, 60);
        match &first.messages[0].blocks[0] {
            ContentBlock::Text { text, .. } => assert!(text.contains("已截断")),
            other => panic!("{other:?}"),
        }
    }
    use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
    use aeroric_core::paths::AppPaths;
    use aeroric_core::storage_lock::LeaseOwner;
    use std::path::{Path, PathBuf};

    struct LocalProbe;
    impl StorageLocationProbe for LocalProbe {
        fn probe(&self, data_dir: &Path) -> StorageLocation {
            StorageLocation {
                directory: data_dir.to_path_buf(),
                volume: VolumeKind::Local,
                private_permissions_enforced: true,
            }
        }
    }

    /// Like `aeroric-cli serve`: the shared context's home is `/`; the real
    /// home (where the fake sessions are) comes in as the external home.
    fn env(root: &Path) -> Arc<Env> {
        let context = context(root);
        Arc::new(Env {
            context,
            home: root.join("home"),
        })
    }

    fn context(root: &Path) -> Arc<CoreContext> {
        let paths = AppPaths::with_data_dir("/", Some(&root.join("data")));
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        std::fs::write(
            paths.data_dir().join("projects.json"),
            serde_json::to_vec(&json!([{ "id": "p1", "name": "app", "path": "/work/app" }]))
                .unwrap(),
        )
        .unwrap();
        Arc::new(
            CoreContext::acquire(
                paths,
                Arc::new(aeroric_core::clock::SystemClock),
                &LeaseOwner {
                    application: "imports-test".into(),
                    version: "0-test".into(),
                },
                &LocalProbe,
            )
            .unwrap(),
        )
    }

    fn request(method: &str, params: Value) -> Request {
        serde_json::from_value(json!({ "v": 1, "id": "r1", "method": method, "params": params }))
            .unwrap()
    }

    fn result(frame: Frame) -> Value {
        match frame {
            Frame::Success(success) => success.result,
            other => panic!("expected success, got {other:?}"),
        }
    }

    fn scratch() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "aeroric-imports-host-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let session = root.join("home/.claude/projects/-work-app/s1.jsonl");
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::write(
            &session,
            concat!(
                r#"{"type":"user","cwd":"/work/app","message":{"role":"user","content":"修复登录页"}}"#,
                "\n",
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"a.rs"}}]}}"#,
                "\n"
            ),
        )
        .unwrap();
        root
    }

    // One test so the process-wide scan state is not shared between parallel tests.
    #[test]
    fn estimate_grant_list_history_and_revoke_round_trip() {
        let root = scratch();
        let context = env(&root);

        let estimated = result(estimate_handler(
            request("imports.estimate", json!({"agent":"claude"})),
            Arc::clone(&context),
        ));
        assert_eq!(
            estimated["root"],
            root.join("home/.claude").to_string_lossy().as_ref(),
            "the session root is under the external home, not the shared context's `/`"
        );
        assert_eq!(estimated["estimate"]["sessions"], 1);
        assert_eq!(estimated["estimate"]["rootExists"], true);
        let status = result(super::status(
            request("imports.status", json!({})),
            Arc::clone(&context),
        ));
        assert_eq!(status["agents"][0]["granted"], false);
        assert_eq!(
            status["agents"][0]["sessions"], 0,
            "nothing is indexed before a grant"
        );

        let granted = result(grant(
            request("imports.grant", json!({"agent":"claude"})),
            Arc::clone(&context),
        ));
        assert_eq!(granted["report"]["added"], 1);
        let listed = result(list(
            request("imports.list", json!({"projectId":"p1"})),
            Arc::clone(&context),
        ));
        let sessions = listed["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["title"], "修复登录页");
        let unassigned = result(list(
            request("imports.list", json!({"unassigned":true})),
            Arc::clone(&context),
        ));
        assert!(unassigned["sessions"].as_array().unwrap().is_empty());

        let id = sessions[0]["id"].as_str().unwrap().to_string();
        let page = result(history(
            request("imports.history", json!({"id": id})),
            Arc::clone(&context),
        ));
        assert_eq!(page["origin"], "imported");
        assert_eq!(page["family"], "claude");
        assert_eq!(page["messages"][1]["blocks"][0]["kind"], "tool_use");

        let revoked = result(revoke(
            request("imports.revoke", json!({"agent":"claude"})),
            Arc::clone(&context),
        ));
        assert_eq!(revoked["revoked"], true);
        match history(
            request("imports.history", json!({"id": id})),
            Arc::clone(&context),
        ) {
            Frame::Failure(_) => {}
            other => panic!("a revoked session must not be readable: {other:?}"),
        }
        match grant(
            request("imports.grant", json!({"agent":"dsh"})),
            Arc::clone(&context),
        ) {
            Frame::Failure(_) => {}
            other => panic!("unsupported agents are rejected: {other:?}"),
        }
        let detected = result(detect(
            request("agents.detect", json!({})),
            Arc::clone(&context),
        ));
        assert_eq!(detected["agents"].as_array().unwrap().len(), 4);

        // 2026-10-27 picker: a session in a folder that is no project yet.
        let fresh = root.join("work-new");
        std::fs::create_dir_all(&fresh).unwrap();
        let other = root.join("home/.claude/projects/-work-new/s2.jsonl");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(
            &other,
            format!(
                "{}\n",
                json!({"type":"user","cwd": fresh.to_string_lossy(),
                       "message":{"role":"user","content":"新项目里的会话"}})
            ),
        )
        .unwrap();
        let preview = result(super::preview(
            request("imports.preview", json!({})),
            Arc::clone(&context),
        ));
        let rows = preview["sessions"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r["selected"] == true));
        let new_id = rows
            .iter()
            .find(|r| r["title"] == "新项目里的会话")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let applied = result(apply(
            request("imports.apply", json!({"selected": [new_id.clone()]})),
            Arc::clone(&context),
        ));
        assert_eq!(applied["imported"], 1);
        assert_eq!(
            applied["createdProjects"].as_array().unwrap().len(),
            1,
            "{applied}"
        );
        let listed = result(list(
            request("imports.list", json!({})),
            Arc::clone(&context),
        ));
        let listed = listed["sessions"].as_array().unwrap();
        assert_eq!(
            listed.len(),
            1,
            "the unticked session is left out: {listed:?}"
        );
        assert!(
            listed[0]["projectId"].is_string(),
            "filed under the new project"
        );

        let archived = result(super::archive(
            request("imports.archive", json!({"id": new_id.clone()})),
            Arc::clone(&context),
        ));
        assert_eq!(archived["archived"], true);
        let listed = result(list(
            request("imports.list", json!({})),
            Arc::clone(&context),
        ));
        assert_eq!(listed["sessions"][0]["archived"], true);
        result(super::remove(
            request("imports.remove", json!({"id": new_id})),
            Arc::clone(&context),
        ));
        let listed = result(list(
            request("imports.list", json!({})),
            Arc::clone(&context),
        ));
        assert!(listed["sessions"].as_array().unwrap().is_empty());
        assert!(other.exists(), "the agent's own file is untouched");
        std::fs::remove_dir_all(&root).ok();
    }
}
