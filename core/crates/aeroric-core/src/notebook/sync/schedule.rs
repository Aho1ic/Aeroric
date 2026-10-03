//! 自动同步的调度决策。**纯逻辑:不开线程、不读时钟、不碰网络。**
//!
//! 时间一律由调用方以 `now_ms` 传进来,和 `engine::run` 同一个口径。这样「攒够 500ms
//! 安静期才跑」「失败之后退避多久」这些规则可以用普通断言钉住,而不是靠 `sleep` 去撞。
//! 线程那一半在 [`super::daemon`],它只负责睡觉、收文件事件、调 `engine::run`。
//!
//! ## 三条触发路径
//!
//! ```text
//! 本地改动 → 攒安静期(debounce)→ 跑
//! 什么都没发生 → 兜底轮询 → 跑          ← 远端改动只能靠这条路发现
//! 上一轮失败 → 退避 → 再跑
//! ```
//!
//! 兜底间隔比参照实现(`usage_index.rs` 的 5s)长一个数量级。那个循环扫的是本地磁盘,
//! 而这里每一轮都要 `read_dir` 一个云盘根 —— 一个 vault 挂两个远端、五个 vault 同时开着
//! 自动同步,5s 一轮就是每分钟一百多次远端请求,对 18 家里限流严的那几家等于自找封禁。
//!
//! ## 退避只认 `Failed`,不认 `Pending`
//!
//! 这一条是这个模块里最容易写错、错了又最难发现的地方。
//!
//! `engine::run` 在有冲突待用户处理时返回 `Pending { detail: "awaiting_user" }`,而且
//! **不推进 seq**。从「这一轮有没有全部落定」的角度看它和失败长得一样,但它是**稳态** ——
//! 用户可能一周都不来处理那个冲突。如果拿它驱动退避,后果是:
//!
//! ```text
//! 一个文件等用户决策 → 每轮都 not settled → 失败计数一路涨 → 退避到 30 分钟一轮
//!                    → 这个 vault 里**其它所有笔记**都跟着停在 30 分钟一轮
//! ```
//!
//! 也就是说一个没人管的冲突会把整个 vault 的同步拖死。所以 [`RunResult`] 把这两件事分开,
//! 只有 `Failed` 进失败计数。

use std::time::Duration;

/// 攒多久安静期才认为「用户停手了」。与 `usage_index.rs` 的 `EVENT_DEBOUNCE` 一致。
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// 安静期最多推迟多久。
///
/// 没有这个上限的话,纯 debounce 会在「用户连续写十分钟」时**一次都不同步** —— 每次
/// 敲键盘都把等待重置。写长笔记正是随手记的主场景,所以第一次改动之后最多再等这么久
/// 就必须跑一轮,哪怕手还没停。
pub const MAX_DEBOUNCE_WAIT: Duration = Duration::from_secs(30);

/// 什么都没发生时的兜底轮询间隔。**远端改动只能靠这条路发现** —— 别人的设备改了云盘,
/// 本机不会收到任何文件事件。
pub const FALLBACK: Duration = Duration::from_secs(60);

/// 第一次失败之后等多久重试。
pub const BACKOFF_BASE: Duration = Duration::from_secs(10);

/// 退避上限。到顶之后不再翻倍 —— 离线一整天回来时,用户希望的是「插上网就恢复」,
/// 而不是「再等两小时」。
pub const BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);

/// 一轮同步的结果,只保留调度关心的那一点信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    /// 全部落定。清失败计数。
    Settled,
    /// 有条目挂起等用户(典型是冲突)。**不算失败**,理由见模块文档。
    Pending,
    /// 真的出错了:网络断、凭据过期、远端拒绝。进失败计数,触发退避。
    Failed,
}

/// 一个远端目标的调度状态。**只在内存里**,进程重启就是干净的 —— 重启后立刻跑一轮
/// 正是想要的行为,而把退避计数持久化会让「重启一下试试」这个用户直觉失效。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteRuntime {
    /// 自动同步开着没有。关着就永远不跑,手动那条路不看这个字段。
    pub auto: bool,
    /// 上一轮**开始**的时间。兜底间隔和退避都从这里算。
    ///
    /// 用开始时间而不是结束时间:一轮同步可能跑几十秒,拿结束时间算的话实际间隔会被
    /// 悄悄拉长成「间隔 + 每轮耗时」,慢的远端上尤其明显。
    pub last_attempt_ms: Option<i64>,
    /// 连续失败次数。只由 [`RunResult::Failed`] 增长。
    pub failures: u32,
    /// 攒着的本地改动。`None` 表示没有待同步的本地改动。
    ///
    /// 两个时间点包在一个 `Option` 里而不是两个字段:它们必须同生同灭,而两个
    /// `Option<i64>` 在类型上允许 `(Some, None)` —— 那个状态没有意义,却会被 [`decide`]
    /// 里的兜底分支静默当成「不脏」,于是用户的改动要等一整个兜底周期才被发现。
    /// 表示不出来的状态不需要测试。
    pub dirty: Option<PendingChanges>,
}

/// 攒着的本地改动的两个时间点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingChanges {
    /// 第一次改动。[`MAX_DEBOUNCE_WAIT`] 那个上限从这里算。
    pub first_ms: i64,
    /// 最近一次改动。安静期从这里算。
    pub last_ms: i64,
}

/// 该干什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 现在就跑。
    Run,
    /// 再等这么多毫秒。调用方可以睡这么久,也可以被文件事件提前唤醒。
    Wait(i64),
    /// 自动同步关着。
    Off,
}

impl RemoteRuntime {
    /// 收到一次本地改动。
    ///
    /// `first_ms` 记的是**第一次**改动,后续改动不覆盖它 —— 覆盖了就等于取消了
    /// [`MAX_DEBOUNCE_WAIT`] 那个上限,连续输入时会永远推迟。
    pub fn note_local_change(&mut self, now_ms: i64) {
        match &mut self.dirty {
            Some(pending) => pending.last_ms = now_ms,
            None => {
                self.dirty = Some(PendingChanges {
                    first_ms: now_ms,
                    last_ms: now_ms,
                })
            }
        }
    }

    /// 一轮开始。
    pub fn note_attempt_started(&mut self, now_ms: i64) {
        self.last_attempt_ms = Some(now_ms);
    }

    /// 一轮结束。`started_ms` 是这一轮 [`note_attempt_started`](Self::note_attempt_started)
    /// 时给的那个值。
    ///
    /// 只在「这一轮开始之后没有新改动」时清 dirty。同步跑一轮要扫全库、传文件,期间用户
    /// 完全可能又改了几个字 —— 无条件清掉的话那几个字要等到下一次兜底轮询(一分钟)才
    /// 被发现,而用户以为自动同步是即时的。
    ///
    /// **失败的那一轮也不清。** dirty 的语义是「有本地改动还没同步成功」,而失败恰好意味着
    /// 没成功,改动确实还在。清掉的后果不只是漏一次:清掉之后 [`decide`] 走不到 debounce
    /// 那条路,只剩兜底轮询,而 [`FALLBACK`](60s)比前几档退避(10/20/40s)都长,于是
    /// `due` 恒取兜底那一边 —— **退避的前三档等于不存在**,一次网络抖动要让用户的编辑
    /// 等满一分钟。
    pub fn note_attempt_finished(&mut self, started_ms: i64, result: RunResult) {
        match result {
            RunResult::Failed => {
                self.failures = self.failures.saturating_add(1);
                // 没传上去,dirty 原样留着,由退避决定什么时候再试。
                return;
            }
            // Pending 不清也不加:它既不是进展也不是错误。挂起的那条要等用户处理,
            // 而不是等一次更快的重试,所以 dirty 照常清 —— 下一轮扫描会重新发现它。
            RunResult::Pending => {}
            RunResult::Settled => self.failures = 0,
        }
        let changed_during_run = self.dirty.is_some_and(|p| p.last_ms > started_ms);
        if !changed_during_run {
            self.dirty = None;
        }
    }
}

/// 失败 `failures` 次之后该等多久。`failures == 0` 时没有额外等待。
///
/// 指数退避,`BACKOFF_BASE * 2^(failures-1)`,封顶 [`BACKOFF_MAX`]。翻倍用
/// `checked_mul` ——  离线几小时后 `failures` 能到几十,`1 << 40` 直接溢出,而溢出成
/// 一个小数字的后果是退避突然消失、变成对着一个连不上的远端每秒重试。
pub fn backoff_delay(failures: u32) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    let shift = failures - 1;
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let millis = (BACKOFF_BASE.as_millis() as u64).saturating_mul(factor);
    Duration::from_millis(millis).min(BACKOFF_MAX)
}

/// 算出现在该干什么。
pub fn decide(now_ms: i64, rt: &RemoteRuntime) -> Decision {
    if !rt.auto {
        return Decision::Off;
    }

    // 从没跑过 → 立刻跑一轮。冷启动时先对齐一次,不然用户开着应用坐一分钟才看到远端
    // 的东西下来。
    let Some(last_attempt) = rt.last_attempt_ms else {
        return Decision::Run;
    };

    // 退避是**下限**,压在下面所有路径之上:失败期间连兜底轮询也要跟着慢下来,否则
    // 「退避」只挡住了因改动触发的那条路,连不上的远端照样每分钟被戳一次。
    let earliest = last_attempt.saturating_add(millis_of(backoff_delay(rt.failures)));

    let target = match rt.dirty {
        Some(pending) => {
            // 安静期到了就跑;手一直没停,也不能超过第一次改动之后的上限。
            let quiet = pending.last_ms.saturating_add(millis_of(DEBOUNCE));
            let capped = pending
                .first_ms
                .saturating_add(millis_of(MAX_DEBOUNCE_WAIT));
            quiet.min(capped)
        }
        // 没有攒着的改动 → 兜底轮询。
        None => last_attempt.saturating_add(millis_of(FALLBACK)),
    };

    let due = target.max(earliest);
    if now_ms >= due {
        Decision::Run
    } else {
        Decision::Wait(due - now_ms)
    }
}

/// `Duration` → 毫秒的 i64。上限截断而不是回绕 —— 这些常量都是秒级,截断只在有人把
/// 常量改成天文数字时才发生,那时候截断成「很久」比回绕成「立刻」安全。
fn millis_of(d: Duration) -> i64 {
    d.as_millis().min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 epoch 量级的基准。不用 0 或 1000 —— 小基准会让「没读到」这类哨兵值和真实
    /// 时间戳长得一样,而那种混淆在这里正好会把 `None` 和「1970 年跑过一轮」搞混。
    const T0: i64 = 1_756_700_000_000;

    fn auto() -> RemoteRuntime {
        RemoteRuntime {
            auto: true,
            ..Default::default()
        }
    }

    fn ms(d: Duration) -> i64 {
        millis_of(d)
    }

    // ---- 开关 ----

    #[test]
    fn auto_sync_off_never_runs() {
        let mut rt = RemoteRuntime::default();
        rt.note_local_change(T0);
        assert_eq!(decide(T0 + 10_000, &rt), Decision::Off);
    }

    #[test]
    fn a_cold_start_runs_immediately() {
        // 没有 last_attempt 就立刻跑:开着应用坐一分钟才看到远端的东西下来是个明显的 bug。
        assert_eq!(decide(T0, &auto()), Decision::Run);
    }

    // ---- 安静期 ----

    #[test]
    fn a_local_change_waits_out_the_quiet_period() {
        let mut rt = auto();
        rt.note_attempt_started(T0);
        rt.note_local_change(T0 + 1_000);

        assert_eq!(decide(T0 + 1_000, &rt), Decision::Wait(ms(DEBOUNCE)));
        assert_eq!(
            decide(T0 + 1_000 + ms(DEBOUNCE) - 1, &rt),
            Decision::Wait(1)
        );
        assert_eq!(decide(T0 + 1_000 + ms(DEBOUNCE), &rt), Decision::Run);
    }

    #[test]
    fn a_later_change_pushes_the_quiet_period_back() {
        let mut rt = auto();
        rt.note_attempt_started(T0);
        rt.note_local_change(T0 + 1_000);
        rt.note_local_change(T0 + 1_400);

        // 从最近一次改动算,不是从第一次。
        assert_eq!(
            decide(T0 + 1_400 + ms(DEBOUNCE) - 1, &rt),
            Decision::Wait(1)
        );
        assert_eq!(decide(T0 + 1_400 + ms(DEBOUNCE), &rt), Decision::Run);
    }

    #[test]
    fn continuous_typing_still_syncs_at_the_cap() {
        // 纯 debounce 在「连续写十分钟」时一次都不同步,因为每次敲键盘都把等待重置。
        // 写长笔记是随手记的主场景,所以第一次改动之后最多再等 MAX_DEBOUNCE_WAIT。
        let mut rt = auto();
        rt.note_attempt_started(T0);
        let first = T0 + 1_000;
        rt.note_local_change(first);
        // 每 100ms 敲一下,一直敲到超过上限。
        let mut at = first;
        while at < first + ms(MAX_DEBOUNCE_WAIT) + 5_000 {
            at += 100;
            rt.note_local_change(at);
        }

        assert_eq!(
            decide(at, &rt),
            Decision::Run,
            "手一直没停也必须在上限处跑一轮"
        );
    }

    #[test]
    fn the_cap_is_measured_from_the_first_change_not_the_last() {
        let mut rt = auto();
        rt.note_attempt_started(T0);
        let first = T0 + 1_000;
        rt.note_local_change(first);
        rt.note_local_change(first + 100);

        // 上限落在 first + MAX_DEBOUNCE_WAIT。此刻安静期(last + DEBOUNCE)更早,所以
        // 取的是安静期 —— 两者取 min。
        assert_eq!(decide(first + 100 + ms(DEBOUNCE), &rt), Decision::Run);
        // dirty_since 不该被后续改动覆盖,否则上限永远推不到。
        assert_eq!(rt.dirty.expect("dirty").first_ms, first);
    }

    // ---- 兜底轮询 ----

    #[test]
    fn with_nothing_dirty_it_falls_back_to_polling() {
        let mut rt = auto();
        rt.note_attempt_started(T0);

        // 远端改动不会产生本地文件事件,只能靠这条路发现。
        assert_eq!(decide(T0, &rt), Decision::Wait(ms(FALLBACK)));
        assert_eq!(decide(T0 + ms(FALLBACK), &rt), Decision::Run);
    }

    #[test]
    fn the_fallback_interval_is_measured_from_the_start_of_the_run() {
        // 拿结束时间算的话,实际间隔会被悄悄拉长成「间隔 + 每轮耗时」。慢的远端上一轮
        // 跑 40 秒,用户看到的就是一分四十秒一次。
        let mut rt = auto();
        rt.note_attempt_started(T0);
        rt.note_attempt_finished(T0, RunResult::Settled);

        assert_eq!(decide(T0 + ms(FALLBACK), &rt), Decision::Run);
    }

    #[test]
    fn the_fallback_is_much_longer_than_the_local_scan_interval() {
        // 参照实现 usage_index 扫本地磁盘,5s 一轮。这里每轮要 read_dir 一个云盘根,
        // 同样的间隔会把限流严的几家云盘打成封禁。
        assert!(
            FALLBACK >= Duration::from_secs(30),
            "兜底间隔太短会对远端造成限流风险"
        );
    }

    // ---- 退避 ----

    #[test]
    fn backoff_grows_then_stops_at_the_cap() {
        assert_eq!(backoff_delay(0), Duration::ZERO);
        assert_eq!(backoff_delay(1), BACKOFF_BASE);
        assert_eq!(backoff_delay(2), BACKOFF_BASE * 2);
        assert_eq!(backoff_delay(3), BACKOFF_BASE * 4);
        assert_eq!(backoff_delay(u32::MAX), BACKOFF_MAX);
    }

    #[test]
    fn a_huge_failure_count_does_not_wrap_around_to_no_backoff() {
        // 离线几小时能攒到几十次失败,`1 << 40` 就溢出了。溢出成一个小数字的后果是
        // 退避突然消失,变成对着一个连不上的远端每秒重试。
        for failures in [30u32, 63, 64, 65, 1_000, u32::MAX] {
            assert_eq!(
                backoff_delay(failures),
                BACKOFF_MAX,
                "failures={failures} 时退避不该回绕"
            );
        }
    }

    #[test]
    fn a_failure_delays_the_next_attempt() {
        let mut rt = auto();
        rt.note_attempt_started(T0);
        rt.note_attempt_finished(T0, RunResult::Failed);
        rt.note_local_change(T0 + 100);

        // 有改动待同步,但退避压着。
        assert_eq!(
            decide(T0 + 1_000, &rt),
            Decision::Wait(ms(BACKOFF_BASE) - 1_000)
        );
        assert_eq!(decide(T0 + ms(BACKOFF_BASE), &rt), Decision::Run);
    }

    #[test]
    fn backoff_also_holds_back_the_fallback_poll() {
        // 退避是下限,压在所有路径之上。只挡改动那条路的话,连不上的远端照样每分钟
        // 被戳一次 —— 那就等于没有退避。
        let mut rt = auto();
        rt.note_attempt_started(T0);
        for _ in 0..8 {
            rt.note_attempt_finished(T0, RunResult::Failed);
        }
        let delay = ms(backoff_delay(8));
        assert!(delay > ms(FALLBACK), "这个用例要求退避已经超过兜底间隔");

        assert_eq!(
            decide(T0 + ms(FALLBACK), &rt),
            Decision::Wait(delay - ms(FALLBACK))
        );
        assert_eq!(decide(T0 + delay, &rt), Decision::Run);
    }

    #[test]
    fn a_success_clears_the_backoff() {
        let mut rt = auto();
        rt.note_attempt_started(T0);
        rt.note_attempt_finished(T0, RunResult::Failed);
        rt.note_attempt_finished(T0, RunResult::Failed);
        assert_eq!(rt.failures, 2);

        rt.note_attempt_started(T0 + 100_000);
        rt.note_attempt_finished(T0 + 100_000, RunResult::Settled);

        assert_eq!(rt.failures, 0, "插上网就该恢复,不能还欠着几轮退避");
        assert_eq!(decide(T0 + 100_000 + ms(FALLBACK), &rt), Decision::Run);
    }

    // ---- Pending 不是失败 ----

    #[test]
    fn a_pending_conflict_does_not_trigger_backoff() {
        // 这个模块里最容易写错的一条。一个没人处理的冲突每轮都让 run 报「没全部落定」,
        // 拿它驱动退避的话,失败计数一路涨到退避封顶,于是**这个 vault 里其它所有笔记**
        // 都跟着停在 15 分钟一轮 —— 一个冲突拖死整库。
        let mut rt = auto();
        rt.note_attempt_started(T0);
        for _ in 0..10 {
            rt.note_attempt_finished(T0, RunResult::Pending);
        }

        assert_eq!(rt.failures, 0, "awaiting_user 是稳态,不是错误");
        assert_eq!(
            decide(T0 + ms(FALLBACK), &rt),
            Decision::Run,
            "有冲突挂着,其它文件也要继续按正常节奏同步"
        );
    }

    #[test]
    fn a_pending_result_does_not_clear_an_existing_backoff_either() {
        // 反过来也要成立:Pending 既不加也不减。上一轮真失败了,这一轮部分成功但有条目
        // 挂起,不该被当成「恢复了」而立刻取消退避。
        let mut rt = auto();
        rt.note_attempt_started(T0);
        rt.note_attempt_finished(T0, RunResult::Failed);
        rt.note_attempt_finished(T0, RunResult::Pending);

        assert_eq!(rt.failures, 1);
    }

    // ---- 同步期间的改动 ----

    #[test]
    fn a_change_during_the_run_is_not_swallowed() {
        // 一轮同步要扫全库、传文件,期间用户完全可能又改了几个字。无条件清 dirty 的话
        // 那几个字要等到下一次兜底轮询(一分钟)才被发现,而用户以为自动同步是即时的。
        let mut rt = auto();
        rt.note_local_change(T0);
        let started = T0 + 1_000;
        rt.note_attempt_started(started);
        rt.note_local_change(started + 500);
        rt.note_attempt_finished(started, RunResult::Settled);

        assert!(rt.dirty.is_some(), "同步期间的改动必须留着");
        assert_eq!(decide(started + 500 + ms(DEBOUNCE), &rt), Decision::Run);
    }

    #[test]
    fn a_change_before_the_run_is_cleared_by_it() {
        let mut rt = auto();
        rt.note_local_change(T0);
        let started = T0 + 1_000;
        rt.note_attempt_started(started);
        rt.note_attempt_finished(started, RunResult::Settled);

        assert_eq!(rt.dirty, None);
        // 清干净之后回到兜底节奏,而不是立刻再跑一轮。
        assert_eq!(decide(started, &rt), Decision::Wait(ms(FALLBACK)));
    }

    #[test]
    fn a_change_exactly_at_the_start_instant_is_covered_by_the_run() {
        // 边界:`>` 而不是 `>=`。同一毫秒发生的改动算在这一轮里 —— 扫描是在
        // note_attempt_started 之后做的,所以它看得见。判成「期间改的」只会让每轮同步
        // 都留下一个假 dirty,于是自动同步永不停歇地空转。
        let mut rt = auto();
        rt.note_local_change(T0);
        rt.note_attempt_started(T0);
        rt.note_attempt_finished(T0, RunResult::Settled);

        assert_eq!(rt.dirty, None);
    }

    #[test]
    fn a_failed_run_keeps_the_pending_changes() {
        // dirty 的语义是「有本地改动还没同步成功」。失败恰好意味着没成功,所以改动还在。
        let mut rt = auto();
        rt.note_local_change(T0);
        rt.note_attempt_started(T0 + 100);
        rt.note_attempt_finished(T0 + 100, RunResult::Failed);

        assert_eq!(
            rt.dirty.expect("dirty").first_ms,
            T0,
            "失败没把改动传上去,不能把它当已同步"
        );
    }

    #[test]
    fn the_first_backoff_steps_are_not_swallowed_by_the_fallback() {
        // 这一条是上面那条 bug 的直接后果,单独钉住。
        //
        // 失败之后若 dirty 被清掉,`decide` 只剩兜底轮询这条路,而 FALLBACK(60s)比前
        // 三档退避(10/20/40s)都长,于是 `due = max(target, earliest)` 恒取兜底那一边 ——
        // 退避的前三档形同不存在,一次网络抖动要让用户的编辑等满一分钟。
        assert!(
            ms(backoff_delay(1)) < ms(FALLBACK),
            "这个用例要求第一档退避比兜底间隔短,否则它证明不了什么"
        );

        let mut rt = auto();
        rt.note_local_change(T0);
        rt.note_attempt_started(T0 + 100);
        rt.note_attempt_finished(T0 + 100, RunResult::Failed);

        assert_eq!(
            decide(T0 + 100, &rt),
            Decision::Wait(ms(BACKOFF_BASE)),
            "有改动待重试时,节奏由退避决定,不是兜底轮询"
        );
        assert_eq!(decide(T0 + 100 + ms(BACKOFF_BASE), &rt), Decision::Run);
    }

    #[test]
    fn each_backoff_step_actually_takes_effect() {
        // 逐档验一遍,不只验第一档。中间任何一档被兜底盖住都说明两个常量的关系出了问题。
        let mut rt = auto();
        rt.note_local_change(T0);
        for step in 1..=6u32 {
            rt.note_attempt_started(T0 + 100);
            rt.note_attempt_finished(T0 + 100, RunResult::Failed);
            assert_eq!(rt.failures, step);
            assert_eq!(
                decide(T0 + 100, &rt),
                Decision::Wait(ms(backoff_delay(step))),
                "第 {step} 档退避没生效"
            );
        }
    }
}
