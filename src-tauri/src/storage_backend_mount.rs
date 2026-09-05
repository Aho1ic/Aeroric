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

use cap_std::ambient_authority;
use cap_std::fs::Dir;

use crate::storage_backend::{
    join_storage_path, normalize_storage_path, system_time_to_ms, validate_storage_mutation_path,
    Capability, StorageBackend, StorageEntry, StorageStat,
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

/// 把存储绝对路径转换为挂载目录句柄下的相对路径。
fn relative_mount_path(path: &str) -> PathBuf {
    let normalized = normalize_storage_path(path);
    let relative = normalized.trim_start_matches('/');
    if relative.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(relative)
    }
}

pub struct MountBackend {
    root: Dir,
}

impl MountBackend {
    fn open(mount_point: &Path) -> Result<Self, String> {
        let root = Dir::open_ambient_dir(mount_point, ambient_authority())
            .map_err(|error| format!("Cannot open mount point: {error}"))?;
        Ok(Self { root })
    }
}

impl StorageBackend for MountBackend {
    fn capability(&self) -> Capability {
        Capability::FULL
    }

    fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String> {
        let parent = normalize_storage_path(path);
        let dir = relative_mount_path(&parent);
        let mut entries = Vec::new();
        for item in self.root.read_dir(&dir).map_err(|e| e.to_string())? {
            let item = item.map_err(|e| e.to_string())?;
            let name = item.file_name().to_string_lossy().to_string();
            let metadata = item.metadata().map_err(|error| {
                format!(
                    "Cannot inspect mounted storage entry {}: {error}",
                    join_storage_path(&parent, &name)
                )
            })?;
            let is_dir = metadata.is_dir();
            entries.push(StorageEntry {
                path: join_storage_path(&parent, &name),
                name,
                is_dir,
                size: if is_dir { None } else { Some(metadata.len()) },
                modified_at_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|time| system_time_to_ms(time.into_std())),
            });
        }
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.root
            .read(relative_mount_path(path))
            .map_err(|e| e.to_string())
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.root
            .write(relative_mount_path(path), bytes)
            .map_err(|e| e.to_string())
    }

    fn create_dir(&self, path: &str) -> Result<(), String> {
        self.root
            .create_dir_all(relative_mount_path(path))
            .map_err(|e| e.to_string())
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let path = validate_storage_mutation_path(path)?;
        let target = relative_mount_path(&path);
        let metadata = self
            .root
            .symlink_metadata(&target)
            .map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            self.root.remove_dir_all(target).map_err(|e| e.to_string())
        } else {
            self.root.remove_file(target).map_err(|e| e.to_string())
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let from = validate_storage_mutation_path(from)?;
        let to = validate_storage_mutation_path(to)?;
        let source = relative_mount_path(&from);
        let destination = relative_mount_path(&to);
        // cap_std's rename is the capability-scoped equivalent of
        // std::fs::rename and replaces an existing file atomically where the
        // mounted filesystem supports POSIX rename semantics. The previous
        // existence check made repeated manifest commits fail deterministically.
        self.root
            .rename(source, &self.root, destination)
            .map_err(|e| e.to_string())
    }

    fn copy(&self, from: &str, to: &str) -> Result<(), String> {
        copy_recursive(
            &self.root,
            &relative_mount_path(from),
            &relative_mount_path(to),
        )
    }

    fn stat(&self, path: &str) -> Result<StorageStat, String> {
        let metadata = self
            .root
            .metadata(relative_mount_path(path))
            .map_err(|e| e.to_string())?;
        let is_dir = metadata.is_dir();
        Ok(StorageStat {
            is_dir,
            size: if is_dir { None } else { Some(metadata.len()) },
            modified_at_ms: metadata
                .modified()
                .ok()
                .and_then(|time| system_time_to_ms(time.into_std())),
        })
    }
}

fn copy_recursive(root: &Dir, source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = root.symlink_metadata(source).map_err(|e| e.to_string())?;
    if !metadata.is_dir() {
        if let Some(parent) = destination.parent() {
            root.create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        root.copy(source, root, destination)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    root.create_dir_all(destination)
        .map_err(|e| e.to_string())?;
    for entry in root.read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        copy_recursive(
            root,
            &source.join(entry.file_name()),
            &destination.join(entry.file_name()),
        )?;
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
    Ok(Box::new(MountBackend::open(&mount_point)?))
}

/// AFP 在新版 macOS 上已废弃,UI 需要提示用户。
pub fn is_deprecated(protocol: StorageProtocol) -> bool {
    matches!(protocol, StorageProtocol::Afp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aeroric-mount-{label}-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

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
    fn storage_paths_are_relative_to_the_open_mount_handle() {
        assert_eq!(relative_mount_path("/a/b.txt"), PathBuf::from("a/b.txt"));
        assert_eq!(relative_mount_path("/"), PathBuf::from("."));
        assert_eq!(
            relative_mount_path("/../../../etc/passwd"),
            PathBuf::from("etc/passwd")
        );
    }

    #[cfg(unix)]
    #[test]
    fn mount_backend_rejects_symlink_escape_for_reads_writes_and_copies() {
        use std::os::unix::fs::symlink;

        let mount = temp_dir("symlink-mount");
        let outside = temp_dir("symlink-outside");
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        symlink(&outside, mount.join("escape")).unwrap();
        symlink(outside.join("secret.txt"), mount.join("file-link")).unwrap();
        std::fs::create_dir(mount.join("container")).unwrap();
        symlink(&outside, mount.join("container/escape")).unwrap();
        let backend = MountBackend::open(&mount).unwrap();

        assert!(backend.read("/escape/secret.txt").is_err());
        assert!(backend.read_dir("/escape").is_err());
        assert!(backend.stat("/escape/secret.txt").is_err());
        assert!(backend.write("/escape/new.txt", b"blocked").is_err());
        assert!(backend.create_dir("/escape/new-dir").is_err());
        assert!(backend.copy("/escape/secret.txt", "/copy.txt").is_err());
        assert!(backend.delete("/escape/secret.txt").is_err());
        assert!(backend
            .rename("/escape/secret.txt", "/renamed.txt")
            .is_err());
        assert!(backend.read("/file-link").is_err());
        assert!(backend.write("/file-link", b"blocked").is_err());
        assert!(backend.stat("/file-link").is_err());
        assert!(backend.copy("/file-link", "/copied-link").is_err());
        backend.delete("/container").unwrap();
        assert!(!outside.join("new.txt").exists());
        assert!(!outside.join("new-dir").exists());
        assert_eq!(
            std::fs::read(outside.join("secret.txt")).unwrap(),
            b"secret"
        );
        assert!(outside.join("secret.txt").exists());
        assert!(!mount.join("copy.txt").exists());
        assert!(!mount.join("copied-link").exists());
        assert!(!mount.join("renamed.txt").exists());
        assert!(!mount.join("container").exists());

        drop(backend);
        std::fs::remove_dir_all(&mount).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn mount_backend_refuses_root_deletion_and_rename() {
        let mount = temp_dir("root-mutation");
        let backend = MountBackend::open(&mount).unwrap();
        assert!(backend.delete("/").is_err());
        assert!(backend.rename("/", "/renamed").is_err());
        drop(backend);
        std::fs::remove_dir_all(&mount).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn mount_backend_propagates_metadata_errors_with_the_entry_path() {
        use std::os::unix::fs::symlink;

        let mount = temp_dir("metadata-error");
        symlink("missing-target", mount.join("broken-link")).unwrap();
        let backend = MountBackend::open(&mount).unwrap();

        let error = backend
            .read_dir("/")
            .expect_err("a dangling entry must not be silently omitted");
        assert!(error.contains("broken-link"), "unexpected error: {error}");

        drop(backend);
        std::fs::remove_dir_all(&mount).unwrap();
    }

    #[test]
    fn mount_backend_rename_replaces_an_existing_file() {
        let mount = temp_dir("rename-replace");
        std::fs::write(mount.join("manifest.json.tmp"), b"new").unwrap();
        std::fs::write(mount.join("manifest.json"), b"old").unwrap();
        let backend = MountBackend::open(&mount).unwrap();

        backend
            .rename("/manifest.json.tmp", "/manifest.json")
            .expect("rename should replace the old manifest");

        assert_eq!(std::fs::read(mount.join("manifest.json")).unwrap(), b"new");
        assert!(!mount.join("manifest.json.tmp").exists());

        drop(backend);
        std::fs::remove_dir_all(&mount).unwrap();
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
