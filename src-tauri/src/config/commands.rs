//! Config IPC. Entire local operations, including the legacy settings loader,
//! remain deferred until Embedded is selected.

use tauri::{AppHandle, Runtime};

use super::ProjectConfig;
use aeroric_core::settings::project_config;

#[tauri::command]
pub async fn read_project_config<R: Runtime>(
    app: AppHandle<R>,
    project_path: String,
) -> Result<ProjectConfig, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "settings.read_project_config",
        serde_json::json!({"projectPath": project_path}),
        async {
            serde_json::to_value(super::read_project_config(project_path)?)
                .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn init_project_config<R: Runtime>(
    app: AppHandle<R>,
    project_path: String,
) -> Result<ProjectConfig, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "settings.init_project_config",
        serde_json::json!({"projectPath": project_path}),
        async {
            serde_json::to_value(project_config::init_project_config_legacy(&project_path)?)
                .map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn write_project_config<R: Runtime>(
    app: AppHandle<R>,
    project_path: String,
    config: ProjectConfig,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "settings.write_project_config",
        serde_json::json!({"projectPath": project_path, "config": config}),
        async {
            project_config::write_project_config_legacy(&project_path, &config)?;
            Ok(serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_agent_config_file_path<R: Runtime>(
    app: AppHandle<R>,
    agent: String,
) -> Result<String, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "settings.get_agent_config_file_path",
        serde_json::json!({"agent": &agent}),
        async move {
            let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
                let settings = crate::app_settings::load_settings_internal();
                Ok(super::agent_config_path_from_settings(&agent, &settings)?
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default())
            })
            .await
            .map_err(|error| error.to_string())??;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Reads the agent's raw local config; None remains distinct from an empty file.
#[tauri::command]
pub async fn read_agent_config_file<R: Runtime>(
    app: AppHandle<R>,
    agent: String,
) -> Result<Option<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "settings.read_agent_config_file",
        serde_json::json!({"agent": &agent}),
        async move {
            let result = tokio::task::spawn_blocking(move || {
                let settings = crate::app_settings::load_settings_internal();
                let Some(path) = super::agent_config_path_from_settings(&agent, &settings)? else {
                    return Ok(None);
                };
                aeroric_core::settings::agent_config_files::read(&path)
            })
            .await
            .map_err(|error| error.to_string())??;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Writes raw content with the unchanged Embedded legacy authority/atomic policy.
#[tauri::command]
pub async fn write_agent_config_file<R: Runtime>(
    app: AppHandle<R>,
    agent: String,
    content: String,
) -> Result<(), String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "settings.write_agent_config_file",
        serde_json::json!({"agent": &agent, "content": &content}),
        async move {
            tokio::task::spawn_blocking(move || {
                let settings = crate::app_settings::load_settings_internal();
                let Some(path) = super::agent_config_path_from_settings(&agent, &settings)? else {
                    return Err("Agent config file path is not configured".to_string());
                };
                aeroric_core::settings::agent_config_files::write_legacy(&path, &content)
            })
            .await
            .map_err(|error| error.to_string())??;
            Ok(serde_json::Value::Null)
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
