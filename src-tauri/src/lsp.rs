//! LSP 命令壳(切片一百一十后):会话引擎、响应解析、DTO 与 uri 编码的
//! **单一定义全在 core**(`ide::lsp_session` / `ide::lsp_face`),本文件只剩
//! 三件 Tauri 侧的事:**SSH 适配**(remote 孪生的命令构建与字符路径守卫)、
//! **窗口 emit 端口**(diagnostics 出口)与**命令转发壳**。

use std::path::Path;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::path_guard::remote_path_has_relative_components;
use crate::ssh::SshConnection;
use aeroric_core::ide::lsp_face;
use aeroric_core::ide::lsp_session::engine;
use aeroric_core::ide::lsp_session::{feature_request_params, LspDocumentRequest, RequestParams};

// 单一定义在 core:DTO 与两簇纯函数全按旧名 re-export,调用点零漂移。
pub use aeroric_core::ide::lsp_face::{
    default_server_command, language_id_for_path, LspDocumentLifecycleSummary, LspServerCommand,
    LspServerStatus,
};
pub use aeroric_core::ide::lsp_session::engine::{
    close_local_lsp_session_document, close_remote_lsp_session_document,
    shutdown_local_lsp_session, take_local_lsp_sessions_with_prefix, take_remote_lsp_session,
    take_remote_lsp_sessions_with_prefix, CommandSpec, DiagnosticsSink, ServerLauncher,
};
pub use aeroric_core::ide::lsp_session::{
    apply_remote_workspace_edit_for_root, apply_workspace_edit_for_root, file_uri,
    parse_code_actions, parse_completion_items, parse_document_symbols, parse_hover,
    parse_inlay_hints, parse_locations, parse_signature_help, parse_workspace_edit,
    parse_workspace_symbols, LspApplyWorkspaceEditSummary, LspCodeAction, LspCommand,
    LspCompletionItem, LspDocumentSnapshot, LspHover, LspInlayHint, LspLocation, LspSignatureHelp,
    LspSymbol, LspWorkspaceEdit, RemoteEditIo,
};

const MAX_REMOTE_LSP_EDIT_FILE_BYTES: u64 = 2 * 1024 * 1024;

fn window_diagnostics_sink(app: &AppHandle) -> DiagnosticsSink {
    let app = app.clone();
    std::sync::Arc::new(move |event: engine::LspDiagnosticsEvent| {
        let _ = app.emit("lsp://diagnostics", event);
    })
}

struct TauriLocalLauncher;

impl ServerLauncher for TauriLocalLauncher {
    fn local(&self, project_root: &Path, server: &LspServerCommand) -> CommandSpec {
        CommandSpec {
            program: server.program.clone(),
            args: server.args.clone(),
            current_dir: Some(project_root.to_path_buf()),
            envs: crate::app_settings::get_login_shell_env().to_vec(),
        }
    }

    fn remote(
        &self,
        _root_uri: &str,
        _remote_root: &str,
        _server: &LspServerCommand,
    ) -> CommandSpec {
        unreachable!("local launcher has no remote server command")
    }
}

/// 远端 server 的启动命令:仍是本文件原来的 ssh 包装(`cd -- '<root>' &&
/// exec <server>` 经 `ssh_command_spec_for_remote_command`),只是把「怎么拉
/// 进程」交给 core 的会话引擎。core 看不到 `SshConnection`,所以适配器持住它。
struct TauriRemoteLauncher<'a> {
    connection: &'a SshConnection,
}

impl ServerLauncher for TauriRemoteLauncher<'_> {
    fn local(&self, _project_root: &Path, _server: &LspServerCommand) -> CommandSpec {
        unreachable!("remote launcher has no local server command")
    }

    fn remote(&self, _root_uri: &str, remote_root: &str, server: &LspServerCommand) -> CommandSpec {
        let spec = crate::ssh::ssh_command_spec_for_remote_command(
            self.connection,
            build_remote_lsp_server_command(remote_root, server),
        );
        let mut envs = spec.env;
        envs.push((
            "PATH".to_string(),
            crate::app_settings::get_login_shell_path().to_string(),
        ));
        CommandSpec {
            program: spec.program,
            args: spec.args,
            current_dir: None,
            envs,
        }
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

/// 远端读文件:走 ssh 执行 `wc -c` 上限判定 + `cat`(上限 2 MiB)。
fn run_remote_lsp_output(
    connection: &SshConnection,
    remote_command: String,
) -> Result<Vec<u8>, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, remote_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

/// 远端写文件:ssh 把内容经 stdin 管道灌进去。
fn write_remote_lsp_text_file(
    connection: &SshConnection,
    remote_path: &str,
    content: &str,
) -> Result<(), String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        remote_write_text_command(remote_path),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    {
        use std::io::Write as _;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open ssh stdin".to_string())?;
        stdin
            .write_all(content.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
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

fn remote_read_text_command(remote_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(remote_path);
    format!(
        "size=$(wc -c < {path}) && [ \"$size\" -le {MAX_REMOTE_LSP_EDIT_FILE_BYTES} ] && cat -- {path}"
    )
}

fn remote_write_text_command(remote_path: &str) -> String {
    format!("cat > {}", crate::ssh::shell_quote_posix(remote_path))
}

/// 一次 LSP 请求的参数形态别名(位置型/整文档范围型/原始值)。
type Params = RequestParams;

async fn request_lsp_feature(
    app: &tauri::AppHandle,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    request_lsp_feature_with_params(app, request, method, None).await
}

/// 本地面命令的转发入口:External 走宿主(方法名与宿主注册逐字一致),
/// Embedded 在进程内直接跑同一份 core 引擎。
async fn dispatch_local(
    app: &tauri::AppHandle,
    method: &str,
    params: Value,
    embedded: impl std::future::Future<Output = Result<Value, String>>,
) -> Result<Value, String> {
    crate::host_dispatch::dispatch(app, method, params, embedded).await
}

async fn request_lsp_feature_with_params(
    app: &tauri::AppHandle,
    request: &LspDocumentRequest,
    method: &str,
    extra_params: Option<Value>,
) -> Result<Value, String> {
    request_with_mode(app, request, method, Params::Position { extra_params }).await
}

async fn request_lsp_document_range_feature(
    app: &tauri::AppHandle,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    request_with_mode(app, request, method, Params::DocumentRange).await
}

async fn request_with_mode(
    app: &tauri::AppHandle,
    request: &LspDocumentRequest,
    method: &str,
    params: Params,
) -> Result<Value, String> {
    let project_root = Path::new(&request.project_path)
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let document_path =
        lsp_face::validate_document_path(&project_root, Path::new(&request.file_path))?;
    let language_id = lsp_face::language_id_for_path(&document_path)
        .ok_or_else(|| "language server is not supported for this file type".to_string())?;
    let server = lsp_face::default_server_command(language_id)
        .ok_or_else(|| "language server is not configured for this file type".to_string())?;
    let session_key = format!("local:{}:{language_id}", project_root.to_string_lossy());
    let documents = lsp_face::lsp_session_document_snapshots(
        &session_key,
        document_path.to_string_lossy().to_string(),
        language_id.to_string(),
        request.content.clone(),
    )?;
    engine::run_lsp_session_request(
        true,
        &TauriLocalLauncher,
        Some(&window_diagnostics_sink(app)),
        session_key,
        &project_root,
        &server,
        file_uri(&project_root),
        display_name(&project_root),
        file_uri(&document_path),
        documents,
        request,
        method,
        params,
        "typescript-language-server is not installed. Run: pnpm add -D typescript-language-server typescript",
    )
    .await
}

async fn request_remote_lsp_feature(
    app: &tauri::AppHandle,
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

async fn request_remote_lsp_feature_with_params(
    app: &tauri::AppHandle,
    connection: &SshConnection,
    remote_project_path: &str,
    request: &LspDocumentRequest,
    method: &str,
    extra_params: Option<Value>,
) -> Result<Value, String> {
    request_remote_with_mode(
        app,
        connection,
        remote_project_path,
        request,
        method,
        Params::Position { extra_params },
    )
    .await
}

async fn request_remote_lsp_document_range_feature(
    app: &tauri::AppHandle,
    connection: &SshConnection,
    remote_project_path: &str,
    request: &LspDocumentRequest,
    method: &str,
) -> Result<Value, String> {
    request_remote_with_mode(
        app,
        connection,
        remote_project_path,
        request,
        method,
        Params::DocumentRange,
    )
    .await
}

async fn request_remote_with_mode(
    app: &tauri::AppHandle,
    connection: &SshConnection,
    remote_project_path: &str,
    request: &LspDocumentRequest,
    method: &str,
    params: Params,
) -> Result<Value, String> {
    let remote_root = normalize_remote_lsp_path(remote_project_path, "Remote project path")?;
    let document_path = validate_remote_lsp_document_path(&remote_root, &request.file_path)?;
    let language_id = lsp_face::language_id_for_path(Path::new(&document_path))
        .ok_or_else(|| "language server is not supported for this file type".to_string())?;
    let server = lsp_face::default_server_command(language_id)
        .ok_or_else(|| "language server is not configured for this file type".to_string())?;
    let session_key = format!("ssh:{}:{remote_root}:{language_id}", connection.id);
    let documents = lsp_face::lsp_session_document_snapshots(
        &session_key,
        document_path.clone(),
        language_id.to_string(),
        request.content.clone(),
    )?;
    let launcher = TauriRemoteLauncher { connection };
    engine::run_lsp_session_request(
        false,
        &launcher,
        Some(&window_diagnostics_sink(app)),
        session_key,
        Path::new(&remote_root),
        &server,
        file_uri(Path::new(&remote_root)),
        remote_workspace_name(&remote_root),
        file_uri(Path::new(&document_path)),
        documents,
        request,
        method,
        params,
        "failed to start remote language server over SSH",
    )
    .await
}

async fn run_lsp_workspace_symbol_request(
    app: &tauri::AppHandle,
    project_root: &Path,
    server: &LspServerCommand,
    query: &str,
) -> Result<Value, String> {
    let session_key = format!("local:{}:typescript", project_root.to_string_lossy());
    let documents = lsp_face::lsp_session_open_document_snapshots(&session_key)?;
    run_workspace_symbol_session_request(
        app,
        &TauriLocalLauncher,
        true,
        session_key,
        project_root,
        server,
        documents,
        query,
        "typescript-language-server is not installed. Run: pnpm add -D typescript-language-server typescript",
    )
    .await
}

async fn run_remote_lsp_workspace_symbol_request(
    app: &tauri::AppHandle,
    connection: &SshConnection,
    remote_root: &str,
    server: &LspServerCommand,
    query: &str,
) -> Result<Value, String> {
    let session_key = format!("ssh:{}:{remote_root}:typescript", connection.id);
    let documents = lsp_face::lsp_session_open_document_snapshots(&session_key)?;
    let launcher = TauriRemoteLauncher { connection };
    run_workspace_symbol_session_request(
        app,
        &launcher,
        false,
        session_key,
        Path::new(remote_root),
        server,
        documents,
        query,
        "failed to start remote language server over SSH",
    )
    .await
}

/// 工作区符号请求:`workspace/symbol` 的参数形态与文档请求不同(只有 query),
/// 所以不复用 `run_lsp_session_request` 的 `feature_request_params`,而是把
/// 参数直接以 `RequestParams::Raw` 交给同一引擎(超时/settle/diagnostics 语义
/// 与其它请求逐字相同)。
async fn run_workspace_symbol_session_request(
    app: &tauri::AppHandle,
    launcher: &dyn engine::ServerLauncher,
    local: bool,
    session_key: String,
    project_root: &Path,
    server: &LspServerCommand,
    documents: Vec<LspDocumentSnapshot>,
    query: &str,
    not_found_message: &str,
) -> Result<Value, String> {
    let request = LspDocumentRequest {
        project_path: project_root.to_string_lossy().to_string(),
        file_path: project_root.to_string_lossy().to_string(),
        content: String::new(),
        line: 0,
        character: 0,
    };
    let root_uri = file_uri(project_root);
    let workspace_name = if local {
        display_name(project_root)
    } else {
        remote_workspace_name(&project_root.to_string_lossy())
    };
    let params = RequestParams::Raw(serde_json::json!({ "query": query }));
    let _ = feature_request_params; // 参数构造在 core,这里显式声明走 Raw 分支
    engine::run_lsp_session_request(
        local,
        launcher,
        Some(&window_diagnostics_sink(app)),
        session_key,
        project_root,
        server,
        root_uri,
        workspace_name,
        file_uri(project_root),
        documents,
        &request,
        "workspace/symbol",
        params,
        not_found_message,
    )
    .await
}

fn display_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string()
}

// ── 切片一百零九:LSP 生命周期面改宿主转发壳。注册表簿记在 core
// `ide::lsp_face`;close 的会话 shutdown 半边留本闭包;remote 孪生留本文件
// SSH 适配层(注册表经 core pub 访问器共享)。──

#[tauri::command]
pub async fn lsp_server_status<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
    file_path: String,
) -> Result<LspServerStatus, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.lsp_server_status",
        serde_json::json!({ "projectPath": project_path, "filePath": file_path }),
        async {
            lsp_face::lsp_server_status(
                project_path,
                file_path,
                crate::app_settings::get_login_shell_env(),
            )
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn lsp_open_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
    file_path: String,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.lsp_open_document",
        serde_json::json!({
            "projectPath": project_path,
            "filePath": file_path,
            "content": content,
            "version": version
        }),
        async {
            lsp_face::lsp_open_document(&project_path, &file_path, content, version)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn lsp_change_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
    file_path: String,
    content: String,
    version: u32,
) -> Result<LspDocumentLifecycleSummary, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.lsp_change_document",
        serde_json::json!({
            "projectPath": project_path,
            "filePath": file_path,
            "content": content,
            "version": version
        }),
        async {
            lsp_face::lsp_change_document(&project_path, &file_path, content, version)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn lsp_close_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
    file_path: String,
) -> Result<LspDocumentLifecycleSummary, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.lsp_close_document",
        serde_json::json!({ "projectPath": project_path, "filePath": file_path }),
        // Embedded 与宿主走同一份实现(切片一百一十):注册表簿记**和**会话
        // 回收都在此。External 判决下这个 future 不会被 poll,所以宿主那份
        // 才是 External 模式的承重实现(见 host `ide_services.rs`)。
        async {
            let (session_key, project_key, file_key, language_id) =
                lsp_face::local_lsp_lifecycle_context(&project_path, &file_path)?;
            let summary = lsp_face::lsp_registry_close(
                session_key.clone(),
                project_key,
                file_key.clone(),
                language_id,
            )?;
            close_local_lsp_session_document(&session_key, &file_key).await;
            if summary.open_documents == 0 {
                shutdown_local_lsp_session(&session_key).await;
            }
            serde_json::to_value(summary).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn lsp_shutdown_project<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
) -> Result<usize, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.lsp_shutdown_project",
        serde_json::json!({ "projectPath": project_path }),
        async {
            let project_root = std::path::Path::new(&project_path)
                .canonicalize()
                .map_err(|err| format!("failed to resolve project root: {err}"))?;
            let closed = lsp_face::lsp_shutdown_project(&project_path)?;
            let session_prefix = format!("local:{}:", project_root.to_string_lossy());
            for handle in take_local_lsp_sessions_with_prefix(&session_prefix) {
                engine::shutdown_lsp_handle(handle).await;
            }
            serde_json::to_value(closed).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    lsp_face::lsp_registry_upsert(
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
    lsp_face::lsp_registry_upsert(
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
    let summary = lsp_face::lsp_registry_close(
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
    let closed_documents = lsp_face::lsp_registry_shutdown(&session_prefix)?;
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
    let value = dispatch_local(
        &app,
        "ide.lsp_hover",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character
        }),
        async {
            let result = request_lsp_feature(&app, &request, "textDocument/hover").await?;
            serde_json::to_value(parse_hover(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    let value = dispatch_local(
        &app,
        "ide.lsp_definition",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character
        }),
        async {
            let result = request_lsp_feature(&app, &request, "textDocument/definition").await?;
            serde_json::to_value(parse_locations(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    let value = dispatch_local(
        &app,
        "ide.lsp_references",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character
        }),
        async {
            let result = request_lsp_feature(&app, &request, "textDocument/references").await?;
            serde_json::to_value(parse_locations(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    let value = dispatch_local(
        &app,
        "ide.lsp_rename",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character,
            "newName": new_name
        }),
        async {
            let result = request_lsp_feature_with_params(
                &app,
                &request,
                "textDocument/rename",
                Some(json!({ "newName": new_name })),
            )
            .await?;
            parse_workspace_edit(Some(&result))
                .ok_or_else(|| "rename returned no edits".to_string())
                .and_then(|edit| serde_json::to_value(edit).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    app: AppHandle,
    project_path: String,
    edit: LspWorkspaceEdit,
) -> Result<LspApplyWorkspaceEditSummary, String> {
    let value = dispatch_local(
        &app,
        "ide.lsp_apply_workspace_edit",
        serde_json::json!({ "projectPath": project_path, "edit": edit }),
        async {
            tauri::async_runtime::spawn_blocking(move || {
                apply_workspace_edit_for_root(Path::new(&project_path), &edit)
            })
            .await
            .map_err(|err| err.to_string())?
            .and_then(|summary| serde_json::to_value(summary).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remote_lsp_apply_workspace_edit(
    connection: SshConnection,
    remote_project_path: String,
    edit: LspWorkspaceEdit,
) -> Result<LspApplyWorkspaceEditSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 远端 IO 端口由本适配层实现(ssh 命令);core 负责路径守卫调用点、
        // 文本区间算法与失败回滚,不依赖连接类型。
        let io = SshRemoteEditIo {
            connection: &connection,
        };
        apply_remote_workspace_edit_for_root(
            &io,
            &remote_project_path,
            validate_remote_lsp_edit_path,
            &edit,
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

/// 远端文件的读/写:仍是本文件原来的 ssh 命令(stderr 非零即错、写入走
/// stdin 管道),只是把它封成 core 的 `RemoteEditIo` 端口。
struct SshRemoteEditIo<'a> {
    connection: &'a SshConnection,
}

impl RemoteEditIo for SshRemoteEditIo<'_> {
    fn read_file(&self, remote_path: &str) -> Result<String, String> {
        let output = run_remote_lsp_output(self.connection, remote_read_text_command(remote_path))?;
        String::from_utf8(output).map_err(|err| err.to_string())
    }

    fn write_file(&self, remote_path: &str, content: &str) -> Result<(), String> {
        write_remote_lsp_text_file(self.connection, remote_path, content)
    }
}

#[tauri::command]
pub async fn lsp_completion(
    app: AppHandle,
    request: LspDocumentRequest,
) -> Result<Vec<LspCompletionItem>, String> {
    let value = dispatch_local(
        &app,
        "ide.lsp_completion",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character
        }),
        async {
            let result = request_lsp_feature(&app, &request, "textDocument/completion").await?;
            serde_json::to_value(parse_completion_items(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    let value = dispatch_local(
        &app,
        "ide.lsp_signature_help",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character
        }),
        async {
            let result = request_lsp_feature(&app, &request, "textDocument/signatureHelp").await?;
            serde_json::to_value(parse_signature_help(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    let value = dispatch_local(
        &app,
        "ide.lsp_code_actions",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character,
            "diagnostics": diagnostics
        }),
        async {
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
            serde_json::to_value(parse_code_actions(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    dispatch_local(
        &app,
        "ide.lsp_execute_command",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character,
            "command": { "title": command.title, "command": command.command, "arguments": command.arguments }
        }),
        async {
            request_with_mode(
                &app,
                &request,
                "workspace/executeCommand",
                RequestParams::Raw(json!({
                    "command": command.command,
                    "arguments": command.arguments
                })),
            )
            .await?;
            Ok(Value::Null)
        },
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
    request_remote_with_mode(
        &app,
        &connection,
        &remote_project_path,
        &request,
        "workspace/executeCommand",
        RequestParams::Raw(json!({
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
    let value = dispatch_local(
        &app,
        "ide.lsp_document_symbols",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character
        }),
        async {
            let uri = file_uri(Path::new(&request.file_path));
            let result = request_lsp_feature(&app, &request, "textDocument/documentSymbol").await?;
            serde_json::to_value(parse_document_symbols(Some(&result), &uri))
                .map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    let value = dispatch_local(
        &app,
        "ide.lsp_inlay_hints",
        serde_json::json!({
            "projectPath": request.project_path,
            "filePath": request.file_path,
            "content": request.content,
            "line": request.line,
            "character": request.character
        }),
        async {
            let result =
                request_lsp_document_range_feature(&app, &request, "textDocument/inlayHint")
                    .await?;
            serde_json::to_value(parse_inlay_hints(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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
    let value = dispatch_local(
        &app,
        "ide.lsp_workspace_symbols",
        serde_json::json!({ "projectPath": project_path, "query": query }),
        async {
            let project_root = Path::new(&project_path)
                .canonicalize()
                .map_err(|err| format!("failed to resolve project root: {err}"))?;
            let server = default_server_command("typescript").ok_or_else(|| {
                "language server is not configured for workspace symbols".to_string()
            })?;
            let result =
                run_lsp_workspace_symbol_request(&app, &project_root, &server, &query).await?;
            serde_json::to_value(parse_workspace_symbols(Some(&result))).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
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

/// 测试用的一次性临时项目根(与 core `ide::lsp_session` 同形;这里保留一份
/// 供本文件剩余的命令级用例使用)。
#[cfg(test)]
fn temp_project(name: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("aeroric-lsp-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::lsp_face::validate_document_path;
    use aeroric_core::ide::lsp_session::{LspPosition, LspRange};
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
            has_password: false,
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

        let open = lsp_face::lsp_open_document(
            project.to_string_lossy().as_ref(),
            file.to_string_lossy().as_ref(),
            "const value = 1;\n".to_string(),
            1,
        )
        .unwrap();
        assert_eq!(open.language_id, "typescript");
        assert_eq!(open.version, 1);
        assert_eq!(open.open_documents, 1);

        let changed = lsp_face::lsp_change_document(
            project.to_string_lossy().as_ref(),
            file.to_string_lossy().as_ref(),
            "const value = 2;\n".to_string(),
            2,
        )
        .unwrap();
        assert_eq!(changed.version, 2);
        assert_eq!(changed.open_documents, 1);

        let closed = lsp_face::lsp_close_document(
            project.to_string_lossy().as_ref(),
            file.to_string_lossy().as_ref(),
        )
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

        lsp_face::lsp_open_document(
            project.to_string_lossy().as_ref(),
            ts_file.to_string_lossy().as_ref(),
            "const value: number = 1;\n".to_string(),
            1,
        )
        .unwrap();
        lsp_face::lsp_open_document(
            project.to_string_lossy().as_ref(),
            js_file.to_string_lossy().as_ref(),
            "export const helper = 1;\n".to_string(),
            1,
        )
        .unwrap();

        assert_eq!(
            lsp_face::lsp_shutdown_project(project.to_string_lossy().as_ref()).unwrap(),
            2
        );
        assert_eq!(
            lsp_face::lsp_shutdown_project(project.to_string_lossy().as_ref()).unwrap(),
            0
        );
        // 会话表清空的可观察形状:按前缀取不到任何句柄(计数口是 core 内部
        // 测试专用,跨 crate 不可见——这里钉同一个事实)。
        assert!(engine::take_local_lsp_sessions_with_prefix("local:").is_empty());
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
        lsp_face::lsp_open_document(
            project_path.as_str(),
            app_path.as_str(),
            "const app = 2;\n".to_string(),
            7,
        )
        .unwrap();
        lsp_face::lsp_open_document(
            project_path.as_str(),
            helper_path.as_str(),
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
        let snapshots = lsp_face::lsp_session_document_snapshots(
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
        let snapshots = lsp_face::lsp_session_document_snapshots(
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
        lsp_face::lsp_open_document(
            project_path.as_str(),
            app_path.as_str(),
            "export const app = 2;\n".to_string(),
            4,
        )
        .unwrap();

        let canonical_project_path = project
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let snapshots = lsp_face::lsp_session_open_document_snapshots(&format!(
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
            lsp_face::lsp_session_open_document_snapshots("local:/missing:typescript")
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
            lsp_face::lsp_session_open_document_snapshots("ssh:ssh-1:/srv/app:typescript").unwrap();
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

        let params = feature_request_params(
            "file:///repo/src/App.tsx",
            &request,
            RequestParams::DocumentRange,
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
