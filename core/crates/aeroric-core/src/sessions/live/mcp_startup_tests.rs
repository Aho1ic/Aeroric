use super::*;
use serde_json::json;

fn message(update: Option<SessionUpdateKind>) -> (String, String) {
    match update {
        Some(SessionUpdateKind::Notice { level, message }) => (level, message),
        other => panic!("not a notice: {other:?}"),
    }
}

/// A failed start names the server, how long it took and Codex's reason;
/// our own server also says which tools the turn goes without.
#[test]
fn a_failed_codex_start_names_the_server_the_time_and_the_reason() {
    let mut startups = CodexStartups::default();
    let at = Instant::now();
    let start = |name: &str| json!({"name": name, "status": "starting"});
    assert!(startups.observe(&start(SERVER_NAME), at).is_none());
    let (level, text) = message(startups.observe(
        &json!({"name": SERVER_NAME, "status": "failed",
            "error": "handshake timed out after 10s"}),
        at + Duration::from_millis(10_400),
    ));
    assert_eq!(level, "error");
    assert!(
        text.contains("Aeroric Context 工具（aeroric_context）"),
        "{text}"
    );
    assert!(text.contains("启动 10.4 秒后"), "{text}");
    assert!(text.contains("handshake timed out after 10s"), "{text}");
    assert!(text.contains("用不了记忆"), "{text}");

    startups.observe(&start("github"), at);
    let (_, text) = message(startups.observe(
        &json!({"name": "github", "status": "failed",
            "failureReason": "reauthenticationRequired"}),
        at + Duration::from_secs(2),
    ));
    assert!(
        text.starts_with("MCP 服务 github 没起来（启动 2 秒后）"),
        "{text}"
    );
    assert!(
        text.contains("需要重新登录") && !text.contains("Aeroric 工具"),
        "{text}"
    );
}

/// A quick start is not said; a slow one is, and a cancel is a warning.
#[test]
fn a_slow_start_and_a_cancel_are_said_a_quick_one_is_not() {
    let mut startups = CodexStartups::default();
    let at = Instant::now();
    let start = json!({"name": "linear", "status": "starting"});
    startups.observe(&start, at);
    assert!(startups
        .observe(
            &json!({"name": "linear", "status": "ready"}),
            at + Duration::from_secs(1)
        )
        .is_none());
    startups.observe(&start, at);
    let (level, text) = message(startups.observe(
        &json!({"name": "linear", "status": "ready"}),
        at + Duration::from_secs(7),
    ));
    assert_eq!(
        (level.as_str(), text.as_str()),
        ("info", "MCP 服务 linear 用了 7 秒才准备好")
    );
    let (level, text) =
        message(startups.observe(&json!({"name": "linear", "status": "cancelled"}), at));
    assert_eq!(level, "warning");
    assert_eq!(text, "MCP 服务 linear 的启动被取消了");
    assert!(startups.observe(&json!({"status": "failed"}), at).is_none());
}

/// Claude repeats `mcp_servers` on every turn: a failure is said once,
/// and again only after the server connected in between.
#[test]
fn a_claude_server_that_failed_is_said_once_until_it_connects() {
    let mut startups = ClaudeStartups::default();
    let init = |status: &str| {
        json!({"type": "system", "subtype": "init", "mcp_servers": [
            {"name": SERVER_NAME, "status": status},
            {"name": "github", "status": "connected"}]})
    };
    let first = startups.observe(&init("failed"));
    assert_eq!(first.len(), 1);
    let (level, text) = message(first.into_iter().next());
    assert_eq!(level, "error");
    assert!(
        text.contains("aeroric_context") && text.contains("failed"),
        "{text}"
    );
    assert!(startups.observe(&init("failed")).is_empty());
    assert!(startups.observe(&init("connected")).is_empty());
    assert_eq!(startups.observe(&init("failed")).len(), 1);
    let (level, _) = message(startups.observe(&init("needs-auth")).into_iter().next());
    assert_eq!(level, "warning");
    assert!(startups.observe(&init("pending")).is_empty());
    assert!(startups.observe(&json!({"subtype": "init"})).is_empty());
}
