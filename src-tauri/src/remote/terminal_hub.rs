//! 终端输出多订阅广播:PTY reader 线程 tee 一份到此,远程订阅者各持一个
//! broadcast receiver。桌面前端的 Channel 直投路径完全不受影响。
//!
//! ## 水位去重协议
//! 每条消息携带 `(epoch, end_offset, data)`:
//! - `end_offset` = 该 chunk 追加后终端历史文件的总字节数(append 先于 publish,
//!   且每任务只有一个 reader 线程,因此严格相等)。
//! - 订阅者先记录快照时刻的文件长度 H,此后只透传 `end_offset > H` 的消息,
//!   与快照精确衔接,既不丢行也不重复。
//! - `epoch` 在历史被 truncate(任务重跑)时递增,订阅者发现 epoch 变化即重新快照。
//!
//! 全局 OnceLock 而非挂在 RemoteState:publish 点位于 pty.rs 的 reader 线程,
//! 与远程服务是否启动无关;无订阅者时 publish 只有一次 map 读锁开销。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use tokio::sync::broadcast;

/// 每个订阅者的缓冲深度:64KB/帧 × 128 ≈ 8MB 上限;落后即 Lagged,
/// 由 server 侧重新快照恢复(历史文件保证无损)。
const CHANNEL_CAPACITY: usize = 128;

#[derive(Clone)]
pub struct TerminalChunk {
    pub epoch: u32,
    pub end_offset: u64,
    pub data: Arc<str>,
}

struct TaskChannel {
    sender: broadcast::Sender<TerminalChunk>,
    epoch: u32,
    /// None = 尚未 publish 过,首次 publish 时从历史文件长度回填。
    offset: Option<u64>,
}

pub struct TerminalHub {
    channels: RwLock<HashMap<String, TaskChannel>>,
}

static HUB: OnceLock<TerminalHub> = OnceLock::new();

pub fn hub() -> &'static TerminalHub {
    HUB.get_or_init(|| TerminalHub {
        channels: RwLock::new(HashMap::new()),
    })
}

impl TerminalHub {
    /// PTY reader 线程调用(紧跟历史 append 之后);无订阅者时近零开销。
    pub fn publish(&self, task_id: &str, data: &str) {
        if data.is_empty() {
            return;
        }
        // 常见情形(无人订阅)只付一次读锁;有订阅者时才进写锁推进水位。
        if !self.channels.read().contains_key(task_id) {
            return;
        }
        self.publish_slow(task_id, data);
    }

    fn publish_slow(&self, task_id: &str, data: &str) {
        let mut channels = self.channels.write();
        let Some(entry) = channels.get_mut(task_id) else {
            return;
        };
        let start = entry.offset.unwrap_or_else(|| {
            // 首次 publish:append 已完成,当前文件长度 - 本 chunk = 起始水位
            crate::storage::task_terminal_history_len(task_id).saturating_sub(data.len() as u64)
        });
        let end_offset = start + data.len() as u64;
        entry.offset = Some(end_offset);
        let chunk = TerminalChunk {
            epoch: entry.epoch,
            end_offset,
            data: Arc::from(data),
        };
        if entry.sender.send(chunk).is_err() && entry.sender.receiver_count() == 0 {
            channels.remove(task_id);
        }
    }

    pub fn subscribe(&self, task_id: &str) -> broadcast::Receiver<TerminalChunk> {
        {
            let channels = self.channels.read();
            if let Some(entry) = channels.get(task_id) {
                return entry.sender.subscribe();
            }
        }
        let mut channels = self.channels.write();
        channels
            .entry(task_id.to_string())
            .or_insert_with(|| TaskChannel {
                sender: broadcast::channel(CHANNEL_CAPACITY).0,
                epoch: 0,
                offset: None,
            })
            .sender
            .subscribe()
    }

    pub fn current_epoch(&self, task_id: &str) -> u32 {
        self.channels.read().get(task_id).map_or(0, |e| e.epoch)
    }

    /// 历史 truncate(任务重跑)时调用:换代并把水位归零。
    pub fn reset_for_truncate(&self, task_id: &str) {
        let mut channels = self.channels.write();
        if let Some(entry) = channels.get_mut(task_id) {
            entry.epoch = entry.epoch.wrapping_add(1);
            entry.offset = Some(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hub() -> TerminalHub {
        TerminalHub {
            channels: RwLock::new(HashMap::new()),
        }
    }

    #[test]
    fn publish_without_subscribers_is_noop() {
        let hub = test_hub();
        hub.publish("t1", "hello");
        assert!(hub.channels.read().is_empty());
    }

    #[tokio::test]
    async fn subscribers_receive_chunks_with_monotonic_offsets() {
        let hub = test_hub();
        let mut rx_a = hub.subscribe("t1");
        let mut rx_b = hub.subscribe("t1");
        hub.publish("t1", "abc");
        hub.publish("t1", "defg");
        let first = rx_a.recv().await.unwrap();
        let second = rx_a.recv().await.unwrap();
        assert_eq!(&*first.data, "abc");
        assert_eq!(second.end_offset - first.end_offset, 4);
        assert_eq!(&*rx_b.recv().await.unwrap().data, "abc");
    }

    #[tokio::test]
    async fn truncate_reset_bumps_epoch_and_rewinds_offset() {
        let hub = test_hub();
        let mut rx = hub.subscribe("t1");
        hub.publish("t1", "old");
        assert_eq!(rx.recv().await.unwrap().epoch, 0);
        hub.reset_for_truncate("t1");
        hub.publish("t1", "new");
        let chunk = rx.recv().await.unwrap();
        assert_eq!(chunk.epoch, 1);
        assert_eq!(chunk.end_offset, 3);
        assert_eq!(hub.current_epoch("t1"), 1);
    }

    #[tokio::test]
    async fn dropped_subscribers_get_cleaned_up_on_next_publish() {
        let hub = test_hub();
        let rx = hub.subscribe("t1");
        drop(rx);
        hub.publish("t1", "chunk");
        assert!(hub.channels.read().is_empty());
    }
}
