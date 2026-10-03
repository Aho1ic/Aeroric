//! 对象存储 / 网盘 / 文件共享连接（宿主 shim）。
//!
//! P7-06：模型与读写已上收 `aeroric_core::storage_backends::conn`
//! （两文件策略——`storage-connections.json` 公开 + `storage-secrets.json`
//! 凭据，均 0600——原样移动，路径参数化为 data root）。这里保留 5 条 Tauri
//! 命令，经 `StorageConnRepo`（数据根 = 宿主 `aeroric_dir()`）调用。

use aeroric_core::storage_backends::conn::{StorageConnRepo, StorageConnection};

fn repo() -> Result<StorageConnRepo, String> {
    Ok(StorageConnRepo::new(crate::storage::aeroric_dir()?))
}

/// 列出连接。**不含凭据** —— 前端只需要知道某个键是否已设置。
#[tauri::command]
pub async fn storage_list_connections() -> Result<Vec<StorageConnection>, String> {
    tokio::task::spawn_blocking(|| repo()?.list_public())
        .await
        .map_err(|e| e.to_string())?
}

/// 返回每条连接已设置的凭据键名(不含值),供表单显示"已保存"状态。
#[tauri::command]
pub async fn storage_secret_keys() -> Result<std::collections::BTreeMap<String, Vec<String>>, String>
{
    tokio::task::spawn_blocking(|| repo()?.secret_keys_set())
        .await
        .map_err(|e| e.to_string())?
}

/// 新增或更新一条连接。`secrets` 中为空字符串的键表示"保留原值"。
#[tauri::command]
pub async fn storage_save_connection(
    connection: StorageConnection,
) -> Result<Vec<StorageConnection>, String> {
    tokio::task::spawn_blocking(move || repo()?.save_connection(connection))
        .await
        .map_err(|e| e.to_string())?
}

/// 删除一条连接及其凭据。
#[tauri::command]
pub async fn storage_delete_connection(
    connection_id: String,
) -> Result<Vec<StorageConnection>, String> {
    tokio::task::spawn_blocking(move || repo()?.delete_connection(&connection_id))
        .await
        .map_err(|e| e.to_string())?
}

/// 记录最近连接时间。
#[tauri::command]
pub async fn storage_touch_connection(connection_id: String, timestamp: i64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || repo()?.touch_connection(&connection_id, timestamp))
        .await
        .map_err(|e| e.to_string())?
}

pub use aeroric_core::storage_backends::conn::StorageProtocol;

/// 按 id 取连接（含凭据）。数据根 = 宿主 `aeroric_dir()`（sftp/ssh 等宿主
/// 调用点保持单参签名不变）。
pub fn find_connection(connection_id: &str) -> Result<StorageConnection, String> {
    aeroric_core::storage_backends::conn::find_connection(
        &crate::storage::aeroric_dir()?,
        connection_id,
    )
}
