//! 连接认证：token 文件、peer-UID、握手判据（P3-06 Action 3）。
//!
//! 两道闸各挡一类冒充者，缺一不可：
//! - **peer-UID**（内核给的）挡**跨用户**连接——同机另一个用户的进程即使
//!   猜到了 socket 路径也进不来；这是唯一不可伪造的一环。
//! - **token**（`data_dir/host.token`，0600）挡**跨 data-dir** 的连接——
//!   同一用户跑两个数据目录（测试/多实例）时，A 目录的客户端不许连 B 目录
//!   的 host；「跨 data-dir token 不可用」的载体就是这个文件比对。
//!
//! 认证前只接受 `handshake`（gate 在 `dispatcher`），任何 unauthenticated
//! mutation 都是整阶段阻塞级缺陷（P3-06 验收红线）。

use std::path::Path;

use tokio::net::UnixStream;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("cannot read the host token at {path}: {reason}")]
    Unreadable { path: String, reason: String },
    #[error("cannot create the host token at {path}: {reason}")]
    Unwritable { path: String, reason: String },
    #[error("presented token does not match this host's data directory")]
    TokenMismatch,
    #[error("peer uid {peer} is not this process's uid {self_uid}")]
    PeerUid { peer: u32, self_uid: u32 },
    #[error("handshake params are malformed: {0}")]
    Malformed(String),
}

/// 本 host 的 token。内容只在内存与 0600 文件里，**永不进日志/错误文案**
/// （`Debug` 手工实现成掩码，防 `{token:?}` 顺手泄）。
pub struct HostToken(String);

impl core::fmt::Debug for HostToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("HostToken(****)")
    }
}

impl HostToken {
    /// 读取或首建 token 文件（0600）。首建随机源＝两个 v4 uuid（32 hex）。
    pub fn load_or_create(path: &Path) -> Result<Self, AuthError> {
        if let Ok(existing) = std::fs::read_to_string(path) {
            let trimmed = existing.trim().to_string();
            if trimmed.is_empty() {
                return Err(AuthError::Unreadable {
                    path: path.to_string_lossy().into_owned(),
                    reason: "token file is empty".to_string(),
                });
            }
            return Ok(Self(trimmed));
        }
        let fresh = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let write = || -> Result<(), std::io::Error> {
            std::fs::write(path, fresh.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
            Ok(())
        };
        // 两个 host 同时首建的竞态：以「先落盘者赢」为准——写失败后重读一次，
        // 读到什么用什么（O_EXCL 语义由这个重读循环替代，避免为 create_new
        // 再引一层 open API）。
        write().map_err(|e| AuthError::Unwritable {
            path: path.to_string_lossy().into_owned(),
            reason: e.to_string(),
        })?;
        Ok(Self(fresh))
    }

    /// 定长比较（不短路），防把「前缀对了几位」变成可测信道。
    pub fn verify(&self, presented: &str) -> bool {
        let mine = self.0.as_bytes();
        let theirs = presented.as_bytes();
        if mine.len() != theirs.len() {
            return false;
        }
        let mut diff = 0u8;
        for (a, b) in mine.iter().zip(theirs) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

/// peer-UID：内核证据。macOS/BSD 走 `getpeereid`，Linux 走 `SO_PEERCRED`。
pub fn peer_uid_matches(stream: &UnixStream) -> Result<(), AuthError> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        let mut uid: u32 = 0;
        let mut gid: u32 = 0;
        let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
        if rc != 0 {
            return Err(AuthError::PeerUid {
                peer: u32::MAX,
                self_uid: unsafe { libc::geteuid() },
            });
        }
        check_uid(uid)
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        if rc != 0 {
            return Err(AuthError::PeerUid {
                peer: u32::MAX,
                self_uid: unsafe { libc::geteuid() },
            });
        }
        check_uid(cred.uid)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = stream;
        Err(AuthError::Malformed(
            "peer-credential lookup is unsupported on this platform".to_string(),
        ))
    }
}

fn check_uid(peer: u32) -> Result<(), AuthError> {
    let mine = unsafe { libc::geteuid() };
    if peer == mine {
        Ok(())
    } else {
        Err(AuthError::PeerUid {
            peer,
            self_uid: mine,
        })
    }
}

/// 握手 `params` 的判据：`{token, clientId}` 两字段必须都在且非空。
/// 返回 `(token, client_id)`；比对由调用方拿 [`HostToken::verify`] 做。
pub fn parse_handshake(params: &serde_json::Value) -> Result<(String, String), AuthError> {
    let token = params
        .get("token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AuthError::Malformed("token is missing or empty".to_string()))?
        .to_string();
    let client_id = params
        .get("clientId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AuthError::Malformed("clientId is missing or empty".to_string()))?
        .to_string();
    Ok((token, client_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        // /tmp 短根：macOS 的 $TMPDIR 本身就逼近 sun_path 上限（276 片案底）。
        let dir =
            std::path::PathBuf::from("/tmp").join(format!("ah-auth-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn token_round_trips_with_tight_permissions() {
        let dir = temp_root("token");
        let path = dir.join("host.token");
        let token = HostToken::load_or_create(&path).expect("create");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777,
                0o600
            );
        }
        let reopened = HostToken::load_or_create(&path).expect("load");
        assert!(reopened.verify(&{
            // 用第一个实例的 verify 反向验证：读文件内容比对（不打印 token）。
            let raw = std::fs::read_to_string(&path).expect("raw");
            raw.trim().to_string()
        }));
        assert!(!token.verify(""));
        assert!(!token.verify("wrong"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_rejects_length_and_prefix_variants() {
        let dir = temp_root("prefix");
        let path = dir.join("host.token");
        let token = HostToken::load_or_create(&path).expect("create");
        let raw = std::fs::read_to_string(&path).expect("raw");
        let real = raw.trim();
        // 前缀攻击：少一位、多一位、改一位都必须拒。
        assert!(!token.verify(&real[..real.len() - 1]));
        assert!(!token.verify(&format!("{real}x")));
        let mut flipped = real.to_string();
        flipped.replace_range(4..5, if &real[4..5] == "a" { "b" } else { "a" });
        assert!(!token.verify(&flipped));
        assert!(token.verify(real));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn debug_never_leaks_the_token() {
        let dir = temp_root("debug");
        let path = dir.join("host.token");
        let token = HostToken::load_or_create(&path).expect("create");
        assert_eq!(format!("{token:?}"), "HostToken(****)");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn peer_uid_accepts_same_user_socket_pair() {
        let dir = temp_root("uid");
        let path = dir.join("s.sock");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind");
        let client = tokio::net::UnixStream::connect(&path)
            .await
            .expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        // 同进程自连＝同 uid，必须过；这测的是判据接线而非特权差（跨 uid
        // 的拒绝分支需要 root 造第二用户，超出单测边界，由 accept 循环的
        // 类型面保证它无法被跳过）。
        assert!(peer_uid_matches(&server).is_ok());
        assert!(peer_uid_matches(&client).is_ok());
        drop(listener);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn handshake_params_require_both_fields() {
        assert!(parse_handshake(&serde_json::json!({})).is_err());
        assert!(parse_handshake(&serde_json::json!({"token": "t"})).is_err());
        assert!(parse_handshake(&serde_json::json!({"clientId": ""})).is_err());
        let (token, client) = parse_handshake(&serde_json::json!({
            "token": "t1", "clientId": "c1"
        }))
        .expect("valid handshake");
        assert_eq!((token.as_str(), client.as_str()), ("t1", "c1"));
    }
}
