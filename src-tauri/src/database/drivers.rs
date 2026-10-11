// ── 切片九十八（database 收尾）：驱动清单改宿主转发壳。静态资产读取在
// core `database::schema_face`（include_str 路径随 crate 换相对深度）。

#[tauri::command]
pub async fn dbx_driver_manifest<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<serde_json::Value, String> {
    crate::host_dispatch::dispatch(
        &app,
        "database.dbx_driver_manifest",
        serde_json::json!({}),
        async { aeroric_core::database::schema_face::driver_manifest_value() },
    )
    .await
}
