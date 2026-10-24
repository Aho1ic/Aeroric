//! 已有会话导入的宿主面（UI 改版 M4）：`imports.*` 与 `agents.detect`。
//!
//! 只在本机 UDS 注册，不进 remote rpc（手机端不可达）。没有授权的 Agent，core 一个会话文件都不读；
//! `imports.estimate` 只做目录计数。扫描是按需增量的：授权后立即扫一次，之后 `imports.list` /
//! `imports.status` 在上次扫描超过 [`RESCAN_AFTER`] 时先补扫（marker 未变的文件不读）。

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aeroric_core::agents::detect::detect_agents;
use aeroric_core::context::CoreContext;
use aeroric_core::projects::mutations;
use aeroric_core::sessions::import::{estimate, ImportAgent, ImportError, ImportStore, ScanReport};
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

pub fn register_import_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    set.register("imports.status", blocking(&context, status));
    set.register("imports.estimate", blocking(&context, estimate_handler));
    set.register("imports.grant", blocking(&context, grant));
    set.register("imports.revoke", blocking(&context, revoke));
    set.register("imports.rescan", blocking(&context, rescan));
    set.register("imports.list", blocking(&context, list));
    set.register("imports.history", blocking(&context, history));
    set.register("agents.detect", blocking(&context, detect));
}

fn blocking(
    context: &Arc<CoreContext>,
    run: impl Fn(Request, Arc<CoreContext>) -> Frame + Send + Sync + 'static,
) -> Handler {
    let context = Arc::clone(context);
    let run = Arc::new(run);
    Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        let run = Arc::clone(&run);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(request, context)).await {
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

fn status(request: Request, context: Arc<CoreContext>) -> Frame {
    let _ = scan(&context, false);
    let store = store(&context);
    let grants = store.grants();
    let index = store.index();
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
                    .unwrap_or_else(|| agent.default_root(context.paths.user_home())),
                "grantedAtMs": grant.map(|g| g.granted_at_ms),
                "sessions": index.iter().filter(|s| s.agent == *agent).count(),
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

fn estimate_handler(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(agent) = agent_param(&request) else {
        return bad_agent(&request);
    };
    let root = agent.default_root(context.paths.user_home());
    let estimate = estimate(agent, &root);
    ok(
        &request,
        json!({ "agent": agent.as_str(), "root": root, "estimate": estimate }),
    )
}

fn grant(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(agent) = agent_param(&request) else {
        return bad_agent(&request);
    };
    // The root is always the host-computed default; a client cannot point the
    // importer at an arbitrary directory.
    let root = agent.default_root(context.paths.user_home());
    if let Err(error) = store(&context).grant(agent, root.clone(), now_ms()) {
        return io_failure(&request, error);
    }
    match scan(&context, true) {
        Ok(report) => ok(
            &request,
            json!({ "agent": agent.as_str(), "root": root, "report": report }),
        ),
        Err(error) => io_failure(&request, error),
    }
}

fn revoke(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(agent) = agent_param(&request) else {
        return bad_agent(&request);
    };
    let _guard = STATE.lock();
    match store(&context).revoke(agent) {
        Ok(()) => ok(
            &request,
            json!({ "agent": agent.as_str(), "revoked": true }),
        ),
        Err(error) => io_failure(&request, error),
    }
}

fn rescan(request: Request, context: Arc<CoreContext>) -> Frame {
    match scan(&context, true) {
        Ok(report) => ok(&request, json!({ "report": report })),
        Err(error) => io_failure(&request, error),
    }
}

/// `params.projectId` filters to one project; `params.unassigned: true` to the 未归类 group.
fn list(request: Request, context: Arc<CoreContext>) -> Frame {
    let _ = scan(&context, false);
    let project = request.params.get("projectId").and_then(Value::as_str);
    let unassigned = request.params.get("unassigned").and_then(Value::as_bool) == Some(true);
    let sessions: Vec<_> = store(&context)
        .index()
        .into_iter()
        .filter(|s| project.is_none_or(|id| s.project_id.as_deref() == Some(id)))
        .filter(|s| !unassigned || s.project_id.is_none())
        .collect();
    ok(&request, json!({ "sessions": sessions }))
}

fn history(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(id) = request.params.get("id").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "imports.history requires params.id",
        );
    };
    match store(&context).messages(id) {
        Ok((session, messages)) => {
            let family = match session.agent {
                ImportAgent::Claude => ProtocolFamily::Claude,
                ImportAgent::Codex => ProtocolFamily::Codex,
            };
            let mut page = serde_json::to_value(session_page(session.id.clone(), family, messages))
                .unwrap_or_default();
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

fn detect(request: Request, context: Arc<CoreContext>) -> Frame {
    let path = std::env::var("PATH").ok();
    let agents = detect_agents(path.as_deref(), context.paths.user_home());
    ok(&request, json!({ "agents": agents }))
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn context(root: &Path) -> Arc<CoreContext> {
        let paths = AppPaths::from_user_home(root.join("home"));
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
        let context = context(&root);

        let estimated = result(estimate_handler(
            request("imports.estimate", json!({"agent":"claude"})),
            Arc::clone(&context),
        ));
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
        std::fs::remove_dir_all(&root).ok();
    }
}
