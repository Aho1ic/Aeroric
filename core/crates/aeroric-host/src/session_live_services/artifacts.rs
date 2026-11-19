//! B6 (2026-11-18): what a task delivered — its artifacts, each one's
//! versions, a version's bytes, and the user's "does not count".
//! B6 step 2 (2026-11-19): the same, for an agent through the context MCP:
//! its own task's, and those of tasks it delegated to, was delegated from,
//! branched or shares a parent with, in its project only.

use std::sync::Arc;

use aeroric_core::sessions::live::artifacts::{self, NO_ARTIFACT};
use aeroric_core::storage::Record;
use aeroric_protocol::artifacts::{self as wire, parse_ref, ArtifactParams};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use super::delegation::DELEGATE_OF_FIELD;
use super::{blocking, ok, params, Live, BRANCH_OF_FIELD};
use crate::dispatcher::ServiceSet;

/// An agent asked for a task it has no tie to.
pub const UNRELATED: &str = "只能读当前任务、以及和它有委派或侧支关系的任务的交付物";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .artifacts()
        .set_dir(live.context.paths.data_dir().join("artifacts"));
    set.register(wire::LIST_METHOD, blocking(live, list));
    set.register(wire::VERSIONS_METHOD, blocking(live, versions));
    set.register(wire::READ_METHOD, blocking(live, read));
    set.register(wire::EXCLUDE_METHOD, blocking(live, exclude));
    set.register(wire::CONTEXT_LIST_METHOD, blocking(live, context_list));
    set.register(wire::CONTEXT_READ_METHOD, blocking(live, context_read));
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextArtifactParams {
    token: String,
    #[serde(default)]
    task_id: Option<String>,
    /// `art-…` or `art-…@3`.
    #[serde(default)]
    artifact: Option<String>,
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
}

/// A task the caller may read, and how it is tied to the caller's.
struct Tie {
    task_id: String,
    title: String,
    relation: &'static str,
}

/// The token's task, its params, and every task it may read (itself first).
fn context_scope(
    live: &Live,
    request: &Request,
) -> Result<(ContextArtifactParams, String, Vec<Tie>), Box<Frame>> {
    let p: ContextArtifactParams = params(request)?;
    let Some((own, project)) = live.executor.context_tokens().resolve(&p.token) else {
        return Err(Box::new(failure(
            request,
            ErrorCode::Unauthenticated,
            "上下文令牌无效或会话已结束",
        )));
    };
    let records = live
        .service
        .load(&project)
        .map_err(|e| Box::new(failure(request, ErrorCode::IoError, &e.to_string())))?
        .records;
    let mine = records.iter().find(|r| id_of(r) == Some(own.as_str()));
    let mut ties: Vec<Tie> = records
        .iter()
        .filter_map(|r| {
            let relation = match mine {
                Some(mine) => relation(mine, r)?,
                None if id_of(r) == Some(own.as_str()) => "当前任务",
                None => return None,
            };
            Some(Tie {
                task_id: id_of(r)?.to_string(),
                title: super::context_title(r),
                relation,
            })
        })
        .collect();
    if !ties.iter().any(|t| t.task_id == own) {
        ties.push(Tie {
            task_id: own.clone(),
            title: String::new(),
            relation: "当前任务",
        });
    }
    ties.sort_by_key(|t| t.task_id != own);
    Ok((p, own, ties))
}

fn id_of(record: &Record) -> Option<&str> {
    record.get("id").and_then(Value::as_str)
}

/// The task a record was delegated or branched from.
fn parent_of(record: &Record) -> Option<&str> {
    [DELEGATE_OF_FIELD, BRANCH_OF_FIELD]
        .iter()
        .find_map(|field| record.get(field).and_then(Value::as_str))
        .filter(|p| !p.is_empty())
}

/// How `other` is tied to `mine`; none: the agent may not read it.
fn relation(mine: &Record, other: &Record) -> Option<&'static str> {
    let (me, them) = (id_of(mine)?, id_of(other)?);
    if me == them {
        return Some("当前任务");
    }
    if parent_of(other) == Some(me) {
        return Some("你委派或分出的任务");
    }
    let parent = parent_of(mine)?;
    if parent == them {
        return Some("上级任务");
    }
    (parent_of(other) == Some(parent)).then_some("同一上级下的任务")
}

/// `context.artifactList {token, taskId?, cursor?}`: a task's deliverables
/// (the caller's own by default) and the tasks it may also read.
fn context_list(live: &Live, request: &Request) -> Frame {
    let (p, own, ties) = match context_scope(live, request) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let wanted = p.task_id.clone().unwrap_or(own);
    if !ties.iter().any(|t| t.task_id == wanted) {
        return failure(request, ErrorCode::NotFound, UNRELATED);
    }
    let mut page = live.executor.artifacts().list(&wanted, p.cursor.as_deref());
    if page.artifacts.is_empty() && p.cursor.is_none() {
        page.unavailable = live
            .executor
            .task_cwd(&wanted)
            .and_then(|cwd| artifacts::unavailable(&cwd));
    }
    let mut out = serde_json::to_value(&page).unwrap_or_default();
    out["taskId"] = json!(wanted);
    out["readable"] = ties
        .iter()
        .map(|t| json!({"taskId": t.task_id, "title": t.title, "relation": t.relation}))
        .collect();
    ok(request, out)
}

/// `context.artifactRead {token, artifact, taskId?, version?, offset?,
/// limit?}`: one version's bytes, the same the app shows. Without a task,
/// the artifact is looked for in every task the caller may read.
fn context_read(live: &Live, request: &Request) -> Frame {
    let (p, _, ties) = match context_scope(live, request) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let Some(artifact) = p.artifact.as_deref() else {
        return failure(
            request,
            ErrorCode::InvalidArgument,
            "缺少 artifact（形如 art-…@3）",
        );
    };
    let (id, named) = parse_ref(artifact);
    let registry = live.executor.artifacts();
    let task = match &p.task_id {
        Some(task) if ties.iter().any(|t| &t.task_id == task) => Some(task.clone()),
        Some(_) => return failure(request, ErrorCode::NotFound, UNRELATED),
        None => ties
            .iter()
            .find(|t| registry.versions(&t.task_id, &id).is_some())
            .map(|t| t.task_id.clone()),
    };
    let Some(task) = task else {
        return failure(request, ErrorCode::NotFound, NO_ARTIFACT);
    };
    match registry.read(
        &task,
        &id,
        p.version.or(named),
        p.offset.unwrap_or(0),
        p.limit,
    ) {
        Ok(content) => ok(request, json!({ "taskId": task, "content": content })),
        Err(reason) => failure(request, ErrorCode::NotFound, &reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, field: Option<(&str, &str)>) -> Record {
        let r = Record::default().with("id", json!(id));
        match field {
            Some((k, v)) => r.with(k, json!(v)),
            None => r,
        }
    }

    #[test]
    fn an_agent_reads_its_own_its_children_its_parent_and_its_siblings_only() {
        let parent = record("p", None);
        let child = record("c", Some((DELEGATE_OF_FIELD, "p")));
        let sibling = record("s", Some((DELEGATE_OF_FIELD, "p")));
        let branch = record("b", Some((BRANCH_OF_FIELD, "c")));
        let stranger = record("x", None);
        let cousin = record("y", Some((DELEGATE_OF_FIELD, "x")));

        assert_eq!(relation(&child, &child), Some("当前任务"));
        assert_eq!(relation(&child, &parent), Some("上级任务"));
        assert_eq!(relation(&child, &sibling), Some("同一上级下的任务"));
        assert_eq!(relation(&child, &branch), Some("你委派或分出的任务"));
        assert_eq!(relation(&parent, &child), Some("你委派或分出的任务"));
        assert_eq!(relation(&child, &stranger), None);
        assert_eq!(relation(&child, &cousin), None);
        assert_eq!(relation(&parent, &branch), None, "a grandchild is not tied");
        assert_eq!(relation(&stranger, &cousin), Some("你委派或分出的任务"));
        assert_eq!(relation(&parent, &stranger), None);
    }
}
