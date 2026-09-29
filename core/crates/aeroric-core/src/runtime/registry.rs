//! 宿主任务资源的命名空间与全部容器(`TaskRegistry`,P3-01 切片三自宿主 `lib.rs`
//! 与 `run_registry.rs` 一次性 move 而来)。宿主 `TaskManager` 是一个委托壳:
//! `Deref` 到本类型,`codex_rpc` 单例留宿主。
//!
//! ## 锁序表(自 `lib.rs` 迁入;新增多锁函数前先对照)
//!
//! 全局顺序(仅列会被同时持有的锁;叶子锁单独持有,不参与排序):
//! 1. `cancelled_tasks` / `manually_completed_tasks` —— 同族函数内成对出现,
//!    先 cancelled 后 manually(见宿主 pty.rs finalize/cancel/complete 三处);
//! 2. 四族 sessions(codex/claude/dsh/omp)彼此独立,从不同时持有两族;
//! 3. **嵌套持有**时的方向只有一个:`claimed_session_paths` → 会话表(dsh/omp 的登记侧)。
//!    拆除侧不嵌套——先删会话条目(临时锁)、再单独取 claim 做 ADR-031 的反查释放。
//!    3a. ADR-030:判断"是否最后一个退出者"要先 retire 自己再读 `runs`(叶子锁)。
//! 4. `pty_masters` → `pty_writers` → `child_handles` → `pending_pty_sizes`
//!    按 reset/cleanup 流程固定次序取放;
//! 5. `codex_rpc`(宿主)、`wsl_active_ids`、`initial_input_signals`、`runs` 为叶子锁。
//!
//! 约束:任何锁不得跨越 `.await` 持有(parking_lot 无中毒,但 await 上的死锁不受锁序
//! 保护);omp/远端 waiter 线程取锁必须用显式作用域限界。
//!
//! 一次性 move 的理由(计划 Action 2):禁止两份全局 map 同时成为真相源,所以这里
//! 不是"core 先立副本再逐表切换",而是宿主 `TaskManager` 同一刀变成委托壳。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;

/// 四族转录的"哪次运行绑了哪份会话"条目(P3-01 切片三自宿主 `session.rs` /
/// `session_dsh.rs` move;宿主侧改为再导出)。全是纯数据:跨 crate 的注册表值
/// 不需要也不得携带窗口对象。
#[derive(Clone)]
pub struct CodexSessionInfo {
    pub session_id: String,
    pub session_path: String,
}

#[derive(Clone)]
pub struct ClaudeSessionInfo {
    pub session_id: String,
    pub session_path: String,
    /// true 表示 lazy attach 预先注入的占位条目(jsonl 还未落盘),
    /// `is_task_active` 和 `finalize_task_exit::had_agent_session` 都应跳过它。
    pub is_placeholder: bool,
}

/// omp 会话信息:仅记录 task_id 关联的 session id 与文件路径。
/// `session_id` 经 task-session 事件同步给前端,持久化在任务字段里。
#[derive(Clone)]
pub struct OmpSessionInfo {
    pub session_id: String,
    pub session_path: String,
}

#[derive(Clone)]
pub struct DshSessionInfo {
    pub session_id: String,
    pub session_path: String,
}

/// The key every `TaskManager` container is addressed by (ADR-020's decision 1).
///
/// `generation` is core's per-task number, arriving through `LaunchRequest`/`ResumeRequest`.
/// One value is reserved and it is not a number core minted:
/// [`RunKey::UNMINTED`] (`0`) means "this run was started by a launch path that has not gone
/// through core yet" — every shell, and every task run the old commands still start. It is a
/// constant, not an allocation, so this is not the fourth run counter ADR-020 exists to
/// prevent; and because core's numbers start above zero, an unminted run can never be
/// mistaken for the newest one once a real generation registers for the same task.
///
/// Two consequences worth stating plainly: two *unminted* runs of one task still share a slot
/// (that is today's overwrite behaviour, kept verbatim — it ends when a launch path starts
/// going through core, which is P3-01's job), and ADR-025's addressing rules are total over
/// the mixed set because `newest` reads registered generations, whatever their number.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunKey {
    id: String,
    generation: u64,
}

impl RunKey {
    /// The generation a run carries when no ledger minted it.
    pub const UNMINTED: u64 = 0;

    pub fn minted(task_id: &str, generation: u64) -> Self {
        debug_assert!(
            generation != Self::UNMINTED,
            "generation 0 belongs to unminted runs; core never mints it"
        );
        RunKey {
            id: task_id.to_string(),
            generation,
        }
    }

    pub fn unminted(id: &str) -> Self {
        RunKey {
            id: id.to_string(),
            generation: Self::UNMINTED,
        }
    }

    /// The task or shell this run belongs to — what the pre-ADR-020 lookups keyed on.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether core's ledger owns this run's identity.
    pub fn is_minted(&self) -> bool {
        self.generation != Self::UNMINTED
    }
}

#[derive(Default)]
pub struct RunRegistry {
    live: Mutex<BTreeMap<String, BTreeSet<u64>>>,
}

impl RunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `generation` of `task_id` is now a run this process knows about.
    /// Called when core's claim turns into a spawn, never to invent a number.
    pub fn register(&self, task_id: &str, generation: u64) {
        self.live
            .lock()
            .entry(task_id.to_string())
            .or_default()
            .insert(generation);
    }

    /// Retires one run. Returns whether anything was actually retired, so a duplicate exit
    /// report can be told apart from the first — an exit monitor that fires twice must not
    /// look like it stopped a second process.
    pub fn retire(&self, task_id: &str, generation: u64) -> bool {
        let mut live = self.live.lock();
        let Some(generations) = live.get_mut(task_id) else {
            return false;
        };
        let retired = generations.remove(&generation);
        if generations.is_empty() {
            live.remove(task_id);
        }
        retired
    }

    /// Drops every run of a task, returning what was live. Used by task deletion.
    pub fn retire_task(&self, task_id: &str) -> Vec<u64> {
        self.live
            .lock()
            .remove(task_id)
            .map(|set| set.into_iter().collect())
            .unwrap_or_default()
    }

    /// What a resource operation addresses (ADR-025 rule 1).
    pub fn newest(&self, task_id: &str) -> Option<u64> {
        self.live
            .lock()
            .get(task_id)
            .and_then(|set| set.iter().next_back().copied())
    }

    /// What an intent operation must reach (ADR-025 rule 2).
    pub fn live_generations(&self, task_id: &str) -> Vec<u64> {
        self.live
            .lock()
            .get(task_id)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn is_live(&self, task_id: &str, generation: u64) -> bool {
        self.live
            .lock()
            .get(task_id)
            .is_some_and(|set| set.contains(&generation))
    }

    /// Whether another run is still in flight for this task — the check behind ADR-017's
    /// "no terminal settlement while another generation is live".
    pub fn has_other_live(&self, task_id: &str, generation: u64) -> bool {
        self.live_generations(task_id)
            .into_iter()
            .any(|other| other != generation)
    }

    pub fn live_task_count(&self) -> usize {
        self.live
            .lock()
            .values()
            .filter(|set| !set.is_empty())
            .count()
    }

    /// ADR-025 rule 1, as a key. `None` means there is nothing to address — the distinction
    /// [`RunKey::UNMINTED`] exists to keep is that a registered unminted run is a real run
    /// (generation 0 is stored like any other), while "the counter reached 5" is not.
    pub fn resource_key(&self, task_id: &str) -> Option<RunKey> {
        self.newest(task_id).map(|generation| {
            if generation == RunKey::UNMINTED {
                RunKey::unminted(task_id)
            } else {
                RunKey::minted(task_id, generation)
            }
        })
    }

    /// ADR-025 rule 2, as keys: every live run an intent must reach, oldest first.
    pub fn intent_keys(&self, task_id: &str) -> Vec<RunKey> {
        self.live_generations(task_id)
            .into_iter()
            .map(|generation| {
                if generation == RunKey::UNMINTED {
                    RunKey::unminted(task_id)
                } else {
                    RunKey::minted(task_id, generation)
                }
            })
            .collect()
    }
}

// ── 启动态门控信号(自宿主 pty.rs move;版本戳只许本文件的 register/clear 对读) ─────

/// See the static below: a per-entry stamp, not a generation.
/// (名字本身不写进注释——ADR-032 的扫描是词法的,注释里的字面量也会被它算成读者。)
static STARTUP_SIGNAL_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 启动态门控信号:首条输入可能需要等 trust folder / hook 授权完成后再投递。
#[derive(Debug)]
pub enum StartupSignal {
    Output(String),
    UserInput,
    SessionReady,
    Cancelled,
}

#[derive(Clone)]
pub struct StartupSignalRegistration {
    /// See the file-level static: a per-entry stamp, not a generation.
    version: u64,
    sender: std::sync::mpsc::Sender<StartupSignal>,
}

/// Keyed by the run that registered it (ADR-020). The value still carries a version stamp,
/// because two unminted runs of one task share a key and the older one must not clear the
/// newer one's registration on its way out (ADR-032).
pub type StartupSignalRegistry = Arc<Mutex<HashMap<RunKey, StartupSignalRegistration>>>;

pub fn register_initial_input_signal(
    registry: &TaskRegistry,
    run: &RunKey,
    sender: std::sync::mpsc::Sender<StartupSignal>,
) -> u64 {
    let version = STARTUP_SIGNAL_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    registry
        .initial_input_signals
        .lock()
        .insert(run.clone(), StartupSignalRegistration { version, sender });
    version
}

pub fn clear_initial_input_signal_if_current(
    signals: &StartupSignalRegistry,
    run: &RunKey,
    version: u64,
) {
    let mut signals = signals.lock();
    if signals
        .get(run)
        .is_some_and(|registration| registration.version == version)
    {
        signals.remove(run);
    }
}

/// Per-run teardown: only this run's pending first input is cancelled.
pub fn cancel_initial_input_signal_for_run(registry: &TaskRegistry, run: &RunKey) {
    let registration = registry.initial_input_signals.lock().remove(run);
    if let Some(registration) = registration {
        let _ = registration.sender.send(StartupSignal::Cancelled);
    }
}

/// Intent operation (cancel / reset of the whole task): every registered run's pending first
/// input goes, not just the newest one (ADR-025 rule 2).
pub fn cancel_initial_input_signal(registry: &TaskRegistry, task_id: &str) {
    let runs: Vec<RunKey> = registry
        .initial_input_signals
        .lock()
        .keys()
        .filter(|run| run.id() == task_id)
        .cloned()
        .collect();
    for run in runs {
        cancel_initial_input_signal_for_run(registry, &run);
    }
}

/// Hook events are task-scoped (`hooks::events_dir_for`), so the newest registered run is
/// the only defensible addressee; with nothing registered, fall back to the unminted key,
/// which is what the legacy launch path itself would have used.
pub fn notify_initial_input_session_ready(registry: &TaskRegistry, task_id: &str) {
    let run = registry
        .resource_run(task_id)
        .unwrap_or_else(|| RunKey::unminted(task_id));
    let sender = registry
        .initial_input_signals
        .lock()
        .get(&run)
        .map(|registration| registration.sender.clone());
    if let Some(sender) = sender {
        let _ = sender.send(StartupSignal::SessionReady);
    }
}

// ── TaskRegistry:宿主 TaskManager 的 14 张容器表 + 它们的全部操作(一次性 move) ──

/// Outcome of asking "may this exit settle the task?" — see
/// [`TaskRegistry::settle_marks_for_exit`] and ADR-030.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitSettlement {
    /// This was the last live run: the marks are consumed and the caller owns the status event
    /// plus the cleanup side effects.
    Settle {
        cancelled: bool,
        manually_completed: bool,
    },
    /// Another run of the same task is still in flight: consume nothing, emit nothing, delete
    /// nothing.
    WaitingForOtherRuns,
}

/// 全部任务资源容器。字段保持 `pub`:宿主经 `Deref` 直达同名字段,316 处读取点
/// 在 move 后一字未改——这正是"委托壳而不是搬运调用点"的落点。
#[derive(Default)]
pub struct TaskRegistry {
    pub pty_masters: Mutex<HashMap<RunKey, Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>>>,
    pub pty_writers: Mutex<HashMap<RunKey, Arc<Mutex<Box<dyn std::io::Write + Send>>>>>,
    pub child_handles:
        Mutex<HashMap<RunKey, Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>>>,
    /// Addressed to "the next run of this id", not to a run: a resize that arrives before the
    /// process exists has no generation to name, which is why every entry here carries
    /// [`RunKey::UNMINTED`] and `register_pty_session` reads it back under that
    /// key whatever it is about to register.
    pub pending_pty_sizes: Mutex<HashMap<RunKey, (u16, u16)>>,
    pub cancelled_tasks: Mutex<HashSet<String>>,
    pub manually_completed_tasks: Mutex<HashSet<String>>,
    pub codex_sessions: Mutex<HashMap<RunKey, CodexSessionInfo>>,
    pub claude_sessions: Mutex<HashMap<RunKey, ClaudeSessionInfo>>,
    pub dsh_sessions: Mutex<HashMap<RunKey, DshSessionInfo>>,
    pub omp_sessions: Mutex<HashMap<RunKey, OmpSessionInfo>>,
    pub claimed_session_paths: Mutex<HashSet<String>>,
    /// 启动态初始输入的门控信号:trust/hook 等交互完成后再投递 prompt。
    pub initial_input_signals: StartupSignalRegistry,
    /// 这台机器上还活着哪些 WSL 侧运行(shell 与任务共用一张表,ADR-032 的存活语义)。
    pub wsl_active_ids: Mutex<HashSet<RunKey>>,
    /// Which runs of a task exist right now (ADR-020 + ADR-025). A leaf: it takes no other
    /// lock while held, so it may be locked last from any of the maps above.
    pub runs: RunRegistry,
}

impl TaskRegistry {
    /// ADR-025 rule 1: which run a resource operation (write input, resize, attach a session)
    /// addresses — the newest registered one, or nothing when the task has no live run.
    /// Centralised here so no call site can invent its own answer, and because `runs` is a leaf
    /// lock: it is resolved before any PTY map is touched.
    pub fn resource_run(&self, task_id: &str) -> Option<RunKey> {
        self.runs.resource_key(task_id)
    }

    pub fn writer_for(&self, task_id: &str) -> Option<Arc<Mutex<Box<dyn std::io::Write + Send>>>> {
        let run = self.resource_run(task_id)?;
        self.pty_writers.lock().get(&run).cloned()
    }

    pub fn child_for(
        &self,
        task_id: &str,
    ) -> Option<Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>> {
        let run = self.resource_run(task_id)?;
        self.child_handles.lock().get(&run).cloned()
    }

    pub fn master_for(
        &self,
        task_id: &str,
    ) -> Option<Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>> {
        let run = self.resource_run(task_id)?;
        self.pty_masters.lock().get(&run).cloned()
    }

    /// Whether this task or shell has a live process at all — **any** run, not the newest one,
    /// because "is it still alive" is answered by a still-running older run too. ADR-020's note
    /// on `child_handles`: this stays the liveness signal, now read through the run namespace.
    pub fn has_live_run(&self, id: &str) -> bool {
        !self.runs.live_generations(id).is_empty()
    }

    /// The size a run should start at, including one that has not been registered yet: a
    /// resize arriving before the process exists is addressed to the next run of that id.
    pub fn pending_size(&self, id: &str) -> Option<(u16, u16)> {
        self.pending_pty_sizes
            .lock()
            .get(&RunKey::unminted(id))
            .copied()
    }

    pub fn stage_pending_size(&self, id: &str, size: (u16, u16)) {
        self.pending_pty_sizes
            .lock()
            .insert(RunKey::unminted(id), size);
    }

    /// 首条用户输入到达:若该运行还挂着启动门控信箱,投递 `UserInput`。
    /// 登记保留(清登记是 clear_initial_input_signal_if_current 的职责)。
    pub fn send_user_input_signal(&self, run: &RunKey) -> bool {
        let sender = self
            .initial_input_signals
            .lock()
            .get(run)
            .map(|registration| registration.sender.clone());
        match sender {
            Some(sender) => sender.send(StartupSignal::UserInput).is_ok(),
            None => false,
        }
    }

    /// What an exiting run is allowed to do about its task's state (ADR-030).
    ///
    /// The cancel/completion marks are **one task-scoped account**, and only the last run to
    /// exit consumes them — together with the settlement event and the cleanup side effects,
    /// which today the *first* exit performed while another run was still writing to those same
    /// directories.
    pub fn settle_marks_for_exit(&self, task_id: &str) -> ExitSettlement {
        // Called after the exiting run has retired itself, so anything still registered here
        // belongs to another run. Reversing those two steps is the bug this comment exists for:
        // checking before the retire would always find "another run" and nothing would ever
        // settle.
        if !self.runs.live_generations(task_id).is_empty() {
            return ExitSettlement::WaitingForOtherRuns;
        }
        let cancelled = self.cancelled_tasks.lock().remove(task_id);
        let manually_completed = self.manually_completed_tasks.lock().remove(task_id);
        ExitSettlement::Settle {
            cancelled,
            manually_completed,
        }
    }

    /// ADR-031: the one rule for putting a session path back on the market.
    ///
    /// Call this *after* removing the run's own session entry, and note the lock order —
    /// session tables first, `claimed_session_paths` last. It deliberately matches the teardown
    /// side of the lock-order table at the top of this file, which is why the registration
    /// sites in `session_omp.rs` / `session_dsh.rs` had to stop holding the claim across an
    /// insert. Without the reverse lookup, the first run to exit would free a path another run
    /// still reads, and two agents would write one transcript.
    pub fn release_session_path_if_unreferenced(&self, path: &str) -> bool {
        let referenced = self
            .codex_sessions
            .lock()
            .values()
            .any(|info| info.session_path == path)
            || self
                .claude_sessions
                .lock()
                .values()
                .any(|info| info.session_path == path)
            || self
                .dsh_sessions
                .lock()
                .values()
                .any(|info| info.session_path == path)
            || self
                .omp_sessions
                .lock()
                .values()
                .any(|info| info.session_path == path);
        if referenced {
            return false;
        }
        self.claimed_session_paths.lock().remove(&path.to_string())
    }

    /// The same teardown for one run: drop its four possible session entries, then let each
    /// path it held go through the reverse lookup. Returns the paths this run had, whatever
    /// happened to the claims, so callers that report "was there a real transcript" can judge
    /// from the entries they saw rather than re-reading the tables.
    pub fn release_run_sessions(&self, run: &RunKey) -> Vec<String> {
        let mut released = Vec::new();
        if let Some(info) = self.codex_sessions.lock().remove(run) {
            released.push(info.session_path);
        }
        if let Some(info) = self.claude_sessions.lock().remove(run) {
            released.push(info.session_path);
        }
        if let Some(info) = self.dsh_sessions.lock().remove(run) {
            released.push(info.session_path);
        }
        if let Some(info) = self.omp_sessions.lock().remove(run) {
            released.push(info.session_path);
        }
        for path in &released {
            self.release_session_path_if_unreferenced(path);
        }
        released
    }

    pub fn has_codex_session(&self, task_id: &str) -> bool {
        self.codex_sessions
            .lock()
            .keys()
            .any(|run| run.id() == task_id)
    }

    pub fn has_claude_session(&self, task_id: &str) -> bool {
        self.claude_sessions
            .lock()
            .keys()
            .any(|run| run.id() == task_id)
    }

    pub fn has_dsh_session(&self, task_id: &str) -> bool {
        self.dsh_sessions
            .lock()
            .keys()
            .any(|run| run.id() == task_id)
    }

    pub fn has_omp_session(&self, task_id: &str) -> bool {
        self.omp_sessions
            .lock()
            .keys()
            .any(|run| run.id() == task_id)
    }

    /// Every run of this task that holds a session entry — what an intent operation such as a
    /// cancellation must clean up (ADR-025 rule 2), as opposed to the one run a read addresses.
    pub fn session_runs_for_task(&self, task_id: &str) -> Vec<RunKey> {
        // One family at a time: lock-order rule 2 forbids holding two families' tables at once,
        // so this cannot be a chain over four guards. The four tables hold different info types,
        // which rules out a loop as well — hence the macro.
        macro_rules! runs_of {
            ($table:expr) => {{
                $table
                    .lock()
                    .keys()
                    .filter(|run| run.id() == task_id)
                    .cloned()
                    .collect::<Vec<RunKey>>()
            }};
        }
        let mut runs = runs_of!(self.codex_sessions);
        runs.extend(runs_of!(self.claude_sessions));
        runs.extend(runs_of!(self.dsh_sessions));
        runs.extend(runs_of!(self.omp_sessions));
        runs.sort();
        runs.dedup();
        runs
    }

    /// PTY 会话注册的唯一入口(P3-03 切片四D 自宿主 `register_pty_handles` 逐字迁移)。
    /// A staged resize is addressed to "the next run of this id", which is why it is read back
    /// under the unminted key whatever this run's generation turns out to be.
    pub fn register_pty_session(
        &self,
        run: &RunKey,
        master: Box<dyn portable_pty::MasterPty + Send>,
        writer: Box<dyn std::io::Write + Send>,
        child: Box<dyn portable_pty::Child + Send + Sync>,
    ) -> Result<Arc<parking_lot::Mutex<Box<dyn portable_pty::Child + Send + Sync>>>, String> {
        let mut masters = self.pty_masters.lock();
        let mut pending_sizes = self.pending_pty_sizes.lock();
        if let Some((cols, rows)) = pending_sizes.remove(&RunKey::unminted(run.id())) {
            master
                .resize(portable_pty::PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| e.to_string())?;
        }
        masters.insert(run.clone(), Arc::new(parking_lot::Mutex::new(master)));
        drop(pending_sizes);
        drop(masters);
        self.pty_writers
            .lock()
            .insert(run.clone(), Arc::new(parking_lot::Mutex::new(writer)));
        let child_handle = Arc::new(parking_lot::Mutex::new(child));
        self.child_handles
            .lock()
            .insert(run.clone(), Arc::clone(&child_handle));
        // Registered only once all three handles exist: `runs` is what ADR-025's addressing reads,
        // and a run that is half-registered would be addressable but unwritable.
        self.runs.register(run.id(), run.generation());
        Ok(child_handle)
    }

    /// Atomically remove one run from all PTY maps (masters, writers, children) and retire it.
    /// Locks are acquired in a fixed order to prevent deadlocks.
    pub fn remove_pty_handles(&self, run: &RunKey) {
        let mut masters = self.pty_masters.lock();
        let mut pending_sizes = self.pending_pty_sizes.lock();
        let mut writers = self.pty_writers.lock();
        let mut children = self.child_handles.lock();
        masters.remove(run);
        pending_sizes.remove(&RunKey::unminted(run.id()));
        writers.remove(run);
        children.remove(run);
        drop(children);
        drop(writers);
        drop(pending_sizes);
        drop(masters);
        self.runs.retire(run.id(), run.generation());
        cancel_initial_input_signal_for_run(self, run);
    }

    /// Remove one finished run's handles, keeping the safety net ADR-020 leaves in place: the
    /// generation decides *which* entry is cleaned up, and the handle identity only gets to
    /// report "the same generation was registered twice", which is an implementation bug
    /// rather than a race between two legitimate runs.
    pub fn remove_pty_handles_if_current(
        &self,
        run: &RunKey,
        expected: &Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    ) -> bool {
        let mut masters = self.pty_masters.lock();
        let mut pending_sizes = self.pending_pty_sizes.lock();
        let mut writers = self.pty_writers.lock();
        let mut children = self.child_handles.lock();
        let Some(current) = children.get(run) else {
            return false;
        };
        if !Arc::ptr_eq(current, expected) {
            debug_assert!(
                false,
                "run {run:?} holds two different children: a generation was registered twice"
            );
            return false;
        }
        masters.remove(run);
        pending_sizes.remove(&RunKey::unminted(run.id()));
        writers.remove(run);
        children.remove(run);
        drop(children);
        drop(writers);
        drop(pending_sizes);
        drop(masters);
        self.runs.retire(run.id(), run.generation());
        true
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// ADR-017 双开:同任务两代(两次不同 operation key)的句柄必须在表里共存,
    /// 后注册的代不得覆盖前一个的 master/writer/child——那是重键(ADR-020)要防的
    /// "无法 kill 的进程"的形状。本用例在 core 侧钉住,宿主验收
    /// `a_second_operation_starts_a_second_run_without_replacing_the_first` 钉的是
    /// 经执行器走完的同一条路径。
    #[test]
    fn two_runs_of_one_task_hold_distinct_handle_entries() {
        let registry = TaskRegistry::default();
        let old = RunKey::minted("t1", 1);
        let new = RunKey::minted("t1", 2);
        for key in [&old, &new] {
            registry
                .pty_masters
                .lock()
                .insert(key.clone(), Arc::new(Mutex::new(real_master())));
        }
        assert_eq!(
            registry.pty_masters.lock().len(),
            2,
            "双开是两个条目,不是覆盖"
        );
        assert!(
            !registry.runs.is_live("t1", 1),
            "表里有两个条目;runs 的账由 register/retire 记"
        );
        registry.runs.register("t1", 1);
        registry.runs.register("t1", 2);

        // 锁序 4 的方向:remove_pty_handles 一次带走一代,另一代原样保留。
        registry.remove_pty_handles(&old);
        assert!(registry.pty_masters.lock().get(&new).is_some());
        assert!(!registry.runs.is_live("t1", 1));
        assert!(registry.runs.is_live("t1", 2));
    }

    /// ADR-030:任务级标记账,最后一个退出者销账。guard 变体(还有别的代在飞)必须
    /// 什么都不消费——把它改成"总是结算"就是回到 first-exit-cleans 的老 bug。
    #[test]
    fn settle_marks_wait_for_the_last_live_run() {
        let registry = TaskRegistry::default();
        registry.cancelled_tasks.lock().insert("t1".to_string());
        registry.runs.register("t1", 1);
        registry.runs.register("t1", 2);
        assert_eq!(
            registry.settle_marks_for_exit("t1"),
            ExitSettlement::WaitingForOtherRuns,
            "另一代还在飞:标记不被消费"
        );
        registry.runs.retire("t1", 2);
        registry.runs.retire("t1", 1);
        assert_eq!(
            registry.settle_marks_for_exit("t1"),
            ExitSettlement::Settle {
                cancelled: true,
                manually_completed: false
            }
        );
        // 读时消费:第二次结算什么都拿不到。
        assert!(matches!(
            registry.settle_marks_for_exit("t1"),
            ExitSettlement::Settle {
                cancelled: false,
                manually_completed: false
            }
        ));
    }

    /// ADR-031 的核心形状:反查仍引用则不放行,删掉条目后才放行。
    #[test]
    fn a_claimed_path_is_only_released_when_no_session_entry_still_reads_it() {
        let registry = TaskRegistry::default();
        let run = RunKey::minted("t1", 1);
        registry
            .claimed_session_paths
            .lock()
            .insert("/tmp/sess.jsonl".to_string());
        registry.codex_sessions.lock().insert(
            run.clone(),
            CodexSessionInfo {
                session_id: "s1".to_string(),
                session_path: "/tmp/sess.jsonl".to_string(),
            },
        );
        assert!(
            !registry.release_session_path_if_unreferenced("/tmp/sess.jsonl"),
            "反查命中:另一个读者还在,路径不上市"
        );
        // release_run_sessions 自己就会对每条路径做反查释放:条目已删 → 反查落空 → claim 放行。
        registry.release_run_sessions(&run);
        assert!(
            !registry
                .claimed_session_paths
                .lock()
                .contains("/tmp/sess.jsonl"),
            "条目删除后,release_run_sessions 已把无人引用的 claim 一并放行"
        );
    }

    /// 信箱语义:resize 早于 spawn 到达时寄给"下一次运行",注册时取回。
    #[test]
    fn a_pending_size_is_addressed_to_the_next_run_of_the_id() {
        let registry = TaskRegistry::default();
        assert_eq!(registry.pending_size("t1"), None);
        registry.stage_pending_size("t1", (120, 40));
        assert_eq!(registry.pending_size("t1"), Some((120, 40)));
        let staged_generation = registry
            .pending_pty_sizes
            .lock()
            .keys()
            .map(|k| k.generation())
            .next();
        assert_eq!(
            staged_generation,
            Some(RunKey::UNMINTED),
            "寄存键是 unminted:它属于下一次运行,不属于任何现存代"
        );
    }

    /// ADR-032 的版本戳语义(自宿主 pty.rs 随本体迁入):两个 unminted 运行共用一个键,
    /// 戳旧的一方退出时不得删掉戳新的登记——否则被顶替运行的首条 prompt 永远不来。
    #[test]
    fn stale_startup_cleanup_does_not_remove_the_replacement_registration() {
        let registry = TaskRegistry::default();
        let run = RunKey::unminted("task-1");
        let (sender, _receiver) = std::sync::mpsc::channel();
        let version = register_initial_input_signal(&registry, &run, sender);

        // 直接在账本上登记一个"更晚"的条目,模拟同键被顶替后新登记的形状。
        // 版本戳是进程全局单调计数,不能假设绝对值;性质是"每次登记严格递增"。
        let (newer_sender, _newer_receiver) = std::sync::mpsc::channel();
        registry.initial_input_signals.lock().insert(
            run.clone(),
            StartupSignalRegistration {
                version: version + 1,
                sender: newer_sender,
            },
        );

        clear_initial_input_signal_if_current(&registry.initial_input_signals, &run, version);
        assert_eq!(
            registry
                .initial_input_signals
                .lock()
                .get(&run)
                .map(|registration| registration.version),
            Some(version + 1),
            "a stale version must leave the current registration alone"
        );
        clear_initial_input_signal_if_current(&registry.initial_input_signals, &run, version + 1);
        assert!(
            registry.initial_input_signals.lock().get(&run).is_none(),
            "the matching version still clears it"
        );
    }

    /// 版本戳盖不住的情形正是键要cover的:两代真运行各等各的门。
    #[test]
    fn two_minted_runs_of_one_task_hold_their_own_registration() {
        let registry = TaskRegistry::default();
        let (first, _rx1) = std::sync::mpsc::channel();
        let (second, _rx2) = std::sync::mpsc::channel();
        // 版本戳是进程全局单调计数,不能假设绝对值;性质是"每次登记严格递增"。
        // （断言只写严格递增：并发跑 lib tests 时,同二进制里别的登记测试会推高
        // 全局计数,把「差恰为 1」变成执行顺序依赖——2026-09-30 门禁实测抓到一次
        // v4=1/v5=3 的假红,补记二百四十六。）
        let v4 = register_initial_input_signal(&registry, &RunKey::minted("task-1", 4), first);
        let v5 = register_initial_input_signal(&registry, &RunKey::minted("task-1", 5), second);
        assert!(v5 > v4, "严格递增：{v4} -> {v5}");
        clear_initial_input_signal_if_current(
            &registry.initial_input_signals,
            &RunKey::minted("task-1", 4),
            v4,
        );
        assert!(registry
            .initial_input_signals
            .lock()
            .get(&RunKey::minted("task-1", 4))
            .is_none());
        assert!(
            registry
                .initial_input_signals
                .lock()
                .get(&RunKey::minted("task-1", 5))
                .is_some(),
            "the surviving run keeps its own gate"
        );
    }

    // Fake agent 不进这里——MasterPty 是 trait object,手写假货要追 vendored trait
    // 的签名漂移(vendored 版的方法返回 anyhow::Error,core 不引 anyhow);直接开一对
    // 真 pty,不起任何子进程。child_handles 的双开共存由宿主验收
    // a_second_operation_starts_a_second_run_without_replacing_the_first 走真内核钉住。
    fn real_master() -> Box<dyn portable_pty::MasterPty + Send> {
        portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize::default())
            .expect("open pty pair")
            .master
    }
}

#[cfg(test)]
mod tests {
    use super::{RunKey, RunRegistry};

    #[test]
    fn newest_means_registered_not_counted() {
        // The distinction ADR-025 calls out: after the only run retires, a task-id-only
        // resource request must find *nothing*, not "the next number".
        let registry = RunRegistry::new();
        registry.register("t1", 1);
        assert_eq!(registry.newest("t1"), Some(1));
        registry.register("t1", 3);
        assert_eq!(
            registry.newest("t1"),
            Some(3),
            "a gap in the numbers is fine; the highest registered run wins"
        );
        assert!(registry.retire("t1", 3));
        assert_eq!(
            registry.newest("t1"),
            Some(1),
            "and addressing falls back to the older run"
        );
        assert!(registry.retire("t1", 1));
        assert_eq!(
            registry.newest("t1"),
            None,
            "with nothing registered there is no generation to address"
        );
    }

    #[test]
    fn an_intent_sees_every_live_generation_and_a_retire_is_idempotent() {
        let registry = RunRegistry::new();
        registry.register("t1", 1);
        registry.register("t1", 2);
        registry.register("t1", 2);
        assert_eq!(
            registry.live_generations("t1"),
            vec![1, 2],
            "a duplicate registration is the same run, not two"
        );
        assert!(registry.has_other_live("t1", 1));
        assert!(registry.retire("t1", 1));
        assert!(registry.retire("t1", 2));
        assert!(
            !registry.retire("t1", 2),
            "a second exit report for the same run retires nothing"
        );
        assert_eq!(registry.live_generations("t1"), Vec::<u64>::new());
        assert!(!registry.has_other_live("t1", 2));
    }

    #[test]
    fn tasks_do_not_share_namespace_and_deletion_clears_the_whole_task() {
        let registry = RunRegistry::new();
        registry.register("t1", 1);
        registry.register("t2", 1);
        assert_eq!(registry.live_task_count(), 2);
        assert_eq!(registry.retire_task("t1"), vec![1]);
        assert_eq!(registry.newest("t1"), None);
        assert_eq!(registry.newest("t2"), Some(1), "its sibling is untouched");
        assert_eq!(registry.retire_task("t1"), Vec::<u64>::new());
        assert_eq!(registry.live_task_count(), 1);
    }

    #[test]
    fn an_unregistered_generation_is_not_live() {
        // Guards against "is_live" degrading into "the task has any run", which would let a
        // late event from a retired generation look legitimate.
        let registry = RunRegistry::new();
        registry.register("t1", 4);
        assert!(registry.is_live("t1", 4));
        assert!(!registry.is_live("t1", 3));
        assert!(!registry.is_live("t9", 4));
    }

    #[test]
    fn an_unminted_run_and_a_minted_one_are_distinguishable_and_ordered() {
        // The migration case: a task started before the handover (unminted) and one started
        // through core's ledger (minted) coexist for the same id. A resource operation must
        // address the minted one, and an intent must reach both.
        let registry = RunRegistry::new();
        registry.register("t1", RunKey::UNMINTED);
        registry.register("t1", 4);
        assert_eq!(
            registry.resource_key("t1"),
            Some(RunKey::minted("t1", 4)),
            "generation 0 never wins an addressing contest"
        );
        assert_eq!(
            registry.intent_keys("t1"),
            vec![RunKey::unminted("t1"), RunKey::minted("t1", 4)],
            "both are live processes, so 'stop that task' reaches both"
        );
        assert!(registry.retire("t1", 4));
        assert_eq!(registry.resource_key("t1"), Some(RunKey::unminted("t1")));
        assert!(!registry.resource_key("t1").unwrap().is_minted());
    }

    #[test]
    fn a_run_that_was_never_registered_is_not_addressable() {
        let registry = RunRegistry::new();
        assert_eq!(registry.resource_key("t1"), None);
        assert_eq!(registry.intent_keys("t1"), Vec::<RunKey>::new());
        registry.register("t1", RunKey::UNMINTED);
        assert_eq!(registry.resource_key("t1"), Some(RunKey::unminted("t1")));
        assert!(registry.retire("t1", RunKey::UNMINTED));
        assert_eq!(registry.resource_key("t1"), None);
    }

    #[test]
    fn a_key_names_its_run_and_carries_the_id_lookups_used_before() {
        let minted = RunKey::minted("t7", 9);
        let unminted = RunKey::unminted("shell:project:1:1700000000000");
        assert_eq!(minted.id(), "t7");
        assert_eq!(minted.generation(), 9);
        assert!(minted.is_minted());
        assert_eq!(
            unminted.id(),
            "shell:project:1:1700000000000",
            "a shell's id is already its run identity"
        );
        assert!(!unminted.is_minted());
        assert_ne!(minted, RunKey::minted("t7", 10), "one task, two runs");
    }

    #[test]
    fn concurrent_registration_and_retirement_do_not_lose_a_run() {
        // Two windows starting two runs at once is the exact situation ADR-017 permits.
        let registry = std::sync::Arc::new(RunRegistry::new());
        let threads: Vec<_> = (0..16u64)
            .map(|index| {
                let registry = std::sync::Arc::clone(&registry);
                std::thread::spawn(move || {
                    registry.register("t1", index);
                    registry.register("t1", index);
                    if index % 2 == 0 {
                        registry.retire("t1", index);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("registry thread");
        }
        let live = registry.live_generations("t1");
        let expected: Vec<u64> = (0..16).filter(|index| index % 2 == 1).collect();
        assert_eq!(
            live, expected,
            "every concurrently registered run is accounted for"
        );
    }
}
