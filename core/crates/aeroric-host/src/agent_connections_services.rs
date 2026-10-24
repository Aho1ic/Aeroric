//! `agentConnections.*`: connections, model catalog, pools and agent configs (B1 step 3).
//!
//! Every mutation is one read-modify-write of `agent-connections.json` under
//! [`DOC_LOCK`]; the document is validated before it is written. Credentials go to
//! the keychain through `context.secrets` and are never returned — reads only
//! report whether one is stored.

use std::sync::Arc;

use aeroric_core::agent_connections::model::{AgentConfig, Connection, ConnectionsDocument, Pool};
use aeroric_core::agent_connections::{fetch, routing, service, store};
use aeroric_core::context::CoreContext;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::ok;

static DOC_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

fn doc_path(context: &CoreContext) -> std::path::PathBuf {
    store::document_path(context.paths.data_dir())
}

fn param<T: DeserializeOwned>(request: &Request, name: &str) -> Result<T, String> {
    let value = request
        .params
        .get(name)
        .cloned()
        .ok_or_else(|| format!("missing `{name}`"))?;
    serde_json::from_value(value).map_err(|e| format!("invalid `{name}`: {e}"))
}

fn optional_str(request: &Request, name: &str) -> Option<String> {
    request
        .params
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Document plus, per connection, whether a credential is stored (never the value).
fn snapshot(context: &CoreContext, doc: &ConnectionsDocument) -> Value {
    let credentials: serde_json::Map<String, Value> = doc
        .connections
        .iter()
        .map(|c| {
            let state = match store::get_credential(context.secrets.as_ref(), &c.id) {
                Ok(Some(_)) => "stored",
                Ok(None) => "missing",
                Err(_) => "unreadable",
            };
            (c.id.clone(), Value::String(state.to_string()))
        })
        .collect();
    json!({ "document": doc, "credentials": credentials })
}

/// Load → mutate → validate+save, all under the lock. Returns the new snapshot.
fn mutate(
    request: &Request,
    context: &CoreContext,
    update: impl FnOnce(&mut ConnectionsDocument) -> Result<(), String>,
) -> Frame {
    let _guard = DOC_LOCK.lock();
    let path = doc_path(context);
    let mut doc = match store::load(&path) {
        Ok(doc) => doc,
        Err(e) => return failure(request.id.clone(), ErrorCode::IoError, &e),
    };
    if let Err(e) = update(&mut doc) {
        return failure(request.id.clone(), ErrorCode::InvalidArgument, &e);
    }
    if let Err(e) = store::save(&path, &doc) {
        return failure(request.id.clone(), ErrorCode::InvalidArgument, &e);
    }
    ok(request, snapshot(context, &doc))
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

fn load(request: Request, context: Arc<CoreContext>) -> Frame {
    let _guard = DOC_LOCK.lock();
    match store::load(&doc_path(&context)) {
        Ok(doc) => ok(&request, snapshot(&context, &doc)),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e),
    }
}

fn put_connection(request: Request, context: Arc<CoreContext>) -> Frame {
    let connection: Connection = match param(&request, "connection") {
        Ok(c) => c,
        Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
    };
    // Credential first: a saved connection that silently lost its key is worse than
    // a failed save the user can retry.
    if let Some(secret) = optional_str(&request, "credential") {
        if let Err(e) = store::set_credential(context.secrets.as_ref(), &connection.id, &secret) {
            return failure(
                request.id.clone(),
                ErrorCode::IoError,
                &format!("keychain: {e}"),
            );
        }
    }
    mutate(&request, &context, |doc| {
        service::put_connection(doc, connection);
        Ok(())
    })
}

fn set_credential(request: Request, context: Arc<CoreContext>) -> Frame {
    let (id, secret) = match (
        param::<String>(&request, "connectionId"),
        param::<String>(&request, "credential"),
    ) {
        (Ok(id), Ok(secret)) => (id, secret),
        (Err(e), _) | (_, Err(e)) => {
            return failure(request.id.clone(), ErrorCode::InvalidArgument, &e)
        }
    };
    let _guard = DOC_LOCK.lock();
    let doc = match store::load(&doc_path(&context)) {
        Ok(doc) => doc,
        Err(e) => return failure(request.id.clone(), ErrorCode::IoError, &e),
    };
    if !doc.connections.iter().any(|c| c.id == id) {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "connection not found",
        );
    }
    // Empty string deletes (Q27: revoking a key takes effect immediately).
    if let Err(e) = store::set_credential(context.secrets.as_ref(), &id, &secret) {
        return failure(
            request.id.clone(),
            ErrorCode::IoError,
            &format!("keychain: {e}"),
        );
    }
    ok(&request, snapshot(&context, &doc))
}

fn delete_connection(request: Request, context: Arc<CoreContext>) -> Frame {
    let id: String = match param(&request, "id") {
        Ok(id) => id,
        Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
    };
    let frame = mutate(&request, &context, |doc| {
        service::delete_connection(doc, &id)
    });
    // Only drop the key once the document no longer references the connection.
    if frame_is_ok(&frame) {
        let _ = store::delete_credential(context.secrets.as_ref(), &id);
    }
    frame
}

fn frame_is_ok(frame: &Frame) -> bool {
    matches!(frame, Frame::Success(_))
}

fn put_pool(request: Request, context: Arc<CoreContext>) -> Frame {
    let pool: Pool = match param(&request, "pool") {
        Ok(p) => p,
        Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
    };
    mutate(&request, &context, |doc| {
        service::put_pool(doc, pool);
        Ok(())
    })
}

fn delete_pool(request: Request, context: Arc<CoreContext>) -> Frame {
    let id: String = match param(&request, "id") {
        Ok(id) => id,
        Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
    };
    mutate(&request, &context, |doc| service::delete_pool(doc, &id))
}

fn put_config(request: Request, context: Arc<CoreContext>) -> Frame {
    let config: AgentConfig = match param(&request, "config") {
        Ok(c) => c,
        Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
    };
    mutate(&request, &context, |doc| service::put_config(doc, config))
}

fn delete_config(request: Request, context: Arc<CoreContext>) -> Frame {
    let id: String = match param(&request, "id") {
        Ok(id) => id,
        Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
    };
    mutate(&request, &context, |doc| service::delete_config(doc, &id))
}

/// For the new-session picker: models a config offers, and for one model which
/// channels can serve it and why the others cannot.
fn candidates(request: Request, context: Arc<CoreContext>) -> Frame {
    let config_id: String = match param(&request, "configId") {
        Ok(id) => id,
        Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
    };
    let _guard = DOC_LOCK.lock();
    let doc = match store::load(&doc_path(&context)) {
        Ok(doc) => doc,
        Err(e) => return failure(request.id.clone(), ErrorCode::IoError, &e),
    };
    let Some(config) = doc.configs.iter().find(|c| c.id == config_id) else {
        return failure(request.id.clone(), ErrorCode::NotFound, "config not found");
    };
    let models = routing::offered_models(&doc, config);
    let model = optional_str(&request, "model").unwrap_or_else(|| config.default_model.clone());
    let (available, excluded) = if model.trim().is_empty() {
        (Vec::new(), Vec::new())
    } else {
        routing::candidates(&doc, config, &model)
    };
    let excluded: Vec<Value> = excluded
        .into_iter()
        .map(|(id, reason)| json!({ "id": id, "reason": reason }))
        .collect();
    ok(
        &request,
        json!({ "models": models, "model": model, "candidates": available, "excluded": excluded }),
    )
}

/// Q134: reads the catalog only. Fetch runs outside the lock (network); the merge
/// re-reads the document under it so a concurrent edit is not overwritten.
fn fetch_models_handler(context: &Arc<CoreContext>) -> Handler {
    let context = Arc::clone(context);
    Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        Box::pin(async move {
            let id: String = match param(&request, "connectionId") {
                Ok(id) => id,
                Err(e) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
            };
            let ctx = Arc::clone(&context);
            let lookup = id.clone();
            let prepared = tokio::task::spawn_blocking(move || {
                let _guard = DOC_LOCK.lock();
                let doc = store::load(&doc_path(&ctx))?;
                let connection = doc
                    .connections
                    .iter()
                    .find(|c| c.id == lookup)
                    .cloned()
                    .ok_or_else(|| "connection not found".to_string())?;
                let secret = store::get_credential(ctx.secrets.as_ref(), &lookup)?
                    .ok_or_else(|| "no credential stored for this connection".to_string())?;
                Ok::<_, String>((connection, secret))
            })
            .await;
            let (connection, secret) = match prepared {
                Ok(Ok(pair)) => pair,
                Ok(Err(e)) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &e),
                Err(e) => return failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
            };
            let fetched = match fetch::fetch_models(&connection, &secret).await {
                Ok(models) => models,
                // A failed fetch never touches the saved catalog (Q134).
                Err(e) => return failure(request.id.clone(), ErrorCode::IoError, &e),
            };
            let ctx = Arc::clone(&context);
            let merged = tokio::task::spawn_blocking(move || {
                let _guard = DOC_LOCK.lock();
                let path = doc_path(&ctx);
                let mut doc = store::load(&path)?;
                let connection = doc
                    .connections
                    .iter_mut()
                    .find(|c| c.id == id)
                    .ok_or_else(|| "connection was deleted during fetch".to_string())?;
                let (added, kept) = service::merge_fetched_models(connection, &fetched);
                store::save(&path, &doc)?;
                Ok::<_, String>((snapshot(&ctx, &doc), added, kept, fetched.len()))
            })
            .await;
            match merged {
                Ok(Ok((snap, added, kept, total))) => ok(
                    &request,
                    json!({ "snapshot": snap, "fetched": total, "added": added, "kept": kept }),
                ),
                Ok(Err(e)) => failure(request.id.clone(), ErrorCode::IoError, &e),
                Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
            }
        })
    })
}

pub fn register_agent_connection_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    set.register("agentConnections.load", blocking(&context, load));
    set.register(
        "agentConnections.putConnection",
        blocking(&context, put_connection),
    );
    set.register(
        "agentConnections.setCredential",
        blocking(&context, set_credential),
    );
    set.register(
        "agentConnections.deleteConnection",
        blocking(&context, delete_connection),
    );
    set.register("agentConnections.putPool", blocking(&context, put_pool));
    set.register(
        "agentConnections.deletePool",
        blocking(&context, delete_pool),
    );
    set.register("agentConnections.putConfig", blocking(&context, put_config));
    set.register(
        "agentConnections.deleteConfig",
        blocking(&context, delete_config),
    );
    set.register(
        "agentConnections.candidates",
        blocking(&context, candidates),
    );
    set.register(
        "agentConnections.fetchModels",
        fetch_models_handler(&context),
    );
}
