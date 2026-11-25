//! 2026-11-24 设置 › Git 与工作副本, the host's side: `gitSettings.get` /
//! `gitSettings.set` (`aeroric_core::settings::git_settings`) and
//! `copies.list`, the isolated copies there are now (read only).

use std::collections::HashMap;
use std::sync::Arc;

use aeroric_core::sessions::live::isolated_copy;
use aeroric_core::settings::git_settings::{self, GitSettings};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const GET_METHOD: &str = "gitSettings.get";
pub const SET_METHOD: &str = "gitSettings.set";
pub const COPIES_METHOD: &str = "copies.list";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(GET_METHOD, blocking(live, get));
    set.register(SET_METHOD, blocking(live, set_settings));
    set.register(COPIES_METHOD, blocking(live, copies));
}

fn get(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!(git_settings::load(live.context.paths.data_dir())),
    )
}

#[derive(serde::Deserialize)]
struct SetParams {
    settings: GitSettings,
}

/// `gitSettings.set {settings}` → what was saved.
fn set_settings(live: &Live, request: &Request) -> Frame {
    let p: SetParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match git_settings::save(live.context.paths.data_dir(), &p.settings) {
        Ok(saved) => ok(request, json!(saved)),
        Err(why) => failure(request.id.clone(), ErrorCode::InvalidArgument, &why),
    }
}

/// `copies.list {}` → `{copies: [{taskId, title?, path, origin, createdMs,
/// clearedMs?}]}`, newest first. `clearedMs` set: nothing left to bring back,
/// so it goes after the kept days.
fn copies(live: &Live, request: &Request) -> Frame {
    let dir = live.context.paths.data_dir().join("copies");
    let mut records = isolated_copy::list(&dir);
    records.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    let titles = titles(live);
    let copies: Vec<Value> = records
        .iter()
        .map(|record| {
            json!({
                "taskId": record.task_id,
                "title": titles.get(&record.task_id),
                "path": record.path,
                "origin": record.origin,
                "createdMs": record.created_ms,
                "clearedMs": record.cleared_ms,
            })
        })
        .collect();
    ok(request, json!({ "copies": copies }))
}

/// Every task's title (its name, else the first line of its prompt).
fn titles(live: &Live) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    let projects = aeroric_core::storage::load_projects(&live.context).unwrap_or_default();
    for project in projects {
        let Some(project_id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        let tasks =
            aeroric_core::storage::tasks::load_tasks(&live.context, project_id).unwrap_or_default();
        for task in tasks {
            let Some(id) = task.get("id").and_then(Value::as_str) else {
                continue;
            };
            let title = ["name", "prompt"].iter().find_map(|key| {
                task.get(*key)
                    .and_then(Value::as_str)
                    .and_then(|text| text.lines().map(str::trim).find(|line| !line.is_empty()))
                    .map(|line| line.chars().take(80).collect::<String>())
            });
            if let Some(title) = title {
                titles.insert(id.to_string(), title);
            }
        }
    }
    titles
}
