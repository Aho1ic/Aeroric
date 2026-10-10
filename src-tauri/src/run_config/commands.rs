//! Local definition IPC. Defer even root validation until Embedded is selected.

use tauri::{AppHandle, Runtime};

use super::RunConfigDocument;

#[tauri::command]
pub async fn read_run_configs<R: Runtime>(
    app: AppHandle<R>,
    project_path: String,
) -> Result<RunConfigDocument, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.read_run_configs",
        serde_json::json!({"projectPath": project_path}),
        async {
            let document = aeroric_core::ide::run_config::read_run_configs(&project_path)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(document).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn write_run_configs<R: Runtime>(
    app: AppHandle<R>,
    project_path: String,
    document: RunConfigDocument,
) -> Result<RunConfigDocument, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.write_run_configs",
        serde_json::json!({"projectPath": project_path, "document": document}),
        async {
            let result =
                aeroric_core::ide::run_config::write_run_configs_legacy(&project_path, document)
                    .map_err(|error| error.to_string())?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
