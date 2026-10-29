//! 只读与幂等登记服务（P3-07 ③a：project.list/open、task.list/get）。
//!
//! 执行类服务（create/start/cancel/resume）在 ③b——那需要 core 侧 PTY executor，
//! 本文件不预建空壳。`project.open` 虽是写路径，但它的幂等由 `newId` 承担
//! （同 id 重发＝reused），与 Action 6 的契约同形，随读侧一起立。
//!
//! handler 的失败一律回 protocol `Failure`（NotFound/InvalidArgument），
//! **不 panic、不断连**——CLI 侧按码映射退出码是 client.rs 的事。

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::projects::{mutations, ProjectService};
use aeroric_core::sessions::dsh::read_compressed_dsh_lines;
use aeroric_core::sessions::history::{
    read_session_page_lines, SessionMessagePage, SESSION_MESSAGE_PAGE_LINES,
};
use aeroric_core::sessions::parser::parse_session_messages_with_format;
use aeroric_core::storage::tasks::{load_stamped, load_tasks};
use aeroric_core::storage::StorageError;
use aeroric_protocol::envelope::{Frame, Request, Success};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};
use crate::file_access::{require_local_record, write_refusal, FileProjectError, FileTarget};

pub fn register_read_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    {
        let context = Arc::clone(&context);
        set.register(
            "project.list",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(
                    async move { project_list(request, context, ProjectListReplyShape::Object) },
                )
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.load_projects",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(
                    async move { project_list(request, context, ProjectListReplyShape::Array) },
                )
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.load_project_tasks",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { project_load_tasks(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.read_task_terminal_history",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { project_read_task_terminal_history(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.delete_task_terminal_histories",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(
                    async move { project_delete_task_terminal_histories(request, context).await },
                )
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.save_projects",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { project_save_projects(request, context).await })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "session.read_session_id",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { session_read_session_id(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "session.session_supports_native_resume",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { session_supports_native_resume(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "session.read_session_message_page",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { session_read_session_message_page(request, context) })
            }),
        );
    }
    {
        set.register(
            "git.worktree_diff_stats",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_worktree_diff_stats(request).await })
            }),
        );
    }
    {
        set.register(
            "git.remove_task_worktree",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_remove_task_worktree(request).await })
            }),
        );
    }
    {
        set.register(
            "git.create_task_worktree",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_create_task_worktree(request).await })
            }),
        );
    }
    {
        set.register(
            "git.merge_task_worktree",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_merge_task_worktree(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_status",
            Arc::new(move |request, _call| Box::pin(async move { git_git_status(request).await })),
        );
    }
    {
        set.register(
            "git.git_list_branches",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_list_branches(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_log",
            Arc::new(move |request, _call| Box::pin(async move { git_git_log(request).await })),
        );
    }
    {
        set.register(
            "git.git_show_diff",
            Arc::new(move |request, _call| Box::pin(async move { git_show_diff(request).await })),
        );
    }
    {
        set.register(
            "git.git_stage",
            Arc::new(move |request, _call| Box::pin(async move { git_stage(request).await })),
        );
    }
    {
        set.register(
            "git.git_unstage",
            Arc::new(move |request, _call| Box::pin(async move { git_unstage(request).await })),
        );
    }
    {
        set.register(
            "git.git_stage_files",
            Arc::new(move |request, _call| Box::pin(async move { git_stage_files(request).await })),
        );
    }
    {
        set.register(
            "git.git_unstage_files",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_unstage_files(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_file_diff",
            Arc::new(move |request, _call| Box::pin(async move { git_file_diff(request).await })),
        );
    }
    {
        set.register(
            "git.git_show_file_diff",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_show_file_diff(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_commit_detail",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_commit_detail(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_discard_file",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_discard_file(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_discard_files",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_discard_files(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_discard_all",
            Arc::new(move |request, _call| Box::pin(async move { git_discard_all(request).await })),
        );
    }
    {
        set.register(
            "git.git_conflict_preview",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_conflict_preview(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_resolve_conflict",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_resolve_conflict(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_fetch",
            Arc::new(move |request, _call| Box::pin(async move { git_fetch(request).await })),
        );
    }
    {
        set.register(
            "git.git_pull",
            Arc::new(move |request, _call| Box::pin(async move { git_pull(request).await })),
        );
    }
    {
        set.register(
            "git.git_push",
            Arc::new(move |request, _call| Box::pin(async move { git_push(request).await })),
        );
    }
    {
        set.register(
            "git.git_clone",
            Arc::new(move |request, _call| Box::pin(async move { git_clone(request).await })),
        );
    }
    {
        set.register(
            "git.generate_commit_message",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_generate_commit_message(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_commit",
            Arc::new(move |request, _call| Box::pin(async move { git_commit(request).await })),
        );
    }
    {
        set.register(
            "git.git_stage_all",
            Arc::new(move |request, _call| Box::pin(async move { git_stage_all(request).await })),
        );
    }
    {
        set.register(
            "git.git_unstage_all",
            Arc::new(move |request, _call| Box::pin(async move { git_unstage_all(request).await })),
        );
    }
    {
        set.register(
            "git.git_conflict_files",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_conflict_files(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_init",
            Arc::new(move |request, _call| Box::pin(async move { git_init(request).await })),
        );
    }
    {
        set.register(
            "git.git_stash_push",
            Arc::new(move |request, _call| Box::pin(async move { git_stash_push(request).await })),
        );
    }
    {
        set.register(
            "git.git_stash_apply",
            Arc::new(move |request, _call| Box::pin(async move { git_stash_apply(request).await })),
        );
    }
    {
        set.register(
            "git.git_stash_drop",
            Arc::new(move |request, _call| Box::pin(async move { git_stash_drop(request).await })),
        );
    }
    {
        set.register(
            "git.git_stash_list",
            Arc::new(move |request, _call| Box::pin(async move { git_stash_list(request).await })),
        );
    }
    {
        set.register(
            "git.git_stash_diff",
            Arc::new(move |request, _call| Box::pin(async move { git_stash_diff(request).await })),
        );
    }
    {
        set.register(
            "git.git_create_branch",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_create_branch(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_checkout_branch",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_checkout_branch(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_remote_counts",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_remote_counts(request).await })
            }),
        );
    }
    {
        set.register(
            "git.git_blame_file",
            Arc::new(move |request, _call| Box::pin(async move { git_blame_file(request).await })),
        );
    }
    {
        set.register(
            "git.git_branch_graph",
            Arc::new(move |request, _call| {
                Box::pin(async move { git_branch_graph(request).await })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.open_ssh",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { project_open_ssh(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "project.open",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { project_open(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "task.list",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { task_list(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "task.get",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { task_get(request, context) })
            }),
        );
    }
    {
        set.register(
            "platform.get_platform_runtime_info",
            Arc::new(move |request, _call| {
                Box::pin(async move { platform_get_platform_runtime_info(request) })
            }),
        );
    }
    {
        set.register(
            "platform.build_runnable_file_command",
            Arc::new(move |request, _call| {
                Box::pin(async move { platform_build_runnable_file_command(request) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "terminal.validate_agent_launch",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { terminal_validate_agent_launch(request, context) })
            }),
        );
    }
}

pub(crate) fn ok(request: &Request, result: Value) -> Frame {
    Frame::Success(Success {
        v: aeroric_protocol::PROTOCOL_VERSION,
        id: request.id.clone(),
        ok: true,
        result,
        extra: Default::default(),
    })
}

// ADR-002 freezes the legacy command's wire name. Its bare array is a distinct
// contract from the intrinsic project.list object; only serialization differs.
enum ProjectListReplyShape {
    Object,
    Array,
}

fn project_list(
    request: Request,
    context: Arc<CoreContext>,
    reply_shape: ProjectListReplyShape,
) -> Frame {
    match ProjectService::new(context).list() {
        Ok(projects) => ok(
            &request,
            match reply_shape {
                ProjectListReplyShape::Object => json!({ "projects": projects }),
                ProjectListReplyShape::Array => json!(projects),
            },
        ),
        Err(e) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("cannot read the project list: {e}"),
        ),
    }
}

fn project_open(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(path) = request
        .params
        .get("path")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.open requires params.path (non-empty)",
        );
    };
    let Some(new_id) = request
        .params
        .get("newId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.open requires params.newId (the caller mints it; ADR-013)",
        );
    };
    let now_ms_inner = context.clock.now_ms() as i64;
    // S3: a folder removed earlier comes back with its old id (and sessions).
    let new_id = &crate::workspace_meta_services::reuse_removed_id(
        &context,
        &format!("local:{}", mutations::normalise_local_path(path)),
        new_id,
    );
    match ProjectService::new(context).open_local(path, new_id, now_ms_inner) {
        Ok((upserted, applied)) => ok(
            &request,
            json!({
                "project": upserted.project,
                "reused": upserted.reused,
                "revision": applied.revision,
            }),
        ),
        Err(e) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("cannot open the project: {e}"),
        ),
    }
}

/// A project on an SSH host (connection + remote directory), 2026-10-27.
fn project_open_ssh(request: Request, context: Arc<CoreContext>) -> Frame {
    let text = |key: &str| {
        request
            .params
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let (Some(connection_id), Some(remote_path), Some(new_id)) =
        (text("connectionId"), text("remotePath"), text("newId"))
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.open_ssh requires params.connectionId, remotePath and newId",
        );
    };
    if let Err(reason) = aeroric_core::connections::ssh::validate_remote_path(&remote_path) {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            &format!("远程目录无效：{reason}"),
        );
    }
    if let Err(reason) = crate::remote_sessions::load_connection(&context, &connection_id) {
        return failure(request.id, ErrorCode::NotFound, &reason);
    }
    let name = text("name").unwrap_or_else(|| {
        remote_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty() && *s != "~")
            .unwrap_or("远程项目")
            .to_string()
    });
    let now = context.clock.now_ms() as i64;
    let new_id = crate::workspace_meta_services::reuse_removed_id(
        &context,
        &format!(
            "ssh:{connection_id}:{}",
            mutations::normalize_remote_path(&remote_path)
        ),
        &new_id,
    );
    match ProjectService::new(context).open_ssh(&name, &connection_id, &remote_path, &new_id, now) {
        Ok((upserted, applied)) => ok(
            &request,
            json!({
                "project": upserted.project,
                "reused": upserted.reused,
                "revision": applied.revision,
            }),
        ),
        Err(e) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("cannot open the remote project: {e}"),
        ),
    }
}

// The legacy query addresses one task file, including an unregistered project.
// Do not replace it with task.list's registry scan and projectId projection.
fn project_load_tasks(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(project_id) = request.params.get("projectId").and_then(Value::as_str) else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.load_project_tasks requires params.projectId (string)",
        );
    };
    match load_tasks(&context, project_id) {
        Ok(tasks) => ok(&request, json!(tasks)),
        Err(error) => {
            let code = if matches!(&error, StorageError::InvalidId(_)) {
                ErrorCode::InvalidArgument
            } else {
                ErrorCode::IoError
            };
            failure(
                request.id,
                code,
                &format!("cannot read tasks of project {project_id}: {error}"),
            )
        }
    }
}
fn task_list(request: Request, context: Arc<CoreContext>) -> Frame {
    let projects = match ProjectService::new(Arc::clone(&context)).list() {
        Ok(projects) => projects,
        Err(e) => {
            return failure(
                request.id,
                ErrorCode::IoError,
                &format!("cannot read the project list: {e}"),
            )
        }
    };
    let wanted_path = request
        .params
        .get("projectPath")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty());
    let mut tasks = Vec::new();
    for project in &projects {
        let id = match mutations::id_of(project) {
            Some(id) => id.to_string(),
            None => continue,
        };
        if let Some(path) = wanted_path {
            if mutations::text(project, "path") != Some(path) {
                continue;
            }
        }
        match load_stamped(&context, &id) {
            Ok(stamped) => {
                for mut record in stamped.records {
                    record.0.insert("projectId".to_string(), json!(id.clone()));
                    tasks.push(record);
                }
            }
            Err(e) => {
                return failure(
                    request.id,
                    ErrorCode::IoError,
                    &format!("cannot read tasks of project {id}: {e}"),
                )
            }
        }
    }
    ok(&request, json!({ "tasks": tasks }))
}

fn task_get(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "task.get requires params.taskId",
        );
    };
    let projects = match ProjectService::new(Arc::clone(&context)).list() {
        Ok(projects) => projects,
        Err(e) => {
            return failure(
                request.id,
                ErrorCode::IoError,
                &format!("cannot read the project list: {e}"),
            )
        }
    };
    for project in &projects {
        let Some(id) = mutations::id_of(project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(&context, id) else {
            continue;
        };
        if let Some(found) = stamped
            .records
            .iter()
            .find(|record| mutations::text(record, "id") == Some(task_id))
        {
            return ok(&request, json!({"task": found, "projectId": id}));
        }
    }
    failure(
        request.id,
        ErrorCode::NotFound,
        "no task with that id in any known project",
    )
}

fn platform_get_platform_runtime_info(request: Request) -> Frame {
    let info = aeroric_core::platform::current_platform_runtime_info();
    match serde_json::to_value(info) {
        Ok(value) => ok(&request, value),
        Err(e) => failure(request.id, ErrorCode::IoError, &e.to_string()),
    }
}

fn platform_build_runnable_file_command(request: Request) -> Frame {
    let Some(params) = request.params.as_object() else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "platform.build_runnable_file_command parameters must be an object",
        );
    };

    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "filePath must be a string",
        );
    };

    let conda_path = match params.get("condaPath") {
        Some(Value::Null) | None => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                "condaPath must be a string or null",
            );
        }
    };

    let conda_python_path = match params.get("condaPythonPath") {
        Some(Value::Null) | None => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                "condaPythonPath must be a string or null",
            );
        }
    };

    let remote = match params.get("remote") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                "remote must be a boolean",
            );
        }
    };

    let shell = aeroric_core::platform::default_shell_command();
    match aeroric_core::platform::runnable_file_command(
        file_path,
        conda_path,
        conda_python_path,
        remote,
        &shell.program,
    ) {
        Ok(result) => match serde_json::to_value(result) {
            Ok(value) => ok(&request, value),
            Err(e) => failure(request.id, ErrorCode::IoError, &e.to_string()),
        },
        Err(reason) => failure(request.id, ErrorCode::InvalidArgument, &reason),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteTerminalHistoriesParams {
    task_ids: Vec<String>,
}

async fn project_delete_task_terminal_histories(
    request: Request,
    context: Arc<CoreContext>,
) -> Frame {
    if !request.params.is_object() {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.delete_task_terminal_histories requires params object",
        );
    }
    let params: DeleteTerminalHistoriesParams = match serde_json::from_value(request.params.clone())
    {
        Ok(params) => params,
        Err(error) => return failure(request.id, ErrorCode::InvalidArgument, &error.to_string()),
    };
    // Decode all element types before IO, but validate each string's ID only
    // when the shared sequential algorithm reaches it (legacy partial failure).
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::terminal::history::delete_task_terminal_histories(&context, &params.task_ids)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => {
            let code = if matches!(
                &error,
                aeroric_core::terminal::history::HistoryDeleteError::InvalidId(_)
            ) {
                ErrorCode::InvalidArgument
            } else {
                ErrorCode::IoError
            };
            failure(request.id, code, &error.to_string())
        }
        Err(error) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("terminal-history deletion worker failed: {error}"),
        ),
    }
}
async fn project_save_projects(request: Request, context: Arc<CoreContext>) -> Frame {
    if !request.params.is_object() {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.save_projects requires params object",
        );
    }
    let Some(projects_val) = request.params.get("projects").and_then(Value::as_array) else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.save_projects requires params.projects (array)",
        );
    };
    let mut records = Vec::new();
    for (i, p) in projects_val.iter().enumerate() {
        let Some(obj) = p.as_object() else {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                &format!("project at index {i} must be an object"),
            );
        };
        records.push(aeroric_core::storage::Record(obj.clone()));
    }
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::storage::save_projects(&context, &records)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(e)) => failure(request.id, ErrorCode::IoError, &e.to_string()),
        Err(e) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("project-save worker failed: {e}"),
        ),
    }
}

fn project_read_task_terminal_history(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(params) = request.params.as_object() else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.read_task_terminal_history requires params object",
        );
    };

    let task_id_val = params.get("taskId").or_else(|| params.get("task_id"));
    let Some(task_id) = task_id_val.and_then(Value::as_str) else {
        return failure(
            request.id,
            ErrorCode::InvalidArgument,
            "project.read_task_terminal_history requires params.taskId (string)",
        );
    };

    let path = match context.paths.terminal_history_file(task_id) {
        Ok(path) => path,
        Err(e) => {
            return failure(
                request.id,
                ErrorCode::InvalidArgument,
                &format!("invalid taskId {task_id:?}: {e}"),
            );
        }
    };

    // **答复必须装得进一帧。** `read_terminal_history_tail_from_path` 的上限是 8 MiB
    // （`HISTORY_MAX_BYTES`），而 `MAX_FRAME_BYTES` 是 1 MiB —— 直接把整个尾部塞进
    // `result` 会让宿主在编码时超限，`write_frame` 失败即断连，客户端只看到
    // "host closed the connection"。那正是"读得到小历史、读不到大历史"的形状，
    // 而大历史恰恰是最需要它的时候（长任务）。
    //
    // 所以窗口钳到帧预算之内：JSON 字符串还要转义（最坏 6×，控制字符）加上信封与 id，
    // 留出量级上的余量。要一次拿更多，走 `maxBytes` 分页读（与 `terminal.plan_replay`
    // 的 `maxBytes` 是同一路数），不是把单帧撑大。
    const HISTORY_REPLY_MAX_BYTES: u64 = 256 * 1024;
    let requested = request
        .params
        .get("maxBytes")
        .and_then(Value::as_u64)
        .filter(|bytes| *bytes > 0)
        .unwrap_or(HISTORY_REPLY_MAX_BYTES)
        .min(HISTORY_REPLY_MAX_BYTES);
    match aeroric_core::terminal::history::read_terminal_history_tail_from_path(&path, requested) {
        Ok(history) => ok(&request, json!(history)),
        Err(e) => failure(
            request.id,
            ErrorCode::IoError,
            &format!("cannot read terminal history for task {task_id}: {e}"),
        ),
    }
}
fn session_read_session_id(request: Request, _context: Arc<CoreContext>) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.read_session_id requires params object",
        );
    };

    let Some(session_path) = params.get("sessionPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.read_session_id requires params.sessionPath (string)",
        );
    };

    let canonical = match std::fs::canonicalize(session_path) {
        Ok(c) => c,
        Err(e) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("Cannot resolve session path: {e}"),
            )
        }
    };

    if !canonical.is_file() {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "Session path is not a regular file",
        );
    }

    let is_codex = params
        .get("isCodex")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let family_str = params
        .get("family")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());

    let session_id = match family_str.as_deref() {
        Some("dsh") => {
            aeroric_core::sessions::dsh::read_dsh_session_header(&canonical).map(|(id, _)| id)
        }
        Some("omp") => aeroric_core::sessions::omp::read_omp_session_header(&canonical),
        _ => aeroric_core::sessions::resume::resolve_session_id_from_file(&canonical, is_codex),
    };

    ok(&request, json!(session_id))
}

fn session_supports_native_resume(request: Request, _context: Arc<CoreContext>) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.session_supports_native_resume requires params object",
        );
    };

    let Some(session_path) = params.get("sessionPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.session_supports_native_resume requires params.sessionPath (string)",
        );
    };

    let canonical = match std::fs::canonicalize(session_path) {
        Ok(c) => c,
        Err(e) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("Cannot resolve session path: {e}"),
            )
        }
    };

    if !canonical.is_file() {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "Session path is not a regular file",
        );
    }

    let is_codex = params
        .get("isCodex")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let family_str = params
        .get("family")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());

    let is_codex_effective = is_codex || family_str.as_deref() == Some("codex");

    match aeroric_core::sessions::resume::session_file_supports_native_resume(
        &canonical,
        is_codex_effective,
    ) {
        Ok(supported) => ok(&request, json!(supported)),
        Err(e) => failure(id, ErrorCode::IoError, &e),
    }
}

/// `session.read_session_message_page {sessionPath, projectPath, isCodex, family, cursor}`：
/// 字节游标反向分页读取（P8-05 slice 58）。与 `session.read_session_id` 同闸——
/// canonicalize ＋ regular-file 存在性校验（slice 25 口径）；Tauri 侧的允许根
/// 校验（`validate_session_path_for`）留在旧客户端，宿主不扫描任何 home。
/// family 分支与宿主命令体逐字镜像：dsh 压缩 transcript 一次解全、首页后不再
/// 重复返回；明文 transcript 走 core 分页（页上限 1 000 行）。
fn session_read_session_message_page(request: Request, _context: Arc<CoreContext>) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.read_session_message_page requires params object",
        );
    };

    let Some(session_path) = params.get("sessionPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.read_session_message_page requires params.sessionPath (string)",
        );
    };
    if params.get("projectPath").and_then(Value::as_str).is_none() {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.read_session_message_page requires params.projectPath (string)",
        );
    }

    let canonical = match std::fs::canonicalize(session_path) {
        Ok(c) => c,
        Err(e) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("Cannot resolve session path: {e}"),
            )
        }
    };
    if !canonical.is_file() {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "Session path is not a regular file",
        );
    }

    let is_codex = params
        .get("isCodex")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let family_str = params
        .get("family")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());
    let cursor = params.get("cursor").and_then(Value::as_u64);

    let page = match family_str.as_deref() {
        Some("dsh") => {
            // 压缩 transcript 没有“字节偏移 ↔ 行”映射：一次解出全部行。
            // 首页之后不再重复返回同一份内容，否则前端分页循环不会停。
            if let Some(lines) = match read_compressed_dsh_lines(&canonical) {
                Ok(lines) => lines,
                Err(e) => {
                    return failure(
                        id,
                        ErrorCode::IoError,
                        &format!("Cannot read session transcript: {e}"),
                    )
                }
            } {
                let messages = if cursor.is_some() {
                    Vec::new()
                } else {
                    match parse_dsh_session_lines(&lines) {
                        Ok(messages) => messages,
                        Err(e) => {
                            return failure(
                                id,
                                ErrorCode::InvalidArgument,
                                &format!("Cannot parse dsh session: {e}"),
                            )
                        }
                    }
                };
                SessionMessagePage {
                    messages,
                    next_cursor: None,
                    has_more: false,
                }
            } else {
                match read_session_page_lines(&canonical, cursor, SESSION_MESSAGE_PAGE_LINES) {
                    Ok((lines, next_cursor, has_more)) => match parse_dsh_session_lines(&lines) {
                        Ok(messages) => SessionMessagePage {
                            messages,
                            next_cursor,
                            has_more,
                        },
                        Err(e) => {
                            return failure(
                                id,
                                ErrorCode::InvalidArgument,
                                &format!("Cannot parse dsh session: {e}"),
                            )
                        }
                    },
                    Err(e) => {
                        return failure(
                            id,
                            ErrorCode::IoError,
                            &format!("Cannot read session: {e}"),
                        )
                    }
                }
            }
        }
        Some("omp") => {
            match read_session_page_lines(&canonical, cursor, SESSION_MESSAGE_PAGE_LINES) {
                Ok((lines, next_cursor, has_more)) => match parse_omp_session_lines(&lines) {
                    Ok(messages) => SessionMessagePage {
                        messages,
                        next_cursor,
                        has_more,
                    },
                    Err(e) => {
                        return failure(
                            id,
                            ErrorCode::InvalidArgument,
                            &format!("Cannot parse omp session: {e}"),
                        )
                    }
                },
                Err(e) => {
                    return failure(id, ErrorCode::IoError, &format!("Cannot read session: {e}"))
                }
            }
        }
        _ => match read_session_page_lines(&canonical, cursor, SESSION_MESSAGE_PAGE_LINES) {
            Ok((lines, next_cursor, has_more)) => SessionMessagePage {
                messages: parse_session_messages_with_format(&lines, is_codex, is_codex),
                next_cursor,
                has_more,
            },
            Err(e) => return failure(id, ErrorCode::IoError, &format!("Cannot read session: {e}")),
        },
    };

    match serde_json::to_value(page) {
        Ok(value) => ok(&request, value),
        Err(e) => failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("Cannot serialize session page: {e}"),
        ),
    }
}

const UNSUPPORTED_WORKTREE_TARGET: &str =
    "Worktree target is not hosted: SSH/WSL worktrees are unavailable";

async fn git_worktree_diff_stats(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.worktree_diff_stats requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.worktree_diff_stats requires params.projectPath (string)",
        );
    };
    let Some(worktree_path) = params.get("worktreePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.worktree_diff_stats requires params.worktreePath (string)",
        );
    };
    let Some(base_branch) = params.get("baseBranch").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.worktree_diff_stats requires params.baseBranch (string)",
        );
    };
    if let Some(error) = crate::local_targets::local_target_error(
        &request,
        project_path,
        UNSUPPORTED_WORKTREE_TARGET,
    ) {
        return error;
    }
    let p_path = project_path.to_string();
    let wt_path = worktree_path.to_string();
    let b_branch = base_branch.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let svc = aeroric_core::git::worktree::WorktreeService::new();
        svc.diff_stats(std::path::Path::new(&p_path), &wt_path, &b_branch)
    })
    .await;
    match result {
        Ok(Ok(stats)) => ok(&request, serde_json::to_value(stats).unwrap_or(Value::Null)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_remove_task_worktree(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.remove_task_worktree requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.remove_task_worktree requires params.projectPath (string)",
        );
    };
    let Some(worktree_path) = params.get("worktreePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.remove_task_worktree requires params.worktreePath (string)",
        );
    };
    let Some(branch) = params.get("branch").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.remove_task_worktree requires params.branch (string)",
        );
    };
    if let Some(error) = crate::local_targets::local_target_error(
        &request,
        project_path,
        UNSUPPORTED_WORKTREE_TARGET,
    ) {
        return error;
    }
    let p_path = project_path.to_string();
    let wt_path = worktree_path.to_string();
    let br = branch.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let svc = aeroric_core::git::worktree::WorktreeService::new();
        svc.remove(std::path::Path::new(&p_path), &wt_path, &br)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_create_task_worktree(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.create_task_worktree requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.create_task_worktree requires params.projectPath (string)",
        );
    };
    let Some(task_id) = params.get("taskId").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.create_task_worktree requires params.taskId (string)",
        );
    };
    let Some(base_branch) = params.get("baseBranch").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.create_task_worktree requires params.baseBranch (string)",
        );
    };
    if let Some(error) = crate::local_targets::local_target_error(
        &request,
        project_path,
        UNSUPPORTED_WORKTREE_TARGET,
    ) {
        return error;
    }
    let p_path = project_path.to_string();
    let t_id = task_id.to_string();
    let b_branch = base_branch.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let svc = aeroric_core::git::worktree::WorktreeService::new();
        svc.create(std::path::Path::new(&p_path), &t_id, &b_branch)
    })
    .await;
    match result {
        Ok(Ok(created)) => ok(
            &request,
            serde_json::to_value(created).unwrap_or(Value::Null),
        ),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_merge_task_worktree(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.merge_task_worktree requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.merge_task_worktree requires params.projectPath (string)",
        );
    };
    let Some(worktree_path) = params.get("worktreePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.merge_task_worktree requires params.worktreePath (string)",
        );
    };
    let Some(branch) = params.get("branch").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.merge_task_worktree requires params.branch (string)",
        );
    };
    let Some(base_branch) = params.get("baseBranch").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.merge_task_worktree requires params.baseBranch (string)",
        );
    };
    if let Some(error) = crate::local_targets::local_target_error(
        &request,
        project_path,
        UNSUPPORTED_WORKTREE_TARGET,
    ) {
        return error;
    }
    let p_path = project_path.to_string();
    let wt_path = worktree_path.to_string();
    let br = branch.to_string();
    let b_branch = base_branch.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let svc = aeroric_core::git::worktree::WorktreeService::new();
        svc.merge(std::path::Path::new(&p_path), &wt_path, &br, &b_branch)
    })
    .await;
    match result {
        Ok(Ok(message)) => ok(&request, Value::String(message)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

const UNSUPPORTED_GIT_TARGET: &str =
    "Git target is not hosted: SSH/WSL git operations are unavailable";

async fn git_git_status(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_status requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_status requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::status(std::path::Path::new(&p_path))
    })
    .await;
    match result {
        Ok(Ok(changes)) => ok(
            &request,
            serde_json::to_value(changes).unwrap_or(Value::Null),
        ),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_list_branches(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_list_branches requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_list_branches requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::list_branches(std::path::Path::new(&p_path))
    })
    .await;
    match result {
        Ok(Ok(branches)) => ok(
            &request,
            serde_json::to_value(branches).unwrap_or(Value::Null),
        ),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_git_log(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_log requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_log requires params.projectPath (string)",
        );
    };
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(50);
    let search = params.get("search").and_then(Value::as_str);
    let branch = params.get("branch").and_then(Value::as_str);
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let s_search = search.map(str::to_string);
    let b_branch = branch.map(str::to_string);
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::log(
            std::path::Path::new(&p_path),
            limit,
            s_search.as_deref(),
            b_branch.as_deref(),
        )
    })
    .await;
    match result {
        Ok(Ok(commits)) => ok(
            &request,
            serde_json::to_value(commits).unwrap_or(Value::Null),
        ),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_show_diff(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_show_diff requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_show_diff requires params.projectPath (string)",
        );
    };
    let Some(commit_hash) = params.get("commitHash").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_show_diff requires params.commitHash (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let c_hash = commit_hash.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::show_diff(std::path::Path::new(&p_path), &c_hash)
    })
    .await;
    match result {
        Ok(Ok(diff)) => ok(&request, Value::String(diff)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_stage(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stage requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stage requires params.projectPath (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stage requires params.filePath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::stage(std::path::Path::new(&p_path), &f_path, &locks)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_unstage(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_unstage requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_unstage requires params.projectPath (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_unstage requires params.filePath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::unstage(std::path::Path::new(&p_path), &f_path, &locks)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_commit(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_commit requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_commit requires params.projectPath (string)",
        );
    };
    let Some(message) = params.get("message").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_commit requires params.message (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let msg = message.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::commit(std::path::Path::new(&p_path), &msg, &locks)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_stage_files(request: Request) -> Frame {
    stage_unstage_files_request(request, "git.git_stage_files", |root, file_paths, locks| {
        aeroric_core::git::service::stage_files(root, &file_paths, locks)
    })
    .await
}

async fn git_unstage_files(request: Request) -> Frame {
    stage_unstage_files_request(
        request,
        "git.git_unstage_files",
        |root, file_paths, locks| {
            aeroric_core::git::service::unstage_files(root, &file_paths, locks)
        },
    )
    .await
}

async fn git_file_diff(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_file_diff requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_file_diff requires params.projectPath (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_file_diff requires params.filePath (string)",
        );
    };
    let Some(staged) = params.get("staged").and_then(Value::as_bool) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_file_diff requires params.staged (boolean)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::file_diff(std::path::Path::new(&p_path), &f_path, staged)
    })
    .await;
    match result {
        Ok(Ok(diff)) => ok(&request, Value::String(diff)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_show_file_diff(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_show_file_diff requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_show_file_diff requires params.projectPath (string)",
        );
    };
    let Some(commit_hash) = params.get("commitHash").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_show_file_diff requires params.commitHash (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_show_file_diff requires params.filePath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let c_hash = commit_hash.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::show_file_diff(std::path::Path::new(&p_path), &c_hash, &f_path)
    })
    .await;
    match result {
        Ok(Ok(content)) => ok(&request, Value::String(content)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_commit_detail(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_commit_detail requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_commit_detail requires params.projectPath (string)",
        );
    };
    let Some(commit_hash) = params.get("commitHash").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_commit_detail requires params.commitHash (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let c_hash = commit_hash.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::commit_detail_result(std::path::Path::new(&p_path), &c_hash)
    })
    .await;
    match result {
        Ok(Ok(detail)) => match serde_json::to_value(detail) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
        },
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_discard_file(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_file requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_file requires params.projectPath (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_file requires params.filePath (string)",
        );
    };
    let Some(untracked) = params.get("untracked").and_then(Value::as_bool) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_file requires params.untracked (boolean)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::discard_file(
            std::path::Path::new(&p_path),
            &f_path,
            untracked,
            &locks,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_discard_files(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_files requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_files requires params.projectPath (string)",
        );
    };
    let Some(untracked) = params.get("untracked").and_then(Value::as_bool) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_files requires params.untracked (boolean)",
        );
    };
    let Some(file_paths) = params.get("filePaths").and_then(Value::as_array) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_files requires params.filePaths (array of strings)",
        );
    };
    let mut paths = Vec::with_capacity(file_paths.len());
    for value in file_paths {
        let Some(path) = value.as_str() else {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_discard_files requires params.filePaths (array of strings)",
            );
        };
        paths.push(path.to_string());
    }
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::discard_files_at(
            std::path::Path::new(&p_path),
            &paths,
            untracked,
            &locks,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_discard_all(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_all requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_discard_all requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::discard_all(std::path::Path::new(&p_path), &locks)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_conflict_preview(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_conflict_preview requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_conflict_preview requires params.projectPath (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_conflict_preview requires params.filePath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::conflict_preview_result(std::path::Path::new(&p_path), &f_path)
    })
    .await;
    match result {
        Ok(Ok(preview)) => match serde_json::to_value(preview) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
        },
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_resolve_conflict(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_resolve_conflict requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_resolve_conflict requires params.projectPath (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_resolve_conflict requires params.filePath (string)",
        );
    };
    let resolution = match params.get("resolution").and_then(Value::as_str) {
        Some("ours") => aeroric_core::git::service::GitConflictResolution::Ours,
        Some("theirs") => aeroric_core::git::service::GitConflictResolution::Theirs,
        Some("both") => aeroric_core::git::service::GitConflictResolution::Both,
        _ => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_resolve_conflict requires params.resolution (ours|theirs|both)",
            )
        }
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::resolve_conflict_with_resolution(
            std::path::Path::new(&p_path),
            &f_path,
            resolution,
            &locks,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_fetch(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_fetch requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_fetch requires params.projectPath (string)",
        );
    };
    let prune = match params.get("prune") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_fetch requires params.prune (boolean)",
            )
        }
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::fetch_result(std::path::Path::new(&p_path), prune)
    })
    .await;
    match result {
        Ok(Ok(text)) => ok(&request, Value::String(text)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_pull(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_pull requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_pull requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::pull_result(std::path::Path::new(&p_path))
    })
    .await;
    match result {
        Ok(Ok(text)) => ok(&request, Value::String(text)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_push(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_push requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_push requires params.projectPath (string)",
        );
    };
    let branch = match params.get("branch") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_push requires params.branch (string)",
            )
        }
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::push_result(std::path::Path::new(&p_path), branch.as_deref())
    })
    .await;
    match result {
        Ok(Ok(text)) => ok(&request, Value::String(text)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_clone(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_clone requires params object",
        );
    };
    let Some(url) = params.get("url").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_clone requires params.url (string)",
        );
    };
    let Some(target_path) = params.get("targetPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_clone requires params.targetPath (string)",
        );
    };
    let branch = match params.get("branch") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_clone requires params.branch (string)",
            )
        }
    };
    let u = url.to_string();
    let t = target_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::clone(&u, &t, branch.as_deref())
    })
    .await;
    match result {
        Ok(Ok(text)) => ok(&request, Value::String(text)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_generate_commit_message(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.generate_commit_message requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.generate_commit_message requires params.projectPath (string)",
        );
    };
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::generate_commit_message(std::path::Path::new(&p_path))
    })
    .await;
    match result {
        Ok(Ok(message)) => ok(&request, Value::String(message)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

/// Shared request shape for the plural stage/unstage twins (slice 82). The wire
/// shape is identical; only the core body differs — one helper, not two copies.
async fn stage_unstage_files_request<F>(request: Request, method: &str, run: F) -> Frame
where
    F: FnOnce(
            &std::path::Path,
            Vec<String>,
            &aeroric_core::git::transport::RepoLocks,
        ) -> Result<(), String>
        + Send
        + 'static,
{
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("{method} requires params object"),
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("{method} requires params.projectPath (string)"),
        );
    };
    let Some(file_paths) = params.get("filePaths").and_then(Value::as_array) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("{method} requires params.filePaths (array of strings)"),
        );
    };
    let mut paths = Vec::with_capacity(file_paths.len());
    for value in file_paths {
        let Some(path) = value.as_str() else {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                &format!("{method} requires params.filePaths (array of strings)"),
            );
        };
        paths.push(path.to_string());
    }
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        run(std::path::Path::new(&p_path), paths, &locks)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_stage_all(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stage_all requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stage_all requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::stage_all(std::path::Path::new(&p_path), &locks)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_stash_list(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stash_list requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stash_list requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::stash_list(std::path::Path::new(&p_path))
    })
    .await;
    match result {
        Ok(Ok(stashes)) => ok(
            &request,
            serde_json::to_value(stashes).unwrap_or(Value::Null),
        ),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_stash_diff(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stash_diff requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stash_diff requires params.projectPath (string)",
        );
    };
    let Some(stash_ref) = params.get("stashRef").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stash_diff requires params.stashRef (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let s_ref = stash_ref.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::stash_diff(std::path::Path::new(&p_path), &s_ref)
    })
    .await;
    match result {
        Ok(Ok(diff)) => match serde_json::to_value(diff) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
        },
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_stash_push(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stash_push requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_stash_push requires params.projectPath (string)",
        );
    };
    let message = match params.get("message") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_stash_push requires params.message (string)",
            )
        }
    };
    let include_untracked = match params.get("includeUntracked") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_stash_push requires params.includeUntracked (boolean)",
            )
        }
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::stash_push(
            std::path::Path::new(&p_path),
            message.as_deref(),
            include_untracked,
            &locks,
        )
    })
    .await;
    match result {
        Ok(Ok(text)) => ok(&request, Value::String(text)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_branch_graph(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_branch_graph requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_branch_graph requires params.projectPath (string)",
        );
    };
    let limit = match params.get("limit") {
        None | Some(Value::Null) => None,
        Some(Value::Number(value)) => match value.as_u64() {
            Some(value) => Some(value as u32),
            None => {
                return failure(
                    id,
                    ErrorCode::InvalidArgument,
                    "git.git_branch_graph requires params.limit (non-negative integer)",
                )
            }
        },
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_branch_graph requires params.limit (number)",
            )
        }
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::branch_graph_result(std::path::Path::new(&p_path), limit)
    })
    .await;
    match result {
        Ok(Ok(graph)) => match serde_json::to_value(graph) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
        },
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_blame_file(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_blame_file requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_blame_file requires params.projectPath (string)",
        );
    };
    let Some(file_path) = params.get("filePath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_blame_file requires params.filePath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let f_path = file_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::blame_file_result(std::path::Path::new(&p_path), &f_path)
    })
    .await;
    match result {
        Ok(Ok(blame)) => match serde_json::to_value(blame) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
        },
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_remote_counts(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_remote_counts requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_remote_counts requires params.projectPath (string)",
        );
    };
    let branch = match params.get("branch") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_remote_counts requires params.branch (string)",
            )
        }
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::remote_counts(std::path::Path::new(&p_path), branch.as_deref())
    })
    .await;
    match result {
        Ok(Ok(counts)) => match serde_json::to_value(counts) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
        },
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_create_branch(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_create_branch requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_create_branch requires params.projectPath (string)",
        );
    };
    let Some(branch_name) = params.get("branchName").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_create_branch requires params.branchName (string)",
        );
    };
    let Some(from_branch) = params.get("fromBranch").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_create_branch requires params.fromBranch (string)",
        );
    };
    let Some(checkout) = params.get("checkout").and_then(Value::as_bool) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_create_branch requires params.checkout (boolean)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let b_name = branch_name.to_string();
    let f_branch = from_branch.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::create_branch_at(
            std::path::Path::new(&p_path),
            &b_name,
            &f_branch,
            checkout,
            &locks,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_checkout_branch(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_checkout_branch requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_checkout_branch requires params.projectPath (string)",
        );
    };
    let Some(branch_name) = params.get("branchName").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_checkout_branch requires params.branchName (string)",
        );
    };
    let Some(is_remote) = params.get("isRemote").and_then(Value::as_bool) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_checkout_branch requires params.isRemote (boolean)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let b_name = branch_name.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::checkout_branch_tracking(
            std::path::Path::new(&p_path),
            &b_name,
            is_remote,
            &locks,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

/// `git.git_stash_apply` 与 `git.git_stash_drop` 共用同一请求外形（projectPath＋stashRef），
/// 只差 core 调用，所以走一个参数化助手——两份复制体迟早会漂移。
async fn stash_ref_request<F>(request: &Request, method: &str, run: F) -> Frame
where
    F: FnOnce(
            &std::path::Path,
            &str,
            &aeroric_core::git::transport::RepoLocks,
        ) -> Result<String, String>
        + Send
        + 'static,
{
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("{method} requires params object"),
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("{method} requires params.projectPath (string)"),
        );
    };
    let Some(stash_ref) = params.get("stashRef").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("{method} requires params.stashRef (string)"),
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let s_ref = stash_ref.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        run(std::path::Path::new(&p_path), &s_ref, &locks)
    })
    .await;
    match result {
        Ok(Ok(text)) => ok(request, Value::String(text)),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_stash_apply(request: Request) -> Frame {
    stash_ref_request(&request, "git.git_stash_apply", |root, stash_ref, locks| {
        aeroric_core::git::service::stash_apply(root, stash_ref, locks)
    })
    .await
}

async fn git_stash_drop(request: Request) -> Frame {
    stash_ref_request(&request, "git.git_stash_drop", |root, stash_ref, locks| {
        aeroric_core::git::service::stash_drop(root, stash_ref, locks)
    })
    .await
}

async fn git_init(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_init requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_init requires params.projectPath (string)",
        );
    };
    let initial_branch = match params.get("initialBranch") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return failure(
                id,
                ErrorCode::InvalidArgument,
                "git.git_init requires params.initialBranch (string)",
            )
        }
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::init(
            std::path::Path::new(&p_path),
            initial_branch.as_deref(),
            &locks,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_conflict_files(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_conflict_files requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_conflict_files requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        aeroric_core::git::service::conflict_files(std::path::Path::new(&p_path))
    })
    .await;
    match result {
        Ok(Ok(files)) => match serde_json::to_value(files) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
        },
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

async fn git_unstage_all(request: Request) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_unstage_all requires params object",
        );
    };
    let Some(project_path) = params.get("projectPath").and_then(Value::as_str) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "git.git_unstage_all requires params.projectPath (string)",
        );
    };
    if let Some(error) =
        crate::local_targets::local_target_error(&request, project_path, UNSUPPORTED_GIT_TARGET)
    {
        return error;
    }
    let p_path = project_path.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let locks = aeroric_core::git::transport::RepoLocks::default();
        aeroric_core::git::service::unstage_all(std::path::Path::new(&p_path), &locks)
    })
    .await;
    match result {
        Ok(Ok(())) => ok(&request, Value::Null),
        Ok(Err(error)) => failure(id, ErrorCode::IoError, &error),
        Err(join_err) => failure(id, ErrorCode::IoError, &join_err.to_string()),
    }
}

// ── 执行服务（b1.2：task.run / task.cancel / task.complete）──────────────

use aeroric_core::tasks::Attachments;
use aeroric_core::tasks::{executor::HostWindowFacts, NewTask, TaskService};

/// 找到任务所在的 project record（与 `task.get` 同一条扫描链——**归属判定
/// 只此一份逻辑**，两处扫描将来要合并成一个 helper）。
pub(crate) fn find_project_of_task(
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
    task_id: &str,
) -> Option<aeroric_core::storage::Record> {
    let projects = ProjectService::new(std::sync::Arc::clone(context))
        .list()
        .ok()?;
    for project in projects {
        let Some(id) = mutations::id_of(&project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(context, id) else {
            continue;
        };
        if stamped
            .records
            .iter()
            .any(|record| mutations::text(record, "id") == Some(task_id))
        {
            return Some(project);
        }
    }
    None
}

pub fn register_task_services(
    set: &mut ServiceSet,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    executor: std::sync::Arc<crate::executor::CliPtyExecutor>,
    terminal: std::sync::Arc<aeroric_core::terminal::pty::TerminalService>,
    registry: std::sync::Arc<aeroric_core::runtime::registry::TaskRegistry>,
) {
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.run",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move {
                    let id = request.id.clone();
                    match tokio::task::spawn_blocking(move || task_run(request, service, context))
                        .await
                    {
                        Ok(frame) => frame,
                        Err(_) => failure(
                            id,
                            ErrorCode::IoError,
                            "task.run worker failed; acceptance unknown",
                        ),
                    }
                })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.cancel",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_settle_verdict(request, service, context, false) })
            }),
        );
    }
    {
        let terminal = std::sync::Arc::clone(&terminal);
        let registry = std::sync::Arc::clone(&registry);
        set.register(
            "task.output",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                let registry = std::sync::Arc::clone(&registry);
                Box::pin(async move { task_output(request, &terminal, &registry) })
            }),
        );
    }
    {
        let executor = std::sync::Arc::clone(&executor);
        set.register(
            "task.resize",
            std::sync::Arc::new(move |request, _call| {
                let executor = std::sync::Arc::clone(&executor);
                Box::pin(async move { task_resize(request, &executor) })
            }),
        );
    }
    {
        let executor = std::sync::Arc::clone(&executor);
        set.register(
            "task.input",
            std::sync::Arc::new(move |request, _call| {
                let executor = std::sync::Arc::clone(&executor);
                Box::pin(async move { task_input(request, &executor) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.complete",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_settle_verdict(request, service, context, true) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.resume",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_resume(request, service, context) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "task.archive",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { task_archive(request, service, context) })
            }),
        );
    }
    {
        let registry = std::sync::Arc::clone(&registry);
        set.register(
            "terminal.get_active_task_ids",
            std::sync::Arc::new(move |request, _call| {
                let registry = std::sync::Arc::clone(&registry);
                Box::pin(async move { terminal_active_task_ids(request, &registry) })
            }),
        );
    }
}

fn terminal_active_task_ids(
    request: Request,
    registry: &std::sync::Arc<aeroric_core::runtime::registry::TaskRegistry>,
) -> Frame {
    ok(&request, json!(registry.live_task_ids()))
}

fn terminal_validate_agent_launch(
    request: Request,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let Some(params) = request.params.as_object() else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.validate_agent_launch requires params object",
        );
    };
    let Some(agent) = params
        .get("agent")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.validate_agent_launch requires params.agent (non-empty string)",
        );
    };
    let Some(project_path) = params
        .get("projectPath")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.validate_agent_launch requires params.projectPath (non-empty string)",
        );
    };
    let project_dir = std::path::Path::new(project_path);
    if !project_dir.is_dir() {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("projectPath does not exist or is not a directory: {project_path}"),
        );
    }

    let raw_settings = match aeroric_core::storage::settings::load_settings(&context) {
        Ok(outcome) => outcome.found().unwrap_or_default(),
        Err(error) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("cannot load settings: {error}"),
            );
        }
    };
    let settings: aeroric_core::settings::schema::AppSettings =
        serde_json::from_value(Value::Object(raw_settings)).unwrap_or_default();
    let launch = aeroric_core::settings::get_agent_launch_spec_from_settings(&settings, agent);
    let cwd = aeroric_core::terminal::launch_validation::agent_process_cwd(
        project_path,
        launch.codex_like,
    );
    match aeroric_core::terminal::launch_validation::preflight_agent_launch_spec(
        &launch,
        Some(&cwd),
        aeroric_core::terminal::launch_validation::AGENT_LAUNCH_PREFLIGHT_TIMEOUT,
    ) {
        Ok(()) => ok(&request, Value::Null),
        Err(err) => {
            let code = if launch.family == aeroric_core::settings::schema::AgentFamily::Dsh
                && err.contains("DeepSeek Harness is not installed")
            {
                ErrorCode::NotFound
            } else {
                ErrorCode::IoError
            };
            failure(id, code, &err)
        }
    }
}

fn task_run(
    request: Request,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    if crate::submission_services::is_versioned(&request.params) {
        return crate::submission_services::run(request, service);
    }
    let id = request.id.clone();
    let Some(path) = request
        .params
        .get("projectPath")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.run requires params.projectPath",
        );
    };
    let Some(agent) = request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.run requires params.agent",
        );
    };
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.run requires params.taskId (the caller mints it; ADR-013)",
        );
    };
    let prompt = request
        .params
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let new_project_id = request
        .params
        .get("newProjectId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("project");
    let projects = ProjectService::new(std::sync::Arc::clone(&context));
    let (upserted, _applied) =
        match projects.open_local(path, new_project_id, context.clock.now_ms() as i64) {
            Ok(opened) => opened,
            Err(e) => {
                return failure(
                    id,
                    ErrorCode::IoError,
                    &format!("cannot open the project: {e}"),
                )
            }
        };
    let project = upserted.project;
    let project_id = mutations::id_of(&project).unwrap_or_default().to_string();
    let permission_mode = request
        .params
        .get("permissionMode")
        .and_then(Value::as_str)
        .unwrap_or("ask")
        .to_string();
    let new_task = NewTask {
        id: task_id.to_string(),
        project_id: project_id.clone(),
        prompt,
        agent: agent.to_string(),
        permission_mode,
        immediate: true,
        selected_model: request
            .params
            .get("selectedModel")
            .and_then(Value::as_str)
            .map(str::to_string),
        reasoning_effort: request
            .params
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::to_string),
        speed: None,
        dsh_agent_preset: None,
        agent_config_id: None,
        images: Vec::new(),
        texts: Vec::new(),
    };
    if let Err(e) = service.create(&project, new_task) {
        return failure(
            id,
            ErrorCode::IoError,
            &format!("cannot create the task: {e}"),
        );
    }
    let terminal_size = request
        .params
        .get("terminalSize")
        .and_then(Value::as_array)
        .and_then(|pair| {
            Some((
                pair.first()?.as_u64()? as u16,
                pair.get(1)?.as_u64()? as u16,
            ))
        });
    let operation_key = format!("cli-run-{task_id}");
    match service.start(
        &project,
        task_id,
        &operation_key,
        true,
        None,
        false,
        Attachments {
            images: Vec::new(),
            texts: Vec::new(),
        },
        HostWindowFacts {
            terminal_size,
            dsh: None,
        },
    ) {
        Ok(applied) => {
            let status = applied
                .records
                .iter()
                .find(|record| mutations::text(record, "id") == Some(task_id))
                .and_then(|record| mutations::text(record, "status"))
                .unwrap_or("unknown")
                .to_string();
            ok(
                &request,
                json!({ "taskId": task_id, "projectId": project_id, "status": status }),
            )
        }
        Err(e) => failure(
            id,
            ErrorCode::IoError,
            &format!("cannot start the task: {e}"),
        ),
    }
}

/// `task.input {taskId, text}`：PTY 输入经控制租约（executor 的短顶替写）——
/// **结构化 Agent 的声明能力路径（dsh prompt 注入等）不在此方法**：dsh 任务
/// 的后续输入走宿主 `prompt_dsh_task` 的专用口，而 CLI host 不接 dsh 启动
/// （⑦ 收尾定性，见 executor 模块头）；对不认识的协议猜按键是 P3-07 验收行
/// 点名禁止的。
fn task_input(request: Request, executor: &crate::executor::CliPtyExecutor) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.input requires params.taskId",
        );
    };
    // `bytes`（base64）优先于 `text`：attach 的任意按键序列（Alt、NUL、Latin-1）
    // 经 base64 无损；`text` 留给脚本化调用的可读性。
    use base64::Engine as _;
    let payload: Vec<u8> = if let Some(b64) = request.params.get("bytes").and_then(Value::as_str) {
        match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(bytes) => bytes,
            Err(e) => return failure(id, ErrorCode::InvalidArgument, &format!("bad base64: {e}")),
        }
    } else if let Some(text) = request.params.get("text").and_then(Value::as_str) {
        text.as_bytes().to_vec()
    } else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.input requires params.bytes (base64) or params.text",
        );
    };
    let written = payload.len();
    match executor.input(task_id, &payload) {
        Ok(()) => ok(&request, json!({ "written": written })),
        Err(e) => failure(id, ErrorCode::NotFound, &e),
    }
}

/// `task.output {taskId, offset}`：字节环的 RPC 读面（attach 的转发源）。
/// `gap:true` = 流已退场（进程结束后环不在），客户端改走 `session.history`
/// 拉全量——**环是直播面，不是历史面**（P3-03 的分界在 wire 上的投影）。
/// `OutOfRange` 是「追平」（返回空 chunk + 当前末端），不是错误。
fn task_output(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
    registry: &aeroric_core::runtime::registry::TaskRegistry,
) -> Frame {
    use aeroric_core::terminal::stream::{StreamError, TerminalIdentity};
    use base64::Engine as _;
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.output requires params.taskId",
        );
    };
    let offset = request
        .params
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let Some(run) = registry.resource_run(task_id) else {
        return ok(
            &request,
            json!({ "gap": true, "reason": "no live run on this host; use session.history" }),
        );
    };
    let identity = TerminalIdentity::new(task_id, run.generation());
    match terminal.read_from(&identity, offset, 64 * 1024) {
        Ok(chunk) => ok(
            &request,
            json!({
                "bytes": base64::engine::general_purpose::STANDARD.encode(&chunk.bytes),
                "nextOffset": offset + chunk.bytes.len() as u64,
                "gap": false,
            }),
        ),
        Err(StreamError::OutOfRange { next_offset }) => ok(
            &request,
            json!({ "bytes": "", "nextOffset": next_offset.min(offset), "gap": false }),
        ),
        Err(StreamError::UnknownStream) => ok(
            &request,
            json!({ "gap": true, "reason": "stream retired; use session.history" }),
        ),
    }
}

fn task_resize(request: Request, executor: &crate::executor::CliPtyExecutor) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.resize requires params.taskId",
        );
    };
    let (Some(cols), Some(rows)) = (
        request.params.get("cols").and_then(Value::as_u64),
        request.params.get("rows").and_then(Value::as_u64),
    ) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.resize requires params.cols and params.rows (numbers)",
        );
    };
    match executor.resize(task_id, cols as u16, rows as u16) {
        Ok(()) => ok(&request, json!({ "cols": cols, "rows": rows })),
        Err(e) => failure(id, ErrorCode::NotFound, &e),
    }
}

fn task_settle_verdict(
    request: Request,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    complete: bool,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "this method requires params.taskId",
        );
    };
    let Some(project) = find_project_of_task(&context, task_id) else {
        return failure(
            id,
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let outcome = if complete {
        service
            .complete(&project, task_id)
            .map(|(completion, _)| json!({ "completion": format!("{completion:?}") }))
    } else {
        service
            .cancel(&project, task_id)
            .map(|target| json!({ "target": target.target }))
    };
    match outcome {
        Ok(result) => ok(&request, result),
        Err(e) => failure(
            id,
            ErrorCode::IoError,
            &format!("the settle verdict failed: {e}"),
        ),
    }
}

/// `task.archive {taskId, archived?}`: puts a finished task in (or takes it out of) the
/// archive. The row keeps its `archivedAt`; running and pending tasks are left as they are.
fn task_archive(
    request: Request,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.archive requires params.taskId",
        );
    };
    let archived = request
        .params
        .get("archived")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let Some(project) = find_project_of_task(&context, task_id) else {
        return failure(
            id,
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let Some(project_id) = mutations::id_of(&project) else {
        return failure(id, ErrorCode::NotFound, "the task's project has no id");
    };
    let ids = [task_id.to_string()];
    let outcome = if archived {
        service.archive(project_id, &ids)
    } else {
        service.unarchive(project_id, &ids)
    };
    match outcome {
        Ok(applied) => ok(&request, json!({ "changed": applied.changed })),
        Err(e) => failure(id, ErrorCode::IoError, &format!("task.archive failed: {e}")),
    }
}

fn task_resume(
    request: Request,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "task.resume requires params.taskId",
        );
    };
    let Some(project) = find_project_of_task(&context, task_id) else {
        return failure(
            id,
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let terminal_size = request
        .params
        .get("terminalSize")
        .and_then(Value::as_array)
        .and_then(|pair| {
            Some((
                pair.first()?.as_u64()? as u16,
                pair.get(1)?.as_u64()? as u16,
            ))
        });
    let session_id = request
        .params
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            let tasks = service.load(project.id().unwrap_or_default()).ok()?.records;
            let record = tasks
                .iter()
                .find(|r| mutations::text(r, "id") == Some(task_id))?;
            let (session_id, session_path) =
                aeroric_core::tasks::session_owner::current_session(record);
            session_id.or(session_path)
        })
        .unwrap_or_else(|| "default-session".to_string());
    let operation_key = format!("cli-resume-{task_id}");
    match service.resume(
        &project,
        task_id,
        &session_id,
        &operation_key,
        true,
        terminal_size,
    ) {
        Ok(applied) => {
            let status = applied
                .records
                .iter()
                .find(|record| mutations::text(record, "id") == Some(task_id))
                .and_then(|record| mutations::text(record, "status"))
                .unwrap_or("running")
                .to_string();
            ok(
                &request,
                json!({
                    "taskId": task_id,
                    "projectId": project.id(),
                    "sessionId": session_id,
                    "status": status,
                }),
            )
        }
        Err(e) => failure(
            id,
            ErrorCode::IoError,
            &format!("cannot resume the task: {e}"),
        ),
    }
}

use aeroric_core::agents::dsh_host::approvals::{
    take_waterfall_answer, PendingWaterfall, WaterfallKind, WaterfallRegistry,
};

#[derive(Debug, Default)]
pub struct HostApprovals {
    pub registry: WaterfallRegistry,
    pub resolved_events: std::collections::HashSet<String>,
}

pub fn register_approval_services(
    set: &mut ServiceSet,
    approvals: std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) {
    {
        let approvals = std::sync::Arc::clone(&approvals);
        set.register(
            "approval.list",
            std::sync::Arc::new(move |request, _call| {
                let approvals = std::sync::Arc::clone(&approvals);
                Box::pin(async move { approval_list(request, &approvals) })
            }),
        );
    }
    {
        let approvals = std::sync::Arc::clone(&approvals);
        set.register(
            "approval.respond",
            std::sync::Arc::new(move |request, _call| {
                let approvals = std::sync::Arc::clone(&approvals);
                Box::pin(async move { approval_respond(request, &approvals) })
            }),
        );
    }
    {
        let approvals = std::sync::Arc::clone(&approvals);
        set.register(
            "approval.test_request",
            std::sync::Arc::new(move |request, _call| {
                let approvals = std::sync::Arc::clone(&approvals);
                Box::pin(async move { approval_test_request(request, &approvals) })
            }),
        );
    }
}

fn approval_list(
    request: Request,
    approvals: &std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) -> Frame {
    let guard = approvals.lock().expect("lock approvals");
    let items: Vec<Value> = guard
        .registry
        .entries()
        .into_iter()
        .map(|(event_id, entry)| {
            json!({
                "eventId": event_id,
                "kind": match entry.kind {
                    WaterfallKind::Approval => "approval",
                    WaterfallKind::Question => "question",
                },
                "sessionId": entry.session_id,
            })
        })
        .collect();
    ok(&request, json!({ "approvals": items }))
}

fn approval_respond(
    request: Request,
    approvals: &std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) -> Frame {
    let id = request.id.clone();
    let Some(event_id) = request
        .params
        .get("eventId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "approval.respond requires params.eventId",
        );
    };
    let Some(outcome) = request.params.get("outcome").and_then(Value::as_object) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "approval.respond requires params.outcome (JSON object)",
        );
    };
    let outcome_val = Value::Object(outcome.clone());
    let mut guard = approvals.lock().expect("lock approvals");
    if guard.resolved_events.contains(event_id) {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("approval request '{event_id}' has already been resolved or expired"),
        );
    }
    if !guard.registry.entries().iter().any(|(k, _)| k == event_id) {
        return failure(
            id,
            ErrorCode::NotFound,
            &format!("approval request '{event_id}' not found"),
        );
    }
    let client_id = guard
        .registry
        .client_id()
        .unwrap_or("cli-default")
        .to_string();
    match take_waterfall_answer(&mut guard.registry, &client_id, event_id, &outcome_val) {
        Ok(answer) => {
            guard.resolved_events.insert(event_id.to_string());
            ok(
                &request,
                json!({
                    "accepted": true,
                    "eventId": event_id,
                    "sessionHint": answer.session_hint,
                    "args": answer.args,
                }),
            )
        }
        Err(e) => failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("cannot answer approval: {e}"),
        ),
    }
}

fn approval_test_request(
    request: Request,
    approvals: &std::sync::Arc<std::sync::Mutex<HostApprovals>>,
) -> Frame {
    let id = request.id.clone();
    let Some(event_id) = request.params.get("eventId").and_then(Value::as_str) else {
        return failure(id, ErrorCode::InvalidArgument, "eventId required");
    };
    let kind = match request.params.get("kind").and_then(Value::as_str) {
        Some("question") => WaterfallKind::Question,
        _ => WaterfallKind::Approval,
    };
    let session_id = request
        .params
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut guard = approvals.lock().expect("lock approvals");
    guard
        .registry
        .insert(event_id.to_string(), PendingWaterfall { kind, session_id });
    ok(&request, json!({ "inserted": event_id }))
}

// ── ④：session 三方法＋task.input（lease 控制的写入口） ──────────────────

use aeroric_core::sessions::{
    dsh::parse_dsh_session_lines,
    export::{export_session_markdown, ExportSource, ExportTaskMeta},
    omp::parse_omp_session_lines,
    parser::parse_session_lines,
    SessionContent, SessionMessage,
};

/// 会话消息的家族分派——与宿主 `session.rs` 同一条四族链（P3-04 的 core 读侧）。
fn parse_messages(lines: &[String], family: ProtocolFamily) -> Vec<SessionMessage> {
    match family {
        ProtocolFamily::Dsh => parse_dsh_session_lines(lines).unwrap_or_default(),
        ProtocolFamily::Omp => parse_omp_session_lines(lines).unwrap_or_default(),
        other => {
            let mut messages = Vec::new();
            parse_session_lines(lines, other == ProtocolFamily::Codex, &mut messages);
            messages
        }
    }
}

/// 解析结果 → v1 `SessionPage`（客户端按 v1 契约解码）。文本与思考是
/// `Text` 块；工具调用、结果与附件保持不透明（kind + 原样 payload），客户端
/// 按 kind 认领，宿主不推断结构。
pub(crate) fn session_page(
    session_id: String,
    family: ProtocolFamily,
    messages: Vec<SessionMessage>,
) -> SessionPage {
    let messages = messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| v1_message(index, message, family))
        .collect();
    SessionPage {
        session_id,
        family,
        messages,
        next_cursor: None,
        has_more: false,
    }
}

fn v1_message(index: usize, message: SessionMessage, family: ProtocolFamily) -> V1Message {
    let role = match message.role.as_str() {
        "user" => MessageRole::User,
        "tool" => MessageRole::Tool,
        "system" => MessageRole::System,
        _ => MessageRole::Assistant,
    };
    let opaque = |kind: &str, source_id: Option<String>, payload: Value| {
        ContentBlock::Unknown(OpaqueContent {
            kind: kind.to_string(),
            source_id,
            payload,
            origin_family: Some(family),
        })
    };
    let blocks = message
        .content
        .into_iter()
        .map(|content| match content {
            SessionContent::Text { text } => ContentBlock::Text {
                kind: "text".into(),
                text,
            },
            SessionContent::Thinking { thinking } => ContentBlock::Text {
                kind: "thinking".into(),
                text: thinking,
            },
            SessionContent::ToolUse { id, name, input } => {
                // input 是序列化后的 JSON 字符串；能解析就给结构，否则原样。
                let input = serde_json::from_str::<Value>(&input).unwrap_or(Value::String(input));
                opaque(
                    "tool_use",
                    Some(id),
                    json!({ "name": name, "input": input }),
                )
            }
            SessionContent::ToolResult { id, output } => {
                opaque("tool_result", Some(id), json!({ "output": output }))
            }
            other => opaque(
                "attachment",
                None,
                serde_json::to_value(other).unwrap_or_default(),
            ),
        })
        .collect();
    V1Message {
        id: message.message_id.unwrap_or_else(|| format!("m{index}")),
        role,
        blocks,
        created_at: None,
    }
}

use aeroric_core::tasks::ProtocolFamily;
use aeroric_protocol::session::{
    ContentBlock, MessageRole, OpaqueContent, SessionMessage as V1Message, SessionPage,
};

#[allow(clippy::type_complexity)]
/// 终端重放面与租约面（P5-01，ADR-038）：`terminal.plan_replay` 承接回放与
/// 活流（同一字节环，轮询节奏客户端自持），`terminal.acquire_control`/
/// `terminal.release_control` 把租约语义上协议。`UnknownStream` 在宿主分类成
/// `notStarted`（在册未产字节）/`evicted`（流已退场）/`unknownStream`（任务
/// 不可知）——判定材料（任务注册表、任务记录）在宿主手里，core 不改。
pub fn register_terminal_replay_services(
    set: &mut ServiceSet,
    terminal: std::sync::Arc<aeroric_core::terminal::pty::TerminalService>,
    registry: std::sync::Arc<aeroric_core::runtime::registry::TaskRegistry>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) {
    {
        let terminal = std::sync::Arc::clone(&terminal);
        let registry = std::sync::Arc::clone(&registry);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "terminal.plan_replay",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                let registry = std::sync::Arc::clone(&registry);
                let context = std::sync::Arc::clone(&context);
                Box::pin(
                    async move { terminal_plan_replay(request, &terminal, &registry, &context) },
                )
            }),
        );
    }
    {
        let terminal = std::sync::Arc::clone(&terminal);
        set.register(
            "terminal.acquire_control",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                Box::pin(async move { terminal_acquire_control(request, &terminal) })
            }),
        );
    }
    {
        let terminal = std::sync::Arc::clone(&terminal);
        set.register(
            "terminal.release_control",
            std::sync::Arc::new(move |request, _call| {
                let terminal = std::sync::Arc::clone(&terminal);
                Box::pin(async move { terminal_release_control(request, &terminal) })
            }),
        );
    }
}

fn terminal_identity_of(request: &Request) -> Option<(String, u64)> {
    let task_id = request.params.get("taskId")?.as_str()?.trim().to_string();
    if task_id.is_empty() {
        return None;
    }
    let generation = request.params.get("generation").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str()?.parse::<u64>().ok())
    })?;
    Some((task_id, generation))
}

/// 在册且未退场的运行 ⇒ 流还没产出第一字节（notStarted）；任务记录还在但
/// 流不在册 ⇒ 已被退场（evicted）；两处都查无 ⇒ 任务本身不可知（unknownStream）。
fn classify_unknown_stream(
    registry: &aeroric_core::runtime::registry::TaskRegistry,
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
    task_id: &str,
    generation: u64,
) -> &'static str {
    if registry
        .resource_run(task_id)
        .is_some_and(|run| run.id() == task_id && run.generation() == generation)
    {
        return "notStarted";
    }
    if find_project_of_task(context, task_id).is_some() {
        return "evicted";
    }
    "unknownStream"
}

fn terminal_plan_replay(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
    registry: &aeroric_core::runtime::registry::TaskRegistry,
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    use aeroric_core::terminal::replay::{plan_replay, ReplayPlan, ResetReason, ResumePoint};
    use aeroric_core::terminal::stream::TerminalIdentity;
    use base64::Engine as _;
    let id = request.id.clone();
    let Some((task_id, generation)) = terminal_identity_of(&request) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.plan_replay requires params.taskId and params.generation",
        );
    };
    let cursor = request.params.get("cursor").cloned().unwrap_or(Value::Null);
    let kind = cursor
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("bytes")
        .to_string();
    let Some(value) = cursor
        .get("value")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse::<u64>().ok()))
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.plan_replay requires cursor.value (a byte offset)",
        );
    };
    let max_bytes = request
        .params
        .get("maxBytes")
        .and_then(Value::as_u64)
        .unwrap_or(aeroric_protocol::terminal::MAX_TERMINAL_CHUNK_BYTES as u64)
        .min(aeroric_protocol::terminal::MAX_TERMINAL_CHUNK_BYTES as u64)
        as usize;
    let identity = TerminalIdentity::new(&task_id, generation);
    let resume = match kind.as_str() {
        "unitsFromStart" => ResumePoint::UnitsFromStart(value),
        _ => ResumePoint::Bytes(value),
    };
    let plan = plan_replay(terminal, &identity, resume, max_bytes);
    let (kind_out, from, bytes, reason) = match &plan {
        ReplayPlan::Seamless { offset, bytes } => ("seamless", *offset, bytes, Value::Null),
        ReplayPlan::Reset {
            from,
            bytes,
            reason,
        } => {
            let reason_json = match reason {
                ResetReason::Compacted { earliest } => {
                    json!({ "kind": "compacted", "earliest": earliest })
                }
                ResetReason::CursorBeyondStream { next_offset } => {
                    json!({ "kind": "cursorBeyondStream", "nextOffset": next_offset })
                }
                // UnknownStream 在宿主分类成两个具名理由（ADR-038 决策 2）：
                // 在册未产字节 ⇒ notStarted；任务还在但流已退场 ⇒ evicted。
                ResetReason::UnknownStream => {
                    let classified =
                        classify_unknown_stream(registry, context, &task_id, generation);
                    json!({ "kind": classified })
                }
                ResetReason::UnitsNotAnchorable => json!({ "kind": "unitsNotAnchorable" }),
            };
            ("reset", *from, bytes, reason_json)
        }
    };
    let ResumePoint::Bytes(resume_after) = plan.resume_after() else {
        // resume_after 的契约恒为字节游标（core 注释钉死），不可达分支。
        return failure(id, ErrorCode::IoError, "unreachable: non-byte resume point");
    };
    ok(
        &request,
        json!({
            "kind": kind_out,
            "from": from,
            "bytes": base64::engine::general_purpose::STANDARD.encode(bytes),
            "reason": reason,
            "resumeAfter": resume_after,
        }),
    )
}

fn terminal_acquire_control(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
) -> Frame {
    use aeroric_core::terminal::stream::TerminalIdentity;
    let id = request.id.clone();
    let Some((task_id, generation)) = terminal_identity_of(&request) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.acquire_control requires params.taskId and params.generation",
        );
    };
    let identity = TerminalIdentity::new(task_id, generation);
    let handle = terminal.acquire_control(&identity);
    ok(&request, json!({ "ticket": handle.ticket() }))
}

fn terminal_release_control(
    request: Request,
    terminal: &aeroric_core::terminal::pty::TerminalService,
) -> Frame {
    use aeroric_core::terminal::lease::LeaseHandle;
    use aeroric_core::terminal::stream::TerminalIdentity;
    let id = request.id.clone();
    let Some((task_id, generation)) = terminal_identity_of(&request) else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.release_control requires params.taskId and params.generation",
        );
    };
    let Some(ticket) = request
        .params
        .get("ticket")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse::<u64>().ok()))
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "terminal.release_control requires params.ticket",
        );
    };
    let identity = TerminalIdentity::new(task_id, generation);
    let released = terminal.release_control(&identity, &LeaseHandle::from_ticket(ticket));
    ok(&request, json!({ "released": released }))
}

pub fn register_session_services(
    set: &mut ServiceSet,
    service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    families: std::sync::Arc<dyn aeroric_core::tasks::AgentFamilies>,
) {
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "session.list",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { session_list(request, service, context) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        let families = std::sync::Arc::clone(&families);
        set.register(
            "session.history",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                let families = std::sync::Arc::clone(&families);
                Box::pin(async move { session_history(request, service, context, families) })
            }),
        );
    }
    {
        let service = std::sync::Arc::clone(&service);
        let context = std::sync::Arc::clone(&context);
        set.register(
            "session.export",
            std::sync::Arc::new(move |request, _call| {
                let service = std::sync::Arc::clone(&service);
                let context = std::sync::Arc::clone(&context);
                Box::pin(async move { session_export(request, service, context) })
            }),
        );
    }
}

/// 找到任务记录（跨项目扫描，与 `find_project_of_task` 同行——⑦ 评审记合并）。
fn find_task_record(
    context: &std::sync::Arc<aeroric_core::context::CoreContext>,
    task_id: &str,
) -> Option<(aeroric_core::storage::Record, aeroric_core::storage::Record)> {
    let projects = ProjectService::new(std::sync::Arc::clone(context))
        .list()
        .ok()?;
    for project in projects {
        let Some(id) = mutations::id_of(&project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(context, id) else {
            continue;
        };
        if let Some(record) = stamped
            .records
            .iter()
            .find(|record| mutations::text(record, "id") == Some(task_id))
        {
            return Some((record.clone(), project));
        }
    }
    None
}

fn session_list(
    request: Request,
    _service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let projects = match ProjectService::new(std::sync::Arc::clone(&context)).list() {
        Ok(projects) => projects,
        Err(e) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("cannot list projects: {e}"),
            )
        }
    };
    let mut sessions = Vec::new();
    for project in &projects {
        let Some(project_id) = mutations::id_of(project) else {
            continue;
        };
        let Ok(stamped) = load_stamped(&context, project_id) else {
            continue;
        };
        for record in &stamped.records {
            let (session_id, session_path) =
                aeroric_core::tasks::session_owner::current_session(record);
            if let Some(path) = session_path {
                sessions.push(json!({
                    "taskId": mutations::text(record, "id"),
                    "projectId": project_id,
                    "sessionPath": path,
                    "sessionId": session_id,
                }));
            }
        }
    }
    ok(&request, json!({ "sessions": sessions }))
}

/// B4: at most this much of a remote transcript is read (its tail).
const REMOTE_HISTORY_MAX_BYTES: u64 = 32 * 1024 * 1024;

fn session_history(
    request: Request,
    _service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
    families: std::sync::Arc<dyn aeroric_core::tasks::AgentFamilies>,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.history requires params.taskId",
        );
    };
    let Some((record, project)) = find_task_record(&context, task_id) else {
        return failure(id, ErrorCode::NotFound, "no task with that id");
    };
    let (recorded_id, recorded_path) = aeroric_core::tasks::session_owner::current_session(&record);
    let Some(path) = recorded_path else {
        return failure(
            id,
            ErrorCode::NotFound,
            "this task has no session transcript on disk yet",
        );
    };
    // B4: a remote session's transcript lives on that machine; read its tail over SSH.
    let remote = match aeroric_core::tasks::launch_plan::working_directory(&record, &project) {
        Some(aeroric_core::tasks::WorkingDirectory::Ssh { connection_id, .. }) => {
            Some(connection_id)
        }
        _ => None,
    };
    let read = match &remote {
        Some(connection_id) => crate::remote_sessions::read_remote_file(
            &context,
            connection_id,
            &path,
            REMOTE_HISTORY_MAX_BYTES,
        )
        .map(Option::unwrap_or_default)
        .map_err(std::io::Error::other),
        None => std::fs::read_to_string(&path),
    };
    let raw = match read {
        Ok(raw) => raw,
        // A new session's transcript appears with its first message (N6a D3).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return failure(
                id,
                ErrorCode::IoError,
                &format!("cannot read transcript: {e}"),
            )
        }
    };
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let agent = mutations::text(&record, "agent").unwrap_or_default();
    let family = families.session_family(agent);
    let session_id = recorded_id.unwrap_or_else(|| task_id.to_string());
    // Paged like imports: one reply must fit the 1 MiB frame, or the client
    // sees `disconnected`. `cursor` = how many older messages remain.
    let before = request
        .params
        .get("cursor")
        .and_then(Value::as_str)
        .and_then(|cursor| cursor.parse::<usize>().ok());
    let page = crate::imports_services::history_page(
        session_page(session_id, family, parse_messages(&lines, family)),
        before,
    );
    let mut reply = serde_json::to_value(page).unwrap_or_default();
    reply["path"] = json!(path);
    // 2026-10-29: the 统计栏 figures for the whole session, on the first page.
    if before.is_none() {
        use aeroric_core::sessions::transcript_stats;
        let stats = match family {
            ProtocolFamily::Codex => transcript_stats::codex(&lines),
            ProtocolFamily::Claude => transcript_stats::claude(&lines),
            _ => None,
        };
        if let Some(stats) = stats {
            reply["stats"] = serde_json::to_value(stats).unwrap_or_default();
        }
    }
    ok(&request, reply)
}

fn session_export(
    request: Request,
    _service: std::sync::Arc<TaskService>,
    context: std::sync::Arc<aeroric_core::context::CoreContext>,
) -> Frame {
    let id = request.id.clone();
    let Some(task_id) = request
        .params
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.export requires params.taskId",
        );
    };
    let Some(out) = request
        .params
        .get("out")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "session.export requires params.out",
        );
    };
    let Some((record, _project)) = find_task_record(&context, task_id) else {
        return failure(id, ErrorCode::NotFound, "no task with that id");
    };
    let (recorded_id, recorded_path) = aeroric_core::tasks::session_owner::current_session(&record);
    let Some(path) = recorded_path else {
        return failure(
            id,
            ErrorCode::NotFound,
            "this task has no session transcript to export",
        );
    };
    let agent = mutations::text(&record, "agent")
        .unwrap_or_default()
        .to_string();
    let source = match agent.as_str() {
        "dsh" => ExportSource::Dsh,
        "omp" => ExportSource::Omp,
        _ => ExportSource::ClaudeOrCodex,
    };
    let meta = ExportTaskMeta {
        name: mutations::text(&record, "name").map(str::to_string),
        prompt: mutations::text(&record, "prompt")
            .unwrap_or_default()
            .to_string(),
        agent,
        created_at: mutations::text(&record, "createdAt")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        session_id: recorded_id,
        worktree_branch: mutations::text(&record, "worktreeBranch").map(str::to_string),
        base_branch: mutations::text(&record, "baseBranch").map(str::to_string),
        additions: None,
        deletions: None,
        failure_reason: mutations::text(&record, "failureReason").map(str::to_string),
    };
    match export_session_markdown(std::path::Path::new(&path), out, source, &meta) {
        Ok(()) => ok(&request, json!({ "exported": out })),
        Err(e) => failure(id, ErrorCode::IoError, &e),
    }
}

/// P6-01：文件服务（宿主 command 只做 adapter；业务主体在 core `files/`）。
/// 路径校验 server 端权威（不依赖客户端过滤 `..`）；projectId → 项目根经
/// `find_project_of_task` 同款的 record 扫描。
pub fn register_file_services(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    ports: crate::file_access::FilePorts,
) {
    {
        let context = Arc::clone(&context);
        set.register(
            "files.read_dir",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_read_dir(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.read_file",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_read_file(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.write_file",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_write_file(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.create_file",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_create(request, context, false) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.create_directory",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_create(request, context, true) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let trash = Arc::clone(&ports.trash);
        set.register(
            "files.delete_path",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                let trash = Arc::clone(&trash);
                Box::pin(async move { files_delete(request, context, trash) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.rename_path",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_rename(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.read_image_preview",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_read_image_preview(request, context) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "files.read_clipboard_file_paths",
            Arc::new(move |request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move { files_read_clipboard_file_paths(request, context) })
            }),
        );
    }
}

/// projectId → 本地项目根。先守卫 record.location，Local 保留原 `path`。
fn find_project_root(
    context: &Arc<CoreContext>,
    project_id: &str,
) -> Result<Option<String>, FileProjectError> {
    let Ok(projects) = aeroric_core::storage::load_projects(context) else {
        // Preserve this batch's existing missing/corrupt registry -> NotFound.
        return Ok(None);
    };
    // A linked folder is browsed as `<project id>#linked:<path>` (2026-10-29):
    // only a folder the project actually lists is served.
    let (project_id, linked) = match project_id.split_once("#linked:") {
        Some((id, path)) => (id, Some(path)),
        None => (project_id, None),
    };
    for record in projects {
        if mutations::text(&record, "id") == Some(project_id) {
            require_local_record(&record)?;
            return Ok(match linked {
                Some(path) => aeroric_core::projects::linked::stored(&record)
                    .into_iter()
                    .find(|stored| stored == path),
                None => mutations::text(&record, "path").map(str::to_string),
            });
        }
    }
    Ok(None)
}

fn file_params(request: &Request) -> Result<(String, String), Box<Frame>> {
    let id = request.id.clone();
    let Some(project_id) = request
        .params
        .get("projectId")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return Err(Box::new(failure(
            id,
            ErrorCode::InvalidArgument,
            "requires params.projectId",
        )));
    };
    let Some(relative) = request.params.get("path").and_then(Value::as_str) else {
        return Err(Box::new(failure(
            id,
            ErrorCode::InvalidArgument,
            "requires params.path",
        )));
    };
    Ok((project_id.to_string(), relative.to_string()))
}

fn files_read_dir(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    let root = match find_project_root(&context, &project_id) {
        Ok(Some(root)) => root,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no project with that id",
            );
        }
        Err(error) => return error.into_frame(&request),
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    if let Some(frame) = target.root_refusal(&request, &root) {
        return frame;
    }
    match aeroric_core::files::read_write::read_dir_entries(&root, &relative) {
        Ok(entries) => ok(&request, serde_json::json!({ "entries": entries })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_read_file(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    let root = match find_project_root(&context, &project_id) {
        Ok(Some(root)) => root,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no project with that id",
            );
        }
        Err(error) => return error.into_frame(&request),
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    if let Some(frame) = target.root_refusal(&request, &root) {
        return frame;
    }
    match aeroric_core::files::read_write::read_file_content(&root, &relative) {
        Ok(content) => ok(&request, serde_json::json!({ "content": content })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

/// 图片预览（切片九十二）。projectId＋相对路径的服务端权威形状（与
/// files_read_file 同一条校验链）；回复是 legacy 的 `ImagePreviewData`
///（camelCase）。**帧上限是硬边界**：UDS 单帧 1 MiB，一张大图的 base64
/// 回复会让 `encode_line` 拒绝并砸断连接——所以在这里先用帧层自己的
/// 判定预检成功帧，超限就回一个干净的 `InvalidArgument`（点明 1 MiB
/// 上限与图片字节数），**绝不写出超长帧**。Embedded（Tauri 进程内）不
/// 经 UDS，10 MiB 旧上限原样保留。
fn files_read_image_preview(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    let root = match find_project_root(&context, &project_id) {
        Ok(Some(root)) => root,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no project with that id",
            );
        }
        Err(error) => return error.into_frame(&request),
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    if let Some(frame) = target.root_refusal(&request, &root) {
        return frame;
    }
    let Ok(validated) = aeroric_core::files::path_policy::resolve_within(&root, &relative) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "path escapes the project root",
        );
    };
    match aeroric_core::files::legacy_read::read_image_preview(&validated) {
        Ok(preview) => match serde_json::to_value(&preview) {
            Ok(value) => {
                let frame = ok(&request, value);
                match aeroric_protocol::envelope::encode_line(&frame) {
                    Ok(_) => frame,
                    Err(_) => failure(
                        request.id.clone(),
                        ErrorCode::InvalidArgument,
                        &format!(
                            "image preview is too large for the host transport (single-frame limit 1 MiB, image is {} B); open the file externally instead",
                            preview.byte_length
                        ),
                    ),
                }
            }
            Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
        },
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

/// 剪贴板文件路径（切片九十二）。legacy 的参数只有注入的运行时句柄，wire
/// params 为空对象；实现走 `CoreContext.platform` 端口——headless 宿主在
/// 同一台机器上，macOS 经 osascript/NSPasteboard 原生可答（与 Tauri 适配层
/// 同一脚本），非 macOS 按端口契约返回空列表。
fn files_read_clipboard_file_paths(request: Request, context: Arc<CoreContext>) -> Frame {
    match context.platform.read_clipboard_file_paths() {
        Ok(paths) => ok(&request, serde_json::json!(paths)),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

fn files_write_file(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(content) = request.params.get("content").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.content",
        );
    };
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    if let Some(frame) = write_refusal(&request, &context) {
        return frame;
    }
    let root = match find_project_root(&context, &project_id) {
        Ok(Some(root)) => root,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no project with that id",
            );
        }
        Err(error) => return error.into_frame(&request),
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    if let Some(frame) = target.root_refusal(&request, &root) {
        return frame;
    }
    match aeroric_core::files::read_write::write_file_content(&root, &relative, content) {
        Ok(()) => ok(&request, serde_json::json!({ "written": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_create(request: Request, context: Arc<CoreContext>, directory: bool) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    if let Some(frame) = write_refusal(&request, &context) {
        return frame;
    }
    let root = match find_project_root(&context, &project_id) {
        Ok(Some(root)) => root,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no project with that id",
            );
        }
        Err(error) => return error.into_frame(&request),
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    if let Some(frame) = target.root_refusal(&request, &root) {
        return frame;
    }
    let result = if directory {
        aeroric_core::files::read_write::create_directory(&root, &relative)
    } else {
        aeroric_core::files::read_write::create_file(&root, &relative)
    };
    match result {
        Ok(()) => ok(&request, serde_json::json!({ "created": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_delete(
    request: Request,
    context: Arc<CoreContext>,
    trash: Arc<crate::file_access::TrashDelete>,
) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    if let Some(frame) = write_refusal(&request, &context) {
        return frame;
    }
    let root = match find_project_root(&context, &project_id) {
        Ok(Some(root)) => root,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no project with that id",
            );
        }
        Err(error) => return error.into_frame(&request),
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    if let Some(frame) = target.root_refusal(&request, &root) {
        return frame;
    }
    // 删除进废纸篓（不默认永久删除）；显式永久删除走独立的授权路径。
    match trash(&root, &relative) {
        Ok(()) => ok(&request, serde_json::json!({ "deleted": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

fn files_rename(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project_id, relative) = match file_params(&request) {
        Ok(v) => v,
        Err(frame) => return *frame,
    };
    let Some(new_name) = request.params.get("newName").and_then(Value::as_str) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params.newName",
        );
    };
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    if let Some(frame) = write_refusal(&request, &context) {
        return frame;
    }
    let root = match find_project_root(&context, &project_id) {
        Ok(Some(root)) => root,
        Ok(None) => {
            return failure(
                request.id.clone(),
                ErrorCode::NotFound,
                "no project with that id",
            );
        }
        Err(error) => return error.into_frame(&request),
    };
    let Ok(root) = aeroric_core::files::path_policy::validate_project_root(&root) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "project root is invalid",
        );
    };
    if let Some(frame) = target.root_refusal(&request, &root) {
        return frame;
    }
    match aeroric_core::files::read_write::rename_path(&root, &relative, new_name) {
        Ok(()) => ok(&request, serde_json::json!({ "renamed": true })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

#[cfg(test)]
mod terminal_replay_tests {
    use super::*;
    use aeroric_core::context::CoreContext;
    use aeroric_core::runtime::registry::TaskRegistry;
    use aeroric_core::terminal::pty::TerminalService;
    use aeroric_core::terminal::stream::TerminalIdentity;
    use aeroric_protocol::envelope::Frame;
    use base64::Engine as _;

    fn request(params: Value) -> Request {
        Request {
            v: 1,
            id: "t1".into(),
            method: "terminal.plan_replay".into(),
            idempotency_key: None,
            params,
            extra: Default::default(),
        }
    }

    fn result_of(frame: Frame) -> Value {
        match frame {
            Frame::Success(success) => success.result,
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    fn temp_context(tag: &str) -> std::sync::Arc<CoreContext> {
        let dir =
            std::env::temp_dir().join(format!("aeroric-replay-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::sync::Arc::new(CoreContext::read_only(
            aeroric_core::paths::AppPaths::with_data_dir(std::path::Path::new("/"), Some(&dir)),
        ))
    }

    #[test]
    fn plan_replay_returns_seamless_bytes_and_a_byte_resume_after() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("seamless");
        let identity = TerminalIdentity::new("task-1", 1);
        let _ = &identity;
        terminal.publish(&identity, b"hello terminal");
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "task-1", "generation": 1,
                "cursor": {"kind": "bytes", "value": 0},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["kind"], "seamless");
        assert_eq!(result["from"], 0);
        assert_eq!(result["resumeAfter"], 14);
        assert_eq!(
            result["bytes"],
            base64::engine::general_purpose::STANDARD.encode(b"hello terminal")
        );
    }

    #[test]
    fn plan_replay_reports_cursor_beyond_the_stream_end() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("beyond");
        let identity = TerminalIdentity::new("task-1", 1);
        let _ = &identity;
        terminal.publish(&identity, b"abc");
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "task-1", "generation": 1,
                "cursor": {"kind": "bytes", "value": 99},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["kind"], "reset");
        assert_eq!(result["reason"]["kind"], "cursorBeyondStream");
        assert_eq!(result["reason"]["nextOffset"], 3);
        assert_eq!(result["resumeAfter"], 3);
    }

    #[test]
    fn plan_replay_classifies_a_registered_live_run_as_not_started() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("not-started");
        registry.runs.register("task-live", 3);
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "task-live", "generation": 3,
                "cursor": {"kind": "bytes", "value": 0},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["kind"], "reset");
        assert_eq!(result["reason"]["kind"], "notStarted");
    }

    #[test]
    fn plan_replay_falls_back_to_unknown_when_nothing_knows_the_task() {
        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("unknown");
        let frame = terminal_plan_replay(
            request(serde_json::json!({
                "taskId": "never-heard-of", "generation": 9,
                "cursor": {"kind": "bytes", "value": 0},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["reason"]["kind"], "unknownStream");
    }

    /// `evicted` 是三个理由里**唯一有实现却没有任何测试**的那个(AP8 勘察点名)。
    ///
    /// 它要回答的问题与 `notStarted` 恰好相反,而客户端的动作也不同:`notStarted`
    /// ⇒"挂起等活流",`evicted` ⇒"这一代没有任何历史可重放,历史走
    /// `session.history`"。两者混同的后果是重连后一片空白却不知道该等还是该死心
    /// ——ADR-038 决策 2 拆这两个理由的全部意义就在这里。
    ///
    /// 区分依据:任务**记录还在**(`find_project_of_task` 命中)但该代**不在册**
    /// (`resource_run` 不命中)。因此这里必须真的建出一个项目+任务记录,否则
    /// 落到的还是 `unknownStream`,用例就测不到它声称的东西。
    #[test]
    fn plan_replay_classifies_a_retired_run_of_a_known_task_as_evicted() {
        use aeroric_core::storage::Record;
        use serde_json::json;

        let terminal = TerminalService::new();
        let registry = TaskRegistry::default();
        let context = temp_context("evicted");

        // 真实落盘一个项目与一条任务记录:这是 `evicted` 与 `unknownStream` 的
        // 唯一分界(记录在 vs 两处都查无)。
        let project_dir = context
            .paths
            .project_dir("proj-evicted")
            .expect("project dir");
        std::fs::create_dir_all(&project_dir).expect("create project dir");
        std::fs::write(
            context.paths.projects_file(),
            serde_json::to_vec(&json!([{
                "id": "proj-evicted",
                "name": "evicted",
                "path": project_dir.to_string_lossy(),
                "orderIndex": 0,
            }]))
            .expect("projects json"),
        )
        .expect("write projects");
        std::fs::write(
            context
                .paths
                .tasks_file("proj-evicted")
                .expect("tasks file"),
            serde_json::to_vec(&json!([{
                "id": "task-retired",
                "projectId": "proj-evicted",
                "agent": "claude",
                "status": "done",
                "permissionMode": "ask",
                "createdAt": 1_700_000_000_000i64,
            }]))
            .expect("tasks json"),
        )
        .expect("write tasks");

        // 该代**不在册**:没有 register,流也从未产出。这正是"已收尾并被 FIFO 退场"。
        let frame = terminal_plan_replay(
            request(json!({
                "taskId": "task-retired", "generation": 2,
                "cursor": {"kind": "bytes", "value": 0},
            })),
            &terminal,
            &registry,
            &context,
        );
        let result = result_of(frame);
        assert_eq!(result["kind"], "reset");
        assert_eq!(
            result["reason"]["kind"], "evicted",
            "a known task with an unregistered generation is 已退场, not 未知流: {result}"
        );
        let _ = Record::default();
    }

    #[test]
    fn lease_tickets_round_trip_through_the_wire_shape() {
        let terminal = TerminalService::new();
        let acquired = result_of(terminal_acquire_control(
            request(serde_json::json!({"taskId": "task-1", "generation": 1})),
            &terminal,
        ));
        let ticket = acquired["ticket"].as_u64().expect("ticket is a number");
        assert!(ticket > 0);
        let released = result_of(terminal_release_control(
            request(serde_json::json!({"taskId": "task-1", "generation": 1, "ticket": ticket})),
            &terminal,
        ));
        assert_eq!(released["released"], true);
        // 过期票据再放＝no-op 成功（P3-07 语义），不报错。
        let again = result_of(terminal_release_control(
            request(serde_json::json!({"taskId": "task-1", "generation": 1, "ticket": ticket})),
            &terminal,
        ));
        assert_eq!(again["released"], false);
    }
    #[test]
    fn active_task_ids_are_sorted_and_do_not_include_shell_namespaces() {
        let registry = std::sync::Arc::new(TaskRegistry::default());
        registry.runs.register("task-b", 1);
        registry.runs.register("task-a", 1);
        registry.runs.register("shell:project:1:1", 1);
        registry.runs.register("ssh:project:1:1", 1);
        let frame = terminal_active_task_ids(request(serde_json::json!({})), &registry);
        assert_eq!(result_of(frame), serde_json::json!(["task-a", "task-b"]));
    }
}
