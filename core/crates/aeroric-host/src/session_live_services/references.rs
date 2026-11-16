//! Q-R6 (2026-11-13) reference library, the host's side: the `references.*`
//! methods the settings page calls, and the `context.reference*` methods
//! behind `search_references` and `read_reference`. Indexing runs on a thread
//! of its own; a search that finds a folder changed starts a rebuild and
//! answers from the index it has. The library is core's
//! (`aeroric_core::knowledge::references`).

use std::sync::Arc;

use aeroric_core::knowledge::references::{ReferenceError, References};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::json;

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .references()
        .set_dir(live.context.paths.data_dir().join("references"));
    set.register("references.list", blocking(live, list));
    set.register("references.add", blocking(live, add));
    set.register("references.remove", blocking(live, remove));
    set.register("references.reindex", blocking(live, reindex));
    set.register(
        "context.referencesAvailable",
        blocking(live, context_available),
    );
    set.register("context.referenceSearch", blocking(live, context_search));
    set.register("context.referenceRead", blocking(live, context_read));
}

fn reference_failure(request: &Request, error: ReferenceError) -> Frame {
    let code = match error {
        ReferenceError::Unknown(_) => ErrorCode::NotFound,
        ReferenceError::Storage(_) => ErrorCode::IoError,
        _ => ErrorCode::InvalidArgument,
    };
    failure(request.id.clone(), code, &error.to_string())
}

/// Index `id` on a thread of its own, unless that is already happening.
fn index_later(library: &Arc<References>, id: &str) {
    if !library.begin_index(id) {
        return;
    }
    let library = Arc::clone(library);
    let id = id.to_string();
    std::thread::spawn(move || {
        if let Err(error) = library.index(&id) {
            eprintln!("[aeroric-host] reference library: indexing {id} failed: {error}");
        }
    });
}

fn list(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "references": live.executor.references().statuses() }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddParams {
    path: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
}

fn add(live: &Live, request: &Request) -> Frame {
    let p: AddParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let library = live.executor.references();
    match library.add(&p.path, p.name.as_deref(), p.purpose.as_deref()) {
        Ok(reference) => {
            index_later(library, &reference.id);
            ok(request, json!({ "reference": library.status(reference) }))
        }
        Err(error) => reference_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdParams {
    id: String,
}

fn remove(live: &Live, request: &Request) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.references().remove(&p.id) {
        Ok(()) => ok(request, json!({ "removed": p.id })),
        Err(error) => reference_failure(request, error),
    }
}

fn reindex(live: &Live, request: &Request) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let library = live.executor.references();
    match library.get(&p.id) {
        Ok(reference) => {
            index_later(library, &reference.id);
            ok(request, json!({ "reference": library.status(reference) }))
        }
        Err(error) => reference_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextParams {
    token: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    start: Option<u32>,
    #[serde(default)]
    end: Option<u32>,
}

/// Any live session may read the library (it is the user's, not a project's).
fn context_params(live: &Live, request: &Request) -> Result<ContextParams, Box<Frame>> {
    let p: ContextParams = params(request)?;
    if live.executor.context_tokens().resolve(&p.token).is_none() {
        return Err(Box::new(failure(
            request.id.clone(),
            ErrorCode::Unauthenticated,
            "上下文令牌无效或会话已结束",
        )));
    }
    Ok(p)
}

fn context_available(live: &Live, request: &Request) -> Frame {
    let available =
        context_params(live, request).is_ok() && !live.executor.references().list().is_empty();
    ok(request, json!({ "available": available }))
}

fn context_search(live: &Live, request: &Request) -> Frame {
    let p = match context_params(live, request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let library = live.executor.references();
    let query = p.query.unwrap_or_default();
    let mut notes = Vec::new();
    for status in library.statuses() {
        if status.stale {
            index_later(library, &status.reference.id);
        }
        if status.stale || status.indexing {
            notes.push(format!(
                "{} 正在更新索引，结果可能不全",
                status.reference.name
            ));
        }
    }
    match library.search(&query, p.repo.as_deref()) {
        Ok(hits) => {
            let mut lines: Vec<String> = hits
                .iter()
                .map(|h| {
                    let kind = h
                        .kind
                        .strip_prefix("symbol:")
                        .map_or(String::new(), |k| format!("（定义 {k}）"));
                    format!("- {} · {}:{}{kind}  {}", h.repo, h.path, h.line, h.excerpt)
                })
                .collect();
            if lines.is_empty() {
                lines.push(
                    "参考代码库里没有找到。换个关键词试试（类名、函数名或中文描述）。".into(),
                );
            } else {
                lines.push("用 read_reference 按 repo 和 path 读全文，可带 start 行号。".into());
            }
            lines.extend(notes);
            ok(request, json!({ "text": lines.join("\n"), "hits": hits }))
        }
        Err(error) => reference_failure(request, error),
    }
}

fn context_read(live: &Live, request: &Request) -> Frame {
    let p = match context_params(live, request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (Some(repo), Some(path)) = (p.repo.as_deref(), p.path.as_deref()) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "read_reference 需要 repo 和 path",
        );
    };
    match live.executor.references().read(repo, path, p.start, p.end) {
        Ok(text) => ok(request, json!({ "text": text })),
        Err(error) => reference_failure(request, error),
    }
}
