//! G1: what the user's own Codex / Claude Code installs are signed in as.
//! Read-only and on demand; tokens are never returned or copied. A task with
//! no agent configuration launches the CLI on exactly this login.

use std::path::Path;

use base64::Engine;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OfficialLogin {
    pub installed: bool,
    pub signed_in: bool,
    /// `chatgpt` | `apikey` | `subscription` | `none`.
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Files read to answer, for the "what will be read" notice.
    pub sources: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct OfficialStatus {
    pub codex: OfficialLogin,
    pub claude: OfficialLogin,
}

/// `home` is the user's home directory. `installed` answers whether a CLI is
/// on the login PATH; `keychain_item` whether the Claude Code credential item
/// exists (attributes only, never its secret).
pub fn detect(
    home: &Path,
    installed: impl Fn(&str) -> bool,
    keychain_item: impl Fn() -> bool,
) -> OfficialStatus {
    OfficialStatus {
        codex: codex(home, installed("codex")),
        claude: claude(home, installed("claude"), keychain_item),
    }
}

fn codex(home: &Path, installed: bool) -> OfficialLogin {
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let auth_path = codex_home.join("auth.json");
    let auth: Option<Value> = std::fs::read_to_string(&auth_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());
    let has_tokens = auth
        .as_ref()
        .and_then(|a| a.get("tokens"))
        .is_some_and(|t| !t.is_null());
    let has_key = auth
        .as_ref()
        .and_then(|a| a.get("OPENAI_API_KEY"))
        .and_then(Value::as_str)
        .is_some_and(|k| !k.trim().is_empty());
    let mode = if has_tokens {
        "chatgpt"
    } else if has_key {
        "apikey"
    } else {
        "none"
    };
    let account = auth
        .as_ref()
        .and_then(|a| a.pointer("/tokens/id_token"))
        .and_then(Value::as_str)
        .and_then(jwt_email);
    OfficialLogin {
        installed,
        signed_in: has_tokens || has_key,
        mode: mode.into(),
        account,
        sources: vec![auth_path.display().to_string()],
    }
}

fn claude(home: &Path, installed: bool, keychain_item: impl Fn() -> bool) -> OfficialLogin {
    let config_path = home.join(".claude.json");
    let account = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| {
            v.pointer("/oauthAccount/emailAddress")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let credentials_file = home.join(".claude").join(".credentials.json");
    let signed_in = keychain_item() || credentials_file.is_file();
    OfficialLogin {
        installed,
        signed_in,
        mode: if signed_in { "subscription" } else { "none" }.into(),
        account,
        sources: vec![
            config_path.display().to_string(),
            credentials_file.display().to_string(),
            "钥匙串条目“Claude Code-credentials”（只看是否存在）".into(),
        ],
    }
}

/// The `email` claim of a JWT, without verifying it (display only).
fn jwt_email(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .get("email")
        .or_else(|| claims.pointer("/https:~1~1api.openai.com~1profile/email"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_logins_without_returning_tokens() {
        let home = std::env::temp_dir().join(format!("aeroric-official-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"email":"me@example.com"}"#);
        std::fs::write(
            home.join(".codex/auth.json"),
            format!(r#"{{"auth_mode":"chatgpt","tokens":{{"id_token":"h.{claims}.s","access_token":"SECRET"}}}}"#),
        )
        .unwrap();
        std::fs::write(
            home.join(".claude.json"),
            r#"{"oauthAccount":{"emailAddress":"c@example.com"}}"#,
        )
        .unwrap();
        let status = detect(&home, |cli| cli == "codex", || true);
        assert!(status.codex.installed && status.codex.signed_in);
        assert_eq!(status.codex.mode, "chatgpt");
        assert_eq!(status.codex.account.as_deref(), Some("me@example.com"));
        assert!(!status.claude.installed);
        assert!(status.claude.signed_in);
        assert_eq!(status.claude.account.as_deref(), Some("c@example.com"));
        let wire = serde_json::to_string(&status).unwrap();
        assert!(!wire.contains("SECRET"));
        let empty = detect(&home.join("nobody"), |_| false, || false);
        assert_eq!(empty.codex.mode, "none");
        assert!(!empty.claude.signed_in);
        std::fs::remove_dir_all(&home).ok();
    }
}
