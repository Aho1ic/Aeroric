//! Project/task-list/history IPC boundaries; synchronous storage callers stay unchanged.

use tauri::{AppHandle, Runtime};

use super::{Project, Task};

#[tauri::command]
pub async fn load_projects<R: Runtime>(app: AppHandle<R>) -> Result<Vec<Project>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "project.load_projects",
        serde_json::json!({}),
        async { serde_json::to_value(super::load_projects()?).map_err(|error| error.to_string()) },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn load_project_tasks<R: Runtime>(
    app: AppHandle<R>,
    project_id: String,
) -> Result<Vec<Task>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "project.load_project_tasks",
        serde_json::json!({"projectId": project_id}),
        async {
            serde_json::to_value(super::load_project_tasks(project_id)?)
                .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_task_terminal_history<R: Runtime>(
    app: AppHandle<R>,
    task_id: String,
) -> Result<String, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "project.read_task_terminal_history",
        serde_json::json!({"taskId": task_id}),
        async {
            serde_json::to_value(super::read_task_terminal_history(task_id).await?)
                .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_task_terminal_histories<R: Runtime>(
    app: AppHandle<R>,
    task_ids: Vec<String>,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "project.delete_task_terminal_histories",
        serde_json::json!({"taskIds": task_ids}),
        async {
            super::delete_task_terminal_histories(task_ids)?;
            Ok(serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
