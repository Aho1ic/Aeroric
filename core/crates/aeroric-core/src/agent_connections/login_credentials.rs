//! The harness's own local login as a gateway credential (2026-10-30, task
//! `10-29-local-login-gateway`, D2/D8).
//!
//! The gateway reads the login the user's CLI already holds: Codex's
//! `auth.json`, Claude Code's keychain item. It renews a token only when it
//! is about to expire or was refused, and then under the CLI's own lock, after
//! reading again: the refresh token rotates on every renewal, so two
//! refreshers racing would sign the user out. Locks and formats follow the
//! CLIs as read on 2026-10-30 (`research/verify.md`):
//! - Codex: `flock` on `$CODEX_HOME/auth.lock`;
//! - Claude Code: proper-lockfile directories `~/.claude/.oauth_refresh.lock`
//!   and `<~/.claude>.lock` (60 s stale, mtime kept fresh every 5 s).
//!
//! Tokens never leave this module except as the bearer for one request; they
//! are not logged, recorded or shown (`LoginToken`'s `Debug` hides them).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::{json, Value};

use super::model::Harness;

/// Renew when less than this is left (Codex uses the same window).
const REFRESH_WINDOW_MS: i64 = 5 * 60 * 1000;
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLAUDE_SERVICE: &str = "Claude Code-credentials";
/// proper-lockfile's defaults as Claude Code sets them.
const CLAUDE_LOCK_STALE: Duration = Duration::from_secs(60);
const CLAUDE_LOCK_TOUCH: Duration = Duration::from_secs(5);
const CLAUDE_LOCK_WAIT: Duration = Duration::from_secs(20);

/// One usable credential for a single upstream request.
#[derive(Clone, PartialEq, Eq)]
pub struct LoginToken {
    pub access_token: String,
    /// ChatGPT workspace (`Chatgpt-Account-Id`); Codex only.
    pub account_id: Option<String>,
    /// Epoch milliseconds; `None` when the login does not say.
    pub expires_at_ms: Option<i64>,
}

impl std::fmt::Debug for LoginToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginToken")
            .field("access_token", &"<hidden>")
            .field("account_id", &self.account_id)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginError {
    /// No login on this machine (or it holds an API key, not a login).
    NotSignedIn,
    /// The login exists but cannot be read right now.
    Unreadable(String),
    /// The provider refused the refresh token: sign in again in the CLI.
    SignedOut,
    /// Renewal failed for another reason; worth trying again later.
    Refresh(String),
    /// Another process held the CLI's lock for too long.
    Busy,
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSignedIn => write!(f, "本机没有可用的登录"),
            Self::Unreadable(reason) => write!(f, "读不到本机登录：{reason}"),
            Self::SignedOut => write!(f, "本机登录已失效，请在 CLI 里重新登录"),
            Self::Refresh(reason) => write!(f, "续期失败：{reason}"),
            Self::Busy => write!(f, "另一个进程正在续期，稍后再试"),
        }
    }
}

/// Keychain access as Claude Code does it (`/usr/bin/security`), so the
/// item's access list keeps trusting the same program.
pub trait Keychain: Send + Sync {
    fn read(&self, service: &str, account: Option<&str>) -> Result<Option<String>, String>;
    fn write(&self, service: &str, account: &str, data: &str) -> Result<(), String>;
}

pub struct SecurityCli;

impl Keychain for SecurityCli {
    fn read(&self, service: &str, account: Option<&str>) -> Result<Option<String>, String> {
        let mut cmd = std::process::Command::new("/usr/bin/security");
        crate::runtime::subprocess::configure_background_command(&mut cmd);
        cmd.args(["find-generic-password", "-s", service]);
        if let Some(account) = account {
            cmd.args(["-a", account]);
        }
        let output = cmd
            .arg("-w")
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot run security: {e}"))?;
        if !output.status.success() {
            // 44 = errSecItemNotFound.
            return if output.status.code() == Some(44) {
                Ok(None)
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            };
        }
        String::from_utf8(output.stdout)
            .map(|raw| Some(raw.trim().to_string()))
            .map_err(|_| "keychain item is not UTF-8".to_string())
    }

    fn write(&self, service: &str, account: &str, data: &str) -> Result<(), String> {
        // `-X` takes hex, so the JSON never sits in argv as plain text.
        let mut cmd = std::process::Command::new("/usr/bin/security");
        crate::runtime::subprocess::configure_background_command(&mut cmd);
        let output = cmd
            .args([
                "add-generic-password",
                "-U",
                "-a",
                account,
                "-s",
                service,
                "-X",
            ])
            .arg(hex::encode(data))
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot run security: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

/// Where the logins live and where they are renewed; tests point these at
/// temporary directories and a fake token server.
#[derive(Clone)]
pub struct LoginPaths {
    pub codex_home: PathBuf,
    pub claude_dir: PathBuf,
    pub user: String,
    pub codex_token_url: String,
    pub claude_token_url: String,
}

impl LoginPaths {
    /// The real locations for `home` (the user's home directory).
    pub fn for_home(home: &Path) -> Self {
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let user = std::env::var("USER")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "claude-code-user".into());
        Self {
            codex_home,
            claude_dir: home.join(".claude"),
            user,
            codex_token_url: CODEX_TOKEN_URL.into(),
            claude_token_url: CLAUDE_TOKEN_URL.into(),
        }
    }
}

pub struct LoginCredentials {
    paths: LoginPaths,
    keychain: Arc<dyn Keychain>,
    http: reqwest::Client,
    cache: Mutex<HashMap<Harness, LoginToken>>,
    /// One renewal at a time per login inside this process.
    renewing: HashMap<Harness, tokio::sync::Mutex<()>>,
}

impl LoginCredentials {
    pub fn new(paths: LoginPaths, keychain: Arc<dyn Keychain>) -> Self {
        let renewing = [Harness::Codex, Harness::Claude]
            .into_iter()
            .map(|h| (h, tokio::sync::Mutex::new(())))
            .collect();
        Self {
            paths,
            keychain,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            cache: Mutex::new(HashMap::new()),
            renewing,
        }
    }

    /// A token for the next request, renewed first when it is about to expire.
    pub async fn current(&self, harness: Harness) -> Result<LoginToken, LoginError> {
        if let Some(token) = self.cached(harness).filter(|t| !expiring(t)) {
            return Ok(token);
        }
        let token = self.read(harness).await?;
        if !expiring(&token) {
            self.remember(harness, &token);
            return Ok(token);
        }
        self.renew(harness, &token.access_token).await
    }

    /// The upstream refused `stale`: use what the CLI renewed meanwhile, or
    /// renew it here.
    pub async fn rejected(&self, harness: Harness, stale: &str) -> Result<LoginToken, LoginError> {
        self.forget(harness);
        self.renew(harness, stale).await
    }

    fn cached(&self, harness: Harness) -> Option<LoginToken> {
        self.cache.lock().ok()?.get(&harness).cloned()
    }

    fn remember(&self, harness: Harness, token: &LoginToken) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(harness, token.clone());
        }
    }

    fn forget(&self, harness: Harness) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&harness);
        }
    }

    async fn read(&self, harness: Harness) -> Result<LoginToken, LoginError> {
        match harness {
            Harness::Codex => codex_token(&read_codex(&self.paths.codex_home)?),
            Harness::Claude => claude_token(&self.read_claude().await?),
            _ => Err(LoginError::NotSignedIn),
        }
    }

    async fn renew(&self, harness: Harness, stale: &str) -> Result<LoginToken, LoginError> {
        let Some(gate) = self.renewing.get(&harness) else {
            return Err(LoginError::NotSignedIn);
        };
        let _one = gate.lock().await;
        let token = match harness {
            Harness::Codex => self.renew_codex(stale).await?,
            Harness::Claude => self.renew_claude(stale).await?,
            _ => return Err(LoginError::NotSignedIn),
        };
        self.remember(harness, &token);
        Ok(token)
    }

    async fn renew_codex(&self, stale: &str) -> Result<LoginToken, LoginError> {
        let home = self.paths.codex_home.clone();
        let _lock = tokio::task::spawn_blocking(move || flock(&home.join("auth.lock")))
            .await
            .map_err(|e| LoginError::Unreadable(e.to_string()))??;
        // Read again under the lock: Codex may have renewed it already.
        let auth = read_codex(&self.paths.codex_home)?;
        let token = codex_token(&auth)?;
        if token.access_token != stale && !expiring(&token) {
            return Ok(token);
        }
        let refresh = auth
            .pointer("/tokens/refresh_token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or(LoginError::SignedOut)?
            .to_string();
        let reply = self
            .post_refresh(
                &self.paths.codex_token_url,
                json!({
                    "client_id": CODEX_CLIENT_ID,
                    "grant_type": "refresh_token",
                    "refresh_token": refresh,
                    "scope": "openid profile email",
                }),
            )
            .await?;
        let access = reply_str(&reply, "access_token")?;
        if access == stale {
            return Err(LoginError::Refresh("续期后拿到的还是同一个令牌".into()));
        }
        let mut auth = auth;
        let tokens = auth
            .get_mut("tokens")
            .and_then(Value::as_object_mut)
            .ok_or(LoginError::SignedOut)?;
        tokens.insert("access_token".into(), Value::String(access));
        for key in ["refresh_token", "id_token"] {
            if let Some(value) = reply.get(key).and_then(Value::as_str) {
                tokens.insert(key.into(), Value::String(value.to_string()));
            }
        }
        if let Some(object) = auth.as_object_mut() {
            object.insert("last_refresh".into(), Value::String(rfc3339_now()));
        }
        write_private(&self.paths.codex_home.join("auth.json"), &auth)?;
        codex_token(&auth)
    }

    async fn read_claude(&self) -> Result<Value, LoginError> {
        let keychain = self.keychain.clone();
        let user = self.paths.user.clone();
        let raw = tokio::task::spawn_blocking(move || {
            // This user's item first (cc-switch e5b04221), then any.
            match keychain.read(CLAUDE_SERVICE, Some(&user))? {
                Some(raw) => Ok(Some(raw)),
                None => keychain.read(CLAUDE_SERVICE, None),
            }
        })
        .await
        .map_err(|e| LoginError::Unreadable(e.to_string()))?
        .map_err(LoginError::Unreadable)?
        .ok_or(LoginError::NotSignedIn)?;
        serde_json::from_str(&raw).map_err(|_| LoginError::Unreadable("钥匙串内容不是 JSON".into()))
    }

    async fn renew_claude(&self, stale: &str) -> Result<LoginToken, LoginError> {
        let lock = ClaudeLock::acquire(&self.paths.claude_dir).await?;
        let result = self.renew_claude_locked(stale).await;
        lock.release();
        result
    }

    async fn renew_claude_locked(&self, stale: &str) -> Result<LoginToken, LoginError> {
        let mut data = self.read_claude().await?;
        let token = claude_token(&data)?;
        if token.access_token != stale && !expiring(&token) {
            return Ok(token);
        }
        let oauth = data
            .get("claudeAiOauth")
            .and_then(Value::as_object)
            .ok_or(LoginError::NotSignedIn)?;
        let refresh = oauth
            .get("refreshToken")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or(LoginError::SignedOut)?
            .to_string();
        let client_id = oauth
            .get("clientId")
            .and_then(Value::as_str)
            .unwrap_or(CLAUDE_CLIENT_ID)
            .to_string();
        let mut body = json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh,
            "client_id": client_id,
        });
        if let Some(scopes) = oauth.get("scopes").and_then(Value::as_array) {
            let scopes: Vec<&str> = scopes.iter().filter_map(Value::as_str).collect();
            if !scopes.is_empty() {
                body["scope"] = Value::String(scopes.join(" "));
            }
        }
        let reply = self
            .post_refresh(&self.paths.claude_token_url, body)
            .await?;
        let access = reply_str(&reply, "access_token")?;
        if access == stale {
            return Err(LoginError::Refresh("续期后拿到的还是同一个令牌".into()));
        }
        let oauth = data
            .get_mut("claudeAiOauth")
            .and_then(Value::as_object_mut)
            .ok_or(LoginError::NotSignedIn)?;
        oauth.insert("accessToken".into(), Value::String(access));
        if let Some(value) = reply.get("refresh_token").and_then(Value::as_str) {
            oauth.insert("refreshToken".into(), Value::String(value.to_string()));
        }
        if let Some(seconds) = reply.get("expires_in").and_then(Value::as_i64) {
            oauth.insert("expiresAt".into(), json!(now_ms() + seconds * 1000));
        }
        let keychain = self.keychain.clone();
        let user = self.paths.user.clone();
        let text = data.to_string();
        tokio::task::spawn_blocking(move || keychain.write(CLAUDE_SERVICE, &user, &text))
            .await
            .map_err(|e| LoginError::Refresh(e.to_string()))?
            .map_err(LoginError::Refresh)?;
        claude_token(&data)
    }

    async fn post_refresh(&self, url: &str, body: Value) -> Result<Value, LoginError> {
        let response = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| LoginError::Refresh(e.without_url().to_string()))?;
        let status = response.status();
        let reply: Value = response.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(reply);
        }
        let code = reply
            .get("error")
            .map(|e| e.get("code").or(e.get("type")).unwrap_or(e))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if refresh_token_dead(code) || status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LoginError::SignedOut);
        }
        Err(LoginError::Refresh(format!("HTTP {}", status.as_u16())))
    }
}

/// Error codes meaning the refresh token itself is gone.
fn refresh_token_dead(code: &str) -> bool {
    matches!(
        code,
        "invalid_grant"
            | "refresh_token_expired"
            | "refresh_token_reused"
            | "refresh_token_invalidated"
    )
}

fn reply_str(reply: &Value, key: &str) -> Result<String, LoginError> {
    reply
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| LoginError::Refresh(format!("续期回复里没有 {key}")))
}

fn read_codex(home: &Path) -> Result<Value, LoginError> {
    let path = home.join("auth.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LoginError::NotSignedIn),
        Err(e) => return Err(LoginError::Unreadable(e.to_string())),
    };
    serde_json::from_str(&raw).map_err(|_| LoginError::Unreadable("auth.json 不是 JSON".into()))
}

fn codex_token(auth: &Value) -> Result<LoginToken, LoginError> {
    let tokens = auth
        .get("tokens")
        .filter(|t| !t.is_null())
        .ok_or(LoginError::NotSignedIn)?;
    let access = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or(LoginError::NotSignedIn)?;
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            tokens
                .get("id_token")
                .and_then(Value::as_str)
                .and_then(jwt_claims)
                .and_then(|c| {
                    c.pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        });
    Ok(LoginToken {
        access_token: access.to_string(),
        account_id,
        expires_at_ms: jwt_claims(access)
            .and_then(|c| c.get("exp").and_then(Value::as_i64))
            .map(|s| s * 1000),
    })
}

fn claude_token(data: &Value) -> Result<LoginToken, LoginError> {
    let oauth = data.get("claudeAiOauth").ok_or(LoginError::NotSignedIn)?;
    let access = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or(LoginError::NotSignedIn)?;
    Ok(LoginToken {
        access_token: access.to_string(),
        account_id: None,
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_i64),
    })
}

fn expiring(token: &LoginToken) -> bool {
    token
        .expires_at_ms
        .is_some_and(|at| at - now_ms() < REFRESH_WINDOW_MS)
}

fn jwt_claims(jwt: &str) -> Option<Value> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Write through a temporary file and a rename, keeping it 0600: Codex opens
/// the file afresh on every read and must never see half of it.
fn write_private(path: &Path, value: &Value) -> Result<(), LoginError> {
    let text =
        serde_json::to_string_pretty(value).map_err(|e| LoginError::Refresh(e.to_string()))?;
    let temp = path.with_extension(format!("json.aeroric-{}", std::process::id()));
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        LoginError::Refresh(format!("写回登录失败：{e}"))
    })
}

/// Codex's lock: an exclusive `flock` held while the file stays open.
fn flock(path: &Path) -> Result<std::fs::File, LoginError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| LoginError::Unreadable(format!("打不开 {}：{e}", path.display())))?;
    file.lock()
        .map_err(|e| LoginError::Unreadable(e.to_string()))?;
    Ok(file)
}

/// Claude Code's two proper-lockfile locks, taken in its order.
struct ClaudeLock {
    dirs: Vec<PathBuf>,
    toucher: tokio::task::JoinHandle<()>,
}

impl ClaudeLock {
    async fn acquire(claude_dir: &Path) -> Result<Self, LoginError> {
        std::fs::create_dir_all(claude_dir).map_err(|e| LoginError::Unreadable(e.to_string()))?;
        let real = std::fs::canonicalize(claude_dir).unwrap_or_else(|_| claude_dir.to_path_buf());
        let mut legacy = real.into_os_string();
        legacy.push(".lock");
        let wanted = [
            claude_dir.join(".oauth_refresh.lock"),
            PathBuf::from(legacy),
        ];
        let mut held: Vec<PathBuf> = Vec::new();
        for dir in wanted {
            if let Err(e) = take_dir_lock(&dir).await {
                for taken in &held {
                    let _ = std::fs::remove_dir(taken);
                }
                return Err(e);
            }
            held.push(dir);
        }
        let touched = held.clone();
        let toucher = tokio::spawn(async move {
            loop {
                tokio::time::sleep(CLAUDE_LOCK_TOUCH).await;
                for dir in &touched {
                    touch(dir);
                }
            }
        });
        Ok(Self {
            dirs: held,
            toucher,
        })
    }

    fn release(self) {
        self.toucher.abort();
        for dir in self.dirs.iter().rev() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

async fn take_dir_lock(dir: &Path) -> Result<(), LoginError> {
    let started = std::time::Instant::now();
    loop {
        match std::fs::create_dir(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(dir)
                    .and_then(|m| m.modified())
                    .map(|at| at.elapsed().unwrap_or_default() > CLAUDE_LOCK_STALE)
                    .unwrap_or(false);
                if stale {
                    // Its holder died; proper-lockfile takes over the same way.
                    let _ = std::fs::remove_dir(dir);
                    continue;
                }
                if started.elapsed() > CLAUDE_LOCK_WAIT {
                    return Err(LoginError::Busy);
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => return Err(LoginError::Unreadable(format!("加锁失败：{e}"))),
        }
    }
}

fn touch(dir: &Path) {
    if let Ok(file) = std::fs::File::open(dir) {
        let _ = file.set_modified(SystemTime::now());
    }
}

static SHARED: std::sync::RwLock<Option<Arc<LoginCredentials>>> = std::sync::RwLock::new(None);

/// The gateway's credentials: the real logins of the user running the host,
/// read with `/usr/bin/security`; `install` swaps them (tests).
pub fn shared() -> Arc<LoginCredentials> {
    if let Some(existing) = SHARED.read().ok().and_then(|s| s.clone()) {
        return existing;
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let fresh = Arc::new(LoginCredentials::new(
        LoginPaths::for_home(&home),
        Arc::new(SecurityCli),
    ));
    match SHARED.write() {
        Ok(mut slot) => slot.get_or_insert(fresh).clone(),
        Err(_) => fresh,
    }
}

pub fn install(credentials: Arc<LoginCredentials>) {
    if let Ok(mut slot) = SHARED.write() {
        *slot = Some(credentials);
    }
}

/// For tests and the status card: what the login looks like right now,
/// without renewing it.
pub async fn peek(
    credentials: &LoginCredentials,
    harness: Harness,
) -> Result<LoginToken, LoginError> {
    credentials.read(harness).await
}

#[cfg(test)]
#[path = "login_credentials_tests.rs"]
mod tests;
