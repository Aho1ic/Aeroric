//! G43 / P8-05: Tauri command adapters only. Search algorithms and DTOs are
//! owned by core; External always dispatches (including unsupported targets).
use crate::path_guard::validate_project_root;
use crate::ssh::SshConnection;
use aeroric_core::files::search as core;
use serde::Serialize;
use serde_json::{json, Value};

pub use core::{
    ReplacePreview, ReplaceSummary, TextReplacement, TextSearchMatch, TextSearchOptions,
};

/// Existing SSH credential/host-key policy stays in the legacy adapter until
/// its headless owner is migrated. No credentials are put into search RPCs.
struct LegacySearchTransport(SshConnection);
impl core::RemoteSearchTransport for LegacySearchTransport {
    fn command(&self, remote_command: String) -> std::process::Command {
        crate::ssh::std_ssh_command_for_remote_command(&self.0, remote_command)
    }
    fn annotate_error(&self, detail: &str) -> String {
        crate::ssh::annotate_ssh_error(&self.0, detail)
    }
}

async fn local<T: Serialize + Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Value, String> {
    let result = tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())??;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_text(
    app: tauri::AppHandle,
    project_path: String,
    query: String,
    options: TextSearchOptions,
) -> Result<Vec<TextSearchMatch>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "files.search_text",
        json!({"projectPath": project_path, "query": query, "options": options}),
        local(move || {
            let root = validate_project_root(&project_path)?;
            core::search_text_for_root(
                &root,
                &query,
                &options,
                "rg",
                crate::app_settings::get_login_shell_env(),
            )
        }),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_search_text(
    app: tauri::AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    query: String,
    options: TextSearchOptions,
) -> Result<Vec<TextSearchMatch>, String> {
    // ADR-006: legacy twin -> parent method + target. The current headless host
    // rejects SSH explicitly; it must never run this Embedded future as fallback.
    let value = crate::host_dispatch::dispatch(
        &app,
        "files.search_text",
        json!({"target": {"kind": "ssh", "connectionId": connection.id,
            "remotePath": remote_project_path}, "query": query, "options": options}),
        local(move || {
            let transport = LegacySearchTransport(connection);
            core::remote_search_text_for_root(&transport, &remote_project_path, &query, &options)
        }),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_structured(
    app: tauri::AppHandle,
    project_path: String,
    pattern: String,
    options: TextSearchOptions,
) -> Result<Vec<TextSearchMatch>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "files.search_structured",
        json!({"projectPath": project_path, "pattern": pattern, "options": options}),
        local(move || {
            let root = validate_project_root(&project_path)?;
            core::structured_search_for_root(&root, &pattern, &options)
        }),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_search_structured(
    app: tauri::AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    pattern: String,
    options: TextSearchOptions,
) -> Result<Vec<TextSearchMatch>, String> {
    // ADR-006: legacy twin -> parent method + target. The current headless host
    // rejects SSH explicitly; it must never run this Embedded future as fallback.
    let value = crate::host_dispatch::dispatch(
        &app,
        "files.search_structured",
        json!({"target": {"kind": "ssh", "connectionId": connection.id,
            "remotePath": remote_project_path}, "pattern": pattern, "options": options}),
        local(move || {
            let transport = LegacySearchTransport(connection);
            core::remote_structured_search_for_root(
                &transport,
                &remote_project_path,
                &pattern,
                &options,
            )
        }),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn replace_text_preview(
    app: tauri::AppHandle,
    project_path: String,
    query: String,
    replacement: String,
    options: TextSearchOptions,
) -> Result<ReplacePreview, String> {
    let value = crate::host_dispatch::dispatch(
        &app, "files.replace_text_preview",
        json!({"projectPath": project_path, "query": query, "replacement": replacement, "options": options}),
        local(move || {
            let root = validate_project_root(&project_path)?;
            core::replace_text_preview_for_root(&root, &query, &replacement, &options)
        }),
    ).await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_replace_text_preview(
    app: tauri::AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    query: String,
    replacement: String,
    options: TextSearchOptions,
) -> Result<ReplacePreview, String> {
    // ADR-006: legacy twin -> parent method + target. The current headless host
    // rejects SSH explicitly; it must never run this Embedded future as fallback.
    let value = crate::host_dispatch::dispatch(
        &app, "files.replace_text_preview",
        json!({"target": {"kind": "ssh", "connectionId": connection.id,
            "remotePath": remote_project_path}, "query": query, "replacement": replacement, "options": options}),
        local(move || {
            let transport = LegacySearchTransport(connection);
            core::remote_replace_text_preview_for_root(&transport, &remote_project_path, &query, &replacement, &options)
        }),
    ).await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_text_replacements(
    app: tauri::AppHandle,
    project_path: String,
    replacements: Vec<TextReplacement>,
) -> Result<ReplaceSummary, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "files.apply_text_replacements",
        json!({"projectPath": project_path, "replacements": replacements}),
        local(move || {
            let root = validate_project_root(&project_path)?;
            core::apply_text_replacements_for_root(&root, &replacements)
        }),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_apply_text_replacements(
    app: tauri::AppHandle,
    connection: SshConnection,
    remote_project_path: String,
    replacements: Vec<TextReplacement>,
) -> Result<ReplaceSummary, String> {
    // ADR-006: legacy twin -> parent method + target. The current headless host
    // rejects SSH explicitly; it must never run this Embedded future as fallback.
    let value = crate::host_dispatch::dispatch(
        &app,
        "files.apply_text_replacements",
        json!({"target": {"kind": "ssh", "connectionId": connection.id,
            "remotePath": remote_project_path}, "replacements": replacements}),
        local(move || {
            let transport = LegacySearchTransport(connection);
            core::apply_remote_text_replacements_for_root(
                &transport,
                &remote_project_path,
                &replacements,
            )
        }),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}
