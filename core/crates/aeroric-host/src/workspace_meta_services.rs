//! The sidebar's 更多 menus (2026-10-28): project display name, folder tint,
//! default Agent, group and removal; session pin, rename and removal. Every
//! write goes through the core services' own reducers; removing a project or
//! a session never touches files on disk or the agents' own transcripts.

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::projects::tags::{TagError, TagStore, TagTarget};
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
    let methods: [(&str, Run); 16] = [
        ("project.linked_folders", project_linked_folders),
        ("project.link_folder", project_link_folder),
        ("project.unlink_folder", project_unlink_folder),
        ("tags.list", tags_list),
        ("tags.create", tags_create),
        ("tags.update", tags_update),
        ("tags.delete", tags_delete),
        ("tags.assign", tags_assign),
        ("project.update_meta", project_update_meta),
        ("project.group_names", project_group_names),
        ("project.remove", project_remove),
        ("task.set_starred", task_set_starred),
        ("task.rename", task_rename),
        ("task.delete", task_delete),
        ("task.fork", task_fork),
        ("imports.continue", import_continue),
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

// MARK: linked folders (2026-10-29)

fn folder_rows(folders: &[String]) -> Value {
    Value::Array(
        folders
            .iter()
            .map(|path| {
                let name = std::path::Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                json!({ "path": path, "name": name, "exists": std::path::Path::new(path).is_dir() })
            })
            .collect(),
    )
}

/// `{}` → `{projects: {<projectId>: [{path, name, exists}]}}`, projects with links only.
fn project_linked_folders(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let service = ProjectService::new(Arc::clone(context));
    match service.list() {
        Ok(projects) => {
            let mut out = serde_json::Map::new();
            for project in &projects {
                let folders = aeroric_core::projects::linked::stored(project);
                if let (Some(id), false) = (mutations::id_of(project), folders.is_empty()) {
                    out.insert(id.to_string(), folder_rows(&folders));
                }
            }
            ok(request, json!({ "projects": out }))
        }
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

/// `{projectId, path}` → `{folders}`; refused with the reason the rule gives.
fn project_link_folder(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let (Some(project_id), Some(path)) = (text(request, "projectId"), text(request, "path")) else {
        return invalid(
            request,
            "project.link_folder requires params.projectId and params.path",
        );
    };
    match ProjectService::new(Arc::clone(context)).link_folder(&project_id, &path) {
        Ok(folders) => ok(request, json!({ "folders": folder_rows(&folders) })),
        Err(message) => invalid(request, &message),
    }
}

/// `{projectId, path}` → `{folders}`.
fn project_unlink_folder(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let (Some(project_id), Some(path)) = (text(request, "projectId"), text(request, "path")) else {
        return invalid(
            request,
            "project.unlink_folder requires params.projectId and params.path",
        );
    };
    match ProjectService::new(Arc::clone(context)).unlink_folder(&project_id, &path) {
        Ok(folders) => ok(request, json!({ "folders": folder_rows(&folders) })),
        Err(message) => failure(request.id.clone(), ErrorCode::IoError, &message),
    }
}

// MARK: tags (2026-10-29)

fn tag_store(context: &CoreContext) -> TagStore {
    TagStore::new(context.paths.tags_file())
}

fn tag_failure(request: &Request, error: TagError) -> Frame {
    let code = match error {
        TagError::Invalid(_) => ErrorCode::InvalidArgument,
        TagError::NotFound => ErrorCode::NotFound,
        TagError::Storage(_) => ErrorCode::IoError,
    };
    failure(request.id.clone(), code, &error.to_string())
}

/// `{}` → `{tags, assignments}`.
fn tags_list(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    match tag_store(context).load() {
        Ok(library) => ok(request, serde_json::to_value(library).unwrap_or_default()),
        Err(error) => tag_failure(request, error),
    }
}

/// `{name, color, projectId?}` → the new tag.
fn tags_create(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let (Some(name), Some(color)) = (text(request, "name"), text(request, "color")) else {
        return invalid(request, "tags.create requires params.name and params.color");
    };
    match tag_store(context).create(&name, &color, text(request, "projectId").as_deref()) {
        Ok(tag) => ok(request, serde_json::to_value(tag).unwrap_or_default()),
        Err(error) => tag_failure(request, error),
    }
}

/// `{id, name?, color?, scope?}` → the changed tag.
fn tags_update(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let Some(id) = text(request, "id") else {
        return invalid(request, "tags.update requires params.id");
    };
    let (name, color) = (text(request, "name"), text(request, "color"));
    // `scope`: "all", or the project id a 仅当前项目 tag belongs to.
    let scope = text(request, "scope");
    let scope = scope.as_deref().map(|s| (s != "all").then_some(s));
    match tag_store(context).update(&id, name.as_deref(), color.as_deref(), scope) {
        Ok(tag) => ok(request, serde_json::to_value(tag).unwrap_or_default()),
        Err(error) => tag_failure(request, error),
    }
}

/// `{id}`: removes the tag from the library and from every row.
fn tags_delete(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let Some(id) = text(request, "id") else {
        return invalid(request, "tags.delete requires params.id");
    };
    match tag_store(context).delete(&id) {
        Ok(()) => ok(request, json!({ "ok": true })),
        Err(error) => tag_failure(request, error),
    }
}

/// `{target, kind: "project"|"session", projectId?, tagIds}`: sets exactly
/// which tags the sidebar row `target` carries.
fn tags_assign(request: &Request, context: &Arc<CoreContext>, _: &TaskService) -> Frame {
    let Some(target) = text(request, "target") else {
        return invalid(request, "tags.assign requires params.target");
    };
    let tag_ids: Vec<String> = request
        .params
        .get("tagIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let project_id = text(request, "projectId");
    let kind = match text(request, "kind").as_deref() {
        Some("project") => TagTarget::Project {
            project_id: project_id.as_deref().unwrap_or(target.as_str()),
        },
        Some("session") => TagTarget::Session {
            project_id: project_id.as_deref(),
        },
        _ => {
            return invalid(
                request,
                "tags.assign requires params.kind project or session",
            )
        }
    };
    match tag_store(context).assign(&target, kind, &tag_ids) {
        Ok(ids) => ok(request, json!({ "tagIds": ids })),
        Err(error) => tag_failure(request, error),
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
    match fork_task(context, tasks, &task_id, "分叉") {
        Ok(new_id) => ok(request, json!({ "taskId": new_id })),
        Err(ForkError::NotFound(message)) => {
            failure(request.id.clone(), ErrorCode::NotFound, message)
        }
        Err(ForkError::Invalid(message)) => {
            failure(request.id.clone(), ErrorCode::InvalidArgument, &message)
        }
    }
}

pub(crate) enum ForkError {
    NotFound(&'static str),
    Invalid(String),
}

/// A new task continuing `task_id`'s conversation, titled "<title>（<suffix>）";
/// its id. Claude and Codex only.
pub(crate) fn fork_task(
    context: &Arc<CoreContext>,
    tasks: &TaskService,
    task_id: &str,
    suffix: &str,
) -> Result<String, ForkError> {
    let task_id = task_id.to_string();
    let Some(project) = crate::services::find_project_of_task(context, &task_id) else {
        return Err(ForkError::NotFound(
            "no task with that id in any known project",
        ));
    };
    let Some(project_id) = mutations::id_of(&project).map(str::to_string) else {
        return Err(ForkError::NotFound("the task's project has no id"));
    };
    (|| -> Result<String, String> {
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
            .rename(&project_id, &new_id, &format!("{title}（{suffix}）"))
            .map_err(|e| e.to_string())?;
        Ok(new_id)
    })()
    .map_err(ForkError::Invalid)
}

/// 接着聊（2026-10-28）：an imported Claude / Codex / DeepSeek Harness / OMP
/// session becomes an Aeroric task bound to the agent's own session, so the
/// next send resumes it. The transcript stays where the agent keeps it; the
/// import row then opens the task. Calling it again returns the same task.
fn import_continue(request: &Request, context: &Arc<CoreContext>, tasks: &TaskService) -> Frame {
    use aeroric_core::sessions::import::{native_session_id, ImportAgent, ImportStore};
    let Some(id) = text(request, "id") else {
        return invalid(request, "imports.continue requires params.id");
    };
    let store = ImportStore::new(context.paths.data_dir());
    let Some(session) = store.index().into_iter().find(|s| s.id == id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no imported session with that id",
        );
    };
    if let Some(task) = store.continued_task(&id) {
        if let Some(project) = crate::services::find_project_of_task(context, &task) {
            return ok(
                request,
                json!({ "taskId": task, "projectId": mutations::id_of(&project) }),
            );
        }
    }
    let (family, harness) = match session.agent {
        ImportAgent::Claude => (ProtocolFamily::Claude, "claude"),
        ImportAgent::Codex => (ProtocolFamily::Codex, "codex"),
        ImportAgent::Dsh => (ProtocolFamily::Dsh, "dsh"),
        ImportAgent::Omp => (ProtocolFamily::Omp, "omp"),
        _ => {
            return invalid(
                request,
                "这个 Agent 的会话不能在 Aeroric 里续接，请分叉为新会话",
            )
        }
    };
    let Some(project_id) = text(request, "projectId").or(session.project_id.clone()) else {
        return invalid(request, "needs_project: 会话所在的文件夹还不是项目");
    };
    // The configuration the client picked for this harness, like a new
    // session's (2026-10-28: without it the task ran on the CLI's own login
    // whatever model was chosen later, and an Agent configuration's model went
    // to the wrong account). The agent comes with it; the harness never changes.
    let configuration = text(request, "configurationId");
    let agent = text(request, "agent").unwrap_or_else(|| harness.to_string());
    if aeroric_core::agents::families::built_in_session_family(&agent).is_some_and(|f| f != family)
    {
        return invalid(request, "所选配置与原会话的 Harness 不一致");
    }
    let result = (|| -> Result<String, String> {
        let project = ProjectService::new(Arc::clone(context))
            .list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|record| mutations::id_of(record) == Some(project_id.as_str()))
            .ok_or("项目不存在")?;
        let native = native_session_id(&session).ok_or("读不到原会话的 id")?;
        let new_id = format!("task-{}", uuid::Uuid::new_v4().simple());
        tasks
            .create(
                &project,
                NewTask {
                    id: new_id.clone(),
                    project_id: project_id.clone(),
                    prompt: String::new(),
                    agent: agent.clone(),
                    permission_mode: text(request, "permissionMode")
                        .unwrap_or_else(|| "ask".into()),
                    immediate: false,
                    selected_model: text(request, "model"),
                    reasoning_effort: text(request, "effort"),
                    speed: None,
                    dsh_agent_preset: None,
                    agent_config_id: configuration
                        .as_deref()
                        .and_then(aeroric_core::agent_connections::launch::config_id_from_catalog)
                        .map(str::to_string),
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
                Some(&native),
                Some(&session.source_path.to_string_lossy()),
            )
            .map_err(|e| e.to_string())?;
        let title: String = session.title.chars().take(60).collect();
        tasks
            .rename(&project_id, &new_id, &title)
            .map_err(|e| e.to_string())?;
        store
            .set_continued(&id, &new_id)
            .map_err(|e| e.to_string())?;
        Ok(new_id)
    })();
    match result {
        Ok(task) => ok(request, json!({ "taskId": task, "projectId": project_id })),
        Err(message) => failure(request.id.clone(), ErrorCode::InvalidArgument, &message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
    use aeroric_core::paths::AppPaths;
    use aeroric_core::sessions::import::{ImportAgent, ImportStore};
    use aeroric_core::storage_lock::LeaseOwner;
    use aeroric_core::tasks::{
        CancelTarget, ExecutorFailure, LaunchRequest, NoLiveRuns, ResetOutcome, ResumeRequest,
        TaskExecutor,
    };
    use std::path::Path;

    struct Local;
    impl StorageLocationProbe for Local {
        fn probe(&self, path: &Path) -> StorageLocation {
            StorageLocation::new(path, VolumeKind::Local)
        }
    }

    /// Nothing is launched here: continuing only creates and binds the task.
    struct NoExecutor;
    impl TaskExecutor for NoExecutor {
        fn launch(&self, _: &LaunchRequest) -> Result<(), ExecutorFailure> {
            panic!("continuing must not launch")
        }
        fn resume(&self, _: &ResumeRequest) -> Result<(), ExecutorFailure> {
            panic!("continuing must not resume yet")
        }
        fn cancel(&self, _: &str, _: &CancelTarget) -> Result<(), ExecutorFailure> {
            panic!("no cancel")
        }
        fn complete(&self, _: &str, _: &str) -> Result<(), ExecutorFailure> {
            panic!("no complete")
        }
        fn reset(&self, _: &str) -> Result<ResetOutcome, ExecutorFailure> {
            panic!("no reset")
        }
    }

    fn request(params: Value) -> Request {
        serde_json::from_value(
            json!({ "v": 1, "id": "r1", "method": "imports.continue", "params": params }),
        )
        .unwrap()
    }

    #[test]
    fn continuing_binds_a_task_to_the_agents_own_session_once() {
        let root = std::env::temp_dir().join(format!("aeroric-continue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project_dir = root.join("work-app");
        std::fs::create_dir_all(&project_dir).unwrap();
        let transcript = root.join("home/.claude/projects/-work-app/0b1c-uuid.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(
            &transcript,
            format!(
                "{}\n",
                json!({"type":"user","cwd": project_dir.to_string_lossy(),
                       "message":{"role":"user","content":"修复登录页"}})
            ),
        )
        .unwrap();
        let context = Arc::new(
            CoreContext::acquire(
                AppPaths::with_data_dir(root.join("home"), Some(&root.join("data"))),
                Arc::new(aeroric_core::clock::SystemClock),
                &LeaseOwner {
                    application: "continue-test".into(),
                    version: "0-test".into(),
                },
                &Local,
            )
            .unwrap(),
        );
        ProjectService::new(Arc::clone(&context))
            .open_local(&project_dir.to_string_lossy(), "p1", 1)
            .unwrap();
        let tasks = TaskService::new(
            Arc::clone(&context),
            Arc::new(crate::families::SettingsFamiliesCli::new(Arc::clone(
                &context,
            ))),
            Arc::new(NoExecutor),
            Arc::new(NoLiveRuns),
        )
        .unwrap();
        let store = ImportStore::new(context.paths.data_dir());
        store
            .grant(ImportAgent::Claude, root.join("home/.claude"), 1)
            .unwrap();
        let project_path = project_dir.to_string_lossy().into_owned();
        store.scan(&[("p1".to_string(), project_path)]).unwrap();
        let imported = store.index()[0].id.clone();

        let picked = json!({"id": imported, "configurationId": "agent-config:ds",
                            "agent": "claude", "model": "deepseek-v4.1-flash"});
        let reply = match import_continue(&request(picked), &context, &tasks) {
            Frame::Success(success) => success.result,
            other => panic!("{other:?}"),
        };
        let task = reply["taskId"].as_str().unwrap().to_string();
        assert_eq!(reply["projectId"], "p1");
        let records = aeroric_core::storage::tasks::load_tasks(&context, "p1").unwrap();
        let record = records
            .iter()
            .find(|r| r.id() == Some(task.as_str()))
            .unwrap();
        assert_eq!(
            record.get("claudeSessionId").and_then(Value::as_str),
            Some("0b1c-uuid")
        );
        assert_eq!(
            record.get("name").and_then(Value::as_str),
            Some("修复登录页")
        );
        assert_eq!(
            record.get("agentConfigId").and_then(Value::as_str),
            Some("ds"),
            "pinned to the picked configuration, like a new session"
        );
        assert_eq!(
            record.get("selectedModel").and_then(Value::as_str),
            Some("deepseek-v4.1-flash")
        );
        assert!(
            store.visible().is_empty(),
            "the import row now opens the task"
        );

        let again = match import_continue(&request(json!({"id": imported})), &context, &tasks) {
            Frame::Success(success) => success.result,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            again["taskId"],
            task.as_str(),
            "continuing twice reuses the task"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
