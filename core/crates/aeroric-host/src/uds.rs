//! UDS listener：路径上限、残留 socket 的**握手探活**、bind（P3-06 Action 1/4）。
//!
//! 两条验收红线住在这里：
//! - **路径过长**：`sockaddr_un::sun_path` 的容量按最严的平台取 104 B
//!   （macOS；Linux 是 108），超限在 bind **之前**拒绝——让 bind 自己报
//!   `FileName too long` 的错误文案不可读，且各平台不一致。
//! - **socket 残留**：启动时 socket 路径已存在，**不许**凭进程名判断谁活着，
//!   也不许直接 unlink 抢绑——先连一次做握手探活：连上＝有活的 listener
//!   （可能是别的 build/别的 host），拒绝启动；`ConnectionRefused`＝文件是
//!   尸体（进程没了内核还替它占着路径），unlink 后重绑。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::net::UnixStream;

/// `sockaddr_un` 路径字节上限，按最严平台（macOS 104）计。
pub const SUN_PATH_LIMIT: usize = 104;

#[derive(Debug, thiserror::Error)]
pub enum UdsError {
    #[error("socket path is {length} bytes, over the sun_path limit of {limit}: {path}")]
    PathTooLong {
        length: usize,
        limit: usize,
        path: String,
    },
    #[error("another live host listener already owns {path} (handshake succeeded); refusing to steal it")]
    AlreadyLive { path: String },
    #[error("cannot remove the stale socket at {path}: {reason}")]
    StaleUnlink { path: String, reason: String },
    #[error("cannot bind {path}: {reason}")]
    Bind { path: String, reason: String },
}

/// 探活结论：`Live` = 有进程在 accept；`Dead` = 文件残留、无人监听。
#[derive(Debug, PartialEq, Eq)]
pub enum Probe {
    Live,
    Dead,
}

/// 对现存 socket 做一次握手连接（带超时）。连上即 `Live`；
/// `ConnectionRefused`/超时/`ENOENT` 之外的错误按 `Dead` 处理——
/// 探活只回答「有没有人 accept」，不回答「是谁」。
pub async fn probe_liveness(path: &Path) -> Probe {
    // 超时给足 2 s：跨机器/负载高的机器上 accept 可能慢，误判 Dead 会
    // 抢掉活人的 socket，比拒绝启动危险。
    let attempt = tokio::time::timeout(Duration::from_secs(2), UnixStream::connect(path)).await;
    match attempt {
        Ok(Ok(_stream)) => Probe::Live,
        Ok(Err(error)) if matches!(error.kind(), ErrorKind::ConnectionRefused) => Probe::Dead,
        Ok(Err(_)) => Probe::Dead,
        Err(_) => Probe::Dead,
    }
}

/// bind 前置检查 + 残留处置 + 监听。**只监听 UDS，永不触碰 TCP**
/// （Action 3 的「不监听 TCP 0.0.0.0」由类型面保证：这里没有任何 TCP 构造）。
pub async fn bind_listener(path: &Path) -> Result<tokio::net::UnixListener, UdsError> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.len() > SUN_PATH_LIMIT {
        return Err(UdsError::PathTooLong {
            length: bytes.len(),
            limit: SUN_PATH_LIMIT,
            path: path.to_string_lossy().into_owned(),
        });
    }
    if path.exists() {
        if probe_liveness(path).await == Probe::Live {
            return Err(UdsError::AlreadyLive {
                path: path.to_string_lossy().into_owned(),
            });
        }
        std::fs::remove_file(path).map_err(|e| UdsError::StaleUnlink {
            path: path.to_string_lossy().into_owned(),
            reason: e.to_string(),
        })?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| UdsError::Bind {
            path: path.to_string_lossy().into_owned(),
            reason: e.to_string(),
        })?;
    }
    let listener = tokio::net::UnixListener::bind(path).map_err(|e| UdsError::Bind {
        path: path.to_string_lossy().into_owned(),
        reason: e.to_string(),
    })?;
    // socket 文件权限 0600：只有属主可连（token 之前的第一道闸）。
    set_owner_only(path)?;
    Ok(listener)
}

/// 收紧 socket 权限到 0600（`FromRawFd` 走 std，避免为这一个 chmod 引 nix）。
fn set_owner_only(path: &Path) -> Result<(), UdsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            UdsError::Bind {
                path: path.to_string_lossy().into_owned(),
                reason: format!("cannot tighten socket permissions: {e}"),
            }
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// 本 host 的默认 socket 路径（data dir 下，随 `AppPaths` 注入，不读环境变量）。
///
/// Sandbox 容器里的 `Application Support` 根可以越过 `sun_path` 的 104 B
/// 上限。Swift 客户端与 CLI/host 必须对同一个 data dir 算出同一条路径，
/// 否则客户端会把 host 的 ready 帧判成 `socketMismatch`，或附着到错误的
/// socket。因此超限时两端使用同一个确定性 FNV-1a 64 位短路径；普通短根
/// 保持 `<dataDir>/host.sock` 不变。
pub fn default_socket_path(data_dir: &Path) -> PathBuf {
    let direct = data_dir.join("host.sock");
    if direct.as_os_str().as_encoded_bytes().len() < SUN_PATH_LIMIT {
        return direct;
    }
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data_dir.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    PathBuf::from(format!("/tmp/aeroric-{hash:016x}.sock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/tmp` 短根，不用 `std::env::temp_dir()`——macOS 的 `$TMPDIR`
    /// 本身就 ~90 B，加测试名直接越过 104 B 红线（`overlong` 用例钉的
    /// 正是这条判据，测试自己不能先撞上）。
    fn temp_root(tag: &str) -> PathBuf {
        let dir = PathBuf::from("/tmp").join(format!(
            "aeroric-host-uds-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[tokio::test]
    async fn overlong_socket_path_is_refused_before_bind() {
        // 造一个 >104 B 的路径：bind 自己也会失败，但错误必须是本模块的
        // 可读判据（含长度与上限），不是平台 errno。
        let long = temp_root("long").join("s".repeat(120)).join("host.sock");
        let error = bind_listener(&long).await.expect_err("must refuse");
        match error {
            UdsError::PathTooLong { length, limit, .. } => {
                assert!(length > limit);
                assert_eq!(limit, SUN_PATH_LIMIT);
            }
            other => panic!("wrong refusal: {other:?}"),
        }
    }

    #[tokio::test]
    async fn binds_and_tightens_permissions() {
        let root = temp_root("bind");
        let path = root.join("host.sock");
        let listener = bind_listener(&path).await.expect("bind");
        assert!(path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "socket 必须只有属主可连");
        }
        drop(listener);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn default_socket_path_matches_the_swift_short_path_contract() {
        assert_eq!(
            default_socket_path(Path::new("/tmp/aeroric-short")),
            PathBuf::from("/tmp/aeroric-short/host.sock")
        );

        // The same fixture and expected value are pinned in Swift's
        // SandboxLifecycleTests; the two implementations must stay aligned.
        let deep = Path::new(
            "/Users/fixture/Library/Containers/com.aeroric.native.dev/Data/Library/Application Support/com.aeroric.native.dev",
        );
        assert_eq!(
            default_socket_path(deep),
            PathBuf::from("/tmp/aeroric-98e1d459c998c1aa.sock")
        );
        assert!(
            default_socket_path(deep)
                .as_os_str()
                .as_encoded_bytes()
                .len()
                < SUN_PATH_LIMIT
        );
    }

    #[tokio::test]
    async fn stale_socket_is_unlinked_and_rebound_live_one_is_refused() {
        let root = temp_root("stale");
        let path = root.join("host.sock");
        let listener = bind_listener(&path).await.expect("first bind");
        // 活人占着：第二个 bind 必须拒绝（探活连上了第一个 listener）。
        let error = bind_listener(&path).await.expect_err("live must refuse");
        assert!(matches!(error, UdsError::AlreadyLive { .. }), "{error:?}");
        // 释放后文件残留（内核不会删）：探活＝Dead，重绑成功。
        drop(listener);
        assert!(path.exists(), "drop 不会移除 socket 文件——残留是常态");
        assert_eq!(probe_liveness(&path).await, Probe::Dead);
        let rebound = bind_listener(&path).await.expect("rebind after stale");
        drop(rebound);
        std::fs::remove_dir_all(&root).ok();
    }
}
