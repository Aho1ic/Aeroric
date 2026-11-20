//! C7 (2026-11-20): `skills.catalog` — every skill from every source, which
//! one wins a name — and `skills.mounted`, what each launch of a session
//! was given (kept by the executor when it starts one). Read only.

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::extensions::catalog;
use aeroric_core::projects::mutations;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::skills as wire;
use serde_json::json;

use super::{blocking, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .skill_mounts()
        .set_dir(live.context.paths.data_dir().to_path_buf());
    set.register(wire::CATALOG_METHOD, blocking(live, skills_catalog));
    set.register(wire::MOUNTED_METHOD, blocking(live, skills_mounted));
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogParams {
    #[serde(default)]
    project_id: Option<String>,
}

/// A registered project's folder; never a path the caller makes up.
fn project_dir(live: &Live, project_id: &str) -> Option<PathBuf> {
    aeroric_core::storage::load_projects(&live.context)
        .ok()?
        .iter()
        .find(|record| mutations::text(record, "id") == Some(project_id))
        .and_then(|record| mutations::text(record, "path"))
        .map(PathBuf::from)
}

/// `skills.catalog {projectId?}`: with a project, its own skill folders and
/// learned skills count too.
fn skills_catalog(live: &Live, request: &Request) -> Frame {
    let p: CatalogParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let cwd = p.project_id.as_deref().and_then(|id| project_dir(live, id));
    let home = aeroric_core::settings::home_dir();
    let skills = catalog::catalog(
        live.context.paths.data_dir(),
        cwd.as_deref(),
        home.as_deref(),
    );
    ok(request, json!({ "skills": skills }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MountedParams {
    task_id: String,
}

/// `skills.mounted {taskId}`: every launch kept, oldest first; none for a
/// session started before C7 (the App says 未记录).
fn skills_mounted(live: &Live, request: &Request) -> Frame {
    let p: MountedParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let launches = live.executor.skill_mounts().launches(&p.task_id);
    ok(request, json!({ "launches": launches }))
}
