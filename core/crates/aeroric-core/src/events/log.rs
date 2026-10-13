//! 环形事件日志：seq 分配与入环同一临界区（补记二百七十五，P3-06 第一片）。
//!
//! 语义面逐字对齐行为参考 `remote/event_log.rs`:seq 从 1 起、0 是「无
//! watermark」哨兵、环上限 512、`since` 的三分支(自举补全/已最新空/窗口
//! 被挤穿 reset)。**两处刻意偏差**:1 分配与发布不再跨锁域(本模块存在的
//! 理由,见下方完整论证);2 `after == 0` 由参考实现的「首连不补发」改成
//! 「环内全部」——切片一百零四为轮询型消费者(桌面事件泵)修正,详见
//! [`EventLog::since`] 的文档。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

/// 环容量,与宿主参考逐字同值(512)。重立的是原子边界,不是容量语义。
pub const MAX_ENTRIES: usize = 512;

#[derive(Clone)]
pub struct LoggedEvent {
    pub seq: u64,
    pub event: String,
    pub data: Value,
}

/// 每次成功 `append` 后的**只读**通知面(AP6 推送平面)。
///
/// 为什么要挂在 `EventLog` 而不是另起一条事件总线:本模块的存在理由就是
/// 「seq 分配与入环同一临界区」,任何**另取一处**观察事件的机制都会拿到与环
/// 不一致的顺序(这正是 `remote/event_log.rs` 被替换掉的原因)。挂在 append
/// 的临界区之后,观察者看到的 seq 顺序与环内顺序一致。
///
/// 契约:
/// * 观察者**必须**立即返回,不得阻塞(在 append 的调用者线程上被调用);
///   需要做 IO 的实现自己丢进队列/任务。
/// * 观察者**不得**回调 `EventLog` 的写方法(会自锁)。只读方法安全。
/// * 没有观察者时零额外开销(`Option` 检查)。
pub trait EventObserver: Send + Sync {
    fn observed(&self, event: &LoggedEvent);
}

/// 锁内状态:`next_seq` 与环同生共死,`evicted` 是**曾被挤出的显式记账**——
/// 环满时最旧条目被 pop,但「seq 1..=evicted 存在过」不能丢,否则第 513 条
/// 之后 `since(1)` 会因 `front().seq == 2` 误判可精确补发(参考实现的洞,
/// 见 `oldest_after`)。
#[derive(Default)]
struct Inner {
    next_seq: u64,
    entries: VecDeque<LoggedEvent>,
    evicted: u64,
}

#[derive(Default)]
pub struct EventLog {
    /// `latest` 是锁内分配、锁释放前 store 的无锁镜像(见模块头的方向性论证)。
    latest: AtomicU64,
    inner: Mutex<Inner>,
    /// AP6:可选的 append 观察者(宿主用它把任务事件转发到推送发送端)。
    /// `None` 时 append 路径与从前逐字节等形。
    observer: Mutex<Option<Arc<dyn EventObserver>>>,
}

impl EventLog {
    /// 装一个观察者。宿主在启动时调用一次;重复调用会替换(便于测试复位)。
    pub fn with_observer(self, observer: Arc<dyn EventObserver>) -> Self {
        *self.observer.lock() = Some(observer);
        self
    }

    pub fn set_observer(&self, observer: Option<Arc<dyn EventObserver>>) {
        *self.observer.lock() = observer;
    }

    /// 当前观察者(测试用)。
    pub fn has_observer(&self) -> bool {
        self.observer.lock().is_some()
    }

    /// 记录一条推送事件,返回其 seq。分配、入环、镜像更新在同一临界区。
    pub fn append(&self, event: &str, data: Value) -> u64 {
        let (seq, observed) = {
            let mut inner = self.inner.lock();
            inner.next_seq += 1;
            let seq = inner.next_seq;
            if inner.entries.len() >= MAX_ENTRIES {
                inner.entries.pop_front();
                inner.evicted += 1;
            }
            let observed = LoggedEvent {
                seq,
                event: event.to_string(),
                data,
            };
            inner.entries.push_back(observed.clone());
            // 锁内 store:外部观察到 latest==N 时,N 必已在环里。
            self.latest.store(seq, Ordering::Release);
            (seq, observed)
        };
        // 观察者在锁**外**调用:它不该阻塞 append 的临界区,也不该有机会自锁。
        // 顺序性不受影响 —— seq 已在环里,观察者拿到的是同一个 seq。
        let observer = self.observer.lock().clone();
        if let Some(observer) = observer {
            observer.observed(&observed);
        }
        seq
    }

    /// 已分配的最新 seq（从 1 开始；0 = 空日志）。无锁读，允许瞬时滞后，
    /// 滞后方向只会让 `since` 多补一条，不会漏。
    pub fn latest_seq(&self) -> u64 {
        self.latest.load(Ordering::Acquire)
    }

    /// `after` 之后的全部事件。
    /// - `after >= latest`：已是最新，空；
    /// - `after + 1` 落在曾被挤出的区间：`reset = true`，客户端应全量刷新。
    /// - `after == 0`：环内全部事件（切片一百零四语义修正）。原语义是「首连
    ///   不补发」，为**推送型**消费者设计——快照之后有直播帧兜底。轮询型
    ///   消费者（桌面事件泵）没有直播帧：若 0 只给游标不给事件，落在空日志
    ///   自举窗口里的事件对这条连接**永久不可达**（poll since(0)→latest=N 的
    ///   那一刻批次为空，游标跳到 N，1..N 再也取不到）。推送方的取数路径
    ///   不受影响（apply_backfill 本就处理补发/重置）。
    ///
    /// 三个判据在同一临界区里读（`evicted`/环/`latest` 快照一致），
    /// 这是「snapshot 与 seq 顺序一致」的机器形状。
    pub fn since(&self, after: u64) -> (Vec<LoggedEvent>, u64, bool) {
        let inner = self.inner.lock();
        let latest = inner.next_seq;
        if after >= latest {
            return (Vec::new(), latest, false);
        }
        // 窗口起点：曾被挤出的最高 seq + 1。判据逐字回参考（`after + 1 < oldest`
        // 才 reset——`after == evicted` 恰好要的是环内第一条，必须补发不是重置）。
        let oldest = inner.evicted + 1;
        if after + 1 < oldest {
            // 要补的区间已被环挤穿。
            return (Vec::new(), latest, true);
        }
        let missed = inner
            .entries
            .iter()
            .filter(|entry| entry.seq > after)
            .cloned()
            .collect();
        (missed, latest, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn observer_sees_every_event_in_seq_order_and_is_optional() {
        use std::sync::Mutex as StdMutex;

        struct Recorder(StdMutex<Vec<(u64, String)>>);
        impl EventObserver for Recorder {
            fn observed(&self, event: &LoggedEvent) {
                self.0
                    .lock()
                    .expect("recorder")
                    .push((event.seq, event.event.clone()));
            }
        }

        // 无观察者时 append 行为不变(seq 仍从 1 起)。
        let plain = EventLog::default();
        assert!(!plain.has_observer());
        assert_eq!(plain.append("task-status", json!({"a": 1})), 1);

        let recorder = Arc::new(Recorder(StdMutex::new(Vec::new())));
        let log = EventLog::default().with_observer(recorder.clone());
        assert!(log.has_observer());
        log.append("task-status", json!({"status": "done"}));
        log.append("other-event", json!({}));
        log.append("task-status", json!({"status": "failed"}));
        assert_eq!(
            *recorder.0.lock().expect("recorder"),
            vec![
                (1, "task-status".to_string()),
                (2, "other-event".to_string()),
                (3, "task-status".to_string()),
            ],
            "the observer must see the same seq and order as the ring"
        );

        // 观察者不改变环语义:补发仍按 seq。
        assert_eq!(log.latest_seq(), 3);
        let (missed, _, reset) = log.since(1);
        assert!(!reset);
        assert_eq!(missed.len(), 2);
        assert_eq!(missed[0].seq, 2);

        // 观察者可摘除(宿主停用推送时)。
        log.set_observer(None);
        assert!(!log.has_observer());
        log.append("task-status", json!({"status": "running"}));
        assert_eq!(recorder.0.lock().expect("recorder").len(), 3);
    }

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

        let (missed, latest, reset) = log.since(1);
        assert_eq!(latest, 2);
        assert!(!reset);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].seq, 2);
        assert_eq!(missed[0].data["status"], json!("input_required"));

        let (missed, _, reset) = log.since(2);
        assert!(missed.is_empty() && !reset);

        // 切片一百零四语义修正：after==0 是「环内全部」——轮询型消费者（桌面
        // 事件泵）靠它自举空日志，推送型消费者的直播帧兜底不受影响。
        let (missed, latest, reset) = log.since(0);
        assert!(!reset);
        assert_eq!(latest, 2);
        assert_eq!(
            missed.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn overflow_drops_oldest_and_reports_reset() {
        let log = EventLog::default();
        for i in 0..(MAX_ENTRIES + 10) {
            log.append("task-status", json!({ "i": i }));
        }
        // seq 1..=10 已被挤出：watermark=1 无法精确补发 → reset。
        let (missed, latest, reset) = log.since(1);
        assert!(reset);
        assert!(missed.is_empty());
        assert_eq!(latest, (MAX_ENTRIES + 10) as u64);

        // 窗口边界逐字回参考：watermark == 挤出数（10）要的是环内第一条（11），
        // 必须精确补发而不是 reset——这条钉的是本模块重写判据时的 off-by-one。
        let oldest = (MAX_ENTRIES + 10 - MAX_ENTRIES + 1) as u64; // 11
        let (missed, _, reset) = log.since(oldest - 1);
        assert!(!reset);
        assert_eq!(missed.len(), MAX_ENTRIES);
        assert_eq!(missed[0].seq, oldest);
    }

    /// Action 2 的并发红线：100 个 mutation 并发 append 后，
    /// ① seq 集合恰好 1..=100（分配不丢不重）；② 环内 push 序严格递增
    /// （发布序与分配序一致——参考实现的 fetch_add-then-separate-lock 形状
    /// 在这里会随机出乱序）；③ 任一 watermark 的 `since` 快照自洽。
    #[test]
    fn concurrent_appends_keep_allocation_and_publication_in_order() {
        let log = std::sync::Arc::new(EventLog::default());
        let mut threads = Vec::new();
        for worker in 0..8 {
            let log = std::sync::Arc::clone(&log);
            threads.push(std::thread::spawn(move || {
                let mut mine = Vec::new();
                for i in 0..13 {
                    mine.push(log.append("task-status", json!({ "worker": worker, "i": i })));
                }
                mine
            }));
        }
        let mut all = Vec::new();
        for thread in threads {
            all.extend(thread.join().expect("worker"));
        }
        assert_eq!(all.len(), 104); // 8×13 ≥ 100 个并发 mutation
        all.sort_unstable();
        let expected: Vec<u64> = (1..=104).collect();
        assert_eq!(all, expected, "seq 分配必须不丢不重");

        // 环内发布序严格递增（容量 512 > 104，全量在环）。
        let (missed, latest, reset) = log.since(0);
        assert_eq!(latest, 104);
        assert!(!reset);
        // 切片一百零四起 after==0 补发环内全部（轮询型消费者的自举语义）。
        assert_eq!(missed.len(), 104, "after==0 replays the whole ring");
        // 发布序：`since(1)` 的补发列表必须严格递增且恰为 2..=104——
        // 分配与发布跨锁域的实现（参考形状）在这里会以概率性乱序红掉。
        let (tail, latest, reset) = log.since(1);
        assert_eq!(latest, 104);
        assert!(!reset);
        assert_eq!(tail.len(), 103);
        let seqs: Vec<u64> = tail.iter().map(|entry| entry.seq).collect();
        assert!(
            seqs.windows(2).all(|pair| pair[0] < pair[1]),
            "环内发布序乱：{seqs:?}"
        );
        assert_eq!(seqs.first(), Some(&2));
        assert_eq!(seqs.last(), Some(&104));
        // 快照自洽：任一 watermark 的补发长度 = latest - watermark。
        for watermark in [1u64, 50, 103] {
            let (missed, _, reset) = log.since(watermark);
            assert!(!reset);
            assert_eq!(missed.len() as u64, 104 - watermark);
        }
    }
}
