//! 远程推送事件日志:重连 watermark 补发的数据源。
//!
//! 只记录「错过会导致状态错乱/漏审批」的低频白名单事件(task-status /
//! task-session),不记高频的 session.appended(chat 视图重连后自行全量重拉)。
//! 环形缓冲上限 512 条;客户端带太旧的 watermark 时返回 reset,提示全量刷新。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde_json::{json, Value};

const MAX_ENTRIES: usize = 512;

#[derive(Clone)]
pub struct LoggedEvent {
    pub seq: u64,
    pub event: String,
    pub data: Value,
}

#[derive(Default)]
pub struct EventLog {
    /// 已分配的最新 seq(从 1 开始;0 = 「无 watermark」哨兵)。
    latest: AtomicU64,
    entries: Mutex<VecDeque<LoggedEvent>>,
}

impl EventLog {
    /// 记录一条推送事件,返回其 seq。
    pub fn append(&self, event: &str, data: Value) -> u64 {
        let seq = self.latest.fetch_add(1, Ordering::Relaxed) + 1;
        let mut entries = self.entries.lock();
        if entries.len() >= MAX_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(LoggedEvent {
            seq,
            event: event.to_string(),
            data,
        });
        seq
    }

    pub fn latest_seq(&self) -> u64 {
        self.latest.load(Ordering::Relaxed)
    }

    /// `after` 之后的全部事件。
    /// - `after == 0`:客户端没有 watermark(首连),不补发;
    /// - 缓冲已丢弃 `after+1`:返回 `reset=true`,客户端应全量刷新。
    pub fn since(&self, after: u64) -> (Vec<LoggedEvent>, u64, bool) {
        let entries = self.entries.lock();
        let latest = self.latest_seq();
        if after == 0 {
            return (Vec::new(), latest, false);
        }
        if after >= latest {
            return (Vec::new(), latest, false);
        }
        let oldest = entries.front().map(|e| e.seq).unwrap_or(latest + 1);
        if after + 1 < oldest {
            return (Vec::new(), latest, true);
        }
        let missed = entries.iter().filter(|e| e.seq > after).cloned().collect();
        (missed, latest, false)
    }
}

/// RPC `events.since { after }` 的响应体。
pub fn since_response(log: &EventLog, after: u64) -> Value {
    let (events, latest_seq, reset) = log.since(after);
    json!({
        "events": events
            .into_iter()
            .map(|e| json!({ "seq": e.seq, "event": e.event, "data": e.data }))
            .collect::<Vec<_>>(),
        "latestSeq": latest_seq,
        "reset": reset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_assigns_monotonic_seq_and_since_replays_missed() {
        let log = EventLog::default();
        assert_eq!(log.latest_seq(), 0);
        let s1 = log.append("task-status", json!({"task_id":"t1","status":"running"}));
        let s2 = log.append(
            "task-status",
            json!({"task_id":"t1","status":"input_required"}),
        );
        assert_eq!((s1, s2), (1, 2));

        // watermark=1 → 只补 seq 2
        let (missed, latest, reset) = log.since(1);
        assert_eq!(latest, 2);
        assert!(!reset);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].seq, 2);
        assert_eq!(missed[0].data["status"], json!("input_required"));

        // 已是最新 → 空
        let (missed, _, reset) = log.since(2);
        assert!(missed.is_empty() && !reset);

        // 无 watermark → 不补发
        let (missed, latest, reset) = log.since(0);
        assert!(missed.is_empty() && !reset);
        assert_eq!(latest, 2);
    }

    #[test]
    fn overflow_drops_oldest_and_reports_reset() {
        let log = EventLog::default();
        for i in 0..(MAX_ENTRIES + 10) {
            log.append("task-status", json!({ "i": i }));
        }
        // seq 1..=10 已被挤出:watermark=1 无法精确补发 → reset
        let (missed, latest, reset) = log.since(1);
        assert!(reset);
        assert!(missed.is_empty());
        assert_eq!(latest, (MAX_ENTRIES + 10) as u64);

        // 缓冲窗口内的 watermark 正常补发
        let oldest = (MAX_ENTRIES + 10 - MAX_ENTRIES + 1) as u64; // 11
        let (missed, _, reset) = log.since(oldest - 1);
        assert!(!reset);
        assert_eq!(missed.len(), MAX_ENTRIES);
        assert_eq!(missed[0].seq, oldest);
    }
}
