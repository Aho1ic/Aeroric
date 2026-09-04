use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write as StdWrite;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{timeout, Duration};

use crate::diagnostics::{DiagnosticItem, DiagnosticSeverity};
use crate::path_guard::remote_path_has_relative_components;
use crate::ssh::SshConnection;

mod edit;
mod parse;
mod protocol;

use protocol::{
    file_uri, markdown_text, parse_completion_items, parse_hover, parse_locations, parse_position,
    parse_range, path_from_file_uri,
};

// 两段实现搬进了子模块,它们都以 `use super::*;` 开头直接拿本文件的 import 和私有类型;
// 这里反过来把它们的项拉回本作用域,于是原有调用点一处都不用改。
// 两段内**都没有** `pub` / `pub(crate)` 项(脚本加了断言),所以不需要额外 re-export。
use edit::*;
use parse::*;

const LSP_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_REMOTE_LSP_EDIT_FILE_BYTES: u64 = 2 * 1024 * 1024;

static LSP_DOCUMENTS: LazyLock<Mutex<BTreeMap<String, BTreeMap<String, LspOpenDocument>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static LOCAL_LSP_SESSIONS: LazyLock<Mutex<BTreeMap<String, Arc<AsyncMutex<LocalLspSession>>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static REMOTE_LSP_SESSIONS: LazyLock<Mutex<BTreeMap<String, Arc<AsyncMutex<LocalLspSession>>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspServerCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspServerStatus {
    pub supported: bool,
    pub available: bool,
    pub language_id: Option<String>,
    pub command: Option<LspServerCommand>,
    pub install_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspLocation {
    pub uri: String,
    pub path: String,
    pub range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspHover {
    pub contents: String,
    pub range: Option<LspRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspCompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspParameterInformation {
    pub label: String,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspSignatureInformation {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<LspParameterInformation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspSignatureHelp {
    pub signatures: Vec<LspSignatureInformation>,
    pub active_signature: Option<u32>,
    pub active_parameter: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspCommand {
    pub title: Option<String>,
    pub command: String,
    #[serde(default)]
    pub arguments: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspCodeAction {
    pub title: String,
    pub kind: Option<String>,
    pub edit: Option<LspWorkspaceEdit>,
    pub command: Option<LspCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspInlayHint {
    pub label: String,
    pub position: LspPosition,
    pub kind: Option<u32>,
    pub tooltip: Option<String>,
    pub padding_left: bool,
    pub padding_right: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspSymbol {
    pub name: String,
    pub kind: u32,
    pub detail: Option<String>,
    pub container_name: Option<String>,
    pub uri: String,
    pub path: String,
    pub range: LspRange,
    pub selection_range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspTextEdit {
    pub range: LspRange,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspFileEdit {
    pub uri: String,
    pub path: String,
    pub edits: Vec<LspTextEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspWorkspaceEdit {
    pub files: Vec<LspFileEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspApplyWorkspaceEditSummary {
    pub files_changed: usize,
    pub edits_applied: usize,
    pub edits_skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDocumentRequest {
    pub project_path: String,
    pub file_path: String,
    pub content: String,
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspDocumentLifecycleSummary {
    pub project_path: String,
    pub file_path: String,
    pub language_id: String,
    pub version: u32,
    pub open_documents: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDiagnosticsEvent {
    pub project_path: String,
    pub file_path: String,
    pub diagnostics: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LspOpenDocument {
    language_id: String,
    version: u32,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LspDocumentSnapshot {
    path: String,
    language_id: String,
    version: u32,
    content: String,
}

struct LspDiagnosticEmitter {
    app: AppHandle,
    project_path: String,
}

struct LocalLspSession {
    stdin: ChildStdin,
    stdout: ChildStdout,
    child: Child,
    next_id: i64,
    open_documents: BTreeMap<String, u32>,
}

impl LocalLspSession {
    fn next_request_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    async fn sync_documents(&mut self, documents: Vec<LspDocumentSnapshot>) -> Result<(), String> {
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

    async fn shutdown(&mut self) {
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
        let _ = crate::subprocess::terminate_tokio_process_tree(&mut self.child).await;
    }
}

pub fn language_id_for_path(path: impl AsRef<Path>) -> Option<&'static str> {
    let ext = path.as_ref().extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "ts" => Some("typescript"),
        "tsx" => Some("typescriptreact"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "jsx" => Some("javascriptreact"),
        _ => None,
    }
}

pub fn default_server_command(language_id: &str) -> Option<LspServerCommand> {
    match language_id {
        "typescript" | "typescriptreact" | "javascript" | "javascriptreact" => {
            Some(LspServerCommand {
                program: "typescript-language-server".to_string(),
                args: vec!["--stdio".to_string()],
            })
        }
        _ => None,
    }
}

fn lsp_server_invocation(server: &LspServerCommand) -> String {
    std::iter::once(crate::ssh::shell_word_posix(&server.program))
        .chain(
            server
                .args
                .iter()
                .map(|arg| crate::ssh::shell_word_posix(arg)),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_remote_lsp_server_command(remote_project_path: &str, server: &LspServerCommand) -> String {
    format!(
        "cd -- {} && exec {}",
        crate::ssh::shell_quote_posix(remote_project_path),
        lsp_server_invocation(server)
    )
}

fn build_remote_lsp_status_command(remote_project_path: &str, program: &str) -> String {
    let script = "cd -- \"$1\" && command -v \"$2\" >/dev/null 2>&1";
    format!(
        "sh -c {} sh {} {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(remote_project_path),
        crate::ssh::shell_quote_posix(program)
    )
}

fn normalize_remote_lsp_path(path: &str, label: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if !trimmed.starts_with('/') {
        return Err(format!("{label} must be absolute"));
    }
    if trimmed.contains('\0') || remote_path_has_relative_components(trimmed) {
        return Err(format!("{label} cannot contain . or .. components"));
    }
    Ok(if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    })
}

fn remote_path_is_inside_root(root: &str, path: &str) -> bool {
    root == "/" || path == root || path.starts_with(&format!("{root}/"))
}

fn validate_remote_lsp_document_path(remote_root: &str, file_path: &str) -> Result<String, String> {
    let root = normalize_remote_lsp_path(remote_root, "Remote project path")?;
    let path = normalize_remote_lsp_path(file_path, "Remote document path")?;
    if !remote_path_is_inside_root(&root, &path) {
        return Err("remote document path is outside project root".to_string());
    }
    if language_id_for_path(Path::new(&path)).is_none() {
        return Err("language server is not supported for this file type".to_string());
    }
    Ok(path)
}

fn remote_path_is_protected(root: &str, path: &str) -> bool {
    let relative = if root == "/" {
        path.trim_start_matches('/')
    } else if path == root {
        ""
    } else {
        path.strip_prefix(&format!("{root}/")).unwrap_or("")
    };
    relative
        .split('/')
        .next()
        .is_some_and(|component| matches!(component, ".git" | ".aeroric"))
}

fn validate_remote_lsp_edit_path(remote_root: &str, file_path: &str) -> Result<String, String> {
    let path = validate_remote_lsp_document_path(remote_root, file_path)?;
    let root = normalize_remote_lsp_path(remote_root, "Remote project path")?;
    if remote_path_is_protected(&root, &path) {
        return Err("remote LSP edit path is protected".to_string());
    }
    Ok(path)
}

fn remote_workspace_name(remote_root: &str) -> String {
    remote_root
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

fn local_workspace_name(project_root: &Path) -> String {
    project_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string()
}

fn local_lsp_lifecycle_context(
    project_path: &str,
    file_path: &str,
) -> Result<(String, String, String, String), String> {
    let project_root = Path::new(project_path)
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let document_path = validate_document_path(&project_root, Path::new(file_path))?;
    let language_id = language_id_for_path(&document_path)
        .ok_or_else(|| "language server is not supported for this file type".to_string())?
        .to_string();
    let project_key = project_root.to_string_lossy().to_string();
    let file_key = document_path.to_string_lossy().to_string();
    let session_key = format!("local:{project_key}:{language_id}");
    Ok((session_key, project_key, file_key, language_id))
}

fn remote_lsp_lifecycle_context(
    connection: &SshConnection,
    remote_project_path: &str,
    file_path: &str,
) -> Result<(String, String, String, String), String> {
    let remote_root = normalize_remote_lsp_path(remote_project_path, "Remote project path")?;
    let document_path = validate_remote_lsp_document_path(&remote_root, file_path)?;
    let language_id = language_id_for_path(Path::new(&document_path))
        .ok_or_else(|| "language server is not supported for this file type".to_string())?
        .to_string();
    let session_key = format!("ssh:{}:{remote_root}:{language_id}", connection.id);
    Ok((session_key, remote_root, document_path, language_id))
}

fn upsert_lsp_document(
    session_key: String,
    project_path: String,
    file_path: String,
    language_id: String,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let mut registry = LSP_DOCUMENTS
        .lock()
        .map_err(|_| "language server document registry is poisoned".to_string())?;
    let documents = registry.entry(session_key).or_default();
    documents.insert(
        file_path.clone(),
        LspOpenDocument {
            language_id: language_id.clone(),
            version,
            content,
        },
    );
    Ok(LspDocumentLifecycleSummary {
        project_path,
        file_path,
        language_id,
        version,
        open_documents: documents.len(),
    })
}

fn close_lsp_document(
    session_key: String,
    project_path: String,
    file_path: String,
    language_id: String,
) -> Result<LspDocumentLifecycleSummary, String> {
    let mut registry = LSP_DOCUMENTS
        .lock()
        .map_err(|_| "language server document registry is poisoned".to_string())?;
    let mut version = 0;
    let mut open_documents = 0;
    let mut remove_session = false;
    if let Some(documents) = registry.get_mut(&session_key) {
        if let Some(document) = documents.remove(&file_path) {
            version = document.version;
        }
        open_documents = documents.len();
        remove_session = documents.is_empty();
    }
    if remove_session {
        registry.remove(&session_key);
    }
    Ok(LspDocumentLifecycleSummary {
        project_path,
        file_path,
        language_id,
        version,
        open_documents,
    })
}

fn shutdown_lsp_project_sessions(session_prefix: &str) -> Result<usize, String> {
    let mut registry = LSP_DOCUMENTS
        .lock()
        .map_err(|_| "language server document registry is poisoned".to_string())?;
    let session_keys = registry
        .keys()
        .filter(|key| key.starts_with(session_prefix))
        .cloned()
        .collect::<Vec<_>>();
    let closed_documents = session_keys
        .iter()
        .filter_map(|key| registry.get(key))
        .map(BTreeMap::len)
        .sum();
    for key in session_keys {
        registry.remove(&key);
    }
    Ok(closed_documents)
}

fn lsp_session_document_snapshots(
    session_key: &str,
    request_path: String,
    request_language_id: String,
    request_content: String,
) -> Result<Vec<LspDocumentSnapshot>, String> {
    let registry = LSP_DOCUMENTS
        .lock()
        .map_err(|_| "language server document registry is poisoned".to_string())?;
    let mut snapshots = registry
        .get(session_key)
        .map(|documents| {
            documents
                .iter()
                .map(|(path, document)| LspDocumentSnapshot {
                    path: path.clone(),
                    language_id: document.language_id.clone(),
                    version: document.version,
                    content: document.content.clone(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !snapshots
        .iter()
        .any(|snapshot| snapshot.path == request_path)
    {
        snapshots.push(LspDocumentSnapshot {
            path: request_path,
            language_id: request_language_id,
            version: 1,
            content: request_content,
        });
    }
    snapshots.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(snapshots)
}

fn lsp_session_open_document_snapshots(
    session_key: &str,
) -> Result<Vec<LspDocumentSnapshot>, String> {
    let registry = LSP_DOCUMENTS
        .lock()
        .map_err(|_| "language server document registry is poisoned".to_string())?;
    let mut snapshots = registry
        .get(session_key)
        .map(|documents| {
            documents
                .iter()
                .map(|(path, document)| LspDocumentSnapshot {
                    path: path.clone(),
                    language_id: document.language_id.clone(),
                    version: document.version,
                    content: document.content.clone(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    snapshots.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(snapshots)
}

fn local_lsp_session_handle(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
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

fn take_local_lsp_session(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
    LOCAL_LSP_SESSIONS
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(session_key))
}

async fn shutdown_lsp_handle(handle: Arc<AsyncMutex<LocalLspSession>>) {
    let mut session = handle.lock().await;
    session.shutdown().await;
}

async fn shutdown_local_lsp_session(session_key: &str) {
    if let Some(handle) = take_local_lsp_session(session_key) {
        shutdown_lsp_handle(handle).await;
    }
}

fn take_local_lsp_sessions_with_prefix(
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

fn remote_lsp_session_handle(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
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

fn take_remote_lsp_session(session_key: &str) -> Option<Arc<AsyncMutex<LocalLspSession>>> {
    REMOTE_LSP_SESSIONS
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(session_key))
}

async fn shutdown_remote_lsp_session(session_key: &str) {
    if let Some(handle) = take_remote_lsp_session(session_key) {
        shutdown_lsp_handle(handle).await;
    }
}

fn take_remote_lsp_sessions_with_prefix(
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
fn local_lsp_session_count() -> usize {
    LOCAL_LSP_SESSIONS
        .lock()
        .map(|sessions| sessions.len())
        .unwrap_or_default()
}

async fn close_lsp_session_document(
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

async fn close_local_lsp_session_document(session_key: &str, file_path: &str) {
    close_lsp_session_document(local_lsp_session_handle(session_key), file_path).await;
}

async fn close_remote_lsp_session_document(session_key: &str, file_path: &str) {
    close_lsp_session_document(remote_lsp_session_handle(session_key), file_path).await;
}

fn tokio_ssh_command_for_remote_command(
    connection: &SshConnection,
    remote_command: String,
) -> Command {
    let spec = crate::ssh::ssh_command_spec_for_remote_command(connection, remote_command);
    let mut command = Command::new(spec.program);
    crate::subprocess::configure_terminable_tokio_process_tree(&mut command);
    command.args(spec.args);
    for (key, value) in spec.env {
        command.env(key, value);
    }
    command.env("PATH", crate::app_settings::get_login_shell_path());
    command
}

fn build_remote_lsp_read_text_command(remote_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(remote_path);
    format!(
        "size=$(wc -c < {path}) && [ \"$size\" -le {MAX_REMOTE_LSP_EDIT_FILE_BYTES} ] && cat -- {path}"
    )
}

fn build_remote_lsp_write_text_command(remote_path: &str) -> String {
    format!("cat > {}", crate::ssh::shell_quote_posix(remote_path))
}

pub fn validate_document_path(project_path: &Path, file_path: &Path) -> Result<PathBuf, String> {
    let project_root = project_path
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let document_path = file_path
        .canonicalize()
        .map_err(|err| format!("failed to resolve document path: {err}"))?;
    if !document_path.starts_with(&project_root) {
        return Err("document path is outside project root".to_string());
    }
    if language_id_for_path(&document_path).is_none() {
        return Err("language server is not supported for this file type".to_string());
    }
    Ok(document_path)
}

#[tauri::command]
pub fn lsp_server_status(
    project_path: String,
    file_path: String,
) -> Result<LspServerStatus, String> {
    let document_path = validate_document_path(Path::new(&project_path), Path::new(&file_path))?;
    let language_id = language_id_for_path(&document_path).map(str::to_string);
    let command = language_id.as_deref().and_then(default_server_command);
    let available = command
        .as_ref()
        .map(|command| command_available(&command.program))
        .unwrap_or(false);

    Ok(LspServerStatus {
        supported: language_id.is_some() && command.is_some(),
        available,
        language_id,
        command,
        install_hint: if available {
            None
        } else {
            Some("pnpm add -D typescript-language-server typescript".to_string())
        },
    })
}

#[tauri::command]
pub fn lsp_open_document(
    project_path: String,
    file_path: String,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, project_key, file_key, language_id) =
        local_lsp_lifecycle_context(&project_path, &file_path)?;
    upsert_lsp_document(
        session_key,
        project_key,
        file_key,
        language_id,
        content,
        version,
    )
}

#[tauri::command]
pub fn lsp_change_document(
    project_path: String,
    file_path: String,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, project_key, file_key, language_id) =
        local_lsp_lifecycle_context(&project_path, &file_path)?;
    upsert_lsp_document(
        session_key,
        project_key,
        file_key,
        language_id,
        content,
        version,
    )
}

#[tauri::command]
pub async fn lsp_close_document(
    project_path: String,
    file_path: String,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, project_key, file_key, language_id) =
        local_lsp_lifecycle_context(&project_path, &file_path)?;
    let summary = close_lsp_document(
        session_key.clone(),
        project_key,
        file_key.clone(),
        language_id,
    )?;
    close_local_lsp_session_document(&session_key, &file_key).await;
    if summary.open_documents == 0 {
        if let Some(handle) = take_local_lsp_session(&session_key) {
            handle.lock().await.shutdown().await;
        }
    }
    Ok(summary)
}

#[tauri::command]
pub async fn lsp_shutdown_project(project_path: String) -> Result<usize, String> {
    let project_root = Path::new(&project_path)
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let project_key = project_root.to_string_lossy();
    let session_prefix = format!("local:{project_key}:");
    let closed_documents = shutdown_lsp_project_sessions(&session_prefix)?;
    for handle in take_local_lsp_sessions_with_prefix(&session_prefix) {
        handle.lock().await.shutdown().await;
    }
    Ok(closed_documents)
}

#[tauri::command]
pub async fn remote_lsp_server_status(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
) -> Result<LspServerStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_lsp_path(&remote_project_path, "Remote project path")?;
        let document_path = validate_remote_lsp_document_path(&remote_root, &file_path)?;
        let language_id = language_id_for_path(Path::new(&document_path)).map(str::to_string);
        let command = language_id.as_deref().and_then(default_server_command);
        let available = if let Some(command) = &command {
            let mut status_command = crate::ssh::std_ssh_command_for_remote_command(
                &connection,
                build_remote_lsp_status_command(&remote_root, &command.program),
            );
            crate::subprocess::configure_background_command(&mut status_command);
            status_command
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        } else {
            false
        };

        Ok(LspServerStatus {
            supported: language_id.is_some() && command.is_some(),
            available,
            language_id,
            command,
            install_hint: if available {
                None
            } else {
                Some(
                    "Install typescript-language-server and typescript on the remote host"
                        .to_string(),
                )
            },
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn remote_lsp_open_document(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, remote_root, document_path, language_id) =
        remote_lsp_lifecycle_context(&connection, &remote_project_path, &file_path)?;
    upsert_lsp_document(
        session_key,
        remote_root,
        document_path,
        language_id,
        content,
        version,
    )
}

#[tauri::command]
pub fn remote_lsp_change_document(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, remote_root, document_path, language_id) =
        remote_lsp_lifecycle_context(&connection, &remote_project_path, &file_path)?;
    upsert_lsp_document(
        session_key,
        remote_root,
        document_path,
        language_id,
        content,
        version,
    )
}

#[tauri::command]
pub async fn remote_lsp_close_document(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, remote_root, document_path, language_id) =
        remote_lsp_lifecycle_context(&connection, &remote_project_path, &file_path)?;
    let summary = close_lsp_document(
        session_key.clone(),
        remote_root,
        document_path.clone(),
        language_id,
    )?;
    close_remote_lsp_session_document(&session_key, &document_path).await;
    if summary.open_documents == 0 {
        if let Some(handle) = take_remote_lsp_session(&session_key) {
            handle.lock().await.shutdown().await;
        }
    }
    Ok(summary)
}

#[tauri::command]
pub async fn remote_lsp_shutdown_project(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<usize, String> {
    let remote_root = normalize_remote_lsp_path(&remote_project_path, "Remote project path")?;
    let session_prefix = format!("ssh:{}:{remote_root}:", connection.id);
    let closed_documents = shutdown_lsp_project_sessions(&session_prefix)?;
    for handle in take_remote_lsp_sessions_with_prefix(&session_prefix) {
        handle.lock().await.shutdown().await;
    }
    Ok(closed_documents)
}

#[tauri::command]
pub async fn lsp_hover(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Option<LspHover>, String> {
    let result = request_lsp_feature(&app, &request, "textDocument/hover").await?;
    Ok(parse_hover(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_hover(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
) -> Result<Option<LspHover>, String> {
    let result = request_remote_lsp_feature(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/hover",
    )
    .await?;
    Ok(parse_hover(Some(&result)))
}

#[tauri::command]
pub async fn lsp_definition(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Vec<LspLocation>, String> {
    let result = request_lsp_feature(&app, &request, "textDocument/definition").await?;
    Ok(parse_locations(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_definition(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
) -> Result<Vec<LspLocation>, String> {
    let result = request_remote_lsp_feature(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/definition",
    )
    .await?;
    Ok(parse_locations(Some(&result)))
}

#[tauri::command]
pub async fn lsp_references(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Vec<LspLocation>, String> {
    let result = request_lsp_feature(&app, &request, "textDocument/references").await?;
    Ok(parse_locations(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_references(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
) -> Result<Vec<LspLocation>, String> {
    let result = request_remote_lsp_feature(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/references",
    )
    .await?;
    Ok(parse_locations(Some(&result)))
}

#[tauri::command]
pub async fn lsp_rename(
    app: AppHandle,
    request: LspDocumentRequest,
    new_name: String,
) -> Result<LspWorkspaceEdit, String> {
    let result = request_lsp_feature_with_params(
        &app,
        &request,
        "textDocument/rename",
        Some(json!({ "newName": new_name })),
    )
    .await?;
    parse_workspace_edit(Some(&result)).ok_or_else(|| "rename returned no edits".to_string())
}

#[tauri::command]
pub async fn remote_lsp_rename(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
    new_name: String,
) -> Result<LspWorkspaceEdit, String> {
    let result = request_remote_lsp_feature_with_params(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/rename",
        Some(json!({ "newName": new_name })),
    )
    .await?;
    parse_workspace_edit(Some(&result)).ok_or_else(|| "rename returned no edits".to_string())
}

#[tauri::command]
pub async fn lsp_apply_workspace_edit(
    project_path: String,
    edit: LspWorkspaceEdit,
) -> Result<LspApplyWorkspaceEditSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        apply_workspace_edit_for_root(Path::new(&project_path), &edit)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn remote_lsp_apply_workspace_edit(
    connection: SshConnection,
    remote_project_path: String,
    edit: LspWorkspaceEdit,
) -> Result<LspApplyWorkspaceEditSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        apply_remote_workspace_edit_for_root(&connection, &remote_project_path, &edit)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn lsp_completion(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Vec<LspCompletionItem>, String> {
    let result = request_lsp_feature(&app, &request, "textDocument/completion").await?;
    Ok(parse_completion_items(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_completion(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
) -> Result<Vec<LspCompletionItem>, String> {
    let result = request_remote_lsp_feature(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/completion",
    )
    .await?;
    Ok(parse_completion_items(Some(&result)))
}

#[tauri::command]
pub async fn lsp_signature_help(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Option<LspSignatureHelp>, String> {
    let result = request_lsp_feature(&app, &request, "textDocument/signatureHelp").await?;
    Ok(parse_signature_help(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_signature_help(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
) -> Result<Option<LspSignatureHelp>, String> {
    let result = request_remote_lsp_feature(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/signatureHelp",
    )
    .await?;
    Ok(parse_signature_help(Some(&result)))
}

#[tauri::command]
pub async fn lsp_code_actions(
    app: AppHandle,
    request: LspDocumentRequest,
    diagnostics: Vec<Value>,
) -> Result<Vec<LspCodeAction>, String> {
    let range = json!({
        "start": {
            "line": request.line,
            "character": request.character
        },
        "end": {
            "line": request.line,
            "character": request.character
        }
    });
    let result = request_lsp_feature_with_params(
        &app,
        &request,
        "textDocument/codeAction",
        Some(json!({
            "range": range,
            "context": {
                "diagnostics": diagnostics
            }
        })),
    )
    .await?;
    Ok(parse_code_actions(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_code_actions(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
    diagnostics: Vec<Value>,
) -> Result<Vec<LspCodeAction>, String> {
    let range = json!({
        "start": {
            "line": request.line,
            "character": request.character
        },
        "end": {
            "line": request.line,
            "character": request.character
        }
    });
    let result = request_remote_lsp_feature_with_params(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/codeAction",
        Some(json!({
            "range": range,
            "context": {
                "diagnostics": diagnostics
            }
        })),
    )
    .await?;
    Ok(parse_code_actions(Some(&result)))
}

#[tauri::command]
pub async fn lsp_execute_command(
    app: AppHandle,
    request: LspDocumentRequest,
    command: LspCommand,
) -> Result<(), String> {
    request_lsp_feature_with_param_mode(
        &app,
        &request,
        "workspace/executeCommand",
        LspFeatureParams::Raw(json!({
            "command": command.command,
            "arguments": command.arguments
        })),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn remote_lsp_execute_command(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
    command: LspCommand,
) -> Result<(), String> {
    request_remote_lsp_feature_with_param_mode(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "workspace/executeCommand",
        LspFeatureParams::Raw(json!({
            "command": command.command,
            "arguments": command.arguments
        })),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn lsp_document_symbols(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Vec<LspSymbol>, String> {
    let uri = file_uri(Path::new(&request.file_path));
    let result = request_lsp_feature(&app, &request, "textDocument/documentSymbol").await?;
    Ok(parse_document_symbols(Some(&result), &uri))
}

#[tauri::command]
pub async fn remote_lsp_document_symbols(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
) -> Result<Vec<LspSymbol>, String> {
    let remote_root = normalize_remote_lsp_path(&remote_project_path, "Remote project path")?;
    let document_path = validate_remote_lsp_document_path(&remote_root, &request.file_path)?;
    let uri = file_uri(Path::new(&document_path));
    let result = request_remote_lsp_feature(
        &app,
        &connection,
        &remote_root,
        &request,
        "textDocument/documentSymbol",
    )
    .await?;
    Ok(parse_document_symbols(Some(&result), &uri))
}

#[tauri::command]
pub async fn lsp_inlay_hints(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Vec<LspInlayHint>, String> {
    let result =
        request_lsp_document_range_feature(&app, &request, "textDocument/inlayHint").await?;
    Ok(parse_inlay_hints(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_inlay_hints(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    request: LspDocumentRequest,
) -> Result<Vec<LspInlayHint>, String> {
    let result = request_remote_lsp_document_range_feature(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "textDocument/inlayHint",
    )
    .await?;
    Ok(parse_inlay_hints(Some(&result)))
}

#[tauri::command]
pub async fn lsp_workspace_symbols(
    app: AppHandle,
    project_path: String,
    query: String,
) -> Result<Vec<LspSymbol>, String> {
    let project_root = Path::new(&project_path)
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let server = default_server_command("typescript")
        .ok_or_else(|| "language server is not configured for workspace symbols".to_string())?;
    let result = run_lsp_workspace_symbol_request(&app, &project_root, &server, &query).await?;
    Ok(parse_workspace_symbols(Some(&result)))
}

#[tauri::command]
pub async fn remote_lsp_workspace_symbols(
    app: AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    query: String,
) -> Result<Vec<LspSymbol>, String> {
    let remote_root = normalize_remote_lsp_path(&remote_project_path, "Remote project path")?;
    let server = default_server_command("typescript")
        .ok_or_else(|| "language server is not configured for workspace symbols".to_string())?;
    let result =
        run_remote_lsp_workspace_symbol_request(&app, &connection, &remote_root, &server, &query)
            .await?;
    Ok(parse_workspace_symbols(Some(&result)))
}

async fn request_lsp_feature(
    app: &AppHandle,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    request_lsp_feature_with_params(app, request, method, None).await
}

async fn request_remote_lsp_feature(
    app: &AppHandle,
    connection: &SshConnection,
    remote_project_path: &str,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    request_remote_lsp_feature_with_params(
        app,
        connection,
        remote_project_path,
        request,
        method,
        None,
    )
    .await
}

async fn request_lsp_feature_with_params(
    app: &AppHandle,
    request: &LspDocumentRequest,
    method: &str,
    extra_params: Option<Value>,
) -> Result<Value, String> {
    request_lsp_feature_with_param_mode(
        app,
        request,
        method,
        LspFeatureParams::Position { extra_params },
    )
    .await
}

async fn request_remote_lsp_feature_with_params(
    app: &AppHandle,
    connection: &SshConnection,
    remote_project_path: &str,
    request: &LspDocumentRequest,
    method: &str,
    extra_params: Option<Value>,
) -> Result<Value, String> {
    request_remote_lsp_feature_with_param_mode(
        app,
        connection,
        remote_project_path,
        request,
        method,
        LspFeatureParams::Position { extra_params },
    )
    .await
}

async fn request_lsp_document_range_feature(
    app: &AppHandle,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    request_lsp_feature_with_param_mode(app, request, method, LspFeatureParams::DocumentRange).await
}

async fn request_remote_lsp_document_range_feature(
    app: &AppHandle,
    connection: &SshConnection,
    remote_project_path: &str,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    request_remote_lsp_feature_with_param_mode(
        app,
        connection,
        remote_project_path,
        request,
        method,
        LspFeatureParams::DocumentRange,
    )
    .await
}

enum LspFeatureParams {
    Position { extra_params: Option<Value> },
    DocumentRange,
    Raw(Value),
}

async fn request_lsp_feature_with_param_mode(
    app: &AppHandle,
    request: &LspDocumentRequest,
    method: &str,
    params: LspFeatureParams,
) -> Result<Value, String> {
    let project_root = Path::new(&request.project_path)
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let document_path = validate_document_path(&project_root, Path::new(&request.file_path))?;
    let language_id = language_id_for_path(&document_path)
        .ok_or_else(|| "language server is not supported for this file type".to_string())?;
    let server = default_server_command(language_id)
        .ok_or_else(|| "language server is not configured for this file type".to_string())?;
    let session_key = format!("local:{}:{language_id}", project_root.to_string_lossy());
    let documents = lsp_session_document_snapshots(
        &session_key,
        document_path.to_string_lossy().to_string(),
        language_id.to_string(),
        request.content.clone(),
    )?;
    run_lsp_request(
        app,
        &project_root,
        &document_path,
        &server,
        documents,
        request,
        method,
        params,
    )
    .await
}

async fn request_remote_lsp_feature_with_param_mode(
    app: &AppHandle,
    connection: &SshConnection,
    remote_project_path: &str,
    request: &LspDocumentRequest,
    method: &str,
    params: LspFeatureParams,
) -> Result<Value, String> {
    let remote_root = normalize_remote_lsp_path(remote_project_path, "Remote project path")?;
    let document_path = validate_remote_lsp_document_path(&remote_root, &request.file_path)?;
    let language_id = language_id_for_path(Path::new(&document_path))
        .ok_or_else(|| "language server is not supported for this file type".to_string())?;
    let server = default_server_command(language_id)
        .ok_or_else(|| "language server is not configured for this file type".to_string())?;
    let session_key = format!("ssh:{}:{remote_root}:{language_id}", connection.id);
    let documents = lsp_session_document_snapshots(
        &session_key,
        document_path.clone(),
        language_id.to_string(),
        request.content.clone(),
    )?;
    run_remote_lsp_request(
        app,
        connection,
        &remote_root,
        &document_path,
        &server,
        documents,
        request,
        method,
        params,
    )
    .await
}

async fn run_lsp_request(
    app: &AppHandle,
    project_root: &Path,
    document_path: &Path,
    server: &LspServerCommand,
    documents: Vec<LspDocumentSnapshot>,
    request: &LspDocumentRequest,
    method: &str,
    params: LspFeatureParams,
) -> Result<Value, String> {
    let language_id = language_id_for_path(document_path)
        .ok_or_else(|| "language server is not supported for this file type".to_string())?;
    let session_key = format!("local:{}:{language_id}", project_root.to_string_lossy());
    run_local_lsp_session_request(
        app,
        session_key,
        project_root,
        server,
        file_uri(document_path),
        documents,
        request,
        method,
        params,
        "typescript-language-server is not installed. Run: pnpm add -D typescript-language-server typescript",
    )
    .await
}

async fn run_local_lsp_session_request(
    app: &AppHandle,
    session_key: String,
    project_root: &Path,
    server: &LspServerCommand,
    document_uri: String,
    documents: Vec<LspDocumentSnapshot>,
    request: &LspDocumentRequest,
    method: &str,
    params: LspFeatureParams,
    not_found_message: &str,
) -> Result<Value, String> {
    let run = async {
        let handle =
            get_or_start_local_lsp_session(&session_key, project_root, server, not_found_message)
                .await?;
        let mut session = handle.lock().await;
        session.sync_documents(documents).await?;
        let request_id = session.next_request_id();
        let request_params = lsp_feature_request_params(&document_uri, request, params);

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

        let diagnostics = LspDiagnosticEmitter {
            app: app.clone(),
            project_path: project_root.to_string_lossy().to_string(),
        };
        let response =
            read_response_with_id(&mut session.stdout, request_id, Some(&diagnostics)).await?;
        if let Some(error) = response.get("error") {
            return Err(format!("language server request failed: {error}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    };

    match timeout(LSP_TIMEOUT, run).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            shutdown_local_lsp_session(&session_key).await;
            Err(err)
        }
        Err(_) => {
            shutdown_local_lsp_session(&session_key).await;
            Err("language server request timed out".to_string())
        }
    }
}

async fn get_or_start_local_lsp_session(
    session_key: &str,
    project_root: &Path,
    server: &LspServerCommand,
    not_found_message: &str,
) -> Result<Arc<AsyncMutex<LocalLspSession>>, String> {
    if let Some(handle) = local_lsp_session_handle(session_key) {
        return Ok(handle);
    }

    let mut command = Command::new(&server.program);
    command
        .args(&server.args)
        .current_dir(project_root)
        .envs(crate::app_settings::get_login_shell_env().iter().cloned());
    let session = start_local_lsp_session(
        command,
        file_uri(project_root),
        local_workspace_name(project_root),
        not_found_message,
    )
    .await?;
    insert_local_lsp_session(session_key.to_string(), session);
    local_lsp_session_handle(session_key)
        .ok_or_else(|| "failed to store language server session".to_string())
}

async fn start_local_lsp_session(
    mut command: Command,
    root_uri: String,
    workspace_name: String,
    not_found_message: &str,
) -> Result<LocalLspSession, String> {
    crate::subprocess::configure_terminable_tokio_process_tree(&mut command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
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
    let response = read_response_with_id(&mut stdout, 1, None).await?;
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

async fn run_remote_lsp_request(
    app: &AppHandle,
    connection: &SshConnection,
    remote_root: &str,
    document_path: &str,
    server: &LspServerCommand,
    documents: Vec<LspDocumentSnapshot>,
    request: &LspDocumentRequest,
    method: &str,
    params: LspFeatureParams,
) -> Result<Value, String> {
    let language_id = language_id_for_path(Path::new(document_path))
        .ok_or_else(|| "language server is not supported for this file type".to_string())?;
    let session_key = format!("ssh:{}:{remote_root}:{language_id}", connection.id);
    run_remote_lsp_session_request(
        app,
        connection,
        session_key,
        remote_root,
        server,
        file_uri(Path::new(document_path)),
        documents,
        request,
        method,
        params,
        "failed to start remote language server over SSH",
    )
    .await
}

async fn run_remote_lsp_session_request(
    app: &AppHandle,
    connection: &SshConnection,
    session_key: String,
    remote_root: &str,
    server: &LspServerCommand,
    document_uri: String,
    documents: Vec<LspDocumentSnapshot>,
    request: &LspDocumentRequest,
    method: &str,
    params: LspFeatureParams,
    not_found_message: &str,
) -> Result<Value, String> {
    let run = async {
        let handle = get_or_start_remote_lsp_session(
            connection,
            &session_key,
            remote_root,
            server,
            not_found_message,
        )
        .await?;
        let mut session = handle.lock().await;
        session.sync_documents(documents).await?;
        let request_id = session.next_request_id();
        let request_params = lsp_feature_request_params(&document_uri, request, params);

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

        let diagnostics = LspDiagnosticEmitter {
            app: app.clone(),
            project_path: remote_root.to_string(),
        };
        let response =
            read_response_with_id(&mut session.stdout, request_id, Some(&diagnostics)).await?;
        if let Some(error) = response.get("error") {
            return Err(format!("language server request failed: {error}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    };

    match timeout(LSP_TIMEOUT, run).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            shutdown_remote_lsp_session(&session_key).await;
            Err(err)
        }
        Err(_) => {
            shutdown_remote_lsp_session(&session_key).await;
            Err("language server request timed out".to_string())
        }
    }
}

async fn get_or_start_remote_lsp_session(
    connection: &SshConnection,
    session_key: &str,
    remote_root: &str,
    server: &LspServerCommand,
    not_found_message: &str,
) -> Result<Arc<AsyncMutex<LocalLspSession>>, String> {
    if let Some(handle) = remote_lsp_session_handle(session_key) {
        return Ok(handle);
    }
    let command = tokio_ssh_command_for_remote_command(
        connection,
        build_remote_lsp_server_command(remote_root, server),
    );
    let session = start_local_lsp_session(
        command,
        file_uri(Path::new(remote_root)),
        remote_workspace_name(remote_root),
        not_found_message,
    )
    .await?;
    insert_remote_lsp_session(session_key.to_string(), session);
    remote_lsp_session_handle(session_key)
        .ok_or_else(|| "failed to store remote language server session".to_string())
}

fn lsp_feature_request_params(
    document_uri: &str,
    request: &LspDocumentRequest,
    params: LspFeatureParams,
) -> Value {
    match params {
        LspFeatureParams::Position { extra_params } => {
            let mut request_params = json!({
                "textDocument": { "uri": document_uri },
                "position": {
                    "line": request.line,
                    "character": request.character
                }
            });
            if let (Some(params), Some(object)) = (extra_params, request_params.as_object_mut()) {
                if let Some(extra) = params.as_object() {
                    for (key, value) in extra {
                        object.insert(key.clone(), value.clone());
                    }
                }
            }
            request_params
        }
        LspFeatureParams::DocumentRange => {
            let range = full_document_range(&request.content);
            json!({
                "textDocument": { "uri": document_uri },
                "range": range
            })
        }
        LspFeatureParams::Raw(value) => value,
    }
}

fn full_document_range(content: &str) -> LspRange {
    let mut end_line = 0_u32;
    let mut last_line = "";
    for (index, line) in content.split('\n').enumerate() {
        end_line = index as u32;
        last_line = line;
    }
    let last_line = last_line.strip_suffix('\r').unwrap_or(last_line);
    LspRange {
        start: LspPosition {
            line: 0,
            character: 0,
        },
        end: LspPosition {
            line: end_line,
            character: last_line.chars().map(|ch| ch.len_utf16() as u32).sum(),
        },
    }
}

async fn run_lsp_workspace_symbol_request(
    app: &AppHandle,
    project_root: &Path,
    server: &LspServerCommand,
    query: &str,
) -> Result<Value, String> {
    let session_key = format!("local:{}:typescript", project_root.to_string_lossy());
    let documents = lsp_session_open_document_snapshots(&session_key)?;
    run_local_lsp_workspace_symbol_session_request(
        app,
        session_key,
        project_root,
        server,
        documents,
        query,
        "typescript-language-server is not installed. Run: pnpm add -D typescript-language-server typescript",
    )
    .await
}

async fn run_local_lsp_workspace_symbol_session_request(
    app: &AppHandle,
    session_key: String,
    project_root: &Path,
    server: &LspServerCommand,
    documents: Vec<LspDocumentSnapshot>,
    query: &str,
    not_found_message: &str,
) -> Result<Value, String> {
    let run = async {
        let handle =
            get_or_start_local_lsp_session(&session_key, project_root, server, not_found_message)
                .await?;
        let mut session = handle.lock().await;
        session.sync_documents(documents).await?;
        let request_id = session.next_request_id();
        write_message(
            &mut session.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "workspace/symbol",
                "params": { "query": query }
            }),
        )
        .await?;

        let diagnostics = LspDiagnosticEmitter {
            app: app.clone(),
            project_path: project_root.to_string_lossy().to_string(),
        };
        let response =
            read_response_with_id(&mut session.stdout, request_id, Some(&diagnostics)).await?;
        if let Some(error) = response.get("error") {
            return Err(format!("language server request failed: {error}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    };

    match timeout(LSP_TIMEOUT, run).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            shutdown_local_lsp_session(&session_key).await;
            Err(err)
        }
        Err(_) => {
            shutdown_local_lsp_session(&session_key).await;
            Err("language server request timed out".to_string())
        }
    }
}

async fn run_remote_lsp_workspace_symbol_request(
    app: &AppHandle,
    connection: &SshConnection,
    remote_root: &str,
    server: &LspServerCommand,
    query: &str,
) -> Result<Value, String> {
    let session_key = format!("ssh:{}:{remote_root}:typescript", connection.id);
    let documents = lsp_session_open_document_snapshots(&session_key)?;
    run_remote_lsp_workspace_symbol_session_request(
        app,
        connection,
        session_key,
        remote_root,
        server,
        documents,
        query,
        "failed to start remote language server over SSH",
    )
    .await
}

async fn run_remote_lsp_workspace_symbol_session_request(
    app: &AppHandle,
    connection: &SshConnection,
    session_key: String,
    remote_root: &str,
    server: &LspServerCommand,
    documents: Vec<LspDocumentSnapshot>,
    query: &str,
    not_found_message: &str,
) -> Result<Value, String> {
    let run = async {
        let handle = get_or_start_remote_lsp_session(
            connection,
            &session_key,
            remote_root,
            server,
            not_found_message,
        )
        .await?;
        let mut session = handle.lock().await;
        session.sync_documents(documents).await?;
        let request_id = session.next_request_id();
        write_message(
            &mut session.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "workspace/symbol",
                "params": { "query": query }
            }),
        )
        .await?;

        let diagnostics = LspDiagnosticEmitter {
            app: app.clone(),
            project_path: remote_root.to_string(),
        };
        let response =
            read_response_with_id(&mut session.stdout, request_id, Some(&diagnostics)).await?;
        if let Some(error) = response.get("error") {
            return Err(format!("language server request failed: {error}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    };

    match timeout(LSP_TIMEOUT, run).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            shutdown_remote_lsp_session(&session_key).await;
            Err(err)
        }
        Err(_) => {
            shutdown_remote_lsp_session(&session_key).await;
            Err("language server request timed out".to_string())
        }
    }
}

async fn write_message<W: AsyncWriteExt + Unpin>(
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

async fn read_response_with_id<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    id: i64,
    diagnostics: Option<&LspDiagnosticEmitter>,
) -> Result<Value, String> {
    loop {
        let value = read_message(reader).await?;
        if let Some(emitter) = diagnostics {
            emit_lsp_diagnostics_if_present(emitter, &value);
        }
        if value.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(value);
        }
    }
}

async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Value, String> {
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

fn emit_lsp_diagnostics_if_present(emitter: &LspDiagnosticEmitter, value: &Value) {
    let Some(params) = value
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| *method == "textDocument/publishDiagnostics")
        .and_then(|_| value.get("params"))
    else {
        return;
    };
    let Some(event) = parse_lsp_diagnostics_event(&emitter.project_path, params) else {
        return;
    };
    let _ = emitter.app.emit("lsp://diagnostics", event);
}

fn parse_lsp_diagnostics_event(project_path: &str, params: &Value) -> Option<LspDiagnosticsEvent> {
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

fn command_available(program: &str) -> bool {
    let mut command = std::process::Command::new(program);
    crate::subprocess::configure_background_command(&mut command);
    command
        .arg("--version")
        .envs(crate::app_settings::get_login_shell_env().iter().cloned())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// 测试用的一次性临时项目根。**放在模块层而不是 `tests` 里**:`lsp/edit.rs` 搬走的
/// 两个 apply-workspace-edit 用例也要用它,提上来让两边共用一份定义(和本仓库
/// `dap/paths.rs` 的 `unique_test_dir` 同一手法)。
#[cfg(test)]
fn temp_project(name: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("aeroric-lsp-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_ssh_connection(id: &str) -> SshConnection {
        SshConnection {
            id: id.to_string(),
            name: "Test SSH".to_string(),
            group: None,
            host: "example.test".to_string(),
            port: 22,
            username: "tester".to_string(),
            identity_file: None,
            password: None,
            remote_path: Some("/srv/app".to_string()),
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 0,
            last_connected_at: None,
        }
    }

    #[test]
    fn detects_ts_js_language_ids() {
        assert_eq!(language_id_for_path("src/App.tsx"), Some("typescriptreact"));
        assert_eq!(language_id_for_path("src/App.ts"), Some("typescript"));
        assert_eq!(language_id_for_path("src/App.jsx"), Some("javascriptreact"));
        assert_eq!(language_id_for_path("src/App.js"), Some("javascript"));
        assert_eq!(language_id_for_path("src/App.rs"), None);
    }

    #[test]
    fn builds_default_typescript_language_server_command() {
        let command = default_server_command("typescript").unwrap();
        assert_eq!(command.program, "typescript-language-server");
        assert_eq!(command.args, vec!["--stdio"]);
    }

    #[test]
    fn builds_remote_language_server_command_with_quoted_root() {
        let command = build_remote_lsp_server_command(
            "/srv/app repo",
            &LspServerCommand {
                program: "typescript-language-server".to_string(),
                args: vec!["--stdio".to_string()],
            },
        );

        assert_eq!(
            command,
            "cd -- '/srv/app repo' && exec typescript-language-server --stdio"
        );
    }

    #[test]
    fn rejects_document_outside_project_root() {
        let project = temp_project("outside");
        let outside =
            std::env::temp_dir().join(format!("aeroric-lsp-outside-{}.ts", std::process::id()));
        fs::write(&outside, "const value = 1;\n").unwrap();

        let err = validate_document_path(&project, &outside).unwrap_err();

        assert!(err.contains("outside project root"));
    }

    #[test]
    fn remote_lsp_document_paths_stay_inside_project_root() {
        assert_eq!(
            validate_remote_lsp_document_path("/srv/app", "/srv/app/src/App.tsx").unwrap(),
            "/srv/app/src/App.tsx"
        );
        assert!(validate_remote_lsp_document_path("/srv/app", "/srv/app2/src/App.tsx").is_err());
        assert!(validate_remote_lsp_document_path("/srv/app", "/srv/app/../App.tsx").is_err());
        assert!(validate_remote_lsp_document_path("/srv/app", "src/App.tsx").is_err());
        assert!(validate_remote_lsp_document_path("/srv/app", "/srv/app/src/main.rs").is_err());
    }

    #[test]
    fn remote_lsp_edit_paths_reject_protected_metadata() {
        assert_eq!(
            validate_remote_lsp_edit_path("/srv/app", "/srv/app/src/App.tsx").unwrap(),
            "/srv/app/src/App.tsx"
        );
        assert!(validate_remote_lsp_edit_path("/srv/app", "/srv/app/.git/index").is_err());
        assert!(
            validate_remote_lsp_edit_path("/srv/app", "/srv/app/.aeroric/local-history/x").is_err()
        );
    }

    #[tokio::test]
    async fn tracks_local_lsp_document_lifecycle() {
        let project = temp_project("lifecycle");
        let file = project.join("src").join("App.ts");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "const value = 1;\n").unwrap();

        let open = lsp_open_document(
            project.to_string_lossy().to_string(),
            file.to_string_lossy().to_string(),
            "const value = 1;\n".to_string(),
            1,
        )
        .unwrap();
        assert_eq!(open.language_id, "typescript");
        assert_eq!(open.version, 1);
        assert_eq!(open.open_documents, 1);

        let changed = lsp_change_document(
            project.to_string_lossy().to_string(),
            file.to_string_lossy().to_string(),
            "const value = 2;\n".to_string(),
            2,
        )
        .unwrap();
        assert_eq!(changed.version, 2);
        assert_eq!(changed.open_documents, 1);

        let closed = lsp_close_document(
            project.to_string_lossy().to_string(),
            file.to_string_lossy().to_string(),
        )
        .await
        .unwrap();
        assert_eq!(closed.version, 2);
        assert_eq!(closed.open_documents, 0);
    }

    #[tokio::test]
    async fn shuts_down_local_lsp_project_sessions() {
        let project = temp_project("shutdown");
        let ts_file = project.join("src").join("App.ts");
        let js_file = project.join("src").join("helper.js");
        fs::create_dir_all(ts_file.parent().unwrap()).unwrap();
        fs::write(&ts_file, "const value: number = 1;\n").unwrap();
        fs::write(&js_file, "export const helper = 1;\n").unwrap();

        lsp_open_document(
            project.to_string_lossy().to_string(),
            ts_file.to_string_lossy().to_string(),
            "const value: number = 1;\n".to_string(),
            1,
        )
        .unwrap();
        lsp_open_document(
            project.to_string_lossy().to_string(),
            js_file.to_string_lossy().to_string(),
            "export const helper = 1;\n".to_string(),
            1,
        )
        .unwrap();

        assert_eq!(
            lsp_shutdown_project(project.to_string_lossy().to_string())
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            lsp_shutdown_project(project.to_string_lossy().to_string())
                .await
                .unwrap(),
            0
        );
        assert_eq!(local_lsp_session_count(), 0);
    }

    #[test]
    fn builds_lsp_snapshots_from_open_session_documents() {
        let project = temp_project("snapshots");
        let app_file = project.join("src").join("App.ts");
        let helper_file = project.join("src").join("helper.ts");
        fs::create_dir_all(app_file.parent().unwrap()).unwrap();
        fs::write(&app_file, "const app = 1;\n").unwrap();
        fs::write(&helper_file, "export const helper = 1;\n").unwrap();

        let project_path = project.to_string_lossy().to_string();
        let app_path = app_file
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let helper_path = helper_file
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        lsp_open_document(
            project_path.clone(),
            app_path.clone(),
            "const app = 2;\n".to_string(),
            7,
        )
        .unwrap();
        lsp_open_document(
            project_path.clone(),
            helper_path.clone(),
            "export const helper = 2;\n".to_string(),
            3,
        )
        .unwrap();

        let canonical_project_path = project
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let session_key = format!("local:{canonical_project_path}:typescript");
        let snapshots = lsp_session_document_snapshots(
            &session_key,
            app_path,
            "typescript".to_string(),
            "const app = 1;\n".to_string(),
        )
        .unwrap();

        assert_eq!(snapshots.len(), 2);
        assert!(snapshots.iter().any(|snapshot| {
            snapshot.path == helper_path
                && snapshot.version == 3
                && snapshot.content == "export const helper = 2;\n"
        }));
        assert!(snapshots
            .iter()
            .any(|snapshot| { snapshot.version == 7 && snapshot.content == "const app = 2;\n" }));
    }

    #[test]
    fn falls_back_to_request_content_when_document_is_not_open() {
        let snapshots = lsp_session_document_snapshots(
            "local:/missing:typescript",
            "/missing/src/App.ts".to_string(),
            "typescript".to_string(),
            "const fromRequest = true;\n".to_string(),
        )
        .unwrap();

        assert_eq!(
            snapshots,
            vec![LspDocumentSnapshot {
                path: "/missing/src/App.ts".to_string(),
                language_id: "typescript".to_string(),
                version: 1,
                content: "const fromRequest = true;\n".to_string(),
            }]
        );
    }

    #[test]
    fn workspace_symbol_snapshots_only_include_open_documents() {
        let project = temp_project("workspace-symbol-snapshots");
        let app_file = project.join("src").join("App.ts");
        fs::create_dir_all(app_file.parent().unwrap()).unwrap();
        fs::write(&app_file, "export const app = 1;\n").unwrap();

        let project_path = project.to_string_lossy().to_string();
        let app_path = app_file
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        lsp_open_document(
            project_path.clone(),
            app_path.clone(),
            "export const app = 2;\n".to_string(),
            4,
        )
        .unwrap();

        let canonical_project_path = project
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let snapshots = lsp_session_open_document_snapshots(&format!(
            "local:{canonical_project_path}:typescript"
        ))
        .unwrap();
        assert_eq!(
            snapshots,
            vec![LspDocumentSnapshot {
                path: app_path,
                language_id: "typescript".to_string(),
                version: 4,
                content: "export const app = 2;\n".to_string(),
            }]
        );
        assert!(
            lsp_session_open_document_snapshots("local:/missing:typescript")
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn remote_workspace_symbol_snapshots_use_connection_root_language_session() {
        let connection = test_ssh_connection("ssh-1");
        remote_lsp_open_document(
            connection.clone(),
            "/srv/app".to_string(),
            "/srv/app/src/App.ts".to_string(),
            "export const app = 2;\n".to_string(),
            4,
        )
        .unwrap();

        let snapshots =
            lsp_session_open_document_snapshots("ssh:ssh-1:/srv/app:typescript").unwrap();
        assert_eq!(
            snapshots,
            vec![LspDocumentSnapshot {
                path: "/srv/app/src/App.ts".to_string(),
                language_id: "typescript".to_string(),
                version: 4,
                content: "export const app = 2;\n".to_string(),
            }]
        );

        assert_eq!(
            remote_lsp_shutdown_project(connection, "/srv/app".to_string())
                .await
                .unwrap(),
            1
        );
    }

    #[test]
    fn parses_lsp_publish_diagnostics_event() {
        let event = parse_lsp_diagnostics_event(
            "/repo",
            &json!({
                "uri": "file:///repo/src/App.ts",
                "diagnostics": [
                    {
                        "range": {
                            "start": { "line": 2, "character": 4 },
                            "end": { "line": 2, "character": 9 }
                        },
                        "severity": 1,
                        "source": "typescript",
                        "code": 2322,
                        "message": "Type 'string' is not assignable to type 'number'."
                    }
                ]
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
    fn parses_definition_location_links() {
        let locations = parse_locations(Some(&json!([
            {
                "targetUri": "file:///repo/src/helper.ts",
                "targetRange": {
                    "start": { "line": 9, "character": 0 },
                    "end": { "line": 12, "character": 1 }
                },
                "targetSelectionRange": {
                    "start": { "line": 10, "character": 4 },
                    "end": { "line": 10, "character": 10 }
                }
            }
        ])));

        assert_eq!(
            locations,
            vec![LspLocation {
                uri: "file:///repo/src/helper.ts".to_string(),
                path: "/repo/src/helper.ts".to_string(),
                range: LspRange {
                    start: LspPosition {
                        line: 10,
                        character: 4
                    },
                    end: LspPosition {
                        line: 10,
                        character: 10
                    }
                }
            }]
        );
    }

    #[test]
    fn parses_single_definition_location() {
        let locations = parse_locations(Some(&json!({
            "uri": "file:///repo/src/App.tsx",
            "range": {
                "start": { "line": 2, "character": 8 },
                "end": { "line": 2, "character": 11 }
            }
        })));

        assert_eq!(
            locations,
            vec![LspLocation {
                uri: "file:///repo/src/App.tsx".to_string(),
                path: "/repo/src/App.tsx".to_string(),
                range: LspRange {
                    start: LspPosition {
                        line: 2,
                        character: 8
                    },
                    end: LspPosition {
                        line: 2,
                        character: 11
                    }
                }
            }]
        );
    }

    #[test]
    fn parses_completion_list_items_with_markdown_documentation() {
        let items = parse_completion_items(Some(&json!({
            "isIncomplete": false,
            "items": [
                {
                    "label": "helper",
                    "detail": "function helper(): string",
                    "documentation": {
                        "kind": "markdown",
                        "value": "Returns a helper value."
                    }
                }
            ]
        })));

        assert_eq!(
            items,
            vec![LspCompletionItem {
                label: "helper".to_string(),
                detail: Some("function helper(): string".to_string()),
                documentation: Some("Returns a helper value.".to_string())
            }]
        );
    }

    #[test]
    fn builds_document_range_request_params_for_inlay_hints() {
        let request = LspDocumentRequest {
            project_path: "/repo".to_string(),
            file_path: "/repo/src/App.tsx".to_string(),
            content: "const value = 1;\nconst emoji = '👍';".to_string(),
            line: 4,
            character: 3,
        };

        let params = lsp_feature_request_params(
            "file:///repo/src/App.tsx",
            &request,
            LspFeatureParams::DocumentRange,
        );

        assert_eq!(
            params,
            json!({
                "textDocument": { "uri": "file:///repo/src/App.tsx" },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 1, "character": 19 }
                }
            })
        );
        assert!(params.get("position").is_none());
    }
}
