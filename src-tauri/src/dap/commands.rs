//! Local definition IPC. Defer even root validation until Embedded is selected.

use tauri::{AppHandle, Runtime};

use super::DebugConfigDocument;

#[tauri::command]
pub async fn read_debug_configs<R: Runtime>(
    app: AppHandle<R>,
    project_path: String,
) -> Result<DebugConfigDocument, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.read_debug_configs",
        serde_json::json!({"projectPath": project_path}),
        async {
            let document = aeroric_core::ide::dap::read_debug_configs(&project_path)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(document).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn write_debug_configs<R: Runtime>(
    app: AppHandle<R>,
    project_path: String,
    document: DebugConfigDocument,
) -> Result<DebugConfigDocument, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.write_debug_configs",
        serde_json::json!({"projectPath": project_path, "document": document}),
        async {
            let result =
                aeroric_core::ide::dap::write_debug_configs_legacy(&project_path, document)
                    .map_err(|error| error.to_string())?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
