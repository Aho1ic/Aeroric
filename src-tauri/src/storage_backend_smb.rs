//! SMB 后端(纯 Rust,基于 `smb2` crate)。
//!
//! 不走系统挂载,因此不需要 sudo,三个平台行为一致。
//!
//! 路径约定:Aeroric 内部统一用 `/` 分隔的绝对路径,SMB 线上格式是 `\`
//! 分隔的相对路径,且 SMB2 非法字符(`<>:"|?*` 与 `\`)会被 crate 映射进
//! U+F000 私有区。`smb2::encode_path` / `decode_path` 负责这层往返转换,
//! 我们必须成对使用:写进 SMB 前 encode,读出条目名后 decode。

use std::sync::Mutex;

use smb2::{ClientConfig, SmbClient, Tree};

use crate::storage_backend::{
    join_storage_path, normalize_storage_path, validate_storage_mutation_path, Capability,
    StorageBackend, StorageEntry, StorageStat,
};
use crate::storage_conn::StorageConnection;

/// `Connection` 没有在 crate 根重新导出,这里给出完整路径别名。
type SmbConnection = smb2::client::connection::Connection;
type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + 'a>>;

const DEFAULT_SMB_PORT: u16 = 445;
const CONNECT_TIMEOUT_SECS: u64 = 20;

/// 会话状态。`smb2` 的操作需要 `&mut Connection`,所以整体放在一个 Mutex 里。
struct SmbSession {
    client: SmbClient,
    tree: Tree,
}

pub struct SmbBackend {
    session: Mutex<SmbSession>,
    runtime: tokio::runtime::Runtime,
    /// 连接层面的路径前缀,所有请求相对它解析。
    root: String,
}

impl SmbBackend {
    /// 把内部绝对路径转成 SMB 线上路径(相对 share 根,`\` 分隔,字符已编码)。
    fn to_smb_path(&self, path: &str) -> String {
        let absolute = if self.root == "/" {
            normalize_storage_path(path)
        } else {
            join_storage_path(
                &self.root,
                normalize_storage_path(path).trim_start_matches('/'),
            )
        };
        smb2::encode_path(&absolute)
    }

    /// 在会话上执行一次操作。闭包收到 `&Tree` 与 `&mut Connection`,
    /// 返回一个 boxed future(SMB 操作都是异步的,而本 trait 是同步签名)。
    fn with_session<T, F>(&self, action: F) -> Result<T, String>
    where
        F: for<'a> FnOnce(&'a Tree, &'a mut SmbConnection) -> BoxFuture<'a, smb2::Result<T>>,
    {
        let mut guard = self
            .session
            .lock()
            .map_err(|_| "SMB session poisoned".to_string())?;
        let SmbSession { client, tree } = &mut *guard;
        let connection = client.connection_mut();
        self.runtime
            .block_on(action(tree, connection))
            .map_err(|error| error.to_string())
    }
}

fn file_time_to_ms(time: smb2::pack::filetime::FileTime) -> Option<u64> {
    crate::storage_backend::system_time_to_ms(time.to_system_time()?)
}

impl StorageBackend for SmbBackend {
    fn capability(&self) -> Capability {
        Capability::FULL
    }

    fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String> {
        let parent = normalize_storage_path(path);
        let smb_path = self.to_smb_path(&parent);
        let listed = self.with_session(move |tree, conn| {
            Box::pin(async move { tree.list_directory(conn, &smb_path).await })
        })?;
        let mut entries = Vec::with_capacity(listed.len());
        for item in listed {
            // 服务端会返回 "." 与 "..",不应出现在 UI 里。
            if item.name == "." || item.name == ".." {
                continue;
            }
            let name = smb2::decode_name(&item.name).to_string();
            entries.push(StorageEntry {
                path: join_storage_path(&parent, &name),
                name,
                is_dir: item.is_directory,
                size: if item.is_directory {
                    None
                } else {
                    Some(item.size)
                },
                modified_at_ms: file_time_to_ms(item.modified),
            });
        }
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let smb_path = self.to_smb_path(path);
        self.with_session(move |tree, conn| {
            Box::pin(async move { tree.read_file(conn, &smb_path).await })
        })
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let smb_path = self.to_smb_path(path);
        let data = bytes.to_vec();
        self.with_session(move |tree, conn| {
            Box::pin(async move { tree.write_file(conn, &smb_path, &data).await.map(|_| ()) })
        })
    }

    fn create_dir(&self, path: &str) -> Result<(), String> {
        let smb_path = self.to_smb_path(path);
        self.with_session(move |tree, conn| {
            Box::pin(async move { tree.create_directory(conn, &smb_path).await })
        })
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let path = validate_storage_mutation_path(path)?;
        let stat = self.stat(&path)?;
        if !stat.is_dir {
            let smb_path = self.to_smb_path(&path);
            return self.with_session(move |tree, conn| {
                Box::pin(async move { tree.delete_file(conn, &smb_path).await })
            });
        }
        // SMB 的 delete_directory 要求目录为空,先递归清空。
        for entry in self.read_dir(&path)? {
            self.delete(&entry.path)?;
        }
        let smb_path = self.to_smb_path(&path);
        self.with_session(move |tree, conn| {
            Box::pin(async move { tree.delete_directory(conn, &smb_path).await })
        })
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let from = validate_storage_mutation_path(from)?;
        let from_path = self.to_smb_path(&from);
        let to_path = self.to_smb_path(to);
        self.with_session(move |tree, conn| {
            Box::pin(async move { tree.rename(conn, &from_path, &to_path).await })
        })
    }

    fn copy(&self, from: &str, to: &str) -> Result<(), String> {
        let stat = self.stat(from)?;
        if stat.is_dir {
            self.create_dir(to)?;
            for entry in self.read_dir(from)? {
                self.copy(&entry.path, &join_storage_path(to, &entry.name))?;
            }
            return Ok(());
        }
        // 逐文件读写。服务端 copychunk 仅在同 share 内可用,读写路径更通用。
        let bytes = self.read(from)?;
        self.write(to, &bytes)
    }

    fn stat(&self, path: &str) -> Result<StorageStat, String> {
        let normalized = normalize_storage_path(path);
        if normalized == "/" {
            return Ok(StorageStat {
                is_dir: true,
                size: None,
                modified_at_ms: None,
            });
        }
        let smb_path = self.to_smb_path(&normalized);
        let info = self.with_session(move |tree, conn| {
            Box::pin(async move { tree.stat(conn, &smb_path).await })
        })?;
        Ok(StorageStat {
            is_dir: info.is_directory,
            size: if info.is_directory {
                None
            } else {
                Some(info.size)
            },
            modified_at_ms: file_time_to_ms(info.modified),
        })
    }
}

/// 解析 `host` 配置为 `addr:port`。允许 `host`、`host:port` 两种写法,
/// 端口也可以单独放在 `port` 配置里。
pub(crate) fn resolve_addr(host: &str, port: Option<&str>) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("\"host\" is required".to_string());
    }
    if host.contains('/') || host.contains('\\') {
        return Err(format!("Invalid host: {host}"));
    }
    // IPv6 字面量必须带方括号才能与端口区分。
    if host.starts_with('[') {
        return Ok(if host.contains("]:") {
            host.to_string()
        } else {
            format!("{host}:{}", parse_port(port)?)
        });
    }
    if let Some((bare_host, bare_port)) = host.rsplit_once(':') {
        if bare_port.chars().all(|ch| ch.is_ascii_digit()) && !bare_port.is_empty() {
            if bare_host.is_empty() {
                return Err(format!("Invalid host: {host}"));
            }
            return Ok(format!("{bare_host}:{bare_port}"));
        }
    }
    Ok(format!("{host}:{}", parse_port(port)?))
}

fn parse_port(port: Option<&str>) -> Result<u16, String> {
    match port.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(DEFAULT_SMB_PORT),
        Some(value) => value
            .parse::<u16>()
            .map_err(|_| format!("Invalid port: {value}")),
    }
}

/// 构造 SMB 后端。share 名不允许带路径分隔符(那属于 `root`)。
pub fn build(connection: &StorageConnection) -> Result<Box<dyn StorageBackend>, String> {
    let host = connection
        .config
        .get("host")
        .map(String::as_str)
        .unwrap_or_default();
    let addr = resolve_addr(host, connection.config.get("port").map(String::as_str))?;
    let share = connection
        .config
        .get("share")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "\"share\" is required".to_string())?;
    if share.contains('/') || share.contains('\\') {
        return Err(format!("Invalid share name: {share}"));
    }
    let root = connection
        .config
        .get("root")
        .map(|value| normalize_storage_path(value))
        .unwrap_or_else(|| "/".to_string());

    let config = ClientConfig {
        addr,
        timeout: std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
        username: connection
            .secrets
            .get("username")
            .cloned()
            .unwrap_or_default(),
        password: connection
            .secrets
            .get("password")
            .cloned()
            .unwrap_or_default(),
        domain: connection.config.get("domain").cloned().unwrap_or_default(),
        auto_reconnect: true,
        compression: false,
        dfs_enabled: false,
        dfs_target_overrides: Default::default(),
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let share_name = share.to_string();
    let session = runtime.block_on(async move {
        let mut client = SmbClient::connect(config)
            .await
            .map_err(|error| error.to_string())?;
        let tree = client
            .connect_share(&share_name)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<SmbSession, String>(SmbSession { client, tree })
    })?;

    Ok(Box::new(SmbBackend {
        session: Mutex::new(session),
        runtime,
        root,
    }))
}

/// SMB 路径在传输前的编码结果。与 `SmbBackend::to_smb_path` 共享同一套规则,
/// 供测试独立验证路径归一化与逃逸防护。
#[cfg(test)]
fn preview_smb_path(root: &str, path: &str) -> String {
    let base = normalize_storage_path(root);
    let absolute = if base == "/" {
        normalize_storage_path(path)
    } else {
        join_storage_path(&base, normalize_storage_path(path).trim_start_matches('/'))
    };
    smb2::encode_path(&absolute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_addr_defaults_to_445() {
        assert_eq!(resolve_addr("nas.local", None).unwrap(), "nas.local:445");
        assert_eq!(
            resolve_addr("  nas.local  ", None).unwrap(),
            "nas.local:445"
        );
    }

    #[test]
    fn resolve_addr_honours_inline_and_separate_ports() {
        assert_eq!(
            resolve_addr("nas.local:4450", None).unwrap(),
            "nas.local:4450"
        );
        assert_eq!(
            resolve_addr("nas.local", Some("1445")).unwrap(),
            "nas.local:1445"
        );
        // 内联端口优先于单独字段,避免出现两个端口。
        assert_eq!(
            resolve_addr("nas.local:4450", Some("1445")).unwrap(),
            "nas.local:4450"
        );
    }

    #[test]
    fn resolve_addr_handles_ipv6_literals() {
        assert_eq!(resolve_addr("[fe80::1]", None).unwrap(), "[fe80::1]:445");
        assert_eq!(
            resolve_addr("[fe80::1]:1445", None).unwrap(),
            "[fe80::1]:1445"
        );
    }

    #[test]
    fn resolve_addr_rejects_invalid_input() {
        assert!(resolve_addr("", None).is_err());
        assert!(resolve_addr("nas.local/share", None).is_err());
        assert!(resolve_addr("nas.local\\share", None).is_err());
        assert!(resolve_addr(":445", None).is_err());
        assert!(resolve_addr("nas.local", Some("70000")).is_err());
        assert!(resolve_addr("nas.local", Some("abc")).is_err());
    }

    #[test]
    fn smb_paths_are_backslash_separated_and_relative() {
        assert_eq!(preview_smb_path("/", "/dir/file.txt"), "dir\\file.txt");
        assert_eq!(preview_smb_path("/", "/"), "");
        assert_eq!(preview_smb_path("/base", "/file.txt"), "base\\file.txt");
    }

    #[test]
    fn smb_paths_cannot_escape_the_configured_root() {
        assert_eq!(preview_smb_path("/base", "/../../etc"), "base\\etc");
        assert_eq!(preview_smb_path("/base", "/a/../../b"), "base\\b");
    }

    #[test]
    fn illegal_smb_characters_round_trip_through_the_private_use_area() {
        // `:` 是 SMB2 非法字符,crate 会映射进 U+F000 区。
        let encoded = smb2::encode_name("a:b");
        assert_ne!(encoded, "a:b");
        assert_eq!(smb2::decode_name(&encoded), "a:b");
    }
}
