//! P8-05 切片一百零九（ide 刀四）：LSP 生命周期面——server_status 与文档
//! 注册表簿记（`src-tauri/src/lsp.rs` verbatim 上收；hover/definition 等
//! 会话面与 remote 孪生留 Tauri，注册表经 pub 访问器共享）。login shell
//! 环境由调用方注入（server_status 的 PATH 探测需要）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::ide::diagnostics_face::DiagnosticItem;
use crate::runtime::subprocess::configure_background_command;

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

/// 切片一百零九：打开文档注册表（Tauri `LSP_DOCUMENTS` verbatim——宿主与
/// Embedded 桌面壳各自进程一份；remote 孪生与 hover 路径经 pub 访问器读写）。
pub static LSP_DOCUMENTS: LazyLock<Mutex<BTreeMap<String, BTreeMap<String, LspOpenDocument>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspOpenDocument {
    pub language_id: String,
    pub version: u32,
    pub content: String,
}

pub fn lsp_registry_upsert(
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

pub fn lsp_registry_close(
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

pub fn lsp_registry_shutdown(session_prefix: &str) -> Result<usize, String> {
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

pub fn lsp_server_status(
    project_path: String,
    file_path: String,
    envs: &[(String, String)],
) -> Result<LspServerStatus, String> {
    let document_path = validate_document_path(Path::new(&project_path), Path::new(&file_path))?;
    let language_id = language_id_for_path(&document_path).map(str::to_string);
    let command = language_id.as_deref().and_then(default_server_command);
    let available = command
        .as_ref()
        .map(|command| command_available(&command.program, envs))
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

pub fn local_lsp_lifecycle_context(
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

/// PATH 可用性探测(Tauri `command_available` verbatim——login env 注入)。
pub fn command_available(program: &str, envs: &[(String, String)]) -> bool {
    let mut command = std::process::Command::new(program);
    configure_background_command(&mut command);
    command
        .arg("--version")
        .envs(envs.iter().cloned())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// 切片一百零九：生命周期命令的宿主入口（Tauri 命令体 verbatim——本地上下文
/// 解析＋注册表簿记；Embedded/宿主共用；会话 shutdown 半边留 Tauri）。
pub fn lsp_open_document(
    project_path: &str,
    file_path: &str,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, project_key, file_key, language_id) =
        local_lsp_lifecycle_context(project_path, file_path)?;
    lsp_registry_upsert(
        session_key,
        project_key,
        file_key,
        language_id,
        content,
        version,
    )
}

pub fn lsp_change_document(
    project_path: &str,
    file_path: &str,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, project_key, file_key, language_id) =
        local_lsp_lifecycle_context(project_path, file_path)?;
    lsp_registry_upsert(
        session_key,
        project_key,
        file_key,
        language_id,
        content,
        version,
    )
}

pub fn lsp_close_document(
    project_path: &str,
    file_path: &str,
) -> Result<LspDocumentLifecycleSummary, String> {
    let (session_key, project_key, file_key, language_id) =
        local_lsp_lifecycle_context(project_path, file_path)?;
    lsp_registry_close(session_key, project_key, file_key, language_id)
}

pub fn lsp_shutdown_project(project_path: &str) -> Result<usize, String> {
    let project_root = Path::new(project_path)
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let session_prefix = format!("local:{}:", project_root.to_string_lossy());
    lsp_registry_shutdown(&session_prefix)
}

/// 切片一百零九：会话文档快照（Tauri hover 路径的读取面 verbatim——含请求
/// 文档的 fallback）。
pub fn lsp_session_document_snapshots(
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

pub fn lsp_session_open_document_snapshots(
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

/// 文档快照类型单一定义在 `ide::lsp_session`(会话面与注册表读取共用一份),
/// 这里 re-export 保持旧路径可用。
pub use crate::ide::lsp_session::LspDocumentSnapshot;
