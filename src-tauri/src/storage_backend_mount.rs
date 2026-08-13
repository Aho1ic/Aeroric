//! AFP / NFS 后端:通过系统 mount 挂载,再复用本地文件系统访问。
//!
//! 挂载点固定在 `~/.aeroric/mounts/<connection-id>` 下。这里**必须**自己做
//! 路径白名单校验 —— 这些路径绕过了 `sftp.rs::validate_sftp_local_path` 的
//! 敏感目录黑名单,如果允许 `..` 逃逸,用户就能借挂载点读写任意本地文件。
//!
//! AFP 说明:macOS 的 AFP 客户端在 Sequoia 15.5 起标记废弃,并在 macOS 27
//! 移除。UI 需要给出提示,建议用户改用 SMB。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::storage_backend::{
    join_storage_path, normalize_storage_path, system_time_to_ms, Capability, StorageBackend,
    StorageEntry, StorageStat,
};
use crate::storage_conn::{StorageConnection, StorageProtocol};

/// 挂载点根目录:`~/.aeroric/mounts`。
pub(crate) fn mounts_root() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("mounts"))
}

/// 连接 id 只允许安全字符,避免拼出 `../` 之类的挂载点。
pub(crate) fn validate_connection_id(connection_id: &str) -> Result<(), String> {
    if connection_id.is_empty() || connection_id.len() > 128 {
        return Err("Invalid connection id".to_string());
    }
    if !connection_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("Invalid connection id".to_string());
    }
    Ok(())
}

/// 某连接的挂载点路径。
pub(crate) fn mount_point_for(connection_id: &str) -> Result<PathBuf, String> {
    validate_connection_id(connection_id)?;
    Ok(mounts_root()?.join(connection_id))
}

/// 校验一个绝对路径确实落在挂载点内部。
///
/// 这是本模块的安全核心:先归一化(消掉 `.` 与 `..`),再确认前缀。
/// 归一化在字符串层完成,不依赖文件存在,因此对尚未创建的目标路径同样有效。
pub(crate) fn resolve_within_mount(mount_point: &Path, path: &str) -> Result<PathBuf, String> {
    let normalized = normalize_storage_path(path);
    let relative = normalized.trim_start_matches('/');
    let candidate = if relative.is_empty() {
        mount_point.to_path_buf()
    } else {
        mount_point.join(relative)
    };
    // normalize_storage_path 已消除 `..`,这里再核对一次前缀,双重保险。
    if !candidate.starts_with(mount_point) {
        return Err("Path escapes the mount point".to_string());
    }
    for component in candidate.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err("Path escapes the mount point".to_string());
        }
    }
    Ok(candidate)
}

pub struct MountBackend {
    mount_point: PathBuf,
}

impl StorageBackend for MountBackend {
    fn capability(&self) -> Capability {
        Capability::FULL
    }

    fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String> {
        let parent = normalize_storage_path(path);
        let dir = resolve_within_mount(&self.mount_point, &parent)?;
        let mut entries = Vec::new();
        for item in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let item = item.map_err(|e| e.to_string())?;
            let name = item.file_name().to_string_lossy().to_string();
            let metadata = match item.metadata() {
                Ok(metadata) => metadata,
                // 挂载卷上偶发的失效条目不应让整个列目录失败。
                Err(_) => continue,
            };
            let is_dir = metadata.is_dir();
            entries.push(StorageEntry {
                path: join_storage_path(&parent, &name),
                name,
                is_dir,
                size: if is_dir { None } else { Some(metadata.len()) },
                modified_at_ms: metadata.modified().ok().and_then(system_time_to_ms),
            });
        }
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let target = resolve_within_mount(&self.mount_point, path)?;
        std::fs::read(target).map_err(|e| e.to_string())
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let target = resolve_within_mount(&self.mount_point, path)?;
        std::fs::write(target, bytes).map_err(|e| e.to_string())
    }

    fn create_dir(&self, path: &str) -> Result<(), String> {
        let target = resolve_within_mount(&self.mount_point, path)?;
        std::fs::create_dir_all(target).map_err(|e| e.to_string())
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let target = resolve_within_mount(&self.mount_point, path)?;
        if target == self.mount_point {
            return Err("Refusing to delete the mount point".to_string());
        }
        let metadata = std::fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            std::fs::remove_dir_all(target).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(target).map_err(|e| e.to_string())
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let source = resolve_within_mount(&self.mount_point, from)?;
        let destination = resolve_within_mount(&self.mount_point, to)?;
        if destination.exists() {
            return Err("A file or folder with that name already exists".to_string());
        }
        std::fs::rename(source, destination).map_err(|e| e.to_string())
    }

    fn copy(&self, from: &str, to: &str) -> Result<(), String> {
        let source = resolve_within_mount(&self.mount_point, from)?;
        let destination = resolve_within_mount(&self.mount_point, to)?;
        copy_recursive(&source, &destination)
    }

    fn stat(&self, path: &str) -> Result<StorageStat, String> {
        let target = resolve_within_mount(&self.mount_point, path)?;
        let metadata = std::fs::metadata(target).map_err(|e| e.to_string())?;
        let is_dir = metadata.is_dir();
        Ok(StorageStat {
            is_dir,
            size: if is_dir { None } else { Some(metadata.len()) },
            modified_at_ms: metadata.modified().ok().and_then(system_time_to_ms),
        })
    }
}

fn copy_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if !metadata.is_dir() {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(source, destination).map_err(|e| e.to_string())?;
        return Ok(());
    }
    std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        copy_recursive(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

/// 判断路径当前是否已是挂载点。
pub(crate) fn is_mounted(mount_point: &Path) -> bool {
    if !mount_point.is_dir() {
        return false;
    }
    let Ok(output) = Command::new("mount").output() else {
        return false;
    };
    let listing = String::from_utf8_lossy(&output.stdout);
    let needle = mount_point.to_string_lossy();
    listing.lines().any(|line| {
        line.contains(&format!(" on {needle} ")) || line.contains(&format!(" {needle} "))
    })
}

/// 构造挂载命令。macOS 用 `mount_afp` / `mount_nfs`,Linux 用 `mount -t nfs`。
///
/// 返回 `(program, args)`。凭据以 URL 形式传给 `mount_afp`,因此调用方不能把
/// 该命令写进日志。
pub(crate) fn build_mount_command(
    protocol: StorageProtocol,
    connection: &StorageConnection,
    mount_point: &Path,
) -> Result<(String, Vec<String>), String> {
    let host = connection
        .config
        .get("host")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "\"host\" is required".to_string())?;
    if host.contains('/') || host.contains(' ') {
        return Err(format!("Invalid host: {host}"));
    }
    let mount_point = mount_point.to_string_lossy().to_string();

    match protocol {
        StorageProtocol::Afp => {
            if !cfg!(target_os = "macos") {
                return Err("AFP mounting is only supported on macOS".to_string());
            }
            let share = connection
                .config
                .get("share")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "\"share\" is required".to_string())?;
            if share.contains('/') {
                return Err(format!("Invalid share name: {share}"));
            }
            let credentials = match (
                connection.secrets.get("username").filter(|v| !v.is_empty()),
                connection.secrets.get("password").filter(|v| !v.is_empty()),
            ) {
                (Some(username), Some(password)) => format!(
                    "{}:{}@",
                    urlencode_component(username),
                    urlencode_component(password)
                ),
                (Some(username), None) => format!("{}@", urlencode_component(username)),
                _ => String::new(),
            };
            Ok((
                "mount_afp".to_string(),
                vec![format!("afp://{credentials}{host}/{share}"), mount_point],
            ))
        }
        StorageProtocol::Nfs => {
            let export = connection
                .config
                .get("export")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "\"export\" is required".to_string())?;
            if !export.starts_with('/') {
                return Err("\"export\" must be an absolute path".to_string());
            }
            let source = format!("{host}:{export}");
            if cfg!(target_os = "macos") {
                Ok((
                    "mount_nfs".to_string(),
                    vec![
                        "-o".to_string(),
                        "nolocks,resvport".to_string(),
                        source,
                        mount_point,
                    ],
                ))
            } else {
                Ok((
                    "mount".to_string(),
                    vec!["-t".to_string(), "nfs".to_string(), source, mount_point],
                ))
            }
        }
        other => Err(format!("{} is not a mount protocol", other.as_str())),
    }
}

/// 卸载命令。macOS 与 Linux 都用 `umount <mount_point>`。
pub(crate) fn build_unmount_command(mount_point: &Path) -> (String, Vec<String>) {
    let target = mount_point.to_string_lossy().to_string();
    ("umount".to_string(), vec![target])
}

/// 最小化的 URL 组件转义,足以安全承载用户名/密码里的特殊字符。
fn urlencode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 确保挂载存在,必要时执行挂载命令。
pub(crate) fn ensure_mounted(connection: &StorageConnection) -> Result<PathBuf, String> {
    let mount_point = mount_point_for(&connection.id)?;
    std::fs::create_dir_all(&mount_point).map_err(|e| e.to_string())?;
    if is_mounted(&mount_point) {
        return Ok(mount_point);
    }
    let (program, args) = build_mount_command(connection.protocol, connection, &mount_point)?;
    let output = Command::new(&program)
        .args(&args)
        .output()
        .map_err(|error| format!("Failed to run {program}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        // 不回显命令本身 —— AFP 的 URL 里带明文凭据。
        return Err(if stderr.is_empty() {
            format!("{program} failed")
        } else {
            format!("{program} failed: {stderr}")
        });
    }
    Ok(mount_point)
}

/// 卸载并清理挂载点目录。
pub(crate) fn unmount(connection_id: &str) -> Result<(), String> {
    let mount_point = mount_point_for(connection_id)?;
    if !mount_point.exists() {
        return Ok(());
    }
    if is_mounted(&mount_point) {
        let (program, args) = build_unmount_command(&mount_point);
        let output = Command::new(&program)
            .args(&args)
            .output()
            .map_err(|error| format!("Failed to run {program}: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("{program} failed: {stderr}"));
        }
    }
    // 只删空目录:非空说明卸载没生效,不能递归删掉远端内容。
    let _ = std::fs::remove_dir(&mount_point);
    Ok(())
}

pub fn build(connection: &StorageConnection) -> Result<Box<dyn StorageBackend>, String> {
    let mount_point = ensure_mounted(connection)?;
    Ok(Box::new(MountBackend { mount_point }))
}

/// AFP 在新版 macOS 上已废弃,UI 需要提示用户。
pub fn is_deprecated(protocol: StorageProtocol) -> bool {
    matches!(protocol, StorageProtocol::Afp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn connection(protocol: StorageProtocol) -> StorageConnection {
        StorageConnection {
            id: "conn-1".to_string(),
            name: "NAS".to_string(),
            group: None,
            protocol,
            config: BTreeMap::from([("host".to_string(), "nas.local".to_string())]),
            secrets: BTreeMap::new(),
            created_at: 1,
            last_connected_at: None,
        }
    }

    #[test]
    fn resolve_within_mount_keeps_paths_inside() {
        let mount = PathBuf::from("/tmp/aeroric-mount");
        assert_eq!(
            resolve_within_mount(&mount, "/a/b.txt").unwrap(),
            mount.join("a/b.txt")
        );
        assert_eq!(resolve_within_mount(&mount, "/").unwrap(), mount);
    }

    #[test]
    fn resolve_within_mount_blocks_traversal() {
        let mount = PathBuf::from("/tmp/aeroric-mount");
        // 归一化后逃逸尝试被夹回挂载点内,绝不会指向 /etc/passwd。
        let resolved = resolve_within_mount(&mount, "/../../../etc/passwd").unwrap();
        assert!(resolved.starts_with(&mount));
        assert_eq!(resolved, mount.join("etc/passwd"));

        let resolved = resolve_within_mount(&mount, "/a/../../..").unwrap();
        assert_eq!(resolved, mount);
    }

    #[test]
    fn connection_ids_with_separators_are_rejected() {
        assert!(validate_connection_id("conn-1").is_ok());
        assert!(validate_connection_id("conn_1").is_ok());
        assert!(validate_connection_id("../etc").is_err());
        assert!(validate_connection_id("a/b").is_err());
        assert!(validate_connection_id("").is_err());
        assert!(validate_connection_id(&"a".repeat(129)).is_err());
    }

    #[test]
    fn mount_point_is_confined_to_the_mounts_directory() {
        let point = mount_point_for("conn-1").unwrap();
        assert!(point.ends_with("mounts/conn-1"));
        assert!(mount_point_for("../escape").is_err());
    }

    #[test]
    fn nfs_mount_command_requires_an_absolute_export() {
        let mut input = connection(StorageProtocol::Nfs);
        input
            .config
            .insert("export".to_string(), "volume1".to_string());
        let error =
            build_mount_command(StorageProtocol::Nfs, &input, Path::new("/tmp/m")).unwrap_err();
        assert!(error.contains("absolute"), "unexpected error: {error}");
    }

    #[test]
    fn nfs_mount_command_builds_host_colon_export() {
        let mut input = connection(StorageProtocol::Nfs);
        input
            .config
            .insert("export".to_string(), "/volume1/media".to_string());
        let (program, args) =
            build_mount_command(StorageProtocol::Nfs, &input, Path::new("/tmp/m")).unwrap();
        assert!(program == "mount_nfs" || program == "mount");
        assert!(args.contains(&"nas.local:/volume1/media".to_string()));
        assert!(args.contains(&"/tmp/m".to_string()));
    }

    #[test]
    fn mount_commands_reject_hosts_with_separators() {
        let mut input = connection(StorageProtocol::Nfs);
        input
            .config
            .insert("host".to_string(), "nas/evil".to_string());
        input
            .config
            .insert("export".to_string(), "/volume1".to_string());
        assert!(build_mount_command(StorageProtocol::Nfs, &input, Path::new("/tmp/m")).is_err());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn afp_mount_command_encodes_credentials_in_the_url() {
        let mut input = connection(StorageProtocol::Afp);
        input
            .config
            .insert("share".to_string(), "media".to_string());
        input
            .secrets
            .insert("username".to_string(), "user@corp".to_string());
        input
            .secrets
            .insert("password".to_string(), "p@ss/word".to_string());
        let (program, args) =
            build_mount_command(StorageProtocol::Afp, &input, Path::new("/tmp/m")).unwrap();
        assert_eq!(program, "mount_afp");
        // `@` 与 `/` 必须转义,否则会破坏 URL 结构。
        assert!(args[0].contains("user%40corp"));
        assert!(args[0].contains("p%40ss%2Fword"));
        assert!(args[0].ends_with("@nas.local/media"));
    }

    #[test]
    fn afp_is_flagged_as_deprecated() {
        assert!(is_deprecated(StorageProtocol::Afp));
        assert!(!is_deprecated(StorageProtocol::Nfs));
        assert!(!is_deprecated(StorageProtocol::Smb));
    }

    #[test]
    fn urlencode_component_escapes_reserved_characters() {
        assert_eq!(urlencode_component("plain-name_1.0~"), "plain-name_1.0~");
        assert_eq!(urlencode_component("a:b@c/d"), "a%3Ab%40c%2Fd");
        assert_eq!(urlencode_component(" "), "%20");
    }

    #[test]
    fn non_mount_protocols_are_rejected() {
        let input = connection(StorageProtocol::S3);
        assert!(build_mount_command(StorageProtocol::S3, &input, Path::new("/tmp/m")).is_err());
    }
}
