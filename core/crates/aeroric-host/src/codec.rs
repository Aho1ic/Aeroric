//! JSONL 帧读写（P3-06 Action 1 的 `codec` 文件）。
//!
//! 帧的**编解码与超长判定不在这里重造**——protocol 的 [`decode_line`]/[`encode_line`]
//! 是唯一实现（1 MiB 上限、`FrameError` 文案都归它）。本文件只管**流边界**：
//! 从异步流里切出一行，并在切的过程中就封顶——读完一整行再拒绝会让对端的
//! 超大帧把本端内存吃掉，验收红线「超长 JSONL 拒绝」要的是**读到上限即断**，
//! 不是读到完整帧再算账。

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use aeroric_protocol::envelope::{decode_line, encode_line, Frame, MAX_FRAME_BYTES};

/// 帧层的错误。`Overlong` 与 protocol 的 `FrameError::TooLong` 是两回事：
/// 这里是**流层**在凑齐一行之前就放弃（内容甚至没解析），那里是单行内容超限。
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("stream closed mid-frame")]
    PartialFrame,
    #[error("frame exceeds the {limit}-byte line limit before it was even delimited")]
    Overlong { limit: usize },
    #[error("frame line is not valid UTF-8")]
    NotUtf8,
    #[error(transparent)]
    Frame(#[from] aeroric_protocol::envelope::FrameError),
    #[error("io: {0}")]
    Io(String),
}

/// 读一帧：按 `\n` 定界，**累计到 `MAX_FRAME_BYTES + 1` 仍未见定界符即断流**
/// （超长的剩余字节从不进内存）。`Ok(None)` = 对端在帧边界处干净关闭。
pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<Frame>, CodecError> {
    let mut buf = Vec::with_capacity(256);
    let mut scratch = [0u8; 4096];
    loop {
        let read = reader
            .read(&mut scratch)
            .await
            .map_err(|e| CodecError::Io(e.to_string()))?;
        if read == 0 {
            return if buf.is_empty() {
                Ok(None)
            } else {
                Err(CodecError::PartialFrame)
            };
        }
        let chunk = &scratch[..read];
        if let Some(nl) = chunk.iter().position(|byte| *byte == b'\n') {
            buf.extend_from_slice(&chunk[..nl]);
            return decode_line(&buf).map(Some).map_err(CodecError::Frame);
        }
        buf.extend_from_slice(chunk);
        if buf.len() > MAX_FRAME_BYTES {
            return Err(CodecError::Overlong {
                limit: MAX_FRAME_BYTES,
            });
        }
    }
}

/// 写一帧（`encode_line` **自带**行尾 `\n`，不再补——补了就是流里的空行，
/// 下一帧读到即 `FrameError::Empty` 断连）+ flush。
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Frame,
) -> Result<(), CodecError> {
    let line = encode_line(frame).map_err(|e| CodecError::Io(e.to_string()))?;
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| CodecError::Io(e.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|e| CodecError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::envelope::Frame;
    use tokio::io::BufReader;
    use tokio::net::unix::OwnedReadHalf;
    use tokio::net::{UnixListener, UnixStream};

    async fn pair() -> (BufReader<OwnedReadHalf>, UnixStream, std::path::PathBuf) {
        let dir = std::path::PathBuf::from("/tmp").join(format!(
            "ahc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("s.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        let client = UnixStream::connect(&path).await.expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        let (read, _server_write) = server.into_split();
        drop(listener);
        (BufReader::new(read), client, dir)
    }

    fn sample() -> Frame {
        serde_json::from_value(serde_json::json!({
            "v": 1, "id": "1", "method": "ping", "params": {}
        }))
        .expect("request frame must decode")
    }

    #[tokio::test]
    async fn round_trips_a_frame_over_the_socket() {
        let (mut reader, mut client, dir) = pair().await;
        // 写走 client 端、读走 server 端——UDS 一对是双端队列，
        // 同端自写自读会永挂（第一版栽在这里，60 s 无输出）。
        let line = encode_line(&sample()).expect("encode");
        use tokio::io::AsyncWriteExt;
        client
            .write_all(line.as_bytes())
            .await
            .expect("client write");
        client.write_all(b"\n").await.expect("client newline");
        client.flush().await.expect("flush");
        let got = read_frame(&mut reader)
            .await
            .expect("read")
            .expect("one frame");
        assert_eq!(
            serde_json::to_string(&got).expect("a"),
            serde_json::to_string(&sample()).expect("b")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn overlong_line_is_killed_before_it_is_delimited() {
        // duplex 让写侧与读侧真正并发：duplex 缓冲满时 write 挂起，
        // 读侧必须恰好在越过 1 MiB 时断流——不等换行、不等 EOF。
        use tokio::io::duplex;
        let (mut tx, rx) = duplex(8 * 1024);
        let mut reader = BufReader::new(rx);
        let pump = tokio::spawn(async move {
            let chunk = vec![b'x'; 64 * 1024];
            let mut total = 0usize;
            for _ in 0..32 {
                if tx.write_all(&chunk).await.is_err() {
                    break;
                }
                total += chunk.len();
            }
            total
        });
        let error = read_frame(&mut reader).await.expect_err("must refuse");
        assert!(matches!(error, CodecError::Overlong { .. }), "{error:?}");
        pump.abort();
    }

    #[tokio::test]
    async fn closed_mid_frame_is_a_partial_not_an_eof() {
        let (mut reader, client, dir) = pair().await;
        // 写半帧就关闭：读到 0 字节且 buf 非空 ⇒ PartialFrame。
        {
            let std = client.into_std().expect("std");
            use std::io::Write as _;
            let mut half = std;
            half.write_all(b"{\"type\":\"request\"")
                .expect("partial write");
            half.flush().ok();
        } // drop 关闭写端
        let error = read_frame(&mut reader).await.expect_err("must be partial");
        assert!(matches!(error, CodecError::PartialFrame), "{error:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
