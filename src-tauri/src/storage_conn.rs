//! 对象存储 / 网盘 / 文件共享连接的持久化与协议模型。
//!
//! 与 `ssh.rs` 保持一致的两文件策略:公开字段写 `storage-connections.json`,
//! 凭据(密钥、token、密码)单独写 `storage-secrets.json`,两者都是 0600。
//! 凭据以明文保存(与现有 SSH 密码策略相同),UI 必须如实告知用户。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// 支持的连接协议。序列化为 camelCase,与前端 `StorageProtocol` 一一对应。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum StorageProtocol {
    S3,
    S3Compatible,
    AliyunOss,
    TencentCos,
    JdCloudOss,
    QiniuKodo,
    Upyun,
    WebdavHttps,
    WebdavHttp,
    Dropbox,
    OneDrive,
    GoogleDrive,
    AliyunDrive,
    Box,
    BaiduNetdisk,
    Smb,
    Afp,
    Nfs,
}

impl StorageProtocol {
    pub const ALL: [StorageProtocol; 18] = [
        StorageProtocol::S3,
        StorageProtocol::S3Compatible,
        StorageProtocol::AliyunOss,
        StorageProtocol::TencentCos,
        StorageProtocol::JdCloudOss,
        StorageProtocol::QiniuKodo,
        StorageProtocol::Upyun,
        StorageProtocol::WebdavHttps,
        StorageProtocol::WebdavHttp,
        StorageProtocol::Dropbox,
        StorageProtocol::OneDrive,
        StorageProtocol::GoogleDrive,
        StorageProtocol::AliyunDrive,
        StorageProtocol::Box,
        StorageProtocol::BaiduNetdisk,
        StorageProtocol::Smb,
        StorageProtocol::Afp,
        StorageProtocol::Nfs,
    ];

    /// 稳定标识符,用于日志与错误信息(不参与序列化)。
    pub fn as_str(self) -> &'static str {
        match self {
            StorageProtocol::S3 => "s3",
            StorageProtocol::S3Compatible => "s3Compatible",
            StorageProtocol::AliyunOss => "aliyunOss",
            StorageProtocol::TencentCos => "tencentCos",
            StorageProtocol::JdCloudOss => "jdCloudOss",
            StorageProtocol::QiniuKodo => "qiniuKodo",
            StorageProtocol::Upyun => "upyun",
            StorageProtocol::WebdavHttps => "webdavHttps",
            StorageProtocol::WebdavHttp => "webdavHttp",
            StorageProtocol::Dropbox => "dropbox",
            StorageProtocol::OneDrive => "oneDrive",
            StorageProtocol::GoogleDrive => "googleDrive",
            StorageProtocol::AliyunDrive => "aliyunDrive",
            StorageProtocol::Box => "box",
            StorageProtocol::BaiduNetdisk => "baiduNetdisk",
            StorageProtocol::Smb => "smb",
            StorageProtocol::Afp => "afp",
            StorageProtocol::Nfs => "nfs",
        }
    }

    /// 该协议需要 OAuth 授权流程。
    pub fn is_oauth(self) -> bool {
        matches!(
            self,
            StorageProtocol::Dropbox
                | StorageProtocol::OneDrive
                | StorageProtocol::GoogleDrive
                | StorageProtocol::AliyunDrive
                | StorageProtocol::Box
                | StorageProtocol::BaiduNetdisk
        )
    }

    /// 该协议通过系统挂载实现(AFP / NFS),需要挂载点管理。
    pub fn is_system_mount(self) -> bool {
        matches!(self, StorageProtocol::Afp | StorageProtocol::Nfs)
    }

    /// 归属 `secrets` 的配置键 —— 这些键永不写入公开文件。
    pub fn secret_keys(self) -> &'static [&'static str] {
        match self {
            StorageProtocol::S3
            | StorageProtocol::S3Compatible
            | StorageProtocol::AliyunOss
            | StorageProtocol::TencentCos
            | StorageProtocol::JdCloudOss
            | StorageProtocol::QiniuKodo => &["accessKeyId", "secretAccessKey", "sessionToken"],
            StorageProtocol::Upyun => &["operator", "password"],
            StorageProtocol::WebdavHttps | StorageProtocol::WebdavHttp => &["username", "password"],
            StorageProtocol::Dropbox
            | StorageProtocol::OneDrive
            | StorageProtocol::GoogleDrive
            | StorageProtocol::AliyunDrive
            | StorageProtocol::Box
            | StorageProtocol::BaiduNetdisk => &[
                "accessToken",
                "refreshToken",
                "clientId",
                "clientSecret",
                "expiresAtMs",
            ],
            StorageProtocol::Smb => &["username", "password"],
            StorageProtocol::Afp => &["username", "password"],
            StorageProtocol::Nfs => &[],
        }
    }
}

/// 一条存储连接。`config` 为公开配置,`secrets` 为凭据(分文件存放)。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StorageConnection {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub protocol: StorageProtocol,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    /// 凭据。持久化时被剥离到 `storage-secrets.json`,读取时重新合并。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "lastConnectedAt", skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<i64>,
}

fn storage_connections_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("storage-connections.json"))
}

fn storage_secrets_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("storage-secrets.json"))
}

static STORAGE_CONNECTIONS_LOCK: Mutex<()> = Mutex::new(());

type SecretMap = BTreeMap<String, BTreeMap<String, String>>;

/// 把凭据从连接列表里剥离出来,返回(公开连接,按 id 分组的凭据)。
fn split_secrets(connections: Vec<StorageConnection>) -> (Vec<StorageConnection>, SecretMap) {
    let mut public = Vec::with_capacity(connections.len());
    let mut secrets: SecretMap = BTreeMap::new();
    for mut connection in connections {
        let taken = std::mem::take(&mut connection.secrets);
        let allowed = connection.protocol.secret_keys();
        let kept: BTreeMap<String, String> = taken
            .into_iter()
            .filter(|(key, value)| !value.is_empty() && allowed.contains(&key.as_str()))
            .collect();
        if !kept.is_empty() {
            secrets.insert(connection.id.clone(), kept);
        }
        public.push(connection);
    }
    (public, secrets)
}

/// 合并编辑表单没有回传的旧凭据，并把结果限制在当前协议声明的白名单内。
fn merge_connection_secrets(next: &mut StorageConnection, existing: Option<&StorageConnection>) {
    let allowed = next.protocol.secret_keys();
    next.secrets
        .retain(|key, value| !value.is_empty() && allowed.contains(&key.as_str()));

    let Some(existing) = existing.filter(|item| item.protocol == next.protocol) else {
        return;
    };
    for (key, value) in &existing.secrets {
        if allowed.contains(&key.as_str())
            && next.secrets.get(key).map(String::is_empty).unwrap_or(true)
        {
            next.secrets.insert(key.clone(), value.clone());
        }
    }
}

fn load_secrets() -> Result<SecretMap, String> {
    let path = storage_secrets_path()?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    crate::storage::ensure_private_file_permissions(&path)?;
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn read_public_connections() -> Result<Vec<StorageConnection>, String> {
    let path = storage_connections_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    crate::storage::ensure_private_file_permissions(&path)?;
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// 读取连接并合并凭据。仅供后端内部使用 —— 凭据不应无条件回传前端。
pub(crate) fn load_connections_with_secrets() -> Result<Vec<StorageConnection>, String> {
    let _guard = STORAGE_CONNECTIONS_LOCK.lock();
    load_connections_with_secrets_unlocked()
}

fn load_connections_with_secrets_unlocked() -> Result<Vec<StorageConnection>, String> {
    let mut connections = read_public_connections()?;
    let secrets = load_secrets()?;
    for connection in &mut connections {
        if let Some(entry) = secrets.get(&connection.id) {
            let allowed = connection.protocol.secret_keys();
            connection.secrets = entry
                .iter()
                .filter(|(key, value)| !value.is_empty() && allowed.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
        }
    }
    Ok(connections)
}

/// 按 id 取出单条连接(含凭据)。
pub(crate) fn find_connection(connection_id: &str) -> Result<StorageConnection, String> {
    load_connections_with_secrets()?
        .into_iter()
        .find(|item| item.id == connection_id)
        .ok_or_else(|| format!("Storage connection not found: {connection_id}"))
}

fn write_connections(connections: Vec<StorageConnection>) -> Result<(), String> {
    crate::storage::ensure_aeroric_dirs()?;
    let (public, secrets) = split_secrets(connections);
    let public_raw = serde_json::to_string_pretty(&public).map_err(|e| e.to_string())?;
    let secret_raw = serde_json::to_string_pretty(&secrets).map_err(|e| e.to_string())?;
    crate::storage::atomic_write_private(&storage_secrets_path()?, &format!("{secret_raw}\n"))?;
    crate::storage::atomic_write_private(&storage_connections_path()?, &format!("{public_raw}\n"))
}

/// 校验一条连接的必填字段。凭据可以为空(OAuth 连接在授权前尚无 token)。
pub(crate) fn validate_connection(connection: &StorageConnection) -> Result<(), String> {
    if connection.id.trim().is_empty() {
        return Err("Connection id is required".to_string());
    }
    if connection.name.trim().is_empty() {
        return Err("Connection name is required".to_string());
    }
    for key in required_config_keys(connection.protocol) {
        if connection
            .config
            .get(*key)
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
        {
            return Err(format!(
                "{} requires \"{key}\"",
                connection.protocol.as_str()
            ));
        }
    }
    Ok(())
}

/// 各协议的必填公开配置键。凭据类字段不在此列(单独校验时机不同)。
pub(crate) fn required_config_keys(protocol: StorageProtocol) -> &'static [&'static str] {
    match protocol {
        StorageProtocol::S3 => &["bucket", "region"],
        StorageProtocol::S3Compatible => &["bucket", "endpoint"],
        StorageProtocol::AliyunOss => &["bucket", "endpoint"],
        StorageProtocol::TencentCos => &["bucket", "endpoint"],
        StorageProtocol::JdCloudOss => &["bucket", "endpoint"],
        StorageProtocol::QiniuKodo => &["bucket", "endpoint"],
        StorageProtocol::Upyun => &["bucket"],
        StorageProtocol::WebdavHttps | StorageProtocol::WebdavHttp => &["endpoint"],
        StorageProtocol::Dropbox
        | StorageProtocol::OneDrive
        | StorageProtocol::GoogleDrive
        | StorageProtocol::AliyunDrive
        | StorageProtocol::Box
        | StorageProtocol::BaiduNetdisk => &[],
        StorageProtocol::Smb => &["host", "share"],
        StorageProtocol::Afp => &["host", "share"],
        StorageProtocol::Nfs => &["host", "export"],
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// 列出连接。**不含凭据** —— 前端只需要知道某个键是否已设置。
#[tauri::command]
pub async fn storage_list_connections() -> Result<Vec<StorageConnection>, String> {
    tokio::task::spawn_blocking(|| {
        let _guard = STORAGE_CONNECTIONS_LOCK.lock();
        read_public_connections()
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 返回每条连接已设置的凭据键名(不含值),供表单显示"已保存"状态。
#[tauri::command]
pub async fn storage_secret_keys() -> Result<BTreeMap<String, Vec<String>>, String> {
    tokio::task::spawn_blocking(|| {
        let _guard = STORAGE_CONNECTIONS_LOCK.lock();
        Ok(load_connections_with_secrets_unlocked()?
            .into_iter()
            .map(|connection| {
                (
                    connection.id,
                    connection.secrets.into_keys().collect::<Vec<_>>(),
                )
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 新增或更新一条连接。`secrets` 中为空字符串的键表示"保留原值"。
#[tauri::command]
pub async fn storage_save_connection(
    connection: StorageConnection,
) -> Result<Vec<StorageConnection>, String> {
    tokio::task::spawn_blocking(move || {
        validate_connection(&connection)?;
        let _guard = STORAGE_CONNECTIONS_LOCK.lock();
        let mut connections = load_connections_with_secrets_unlocked()?;
        let mut next = connection;
        let existing = connections.iter().find(|item| item.id == next.id);
        merge_connection_secrets(&mut next, existing);
        if let Some(existing) = existing {
            if next.created_at == 0 {
                next.created_at = existing.created_at;
            }
            if existing.protocol.is_system_mount()
                && (existing.protocol != next.protocol
                    || existing.config != next.config
                    || existing.secrets != next.secrets)
            {
                crate::storage_backend_mount::unmount(&existing.id)?;
            }
        }
        match connections.iter_mut().find(|item| item.id == next.id) {
            Some(slot) => *slot = next,
            None => connections.push(next),
        }
        write_connections(connections.clone())?;
        Ok(split_secrets(connections).0)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除一条连接及其凭据。
#[tauri::command]
pub async fn storage_delete_connection(
    connection_id: String,
) -> Result<Vec<StorageConnection>, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = STORAGE_CONNECTIONS_LOCK.lock();
        let mut connections = load_connections_with_secrets_unlocked()?;
        if let Some(existing) = connections.iter().find(|item| item.id == connection_id) {
            if existing.protocol.is_system_mount() {
                crate::storage_backend_mount::unmount(&existing.id)?;
            }
        }
        connections.retain(|item| item.id != connection_id);
        write_connections(connections.clone())?;
        Ok(split_secrets(connections).0)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 记录最近连接时间。
#[tauri::command]
pub async fn storage_touch_connection(connection_id: String, timestamp: i64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _guard = STORAGE_CONNECTIONS_LOCK.lock();
        let mut connections = load_connections_with_secrets_unlocked()?;
        let Some(slot) = connections.iter_mut().find(|item| item.id == connection_id) else {
            return Ok(());
        };
        slot.last_connected_at = Some(timestamp);
        write_connections(connections)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(protocol: StorageProtocol) -> StorageConnection {
        StorageConnection {
            id: "conn-1".to_string(),
            name: "Bucket".to_string(),
            group: None,
            protocol,
            config: BTreeMap::from([
                ("bucket".to_string(), "media".to_string()),
                ("region".to_string(), "us-east-1".to_string()),
            ]),
            secrets: BTreeMap::from([
                ("accessKeyId".to_string(), "AKIA".to_string()),
                ("secretAccessKey".to_string(), "s3cr3t".to_string()),
            ]),
            created_at: 1,
            last_connected_at: None,
        }
    }

    #[test]
    fn split_secrets_strips_credentials_from_public_connections() {
        let (public, secrets) = split_secrets(vec![connection(StorageProtocol::S3)]);
        assert!(public[0].secrets.is_empty());
        assert_eq!(secrets["conn-1"]["accessKeyId"], "AKIA");
        assert_eq!(secrets["conn-1"]["secretAccessKey"], "s3cr3t");
    }

    #[test]
    fn split_secrets_drops_empty_values() {
        let mut input = connection(StorageProtocol::S3);
        input
            .secrets
            .insert("sessionToken".to_string(), String::new());
        let (_, secrets) = split_secrets(vec![input]);
        assert!(!secrets["conn-1"].contains_key("sessionToken"));
    }

    #[test]
    fn split_secrets_drops_keys_not_owned_by_the_protocol() {
        let mut input = connection(StorageProtocol::S3);
        input
            .secrets
            .insert("refreshToken".to_string(), "stale-oauth-token".to_string());
        input
            .secrets
            .insert("unexpected".to_string(), "not-allowed".to_string());
        let (_, secrets) = split_secrets(vec![input]);
        assert!(!secrets["conn-1"].contains_key("refreshToken"));
        assert!(!secrets["conn-1"].contains_key("unexpected"));
    }

    #[test]
    fn merge_secrets_preserves_same_protocol_but_drops_cross_protocol_credentials() {
        let existing = connection(StorageProtocol::S3);
        let mut same_protocol = connection(StorageProtocol::S3);
        same_protocol.secrets.clear();
        merge_connection_secrets(&mut same_protocol, Some(&existing));
        assert_eq!(same_protocol.secrets["accessKeyId"], "AKIA");

        let mut switched = connection(StorageProtocol::WebdavHttps);
        switched.config = BTreeMap::from([(
            "endpoint".to_string(),
            "https://dav.example.com".to_string(),
        )]);
        switched.secrets.clear();
        merge_connection_secrets(&mut switched, Some(&existing));
        assert!(switched.secrets.is_empty());
    }

    #[test]
    fn public_connection_json_never_contains_secret_values() {
        let (public, _) = split_secrets(vec![connection(StorageProtocol::S3)]);
        let raw = serde_json::to_string(&public).unwrap();
        assert!(!raw.contains("s3cr3t"));
        assert!(!raw.contains("AKIA"));
        assert!(!raw.contains("secrets"));
    }

    #[test]
    fn validate_connection_requires_configured_keys() {
        let mut input = connection(StorageProtocol::S3);
        input.config.remove("region");
        assert!(validate_connection(&input).is_err());
        input.config.insert("region".to_string(), "  ".to_string());
        assert!(validate_connection(&input).is_err());
    }

    #[test]
    fn validate_connection_accepts_oauth_without_tokens() {
        let mut input = connection(StorageProtocol::Dropbox);
        input.config.clear();
        input.secrets.clear();
        assert!(validate_connection(&input).is_ok());
    }

    #[test]
    fn validate_connection_rejects_blank_identity() {
        let mut input = connection(StorageProtocol::S3);
        input.name = "   ".to_string();
        assert!(validate_connection(&input).is_err());
        input.name = "Bucket".to_string();
        input.id = String::new();
        assert!(validate_connection(&input).is_err());
    }

    #[test]
    fn protocol_round_trips_through_json() {
        for protocol in StorageProtocol::ALL {
            let raw = serde_json::to_string(&protocol).unwrap();
            let parsed: StorageProtocol = serde_json::from_str(&raw).unwrap();
            assert_eq!(protocol, parsed);
        }
    }

    #[test]
    fn oauth_protocols_are_exactly_the_six_token_based_services() {
        let oauth: Vec<&str> = StorageProtocol::ALL
            .into_iter()
            .filter(|protocol| protocol.is_oauth())
            .map(StorageProtocol::as_str)
            .collect();
        assert_eq!(
            oauth,
            vec![
                "dropbox",
                "oneDrive",
                "googleDrive",
                "aliyunDrive",
                "box",
                "baiduNetdisk"
            ]
        );
    }

    #[test]
    fn system_mount_protocols_are_afp_and_nfs() {
        let mounts: Vec<&str> = StorageProtocol::ALL
            .into_iter()
            .filter(|protocol| protocol.is_system_mount())
            .map(StorageProtocol::as_str)
            .collect();
        assert_eq!(mounts, vec!["afp", "nfs"]);
    }
}
