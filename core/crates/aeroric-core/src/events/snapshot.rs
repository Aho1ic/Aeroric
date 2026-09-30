//! watermark 快照与补发/直播合并（P3-06 第一片之二）。
//!
//! 客户端持有一个 watermark（最后**应用**的 seq）。两条路径会喂它：
//! - 重连补发：[`since_response`]——`events.since { after }` 的 RPC 响应体，
//!   与宿主参考逐字同形（`events[]/latestSeq/reset`）；
//! - 直播推送：[`SnapshotDecision::apply_live`]——`seq <= watermark` 的帧
//!   必须丢弃（补发已含），否则重连窗口里同一事件应用两次。
//!
//! 合并语义只有这一处实现；dispatcher（第三片）与测试都走这里，不再各写
//! 一份「比大小」。

use serde_json::{json, Value};

use super::log::{EventLog, LoggedEvent};

/// RPC `events.since { after }` 的响应体（字段名与宿主参考逐字一致）。
pub fn since_response(log: &EventLog, after: u64) -> Value {
    let (events, latest_seq, reset) = log.since(after);
    json!({
        "events": events
            .into_iter()
            .map(|event| json!({ "seq": event.seq, "event": event.event, "data": event.data }))
            .collect::<Vec<_>>(),
        "latestSeq": latest_seq,
        "reset": reset,
    })
}

/// 一个客户端的快照状态：最后应用的 seq（0 = 首连，无 watermark）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotDecision {
    pub watermark: u64,
    /// `reset` 为真：环已挤穿要补的区间，客户端应全量刷新而非增量应用。
    pub needs_full_refresh: bool,
}

impl SnapshotDecision {
    pub const fn at(watermark: u64) -> Self {
        Self {
            watermark,
            needs_full_refresh: false,
        }
    }

    /// 应用一份补发批次：逐条按序推进 watermark；`reset` 翻全量刷新位并
    /// 把 watermark 抬到批后 `latest`（全量刷新完成后客户端自行归零重记，
    /// 这里只保证不把旧洞当新数据）。
    pub fn apply_backfill(mut self, events: &[LoggedEvent], latest: u64, reset: bool) -> Self {
        if reset {
            self.needs_full_refresh = true;
            self.watermark = latest;
            return self;
        }
        for event in events {
            // 补发批次理论上严格递增；防御性地不回退。
            if event.seq > self.watermark {
                self.watermark = event.seq;
            }
        }
        self
    }

    /// 应用一帧直播推送。返回 `true` = 该帧应被消费（并把 watermark 抬到
    /// `seq`）；`false` = 重连窗口里的重复帧，丢弃。
    pub fn apply_live(&mut self, seq: u64) -> bool {
        if seq <= self.watermark {
            return false;
        }
        self.watermark = seq;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::log::{LoggedEvent, MAX_ENTRIES};
    use serde_json::json;

    fn event(seq: u64) -> LoggedEvent {
        LoggedEvent {
            seq,
            event: "task-status".to_string(),
            data: json!({ "seq": seq }),
        }
    }

    #[test]
    fn backfill_advances_watermark_in_order() {
        let mut snapshot = SnapshotDecision::at(0);
        snapshot = snapshot.apply_backfill(&[event(1), event(2), event(3)], 3, false);
        assert_eq!(snapshot.watermark, 3);
        assert!(!snapshot.needs_full_refresh);
        // 不回退：迟到的旧帧（理论上补发不含，防御性钉住）。
        snapshot = snapshot.apply_backfill(&[event(2)], 3, false);
        assert_eq!(snapshot.watermark, 3);
    }

    #[test]
    fn reset_marks_full_refresh_and_lifts_watermark() {
        let mut snapshot = SnapshotDecision::at(4);
        snapshot = snapshot.apply_backfill(&[], 600, true);
        assert!(snapshot.needs_full_refresh);
        assert_eq!(snapshot.watermark, 600, "全量刷新前不把旧洞当增量数据");
    }

    #[test]
    fn live_frames_at_or_below_watermark_are_dropped() {
        // 重连窗口：补发到 7 后，直播里 seq<=7 的重复帧必须丢。
        let mut snapshot = SnapshotDecision::at(7);
        assert!(!snapshot.apply_live(5));
        assert!(!snapshot.apply_live(7));
        assert!(snapshot.apply_live(8));
        assert_eq!(snapshot.watermark, 8);
    }

    #[test]
    fn since_response_keeps_the_reference_wire_shape() {
        let log = EventLog::default();
        for seq in 1..=3 {
            log.append("task-status", json!({ "i": seq }));
        }
        let response = since_response(&log, 1);
        assert_eq!(response["latestSeq"], 3);
        assert_eq!(response["reset"], false);
        assert_eq!(response["events"].as_array().expect("events").len(), 2);
        assert_eq!(response["events"][0]["seq"], 2);
        assert_eq!(response["events"][0]["event"], "task-status");

        // 挤穿窗口后：reset=true、events 空。
        for i in 4..=(MAX_ENTRIES + 5) {
            log.append("task-status", json!({ "i": i }));
        }
        let response = since_response(&log, 1);
        assert_eq!(response["reset"], true);
        assert_eq!(response["events"].as_array().expect("events").len(), 0);
    }
}
