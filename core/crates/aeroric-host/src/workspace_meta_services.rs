//! The sidebar's 更多 menus (2026-10-28): project display name, folder tint,
//! default Agent, group and removal; session pin, rename and removal. Every
//! write goes through the core services' own reducers; removing a project or
//! a session never touches files on disk or the agents' own transcripts.

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::projects::{mutations, GroupConfig, ProjectService};
use aeroric_core::tasks::{NewTask, ProtocolFamily, TaskService};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

fn text(request: &Request, key: &str) -> Option<String> {
    request
        .params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn invalid(request: &Request, message: &str) -> Frame {
    failure(request.id.clone(), ErrorCode::InvalidArgument, message)
}

pub fn register(set: &mut ServiceSet, context: Arc<CoreContext>, tasks: Arc<TaskService>) {
    type Run = fn(&Request, &Arc<CoreContext>, &TaskService) -> Frame;
    let methods: [(&str, Run); 7] = [
        ("project.update_meta", project_update_meta),
        ("project.group_names", project_group_names),
        ("project.remove", project_remove),
        ("task.set_starred", task_set_starred),
        ("task.rename", task_rename),
        ("task.delete", task_delete),
        ("task.fork", task_fork),
    ];
    for (name, run) in methods {
        let context = Arc::clone(&context);
        let tasks = Arc::clone(&tasks);
        set.register(
            name,
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let tasks = Arc::clone(&tasks);
                Box::pin(async move {
                    let id = request.id.clone();
                    match tokio::task::spawn_blocking(move || run(&request, &context, &tasks)).await
                    {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }
}

/// `{projectId, name?, color?, defaultAgent?, group?}`: present keys apply,
/// `null` (or an empty name) clears back to the default.
fn project_update_meta(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let Some(project_id) = text(request, "projectId") else {
        return invalid(request, "project.update_meta requires params.projectId");
    };
    let service = ProjectService::new(Arc::clone(context));
    let params = &request.params;
    let result = (|| -> Result<(), String> {
        if let Some(name) = params.get("name") {
            let name = name.as_str().map(str::trim).unwrap_or_default().to_string();
            let name = if name.is_empty() {
                let projects = service.list().map_err(|e| e.to_string())?;
                let project = projects
                    .iter()
                    .find(|p| mutations::id_of(p) == Some(project_id.as_str()))
                    .ok_or("项目不存在")?;
                mutations::derive_project_name(mutations::text(project, "path").unwrap_or_default())
            } else {
                name
            };
            service
                .rename(&project_id, &name)
                .map_err(|e| e.to_string())?;
        }
        for key in mutations::META_KEYS {
            if let Some(value) = params.get(key) {
                let value = value
                    .as_str()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(|v| Value::from(v.to_string()));
                service
                    .set_meta(&project_id, key, value)
                    .map_err(|e| e.to_string())?;
            }
        }
        if let Some(group) = params.get("group") {
            let group = group
                .as_str()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| Value::from(v.to_string()));
            service
                .assign_group(&project_id, group)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => ok(request, json!({ "ok": true })),
        Err(message) => failure(request.id.clone(), ErrorCode::IoError, &message),
    }
}

fn project_group_names(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let service = ProjectService::new(Arc::clone(context));
    let names = service
        .configured_groups()
        .and_then(|names| service.group_names(&GroupConfig { names }));
    match names {
        Ok(names) => ok(request, json!({ "groups": names })),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
    }
}

fn removed_file(context: &CoreContext) -> std::path::PathBuf {
    context.paths.data_dir().join("removed-projects.json")
}

static REMOVED_LOCK: Mutex<()> = Mutex::new(());

fn load_removed(context: &CoreContext) -> serde_json::Map<String, Value> {
    std::fs::read(removed_file(context))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn save_removed(context: &CoreContext, removed: serde_json::Map<String, Value>) {
    if let Ok(bytes) = serde_json::to_vec_pretty(&Value::Object(removed)) {
        let _ = aeroric_core::storage::atomic::write_atomic(
            &removed_file(context),
            &bytes,
            aeroric_core::storage::atomic::FileMode::Private,
        );
    }
}

/// Removing a project only drops it from the list; its id is kept by
/// location so opening the same folder again brings its sessions back (S3).
fn project_remove(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let Some(project_id) = text(request, "projectId") else {
        return invalid(request, "project.remove requires params.projectId");
    };
    let service = ProjectService::new(Arc::clone(context));
    let identity = service.list().ok().and_then(|projects| {
        projects
            .iter()
            .find(|p| mutations::id_of(p) == Some(project_id.as_str()))
            .map(mutations::identity_of)
    });
    match service.remove(&project_id) {
        Ok(_) => {
            if let Some(identity) = identity {
                let _guard = REMOVED_LOCK.lock();
                let mut removed = load_removed(context);
                removed.insert(identity, Value::from(project_id));
                save_removed(context, removed);
            }
            ok(request, json!({ "removed": true }))
        }
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
    }
}

/// The id a re-opened project at `identity` gets: the one it had before it
/// was removed, else the caller's.
pub fn reuse_removed_id(context: &CoreContext, identity: &str, new_id: &str) -> String {
    let _guard = REMOVED_LOCK.lock();
    let mut removed = load_removed(context);
    match removed
        .remove(identity)
        .and_then(|v| v.as_str().map(str::to_string))
    {
        Some(old) => {
            save_removed(context, removed);
            old
        }
        None => new_id.to_string(),
    }
}

fn task_project(
    request: &Request,
    context: &Arc<CoreContext>,
    task_id: &str,
) -> Result<String, Box<Frame>> {
    crate::services::find_project_of_task(context, task_id)
        .and_then(|project| mutations::id_of(&project).map(str::to_string))
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no task with that id in any known project",
            ))
        })
}

fn task_set_starred(request: &Request, context: &Arc<CoreContext>, tasks: &TaskService) -> Frame {
    let Some(task_id) = text(request, "taskId") else {
        return invalid(request, "task.set_starred requires params.taskId");
    };
    let wanted = request
        .params
        .get("starred")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let project_id = match task_project(request, context, &task_id) {
        Ok(id) => id,
        Err(frame) => return *frame,
    };
    let current = aeroric_core::storage::tasks::load_tasks(context, &project_id)
        .ok()
        .and_then(|records| {
            records
                .iter()
                .find(|r| r.id() == Some(task_id.as_str()))
                .map(|r| r.get("starred").and_then(Value::as_bool).unwrap_or(false))
        })
        .unwrap_or(false);
    if current == wanted {
        return ok(request, json!({ "changed": false }));
    }
    match tasks.toggle_star(&project_id, &task_id) {
        Ok(applied) => ok(request, json!({ "changed": applied.changed })),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
    }
}

fn task_rename(request: &Request, context: &Arc<CoreContext>, tasks: &TaskService) -> Frame {
    let Some(task_id) = text(request, "taskId") else {
        return invalid(request, "task.rename requires params.taskId");
    };
    let name = request
        .params
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let project_id = match task_project(request, context, &task_id) {
        Ok(id) => id,
        Err(frame) => return *frame,
    };
    match tasks.rename(&project_id, &task_id, &name) {
        Ok(applied) => ok(request, json!({ "changed": applied.changed })),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
    }
}

/// Removes task rows only; transcripts on disk stay (S7). Pinned rows are
/// reported back, not removed.
fn task_delete(request: &Request, context: &Arc<CoreContext>, tasks: &TaskService) -> Frame {
    let ids: Vec<String> = request
        .params
        .get("taskIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let Some(first) = ids.first() else {
        return invalid(request, "task.delete requires params.taskIds");
    };
    let project_id = match task_project(request, context, first) {
        Ok(id) => id,
        Err(frame) => return *frame,
    };
    match tasks.delete(&project_id, &ids) {
        Ok(deleted) => ok(
            request,
            json!({ "removed": deleted.removed, "skippedStarred": deleted.skipped_starred }),
        ),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
    }
}

/// S6: a new session continuing `taskId`'s conversation as its own. The new
/// task waits (todo) with `fork:<source>` as its session; its first prompt
/// starts the fork. Claude and Codex only — omp has no fork in its protocol.
fn task_fork(request: &Request, context: &Arc<CoreContext>, tasks: &TaskService) -> Frame {
    let Some(task_id) = text(request, "taskId") else {
        return invalid(request, "task.fork requires params.taskId");
    };
    let Some(project) = crate::services::find_project_of_task(context, &task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let Some(project_id) = mutations::id_of(&project).map(str::to_string) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "the task's project has no id",
        );
    };
    let result = (|| -> Result<String, String> {
        let records = aeroric_core::storage::tasks::load_tasks(context, &project_id)
            .map_err(|e| e.to_string())?;
        let source = records
            .iter()
            .find(|r| r.id() == Some(task_id.as_str()))
            .ok_or("会话不存在")?;
        let field = |key: &str| {
            source
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let family = source
            .get("sessionFamily")
            .and_then(Value::as_str)
            .and_then(|name| {
                serde_json::from_value::<ProtocolFamily>(Value::String(name.into())).ok()
            })
            .unwrap_or(ProtocolFamily::Codex);
        if !matches!(family, ProtocolFamily::Claude | ProtocolFamily::Codex) {
            return Err("只有 Claude Code 和 Codex 的会话可以分叉".into());
        }
        let (session, _) = aeroric_core::tasks::session_owner::current_session(source);
        let session = session
            .filter(|s| aeroric_core::sessions::live::fork_source(s).is_none())
            .ok_or("这个会话还没有可分叉的对话")?;
        let agent = field("agent").ok_or("会话没有记录 Agent")?;
        let new_id = format!("task-{}", uuid::Uuid::new_v4().simple());
        tasks
            .create(
                &project,
                NewTask {
                    id: new_id.clone(),
                    project_id: project_id.clone(),
                    prompt: String::new(),
                    agent: agent.clone(),
                    permission_mode: field("permissionMode").unwrap_or_else(|| "ask".into()),
                    immediate: false,
                    selected_model: field("selectedModel"),
                    reasoning_effort: field("reasoningEffort"),
                    speed: field("speed"),
                    dsh_agent_preset: None,
                    agent_config_id: field("agentConfigId"),
                    images: Vec::new(),
                    texts: Vec::new(),
                },
            )
            .map_err(|e| e.to_string())?;
        tasks
            .attach_session(
                &project_id,
                &new_id,
                family,
                &agent,
                Some(&format!(
                    "{}{session}",
                    aeroric_core::sessions::live::FORK_PREFIX
                )),
                None,
            )
            .map_err(|e| e.to_string())?;
        let title = field("name")
            .filter(|n| n != &agent)
            .or_else(|| field("title"))
            .or_else(|| field("prompt").and_then(|p| p.lines().next().map(str::to_string)))
            .unwrap_or_else(|| "会话".into());
        let title: String = title.chars().take(60).collect();
        tasks
            .rename(&project_id, &new_id, &format!("{title}（分叉）"))
            .map_err(|e| e.to_string())?;
        Ok(new_id)
    })();
    match result {
        Ok(new_id) => ok(request, json!({ "taskId": new_id })),
        Err(message) => failure(request.id.clone(), ErrorCode::InvalidArgument, &message),
    }
}
