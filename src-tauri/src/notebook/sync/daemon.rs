//! 自动同步的后台线程。决策全在 [`super::schedule`],这里只负责睡觉、收文件事件、
//! 调 `engine::run`。
//!
//! ## 一条线程,不是每个远端一条
//!
//! 一个 vault 可以挂多个远端,用户可以开着好几个 vault。按远端起线程的话数量没有上界,
//! 而每条线程绝大多数时间在 `sleep`。这里改成一条循环:每轮问所有远端「你要等多久」,
//! 睡其中最短的那个。代价是同一时刻只跑一个远端的同步 —— 那恰好也是想要的,几个远端
//! 同时上传会把上行带宽分光,每个都变慢。
//!
//! ## 事件回环是这个模块最大的坑
//!
//! 同步自己要写文件:下载写笔记、`store` 每轮写 `sync.db`(就在 vault 的 `.notebook/`
//! 下面)。这些写入会被 watcher 收成「本地改动」,于是:
//!
//! ```text
//! 同步一轮 → 写 sync.db → watcher 报改动 → 攒 500ms → 又同步一轮 → …
//! ```
//!
//! 这是个活锁,而且每轮都要 `read_dir` 一次云盘 —— 500ms 一次远端请求,几分钟就被限流。
//! 挡它的是 [`super::scan::is_out_of_scope`]:同一个谓词,本地扫描、远端列举、本地写入、
//! 这里的事件过滤,四处共用。`.notebook/` 在它的跳过名单里。
//!
//! 下载写出的**笔记**不在跳过名单里,那些事件会照常进来。那没关系而且是对的:引擎把
//! 下载后的内容写进基线,下一轮扫描发现本地 hash 与基线一致,得到一个空计划。也就是
//! 每次下载会多一轮空跑,不会连锁。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Manager};

use super::schedule::{self, Decision, RemoteRuntime, RunResult};
use super::store::{self, RemoteTarget};

/// 状态变化时发给前端的事件名。
pub const SYNC_EVENT: &str = "notebook-sync-updated";

/// 一轮循环最多睡多久。
///
/// 即使所有远端都说「等一小时」也只睡这么久:期间用户可能在设置里打开了某个远端的自动
/// 同步,而那个动作不产生文件事件。上限比[兜底间隔](schedule::FALLBACK)短一截,
/// 保证开关拨动之后最迟这么久就被看到。
const MAX_TICK: Duration = Duration::from_secs(5);

/// 一个远端的键。vault 路径 + 远端 id —— 两个 vault 可以用同一个 remote_id。
type Key = (PathBuf, String);

/// 进程内的调度状态。
///
/// 不持久化:重启之后立刻跑一轮、退避从零开始,正是用户「重启一下试试」的直觉。
static RUNTIME: OnceLock<Mutex<HashMap<Key, RemoteRuntime>>> = OnceLock::new();

fn runtime() -> &'static Mutex<HashMap<Key, RemoteRuntime>> {
    RUNTIME.get_or_init(|| Mutex::new(HashMap::new()))
}

fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 前端能看到的一个远端的当前状态。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub remote_id: String,
    pub auto_sync: bool,
    /// 连续失败次数。0 表示当前没有在退避。
    pub failures: u32,
    /// 有本地改动攒着还没同步成功。
    pub dirty: bool,
    /// 上一轮开始的时间。`None` 表示本次启动后还没跑过。
    pub last_attempt_ms: Option<i64>,
    /// 距离下一轮还有多少毫秒。`None` 表示自动同步关着。
    pub next_run_in_ms: Option<i64>,
}

/// 查一个 vault 下所有远端的调度状态。
///
/// 只读内存里那份 —— 目标列表由 `notebook_sync_remotes` 单独给,两者在前端合并。这样
/// 这条命令不碰数据库,可以被状态栏高频调用。
pub fn status_for(vault: &Path, remote_ids: &[String]) -> Vec<RemoteStatus> {
    let now = epoch_ms();
    let map = match runtime().lock() {
        Ok(map) => map,
        // 锁被毒化(某个持锁线程 panic 过)时报「没有状态」而不是跟着 panic:状态查询是
        // 显示用的,让一次显示失败把整个面板带崩没有道理。
        Err(poisoned) => poisoned.into_inner(),
    };
    remote_ids
        .iter()
        .map(|id| {
            let key = (vault.to_path_buf(), id.clone());
            let rt = map.get(&key).cloned().unwrap_or_default();
            let next = match schedule::decide(now, &rt) {
                Decision::Run => Some(0),
                Decision::Wait(ms) => Some(ms),
                Decision::Off => None,
            };
            RemoteStatus {
                remote_id: id.clone(),
                auto_sync: rt.auto,
                failures: rt.failures,
                dirty: rt.dirty.is_some(),
                last_attempt_ms: rt.last_attempt_ms,
                next_run_in_ms: next,
            }
        })
        .collect()
}

/// 让某个 vault 的所有远端立刻重算一次(用户拨了自动同步开关之后调)。
///
/// 只清 `auto` 之外的东西不动:开关的真值在数据库里,循环下一轮会读到。这里做的是把
/// 线程叫醒的效果 —— 实际上靠 [`MAX_TICK`] 兜住,所以这个函数只负责在关掉时把攒着的
/// 改动丢掉,免得重新打开时立刻按一个很旧的 `first_ms` 触发。
pub fn forget_pending(vault: &Path, remote_id: &str) {
    let mut map = match runtime().lock() {
        Ok(map) => map,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(rt) = map.get_mut(&(vault.to_path_buf(), remote_id.to_string())) {
        rt.dirty = None;
        rt.failures = 0;
    }
}

/// 一个 vault 里发生了一次改动(由 watcher 或前端保存路径调用)。
pub fn note_local_change(vault: &Path, now_ms: i64) {
    let mut map = match runtime().lock() {
        Ok(map) => map,
        Err(poisoned) => poisoned.into_inner(),
    };
    // 改动是 vault 级的:一次保存影响这个 vault 下所有远端。
    for ((v, _), rt) in map.iter_mut() {
        if v == vault {
            rt.note_local_change(now_ms);
        }
    }
}

/// 把数据库里的目标同步进内存状态,并返回这一轮要考虑的键。
///
/// 内存那份跟着数据库**收缩**:解绑之后 key 必须消失,否则重新绑同一个 id 会继承上次的
/// 退避计数,用户看到的是「刚绑好就说要等十五分钟」。
fn reconcile(vault: &Path, targets: &[RemoteTarget]) -> Vec<(String, RemoteRuntime)> {
    let mut map = match runtime().lock() {
        Ok(map) => map,
        Err(poisoned) => poisoned.into_inner(),
    };
    let live: HashSet<String> = targets.iter().map(|t| t.id.clone()).collect();
    map.retain(|(v, id), _| v != vault || live.contains(id));

    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let entry = map
            .entry((vault.to_path_buf(), target.id.clone()))
            .or_default();
        entry.auto = target.auto_sync;
        out.push((target.id.clone(), entry.clone()));
    }
    out
}

fn mark_started(vault: &Path, remote_id: &str, now_ms: i64) {
    let mut map = match runtime().lock() {
        Ok(map) => map,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(rt) = map.get_mut(&(vault.to_path_buf(), remote_id.to_string())) {
        rt.note_attempt_started(now_ms);
    }
}

fn mark_finished(vault: &Path, remote_id: &str, started_ms: i64, result: RunResult) {
    let mut map = match runtime().lock() {
        Ok(map) => map,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(rt) = map.get_mut(&(vault.to_path_buf(), remote_id.to_string())) {
        rt.note_attempt_finished(started_ms, result);
    }
}

/// 把一轮同步的结果归成调度关心的三档。
///
/// `seq` 是引擎给的判据:它只在**全部落定**时才推进。所以有 `Some` 就是 `Settled`。
/// 剩下两档要分清 —— 见 [`schedule`] 模块文档里「退避只认 Failed」那一段。
pub(crate) fn classify(report: &super::engine::SyncReport) -> RunResult {
    use super::engine::OutcomeStatus;
    if report.seq.is_some() {
        return RunResult::Settled;
    }
    let failed = report
        .outcomes
        .iter()
        .any(|o| matches!(o.status, OutcomeStatus::Failed { .. }));
    if failed {
        RunResult::Failed
    } else {
        RunResult::Pending
    }
}

/// 这个路径的改动该不该唤起同步。
///
/// **漏掉这个过滤就是活锁**:`sync.db` 在 vault 的 `.notebook/` 下,每轮同步都写它,
/// 于是「同步 → 写库 → 收到事件 → 500ms 后再同步」无穷循环,而每轮都要 `read_dir` 一次
/// 云盘。用同一个 `is_out_of_scope`,和扫描、远端列举、本地写入四处共用一套口径。
///
/// 拿不到相对路径(事件路径不在 vault 下)时返回 `false`:不认识的东西不触发同步。
pub(crate) fn event_is_in_scope(vault: &Path, changed: &Path) -> bool {
    let Ok(rel) = changed.strip_prefix(vault) else {
        return false;
    };
    let rel_str = rel_to_slash(rel);
    if rel_str.is_empty() {
        return false;
    }
    !super::scan::is_out_of_scope(&rel_str)
}

/// 相对路径 → `/` 分隔的字符串,和 `scan::FileSig::path` 同口径。
///
/// **必须是 `/`,不能是平台分隔符。**`is_out_of_scope` 按 `/` 切段,拿 `\` 分隔的字符串
/// 去问它的话整条路径会被当成一个段名,于是 `.notebook\sync.db` 说自己在范围内 —— 活锁
/// 只在 Windows 上出现,而那正是最难复现的一种。
///
/// 抽成独立函数是为了让那个性质有个直接的落点:在 Unix 上 `MAIN_SEPARATOR_STR` 恰好就是
/// `/`,所以「用错分隔符」这个改动在本机是等价变异,任何 macOS 上的断言都杀不掉它。
/// 唯一能杀的是 Windows 上跑的那条 `#[cfg(windows)]` 测试。
fn rel_to_slash(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// 跑一轮云盘同步。返回 `None` 表示这个目标这一轮不该由守护线程处理。
fn run_once(vault: &Path, target: &RemoteTarget) -> Option<Result<RunResult, String>> {
    // git / p2p 目标不走这条路。它们的失败模式和用户确认都不一样。
    if target.kind != "cloud" {
        return None;
    }
    let started = epoch_ms();
    mark_started(vault, &target.id, started);

    let outcome = (|| -> Result<RunResult, String> {
        let device = {
            let conn = store::open(vault)?;
            store::device_id(&conn)?
        };
        let connection = crate::storage_conn::find_connection(&target.connection_id)?;
        let backend = crate::storage_backend::build_backend(&connection)?;
        let mut remote = super::remote::StorageRemote::open(
            backend.as_ref(),
            &target.root,
            &device,
            target.seq + 1,
        );
        let mut local = super::local::VaultLocalFs;
        let report = super::engine::run(
            vault,
            &target.id,
            super::diff::ConflictStrategy::Ask,
            epoch_ms(),
            &mut remote,
            &mut local,
        )?;
        Ok(classify(&report))
    })();

    // 连不上、凭据没了、库打不开 —— 都算失败,进退避。不区分是因为对调度来说没区别:
    // 都是「这一轮没成」,而退避的意义正是别对着一个连不上的目标猛敲。
    let result = match &outcome {
        Ok(result) => *result,
        Err(_) => RunResult::Failed,
    };
    mark_finished(vault, &target.id, started, result);
    Some(outcome)
}

/// watcher 的监听根跟着已注册 vault 走。
fn sync_watched(
    watcher: &mut Option<notify::RecommendedWatcher>,
    watched: &mut HashSet<PathBuf>,
    vaults: &[PathBuf],
) {
    let Some(watcher) = watcher.as_mut() else {
        return;
    };
    let live: HashSet<&PathBuf> = vaults.iter().collect();
    // 先撤掉已经注销的 —— 用户关掉一个 vault 之后还盯着它的目录,等于替一个不再同步的
    // 目标白收事件。
    let stale: Vec<PathBuf> = watched
        .iter()
        .filter(|p| !live.contains(p))
        .cloned()
        .collect();
    for path in stale {
        let _ = watcher.unwatch(&path);
        watched.remove(&path);
    }
    for vault in vaults {
        if watched.insert(vault.clone()) && watcher.watch(vault, RecursiveMode::Recursive).is_err()
        {
            // 加不上就别记着,下一轮再试(目录可能刚被临时移走)。
            watched.remove(vault);
        }
    }
}

fn registered_vaults(app: &AppHandle) -> Vec<PathBuf> {
    app.try_state::<crate::notebook::state::NotebookState>()
        .and_then(|state| state.registered_vaults().ok())
        .unwrap_or_default()
}

fn run_loop(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default()).ok();
    let mut watched: HashSet<PathBuf> = HashSet::new();

    loop {
        let vaults = registered_vaults(&app);
        sync_watched(&mut watcher, &mut watched, &vaults);

        let mut soonest = MAX_TICK;
        let mut changed_any = false;

        for vault in &vaults {
            // 库不存在就跳过 —— 用户还没配过同步,不该因为开着应用就在 vault 里建个空库。
            let Ok(Some(conn)) = store::open_existing(vault) else {
                continue;
            };
            let Ok(targets) = store::list_remotes(&conn) else {
                continue;
            };
            drop(conn);

            let decisions = reconcile(vault, &targets);
            for (id, rt) in decisions {
                match schedule::decide(epoch_ms(), &rt) {
                    Decision::Off => {}
                    Decision::Wait(ms) => {
                        let wait = Duration::from_millis(ms.max(0) as u64);
                        soonest = soonest.min(wait);
                    }
                    Decision::Run => {
                        let Some(target) = targets.iter().find(|t| t.id == id) else {
                            continue;
                        };
                        if run_once(vault, target).is_some() {
                            changed_any = true;
                        }
                    }
                }
            }
        }

        if changed_any {
            let _ = app.emit(SYNC_EVENT, ());
            // 同步自己写了文件(下载的笔记、`sync.db`)。范围内的那些事件已经排在队列里,
            // 收掉它们免得刚跑完的这一轮立刻把自己再触发一次。
            while rx.try_recv().is_ok() {}
        }

        match rx.recv_timeout(soonest) {
            Ok(Ok(event)) => {
                let now = epoch_ms();
                for path in &event.paths {
                    for vault in &vaults {
                        if event_is_in_scope(vault, path) {
                            note_local_change(vault, now);
                        }
                    }
                }
            }
            // 事件本身出错(watcher 内部丢事件、队列溢出)不该让循环停。下一轮兜底轮询
            // 会把漏掉的改动扫出来 —— 这也是兜底那条路存在的另一个理由。
            Ok(Err(_)) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // 发送端全没了,watcher 死了。退回纯轮询,别空转。
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                watcher = None;
                watched.clear();
                thread::sleep(soonest);
            }
        }
    }
}

pub fn start(app: AppHandle) {
    thread::spawn(move || run_loop(app));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notebook::sync::diff::{PlanSummary, SyncPlan};
    use crate::notebook::sync::engine::{ActionOutcome, OutcomeStatus, SyncReport};

    fn vault() -> PathBuf {
        PathBuf::from("/tmp/aeroric-daemon-test-vault")
    }

    // ---- 事件过滤:漏了就是活锁 ----

    #[test]
    fn a_write_to_the_sync_database_does_not_wake_the_scheduler() {
        // 每轮同步都写 `.notebook/sync.db`。不挡住的话「同步 → 写库 → 收事件 → 500ms 后
        // 再同步」无穷循环,而每轮要 read_dir 一次云盘 —— 几分钟就被限流。
        let v = vault();
        assert!(!event_is_in_scope(&v, &v.join(".notebook/sync.db")));
        assert!(!event_is_in_scope(&v, &v.join(".notebook/sync.db-wal")));
        assert!(!event_is_in_scope(
            &v,
            &v.join(".notebook").join("anything")
        ));
    }

    #[test]
    fn a_write_to_a_note_does_wake_the_scheduler() {
        let v = vault();
        assert!(event_is_in_scope(&v, &v.join("a.md")));
        assert!(event_is_in_scope(&v, &v.join("sub/b.md")));
    }

    #[test]
    fn the_skip_list_is_shared_with_the_scanner() {
        // 同一个谓词,四处共用(扫描、远端列举、本地写入、这里)。各自维护一份名单迟早
        // 会漂,而漂的方向恰好是「监听侧多收一类事件」= 活锁。
        let v = vault();
        for skipped in [".git", "node_modules", ".notebook"] {
            assert!(
                !event_is_in_scope(&v, &v.join(skipped).join("x")),
                "{skipped} 该被跳过"
            );
            assert!(super::super::scan::is_out_of_scope(&format!("{skipped}/x")));
        }
    }

    #[test]
    fn a_path_outside_the_vault_is_ignored() {
        // watcher 一次可能报好几个 vault 的事件,循环拿每个 vault 逐一问过。不在这个
        // vault 下的路径不该算它的改动。
        let v = vault();
        assert!(!event_is_in_scope(&v, Path::new("/tmp/elsewhere/a.md")));
        assert!(!event_is_in_scope(&v, &v));
    }

    #[test]
    fn a_nested_path_is_joined_with_forward_slashes() {
        // `is_out_of_scope` 按 `/` 切段,所以这里必须产出 `/` 分隔的串。
        //
        // 诚实地说清这条断言的边界:在 Unix 上 `MAIN_SEPARATOR_STR` 就是 `/`,所以把
        // `join("/")` 换成平台分隔符是**等价变异**,这条断言在本机杀不掉它。它的价值在
        // Windows 上 —— 见下面那条 `#[cfg(windows)]`。留在这里是为了钉住「多段路径要被
        // 逐段拼起来」这件事本身(比如有人改成只取最后一段)。
        let nested = PathBuf::from("a").join("b").join("c");
        assert_eq!(rel_to_slash(&nested), "a/b/c");
        assert_eq!(rel_to_slash(Path::new("")), "");
    }

    #[test]
    #[cfg(windows)]
    fn on_windows_a_backslash_path_is_still_filtered() {
        // 这条是上面那个等价变异的唯一杀手,只在 Windows 上跑(CI 覆盖)。
        //
        // 拿平台分隔符拼的话,`.notebook\sync.db` 整条被当成一个段名,`is_out_of_scope`
        // 说「不在跳过名单里」,于是每轮同步写完 sync.db 就把自己再触发一次 —— 活锁,
        // 而且只在这个平台出现。
        let v = PathBuf::from("C:\\aeroric-daemon-test-vault");
        assert!(!event_is_in_scope(&v, &v.join(".notebook").join("sync.db")));
        assert_eq!(rel_to_slash(Path::new("a\\b")), "a/b");
    }

    // ---- classify:三档不能混 ----

    fn report(seq: Option<i64>, statuses: Vec<OutcomeStatus>) -> SyncReport {
        SyncReport {
            plan: SyncPlan {
                actions: Vec::new(),
                summary: PlanSummary::default(),
            },
            outcomes: statuses
                .into_iter()
                .enumerate()
                .map(|(i, status)| ActionOutcome {
                    path: format!("f{i}.md"),
                    reason: "test",
                    status,
                })
                .collect(),
            tombstones_written: 0,
            seq,
        }
    }

    #[test]
    fn a_fully_settled_round_is_settled() {
        // seq 只在全部落定时推进,所以有 Some 就够了。
        let r = report(Some(8), vec![OutcomeStatus::Done]);
        assert_eq!(classify(&r), RunResult::Settled);
    }

    #[test]
    fn a_round_with_a_failure_is_failed() {
        let r = report(
            None,
            vec![
                OutcomeStatus::Done,
                OutcomeStatus::Failed {
                    error: "network".to_string(),
                },
            ],
        );
        assert_eq!(classify(&r), RunResult::Failed);
    }

    #[test]
    fn a_round_that_only_awaits_the_user_is_pending_not_failed() {
        // 这一条是退避正确性的入口。判成 Failed 的话,一个没人处理的冲突会让失败计数
        // 一路涨到退避封顶,把**整个 vault** 的同步拖到 15 分钟一轮。
        let r = report(
            None,
            vec![OutcomeStatus::Pending {
                detail: "awaiting_user",
            }],
        );
        assert_eq!(classify(&r), RunResult::Pending);
    }

    #[test]
    fn a_failure_outranks_a_pending_in_the_same_round() {
        // 混合的一轮:有条目挂起、也有条目真失败了。失败要算失败 —— 否则网络断了但正好
        // 还有个冲突挂着时,退避永远不启动,变成对着断网的远端每分钟猛敲。
        let r = report(
            None,
            vec![
                OutcomeStatus::Pending {
                    detail: "awaiting_user",
                },
                OutcomeStatus::Failed {
                    error: "network".to_string(),
                },
            ],
        );
        assert_eq!(classify(&r), RunResult::Failed);
    }

    // ---- reconcile:内存状态跟着数据库收缩 ----

    fn target(id: &str, auto: bool) -> RemoteTarget {
        RemoteTarget {
            id: id.to_string(),
            kind: "cloud".to_string(),
            root: "/notes".to_string(),
            connection_id: "c1".to_string(),
            last_sync_at: 0,
            seq: 0,
            auto_sync: auto,
        }
    }

    /// 测试之间共用一个进程内 static,所以每个用例用自己的 vault 路径隔开。
    fn scoped(tag: &str) -> PathBuf {
        PathBuf::from(format!("/tmp/aeroric-daemon-{tag}"))
    }

    #[test]
    fn reconcile_picks_up_the_auto_flag_from_the_database() {
        let v = scoped("auto-flag");
        let got = reconcile(&v, &[target("r1", true), target("r2", false)]);
        assert_eq!(got.len(), 2);
        assert!(got[0].1.auto);
        assert!(!got[1].1.auto);
    }

    #[test]
    fn unbinding_drops_the_runtime_state() {
        // 内存那份不跟着收缩的话,重新绑同一个 id 会继承上次的退避计数,用户看到的是
        // 「刚绑好就说要等十五分钟」。
        let v = scoped("unbind");
        reconcile(&v, &[target("r1", true)]);
        note_local_change(&v, 1_756_700_000_000);
        mark_finished(&v, "r1", 1_756_700_000_000, RunResult::Failed);
        assert_eq!(status_for(&v, &["r1".to_string()])[0].failures, 1);

        // 解绑:数据库里没有它了。
        reconcile(&v, &[]);
        let after = status_for(&v, &["r1".to_string()]);
        assert_eq!(after[0].failures, 0, "解绑之后不该留着退避计数");
        assert!(!after[0].dirty);
    }

    #[test]
    fn reconcile_only_touches_the_given_vault() {
        // 一个 vault 的目标列表变化不该影响另一个 vault 的状态。
        let a = scoped("scope-a");
        let b = scoped("scope-b");
        reconcile(&a, &[target("r1", true)]);
        reconcile(&b, &[target("r1", true)]);
        mark_finished(&b, "r1", 1_756_700_000_000, RunResult::Failed);

        reconcile(&a, &[]);
        assert_eq!(
            status_for(&b, &["r1".to_string()])[0].failures,
            1,
            "另一个 vault 的状态不该被牵连"
        );
    }

    // ---- 状态出口 ----

    #[test]
    fn status_reports_off_when_auto_sync_is_disabled() {
        let v = scoped("status-off");
        reconcile(&v, &[target("r1", false)]);
        let got = status_for(&v, &["r1".to_string()]);
        assert!(!got[0].auto_sync);
        assert_eq!(got[0].next_run_in_ms, None, "关着的时候没有「下一轮」");
    }

    #[test]
    fn status_for_an_unknown_remote_is_a_default_not_a_panic() {
        // 前端拿到的目标列表和内存状态可能差一拍(刚绑好、还没被循环看到)。那时候要给
        // 一个「关着」的默认值,不能索引崩掉。
        let v = scoped("status-unknown");
        let got = status_for(&v, &["never-seen".to_string()]);
        assert_eq!(got.len(), 1);
        assert!(!got[0].auto_sync);
        assert_eq!(got[0].failures, 0);
        assert_eq!(got[0].last_attempt_ms, None);
    }

    #[test]
    fn forget_pending_clears_the_backoff_too() {
        // 关掉再打开时,预期是「从现在开始」,不是「把三天前那次编辑补上」。
        let v = scoped("forget");
        reconcile(&v, &[target("r1", true)]);
        note_local_change(&v, 1_756_700_000_000);
        mark_finished(&v, "r1", 1_756_700_000_000, RunResult::Failed);

        forget_pending(&v, "r1");
        let got = status_for(&v, &["r1".to_string()]);
        assert!(!got[0].dirty);
        assert_eq!(got[0].failures, 0);
    }

    #[test]
    fn a_local_change_marks_every_remote_of_that_vault() {
        // 一次保存影响这个 vault 下所有远端 —— 两个远端各自要把这次改动传上去。
        let v = scoped("change-all");
        reconcile(&v, &[target("r1", true), target("r2", true)]);
        note_local_change(&v, 1_756_700_000_000);

        let got = status_for(&v, &["r1".to_string(), "r2".to_string()]);
        assert!(got[0].dirty);
        assert!(got[1].dirty);
    }

    #[test]
    fn a_non_cloud_target_is_left_to_its_own_command() {
        // git 目标的失败模式和用户确认都不一样,混进这条路只会让「同步失败」失去意义。
        let v = scoped("kind-guard");
        let mut git = target("g1", true);
        git.kind = "git".to_string();
        reconcile(&v, &[git.clone()]);

        assert!(
            run_once(&v, &git).is_none(),
            "守护线程不该替 git 目标跑云盘同步"
        );
    }
}
