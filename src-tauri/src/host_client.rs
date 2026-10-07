//! 与**外部宿主**（external host）通话的客户端（Task P8-05 Action 1）。
//!
//! 形状照抄 `aeroric-cli` 的 `client.rs`（同一份线协议、同一个 token 文件、同一套
//! 握手），差别只有两点，都是宿主进程特有的：
//!
//! 1. **同进程只连一次**：Tauri 的每个窗口不是一条连接，只有一条（[`HostLink`]），
//!    事件面靠 `events.since` 增量拉，而不是给每个视图各开一条流。
//! 2. **不映射退出码**：GUI 不做进程退出码，失败是给 UI 的文案（`HostClientError`）。
//!
//! 为什么镜像而不复用 CLI 的客户端：`src-tauri` 不依赖 `aeroric-host`（那条依赖会把
//! 宿主 crate 的 `portable-pty` 与工作区 patches 拉进 app 的依赖图，只为一个 socket
//! 不值）。两侧对不上的地方由门禁用**真进程**对账：
//! `scripts/native/tauri-host-smoke.mjs` 起真宿主、本文件的用例起脚本化 UDS 服务端。

use std::path::{Path, PathBuf};

use aeroric_protocol::envelope::{decode_line, encode_line, Frame, Request, Success};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;

/// socket 落点：`<data-dir>/host.sock`（与 `aeroric_host::uds::default_socket_path` 同一条
/// 约定；镜像的理由见模块头）。
pub fn host_socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("host.sock")
}

/// token 文件：`<data-dir>/host.token`。同用户文件权限就是它的分发渠道。
pub fn host_token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("host.token")
}

/// 消费面：`Rpc`/`socket()`/`request()`/`events_since()` 由**下一切片**（domain 命令转发
/// 与事件桥）使用；本片用到的是 `connect`（握手）与它建立的连接本身。逐个 allow 而不
/// 用模块级 allow，理由可复核且到期必须删。
#[derive(Debug, PartialEq, Eq)]
pub enum HostClientError {
    /// socket 不存在/连不上（没有活宿主）。
    Unreachable { socket: PathBuf, reason: String },
    /// 没有 token 文件：这个 data dir 从来没有宿主启动过。
    NoToken { path: PathBuf },
    /// 握手被拒（token 不匹配、或这条连接已经握过手）。
    Rejected { code: String, message: String },
    /// 方法本身失败：连接是好的，是这次调用没成功。
    #[allow(
        dead_code,
        reason = "domain 命令转发（下一切片）才会收到 Rpc；本片只握手"
    )]
    Rpc { code: String, message: String },
    /// 帧层错误（解码失败/连接断开/超时）。
    Transport(String),
}

impl std::fmt::Display for HostClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostClientError::Unreachable { socket, reason } => {
                write!(formatter, "cannot reach the host at {socket:?}: {reason}")
            }
            HostClientError::NoToken { path } => {
                write!(
                    formatter,
                    "no host token at {path:?}: no host has served this data directory"
                )
            }
            HostClientError::Rejected { code, message } => {
                write!(formatter, "handshake rejected ({code}): {message}")
            }
            HostClientError::Rpc { code, message } => {
                write!(formatter, "host answered {code}: {message}")
            }
            HostClientError::Transport(reason) => write!(formatter, "stream error: {reason}"),
        }
    }
}

impl std::error::Error for HostClientError {}

/// 一条**已认证**的连接。构造即握手成功，不存在「连上了但没握手」的中间态给调用方误用。
#[derive(Debug)]
pub struct HostClient {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_id: u64,
    #[allow(dead_code, reason = "诊断文案与下一切片的转发要用；本片由用例读")]
    socket: PathBuf,
}

impl HostClient {
    /// 连接 + 握手。`client_id` 是这次连接的身份（审批回执的 clientId 判据要求每条连接
    /// 可区分，所以由调用方给，不在这里造随机数——造了就没法在日志里对上）。
    pub async fn connect(data_dir: &Path, client_id: &str) -> Result<Self, HostClientError> {
        let socket = host_socket_path(data_dir);
        let token_path = host_token_path(data_dir);
        let token = std::fs::read_to_string(&token_path)
            .map(|raw| raw.trim().to_string())
            .map_err(|_| HostClientError::NoToken {
                path: token_path.clone(),
            })?;
        if token.is_empty() {
            return Err(HostClientError::NoToken { path: token_path });
        }

        let stream =
            UnixStream::connect(&socket)
                .await
                .map_err(|error| HostClientError::Unreachable {
                    socket: socket.clone(),
                    reason: error.to_string(),
                })?;
        let (read, write) = stream.into_split();
        let mut client = Self {
            reader: BufReader::new(read),
            writer: write,
            next_id: 1,
            socket,
        };
        client.handshake(&token, client_id).await?;
        Ok(client)
    }

    /// 这条连接面对的 socket（诊断文案要用它）。
    #[allow(dead_code, reason = "诊断与下一切片；本片由 `host_client_tests` 读")]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    async fn handshake(&mut self, token: &str, client_id: &str) -> Result<(), HostClientError> {
        let reply = self
            .request_frames(
                "handshake",
                serde_json::json!({ "token": token, "clientId": client_id }),
            )
            .await?;
        match reply {
            Frame::Success(_) => Ok(()),
            Frame::Failure(failure) => Err(HostClientError::Rejected {
                code: format!("{:?}", failure.error.code),
                message: failure.error.message,
            }),
            // 握手阶段收到事件或请求帧＝对面不是宿主（或线被谁插了话）。当成帧层错误，
            // 不猜它的意思。
            Frame::Event(_) | Frame::Request(_) => Err(HostClientError::Transport(
                "unexpected frame during handshake".to_string(),
            )),
        }
    }

    /// 一次方法调用；成功时返回应答的 `result`。
    #[allow(
        dead_code,
        reason = "domain 命令转发（下一切片）；本片由用例覆盖其线形状"
    )]
    pub async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, HostClientError> {
        match self.request_frames(method, params).await? {
            Frame::Success(Success { result, .. }) => Ok(result),
            Frame::Failure(failure) => Err(HostClientError::Rpc {
                code: format!("{:?}", failure.error.code),
                message: failure.error.message,
            }),
            Frame::Event(_) | Frame::Request(_) => Err(HostClientError::Transport(
                "unexpected frame while waiting for a reply".to_string(),
            )),
        }
    }

    /// 读事件（`events.since`）。返回 `(帧, 流尾序号)`——序号由调用方保存，重连时从这里续。
    #[allow(
        dead_code,
        reason = "host_event_bridge（下一切片）的读取面；本片不接事件"
    )]
    pub async fn events_since(
        &mut self,
        after: &str,
    ) -> Result<(Frame, Option<String>), HostClientError> {
        let frame = self
            .request_frames("events.since", serde_json::json!({ "after": after }))
            .await?;
        let cursor = match &frame {
            Frame::Success(Success { result, .. }) => result
                .get("cursor")
                .or_else(|| result.get("next"))
                .and_then(|value| value.as_str())
                .map(str::to_string),
            _ => None,
        };
        Ok((frame, cursor))
    }

    async fn request_frames(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<Frame, HostClientError> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        let frame = Frame::Request(Request {
            v: aeroric_protocol::PROTOCOL_VERSION,
            id,
            method: method.to_string(),
            params,
            idempotency_key: None,
            extra: Default::default(),
        });
        let line =
            encode_line(&frame).map_err(|error| HostClientError::Transport(error.to_string()))?;
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|error| HostClientError::Transport(error.to_string()))?;
        self.writer
            .flush()
            .await
            .map_err(|error| HostClientError::Transport(error.to_string()))?;

        let mut reply = String::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.reader.read_line(&mut reply),
        )
        .await
        .map_err(|_| HostClientError::Transport("timed out waiting for a reply".to_string()))?
        .map_err(|error| HostClientError::Transport(error.to_string()))?;
        if read == 0 {
            return Err(HostClientError::Transport(
                "the host closed the connection".to_string(),
            ));
        }
        decode_line(reply.trim_end().as_bytes())
            .map_err(|error| HostClientError::Transport(error.to_string()))
    }
}

/// 脚本化的真 UDS 服务端（测试专供）。
///
/// 放在这里而不是某个测试文件里：`host_client` 与 `host_dispatch` 两套用例都要用它，
/// 而重复实现一份"会解的假宿主"意味着两边可能各自漂移（线形状的判据就不可信了）。
#[cfg(test)]
pub(crate) mod scripted {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use aeroric_protocol::envelope::{decode_line, encode_line, Failure, Frame, Success};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    use super::host_socket_path;

    pub type Script = Arc<dyn Fn(&str, &serde_json::Value) -> Option<Frame> + Send + Sync>;

    pub struct ScriptedHost {
        pub socket: PathBuf,
        pub data_dir: PathBuf,
        seen: Arc<Mutex<Vec<String>>>,
        shutdown: Option<tokio::sync::oneshot::Sender<()>>,
        handle: tokio::task::JoinHandle<()>,
    }

    impl ScriptedHost {
        pub async fn start(tag: &str, script: Script, token: Option<&str>) -> Self {
            // `/tmp` 短根：macOS 的 `$TMPDIR` 逼近 `sun_path` 上限（276 片案底）。
            let root = PathBuf::from("/tmp").join(format!(
                "ahc-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&root).expect("root");
            if let Some(token) = token {
                std::fs::write(super::host_token_path(&root), token).expect("token");
            }
            let socket = host_socket_path(&root);
            let listener = UnixListener::bind(&socket).expect("bind");
            let seen = Arc::new(Mutex::new(Vec::new()));
            let seen_for_task = Arc::clone(&seen);
            let (shutdown, mut stop) = tokio::sync::oneshot::channel();
            let handle = tokio::spawn(async move {
                loop {
                    let accepted = tokio::select! {
                        _ = &mut stop => break,
                        accepted = listener.accept() => accepted,
                    };
                    let Ok((stream, _)) = accepted else { break };
                    let script = Arc::clone(&script);
                    let seen = Arc::clone(&seen_for_task);
                    tokio::spawn(async move {
                        let (read, mut write) = stream.into_split();
                        let mut reader = BufReader::new(read);
                        loop {
                            let mut line = String::new();
                            match reader.read_line(&mut line).await {
                                Ok(0) | Err(_) => return,
                                Ok(_) => {}
                            }
                            let Ok(Frame::Request(request)) =
                                decode_line(line.trim_end().as_bytes())
                            else {
                                return;
                            };
                            seen.lock().unwrap().push(request.method.clone());
                            let Some(reply) = script(&request.method, &request.params) else {
                                return; // 脚本说"断开"
                            };
                            let reply = match reply {
                                Frame::Success(mut success) => {
                                    success.id = request.id.clone();
                                    Frame::Success(success)
                                }
                                Frame::Failure(mut failure) => {
                                    failure.id = request.id.clone();
                                    Frame::Failure(failure)
                                }
                                other => other,
                            };
                            let Ok(encoded) = encode_line(&reply) else {
                                return;
                            };
                            if write.write_all(encoded.as_bytes()).await.is_err() {
                                return;
                            }
                            let _ = write.flush().await;
                        }
                    });
                }
            });
            Self {
                socket,
                data_dir: root,
                seen,
                shutdown: Some(shutdown),
                handle,
            }
        }

        pub fn methods(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl Drop for ScriptedHost {
        fn drop(&mut self) {
            if let Some(shutdown) = self.shutdown.take() {
                let _ = shutdown.send(());
            }
            self.handle.abort();
            std::fs::remove_dir_all(&self.data_dir).ok();
        }
    }

    pub fn success(result: serde_json::Value) -> Frame {
        Frame::Success(Success {
            v: aeroric_protocol::PROTOCOL_VERSION,
            id: String::new(),
            ok: true,
            result,
            extra: Default::default(),
        })
    }

    pub fn failure(code: aeroric_protocol::error::ErrorCode, message: &str) -> Frame {
        Frame::Failure(Failure {
            v: aeroric_protocol::PROTOCOL_VERSION,
            id: String::new(),
            ok: false,
            error: aeroric_protocol::error::RpcError {
                code,
                message: message.to_string(),
                retryable: false,
                detail: None,
            },
            extra: Default::default(),
        })
    }

    /// 只接受 `token` 的握手脚本（握手以外的请求都断开）。
    pub fn handshake_script(expected: &'static str) -> Script {
        Arc::new(move |method, params| {
            if method != "handshake" {
                return None;
            }
            let presented = params
                .get("token")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if presented != expected {
                return Some(failure(
                    aeroric_protocol::error::ErrorCode::Unauthenticated,
                    "presented token does not match this host's data directory",
                ));
            }
            Some(success(serde_json::json!({
                "clientId": params.get("clientId").cloned().unwrap_or_default(),
                "capabilities": [],
            })))
        })
    }
}
