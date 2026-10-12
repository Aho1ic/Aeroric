//! P8-05 切片一百一十(ide 刀五):LSP **会话面**的宿主服务。
//!
//! 会话引擎在 core `ide::lsp_session::engine`(帧读写/握手/超时/settle 只有
//! 一份实现);本文件是宿主侧的三个适配:**方法注册**、**diagnostics 出口**
//! (落 EventLog)与 **server 启动器**(program/env 由装配方注入)。
//!
//! 远端孪生(`ide.remote_lsp_*`)不在本文件:SSH 是客户端的连接面(切片94
//! 先例),宿主只服务本地面;`local_target_error` 保证带远端 target 的请求被
//! 逐字拒绝,而不是静默回落到本地。

use std::path::Path;
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::events::log::EventLog;
use aeroric_core::ide::lsp_face;
use aeroric_core::ide::lsp_session::engine;
use aeroric_core::ide::lsp_session::{
    file_uri, parse_code_actions, parse_completion_items, parse_document_symbols, parse_hover,
    parse_inlay_hints, parse_locations, parse_signature_help, parse_workspace_edit,
    parse_workspace_symbols, LspDocumentRequest, LspWorkspaceEdit, RequestParams,
};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde::de::DeserializeOwned;

use crate::dispatcher::{failure, ServiceSet};
use crate::local_targets::local_target_error;

use crate::services::ok;

/// 宿主装配注入的语言服务器信息。`program` 为空/None = 该宿主没有 LSP 能力
/// (方法根本不注册,客户端拿到 method-not-found,而不是一个起不来进程的空壳)。
#[derive(Debug, Clone, Default)]
pub struct LspServerInjection {
    /// 语言服务器可执行名;**给了就替代 core 发现出来的默认命令**。
    pub program: Option<String>,
    /// 与该可执行名配套的参数(替代默认命令的参数)。真实 server 一般留空,
    /// 由 core 的默认命令给 `--stdio`;测试注入的包装器(sh/fake server)用得上。
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
    /// diagnostics 落 EventLog 用的事件名;默认与旧窗口通道同字面量
    /// (`lsp://diagnostics`,ADR-007 的 v1 主题名另记)。
    pub diagnostics_event: Option<String>,
}

/// 本地面 server 启动器:program(注入优先)/args + 项目根 cwd + 注入的 env。
struct InjectedLauncher {
    program: String,
    args: Vec<String>,
    envs: Vec<(String, String)>,
}

impl engine::ServerLauncher for InjectedLauncher {
    fn local(
        &self,
        project_root: &Path,
        server: &lsp_face::LspServerCommand,
    ) -> engine::CommandSpec {
        // 注入的 program/args 覆盖默认发现;`server` 只用来判定可服务性
        // (core 已经用窄表确认过语言 id)。
        let _ = server;
        engine::CommandSpec {
            program: self.program.clone(),
            args: self.args.clone(),
            current_dir: Some(project_root.to_path_buf()),
            envs: self.envs.clone(),
        }
    }

    fn remote(
        &self,
        _root_uri: &str,
        _remote_root: &str,
        _server: &lsp_face::LspServerCommand,
    ) -> engine::CommandSpec {
        unreachable!("host serves the local LSP face only; remote twins stay client-side")
    }
}

/// 会话面注册。`injection` 没给 = 这台宿主没有语言服务器能力(一条都不注册)。
pub fn register_lsp_services(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    log: Arc<EventLog>,
    injection: Option<LspServerInjection>,
) {
    let Some(injection) = injection else { return };
    let Some(program) = injection.program.clone().filter(|p| !p.is_empty()) else {
        return;
    };
    let launcher = Arc::new(InjectedLauncher {
        program,
        args: injection.args.clone(),
        envs: injection.envs.clone(),
    });
    let event_name = injection
        .diagnostics_event
        .unwrap_or_else(|| "lsp://diagnostics".to_string());
    let sink: engine::DiagnosticsSink = {
        let log = Arc::clone(&log);
        Arc::new(move |event: engine::LspDiagnosticsEvent| {
            // 载荷形状 = core 的 `LspDiagnosticsEvent`(serde camelCase),宿主
            // EventLog 逐字记住。
            if let Ok(value) = serde_json::to_value(&event) {
                log.append(&event_name, value);
            }
        })
    };

    // ── 位置型(projectPath/filePath/content/line/character)──
    // 注册名写成字面量的 `set.register("<name>", …)`:host-coverage 的唯一
    // 真相源就是这些调用点(它不猜辅助函数),经 helper 传名会让这 6 条命令
    // 在覆盖率台账里凭空消失。辅助函数只负责造 handler。
    set.register(
        "ide.lsp_hover",
        position_handler(
            &context,
            &launcher,
            &sink,
            "textDocument/hover",
            RequestParams::Position { extra_params: None },
            |value| {
                Ok(serde_json::to_value(parse_hover(Some(value)))
                    .unwrap_or(serde_json::Value::Null))
            },
        ),
    );
    set.register(
        "ide.lsp_definition",
        position_handler(
            &context,
            &launcher,
            &sink,
            "textDocument/definition",
            RequestParams::Position { extra_params: None },
            |value| serde_json::to_value(parse_locations(Some(value))).map_err(|e| e.to_string()),
        ),
    );
    set.register(
        "ide.lsp_references",
        position_handler(
            &context,
            &launcher,
            &sink,
            "textDocument/references",
            RequestParams::Position { extra_params: None },
            |value| serde_json::to_value(parse_locations(Some(value))).map_err(|e| e.to_string()),
        ),
    );
    set.register(
        "ide.lsp_completion",
        position_handler(
            &context,
            &launcher,
            &sink,
            "textDocument/completion",
            RequestParams::Position { extra_params: None },
            |value| {
                serde_json::to_value(parse_completion_items(Some(value))).map_err(|e| e.to_string())
            },
        ),
    );
    set.register(
        "ide.lsp_signature_help",
        position_handler(
            &context,
            &launcher,
            &sink,
            "textDocument/signatureHelp",
            RequestParams::Position { extra_params: None },
            |value| {
                serde_json::to_value(parse_signature_help(Some(value))).map_err(|e| e.to_string())
            },
        ),
    );
    set.register(
        "ide.lsp_inlay_hints",
        position_handler(
            &context,
            &launcher,
            &sink,
            "textDocument/inlayHint",
            // legacy 的 inlay hints 走整文档 range,不是 position。
            RequestParams::DocumentRange,
            |value| serde_json::to_value(parse_inlay_hints(Some(value))).map_err(|e| e.to_string()),
        ),
    );
    // 文档符号:返回的每个 symbol 都要带 uri/path,documents 里的 uri 由
    // 请求文档决定,所以投影函数多一个 uri 参数。
    register_document_symbols(set, &context, &launcher, &sink);
    // 重命名:额外 new_name 参数,返回 WorkspaceEdit。
    register_rename(set, &context, &launcher, &sink);
    // 代码动作:额外 diagnostics 数组。
    register_code_actions(set, &context, &launcher, &sink);
    // 执行命令:原始参数(command/arguments)。
    register_execute_command(set, &context, &launcher, &sink);
    // 工作区符号:只有 projectPath + query。
    register_workspace_symbols(set, &context, &launcher, &sink);
    // 工作区编辑落盘:纯本地文件面,不经会话。
    register_apply_workspace_edit(set, &context);
}

fn position_params(
    request: &Request,
) -> Result<(String, String, String, u32, u32), (ErrorCode, String)> {
    let project_path: String =
        param(request, "projectPath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    let file_path: String =
        param(request, "filePath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    let content: String =
        param(request, "content").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    let line: u32 = param(request, "line").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    let character: u32 =
        param(request, "character").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    if let Some(error) = local_target_error(
        request,
        &project_path,
        "LSP target is not hosted: SSH/WSL language servers are unavailable",
    ) {
        return Err((ErrorCode::InvalidArgument, error_to_string(error)));
    }
    Ok((project_path, file_path, content, line, character))
}

/// 一次本地面会话请求的公共前半:解析上下文 → 文档快照 → 引擎调用。
async fn run_local_feature(
    launcher: &InjectedLauncher,
    sink: &engine::DiagnosticsSink,
    request: &LspDocumentRequest,
    method: &str,
    params: RequestParams,
) -> Result<serde_json::Value, String> {
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
    let workspace_name = project_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string();
    engine::run_lsp_session_request(
        true,
        launcher,
        Some(sink),
        session_key,
        &project_root,
        &server,
        file_uri(&project_root),
        workspace_name,
        file_uri(&document_path),
        documents,
        request,
        method,
        params,
        "typescript-language-server is not installed. Run: pnpm add -D typescript-language-server typescript",
    )
    .await
}

/// 位置型命令的 handler 工厂。`params` 是**旧命令逐字用的参数形态**:inlay
/// hints 在 legacy 走 `request_lsp_document_range_feature`(整文档 range),其余
/// 走位置型;把这个差异留在 host 面,否则同一前端调用会因 Embedded/External
/// 模式不同而真的发出两种 params(legacy 两种模式都发 range)。
fn position_handler(
    context: &Arc<CoreContext>,
    launcher: &Arc<InjectedLauncher>,
    sink: &engine::DiagnosticsSink,
    method: &'static str,
    params: RequestParams,
    project: fn(&serde_json::Value) -> Result<serde_json::Value, String>,
) -> crate::dispatcher::Handler {
    let launcher = Arc::clone(launcher);
    let sink = Arc::clone(sink);
    let handler: crate::dispatcher::Handler = Arc::new(move |request: Request, _call| {
        let launcher = Arc::clone(&launcher);
        let sink = Arc::clone(&sink);
        let params = params.clone();
        Box::pin(async move {
            let (project_path, file_path, content, line, character) =
                match position_params(&request) {
                    Ok(parts) => parts,
                    Err((code, error)) => return failure(request.id.clone(), code, &error),
                };
            let document_request = LspDocumentRequest {
                project_path,
                file_path,
                content,
                line,
                character,
            };
            match run_local_feature(&launcher, &sink, &document_request, method, params).await {
                Ok(value) => match project(&value) {
                    Ok(value) => ok(&request, value),
                    Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                },
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
            }
        })
    });
    let _ = context;
    handler
}

fn register_document_symbols(
    set: &mut ServiceSet,
    context: &Arc<CoreContext>,
    launcher: &Arc<InjectedLauncher>,
    sink: &engine::DiagnosticsSink,
) {
    let launcher = Arc::clone(launcher);
    let sink = Arc::clone(sink);
    let handler: crate::dispatcher::Handler = Arc::new(move |request: Request, _call| {
        let launcher = Arc::clone(&launcher);
        let sink = Arc::clone(&sink);
        Box::pin(async move {
            let (project_path, file_path, content, line, character) =
                match position_params(&request) {
                    Ok(parts) => parts,
                    Err((code, error)) => return failure(request.id.clone(), code, &error),
                };
            let document_request = LspDocumentRequest {
                project_path,
                file_path,
                content,
                line,
                character,
            };
            match run_local_feature(
                &launcher,
                &sink,
                &document_request,
                "textDocument/documentSymbol",
                RequestParams::Position { extra_params: None },
            )
            .await
            {
                Ok(value) => {
                    let uri = file_uri(Path::new(&document_request.file_path));
                    match serde_json::to_value(parse_document_symbols(Some(&value), &uri)) {
                        Ok(value) => ok(&request, value),
                        Err(error) => {
                            failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
                        }
                    }
                }
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
            }
        })
    });
    set.register("ide.lsp_document_symbols", handler);
    let _ = context;
}

fn register_rename(
    set: &mut ServiceSet,
    context: &Arc<CoreContext>,
    launcher: &Arc<InjectedLauncher>,
    sink: &engine::DiagnosticsSink,
) {
    let launcher = Arc::clone(launcher);
    let sink = Arc::clone(sink);
    let handler: crate::dispatcher::Handler = Arc::new(move |request: Request, _call| {
        let launcher = Arc::clone(&launcher);
        let sink = Arc::clone(&sink);
        Box::pin(async move {
            let (project_path, file_path, content, line, character) =
                match position_params(&request) {
                    Ok(parts) => parts,
                    Err((code, error)) => return failure(request.id.clone(), code, &error),
                };
            let new_name: String = match param(&request, "newName") {
                Ok(value) => value,
                Err(error) => {
                    return failure(request.id.clone(), ErrorCode::InvalidArgument, &error)
                }
            };
            let document_request = LspDocumentRequest {
                project_path,
                file_path,
                content,
                line,
                character,
            };
            match run_local_feature(
                &launcher,
                &sink,
                &document_request,
                "textDocument/rename",
                RequestParams::Position {
                    extra_params: Some(serde_json::json!({ "newName": new_name })),
                },
            )
            .await
            {
                Ok(value) => match parse_workspace_edit(Some(&value)) {
                    Some(edit) => match serde_json::to_value(edit) {
                        Ok(value) => ok(&request, value),
                        Err(error) => {
                            failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
                        }
                    },
                    None => failure(
                        request.id.clone(),
                        ErrorCode::IoError,
                        "rename returned no edits",
                    ),
                },
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
            }
        })
    });
    set.register("ide.lsp_rename", handler);
    let _ = context;
}

fn register_code_actions(
    set: &mut ServiceSet,
    context: &Arc<CoreContext>,
    launcher: &Arc<InjectedLauncher>,
    sink: &engine::DiagnosticsSink,
) {
    let launcher = Arc::clone(launcher);
    let sink = Arc::clone(sink);
    let handler: crate::dispatcher::Handler = Arc::new(move |request: Request, _call| {
        let launcher = Arc::clone(&launcher);
        let sink = Arc::clone(&sink);
        Box::pin(async move {
            let (project_path, file_path, content, line, character) =
                match position_params(&request) {
                    Ok(parts) => parts,
                    Err((code, error)) => return failure(request.id.clone(), code, &error),
                };
            let diagnostics: Vec<serde_json::Value> = match param(&request, "diagnostics") {
                Ok(value) => value,
                Err(error) => {
                    return failure(request.id.clone(), ErrorCode::InvalidArgument, &error)
                }
            };
            let range = serde_json::json!({
                "start": { "line": line, "character": character },
                "end": { "line": line, "character": character }
            });
            let document_request = LspDocumentRequest {
                project_path,
                file_path,
                content,
                line,
                character,
            };
            match run_local_feature(
                &launcher,
                &sink,
                &document_request,
                "textDocument/codeAction",
                RequestParams::Position {
                    extra_params: Some(serde_json::json!({
                        "range": range,
                        "context": { "diagnostics": diagnostics }
                    })),
                },
            )
            .await
            {
                Ok(value) => match serde_json::to_value(parse_code_actions(Some(&value))) {
                    Ok(value) => ok(&request, value),
                    Err(error) => {
                        failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
                    }
                },
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
            }
        })
    });
    set.register("ide.lsp_code_actions", handler);
    let _ = context;
}

fn register_execute_command(
    set: &mut ServiceSet,
    context: &Arc<CoreContext>,
    launcher: &Arc<InjectedLauncher>,
    sink: &engine::DiagnosticsSink,
) {
    let launcher = Arc::clone(launcher);
    let sink = Arc::clone(sink);
    let handler: crate::dispatcher::Handler = Arc::new(move |request: Request, _call| {
        let launcher = Arc::clone(&launcher);
        let sink = Arc::clone(&sink);
        Box::pin(async move {
            let (project_path, file_path, content, line, character) =
                match position_params(&request) {
                    Ok(parts) => parts,
                    Err((code, error)) => return failure(request.id.clone(), code, &error),
                };
            #[derive(serde::Deserialize)]
            struct CommandParam {
                command: String,
                #[serde(default)]
                arguments: Vec<serde_json::Value>,
            }
            let command: CommandParam = match param(&request, "command") {
                Ok(value) => value,
                Err(error) => {
                    return failure(request.id.clone(), ErrorCode::InvalidArgument, &error)
                }
            };
            let document_request = LspDocumentRequest {
                project_path,
                file_path,
                content,
                line,
                character,
            };
            match run_local_feature(
                &launcher,
                &sink,
                &document_request,
                "workspace/executeCommand",
                RequestParams::Raw(serde_json::json!({
                    "command": command.command,
                    "arguments": command.arguments
                })),
            )
            .await
            {
                // 旧命令返回 `()`(wire 是 null)。
                Ok(_) => ok(&request, serde_json::Value::Null),
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
            }
        })
    });
    set.register("ide.lsp_execute_command", handler);
    let _ = context;
}

fn register_workspace_symbols(
    set: &mut ServiceSet,
    context: &Arc<CoreContext>,
    launcher: &Arc<InjectedLauncher>,
    sink: &engine::DiagnosticsSink,
) {
    let launcher = Arc::clone(launcher);
    let sink = Arc::clone(sink);
    let handler: crate::dispatcher::Handler = Arc::new(move |request: Request, _call| {
        let launcher = Arc::clone(&launcher);
        let sink = Arc::clone(&sink);
        Box::pin(async move {
            let project_path: String = match param(&request, "projectPath") {
                Ok(value) => value,
                Err(error) => {
                    return failure(request.id.clone(), ErrorCode::InvalidArgument, &error)
                }
            };
            let query: String = match param(&request, "query") {
                Ok(value) => value,
                Err(error) => {
                    return failure(request.id.clone(), ErrorCode::InvalidArgument, &error)
                }
            };
            if let Some(error) = local_target_error(
                &request,
                &project_path,
                "LSP target is not hosted: SSH/WSL language servers are unavailable",
            ) {
                return failure(
                    request.id.clone(),
                    ErrorCode::InvalidArgument,
                    &error_to_string(error),
                );
            }
            let project_root = match Path::new(&project_path).canonicalize() {
                Ok(root) => root,
                Err(error) => {
                    return failure(
                        request.id.clone(),
                        ErrorCode::IoError,
                        &format!("failed to resolve project root: {error}"),
                    )
                }
            };
            // 工作区符号固定走 typescript 会话(旧语义:先看该会话已开文档)。
            let session_key = format!("local:{}:typescript", project_root.to_string_lossy());
            let documents = match lsp_face::lsp_session_open_document_snapshots(&session_key) {
                Ok(documents) => documents,
                Err(error) => return failure(request.id.clone(), ErrorCode::IoError, &error),
            };
            let server = match lsp_face::default_server_command("typescript") {
                Some(server) => server,
                None => {
                    return failure(
                        request.id.clone(),
                        ErrorCode::IoError,
                        "language server is not configured for this file type",
                    )
                }
            };
            let workspace_name = project_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("workspace")
                .to_string();
            let request_shape = LspDocumentRequest {
                project_path: project_root.to_string_lossy().to_string(),
                file_path: project_root.to_string_lossy().to_string(),
                content: String::new(),
                line: 0,
                character: 0,
            };
            match engine::run_lsp_session_request(
                true,
                launcher.as_ref(),
                Some(&sink),
                session_key,
                &project_root,
                &server,
                file_uri(&project_root),
                workspace_name,
                file_uri(&project_root),
                documents,
                &request_shape,
                "workspace/symbol",
                RequestParams::Raw(serde_json::json!({ "query": query })),
                "typescript-language-server is not installed. Run: pnpm add -D typescript-language-server typescript",
            )
            .await
            {
                Ok(value) => match serde_json::to_value(parse_workspace_symbols(Some(&value))) {
                    Ok(value) => ok(&request, value),
                    Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
                },
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
            }
        })
    });
    set.register("ide.lsp_workspace_symbols", handler);
    let _ = context;
}

fn register_apply_workspace_edit(set: &mut ServiceSet, context: &Arc<CoreContext>) {
    let context = Arc::clone(context);
    let handler: crate::dispatcher::Handler = Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        Box::pin(async move {
            let _ = &context;
            let project_path: String = match param(&request, "projectPath") {
                Ok(value) => value,
                Err(error) => {
                    return failure(request.id.clone(), ErrorCode::InvalidArgument, &error)
                }
            };
            let edit: LspWorkspaceEdit = match param(&request, "edit") {
                Ok(value) => value,
                Err(error) => {
                    return failure(request.id.clone(), ErrorCode::InvalidArgument, &error)
                }
            };
            if let Some(error) = local_target_error(
                &request,
                &project_path,
                "LSP target is not hosted: SSH/WSL language servers are unavailable",
            ) {
                return failure(
                    request.id.clone(),
                    ErrorCode::InvalidArgument,
                    &error_to_string(error),
                );
            }
            match tokio::task::spawn_blocking(move || {
                aeroric_core::ide::lsp_session::apply_workspace_edit_for_root(
                    Path::new(&project_path),
                    &edit,
                )
            })
            .await
            {
                Ok(Ok(summary)) => ok(&request, serde_json::json!(summary)),
                Ok(Err(error)) => failure(request.id.clone(), ErrorCode::IoError, &error),
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
            }
        })
    });
    set.register("ide.lsp_apply_workspace_edit", handler);
}

fn param<T: DeserializeOwned>(request: &Request, key: &str) -> Result<T, String> {
    let params = request
        .params
        .as_object()
        .ok_or_else(|| "params must be an object".to_string())?;
    let value = params
        .get(key)
        .ok_or_else(|| format!("missing parameter: {key}"))?;
    serde_json::from_value(value.clone()).map_err(|error| format!("{key}: {error}"))
}

fn error_to_string(error: Frame) -> String {
    match error {
        Frame::Failure(reply) => reply.error.message,
        other => format!("unexpected frame: {other:?}"),
    }
}
