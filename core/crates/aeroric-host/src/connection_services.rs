//! AP2 / G43: Mac 宿主侧的 SSH 连接、凭据 (`SecretStore`)、HostKey 与 WSL 退休拒绝服务。
//!
//! 核心契约：
//! 1. 连接配置落在 `data_dir/ssh-connections.json`，明文密码绝不落入该文件或列表回包（由
//!    `SshConnection::password` 的 `skip_serializing` 保证）。
//! 2. 密码经 `CoreContext::secrets`（`SecretKey("ssh-password:<id>")`）存取；支持从
//!    legacy `ssh-passwords.json` 读时迁移，且编辑非密码字段（`password: None`）保留已有密码。
//! 3. 所有写操作（save/delete/trust）先检查 `context.is_writer()`，只读宿主在产生任何副作用前
//!    返回 `PermissionDenied`。
//! 4. WSL 相关方法显式返回 `UnsupportedCapability`（F12 退休），绝不伪装成空成功或回退本地。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::connections::ssh::{
    fingerprint_of, parse_scan_output, SshConnection, SshHostKeyStatus,
};
use aeroric_core::context::{CoreContext, SecretKey};
use aeroric_protocol::envelope::{Frame, Request, Success};
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};

static SSH_STORAGE_LOCK: Mutex<()> = Mutex::new(());

fn ok(request: &Request, result: Value) -> Frame {
    Frame::Success(Success {
        v: 1,
        id: request.id.clone(),
        ok: true,
        result,
        extra: Default::default(),
    })
}

fn ssh_connections_file(context: &CoreContext) -> PathBuf {
    context.paths.data_dir().join("ssh-connections.json")
}

fn legacy_ssh_passwords_file(context: &CoreContext) -> PathBuf {
    context.paths.data_dir().join("ssh-passwords.json")
}

fn known_hosts_file(context: &CoreContext) -> PathBuf {
    context.paths.data_dir().join("known_hosts")
}

fn secret_key_for(connection_id: &str) -> SecretKey {
    SecretKey(format!("ssh-password:{connection_id}"))
}

fn load_legacy_passwords(context: &CoreContext) -> BTreeMap<String, String> {
    let path = legacy_ssh_passwords_file(context);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<BTreeMap<String, String>>(&raw).ok())
        .unwrap_or_default()
}

fn has_password_for(
    context: &CoreContext,
    connection_id: &str,
    legacy: &BTreeMap<String, String>,
) -> bool {
    if let Ok(Some(pw)) = context.secrets.get(&secret_key_for(connection_id)) {
        if !pw.is_empty() {
            return true;
        }
    }
    legacy
        .get(connection_id)
        .map(|pw| !pw.is_empty())
        .unwrap_or(false)
}

fn read_connections_raw(context: &CoreContext) -> Result<Vec<SshConnection>, String> {
    let path = ssh_connections_file(context);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<SshConnection>>(&raw).map_err(|e| e.to_string())
}

fn write_private_file(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn connection_id_param(params: &Value) -> Option<String> {
    params
        .get("connectionId")
        .or_else(|| params.get("connection_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn register_connection_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    {
        let context = Arc::clone(&context);
        set.register(
            "connections.load_ssh_connections",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { load_ssh_connections(request, &context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "connections.get_ssh_connection_password",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { get_ssh_connection_password(request, &context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "connections.save_ssh_connections",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { save_ssh_connections(request, &context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "connections.delete_ssh_connection",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { delete_ssh_connection(request, &context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "connections.check_ssh_host_key",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { check_ssh_host_key(request, &context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "connections.trust_ssh_host_key",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { trust_ssh_host_key(request, &context) })
            }),
        );
    }

    // F12 (WSL) 退休：所有 WSL 方法显式返回 UnsupportedCapability，不假装成功或回退本地。
    const RETIRED_WSL_METHODS: &[&str] = &[
        "connections.get_wsl_status",
        "connections.list_wsl_distributions",
        "connections.probe_wsl_distribution",
        "connections.read_wsl_environment",
        "connections.load_wsl_settings",
        "connections.save_wsl_settings",
        "connections.read_wsl_config_file",
        "connections.write_wsl_config_file",
        "connections.get_wsl_agent_status",
        "connections.upgrade_wsl_agent_versions",
        "connections.read_wsl_agent_config",
        "connections.write_wsl_agent_config",
        "connections.restart_wsl",
        "connections.validate_wsl_project_path",
        "connections.open_wsl_shell",
        "connections.kill_wsl_shell",
        "connections.run_wsl_task",
        "connections.resume_wsl_task",
        "connections.cancel_wsl_task",
    ];
    for method in RETIRED_WSL_METHODS {
        let method_name = (*method).to_string();
        set.register(
            method,
            Arc::new(move |request, _call| {
                let method_name = method_name.clone();
                Box::pin(async move {
                    failure(
                        request.id,
                        ErrorCode::UnsupportedCapability,
                        &format!(
                            "{method_name} is retired on the Apple-only platform (F12 WSL retired); existing records remain preserved as read-only"
                        ),
                    )
                })
            }),
        );
    }
}

fn load_ssh_connections(request: Request, context: &CoreContext) -> Frame {
    let _guard = SSH_STORAGE_LOCK.lock();
    let mut connections = match read_connections_raw(context) {
        Ok(list) => list,
        Err(e) => return failure(request.id, ErrorCode::IoError, &e),
    };
    let legacy = load_legacy_passwords(context);
    for conn in &mut connections {
        conn.password = None;
        conn.has_password = has_password_for(context, &conn.id, &legacy);
    }
    match serde_json::to_value(connections) {
        Ok(val) => ok(&request, val),
        Err(e) => failure(request.id, ErrorCode::IoError, &e.to_string()),
    }
}

fn get_ssh_connection_password(request: Request, context: &CoreContext) -> Frame {
    let _guard = SSH_STORAGE_LOCK.lock();
    let Some(connection_id) = connection_id_param(&request.params) else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "connections.get_ssh_connection_password requires params.connectionId",
        );
    };
    let key = secret_key_for(&connection_id);
    match context.secrets.get(&key) {
        Ok(Some(pw)) if !pw.is_empty() => return ok(&request, Value::String(pw)),
        Ok(_) => {}
        Err(e) => return failure(request.id, ErrorCode::IoError, &e),
    }
    let legacy = load_legacy_passwords(context);
    if let Some(pw) = legacy.get(&connection_id).filter(|s| !s.is_empty()) {
        // Migrate-on-read into SecretStore when writer-capable.
        if context.require_write().is_ok() {
            let _ = context.secrets.set(&key, pw);
        }
        return ok(&request, Value::String(pw.clone()));
    }
    ok(&request, Value::Null)
}

fn save_ssh_connections(request: Request, context: &CoreContext) -> Frame {
    if let Err(err) = context.require_write() {
        return failure(request.id, ErrorCode::IoError, &err.to_string());
    }
    let Some(raw_list) = request.params.get("connections") else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "connections.save_ssh_connections requires params.connections (array)",
        );
    };
    let mut incoming: Vec<SshConnection> = match serde_json::from_value(raw_list.clone()) {
        Ok(list) => list,
        Err(e) => {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                &format!("invalid connections payload: {e}"),
            )
        }
    };
    let _guard = SSH_STORAGE_LOCK.lock();
    let legacy = load_legacy_passwords(context);
    for conn in &mut incoming {
        if let Some(pw) = conn.password.take().filter(|s| !s.trim().is_empty()) {
            if let Err(e) = context.secrets.set(&secret_key_for(&conn.id), &pw) {
                return failure(request.id, ErrorCode::IoError, &e);
            }
            conn.has_password = true;
        } else {
            conn.has_password = has_password_for(context, &conn.id, &legacy);
        }
    }
    let bytes = match serde_json::to_vec_pretty(&incoming) {
        Ok(b) => b,
        Err(e) => return failure(request.id, ErrorCode::IoError, &e.to_string()),
    };
    if let Err(e) = write_private_file(&ssh_connections_file(context), &bytes) {
        return failure(request.id, ErrorCode::IoError, &e);
    }
    ok(&request, json!({ "saved": incoming.len() }))
}

fn delete_ssh_connection(request: Request, context: &CoreContext) -> Frame {
    if let Err(err) = context.require_write() {
        return failure(request.id, ErrorCode::IoError, &err.to_string());
    }
    let Some(connection_id) = connection_id_param(&request.params) else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "connections.delete_ssh_connection requires params.connectionId",
        );
    };
    let _guard = SSH_STORAGE_LOCK.lock();
    let mut list = match read_connections_raw(context) {
        Ok(list) => list,
        Err(e) => return failure(request.id, ErrorCode::IoError, &e),
    };
    list.retain(|c| c.id != connection_id);
    for conn in &mut list {
        conn.password = None;
    }
    let bytes = match serde_json::to_vec_pretty(&list) {
        Ok(b) => b,
        Err(e) => return failure(request.id, ErrorCode::IoError, &e.to_string()),
    };
    if let Err(e) = write_private_file(&ssh_connections_file(context), &bytes) {
        return failure(request.id, ErrorCode::IoError, &e);
    }
    let _ = context.secrets.delete(&secret_key_for(&connection_id));
    ok(&request, json!({ "deleted": connection_id }))
}

fn host_target_tokens(host: &str, port: u16) -> Vec<String> {
    if port == 22 {
        vec![host.to_string(), format!("[{host}]:22")]
    } else {
        vec![format!("[{host}]:{port}")]
    }
}

fn check_ssh_host_key(request: Request, context: &CoreContext) -> Frame {
    let Some(host) = request
        .params
        .get("host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "connections.check_ssh_host_key requires params.host",
        );
    };
    let port = request
        .params
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(22);
    let target = if port == 22 {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    let tokens = host_target_tokens(host, port);
    let kh_path = known_hosts_file(context);
    if let Ok(raw) = std::fs::read_to_string(&kh_path) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let mut parts = trimmed.split_whitespace();
            let Some(first) = parts.next() else {
                continue;
            };
            let host_field = if first.starts_with('@') {
                parts.next().unwrap_or_default()
            } else {
                first
            };
            if host_field
                .split(',')
                .any(|entry| tokens.iter().any(|t| t == entry))
                && fingerprint_of(trimmed).is_some()
            {
                let val = serde_json::to_value(SshHostKeyStatus::Trusted).unwrap_or(json!({}));
                return ok(&request, val);
            }
        }
    }
    if let Some(scan_stdout) = request.params.get("scanStdout").and_then(Value::as_str) {
        let keys = parse_scan_output(scan_stdout);
        let status = if keys.is_empty() {
            SshHostKeyStatus::Unreachable { target }
        } else {
            SshHostKeyStatus::Unknown { target, keys }
        };
        return ok(&request, serde_json::to_value(status).unwrap_or(json!({})));
    }
    let status = SshHostKeyStatus::Unreachable { target };
    ok(&request, serde_json::to_value(status).unwrap_or(json!({})))
}

fn trust_ssh_host_key(request: Request, context: &CoreContext) -> Frame {
    if let Err(err) = context.require_write() {
        return failure(request.id, ErrorCode::IoError, &err.to_string());
    }
    let mut lines_to_add: Vec<String> = Vec::new();
    if let Some(line) = request.params.get("knownHostsLine").and_then(Value::as_str) {
        lines_to_add.push(line.trim().to_string());
    }
    if let Some(arr) = request
        .params
        .get("knownHostsLines")
        .and_then(Value::as_array)
    {
        for item in arr {
            if let Some(line) = item.as_str() {
                lines_to_add.push(line.trim().to_string());
            }
        }
    }
    if lines_to_add.is_empty() {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "connections.trust_ssh_host_key requires params.knownHostsLine or knownHostsLines",
        );
    }
    for line in &lines_to_add {
        if fingerprint_of(line).is_none() {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                "refusing to trust malformed ssh known_hosts line",
            );
        }
    }
    let kh_path = known_hosts_file(context);
    let mut existing = std::fs::read_to_string(&kh_path).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    for line in &lines_to_add {
        if !existing.lines().any(|l| l.trim() == line) {
            existing.push_str(line);
            existing.push('\n');
        }
    }
    if let Err(e) = write_private_file(&kh_path, existing.as_bytes()) {
        return failure(request.id, ErrorCode::IoError, &e);
    }
    ok(&request, json!({ "trusted": lines_to_add.len() }))
}
