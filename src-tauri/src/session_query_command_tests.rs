//! Slice 25 regressions at the registered session-query command boundary.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aeroric_core::paths::AppPaths;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use crate::core_adapter::{CoreGateway, CorePlacement, HostAvailability, PlacementProbe};
use crate::host_client::scripted::{failure, handshake_script, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::platform;
use crate::session;

const READ_SESSION_ID_METHOD: &str = "session.read_session_id";
const SUPPORTS_RESUME_METHOD: &str = "session.session_supports_native_resume";
const MESSAGE_PAGE_METHOD: &str = "session.read_session_message_page";
const TOKEN: &str = "session-query-synthetic-token";

type MockApp = tauri::App<tauri::test::MockRuntime>;

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from("/tmp").join(format!("asqc-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).expect("create scratch root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn data_dir(&self) -> PathBuf {
        self.path().join(".aeroric")
    }

    fn assert_local_absent(&self) {
        assert!(
            !self.data_dir().exists(),
            "External must not create the local .aeroric directory"
        );
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

struct AbsentProbe;

impl PlacementProbe for AbsentProbe {
    fn availability(&self, _data_dir: &Path) -> HostAvailability {
        HostAvailability::Absent
    }

    fn attach(&self, _data_dir: &Path) -> Result<(HostClient, bool), String> {
        panic!("an absent-host fixture must not attach")
    }
}

fn embedded_app(scratch: &ScratchRoot) -> MockApp {
    let gateway_dir = scratch.path().join("gateway");
    let gateway = CoreGateway::startup_with_probe(
        AppPaths::with_data_dir(&gateway_dir, Some(&gateway_dir)),
        "session-query-test",
        &AbsentProbe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);
    let app = tauri::test::mock_app();
    app.manage(gateway);
    app
}

struct ExternalRig {
    app: MockApp,
    host: ScriptedHost,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl ExternalRig {
    async fn new(
        tag: &str,
        reply: impl Fn(&str, &Value) -> Option<Frame> + Send + Sync + 'static,
    ) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let handshake = handshake_script(TOKEN);
        let script: Script = Arc::new(move |method, params| {
            if method == READ_SESSION_ID_METHOD
                || method == SUPPORTS_RESUME_METHOD
                || method == MESSAGE_PAGE_METHOD
            {
                observed.lock().push((method.to_owned(), params.clone()));
                reply(method, params)
            } else {
                handshake(method, params)
            }
        });
        let host = ScriptedHost::start(tag, script, Some(TOKEN)).await;
        let client = HostClient::connect(&host.data_dir, "tauri")
            .await
            .expect("authenticated scratch handshake");
        assert!(host.methods().iter().any(|method| method == "handshake"));
        let gateway = CoreGateway::with_host_session_for_test(
            AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        assert_eq!(gateway.placement(), CorePlacement::External);

        let app = tauri::test::mock_app();
        app.manage(gateway);
        Self { app, host, calls }
    }

    fn assert_single_call(&self, method: &str, params: Value) {
        assert!(
            self.host.methods().iter().any(|seen| seen == method),
            "the real command must reach {method}"
        );
        assert_eq!(self.calls.lock().as_slice(), &[(method.to_owned(), params)]);
    }
}

#[tokio::test]
async fn embedded_answers_from_local_logic() {
    let scratch = ScratchRoot::new("embedded-sess");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let app = embedded_app(&scratch);

    // Missing file returns error from local validation
    let err = session::read_session_id(
        app.handle().clone(),
        "/nonexistent/session.jsonl".to_string(),
        "/path/to/project".to_string(),
        false,
        Some("claude".to_string()),
    )
    .await;
    assert!(err.is_err());

    // 消息分页命令在 Embedded 下同走本地校验(不转发)。
    let page_err = session::read_session_message_page(
        app.handle().clone(),
        "/nonexistent/session.jsonl".to_string(),
        "/path/to/project".to_string(),
        false,
        Some("claude".to_string()),
        None,
    )
    .await;
    assert!(page_err.is_err());
}

#[tokio::test]
async fn external_session_queries_dispatch_to_host() {
    let scratch = ScratchRoot::new("ext-query");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let rig = ExternalRig::new("sess-q-ext", |method, _params| {
        if method == READ_SESSION_ID_METHOD {
            Some(success(json!("remote-session-42")))
        } else if method == SUPPORTS_RESUME_METHOD {
            Some(success(json!(true)))
        } else if method == MESSAGE_PAGE_METHOD {
            Some(success(json!({
                "messages": [],
                "nextCursor": null,
                "hasMore": false,
            })))
        } else {
            None
        }
    })
    .await;
    scratch.assert_local_absent();

    let id = session::read_session_id(
        rig.app.handle().clone(),
        "/path/to/session.jsonl".to_string(),
        "/path/to/project".to_string(),
        false,
        Some("claude".to_string()),
    )
    .await
    .expect("external read_session_id succeeds");
    assert_eq!(id, Some("remote-session-42".to_string()));
    rig.assert_single_call(
        READ_SESSION_ID_METHOD,
        json!({
            "sessionPath": "/path/to/session.jsonl",
            "projectPath": "/path/to/project",
            "isCodex": false,
            "family": "claude",
        }),
    );

    let resume = session::session_supports_native_resume(
        rig.app.handle().clone(),
        "/path/to/session.jsonl".to_string(),
        "/path/to/project".to_string(),
        false,
        Some("claude".to_string()),
    )
    .await
    .expect("external session_supports_native_resume succeeds");
    assert!(resume);

    // 消息分页命令同样走宿主,且不在本地执行。
    let page = session::read_session_message_page(
        rig.app.handle().clone(),
        "/path/to/session.jsonl".to_string(),
        "/path/to/project".to_string(),
        false,
        Some("claude".to_string()),
        None,
    )
    .await
    .expect("external read_session_message_page succeeds");
    assert!(page.messages.is_empty());
    assert!(
        rig.host
            .methods()
            .iter()
            .any(|seen| seen == MESSAGE_PAGE_METHOD),
        "the real command must reach session.read_session_message_page"
    );
    assert_eq!(
        rig.calls.lock().last(),
        Some(&(
            MESSAGE_PAGE_METHOD.to_owned(),
            json!({
                "sessionPath": "/path/to/session.jsonl",
                "projectPath": "/path/to/project",
                "isCodex": false,
                "family": "claude",
                "cursor": null,
            })
        )),
    );
}
#[tokio::test]
async fn external_session_query_failures_do_not_fall_back_locally() {
    let scratch = ScratchRoot::new("ext-fail");
    let _home = platform::install_scratch_home(scratch.path().to_path_buf());
    let rig = ExternalRig::new("sess-q-fail", |_method, _params| {
        Some(failure(
            ErrorCode::UnsupportedCapability,
            "unknown method: session.read_session_id",
        ))
    })
    .await;
    scratch.assert_local_absent();

    let err = session::read_session_id(
        rig.app.handle().clone(),
        "/path/to/session.jsonl".to_string(),
        "/path/to/project".to_string(),
        false,
        Some("claude".to_string()),
    )
    .await
    .expect_err("external failure must not fall back to local");
    assert!(err.contains("协调重启"), "error: {err}");
    scratch.assert_local_absent();
}
