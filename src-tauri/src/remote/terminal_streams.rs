//! 单个 WS 连接上的终端流管理:Subscribe/Unsubscribe/Input/Resize 帧处理,
//! 以及「历史快照 + hub live 输出」的转发任务。
//!
//! 快照即恢复:每次(重)订阅先发历史尾部,再按 hub 水位协议
//! (见 terminal_hub.rs)透传增量;落后(Lagged)或任务重跑(epoch 变化)
//! 时自动重新快照,保证手机端画面最终一致。

use std::collections::HashMap;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite::protocol::Message;

use super::server::OutboundSender;
use super::terminal_frames::{
    decode_terminal_frame, TerminalFrame, TerminalOpcode, MAX_PAYLOAD_BYTES,
};
use super::terminal_hub::hub;
use super::RemoteState;
use crate::TaskManager;

/// 快照取历史尾部的上限。256KB 覆盖 TUI 整屏重绘绰绰有余,又不至于
/// 让长任务的首屏同步拖慢。
const SNAPSHOT_TAIL_BYTES: u64 = 256 * 1024;
const SNAPSHOT_CHUNK_BYTES: usize = 32 * 1024;
const MAX_STREAMS_PER_CONNECTION: usize = 8;
const DEFAULT_DESKTOP_PTY_SIZE: (u16, u16) = (220, 50);

fn frame_message(opcode: TerminalOpcode, stream_id: u32, seq: u64, payload: Vec<u8>) -> Message {
    Message::Binary(TerminalFrame::new(opcode, stream_id, seq, payload).encode())
}

struct StreamHandle {
    task_id: String,
    forwarder: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct TerminalLeaseRegistry {
    inner: Mutex<HashMap<String, TerminalLease>>,
}

struct TerminalLease {
    subscribers: usize,
    original_size: Option<(u16, u16)>,
}

impl TerminalLeaseRegistry {
    fn acquire(&self, task_id: &str, current_size: Option<(u16, u16)>) {
        let mut leases = self.inner.lock();
        let lease = leases.entry(task_id.to_string()).or_insert(TerminalLease {
            subscribers: 0,
            original_size: Some(current_size.unwrap_or(DEFAULT_DESKTOP_PTY_SIZE)),
        });
        lease.subscribers += 1;
    }

    fn release(&self, task_id: &str) -> Option<(u16, u16)> {
        let mut leases = self.inner.lock();
        let lease = leases.get_mut(task_id)?;
        lease.subscribers = lease.subscribers.saturating_sub(1);
        if lease.subscribers > 0 {
            return None;
        }
        leases.remove(task_id).and_then(|lease| lease.original_size)
    }

    #[cfg(test)]
    fn subscriber_count(&self, task_id: &str) -> usize {
        self.inner
            .lock()
            .get(task_id)
            .map(|lease| lease.subscribers)
            .unwrap_or(0)
    }
}

pub struct TerminalStreams<R: Runtime> {
    app: AppHandle<R>,
    outbound: OutboundSender,
    streams: HashMap<u32, StreamHandle>,
}

impl<R: Runtime> TerminalStreams<R> {
    pub fn new(app: AppHandle<R>, outbound: OutboundSender) -> Self {
        Self {
            app,
            outbound,
            streams: HashMap::new(),
        }
    }

    /// 连接收到 Binary 消息时调用;非终端帧静默忽略。
    pub fn handle_frame(&mut self, bytes: &[u8]) {
        let Some(frame) = decode_terminal_frame(bytes) else {
            return;
        };
        match frame.opcode {
            TerminalOpcode::Subscribe => self.handle_subscribe(frame),
            TerminalOpcode::Unsubscribe => {
                if let Some(handle) = self.streams.remove(&frame.stream_id) {
                    handle.forwarder.abort();
                    self.release_task(&handle.task_id);
                }
            }
            TerminalOpcode::Input => self.handle_input(&frame),
            TerminalOpcode::Resize => self.handle_resize(&frame),
            // 服务端→客户端方向的 opcode 不应从客户端出现,忽略
            _ => {}
        }
    }

    fn handle_subscribe(&mut self, frame: TerminalFrame) {
        let stream_id = frame.stream_id;
        let task_id = serde_json::from_slice::<Value>(&frame.payload)
            .ok()
            .and_then(|v| v.get("taskId").and_then(Value::as_str).map(str::to_string));
        let Some(task_id) = task_id else {
            let _ = self.outbound.send(frame_message(
                TerminalOpcode::Error,
                stream_id,
                0,
                b"Subscribe requires taskId".to_vec(),
            ));
            return;
        };
        if crate::pty::validate_task_id(&task_id).is_err() {
            let _ = self.outbound.send(frame_message(
                TerminalOpcode::Error,
                stream_id,
                0,
                b"Invalid taskId".to_vec(),
            ));
            return;
        }
        if !self.streams.contains_key(&stream_id)
            && self.streams.len() >= MAX_STREAMS_PER_CONNECTION
        {
            let _ = self.outbound.send(frame_message(
                TerminalOpcode::Error,
                stream_id,
                0,
                format!("Too many terminal streams (max {MAX_STREAMS_PER_CONNECTION})")
                    .into_bytes(),
            ));
            return;
        }
        // 同 streamId 重复订阅按「换任务」处理:停旧起新
        let mut already_acquired = false;
        if let Some(old) = self.streams.remove(&stream_id) {
            old.forwarder.abort();
            if old.task_id == task_id {
                already_acquired = true;
            } else {
                self.release_task(&old.task_id);
            }
        }
        if !already_acquired {
            let size = {
                let tm = self.app.state::<TaskManager>();
                crate::pty::current_task_pty_size(&tm, &task_id)
            };
            self.app
                .state::<RemoteState>()
                .terminal_leases
                .acquire(&task_id, size);
        }
        let app = self.app.clone();
        let outbound = self.outbound.clone();
        let forwarder_task_id = task_id.clone();
        let forwarder = tauri::async_runtime::spawn(async move {
            run_forwarder(app, outbound, stream_id, forwarder_task_id).await;
        });
        self.streams
            .insert(stream_id, StreamHandle { task_id, forwarder });
    }

    fn handle_input(&self, frame: &TerminalFrame) {
        let Some(handle) = self.streams.get(&frame.stream_id) else {
            return;
        };
        let Ok(data) = std::str::from_utf8(&frame.payload) else {
            return;
        };
        // 与桌面 send_input 同内核;writer 不存在(任务已结束)时静默忽略
        let tm = self.app.state::<TaskManager>();
        let _ = crate::pty::write_task_input(&tm, &handle.task_id, data);
    }

    fn handle_resize(&self, frame: &TerminalFrame) {
        let Some(handle) = self.streams.get(&frame.stream_id) else {
            return;
        };
        let Ok(meta) = serde_json::from_slice::<Value>(&frame.payload) else {
            return;
        };
        let (Some(cols), Some(rows)) = (
            meta.get("cols").and_then(Value::as_u64),
            meta.get("rows").and_then(Value::as_u64),
        ) else {
            return;
        };
        let (cols, rows) = (
            cols.min(u16::MAX as u64) as u16,
            rows.min(u16::MAX as u64) as u16,
        );
        let tm = self.app.state::<TaskManager>();
        if crate::pty::resize_task_pty(&tm, &handle.task_id, cols, rows).is_ok() {
            // 通知桌面前端(后写优先策略;桌面重新聚焦该终端时自然 refit)
            let _ = self.app.emit(
                "remote-terminal-resized",
                json!({ "task_id": handle.task_id, "cols": cols, "rows": rows }),
            );
            let _ = self.outbound.send(frame_message(
                TerminalOpcode::Resized,
                frame.stream_id,
                0,
                serde_json::to_vec(&json!({ "cols": cols, "rows": rows })).unwrap_or_default(),
            ));
        }
    }

    /// 连接关闭时停掉全部转发任务。
    pub fn abort_all(&mut self) {
        let handles: Vec<_> = self.streams.drain().map(|(_, handle)| handle).collect();
        for handle in handles {
            handle.forwarder.abort();
            self.release_task(&handle.task_id);
        }
    }

    fn release_task(&self, task_id: &str) {
        let original_size = self
            .app
            .state::<RemoteState>()
            .terminal_leases
            .release(task_id);
        let Some((cols, rows)) = original_size else {
            return;
        };
        let tm = self.app.state::<TaskManager>();
        if crate::pty::resize_task_pty(&tm, task_id, cols, rows).is_ok() {
            let _ = self.app.emit(
                "remote-terminal-resized",
                json!({
                    "task_id": task_id,
                    "cols": cols,
                    "rows": rows,
                    "restored": true,
                }),
            );
        }
    }
}

/// 快照 + live 转发主循环。外层 loop 的每一轮 = 一次完整快照;
/// 返回 = 连接关闭(outbound 失效)或 hub 通道关闭。
async fn run_forwarder<R: Runtime>(
    app: AppHandle<R>,
    outbound: OutboundSender,
    stream_id: u32,
    task_id: String,
) {
    let mut rx = hub().subscribe(&task_id);
    let mut seq: u64 = 0;
    loop {
        let epoch = hub().current_epoch(&task_id);
        let (live, size) = {
            let tm = app.state::<TaskManager>();
            let live = tm.pty_writers.lock().contains_key(&task_id);
            (live, crate::pty::current_task_pty_size(&tm, &task_id))
        };
        let tail_task_id = task_id.clone();
        let tail = tauri::async_runtime::spawn_blocking(move || {
            crate::storage::read_task_terminal_history_tail(&tail_task_id, SNAPSHOT_TAIL_BYTES)
        })
        .await;
        let (watermark, text) = match tail {
            Ok(Ok(pair)) => pair,
            _ => (0, String::new()),
        };

        let meta = json!({
            "cols": size.map(|s| s.0),
            "rows": size.map(|s| s.1),
            "live": live,
        });
        if outbound
            .send(frame_message(
                TerminalOpcode::SnapshotStart,
                stream_id,
                0,
                serde_json::to_vec(&meta).unwrap_or_default(),
            ))
            .is_err()
        {
            return;
        }
        for chunk in chunk_at_char_boundaries(&text, SNAPSHOT_CHUNK_BYTES) {
            if outbound
                .send(frame_message(
                    TerminalOpcode::SnapshotChunk,
                    stream_id,
                    0,
                    chunk.as_bytes().to_vec(),
                ))
                .is_err()
            {
                return;
            }
        }
        if outbound
            .send(frame_message(
                TerminalOpcode::SnapshotEnd,
                stream_id,
                0,
                Vec::new(),
            ))
            .is_err()
        {
            return;
        }

        // live 增量:水位之上的才透传;epoch 变化或落后 → 重新快照。
        // 高频输出时先排空积压合并成大帧(见 drain_pending),降低帧数与手机端渲染压力。
        loop {
            match rx.recv().await {
                Ok(chunk) => {
                    let mut merged = String::new();
                    if chunk.epoch != epoch {
                        break;
                    }
                    if chunk.end_offset > watermark {
                        merged.push_str(&chunk.data);
                    }
                    let outcome = drain_pending(&mut rx, epoch, watermark, &mut merged);
                    if !merged.is_empty() {
                        seq += 1;
                        // 超大合并批防御:按帧上限切片(正常批量 ≤64KB,远低于上限)
                        for piece in chunk_at_char_boundaries(&merged, MAX_PAYLOAD_BYTES) {
                            if outbound
                                .send(frame_message(
                                    TerminalOpcode::Output,
                                    stream_id,
                                    seq,
                                    piece.as_bytes().to_vec(),
                                ))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    match outcome {
                        DrainOutcome::Continue => {}
                        DrainOutcome::Resnapshot => break,
                        DrainOutcome::Closed => return,
                    }
                }
                Err(RecvError::Lagged(_)) => break,
                Err(RecvError::Closed) => return,
            }
        }
    }
}

enum DrainOutcome {
    /// 队列已排空,继续等下一批。
    Continue,
    /// 遇到 epoch 变化或 Lagged:发完手头内容后重新快照。
    Resnapshot,
    /// hub 通道关闭。
    Closed,
}

/// 合帧上限:单次合并不超过该字节数,防止长时间不出帧。
const COALESCE_LIMIT_BYTES: usize = 48 * 1024;

/// 把 broadcast 队列里已积压的 chunk(同 epoch、水位之上)合并进 `merged`。
/// 不等待新数据(try_recv),只吃掉当下积压。
fn drain_pending(
    rx: &mut tokio::sync::broadcast::Receiver<super::terminal_hub::TerminalChunk>,
    epoch: u32,
    watermark: u64,
    merged: &mut String,
) -> DrainOutcome {
    use tokio::sync::broadcast::error::TryRecvError;
    loop {
        if merged.len() >= COALESCE_LIMIT_BYTES {
            return DrainOutcome::Continue;
        }
        match rx.try_recv() {
            Ok(chunk) => {
                if chunk.epoch != epoch {
                    return DrainOutcome::Resnapshot;
                }
                if chunk.end_offset <= watermark {
                    continue;
                }
                merged.push_str(&chunk.data);
            }
            Err(TryRecvError::Empty) => return DrainOutcome::Continue,
            Err(TryRecvError::Lagged(_)) => return DrainOutcome::Resnapshot,
            Err(TryRecvError::Closed) => return DrainOutcome::Closed,
        }
    }
}

fn chunk_at_char_boundaries(text: &str, max_bytes: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.len() <= max_bytes {
            chunks.push(rest);
            break;
        }
        let mut end = max_bytes;
        while end > 0 && !rest.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            // 理论不可达(单字符 ≤4 字节 << max_bytes),防御死循环
            chunks.push(rest);
            break;
        }
        let (head, tail) = rest.split_at(end);
        chunks.push(head);
        rest = tail;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunking_respects_char_boundaries() {
        let text = "你好世界".repeat(100); // 1200 字节,全 3 字节字符
        let chunks = chunk_at_char_boundaries(&text, 32);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.len() <= 32));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunking_small_input_is_single_chunk() {
        assert_eq!(chunk_at_char_boundaries("abc", 32), vec!["abc"]);
        assert!(chunk_at_char_boundaries("", 32).is_empty());
    }

    fn chunk(
        epoch: u32,
        end_offset: u64,
        data: &str,
    ) -> crate::remote::terminal_hub::TerminalChunk {
        crate::remote::terminal_hub::TerminalChunk {
            epoch,
            end_offset,
            data: std::sync::Arc::from(data),
        }
    }

    #[test]
    fn drain_pending_merges_backlog_and_respects_watermark() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        assert!(tx.send(chunk(1, 5, "below-watermark")).is_ok());
        assert!(tx.send(chunk(1, 15, "abc")).is_ok());
        assert!(tx.send(chunk(1, 20, "def")).is_ok());
        let mut merged = String::new();
        let outcome = drain_pending(&mut rx, 1, 10, &mut merged);
        assert!(matches!(outcome, DrainOutcome::Continue));
        assert_eq!(merged, "abcdef");
        // 队列已排空
        let outcome = drain_pending(&mut rx, 1, 10, &mut merged);
        assert!(matches!(outcome, DrainOutcome::Continue));
        assert_eq!(merged, "abcdef");
    }

    #[test]
    fn drain_pending_signals_resnapshot_on_epoch_change_keeping_merged() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        assert!(tx.send(chunk(1, 10, "old")).is_ok());
        assert!(tx.send(chunk(2, 5, "new-epoch")).is_ok());
        let mut merged = String::new();
        let outcome = drain_pending(&mut rx, 1, 0, &mut merged);
        assert!(matches!(outcome, DrainOutcome::Resnapshot));
        // epoch 变化前的内容保留,由调用方发出后再重快照
        assert_eq!(merged, "old");
    }

    #[test]
    fn drain_pending_reports_closed_channel() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        drop(tx);
        let mut merged = String::new();
        assert!(matches!(
            drain_pending(&mut rx, 1, 0, &mut merged),
            DrainOutcome::Closed
        ));
    }

    #[test]
    fn terminal_lease_restores_first_size_after_last_subscriber() {
        let leases = TerminalLeaseRegistry::default();
        leases.acquire("task-1", Some((120, 40)));
        leases.acquire("task-1", Some((80, 24)));

        assert_eq!(leases.subscriber_count("task-1"), 2);
        assert_eq!(leases.release("task-1"), None);
        assert_eq!(leases.subscriber_count("task-1"), 1);
        assert_eq!(leases.release("task-1"), Some((120, 40)));
        assert_eq!(leases.subscriber_count("task-1"), 0);
    }
}
