//! P6-01 Action 5：文件 watcher 的 coalesce/overflow 策略（core 独占的
//! 纯决策；服务持有取消 token，不用 Swift Timer 高频轮询）。
//!
//! 语义与 `event_watcher.rs` 同源：recv_timeout 后排空同批事件（coalesce）；
//! 事件量溢出（overflow）→ rescan（整仓重扫一次，而不是每个 event 重读）。

/// watcher 决策：同批事件的去重路径集合；空集＝无事件。
pub struct WatchBatch {
    pub paths: Vec<String>,
}

/// coalesce：同批事件按 path 去重（每个 path 最多触发一次刷新，不是每个
/// event 重读）。
pub fn coalesce(paths: &[String]) -> WatchBatch {
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if seen.insert(path.clone()) {
            unique.push(path.clone());
        }
    }
    WatchBatch { paths: unique }
}

/// overflow 判定（Action 5：事件量溢出 → rescan）：单批超过 `overflow_limit`
/// 条即判定溢出（notify 环形缓冲被冲掉的信号），调用方整仓 rescan 一次。
pub const OVERFLOW_LIMIT: usize = 256;

pub fn should_rescan(batch_size: usize) -> bool {
    batch_size > OVERFLOW_LIMIT
}

/// 取消 token（服务持有；drop 即取消——不用 Swift Timer 轮询的显式化）。
pub struct CancelToken {
    cancelled: std::sync::atomic::AtomicBool,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            cancelled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}
