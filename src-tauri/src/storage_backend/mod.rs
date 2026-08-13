//! 存储后端抽象:把 18 种协议归一到一组文件操作上,供 `sftp.rs` 的
//! `SftpEndpoint::Storage` 分支调用。
//!
//! 不同协议能力差异很大(对象存储没有真正的目录、网盘不支持服务端 rename 等),
//! 所以每个后端都要声明自己的 [`Capability`] 位,前端据此禁用不支持的动作,
//! 而不是让用户点了之后收到一个后端错误。

use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::storage_conn::{StorageConnection, StorageProtocol};

/// 后端支持的能力位。前端用它 gate 工具栏按钮与右键菜单。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    /// 能列目录。所有后端都必须支持。
    pub read_dir: bool,
    /// 能读文件内容(文本预览 / 图片预览 / 下载)。
    pub read: bool,
    /// 能写文件。
    pub write: bool,
    /// 能创建目录。对象存储通过写 `dir/` 占位对象模拟。
    pub create_dir: bool,
    /// 能删除。
    pub delete: bool,
    /// 能重命名(多数对象存储需要 copy + delete 模拟)。
    pub rename: bool,
    /// 能在同一后端内复制。
    pub copy: bool,
    /// 能取单个条目的元信息。
    pub stat: bool,
    /// 目录条目自带大小/时间(对象存储 list 通常带,挂载类一定带)。
    pub rich_metadata: bool,
}

impl Capability {
    /// 完整的 POSIX 式能力(挂载类后端、WebDAV)。
    pub const FULL: Capability = Capability {
        read_dir: true,
        read: true,
        write: true,
        create_dir: true,
        delete: true,
        rename: true,
        copy: true,
        stat: true,
        rich_metadata: true,
    };

    /// 对象存储:目录是前缀模拟,rename/copy 由 copy+delete 实现。
    pub const OBJECT_STORE: Capability = Capability {
        read_dir: true,
        read: true,
        write: true,
        create_dir: true,
        delete: true,
        rename: true,
        copy: true,
        stat: true,
        rich_metadata: true,
    };
}

/// 归一后的目录条目。字段与 `SftpEntry` 对齐,便于直接转换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

/// 单个条目的元信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StorageStat {
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

/// 所有存储后端的统一接口。
///
/// 实现者只需处理"路径 → 操作",鉴权与客户端构造在 [`build_backend`] 里完成。
/// 方法为同步签名,由调用方放进 `spawn_blocking`,与现有 SFTP 代码风格一致。
pub trait StorageBackend: Send + Sync {
    fn capability(&self) -> Capability;

    fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String>;

    fn read(&self, path: &str) -> Result<Vec<u8>, String>;

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String>;

    fn create_dir(&self, path: &str) -> Result<(), String>;

    fn delete(&self, path: &str) -> Result<(), String>;

    fn rename(&self, from: &str, to: &str) -> Result<(), String>;

    fn copy(&self, from: &str, to: &str) -> Result<(), String>;

    fn stat(&self, path: &str) -> Result<StorageStat, String>;
}

/// 协议展示名(错误信息用;用户可见文案仍走前端 i18n)。
pub fn protocol_display_name(protocol: StorageProtocol) -> &'static str {
    match protocol {
        StorageProtocol::S3 => "Amazon S3",
        StorageProtocol::S3Compatible => "S3-compatible storage",
        StorageProtocol::AliyunOss => "Alibaba Cloud OSS",
        StorageProtocol::TencentCos => "Tencent Cloud COS",
        StorageProtocol::JdCloudOss => "JD Cloud OSS",
        StorageProtocol::QiniuKodo => "Qiniu KODO",
        StorageProtocol::Upyun => "Upyun USS",
        StorageProtocol::WebdavHttps => "WebDAV (HTTPS)",
        StorageProtocol::WebdavHttp => "WebDAV (HTTP)",
        StorageProtocol::Dropbox => "Dropbox",
        StorageProtocol::OneDrive => "OneDrive",
        StorageProtocol::GoogleDrive => "Google Drive",
        StorageProtocol::AliyunDrive => "Aliyun Drive",
        StorageProtocol::Box => "Box",
        StorageProtocol::BaiduNetdisk => "Baidu Netdisk",
        StorageProtocol::Smb => "SMB",
        StorageProtocol::Afp => "AFP",
        StorageProtocol::Nfs => "NFS",
    }
}

/// 声明式能力表。即使后端尚未接线,前端也能拿到正确的能力位。
pub fn capability_for(protocol: StorageProtocol) -> Capability {
    match protocol {
        StorageProtocol::S3
        | StorageProtocol::S3Compatible
        | StorageProtocol::AliyunOss
        | StorageProtocol::TencentCos
        | StorageProtocol::JdCloudOss
        | StorageProtocol::QiniuKodo
        | StorageProtocol::Upyun => Capability::OBJECT_STORE,
        StorageProtocol::WebdavHttps | StorageProtocol::WebdavHttp => Capability::FULL,
        StorageProtocol::Dropbox
        | StorageProtocol::OneDrive
        | StorageProtocol::GoogleDrive
        | StorageProtocol::AliyunDrive
        | StorageProtocol::Box
        | StorageProtocol::BaiduNetdisk => Capability::FULL,
        StorageProtocol::Smb => Capability::FULL,
        StorageProtocol::Afp | StorageProtocol::Nfs => Capability::FULL,
    }
}

/// 归一存储路径:统一为以 `/` 开头、不以 `/` 结尾(根除外)的绝对路径。
pub fn normalize_storage_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return "/".to_string();
    }
    format!("/{}", parts.join("/"))
}

/// 校验会破坏数据的操作目标。连接根目录只用于浏览，永远不能作为删除目标。
///
/// 这个检查放在共享后端层，避免前端回归或直接 IPC 调用绕过 UI 限制。
pub fn validate_storage_mutation_path(path: &str) -> Result<String, String> {
    let normalized = normalize_storage_path(path);
    if normalized == "/" {
        return Err("Refusing to modify the storage root".to_string());
    }
    Ok(normalized)
}

/// 从路径取末段名称。
pub fn path_basename(path: &str) -> String {
    let normalized = normalize_storage_path(path);
    if normalized == "/" {
        return "/".to_string();
    }
    normalized
        .rsplit('/')
        .next()
        .unwrap_or(&normalized)
        .to_string()
}

/// 取父目录。
pub fn path_parent(path: &str) -> String {
    let normalized = normalize_storage_path(path);
    if normalized == "/" {
        return "/".to_string();
    }
    match normalized.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => normalized[..index].to_string(),
    }
}

/// 拼接父目录与条目名。
pub fn join_storage_path(parent: &str, name: &str) -> String {
    let base = normalize_storage_path(parent);
    if base == "/" {
        return normalize_storage_path(&format!("/{name}"));
    }
    normalize_storage_path(&format!("{base}/{name}"))
}

/// `SystemTime` → 毫秒时间戳。
pub fn system_time_to_ms(time: std::time::SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_millis() as u64)
}

/// 单个协议的元信息,驱动前端的协议选择器与动态表单。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolDescriptor {
    pub protocol: StorageProtocol,
    /// 英文展示名。用户可见文案由前端 i18n 决定,这里仅作兜底。
    pub display_name: String,
    pub capability: Capability,
    /// 必填的公开配置键。
    pub required_config_keys: Vec<String>,
    /// 归属凭据(写入 0600 secrets 文件)的键。
    pub secret_keys: Vec<String>,
    /// 端点占位符(部分厂商有固定 S3 兼容端点)。
    pub default_endpoint: Option<String>,
    /// 是否需要 OAuth 授权。
    pub oauth: bool,
    /// 是否通过系统挂载实现(需要 mount 权限)。
    pub system_mount: bool,
    /// 协议已被平台标记废弃(AFP)。
    pub deprecated: bool,
}

/// 全部 18 种协议的元信息。
pub fn protocol_descriptors() -> Vec<ProtocolDescriptor> {
    StorageProtocol::ALL
        .into_iter()
        .map(|protocol| ProtocolDescriptor {
            protocol,
            display_name: protocol_display_name(protocol).to_string(),
            capability: capability_for(protocol),
            required_config_keys: crate::storage_conn::required_config_keys(protocol)
                .iter()
                .map(|key| (*key).to_string())
                .collect(),
            secret_keys: protocol
                .secret_keys()
                .iter()
                .map(|key| (*key).to_string())
                .collect(),
            default_endpoint: crate::storage_backend_opendal::default_endpoint(protocol)
                .map(str::to_string),
            oauth: protocol.is_oauth(),
            system_mount: protocol.is_system_mount(),
            deprecated: crate::storage_backend_mount::is_deprecated(protocol),
        })
        .collect()
}

/// 按连接构造后端实例。各协议实现随阶段逐步接入。
pub fn build_backend(connection: &StorageConnection) -> Result<Box<dyn StorageBackend>, String> {
    match connection.protocol {
        StorageProtocol::S3
        | StorageProtocol::S3Compatible
        | StorageProtocol::AliyunOss
        | StorageProtocol::TencentCos
        | StorageProtocol::JdCloudOss
        | StorageProtocol::QiniuKodo
        | StorageProtocol::Upyun
        | StorageProtocol::WebdavHttps
        | StorageProtocol::WebdavHttp
        | StorageProtocol::Dropbox
        | StorageProtocol::OneDrive
        | StorageProtocol::GoogleDrive
        | StorageProtocol::AliyunDrive => super::storage_backend_opendal::build(connection),
        StorageProtocol::Box => super::storage_backend_box::build(connection),
        StorageProtocol::BaiduNetdisk => super::storage_backend_baidu::build(connection),
        StorageProtocol::Smb => super::storage_backend_smb::build(connection),
        StorageProtocol::Afp | StorageProtocol::Nfs => {
            super::storage_backend_mount::build(connection)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_storage_path_collapses_and_resolves() {
        assert_eq!(normalize_storage_path(""), "/");
        assert_eq!(normalize_storage_path("/"), "/");
        assert_eq!(normalize_storage_path("a/b"), "/a/b");
        assert_eq!(normalize_storage_path("/a//b/"), "/a/b");
        assert_eq!(normalize_storage_path("/a/./b"), "/a/b");
        assert_eq!(normalize_storage_path("/a/b/../c"), "/a/c");
    }

    #[test]
    fn normalize_storage_path_cannot_escape_root() {
        assert_eq!(normalize_storage_path("/../../etc/passwd"), "/etc/passwd");
        assert_eq!(normalize_storage_path("../.."), "/");
        assert_eq!(normalize_storage_path("/a/../.."), "/");
    }

    #[test]
    fn mutation_paths_reject_every_root_alias() {
        for path in ["/", "/.", "/a/..", "../.."] {
            assert!(validate_storage_mutation_path(path).is_err(), "{path}");
        }
        assert_eq!(validate_storage_mutation_path("/a/../b").unwrap(), "/b");
    }

    #[test]
    fn basename_and_parent_handle_root() {
        assert_eq!(path_basename("/a/b.txt"), "b.txt");
        assert_eq!(path_basename("/"), "/");
        assert_eq!(path_parent("/a/b.txt"), "/a");
        assert_eq!(path_parent("/a"), "/");
        assert_eq!(path_parent("/"), "/");
    }

    #[test]
    fn join_storage_path_normalizes_result() {
        assert_eq!(join_storage_path("/", "a"), "/a");
        assert_eq!(join_storage_path("/a", "b"), "/a/b");
        assert_eq!(join_storage_path("/a/", "b"), "/a/b");
    }

    #[test]
    fn every_protocol_can_at_least_read() {
        for protocol in StorageProtocol::ALL {
            let capability = capability_for(protocol);
            assert!(capability.read_dir, "{} cannot read_dir", protocol.as_str());
            assert!(capability.read, "{} cannot read", protocol.as_str());
        }
    }

    #[test]
    fn every_protocol_has_a_display_name() {
        for protocol in StorageProtocol::ALL {
            assert!(!protocol_display_name(protocol).is_empty());
        }
    }
}
