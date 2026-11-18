//! B6 (2026-11-18): what a task delivered — its artifacts, each one's
//! versions, a version's bytes, and the user's "does not count".

use std::sync::Arc;

use aeroric_core::sessions::live::artifacts::{self, NO_ARTIFACT};
use aeroric_protocol::artifacts::{self as wire, ArtifactParams};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::json;

use super::{blocking, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .artifacts()
        .set_dir(live.context.paths.data_dir().join("artifacts"));
    set.register(wire::LIST_METHOD, blocking(live, list));
    set.register(wire::VERSIONS_METHOD, blocking(live, versions));
    set.register(wire::READ_METHOD, blocking(live, read));
    set.register(wire::EXCLUDE_METHOD, blocking(live, exclude));
}

fn list(live: &Live, request: &Request) -> Frame {
    let p: ArtifactParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let mut page = live
        .executor
        .artifacts()
        .list(&p.task_id, p.cursor.as_deref());
    // Nothing kept: say so when the task works outside Git.
    if page.artifacts.is_empty() && p.cursor.is_none() {
        page.unavailable = live
            .executor
            .task_cwd(&p.task_id)
            .and_then(|cwd| artifacts::unavailable(&cwd));
    }
    ok(request, serde_json::to_value(&page).unwrap_or_default())
}

fn versions(live: &Live, request: &Request) -> Frame {
    let p: ArtifactParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(id) = p.artifact_id.as_deref() else {
        return failure(request, ErrorCode::InvalidArgument, "缺少 artifactId");
    };
    match live.executor.artifacts().versions(&p.task_id, id) {
        Some(versions) => ok(request, json!({ "versions": versions })),
        None => failure(request, ErrorCode::NotFound, NO_ARTIFACT),
    }
}

fn read(live: &Live, request: &Request) -> Frame {
    let p: ArtifactParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(id) = p.artifact_id.as_deref() else {
        return failure(request, ErrorCode::InvalidArgument, "缺少 artifactId");
    };
    match live
        .executor
        .artifacts()
        .read(&p.task_id, id, p.version, p.offset.unwrap_or(0), p.limit)
    {
        Ok(content) => ok(request, json!({ "content": content })),
        // No such artifact or version, or its bytes are gone (`GONE`).
        Err(reason) => failure(request, ErrorCode::NotFound, &reason),
    }
}

fn exclude(live: &Live, request: &Request) -> Frame {
    let p: ArtifactParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (Some(id), Some(excluded)) = (p.artifact_id.as_deref(), p.excluded) else {
        return failure(
            request,
            ErrorCode::InvalidArgument,
            "缺少 artifactId 或 excluded",
        );
    };
    match live.executor.artifacts().exclude(&p.task_id, id, excluded) {
        Ok(()) => ok(request, json!({ "artifactId": id, "excluded": excluded })),
        Err(reason) => failure(request, ErrorCode::NotFound, &reason),
    }
}

fn failure(request: &Request, code: ErrorCode, message: &str) -> Frame {
    crate::dispatcher::failure(request.id.clone(), code, message)
}
