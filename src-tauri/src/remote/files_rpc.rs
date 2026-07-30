//! 只读 files / git RPC(M5):手机端 Diff 查看与文件浏览。
//!
//! 全部复用桌面既有实现与安全校验:
//! - 目录/文件读取走 `fs.rs` 的 `read_dir_entries` / `read_file_content`
//!   (内含 canonicalize + 项目根前缀校验,防路径穿越);
//! - git 状态/diff 走 `git.rs` 的 `git_status` / `git_file_diff`
//!   (参数经 `--` 分隔防选项注入;本层再做相对路径词法校验,
//!   杜绝 `--no-index` 兜底分支读到项目外文件)。
//!
//! 与 session.messages 一致:SSH / WSL 项目不报错,返回 `available:false` +
//! 原因,手机端据此展示「请在桌面查看」。写操作一律不暴露。

use serde_json::{json, Value};

use super::rpc::str_param;
use super::session_push::load_project_task;
use crate::storage::{self, Project, ProjectLocation};

/// 手机端单次读文件的返回上限(与终端快照同量级,避免撑爆移动端)。
const MAX_FILE_RESPONSE_BYTES: usize = 200 * 1024;

fn non_local_reason(project: &Project) -> Option<&'static str> {
    match project.location {
        Some(ProjectLocation::Ssh { .. }) => Some("ssh"),
        Some(ProjectLocation::Wsl { .. }) => Some("wsl"),
        Some(ProjectLocation::Local { .. }) | None => None,
    }
}

async fn load_project(project_id: String) -> Result<Project, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let projects = storage::load_projects()?;
        projects
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project not found: {project_id}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// git 文件路径必须是仓库内相对路径:拒绝绝对路径与任何 `..`/前缀成分。
fn validate_relative_path(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if path.is_empty() || p.is_absolute() {
        return Err("Invalid file path".to_string());
    }
    for component in p.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => return Err("Invalid file path".to_string()),
        }
    }
    Ok(())
}

/// diff 的工作目录:任务在 worktree 里跑就看 worktree,否则项目根。
async fn resolve_git_root(params: &Value) -> Result<Result<String, &'static str>, String> {
    let project_id = str_param(params, "projectId")?;
    if let Some(task_id) = params.get("taskId").and_then(Value::as_str) {
        let (project, task) = load_project_task(project_id, task_id.to_string()).await?;
        if let Some(reason) = non_local_reason(&project) {
            return Ok(Err(reason));
        }
        return Ok(Ok(task.worktree_path.unwrap_or(project.path)));
    }
    let project = load_project(project_id).await?;
    if let Some(reason) = non_local_reason(&project) {
        return Ok(Err(reason));
    }
    Ok(Ok(project.path))
}

fn unavailable(reason: &str) -> Value {
    json!({ "available": false, "reason": reason })
}

/// RPC `project.files { projectId, path? }`:列目录(缺省=项目根)。
/// entries 携带绝对路径,手机端拿它继续下钻;每次调用都重新过路径校验。
pub(crate) async fn project_files(params: Value) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let project = load_project(project_id).await?;
    if let Some(reason) = non_local_reason(&project) {
        return Ok(unavailable(reason));
    }
    let root = project.path.clone();
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| root.clone());
    let entries = crate::fs::read_dir_entries(path, root.clone()).await?;
    Ok(json!({
        "available": true,
        "root": root,
        "entries": serde_json::to_value(entries).map_err(|e| e.to_string())?,
    }))
}

/// RPC `project.readFile { projectId, path }`:只读文本,超限截断。
pub(crate) async fn project_read_file(params: Value) -> Result<Value, String> {
    let project_id = str_param(&params, "projectId")?;
    let path = str_param(&params, "path")?;
    let project = load_project(project_id).await?;
    if let Some(reason) = non_local_reason(&project) {
        return Ok(unavailable(reason));
    }
    let content = crate::fs::read_file_content(path, project.path).await?;
    let total_bytes = content.len();
    let truncated = total_bytes > MAX_FILE_RESPONSE_BYTES;
    let content = if truncated {
        let mut end = MAX_FILE_RESPONSE_BYTES;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        content[..end].to_string()
    } else {
        content
    };
    Ok(json!({
        "available": true,
        "content": content,
        "truncated": truncated,
        "totalBytes": total_bytes,
    }))
}

/// RPC `git.changes { projectId, taskId? }`:工作区变更列表。
pub(crate) async fn git_changes(params: Value) -> Result<Value, String> {
    let root = match resolve_git_root(&params).await? {
        Ok(root) => root,
        Err(reason) => return Ok(unavailable(reason)),
    };
    let changes = crate::git::git_status(root).await?;
    Ok(json!({
        "available": true,
        "changes": serde_json::to_value(changes).map_err(|e| e.to_string())?,
    }))
}

/// RPC `git.diff { projectId, taskId?, path, staged? }`:单文件 unified diff。
pub(crate) async fn git_diff(params: Value) -> Result<Value, String> {
    let file_path = str_param(&params, "path")?;
    validate_relative_path(&file_path)?;
    let staged = params
        .get("staged")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let root = match resolve_git_root(&params).await? {
        Ok(root) => root,
        Err(reason) => return Ok(unavailable(reason)),
    };
    let diff = crate::git::git_file_diff(root, file_path, staged).await?;
    Ok(json!({ "available": true, "diff": diff }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_validation_blocks_traversal_and_absolute() {
        assert!(validate_relative_path("src/main.rs").is_ok());
        assert!(validate_relative_path("./a/b.txt").is_ok());
        assert!(validate_relative_path("").is_err());
        assert!(validate_relative_path("/etc/passwd").is_err());
        assert!(validate_relative_path("../outside.txt").is_err());
        assert!(validate_relative_path("a/../../outside.txt").is_err());
        #[cfg(windows)]
        assert!(validate_relative_path("C:\\windows\\system32").is_err());
    }
}
