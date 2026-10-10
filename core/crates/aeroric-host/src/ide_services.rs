//! Definition-only IDE services. No run/debug process or remote executor here.

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::ide::dap::{self, DebugConfigDocument, DebugConfigError};
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
