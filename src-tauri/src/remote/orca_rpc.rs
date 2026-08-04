//! Minimal Orca runtime RPC envelope compatibility.
//!
//! Aeroric and Orca expose different runtime surfaces.  The transport layer
//! can still be shared safely: parse Orca's string-id envelope, expose the
//! small set of host/introspection calls that have an unambiguous Aeroric
//! equivalent, and report every other method as unsupported.  Returning an
//! explicit error is important here; silently translating an Orca mutation to
//! an Aeroric operation could target the wrong task or worktree.

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use super::RemoteState;

pub(crate) async fn handle<R: Runtime>(app: &AppHandle<R>, raw: &str) -> String {
    let runtime_id = app.state::<RemoteState>().keys.host_id();
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
        "status.get" => status(app, &runtime_id),
        "hello" => hello(app, &runtime_id),
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

fn status<R: Runtime>(app: &AppHandle<R>, runtime_id: &str) -> Result<Value, (String, String)> {
    let info = app.package_info();
    Ok(json!({
        "runtimeId": runtime_id,
        "rendererGraphEpoch": 0,
        "graphStatus": "ready",
        "authoritativeWindowId": Value::Null,
        "desktopWindowStatus": "available",
        "liveTabCount": 0,
        "liveLeafCount": 0,
        "runtimeProtocolVersion": 2,
        "minCompatibleRuntimeClientVersion": info.version.to_string(),
        "capabilities": [
            "aeroric.remote.v2",
            "aeroric.tasks.v1",
            "aeroric.terminal.v1"
        ],
        "hostPlatform": std::env::consts::OS,
        "terminalWindowsShell": Value::Null,
        "floatingWorkspaceEnabled": true,
        "protocolVersion": 2,
        "minCompatibleMobileVersion": info.version.to_string()
    }))
}

fn hello<R: Runtime>(app: &AppHandle<R>, runtime_id: &str) -> Result<Value, (String, String)> {
    let info = app.package_info();
    Ok(json!({
        "name": "aeroric",
        "version": info.version.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_method_uses_orca_failure_envelope() {
        let app = tauri::test::mock_app();
        app.manage(RemoteState::new_in_memory());
        let response = tauri::async_runtime::block_on(handle(
            &app.handle().clone(),
            r#"{"id":"rpc-1","method":"worktree.create","params":{}}"#,
        ));
        let value: Value = serde_json::from_str(&response).expect("valid response");
        assert_eq!(value["id"], "rpc-1");
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "method_not_found");
        assert!(value["_meta"]["runtimeId"].as_str().is_some());
    }

    #[test]
    fn status_has_runtime_metadata() {
        let app = tauri::test::mock_app();
        app.manage(RemoteState::new_in_memory());
        let response = tauri::async_runtime::block_on(handle(
            &app.handle().clone(),
            r#"{"id":"rpc-2","method":"status.get"}"#,
        ));
        let value: Value = serde_json::from_str(&response).expect("valid response");
        assert_eq!(value["ok"], true);
        assert_eq!(value["result"]["graphStatus"], "ready");
        assert_eq!(value["result"]["runtimeId"], value["_meta"]["runtimeId"]);
    }
}
