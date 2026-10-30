//! All in temporary directories, a fake keychain and a fake token server:
//! nothing here touches the real `~/.codex`, `~/.claude` or keychain.

use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{extract::State, routing::post, Json, Router};

use super::*;

struct FakeKeychain(Mutex<HashMap<String, String>>);

impl Keychain for FakeKeychain {
    fn read(&self, service: &str, account: Option<&str>) -> Result<Option<String>, String> {
        let items = self.0.lock().unwrap();
        Ok(match account {
            Some(account) => items.get(&format!("{service}/{account}")).cloned(),
            None => items.values().next().cloned(),
        })
    }

    fn write(&self, service: &str, account: &str, data: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(format!("{service}/{account}"), data.to_string());
        Ok(())
    }
}

#[derive(Clone)]
struct Server {
    calls: Arc<AtomicUsize>,
    reply: Arc<Mutex<(u16, Value)>>,
}

async fn token(
    State(server): State<Server>,
    Json(_body): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    server.calls.fetch_add(1, Ordering::SeqCst);
    // Slow enough that two racing renewals overlap.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (status, body) = server.reply.lock().unwrap().clone();
    (
        axum::http::StatusCode::from_u16(status).unwrap(),
        Json(body),
    )
}

async fn start(status: u16, reply: Value) -> (String, Server) {
    let server = Server {
        calls: Arc::new(AtomicUsize::new(0)),
        reply: Arc::new(Mutex::new((status, reply))),
    };
    let app = Router::new()
        .route("/token", post(token))
        .with_state(server.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    (url, server)
}

fn jwt(claims: Value) -> String {
    let encode = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    format!(
        "{}.{}.sig",
        encode(&json!({"alg": "none"})),
        encode(&claims)
    )
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("aeroric-login-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn credentials(url: &str, keychain: Arc<FakeKeychain>) -> (LoginCredentials, PathBuf) {
    let root = temp_dir();
    let paths = LoginPaths {
        codex_home: root.join("codex"),
        claude_dir: root.join("claude"),
        user: "tester".into(),
        codex_token_url: url.into(),
        claude_token_url: url.into(),
    };
    std::fs::create_dir_all(&paths.codex_home).unwrap();
    (LoginCredentials::new(paths, keychain), root)
}

fn write_codex(root: &Path, access: &str) {
    let auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "access_token": access,
            "refresh_token": "rt-1",
            "id_token": jwt(json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acct-1"}})),
        },
        "last_refresh": "2026-10-01T00:00:00Z",
        "keep": "me",
    });
    std::fs::write(root.join("codex/auth.json"), auth.to_string()).unwrap();
}

fn keychain() -> Arc<FakeKeychain> {
    Arc::new(FakeKeychain(Mutex::new(HashMap::new())))
}

fn expired() -> String {
    jwt(json!({"exp": now_ms() / 1000 - 60}))
}

fn fresh() -> String {
    jwt(json!({"exp": now_ms() / 1000 + 3600}))
}

#[tokio::test]
async fn a_fresh_codex_login_is_used_without_renewing() {
    let (url, server) = start(200, json!({})).await;
    let (creds, root) = credentials(&url, keychain());
    let access = fresh();
    write_codex(&root, &access);
    let token = creds.current(Harness::Codex).await.unwrap();
    assert_eq!(token.access_token, access);
    assert_eq!(token.account_id.as_deref(), Some("acct-1"));
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_expiring_codex_login_is_renewed_and_written_back_whole() {
    let renewed = fresh();
    let (url, server) = start(
        200,
        json!({"access_token": renewed, "refresh_token": "rt-2"}),
    )
    .await;
    let (creds, root) = credentials(&url, keychain());
    write_codex(&root, &expired());
    let token = creds.current(Harness::Codex).await.unwrap();
    assert_eq!(token.access_token, renewed);
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("codex/auth.json")).unwrap())
            .unwrap();
    assert_eq!(saved["tokens"]["refresh_token"], "rt-2");
    assert_eq!(saved["keep"], "me", "other fields stay");
    assert_ne!(saved["last_refresh"], "2026-10-01T00:00:00Z");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(root.join("codex/auth.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[tokio::test]
async fn a_login_the_cli_already_renewed_is_taken_as_is() {
    let (url, server) = start(200, json!({"access_token": "never"})).await;
    let (creds, root) = credentials(&url, keychain());
    let renewed_by_cli = fresh();
    write_codex(&root, &renewed_by_cli);
    let token = creds
        .rejected(Harness::Codex, "the-old-token")
        .await
        .unwrap();
    assert_eq!(token.access_token, renewed_by_cli);
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn racing_requests_renew_once() {
    let renewed = fresh();
    let (url, server) = start(200, json!({"access_token": renewed})).await;
    let (creds, root) = credentials(&url, keychain());
    let stale = expired();
    write_codex(&root, &stale);
    let creds = Arc::new(creds);
    let (a, b) = tokio::join!(
        {
            let c = creds.clone();
            let s = stale.clone();
            async move { c.rejected(Harness::Codex, &s).await }
        },
        {
            let c = creds.clone();
            let s = stale.clone();
            async move { c.rejected(Harness::Codex, &s).await }
        },
    );
    assert_eq!(a.unwrap().access_token, renewed);
    assert_eq!(b.unwrap().access_token, renewed);
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn getting_the_same_token_back_is_not_a_renewal() {
    let stale = expired();
    let (url, _server) = start(200, json!({"access_token": stale})).await;
    let (creds, root) = credentials(&url, keychain());
    write_codex(&root, &stale);
    let error = creds.rejected(Harness::Codex, &stale).await.unwrap_err();
    assert!(matches!(error, LoginError::Refresh(_)), "{error:?}");
}

#[tokio::test]
async fn a_dead_refresh_token_means_signed_out() {
    let (url, _server) = start(400, json!({"error": "invalid_grant"})).await;
    let (creds, root) = credentials(&url, keychain());
    let stale = expired();
    write_codex(&root, &stale);
    assert_eq!(
        creds.rejected(Harness::Codex, &stale).await.unwrap_err(),
        LoginError::SignedOut
    );
    let (url, _server) = start(401, json!({"error": {"code": "refresh_token_reused"}})).await;
    let (creds, root) = credentials(&url, keychain());
    write_codex(&root, &stale);
    assert_eq!(
        creds.rejected(Harness::Codex, &stale).await.unwrap_err(),
        LoginError::SignedOut
    );
}

#[tokio::test]
async fn an_api_key_or_missing_file_is_not_a_login() {
    let (url, _server) = start(200, json!({})).await;
    let (creds, root) = credentials(&url, keychain());
    assert_eq!(
        creds.current(Harness::Codex).await.unwrap_err(),
        LoginError::NotSignedIn
    );
    std::fs::write(
        root.join("codex/auth.json"),
        r#"{"auth_mode":"apikey","OPENAI_API_KEY":"k","tokens":null}"#,
    )
    .unwrap();
    assert_eq!(
        creds.current(Harness::Codex).await.unwrap_err(),
        LoginError::NotSignedIn
    );
}

#[tokio::test]
async fn claude_renews_into_the_same_keychain_item_and_keeps_other_fields() {
    let (url, server) = start(
        200,
        json!({"access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 3600}),
    )
    .await;
    let keychain = keychain();
    keychain
        .write(
            CLAUDE_SERVICE,
            "tester",
            &json!({
                "claudeAiOauth": {"accessToken": "old", "refreshToken": "rt", "expiresAt": now_ms() - 1000,
                                   "scopes": ["user:inference"], "subscriptionType": "max"},
                "mcpOAuth": {"keep": true},
            })
            .to_string(),
        )
        .unwrap();
    let (creds, root) = credentials(&url, keychain.clone());
    let token = creds.current(Harness::Claude).await.unwrap();
    assert_eq!(token.access_token, "new-access");
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    let saved: Value = serde_json::from_str(
        &keychain
            .read(CLAUDE_SERVICE, Some("tester"))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved["claudeAiOauth"]["refreshToken"], "new-refresh");
    assert_eq!(saved["claudeAiOauth"]["subscriptionType"], "max");
    assert_eq!(saved["mcpOAuth"]["keep"], true);
    assert!(saved["claudeAiOauth"]["expiresAt"].as_i64().unwrap() > now_ms());
    // Both of Claude Code's locks are gone again.
    assert!(!root.join("claude/.oauth_refresh.lock").exists());
    let mut legacy = std::fs::canonicalize(root.join("claude"))
        .unwrap()
        .into_os_string();
    legacy.push(".lock");
    assert!(!PathBuf::from(legacy).exists());
}

#[tokio::test]
async fn a_stale_claude_lock_is_taken_over() {
    let root = temp_dir();
    let lock = root.join("held.lock");
    std::fs::create_dir(&lock).unwrap();
    let old = SystemTime::now() - Duration::from_secs(120);
    std::fs::File::open(&lock)
        .unwrap()
        .set_modified(old)
        .unwrap();
    take_dir_lock(&lock).await.unwrap();
    assert!(lock.exists());
}

#[test]
fn tokens_never_show_in_debug_output() {
    let token = LoginToken {
        access_token: "secret-value".into(),
        account_id: None,
        expires_at_ms: None,
    };
    assert!(!format!("{token:?}").contains("secret-value"));
}
