//! Minimal Orca runtime RPC envelope compatibility.
//!
//! Aeroric and Orca expose different runtime surfaces.  The transport layer
//! can still be shared safely: parse Orca's string-id envelope, expose the
//! small set of host/introspection calls that have an unambiguous Aeroric
//! equivalent, and report every other method as unsupported.  Returning an
//! explicit error is important here; silently translating an Orca mutation to
//! an Aeroric operation could target the wrong task or worktree.

use crate::remote::state::RemoteState;
use serde_json::{json, Value};

pub async fn handle(state: &RemoteState, desktop_version: &str, raw: &str) -> String {
    let runtime_id = state.keys.host_id();
    let parsed = serde_json::from_str::<Value>(raw);
    let Ok(request) = parsed else {
        return failure(
            "",
            "invalid_argument",
            "Malformed Orca RPC request",
            &runtime_id,
        );
    };
    let Some(object) = request.as_object() else {
        return failure(
            "",
            "invalid_argument",
            "Orca RPC request must be an object",
            &runtime_id,
        );
    };
    let id = object.get("id").and_then(Value::as_str).unwrap_or("");
    if id.is_empty() {
        return failure("", "invalid_argument", "Missing Orca RPC id", &runtime_id);
    }
    let method = object.get("method").and_then(Value::as_str);
    let Some(method) = method.filter(|value| !value.is_empty()) else {
        return failure(
            id,
            "invalid_argument",
            "Missing Orca RPC method",
            &runtime_id,
        );
    };

    let result = match method {
        "status.get" => status(desktop_version, &runtime_id),
        "hello" => hello(desktop_version, &runtime_id),
        "ping" => Ok(json!("pong")),
        _ => Err((
            "method_not_found".to_string(),
            format!("Unknown method: {method}"),
        )),
    };
    match result {
        Ok(result) => success(id, result, &runtime_id),
        Err((code, message)) => failure(id, &code, &message, &runtime_id),
    }
}

fn status(desktop_version: &str, runtime_id: &str) -> Result<Value, (String, String)> {
    Ok(json!({
        "runtimeId": runtime_id,
        "rendererGraphEpoch": 0,
        "graphStatus": "ready",
        "authoritativeWindowId": Value::Null,
        "desktopWindowStatus": "available",
        "liveTabCount": 0,
        "liveLeafCount": 0,
        "runtimeProtocolVersion": 2,
        "minCompatibleRuntimeClientVersion": desktop_version.to_string(),
        "capabilities": [
            "aeroric.remote.v2",
            "aeroric.tasks.v1",
            "aeroric.terminal.v1"
        ],
        "hostPlatform": std::env::consts::OS,
        "terminalWindowsShell": Value::Null,
        "floatingWorkspaceEnabled": true,
        "protocolVersion": 2,
        "minCompatibleMobileVersion": desktop_version.to_string()
    }))
}

fn hello(desktop_version: &str, runtime_id: &str) -> Result<Value, (String, String)> {
    Ok(json!({
        "name": "aeroric",
        "version": desktop_version.to_string(),
        "platform": std::env::consts::OS,
        "hostId": runtime_id
    }))
}

fn success(id: &str, result: Value, runtime_id: &str) -> String {
    json!({
        "id": id,
        "ok": true,
        "result": result,
        "_meta": { "runtimeId": runtime_id }
    })
    .to_string()
}

fn failure(id: &str, code: &str, message: &str, runtime_id: &str) -> String {
    json!({
        "id": id,
        "ok": false,
        "error": { "code": code, "message": message },
        "_meta": { "runtimeId": runtime_id }
    })
    .to_string()
}

// 测试（unsupported envelope/runtime metadata 两条）随刀三b 测试片迁 core tests/remote_headless.rs。
