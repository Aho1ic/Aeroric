//! P8-05 切片一百一十(ide 刀五):LSP **会话引擎**——JSON-RPC 帧读写、进程
//! 握手、文档同步、请求超时与 settle。
//!
//! 自 `src-tauri/src/lsp.rs` verbatim 上收;两处注入保证 core 不依赖 Tauri:
//! **diagnostics 出口**([`DiagnosticsSink`],桌面壳接 `app.emit`、宿主接
//! EventLog)与 **server 启动器**([`ServerLauncher`],login env/平台 shell
//! 差异全在调用方)。远端孪生共用本引擎——SSH 命令构建留在客户端适配层
//! ([`CommandSpec`] 由 [`ServerLauncher`] 返回,core 不问它是不是 ssh)。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{timeout, Duration};

use crate::ide::diagnostics_face::{DiagnosticItem, DiagnosticSeverity};
use crate::ide::lsp_face::LspServerCommand;
use crate::ide::lsp_session::protocol::{file_uri, parse_position, path_from_file_uri};
use crate::ide::lsp_session::LspDocumentRequest;

/// 请求超时(与原 Tauri 命令逐字同值:8s)。
pub const LSP_TIMEOUT: Duration = Duration::from_secs(8);

/// diagnostics 通知的出口(`lsp://diagnostics` 的处理落在注入方)。
pub type DiagnosticsSink = Arc<dyn Fn(LspDiagnosticsEvent) + Send + Sync>;

/// 启动一个语言服务器所需的命令(program/args/cwd/env)——远端孪生把 ssh
/// 包装后的东西填进来,core 只负责把它当进程拉起。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub current_dir: Option<std::path::PathBuf>,
    pub envs: Vec<(String, String)>,
}

/// 语言服务器的启动器端口(客户端适配层注入:本地直接拉起、远端经 ssh)。
pub trait ServerLauncher: Send + Sync {
    /// 本地 server(server.program/args + 项目根 cwd)。
    fn local(&self, project_root: &Path, server: &LspServerCommand) -> CommandSpec;
    /// 远端 server(ssh 包装后的命令)。
    fn remote(&self, root_uri: &str, remote_root: &str, server: &LspServerCommand) -> CommandSpec;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDiagnosticsEvent {
    pub project_path: String,
    pub file_path: String,
    pub diagnostics: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspDocumentSnapshot {
    pub path: String,
    pub language_id: String,
    pub version: u32,
    pub content: String,
}

static LOCAL_LSP_SESSIONS: LazyLock<Mutex<BTreeMap<String, Arc<AsyncMutex<LocalLspSession>>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static REMOTE_LSP_SESSIONS: LazyLock<Mutex<BTreeMap<String, Arc<AsyncMutex<LocalLspSession>>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

pub struct LocalLspSession {
    stdin: ChildStdin,
    stdout: ChildStdout,
    child: Child,
    next_id: i64,
    open_documents: BTreeMap<String, u32>,
}

impl LocalLspSession {
    pub fn next_request_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub async fn sync_documents(
        &mut self,
        documents: Vec<LspDocumentSnapshot>,
    ) -> Result<(), String> {
        for document in documents {
            let current_version = self.open_documents.get(&document.path).copied();
            if current_version == Some(document.version) {
                continue;
            }
            let uri = file_uri(Path::new(&document.path));
            if current_version.is_some() {
                write_message(
                    &mut self.stdin,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didChange",
                        "params": {
                            "textDocument": {
                                "uri": uri,
                                "version": document.version
                            },
                            "contentChanges": [{ "text": document.content }]
                        }
                    }),
                )
                .await?;
            } else {
                write_message(
                    &mut self.stdin,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didOpen",
                        "params": {
                            "textDocument": {
                                "uri": uri,
                                "languageId": document.language_id,
                                "version": document.version,
                                "text": document.content
                            }
                        }
                    }),
                )
                .await?;
            }
            self.open_documents.insert(document.path, document.version);
        }
        Ok(())
    }

    pub async fn shutdown(&mut self) {
        let shutdown_id = self.next_request_id();
        let _ = write_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": shutdown_id,
                "method": "shutdown",
                "params": null
            }),
        )
        .await;
        let _ = write_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "exit"
            }),
        )
        .await;
        let _ = crate::runtime::subprocess::terminate_tokio_process_tree(&mut self.child).await;
    }
}

pub fn local_lsp_session_handle(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
    LOCAL_LSP_SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(session_key).cloned())
}

fn insert_local_lsp_session(session_key: String, session: LocalLspSession) {
    if let Ok(mut sessions) = LOCAL_LSP_SESSIONS.lock() {
        sessions.insert(session_key, Arc::new(AsyncMutex::new(session)));
    }
}

pub fn take_local_lsp_session(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
    LOCAL_LSP_SESSIONS
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(session_key))
}

pub async fn shutdown_lsp_handle(handle: Arc<AsyncMutex<LocalLspSession>>) {
    let mut session = handle.lock().await;
    session.shutdown().await;
}

pub async fn shutdown_local_lsp_session(session_key: &str) {
    if let Some(handle) = take_local_lsp_session(session_key) {
        shutdown_lsp_handle(handle).await;
    }
}

pub fn take_local_lsp_sessions_with_prefix(
    session_prefix: &str,
) -> Vec<Arc<AsyncMutex<LocalLspSession>>> {
    let Ok(mut sessions) = LOCAL_LSP_SESSIONS.lock() else {
        return Vec::new();
    };
    let keys = sessions
        .keys()
        .filter(|key| key.starts_with(session_prefix))
        .cloned()
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| sessions.remove(&key))
        .collect()
}

pub fn remote_lsp_session_handle(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
    REMOTE_LSP_SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(session_key).cloned())
}

fn insert_remote_lsp_session(session_key: String, session: LocalLspSession) {
    if let Ok(mut sessions) = REMOTE_LSP_SESSIONS.lock() {
        sessions.insert(session_key, Arc::new(AsyncMutex::new(session)));
    }
}

pub fn take_remote_lsp_session(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
    REMOTE_LSP_SESSIONS
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(session_key))
}

pub async fn shutdown_remote_lsp_session(session_key: &str) {
    if let Some(handle) = take_remote_lsp_session(session_key) {
        shutdown_lsp_handle(handle).await;
    }
}

pub fn take_remote_lsp_sessions_with_prefix(
    session_prefix: &str,
) -> Vec<Arc<AsyncMutex<LocalLspSession>>> {
    let Ok(mut sessions) = REMOTE_LSP_SESSIONS.lock() else {
        return Vec::new();
    };
    let keys = sessions
        .keys()
        .filter(|key| key.starts_with(session_prefix))
        .cloned()
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| sessions.remove(&key))
        .collect()
}

#[cfg(test)]
pub fn local_lsp_session_count() -> usize {
    LOCAL_LSP_SESSIONS
        .lock()
        .map(|sessions| sessions.len())
        .unwrap_or_default()
}

pub async fn close_lsp_session_document(
    handle: Option<Arc<AsyncMutex<LocalLspSession>>>,
    file_path: &str,
) {
    let Some(handle) = handle else { return };
    let mut session = handle.lock().await;
    if session.open_documents.remove(file_path).is_none() {
        return;
    }
    let _ = write_message(
        &mut session.stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": {
                "textDocument": {
                    "uri": file_uri(Path::new(file_path))
                }
            }
        }),
    )
    .await;
}

pub async fn close_local_lsp_session_document(session_key: &str, file_path: &str) {
    close_lsp_session_document(local_lsp_session_handle(session_key), file_path).await;
}

pub async fn close_remote_lsp_session_document(session_key: &str, file_path: &str) {
    close_lsp_session_document(remote_lsp_session_handle(session_key), file_path).await;
}

pub async fn write_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|err| err.to_string())?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|err| err.to_string())?;
    writer
        .write_all(&body)
        .await
        .map_err(|err| err.to_string())?;
    writer.flush().await.map_err(|err| err.to_string())
}

pub async fn read_response_with_id<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    id: i64,
    diagnostics: Option<&DiagnosticsSink>,
    project_path: &str,
) -> Result<Value, String> {
    loop {
        let value = read_message(reader).await?;
        if let Some(emitter) = diagnostics {
            emit_lsp_diagnostics_if_present(emitter, project_path, &value);
        }
        if value.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(value);
        }
    }
}

pub async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Value, String> {
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        reader
            .read_exact(&mut byte)
            .await
            .map_err(|err| format!("failed to read language server header: {err}"))?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 8192 {
            return Err("language server header is too large".to_string());
        }
    }

    let header = String::from_utf8(header).map_err(|err| err.to_string())?;
    let content_length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .ok_or_else(|| "language server response missing Content-Length".to_string())?;
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|err| format!("failed to read language server body: {err}"))?;
    serde_json::from_slice(&body).map_err(|err| err.to_string())
}

pub fn emit_lsp_diagnostics_if_present(sink: &DiagnosticsSink, project_path: &str, value: &Value) {
    let Some(params) = value
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| *method == "textDocument/publishDiagnostics")
        .and_then(|_| value.get("params"))
    else {
        return;
    };
    let Some(event) = parse_lsp_diagnostics_event(project_path, params) else {
        return;
    };
    sink(event);
}

pub fn parse_lsp_diagnostics_event(
    project_path: &str,
    params: &Value,
) -> Option<LspDiagnosticsEvent> {
    let uri = params.get("uri").and_then(Value::as_str)?;
    let file_path = path_from_file_uri(uri);
    let diagnostics = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_lsp_diagnostic_item(&file_path, item))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(LspDiagnosticsEvent {
        project_path: project_path.to_string(),
        file_path,
        diagnostics,
    })
}

fn parse_lsp_diagnostic_item(file_path: &str, value: &Value) -> Option<DiagnosticItem> {
    let range = value.get("range")?;
    let start = parse_position(range.get("start")?)?;
    let severity = match value.get("severity").and_then(Value::as_u64).unwrap_or(3) {
        1 => DiagnosticSeverity::Error,
        2 => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    };
    let code = value.get("code").and_then(|code| {
        code.as_str()
            .map(str::to_string)
            .or_else(|| code.as_i64().map(|number| number.to_string()))
            .or_else(|| code.as_u64().map(|number| number.to_string()))
    });
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.trim().is_empty())
        .unwrap_or("language-server");
    Some(DiagnosticItem {
        source: format!("lsp:{source}"),
        severity,
        message: value.get("message")?.as_str()?.to_string(),
        file: file_path.to_string(),
        line: start.line as usize + 1,
        column: start.character as usize + 1,
        code,
    })
}

/// 启动一个会话并完成 `initialize`/`initialized` 握手(原实现逐字:握手失败
/// 的错误文案也保留)。
pub async fn start_lsp_session(
    spec: CommandSpec,
    root_uri: String,
    workspace_name: String,
    not_found_message: &str,
) -> Result<LocalLspSession, String> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .envs(spec.envs.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(cwd) = &spec.current_dir {
        command.current_dir(cwd);
    }
    crate::runtime::subprocess::configure_terminable_tokio_process_tree(&mut command);
    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            // 逐字保留 legacy 的 not_found_message:这是用户可见的 wire 文案,
            // 远端孪生那半仍报同一句,两边不得对同一失败原因给不同句子。
            not_found_message.to_string()
        } else {
            format!("failed to start language server: {err}")
        }
    })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "failed to open language server stdin".to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to open language server stdout".to_string())?;

    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": root_uri.clone(),
                "capabilities": {},
                "workspaceFolders": [{
                    "uri": root_uri,
                    "name": workspace_name
                }]
            }
        }),
    )
    .await?;
    let response = read_response_with_id(&mut stdout, 1, None, "").await?;
    if let Some(error) = response.get("error") {
        return Err(format!("language server initialize failed: {error}"));
    }

    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )
    .await?;

    Ok(LocalLspSession {
        stdin,
        stdout,
        child,
        next_id: 2,
        open_documents: BTreeMap::new(),
    })
}

/// 取会话(没有就启动并登记);`remote` 决定登记到哪张表。
pub async fn get_or_start_lsp_session(
    local: bool,
    launcher: &dyn ServerLauncher,
    session_key: &str,
    project_root: &Path,
    server: &LspServerCommand,
    root_uri: String,
    workspace_name: String,
    not_found_message: &str,
) -> Result<Arc<AsyncMutex<LocalLspSession>>, String> {
    if let Some(handle) = if local {
        local_lsp_session_handle(session_key)
    } else {
        remote_lsp_session_handle(session_key)
    } {
        return Ok(handle);
    }
    let spec = if local {
        launcher.local(project_root, server)
    } else {
        launcher.remote(&root_uri, &project_root.to_string_lossy(), server)
    };
    let session = start_lsp_session(spec, root_uri, workspace_name, not_found_message).await?;
    if local {
        insert_local_lsp_session(session_key.to_string(), session);
        local_lsp_session_handle(session_key)
    } else {
        insert_remote_lsp_session(session_key.to_string(), session);
        remote_lsp_session_handle(session_key)
    }
    .ok_or_else(|| "failed to store language server session".to_string())
}

/// 一次会话请求的完整生命周期:取/起会话 → 同步文档 → 发请求 → 读回;错误与
/// 超时都结算掉会话(原实现逐字),`LSP_TIMEOUT` 之外不留半开连接。
pub async fn run_lsp_session_request(
    local: bool,
    launcher: &dyn ServerLauncher,
    sink: Option<&DiagnosticsSink>,
    session_key: String,
    project_root: &Path,
    server: &LspServerCommand,
    root_uri: String,
    workspace_name: String,
    document_uri: String,
    documents: Vec<LspDocumentSnapshot>,
    request: &LspDocumentRequest,
    method: &str,
    params: crate::ide::lsp_session::RequestParams,
    not_found_message: &str,
) -> Result<Value, String> {
    let project_key = project_root.to_string_lossy().to_string();
    let run = async {
        let handle = get_or_start_lsp_session(
            local,
            launcher,
            &session_key,
            project_root,
            server,
            root_uri,
            workspace_name,
            not_found_message,
        )
        .await?;
        let mut session = handle.lock().await;
        session.sync_documents(documents).await?;
        let request_id = session.next_request_id();
        let request_params =
            crate::ide::lsp_session::feature_request_params(&document_uri, request, params);

        write_message(
            &mut session.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": request_params
            }),
        )
        .await?;

        let response =
            read_response_with_id(&mut session.stdout, request_id, sink, &project_key).await?;
        if let Some(error) = response.get("error") {
            return Err(format!("language server request failed: {error}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    };

    match timeout(LSP_TIMEOUT, run).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            settle_lsp_session(local, &session_key).await;
            Err(err)
        }
        Err(_) => {
            settle_lsp_session(local, &session_key).await;
            Err("language server request timed out".to_string())
        }
    }
}

async fn settle_lsp_session(local: bool, session_key: &str) {
    if local {
        shutdown_local_lsp_session(session_key).await;
    } else {
        shutdown_remote_lsp_session(session_key).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_publish_diagnostics_into_typed_items() {
        let event = parse_lsp_diagnostics_event(
            "/repo",
            &json!({
                "uri": "file:///repo/src/App.ts",
                "diagnostics": [{
                    "range": {
                        "start": { "line": 2, "character": 4 },
                        "end": { "line": 2, "character": 9 }
                    },
                    "severity": 1,
                    "source": "typescript",
                    "code": 2322,
                    "message": "Type 'string' is not assignable to type 'number'."
                }]
            }),
        )
        .unwrap();
        assert_eq!(event.project_path, "/repo");
        assert_eq!(event.file_path, "/repo/src/App.ts");
        assert_eq!(event.diagnostics.len(), 1);
        assert_eq!(event.diagnostics[0].source, "lsp:typescript");
        assert_eq!(event.diagnostics[0].severity, DiagnosticSeverity::Error);
        assert_eq!(event.diagnostics[0].line, 3);
        assert_eq!(event.diagnostics[0].column, 5);
        assert_eq!(event.diagnostics[0].code.as_deref(), Some("2322"));
    }

    #[test]
    fn only_publish_diagnostics_notifications_reach_the_sink() {
        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink: DiagnosticsSink = {
            let seen = Arc::clone(&seen);
            Arc::new(move |event: LspDiagnosticsEvent| seen.lock().push(event))
        };
        // 一条响应(没有 method)与一条别的通知都不该触发出口。
        emit_lsp_diagnostics_if_present(&sink, "/repo", &json!({ "id": 3, "result": {} }));
        emit_lsp_diagnostics_if_present(
            &sink,
            "/repo",
            &json!({ "method": "window/logMessage", "params": {} }),
        );
        assert!(seen.lock().is_empty());
        emit_lsp_diagnostics_if_present(
            &sink,
            "/repo",
            &json!({
                "method": "textDocument/publishDiagnostics",
                "params": { "uri": "file:///repo/a.ts", "diagnostics": [] }
            }),
        );
        assert_eq!(seen.lock().len(), 1);
    }
}
