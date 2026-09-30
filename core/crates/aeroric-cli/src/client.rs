//! UDS 客户端：连接、握手、帧往返、退出码映射（P3-07 ②b）。
//!
//! 握手是连接后第一帧且必须成功——`HostClient` 的构造返回的就是**已认证**
//! 连接，不存在「连上了但没握手」的中间态给调用方误用。token 从
//! `data_dir/host.token` 读（同用户文件权限就是它的分发渠道；跨用户被
//! peer-UID 闸挡、跨 data-dir 被 token 比对挡）。

use std::path::{Path, PathBuf};

use aeroric_protocol::envelope::{encode_line, Frame, Request, Success};
use aeroric_protocol::error::ErrorCode;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;

use crate::args::exit;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("cannot reach the host at {path}: {reason}")]
    Connect { path: String, reason: String },
    #[error("handshake was rejected: {0}")]
    Auth(String),
    #[error("host answered {code:?}: {message}")]
    Rpc { code: ErrorCode, message: String },
    #[error("stream error: {0}")]
    Transport(String),
}

impl ClientError {
    /// Action 4 的映射表。`Rpc` 的码里只有 task 失败语义走 4；其余
    /// （NotFound/InvalidArgument/...）是「这次调用没成功但连接与认证没问题」，
    /// CLI 层统一 3 会撒谎、统一 4 会误导脚本——③/④ 片注册真方法时按码细化，
    /// 这里先立两条确定的：连接/认证=3，其余暂 3 并在消息里保留原码。
    pub fn exit_code(&self) -> i32 {
        match self {
            ClientError::Connect { .. } | ClientError::Auth(_) => exit::CONNECT,
            ClientError::Rpc { .. } | ClientError::Transport(_) => exit::CONNECT,
        }
    }
}

pub struct HostClient {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_id: u64,
    socket_path: PathBuf,
}

impl HostClient {
    /// 连接 + 握手。`client_id` 是这次 CLI 调用的身份（uuid v4——审批回执
    /// 的 clientId 判据要求每条连接可区分）。
    pub async fn connect(data_dir: &Path, client_id: &str) -> Result<Self, ClientError> {
        let socket_path = aeroric_host::uds::default_socket_path(data_dir);
        let token_path = data_dir.join("host.token");
        let token = std::fs::read_to_string(&token_path)
            .map(|raw| raw.trim().to_string())
            .unwrap_or_default();
        if token.is_empty() {
            return Err(ClientError::Connect {
                path: socket_path.to_string_lossy().into_owned(),
                reason: format!(
                    "no host token at {}; is a host serving this --data-dir? (aeroric-cli --data-dir {} serve)",
                    token_path.display(),
                    data_dir.display()
                ),
            });
        }
        let stream = UnixStream::connect(&socket_path)
            .await
            .map_err(|e| ClientError::Connect {
                path: socket_path.to_string_lossy().into_owned(),
                reason: e.to_string(),
            })?;
        let (read, write) = stream.into_split();
        let mut client = Self {
            reader: BufReader::new(read),
            writer: write,
            next_id: 0,
            socket_path,
        };
        match client
            .call(
                "handshake",
                serde_json::json!({ "token": token, "clientId": client_id }),
            )
            .await
        {
            Ok(_) => Ok(client),
            Err(ClientError::Rpc { message, .. }) => Err(ClientError::Auth(message)),
            Err(other) => Err(other),
        }
    }

    /// 一次请求-响应。行读用 `read_until`（BufReader 的缓冲归本结构独占，
    /// 不存在半帧被两次 `read` 抢丢的问题——host 侧测试的教训反过来用）。
    pub async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.next_id += 1;
        let id = format!("{}", self.next_id);
        let request = Frame::Request(Request {
            v: aeroric_protocol::PROTOCOL_VERSION,
            id: id.clone(),
            method: method.to_string(),
            params,
            idempotency_key: None,
            extra: Default::default(),
        });
        let line = encode_line(&request)
            .map_err(|e| ClientError::Transport(format!("encode failed: {e}")))?;
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| self.transport_lost(e))?;
        self.writer
            .flush()
            .await
            .map_err(|e| self.transport_lost(e))?;
        let mut raw = Vec::new();
        let read = self
            .reader
            .read_until(b'\n', &mut raw)
            .await
            .map_err(|e| self.transport_lost(e))?;
        if read == 0 {
            return Err(ClientError::Transport(format!(
                "host closed the connection at {}",
                self.socket_path.display()
            )));
        }
        match aeroric_protocol::envelope::decode_line(&raw)
            .map_err(|e| ClientError::Transport(e.to_string()))?
        {
            Frame::Success(Success { result, .. }) => Ok(result),
            Frame::Failure(failure) => Err(ClientError::Rpc {
                code: failure.error.code,
                message: failure.error.message,
            }),
            other => Err(ClientError::Transport(format!(
                "expected a response frame, got {other:?}"
            ))),
        }
    }

    fn transport_lost(&self, e: std::io::Error) -> ClientError {
        ClientError::Transport(format!("{}: {e}", self.socket_path.display()))
    }
}
