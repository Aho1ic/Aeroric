//! The local login as a virtual gateway connection (2026-10-30).

use serde_json::json;

use super::model::{ConnectionsDocument, Harness};
use super::routing;

fn doc(login_on: bool, accepted: bool) -> ConnectionsDocument {
    serde_json::from_value(json!({
        "version": 1,
        "connections": [{
            "id": "ds", "label": "DeepSeek", "kind": "api_key", "vendor": "other",
            "base_url": "https://api.deepseek.com", "protocols": ["chat_completions"],
            "models": [{"upstream_id": "deepseek-v4.1-flash", "enabled": true}],
        }],
        "login_gateway": {
            "codex": {
                "enabled": login_on,
                "accepted_at": if accepted { json!("2026-10-30T00:00:00Z") } else { json!(null) },
                "models": [{"upstream_id": "gpt-5.5-codex", "enabled": true}],
            },
        },
    }))
    .unwrap()
}

fn served_by(doc: &ConnectionsDocument, config_id: &str, model: &str) -> Vec<String> {
    let config = doc.config(config_id).unwrap();
    routing::candidates(doc, &config, model)
        .0
        .into_iter()
        .map(|c| c.connection_id)
        .collect()
}

#[test]
fn off_changes_nothing_and_writes_nothing() {
    let plain: ConnectionsDocument = serde_json::from_value(json!({"version": 1})).unwrap();
    assert_eq!(plain.clone().with_logins(), plain);
    assert!(!serde_json::to_string(&plain)
        .unwrap()
        .contains("login_gateway"));
    let off = doc(false, true);
    assert_eq!(off.clone().with_logins(), off);
    assert!(off
        .clone()
        .with_logins()
        .config("via-codex-login")
        .is_none());
}

#[test]
fn turned_on_without_accepting_is_still_off() {
    let doc = doc(true, false);
    assert!(!doc.login_gateway.active(Harness::Codex));
    assert_eq!(doc.clone().with_logins(), doc);
}

#[test]
fn the_login_configuration_routes_the_login_and_third_party_models() {
    let doc = doc(true, true).with_logins();
    let config = doc.config("via-codex-login").unwrap();
    assert_eq!(config.label, "本机登录");
    assert_eq!(config.default_model, "gpt-5.5-codex");
    assert_eq!(
        served_by(&doc, "via-codex-login", "gpt-5.5-codex"),
        ["login-codex"]
    );
    assert_eq!(
        served_by(&doc, "via-codex-login", "deepseek-v4.1-flash"),
        ["ds"]
    );
}

#[test]
fn the_login_serves_only_its_own_harness_login_configuration() {
    let doc = doc(true, true).with_logins();
    // 「全部第三方模型」 stays third-party only.
    assert!(served_by(&doc, "via-codex-all", "gpt-5.5-codex").is_empty());
    // D1: Claude never gets the ChatGPT login, and has no login of its own here.
    assert!(doc.config("via-claude-login").is_none());
    assert!(served_by(&doc, "via-claude-all", "gpt-5.5-codex").is_empty());
    // No per-connection entry is derived from a login.
    assert!(doc
        .connection_configs()
        .iter()
        .all(|c| c.label != "本机登录 · Codex"),);
}

#[test]
fn a_new_builtin_session_uses_the_login_configuration_only_while_it_is_on() {
    let data = std::env::temp_dir()
        .join("aeroric-login-launch")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&data).unwrap();
    let path = super::store::document_path(&data);
    let write = |doc: &ConnectionsDocument| {
        std::fs::write(&path, serde_json::to_string(doc).unwrap()).unwrap();
    };
    write(&doc(true, true));
    assert_eq!(
        super::launch::login_config_for(&data, "codex").as_deref(),
        Some("via-codex-login")
    );
    assert_eq!(super::launch::login_config_for(&data, "claude"), None);
    assert_eq!(super::launch::login_config_for(&data, "omp"), None);
    write(&doc(false, true));
    assert_eq!(super::launch::login_config_for(&data, "codex"), None);
    // The virtual connection is never written back.
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("login-codex"));
}

#[test]
fn under_the_login_configuration_the_login_takes_models_nobody_else_serves() {
    let doc = doc(true, true).with_logins();
    // Claude Code-style full ids are not in the login's list.
    assert_eq!(
        served_by(&doc, "via-codex-login", "gpt-5.5-codex-mini"),
        ["login-codex"]
    );
    // A third-party model still goes to its channel, not the login.
    assert_eq!(
        served_by(&doc, "via-codex-login", "deepseek-v4.1-flash"),
        ["ds"]
    );
    // Elsewhere there is no catch-all.
    assert!(served_by(&doc, "via-codex-all", "gpt-5.5-codex-mini").is_empty());
}
