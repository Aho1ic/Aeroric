//! Definition-only IDE services. No run/debug process or remote executor here.

use std::path::Path;
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::ide::dap::{self, DebugConfigDocument, DebugConfigError};
use aeroric_core::ide::lsp_session::engine;
use aeroric_core::ide::run_config::{self, RunConfigDocument, RunConfigError};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde::de::DeserializeOwned;

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::local_targets::local_target_error;
use crate::services::ok;

const UNSUPPORTED_TARGET: &str =
    "Run-config target is not hosted: SSH/WSL definition IO is unavailable";
const UNSUPPORTED_DEBUG_TARGET: &str =
    "Debug-config target is not hosted: SSH/WSL definition IO is unavailable";

pub fn register_ide_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    set.register(
        "ide.read_run_configs",
        definition_handler(&context, read_run_configs),
    );
    set.register(
        "ide.write_run_configs",
        definition_handler(&context, write_run_configs),
    );
    set.register(
        "ide.read_debug_configs",
        definition_handler(&context, read_debug_configs),
    );
    set.register(
        "ide.write_debug_configs",
        definition_handler(&context, write_debug_configs),
    );
    // ── 切片一百零六：tests 命令族本地面（core `ide::tests_face` 与 Tauri
    // 转发壳同一实现；remote 孪生留 Tauri SSH 适配层）──
    set.register(
        "ide.discover_tests",
        definition_handler(&context, discover_tests),
    );
    set.register("ide.run_tests", definition_handler(&context, run_tests));
    // ── 切片一百零七：diagnostics 命令族本地面（与 tests_face 同款注入）──
    set.register(
        "ide.run_diagnostics",
        definition_handler(&context, run_diagnostics),
    );
    // ── 切片一百零九：LSP 生命周期面（注册表簿记；hover/definition 等会话
    // 面需活语言服务器，留 Tauri 会话层）──
    set.register(
        "ide.lsp_server_status",
        definition_handler(&context, lsp_server_status),
    );
    set.register(
        "ide.lsp_open_document",
        definition_handler(&context, lsp_open_document),
    );
    set.register(
        "ide.lsp_change_document",
        definition_handler(&context, lsp_change_document),
    );
    set.register(
        "ide.lsp_close_document",
        definition_handler(&context, lsp_close_document),
    );
    set.register(
        "ide.lsp_shutdown_project",
        definition_handler(&context, lsp_shutdown_project),
    );
    // ── 切片一百零八：运行面（有状态进程注册表——每宿主实例一份，三个
    // handler 共享；remote 孪生留 Tauri SSH 适配层）──
    let run_state = Arc::new(aeroric_core::ide::run_face::RunProcessState::default());
    set.register(
        "ide.start_run_config",
        definition_handler(&context, {
            let run_state = Arc::clone(&run_state);
            move |request, context| run_start_run_config(request, context, &run_state)
        }),
    );
    set.register(
        "ide.stop_run_config",
        definition_handler(&context, {
            let run_state = Arc::clone(&run_state);
            move |request, context| run_stop_run_config(request, context, &run_state)
        }),
    );
    set.register(
        "ide.read_run_process",
        definition_handler(&context, {
            let run_state = Arc::clone(&run_state);
            move |request, context| run_read_run_process(request, context, &run_state)
        }),
    );
}

fn lsp_server_status(request: Request, _context: Arc<CoreContext>) -> Frame {
    let decode = || -> Result<(String, String), (ErrorCode, String)> {
        let path: String =
            param(&request, "projectPath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
        let file: String =
            param(&request, "filePath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
        if let Some(error) = local_target_error(
            &request,
            &path,
            "LSP target is not hosted: SSH/WSL language servers are unavailable",
        ) {
            return Err((ErrorCode::InvalidArgument, error_to_string(error)));
        }
        Ok((path, file))
    };
    let (path, file) = match decode() {
        Ok(params) => params,
        Err((code, error)) => return failure(request.id.clone(), code, &error),
    };
    match aeroric_core::ide::lsp_face::lsp_server_status(path, file, &[]) {
        Ok(status) => ok(&request, serde_json::json!(status)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn lsp_lifecycle_params(
    request: &Request,
) -> Result<(String, String, String, u32), (ErrorCode, String)> {
    let path: String =
        param(request, "projectPath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    let file: String =
        param(request, "filePath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    let content: String =
        param(request, "content").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    let version: u32 =
        param(request, "version").map_err(|error| (ErrorCode::InvalidArgument, error))?;
    Ok((path, file, content, version))
}

fn lsp_open_document(request: Request, _context: Arc<CoreContext>) -> Frame {
    let decode = lsp_lifecycle_params(&request);
    let (path, file, content, version) = match decode {
        Ok(params) => params,
        Err((code, error)) => return failure(request.id.clone(), code, &error),
    };
    match aeroric_core::ide::lsp_face::lsp_open_document(&path, &file, content, version) {
        Ok(summary) => ok(&request, serde_json::json!(summary)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn lsp_change_document(request: Request, _context: Arc<CoreContext>) -> Frame {
    let decode = lsp_lifecycle_params(&request);
    let (path, file, content, version) = match decode {
        Ok(params) => params,
        Err((code, error)) => return failure(request.id.clone(), code, &error),
    };
    match aeroric_core::ide::lsp_face::lsp_change_document(&path, &file, content, version) {
        Ok(summary) => ok(&request, serde_json::json!(summary)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn lsp_close_document(request: Request, _context: Arc<CoreContext>) -> Frame {
    let decode = || -> Result<(String, String), (ErrorCode, String)> {
        let path: String =
            param(&request, "projectPath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
        let file: String =
            param(&request, "filePath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
        Ok((path, file))
    };
    let (path, file) = match decode() {
        Ok(params) => params,
        Err((code, error)) => return failure(request.id.clone(), code, &error),
    };
    match aeroric_core::ide::lsp_face::lsp_close_document(&path, &file) {
        Ok(summary) => {
            // legacy 的这一半**必须落在宿主**:External 判决下 Tauri 的 Embedded
            // 闭包根本不会被 poll,把 shutdown 留在那里等于会话/子进程永不回收
            // (每个项目留一份常驻语言服务器)。注册表簿记在 core(已做完),会话
            // 回收在这里——两者都属本命令的语义,顺序也照 legacy:先 didClose,
            // 再在**只剩零个文档**时整个会话 shutdown。
            let session = aeroric_core::ide::lsp_face::local_lsp_lifecycle_context(&path, &file)
                .ok()
                .map(|(session_key, _, file_key, _)| (session_key, file_key));
            let open_documents = summary.open_documents;
            if let Some((session_key, file_key)) = session {
                tokio::task::spawn(async move {
                    engine::close_local_lsp_session_document(&session_key, &file_key).await;
                    if open_documents == 0 {
                        engine::shutdown_local_lsp_session(&session_key).await;
                    }
                });
            }
            ok(&request, serde_json::json!(summary))
        }
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn lsp_shutdown_project(request: Request, _context: Arc<CoreContext>) -> Frame {
    let path: String = match param(&request, "projectPath") {
        Ok(path) => path,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    match aeroric_core::ide::lsp_face::lsp_shutdown_project(&path) {
        Ok(closed) => {
            let prefix = std::path::Path::new(&path)
                .canonicalize()
                .map(|root| format!("local:{}:", root.to_string_lossy()))
                .ok();
            if let Some(prefix) = prefix {
                tokio::task::spawn(async move {
                    for handle in engine::take_local_lsp_sessions_with_prefix(&prefix) {
                        engine::shutdown_lsp_handle(handle).await;
                    }
                });
            }
            // 标量按标量回:legacy 的 `lsp_shutdown_project` 返回裸 usize,Tauri
            // 壳 `from_value::<usize>` 解一个对象会失败(External 模式直接报错)。
            // 同切片108 的 `stop_run_config` 先例——只有结构体才包成对象。
            ok(&request, serde_json::json!(closed))
        }
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn run_start_run_config(
    request: Request,
    _context: Arc<CoreContext>,
    state: &aeroric_core::ide::run_face::RunProcessState,
) -> Frame {
    let decode =
        || -> Result<(String, aeroric_core::ide::run_config::RunConfig), (ErrorCode, String)> {
            let path: String = param(&request, "projectPath")
                .map_err(|error| (ErrorCode::InvalidArgument, error))?;
            let config: aeroric_core::ide::run_config::RunConfig =
                param(&request, "config").map_err(|error| (ErrorCode::InvalidArgument, error))?;
            if let Some(error) = local_target_error(
                &request,
                &path,
                "Run target is not hosted: SSH/WSL run IO is unavailable",
            ) {
                return Err((ErrorCode::InvalidArgument, error_to_string(error)));
            }
            Ok((path, config))
        };
    let (path, config) = match decode() {
        Ok(params) => params,
        Err((code, error)) => return failure(request.id.clone(), code, &error),
    };
    match aeroric_core::ide::run_face::start_run_config(
        Path::new(&path),
        config,
        state,
        &aeroric_core::ide::run_face::posix_shell_adapter,
        &[],
    ) {
        Ok(snapshot) => ok(&request, serde_json::json!(snapshot)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn run_stop_run_config(
    request: Request,
    _context: Arc<CoreContext>,
    state: &aeroric_core::ide::run_face::RunProcessState,
) -> Frame {
    let run_id: String = match param(&request, "runId") {
        Ok(run_id) => run_id,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    match aeroric_core::ide::run_face::stop_run_config(run_id.clone(), state) {
        Ok(snapshot) => ok(&request, serde_json::json!(snapshot)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn run_read_run_process(
    request: Request,
    _context: Arc<CoreContext>,
    state: &aeroric_core::ide::run_face::RunProcessState,
) -> Frame {
    let run_id: String = match param(&request, "runId") {
        Ok(run_id) => run_id,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    match aeroric_core::ide::run_face::read_run_process(run_id.clone(), state) {
        Ok(snapshot) => ok(&request, serde_json::json!(snapshot)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn run_diagnostics(request: Request, _context: Arc<CoreContext>) -> Frame {
    let decode = || -> Result<(String, String), (ErrorCode, String)> {
        let path: String =
            param(&request, "projectPath").map_err(|error| (ErrorCode::InvalidArgument, error))?;
        let profile: String =
            param(&request, "profile").map_err(|error| (ErrorCode::InvalidArgument, error))?;
        if let Some(error) = local_target_error(
            &request,
            &path,
            "Diagnostics target is not hosted: SSH/WSL diagnostics IO is unavailable",
        ) {
            return Err((ErrorCode::InvalidArgument, error_to_string(error)));
        }
        Ok((path, profile))
    };
    let (path, profile) = match decode() {
        Ok(params) => params,
        Err((code, error)) => return failure(request.id.clone(), code, &error),
    };
    match aeroric_core::ide::diagnostics_face::run_profile(Path::new(&path), &profile, &[]) {
        Ok(result) => ok(&request, serde_json::json!(result)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn discover_tests(request: Request, _context: Arc<CoreContext>) -> Frame {
    let path: String = match param(&request, "projectPath") {
        Ok(path) => path,
        Err(error) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    };
    if let Some(error) = local_target_error(
        &request,
        &path,
        "Test target is not hosted: SSH/WSL test IO is unavailable",
    ) {
        return error;
    }
    match aeroric_core::ide::tests_face::discover_test_profiles_from_root(Path::new(&path)) {
        Ok(profiles) => ok(&request, serde_json::json!({ "profiles": profiles })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn run_tests(request: Request, _context: Arc<CoreContext>) -> Frame {
    let decode = || -> Result<(String, String, Option<aeroric_core::ide::tests_face::TestRunTarget>, bool), (ErrorCode, String)> {
        let path: String = param(&request, "projectPath")
            .map_err(|error| (ErrorCode::InvalidArgument, error))?;
        let profile: String = param(&request, "profile")
            .map_err(|error| (ErrorCode::InvalidArgument, error))?;
        let target: Option<aeroric_core::ide::tests_face::TestRunTarget> =
            param(&request, "target").ok();
        let coverage: Option<bool> = param(&request, "coverage").ok();
        if let Some(error) = local_target_error(
            &request,
            &path,
            "Test target is not hosted: SSH/WSL test IO is unavailable",
        ) {
            return Err((ErrorCode::InvalidArgument, error_to_string(error)));
        }
        Ok((path, profile, target, coverage.unwrap_or(false)))
    };
    let (path, profile, target, coverage) = match decode() {
        Ok(params) => params,
        Err((code, error)) => return failure(request.id.clone(), code, &error),
    };
    match aeroric_core::ide::tests_face::run_profile(
        Path::new(&path),
        &profile,
        target.as_ref(),
        coverage,
        &aeroric_core::ide::tests_face::default_python_program,
        &[],
    ) {
        Ok(result) => ok(&request, serde_json::json!(result)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn error_to_string(error: Frame) -> String {
    match error {
        Frame::Failure(reply) => reply.error.message,
        other => format!("unexpected frame: {other:?}"),
    }
}

fn definition_handler(
    context: &Arc<CoreContext>,
    run: impl Fn(Request, Arc<CoreContext>) -> Frame + Send + Sync + 'static,
) -> Handler {
    let context = Arc::clone(context);
    let run = Arc::new(run);
    Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        let run = Arc::clone(&run);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(request, context)).await {
                Ok(frame) => frame,
                Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
            }
        })
    })
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

fn document_reply(request: &Request, result: Result<RunConfigDocument, RunConfigError>) -> Frame {
    match result {
        Ok(document) => ok(request, serde_json::json!(document)),
        Err(error) => {
            let code = match &error {
                RunConfigError::InvalidArgument(_) => ErrorCode::InvalidArgument,
                RunConfigError::Io(_) | RunConfigError::WritePermission(_) => ErrorCode::IoError,
            };
            failure(request.id.clone(), code, &error.to_string())
        }
    }
}

fn read_run_configs(request: Request, _context: Arc<CoreContext>) -> Frame {
    let path: String = match param(&request, "projectPath") {
        Ok(path) => path,
        Err(error) => return document_reply(&request, Err(RunConfigError::InvalidArgument(error))),
    };
    if let Some(error) = local_target_error(&request, &path, UNSUPPORTED_TARGET) {
        return error;
    }
    document_reply(&request, run_config::read_run_configs(&path))
}

fn write_run_configs(request: Request, context: Arc<CoreContext>) -> Frame {
    let params = (|| {
        let path: String =
            param(&request, "projectPath").map_err(RunConfigError::InvalidArgument)?;
        let document: RunConfigDocument =
            param(&request, "document").map_err(RunConfigError::InvalidArgument)?;
        Ok((path, document))
    })();
    let (path, document) = match params {
        Ok(params) => params,
        Err(error) => return document_reply(&request, Err(error)),
    };
    if let Some(error) = local_target_error(&request, &path, UNSUPPORTED_TARGET) {
        return error;
    }
    document_reply(
        &request,
        run_config::write_run_configs(&context, &path, document),
    )
}

fn debug_document_reply(
    request: &Request,
    result: Result<DebugConfigDocument, DebugConfigError>,
) -> Frame {
    match result {
        Ok(document) => ok(request, serde_json::json!(document)),
        Err(error) => {
            let code = match &error {
                DebugConfigError::InvalidArgument(_) => ErrorCode::InvalidArgument,
                DebugConfigError::Io(_) | DebugConfigError::WritePermission(_) => {
                    ErrorCode::IoError
                }
            };
            failure(request.id.clone(), code, &error.to_string())
        }
    }
}

fn read_debug_configs(request: Request, _context: Arc<CoreContext>) -> Frame {
    let path: String = match param(&request, "projectPath") {
        Ok(path) => path,
        Err(error) => {
            return debug_document_reply(&request, Err(DebugConfigError::InvalidArgument(error)))
        }
    };
    if let Some(error) = local_target_error(&request, &path, UNSUPPORTED_DEBUG_TARGET) {
        return error;
    }
    debug_document_reply(&request, dap::read_debug_configs(&path))
}

fn write_debug_configs(request: Request, context: Arc<CoreContext>) -> Frame {
    let params = (|| {
        let path: String =
            param(&request, "projectPath").map_err(DebugConfigError::InvalidArgument)?;
        let document: DebugConfigDocument =
            param(&request, "document").map_err(DebugConfigError::InvalidArgument)?;
        Ok::<_, DebugConfigError>((path, document))
    })();
    let (path, document) = match params {
        Ok(params) => params,
        Err(error) => return debug_document_reply(&request, Err(error)),
    };
    if let Some(error) = local_target_error(&request, &path, UNSUPPORTED_DEBUG_TARGET) {
        return error;
    }
    debug_document_reply(
        &request,
        dap::write_debug_configs(&context, &path, document),
    )
}
