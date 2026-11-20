//! H6 step 3 (2026-11-21): the 能力包 page's methods (`packs.*`). What each
//! launch gets is decided in core (`extensions::packs`); these only read and
//! change the packs on disk.

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::extensions::{builtin_skills, pack_store};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::packs::PackServer;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::ok;

pub const LIST_METHOD: &str = "packs.list";
pub const SAVE_SERVER_METHOD: &str = "packs.saveServer";
pub const IMPORT_METHOD: &str = "packs.import";
pub const SET_ENABLED_METHOD: &str = "packs.setEnabled";
pub const SET_CONFIRMED_METHOD: &str = "packs.setConfirmed";
pub const REMOVE_METHOD: &str = "packs.remove";

pub fn register_pack_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    set.register(LIST_METHOD, blocking(&context, list));
    set.register(SAVE_SERVER_METHOD, blocking(&context, save_server));
    set.register(IMPORT_METHOD, blocking(&context, import));
    set.register(SET_ENABLED_METHOD, blocking(&context, set_enabled));
    set.register(SET_CONFIRMED_METHOD, blocking(&context, set_confirmed));
    set.register(REMOVE_METHOD, blocking(&context, remove));
}

fn blocking(
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
                Err(e) => failure(id, ErrorCode::IoError, &e.to_string()),
            }
        })
    })
}

fn invalid(request: &Request, message: &str) -> Frame {
    failure(request.id.clone(), ErrorCode::InvalidArgument, message)
}

fn done(request: &Request, result: Result<Value, String>) -> Frame {
    match result {
        Ok(value) => ok(request, value),
        Err(message) => failure(request.id.clone(), ErrorCode::InvalidArgument, &message),
    }
}

/// `packs.list` → `{packs: [{pack, trusted, places: [{place, items}]}], broken}`.
fn list(request: Request, context: Arc<CoreContext>) -> Frame {
    let view = pack_store::overview(context.paths.data_dir());
    ok(&request, serde_json::to_value(view).unwrap_or(Value::Null))
}

/// `packs.saveServer {server}`: add or change a server of the user's own.
fn save_server(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(server) = request
        .params
        .get("server")
        .cloned()
        .and_then(|v| serde_json::from_value::<PackServer>(v).ok())
    else {
        return invalid(&request, "packs.saveServer requires a server");
    };
    let result = pack_store::save_server(context.paths.data_dir(), server)
        .map(|pack| json!({ "pack": pack }));
    done(&request, result)
}

/// `packs.import {path}`: copy a pack folder in, unconfirmed.
fn import(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(path) = request.params.get("path").and_then(Value::as_str) else {
        return invalid(&request, "packs.import requires a path");
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_dir() {
        return invalid(&request, "packs.import requires an existing folder");
    }
    let result =
        pack_store::import(context.paths.data_dir(), &path).map(|pack| json!({ "pack": pack }));
    done(&request, result)
}

fn id_and_flag(request: &Request, flag: &str) -> Option<(String, bool)> {
    let id = request.params.get("id").and_then(Value::as_str)?;
    let on = request.params.get(flag).and_then(Value::as_bool)?;
    Some((id.to_string(), on))
}

/// `packs.setEnabled {id, enabled}`.
fn set_enabled(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some((id, enabled)) = id_and_flag(&request, "enabled") else {
        return invalid(&request, "packs.setEnabled requires id and enabled");
    };
    let result = pack_store::set_enabled(context.paths.data_dir(), &id, enabled)
        .map(|()| json!({ "enabled": enabled }));
    done(&request, result)
}

/// `packs.setConfirmed {id, confirmed}`: the user confirms (or withdraws)
/// a pack's source; its servers and handlers run only when confirmed.
fn set_confirmed(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some((id, confirmed)) = id_and_flag(&request, "confirmed") else {
        return invalid(&request, "packs.setConfirmed requires id and confirmed");
    };
    let result = pack_store::set_confirmed(context.paths.data_dir(), &id, confirmed)
        .map(|()| json!({ "confirmed": confirmed }));
    done(&request, result)
}

/// `packs.remove {id}`: the pack's folder goes to the Trash.
fn remove(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(id) = request.params.get("id").and_then(Value::as_str) else {
        return invalid(&request, "packs.remove requires an id");
    };
    let result = pack_store::remove(context.paths.data_dir(), id, &builtin_skills::move_to_trash)
        .map(|()| json!({ "removed": id }));
    done(&request, result)
}
