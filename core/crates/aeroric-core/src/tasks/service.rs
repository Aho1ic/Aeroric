//! `TaskService` — the one place a task's state and a process's life are decided together.
//!
//! Three ordering rules are the whole reason this is a service rather than a set of
//! helpers, and all three were enforced by whichever handler happened to run last:
//!
//! 1. **Persist, then spawn.** A task that is only in memory cannot be resumed after a
//!    crash, and a spawn that precedes its save can outlive the record that names it. A
//!    failed save therefore returns without ever reaching the executor.
//! 2. **Claim, then persist, then spawn.** [`OperationLedger`](super::operations::OperationLedger)
//!    keys on `(task, kind, generation)`, so a retry of the same attempt is refused before
//!    the disk is touched and one attempt spawns one process — across retries, windows and
//!    processes that share this directory.
//! 3. **Settle a claim only once the outcome is known.** A claim left open across a crash
//!    is what [`TaskService::reconcile`] turns into `interrupted`, which is the difference
//!    between "this task was cut off" and "let's start a second one".
//!
//! Notifications, which task is selected, terminal buffers and toasts are *not* here: the
//! acceptance test for this module is that start, resume and cancel all work with no
//! listener attached.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use serde_json::Value;

use crate::clock::Clock;
use crate::context::CoreContext;
use crate::projects::mutations::is_addressable;
use crate::storage::atomic::StorageError;
use crate::storage::compat::Record;
use crate::storage::fingerprint::Cover;
use crate::storage::projects;
use crate::storage::runtime;
use crate::storage::tasks as task_storage;
use crate::tasks::events::TaskNotification;
use crate::tasks::executor::{
    settled_by_exit, ExecutorFailure, HostWindowFacts, LiveRuns, TaskEvent, TaskExecutor,
};
use crate::tasks::launch_plan::{self, Completion};
use crate::tasks::operations::{Claim, Claimed, OperationKind, OperationLedger};
use crate::tasks::reducer;
use crate::tasks::session_owner::{self, AgentFamilies, ConfigSwitchStrategy, SessionOwner};

/// Files carried into a run. A remote project cannot take them: the client's submit path
/// refuses with `remoteProjectNoAttachments` (`src/state/app/taskLifecycle.ts:292-295`),
/// and the reason is a channel limit rather than a preference.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attachments {
    pub images: Vec<String>,
    pub texts: Vec<String>,
}

/// What a caller asked to start. `immediate` is the client's "run now" switch: without it
/// the task is a `todo`, which is a plan rather than a request for a process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTask {
    pub id: String,
    pub project_id: String,
    pub prompt: String,
    pub agent: String,
    pub permission_mode: String,
    pub immediate: bool,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub speed: Option<String>,
    pub dsh_agent_preset: Option<String>,
    /// `agent-connections.json` config the task was created with (Q27: pinned at
    /// creation). `None` for tasks that use the legacy agent settings.
    pub agent_config_id: Option<String>,
    pub images: Vec<String>,
    pub texts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub records: Vec<Record>,
    /// The revision this change bought. `None` when nothing was written, which is also the
    /// answer to "did the revision advance?".
    pub revision: Option<String>,
    pub changed: bool,
    /// The cover the caller's *next* write in this session must present.
    pub fingerprint: Option<Value>,
    /// Which run number the caller owes a spawn to, and only for a change that does *not* reach
    /// the executor itself: a config switch decides the next run but leaves starting it to the
    /// host, so the number has to come back here. A start or resume reports `None` on purpose —
    /// the host already received that generation inside `LaunchRequest`/`ResumeRequest`, and a
    /// second copy on the return value would be a second place for it to disagree.
    pub generation: Option<u64>,
}

/// What the user asked a task to become.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigSwitch {
    pub agent: String,
    pub permission_mode: String,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub speed: Option<String>,
}

/// What only the host can know before a switch is decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SwitchEvidence {
    /// `session_supports_native_resume` on the transcript.
    pub native_resume_supported: bool,
    pub session_id: Option<String>,
    /// The session id and path after `adopt_session_for_agent` moved the transcript.
    pub adopted_session_id: Option<String>,
    pub adopted_session_path: Option<String>,
    /// `formatSessionHandoff`, when the continuation is a handoff rather than a resume.
    pub handoff_prompt: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum TaskError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("task {task_id} is not in project {project_id}")]
    NotFound { project_id: String, task_id: String },
    #[error("project {0} has no readable location, so nothing can be run inside it")]
    Unaddressable(String),
    #[error("the task could not be saved, so no process was started: {0}")]
    PersistFailed(String),
    #[error("task id {0} is already in this project; two rows cannot share one id")]
    DuplicateId(String),
    #[error(
        "this attempt to {kind} task {task_id} is already in flight (claimed at {claimed_at_ms}ms)"
    )]
    AlreadyClaimed {
        task_id: String,
        kind: &'static str,
        claimed_at_ms: u64,
    },
    #[error("task {0} has no session to continue from; refusing to start a blank one")]
    NoSession(String),
    #[error("the process could not be started ({0}); the task is marked failed")]
    Launch(String),
    #[error("task {task_id} was cut off by the previous session and has not been acknowledged; resume is held until it is")]
    AwaitingAcknowledgement { task_id: String },
    #[error("task {0} has no worktree to resume into any more; continuing would replay a transcript whose tree is gone")]
    WorktreeDiscarded(String),
    #[error("task {0} has neither a transcript, terminal history, nor a prompt to continue from; switching would open an amnesiac session")]
    NoContinuationContext(String),
    #[error("a {kind} project cannot carry {what}; the run was not started")]
    UnsupportedOnRemote {
        kind: &'static str,
        what: &'static str,
    },
    #[error("an event for run {seen} of task {task_id} arrived before this ledger had reached that run (at {current})")]
    FutureGeneration {
        task_id: String,
        seen: u64,
        current: u64,
    },
}

/// One event accepted or refused, so a host can log *why* a status did not move without
/// guessing from the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventOutcome {
    Applied {
        status: TaskStatus,
        changed: bool,
        /// The reason core attached to this status, from the event itself or synthesized for a
        /// nonzero exit. It travels in the outcome because the announcement path is not allowed
        /// to re-derive it: the shipping client's `failed` emits carry `failure_reason`, and a
        /// sink that had to invent one would be a second place the text diverges (ADR-023).
        failure_reason: Option<String>,
    },
    /// Dropped: an older generation, or a transition the machine ignores.
    Dropped(&'static str),
}

impl EventOutcome {
    /// ADR-035's announcement rule, kept here so no adapter has to decide it: **an admitted
    /// observation announces, whether or not the row moved** — 25 of the 28 legacy sites emit
    /// unconditionally today, and folding "nothing changed" into "say nothing" would be a
    /// user-visible behaviour change smuggled into a handover (ADR-033 §4). Only `Dropped` is
    /// silent, and it is silent for a reason a host can log.
    pub fn notification_for(&self, task_id: &str) -> Option<TaskNotification> {
        match self {
            EventOutcome::Applied {
                status,
                failure_reason,
                ..
            } => Some(match failure_reason {
                Some(reason) => TaskNotification::status_with_reason(task_id, *status, reason),
                None => TaskNotification::status(task_id, *status),
            }),
            EventOutcome::Dropped(_) => None,
        }
    }
}

/// Whether an accepted observation also gets written. ADR-035 splits this from the decision:
/// reads need no write authority (only `save_covering` asks for it), so a host that is not the
/// writer can still let core decide what a task's status is — and therefore what gets emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Persist {
    Commit,
    DecideOnly,
    /// ADR-021 + §12: core decides, the *host* stores the bytes through its
    /// [`crate::context::HostTaskSaver`]. Not a guess core makes about platforms - the
    /// adapter picks this arm when its gateway says `HandoverDeferred` - and never a
    /// silent fallback: with no saver installed the write is an error.
    ViaHost,
}

/// ADR-022 决策 1 里宿主队列交出来的东西：一条意图，不是一份快照。
///
/// 为什么要有具名类型而不是让宿主直接调 `rename`/`archive`：宿主最终只会有**一个**入口
/// （收到 op 名与载荷，转成这个枚举），而 ADR-021 让"落不落盘"按平台分叉——两条路径共用同一
/// 个意图类型，才不会变成"macOS 走 op、别的平台走快照"那种 ADR-022 明确否掉的混合形态。
/// 重试身份（operation key）不在这里：那是调用方的事，账本按 ADR-017 记。
///
/// 线上形状是 `{"op":"rename","taskId":"t1","name":"…"}`：`op` 做判别式、字段走 camelCase，
/// 与 `AgentId` 等既有跨端类型同一套约定，免得宿主再写一个转换器。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TaskIntent {
    Rename {
        task_id: String,
        name: String,
        /// ADR-037 决策 4：LLM 生成的名字带前置，用户手输的不带。缺省 = 无条件改名，
        /// 所以不发这个字段的旧调用方行为不变。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<reducer::RenameExpect>,
    },
    ToggleStar {
        task_id: String,
    },
    Archive {
        task_ids: Vec<String>,
    },
    Unarchive {
        task_ids: Vec<String>,
    },
    /// #21/#9：会话挂接。`family` 是宿主从终端事件里拿到的**事实**，"清四族只设一族"的规则在
    /// core（ADR-037 决策 1）。
    AttachSession {
        task_id: String,
        family: ProtocolFamily,
        session_agent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_path: Option<String>,
    },
    SetWorktree {
        task_id: String,
        path: String,
        branch: String,
        base_branch: String,
    },
    DiscardWorktree {
        task_id: String,
    },
    /// #16：完成后异步算出来的两个数字。幂等由调用方的 operation key 负责，不由这条意图判等
    /// ——两个数字算出同样的值，与"这一对已经写过"是两回事。
    RecordDiffStats {
        task_id: String,
        additions: u64,
        deletions: u64,
    },
    /// #3（`taskLifecycle.ts:338`）：建行。行的形状（三族钉法、空 prompt 用 agent 标签当名字、
    /// `immediate ⇒ pending`）是 core 的既有具名 `create` 的，意图只是把它接进同一条 persist
    /// 通道；id 由宿主生成（ADR-037 决策 2：建与删都是意图，不是"推迟到 spawn 成功"）。
    Create {
        id: String,
        prompt: String,
        agent: String,
        permission_mode: String,
        immediate: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected_model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speed: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dsh_agent_preset: Option<String>,
    },
    /// #2/#4/#6：删行。"跳过收藏、拒绝在飞、删空 ⇒ 删文件"三条规则都在这条意图里由 core 执行
    /// （ADR-037 后果 1），#4/#6 的补偿删走的也是这一条——被删的行是真落过盘的。
    Delete {
        task_ids: Vec<String>,
    },
    /// #11（`useAppStartup.ts:67`）：启动修复。宿主交的是**事实**（哪些 id 还挂着活的子进程），
    /// "有活孩子 ⇒ detached、没活孩子 ⇒ interrupted、`attentionRequestedAt ?? now`、
    /// dsh 的 `fast → standard`"这四条规则全在 core。逐项目投递（JS 的扇出循环在宿主）。
    NormalizeAfterBoot {
        live_task_ids: Vec<String>,
    },
    /// #12（`taskSwitch.ts:409`）：配置切换的那**一次**写。ADR-037 决策 3——复合写是一条原子
    /// 意图，拆开投递会留下"会话已挂、状态未改"的中间态。三个会话快照里宿主只交得出
    /// previous/resolved 两份，第三份"当前"由 core 从权威快照读——三路合并的规则在 core，
    /// 陈旧的那份不再是任何人的落盘依据。
    SwitchConfig {
        task_id: String,
        agent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected_model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speed: Option<String>,
        permission_mode: String,
        previous_session: SessionSnapshot,
        resolved_session: SessionSnapshot,
    },
    /// #9（`taskLifecycle.ts:644`）：续跑前的那次写——11 个会话字段 + `agent` + `status` 同轮
    /// 落定。宿主交的是解析结果这份**事实**；"清四族只设一族"归 `attach_session` 共用的那条
    /// 规则（`session_owner::apply_resolved_session`）。启动本身不在这条意图里。
    ResumeWithSession {
        task_id: String,
        agent: String,
        family: ProtocolFamily,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_path: Option<String>,
    },
    /// #20（`App.tsx:783`）：todo 编辑三键（Q72 拍板的第 14 条，元数据类）。
    /// `status === "todo"` 的前置在 core——权威状态在 core，守卫不许留在宿主。
    EditTodo {
        task_id: String,
        prompt: String,
        agent: String,
        permission_mode: String,
    },
}

/// 一份行的会话字段族（11 个键）的可序列化形状，`SwitchConfig` 的三路合并拿它当输入。
/// 键缺席＝`undefined`，与 JS 的 `Object.is` 判等一致——"没带这个键"与"值是空串"是两回事，
/// 前者按缺席合并、后者按改值合并。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsh_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsh_session_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_session_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_codex_like: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_family: Option<String>,
}

impl SessionSnapshot {
    /// 十一键按磁盘名对齐的两份快照；`taskSwitch.ts:34-69` 逐键 `Object.is` 的那张表，
    /// 搬到 core 侧只多不少。
    fn zip_entries(&self, other: &Self) -> Vec<(&'static str, Option<Value>, Option<Value>)> {
        let text = |value: &Option<String>| value.as_ref().map(|text| Value::String(text.clone()));
        let boolean = |value: &Option<bool>| value.map(Value::Bool);
        let entries = vec![
            (
                "claudeSessionId",
                text(&self.claude_session_id),
                text(&other.claude_session_id),
            ),
            (
                "claudeSessionPath",
                text(&self.claude_session_path),
                text(&other.claude_session_path),
            ),
            (
                "codexSessionId",
                text(&self.codex_session_id),
                text(&other.codex_session_id),
            ),
            (
                "codexSessionPath",
                text(&self.codex_session_path),
                text(&other.codex_session_path),
            ),
            (
                "dshSessionId",
                text(&self.dsh_session_id),
                text(&other.dsh_session_id),
            ),
            (
                "dshSessionPath",
                text(&self.dsh_session_path),
                text(&other.dsh_session_path),
            ),
            (
                "ompSessionId",
                text(&self.omp_session_id),
                text(&other.omp_session_id),
            ),
            (
                "ompSessionPath",
                text(&self.omp_session_path),
                text(&other.omp_session_path),
            ),
            (
                "sessionAgent",
                text(&self.session_agent),
                text(&other.session_agent),
            ),
            (
                "sessionCodexLike",
                boolean(&self.session_codex_like),
                boolean(&other.session_codex_like),
            ),
            (
                "sessionFamily",
                text(&self.session_family),
                text(&other.session_family),
            ),
        ];
        entries
    }
}

/// ADR-037 词汇表的分类（ADR-026）：**元数据类**冲突可自动重放一次，**状态类**永不重放。
/// 这张表必须显式存在且可数——否则"按 op 分类的冲突处置"会退化成逐条 `if`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskIntentKind {
    Metadata,
    State,
}

/// `op` 名与 `TaskIntent` 的 serde 判别式同一套 camelCase 拼法；条目数与 10/4 的分配
/// 由单测钉死（`task_lifecycle.rs` 的词汇表用例）。14 = 13（ADR-037）+ `EditTodo`（Q72 修正案）。
pub const TASK_INTENT_VOCABULARY: [(&str, TaskIntentKind); 14] = [
    ("rename", TaskIntentKind::Metadata),
    ("toggleStar", TaskIntentKind::Metadata),
    ("archive", TaskIntentKind::Metadata),
    ("unarchive", TaskIntentKind::Metadata),
    ("attachSession", TaskIntentKind::Metadata),
    ("setWorktree", TaskIntentKind::Metadata),
    ("discardWorktree", TaskIntentKind::Metadata),
    ("recordDiffStats", TaskIntentKind::Metadata),
    ("normalizeAfterBoot", TaskIntentKind::Metadata),
    ("editTodo", TaskIntentKind::Metadata),
    ("create", TaskIntentKind::State),
    ("delete", TaskIntentKind::State),
    ("switchConfig", TaskIntentKind::State),
    ("resumeWithSession", TaskIntentKind::State),
];

impl TaskIntent {
    /// 判别式的 `op` 名——与线上 JSON 的 `"op"` 字段同一个拼法，词汇表因此可以对着它数。
    pub fn op_name(&self) -> &'static str {
        match self {
            TaskIntent::Rename { .. } => "rename",
            TaskIntent::ToggleStar { .. } => "toggleStar",
            TaskIntent::Archive { .. } => "archive",
            TaskIntent::Unarchive { .. } => "unarchive",
            TaskIntent::AttachSession { .. } => "attachSession",
            TaskIntent::SetWorktree { .. } => "setWorktree",
            TaskIntent::DiscardWorktree { .. } => "discardWorktree",
            TaskIntent::RecordDiffStats { .. } => "recordDiffStats",
            TaskIntent::Create { .. } => "create",
            TaskIntent::Delete { .. } => "delete",
            TaskIntent::NormalizeAfterBoot { .. } => "normalizeAfterBoot",
            TaskIntent::SwitchConfig { .. } => "switchConfig",
            TaskIntent::ResumeWithSession { .. } => "resumeWithSession",
            TaskIntent::EditTodo { .. } => "editTodo",
        }
    }

    pub fn kind(&self) -> TaskIntentKind {
        TASK_INTENT_VOCABULARY
            .iter()
            .find(|(name, _)| *name == self.op_name())
            .expect("every op name is in the vocabulary")
            .1
    }
}

pub struct TaskService {
    /// Shared, because the host's adapter owns the same context: the gateway is what took the
    /// writer lease, and a service that carried its own copy would be a second answer to
    /// "may this process write" — the thing ADR-021/027 exist to keep singular.
    context: Arc<CoreContext>,
    families: Arc<dyn AgentFamilies>,
    executor: Arc<dyn TaskExecutor>,
    /// ADR-036: which runs are still alive is asked, not stored. Core's own claims cover the
    /// runs it started; the shipping client starts most runs through the legacy commands, and
    /// those only exist here.
    live_runs: Arc<dyn LiveRuns>,
    /// ADR-035's router: task id → the project whose `tasks.json` holds the row. See
    /// [`TaskService::project_of`].
    task_projects: Mutex<HashMap<String, String>>,
    ledger: Mutex<OperationLedger>,
    /// 变更串行闸（load→commit 全程持锁）。`commit` 的覆盖租约有个前提：
    /// "every mutation loads first, so 'the last thing I saw' is by construction
    /// what is on disk"——这条只在变更的读改写本身不被并发撕开时成立。桌面端
    /// 由 React 的 mutation 串行队列（G05 第三处）单线程保序；core 里 settle
    /// 线程与 RPC 线程并发跑同一条链，save→bump 与后随读者的 check_cover 撞窗
    /// 时，轻则误拒（smoke-headless 首跑绿二跑红的长任务创建），重则陈旧快照
    /// 盖回 settle 的状态更新。core 侧自己串行化，守卫继续专防外来写者。
    mutations: Mutex<()>,
    /// Serializes durable submission receipt → create/start, separately from
    /// task mutations so the existing mutation/ledger lock order stays intact.
    submissions: Mutex<()>,
}

impl TaskService {
    pub fn new(
        context: Arc<CoreContext>,
        families: Arc<dyn AgentFamilies>,
        executor: Arc<dyn TaskExecutor>,
        live_runs: Arc<dyn LiveRuns>,
    ) -> Result<Self, StorageError> {
        let ledger = match OperationLedger::load_state(&context)? {
            Some(state) => OperationLedger::new(state),
            None => OperationLedger::empty(),
        };
        // The host's runtime observer is the *caller* of `observe`: an exit, a hook or a
        // terminal-ready edge arrives through it and lands here, so exactly one place
        // writes a task's status and it is never reached from two directions.
        Ok(TaskService {
            context,
            families,
            executor,
            live_runs,
            task_projects: Mutex::new(HashMap::new()),
            ledger: Mutex::new(ledger),
            mutations: Mutex::new(()),
            submissions: Mutex::new(()),
        })
    }

    /// 变更串行闸的唯一入口：包裹「load→决定→commit」整段的变更流。
    /// 六个包裹点（`run_intent`/`create`/`begin`/`complete`/`cancel`/
    /// `apply_observation`）互不嵌套，锁只在最外层持有一次。
    fn serial<T>(&self, f: impl FnOnce() -> Result<T, TaskError>) -> Result<T, TaskError> {
        let _guard = self
            .mutations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f()
    }

    pub fn context(&self) -> &CoreContext {
        &self.context
    }

    /// Native composer opt-in. Validation failures are immutable receipts;
    /// indeterminate execution failures never become a second launch.
    pub fn submit(
        &self,
        request: &aeroric_protocol::submission::SubmissionRequest,
    ) -> Result<aeroric_protocol::submission::SubmissionReceipt, super::submissions::SubmissionError> {
        // Lock order: settings → submissions → task mutations → operation ledger.
        // The execution resolver reads the same settings snapshot without a write.
        let _settings = crate::storage::settings::settings_lock().lock();
        crate::storage::compat::require_writer(&self.context)?;
        let (project, reasons) = match crate::settings::session_catalog::for_project(
            &self.context,
            &request.project_id,
        ) {
            Ok((project, catalog)) => (
                Some(project),
                crate::settings::session_catalog::validate(&catalog, request),
            ),
            Err(reason) => (None, vec![reason]),
        };
        super::submissions::submit(&self.context, &self.submissions, request, reasons, || {
            let project = project
                .as_ref()
                .ok_or_else(|| TaskError::Unaddressable(request.project_id.clone()))?;
            self.create(
                project,
                NewTask {
                    id: request.submission_id.clone(),
                    project_id: request.project_id.clone(),
                    prompt: request.prompt.clone(),
                    agent: request.agent.clone(),
                    permission_mode: request.permission_mode.clone(),
                    immediate: true,
                    selected_model: request.selected_model.clone(),
                    reasoning_effort: request.reasoning_effort.clone(),
                    speed: None,
                    dsh_agent_preset: None,
                    // Q27: the task pins the configuration it was submitted with.
                    // Only an `agent-config:` catalog entry names one; legacy and
                    // built-in entries keep their own launch path.
                    agent_config_id: crate::agent_connections::launch::config_id_from_catalog(
                        &request.configuration_id,
                    )
                    .map(str::to_string),
                    images: vec![],
                    texts: vec![],
                },
            )?;
            self.start(
                project,
                &request.submission_id,
                &format!("submission-{}", request.submission_id),
                true,
                None,
                false,
                Attachments::default(),
                HostWindowFacts::default(),
            )?;
            Ok(())
        })
    }

    pub fn submission(
        &self,
        id: &str,
        fingerprint: &str,
    ) -> Result<aeroric_protocol::submission::SubmissionLookup, super::submissions::SubmissionError> {
        super::submissions::lookup(&self.context, &self.submissions, id, fingerprint)
    }

    pub fn clock(&self) -> &dyn Clock {
        self.context.clock.as_ref()
    }

    fn ledger<T>(&self, mutate: impl FnOnce(&mut OperationLedger) -> T) -> T {
        let mut guard = self
            .ledger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mutate(&mut guard)
    }

    pub fn generation(&self, task_id: &str) -> u64 {
        self.ledger(|ledger| ledger.peek_generation(task_id))
    }

    pub fn open_claims(&self, task_id: &str) -> Vec<Claim> {
        self.ledger(|ledger| ledger.open_for(task_id).into_iter().cloned().collect())
    }

    /// ADR-035's router: which project's `tasks.json` holds this task. The host's 28 report
    /// sites only carry a `task_id` (`emit_task_status(app, task_id, status)`, `pty.rs:1970`,
    /// `dsh_webui.rs:1473`, `event_watcher.rs:319`), while every core mutation is addressed by
    /// project — so an observation cannot reach the reducer without this question being
    /// answerable somewhere.
    ///
    /// The answer comes from disk, never from a guess: `Ok(None)` means "this process has no
    /// row for that id", and callers must say so out loud rather than pick a project. Writing
    /// one task into two projects' files is the divergence ADR-027 refuses to create.
    ///
    /// The scan is O(projects) file reads, which is why the result is remembered. Remembering
    /// is safe here for one specific reason: `projectId` is written when the task is created
    /// and never moved afterwards. `delete` is therefore the only operation that can make an
    /// entry wrong, and it forgets what it removes.
    pub fn project_of(&self, task_id: &str) -> Result<Option<String>, TaskError> {
        let remembered = self
            .task_projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(task_id)
            .cloned();
        if let Some(project_id) = remembered {
            return Ok(Some(project_id));
        }
        for project in projects::load_projects(&self.context)? {
            let Some(project_id) = project.id() else {
                continue;
            };
            let rows = task_storage::load_tasks(&self.context, project_id)?;
            if Self::find(&rows, task_id).is_some() {
                let found = project_id.to_string();
                self.remember_project(task_id, &found);
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    fn remember_project(&self, task_id: &str, project_id: &str) {
        self.task_projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(task_id.to_string(), project_id.to_string());
    }

    fn forget_projects(&self, task_ids: &[String]) {
        let mut guard = self
            .task_projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for id in task_ids {
            guard.remove(id);
        }
    }

    /// A snapshot plus the bytes it came from. Every mutation in this service starts here,
    /// because a write needs the cover the read established.
    pub fn load(&self, project_id: &str) -> Result<task_storage::StampedTasks, TaskError> {
        Ok(task_storage::load_stamped(&self.context, project_id)?)
    }

    /// The only write path. Persist the new list, publish its cover with the revision bump,
    /// then mirror the ledger — in that order, so a crash between the data and the metadata
    /// leaves the ledger claiming less than the disk holds, never more.
    ///
    /// The cover is always [`Cover::LastCoreWrite`], and that choice is the point: a write
    /// may only replace the bytes this core last published. Re-baselining on a fresh read
    /// would make the check unsatisfiable — every mutation loads first, so "the last thing
    /// I saw" is by construction what is on disk — and the guard would exist only in name.
    /// With the durable table as the baseline, a save the shipping client made in between
    /// is a refusal instead of an overwrite, and the caller reloads.
    fn commit(&self, project_id: &str, records: &[Record]) -> Result<Applied, TaskError> {
        let records = self.stamp_generation_copies(records);
        let (fingerprint, key) =
            task_storage::save_covering(&self.context, project_id, &records, Cover::LastCoreWrite)?;
        let bumped =
            runtime::bump_covering(&self.context, &[(key.clone(), Some(fingerprint.clone()))])?;
        self.ledger(|ledger| ledger.persist(&self.context))?;
        Ok(Applied {
            records,
            revision: Some(bumped.metadata.revision),
            changed: true,
            generation: None,
            fingerprint: Some(
                serde_json::to_value(&fingerprint).expect("a fingerprint is a value"),
            ),
        })
    }

    fn unchanged(
        &self,
        records: Vec<Record>,
        fingerprint: Option<crate::storage::fingerprint::Fingerprint>,
    ) -> Applied {
        Applied {
            records,
            revision: None,
            changed: false,
            generation: None,
            fingerprint: fingerprint
                .map(|value| serde_json::to_value(value).expect("a fingerprint is a value")),
        }
    }

    /// Mirror each row's generation counter into the record itself (ADR-016).
    ///
    /// The ledger stays the only authority; this copy exists so a project that is exported,
    /// synced or opened by a core that has never seen it still shows which run it was on.
    /// The copy is rewritten to the ledger's number on every write, including down to zero:
    /// a value that survives an import while the ledger did not would look authoritative and
    /// not be, which is worse than the history it preserves.
    fn stamp_generation_copies(&self, records: &[Record]) -> Vec<Record> {
        let mut next = records.to_vec();
        for row in &mut next {
            let Some(id) = row.id() else { continue };
            let known = self.generation(id);
            if row.get("generation") != Some(&Value::String(known.to_string())) {
                row.0
                    .insert("generation".to_string(), Value::String(known.to_string()));
            }
        }
        next
    }

    fn find<'a>(records: &'a [Record], task_id: &str) -> Option<&'a Record> {
        records.iter().find(|record| record.id() == Some(task_id))
    }

    /// The `todo`/`pending` choice plus the record shape `handleSubmitTask` built.
    pub fn create(&self, project: &Record, spec: NewTask) -> Result<(Record, Applied), TaskError> {
        let project_id = project
            .id()
            .ok_or(StorageError::InvalidId(crate::paths::PathError::EmptyId {
                label: "project",
            }))?
            .to_string();
        if !is_addressable(project) {
            return Err(TaskError::Unaddressable(project_id));
        }
        self.serial(|| self.insert_new_task(&project_id, &spec, Persist::Commit))
    }

    /// 建行行的完整形状——具名 `create` 与 `Create` 意图共用这**一个**构造器。
    /// 分成两份就是"宿主一套、core 一套"的开始，这里不留那个口子。
    fn insert_new_task(
        &self,
        project_id: &str,
        spec: &NewTask,
        persist: Persist,
    ) -> Result<(Record, Applied), TaskError> {
        let snapshot = self.load(project_id)?;
        if Self::find(&snapshot.records, &spec.id).is_some() {
            return Err(TaskError::DuplicateId(spec.id.clone()));
        }
        let now = self.clock().now_ms();
        let (record, records) = self.build_new_records(project_id, spec, &snapshot.records, now);
        let applied = self.finish_intent(project_id, records, persist)?;
        Ok((record, applied))
    }

    /// 行构造的确定性半边：给定 (spec, 当前快照, 时钟) 就给定同一批 records。
    /// 从 `insert_new_task` 抽出来只为让上面的重试循环能整体重做一遍。
    fn build_new_records(
        &self,
        project_id: &str,
        spec: &NewTask,
        snapshot_records: &[Record],
        now: u64,
    ) -> (Record, Vec<Record>) {
        // A newly created task is pinned to all three families (ADR-028, ADR-029): which
        // argument shape starts it, which transcript format it writes, and which CLI's
        // `--resume` can read that transcript. `claude_gpt55` splits all three ways —
        // launch and resume are codex-ish while the transcript is Claude — which is exactly
        // why one field could never carry them.
        let launch_family = self.families.launch_family(&spec.agent);
        let session_family = self.families.session_family(&spec.agent);
        let resume_family = self.families.native_resume_family(&spec.agent);
        let mut record = Record(Default::default());
        record.0.insert("id".into(), Value::String(spec.id.clone()));
        record
            .0
            .insert("projectId".into(), Value::String(project_id.to_string()));
        record
            .0
            .insert("prompt".into(), Value::String(spec.prompt.clone()));
        // An empty prompt gets the agent's own label as its name; a blank name is not a
        // name and would leave a row nothing can clear.
        if !spec.prompt.trim().is_empty() {
            record.0.remove("name");
        } else {
            record
                .0
                .insert("name".into(), Value::String(spec.agent.clone()));
        }
        record
            .0
            .insert("agent".into(), Value::String(spec.agent.clone()));
        for (key, family) in [
            ("launchFamily", launch_family),
            ("sessionFamily", session_family),
            ("resumeFamily", resume_family),
        ] {
            record.0.insert(
                key.into(),
                Value::String(
                    serde_json::to_value(family)
                        .expect("a family is a string")
                        .as_str()
                        .expect("a family is a string")
                        .to_string(),
                ),
            );
        }
        if let Some(model) = &spec.selected_model {
            record
                .0
                .insert("selectedModel".into(), Value::String(model.clone()));
        }
        if let Some(effort) = &spec.reasoning_effort {
            record
                .0
                .insert("reasoningEffort".into(), Value::String(effort.clone()));
        }
        if let Some(speed) = &spec.speed {
            record
                .0
                .insert("speed".into(), Value::String(speed.clone()));
        }
        if let Some(config_id) = &spec.agent_config_id {
            record
                .0
                .insert("agentConfigId".into(), Value::String(config_id.clone()));
        }
        if launch_family == ProtocolFamily::Dsh {
            record.0.insert(
                "dshAgentPreset".into(),
                Value::String(
                    spec.dsh_agent_preset
                        .clone()
                        .unwrap_or_else(|| "standard".into()),
                ),
            );
        }
        record.0.insert(
            "permissionMode".into(),
            Value::String(spec.permission_mode.clone()),
        );
        record.0.insert(
            "status".into(),
            Value::String(
                serde_json::to_value(if spec.immediate {
                    TaskStatus::Pending
                } else {
                    TaskStatus::Todo
                })
                .expect("a status is a string")
                .as_str()
                .expect("a status is a string")
                .to_string(),
            ),
        );
        record
            .0
            .insert("createdAt".into(), Value::Number(now.into()));

        let mut records = snapshot_records.to_vec();
        records.insert(0, record.clone());
        (record, records)
    }

    /// Start a `todo`, or a freshly created immediate task. `persist_before_launch` is the
    /// remote-launch contract: the save must be durable *and* still be the newest word
    /// before a process exists.
    ///
    /// `window` carries the two facts only the calling window has (wiring §12) — the view's
    /// terminal size and, for DSH, its per-window launch facts. The request is the single
    /// description of the launch, so these ride on it rather than on any host-side side
    /// table; tests with no view pass [`HostWindowFacts::default`].
    pub fn start(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
        prompt_override: Option<String>,
        inject_prompt_into_terminal: bool,
        attachments: Attachments,
        host_facts: HostWindowFacts,
    ) -> Result<Applied, TaskError> {
        self.start_as(
            project,
            task_id,
            operation_key,
            persist_before_launch,
            prompt_override,
            inject_prompt_into_terminal,
            attachments,
            host_facts,
            Persist::Commit,
        )
    }

    /// 非递交平台的执行入口（§12）：claim/generation/executor 完全同 `start`，只有落盘
    /// 那一步改经宿主桥。远程 `persistBeforeLaunch` 在这条路上同样成立——桥是同步写。
    pub fn start_via_host(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
        prompt_override: Option<String>,
        inject_prompt_into_terminal: bool,
        attachments: Attachments,
        host_facts: HostWindowFacts,
    ) -> Result<Applied, TaskError> {
        self.start_as(
            project,
            task_id,
            operation_key,
            persist_before_launch,
            prompt_override,
            inject_prompt_into_terminal,
            attachments,
            host_facts,
            Persist::ViaHost,
        )
    }

    fn start_as(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
        prompt_override: Option<String>,
        inject_prompt_into_terminal: bool,
        attachments: Attachments,
        host_facts: HostWindowFacts,
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        self.begin(
            project,
            task_id,
            operation_key,
            OperationKind::Start,
            persist_before_launch,
            &attachments,
            persist,
            |service, record, project, generation| {
                let mut request = launch_plan::launch_request(
                    record,
                    project,
                    service.families.as_ref(),
                    operation_key,
                    generation,
                    attachments.images.clone(),
                    attachments.texts.clone(),
                    inject_prompt_into_terminal,
                    prompt_override,
                    host_facts.dsh,
                )
                .ok_or_else(|| {
                    TaskError::Unaddressable(project.id().unwrap_or_default().to_string())
                })?;
                request.terminal_size = host_facts.terminal_size;
                service.executor.launch(&request).map_err(|failure| {
                    service.abandon_launch(
                        record,
                        failure,
                        OperationKind::Start,
                        operation_key,
                        persist,
                    )
                })
            },
        )
    }

    /// Continue a settled task. Resolving *which* session to continue is a host
    /// responsibility (it reads the filesystem and the agent homes); deciding that an
    /// unresolvable session must not silently become a new conversation is this service's.
    ///
    /// The view's terminal size rides along (`None` in tests): without it every resumed
    /// PTY would open at the driver default and visibly resize, which is a behaviour
    /// change dressed as an omission. DSH never goes through here — its "resume" is a run
    /// command; see the executor.
    pub fn resume(
        &self,
        project: &Record,
        task_id: &str,
        session_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
        terminal_size: Option<(u16, u16)>,
    ) -> Result<Applied, TaskError> {
        self.resume_as(
            project,
            task_id,
            session_id,
            operation_key,
            persist_before_launch,
            terminal_size,
            Persist::Commit,
        )
    }

    /// 非递交平台的续跑入口（§12 步骤②c）：与 `resume` 同一条守卫链、同一次 executor 调用，
    /// 只有落盘改经宿主桥。守卫一条都不能少 —— `AwaitingAcknowledgement`（ADR-018）与空
    /// session 的拒绝在桥这一侧同样是"不许把旧对话悄悄换成一段新的"。
    ///
    /// 两侧一致的另一件事：**解析出的 session 不由 `resume` 落盘**。它只写状态那一段；把会话与
    /// 状态当一次原子递交写下去是 `ResumeWithSession` 那条复合意图的职责（Q70）。接线时如果以为
    /// spawn 顺手把会话写了，就会漏发那条意图；如果在这里补上第二段写，就又开出一个会话入口。
    pub fn resume_via_host(
        &self,
        project: &Record,
        task_id: &str,
        session_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
        terminal_size: Option<(u16, u16)>,
    ) -> Result<Applied, TaskError> {
        self.resume_as(
            project,
            task_id,
            session_id,
            operation_key,
            persist_before_launch,
            terminal_size,
            Persist::ViaHost,
        )
    }

    fn resume_as(
        &self,
        project: &Record,
        task_id: &str,
        session_id: &str,
        operation_key: &str,
        persist_before_launch: bool,
        terminal_size: Option<(u16, u16)>,
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        let blocked = self
            .load(project.id().unwrap_or_default())?
            .records
            .iter()
            .find(|row| row.id() == Some(task_id))
            .and_then(|row| row.get("worktreeDiscarded").and_then(Value::as_bool));
        if blocked == Some(true) {
            // The rule is `RunningView.tsx:724,741` today: the button simply is not shown
            // once the worktree has been merged or discarded, because the transcript it
            // would resume into belongs to a tree that no longer exists. A hidden button is
            // not a rule — any other caller could still ask — so the core refuses it too.
            return Err(TaskError::WorktreeDiscarded(task_id.to_string()));
        }
        if let Some(pending) = self.awaiting_acknowledgement(task_id) {
            // ADR-018: a run the last process cut off is not continuing material until
            // somebody has seen it. `start` clears this on purpose — beginning a new run is
            // itself an acknowledgement of the last one.
            let _ = pending;
            return Err(TaskError::AwaitingAcknowledgement {
                task_id: task_id.to_string(),
            });
        }
        if session_id.trim().is_empty() {
            // `handleResumeTask` toasts `running.resumeUnavailable` and returns without
            // touching anything. A blank id is not "resume with no session" — the CLI would
            // open a fresh conversation with the user's old prompt in it, and the user would
            // read the result as their history coming back.
            return Err(TaskError::NoSession(task_id.to_string()));
        }
        self.begin(
            project,
            task_id,
            operation_key,
            OperationKind::Resume,
            persist_before_launch,
            &Attachments::default(),
            persist,
            |service, record, project, generation| {
                let owner = session_owner::resolve_session_owner(record, service.families.as_ref());
                let with_session =
                    session_owner::apply_resolved_session(record, &owner, Some(session_id), None);
                // The record in the list has to be the one carrying the session, so the write
                // and the request agree about which transcript is being continued.
                let mut request = launch_plan::resume_request(
                    &with_session,
                    project,
                    service.families.as_ref(),
                    operation_key,
                    generation,
                    session_id,
                )
                .ok_or_else(|| {
                    TaskError::Unaddressable(project.id().unwrap_or_default().to_string())
                })?;
                request.terminal_size = terminal_size;
                service.executor.resume(&request).map_err(|failure| {
                    service.abandon_launch(
                        &with_session,
                        failure,
                        OperationKind::Resume,
                        operation_key,
                        persist,
                    )
                })
            },
        )
    }

    /// The remote channel limits, enforced rather than hidden behind a disabled button: a
    /// worktree is a local git object and attachments are typed into the local terminal
    /// (`src/state/app/taskLifecycle.ts:283-296`).
    fn check_target_shape(
        &self,
        project: &Record,
        task_id: &str,
        attachments: &Attachments,
    ) -> Result<(), TaskError> {
        let location = crate::projects::mutations::resolve_location(project);
        if location.is_local() {
            return Ok(());
        }
        let kind = if matches!(location, aeroric_protocol::ProjectTarget::Ssh { .. }) {
            "ssh"
        } else {
            "wsl"
        };
        if !attachments.images.is_empty() || !attachments.texts.is_empty() {
            return Err(TaskError::UnsupportedOnRemote {
                kind,
                what: "attachments",
            });
        }
        if let Some(row) = Self::find(
            &self.load(project.id().unwrap_or_default())?.records,
            task_id,
        ) {
            if row
                .get("worktreePath")
                .and_then(Value::as_str)
                .is_some_and(|path| !path.is_empty())
            {
                return Err(TaskError::UnsupportedOnRemote {
                    kind,
                    what: "a worktree",
                });
            }
        }
        Ok(())
    }

    /// The shared shape of start and resume: claim → persist → spawn, with the claim
    /// released on every path that never reached a process.
    fn begin(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        kind: OperationKind,
        persist_before_launch: bool,
        attachments: &Attachments,
        persist: Persist,
        spawn: impl FnOnce(&Self, &Record, &Record, u64) -> Result<(), TaskError>,
    ) -> Result<Applied, TaskError> {
        self.serial(|| {
            self.begin_under_lock(
                project,
                task_id,
                operation_key,
                kind,
                persist_before_launch,
                attachments,
                persist,
                spawn,
            )
        })
    }

    /// `begin` 持串行闸后的实体；`spawn` 闭包在闸内执行（进程诞生有序于一切变更）。
    fn begin_under_lock(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        kind: OperationKind,
        persist_before_launch: bool,
        attachments: &Attachments,
        persist: Persist,
        spawn: impl FnOnce(&Self, &Record, &Record, u64) -> Result<(), TaskError>,
    ) -> Result<Applied, TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        if !is_addressable(project) {
            return Err(TaskError::Unaddressable(project_id));
        }
        let snapshot = self.load(&project_id)?;
        let record = Self::find(&snapshot.records, task_id)
            .ok_or_else(|| TaskError::NotFound {
                project_id: project_id.clone(),
                task_id: task_id.to_string(),
            })?
            .clone();
        self.check_target_shape(project, task_id, attachments)?;

        // One in-flight attempt per task and kind: the check comes first so a retry neither
        // reserves a generation nor reaches the executor. Advancing the counter before
        // asking would hand every retry a brand-new key, and "spawn once" would be a rule
        // with no mechanism behind it.
        let (generation, claim) = self.ledger(|ledger| {
            match ledger.open_claim(task_id, kind, operation_key) {
                // The same operation key arriving twice is one attempt, whoever sent it. A
                // different key on the same task is a different attempt and is allowed
                // through — that is what makes two windows able to run one task (ADR-017).
                Some(existing) => (
                    ledger.peek_generation(task_id),
                    Claimed::Duplicate(existing.clone()),
                ),
                None => {
                    let generation = ledger.next_generation(task_id);
                    let claim = ledger.claim(
                        task_id,
                        kind,
                        operation_key,
                        generation,
                        self.clock().now_ms(),
                    );
                    (generation, claim)
                }
            }
        });
        match claim {
            Claimed::New(claim) => claim,
            Claimed::Duplicate(claim) => {
                return Err(TaskError::AlreadyClaimed {
                    task_id: task_id.to_string(),
                    kind: kind.as_str(),
                    claimed_at_ms: claim.claimed_at_ms,
                });
            }
        };

        let prepared = prepare_for_launch(&record, kind);
        let mut records = snapshot.records.clone();
        let position = records
            .iter()
            .position(|row| row.id() == Some(task_id))
            .expect("found above");
        records[position] = prepared.clone();

        let applied = match self.persist_launch(&project_id, &records, persist) {
            Ok(applied) => applied,
            Err(error) => {
                // Nothing reached the disk, so the claim describes an attempt that will
                // never be made.
                self.ledger(|ledger| {
                    ledger.settle(task_id, kind, operation_key);
                    ledger.persist(&self.context)
                })
                .ok();
                return Err(match error {
                    TaskError::Storage(inner) if inner.is_stale_write() => {
                        TaskError::PersistFailed(inner.to_string())
                    }
                    other => other,
                });
            }
        };

        if kind == OperationKind::Start && self.acknowledge(task_id).unwrap_or(false) {
            // Beginning a fresh run is itself the acknowledgement of the cut-off one: the
            // user chose a new start over "continue where it stopped", so holding the flag
            // any longer would only block the next resume for no information gained.
        }
        if let Err(error) = spawn(self, &prepared, project, generation) {
            // The record is already saved; the process never came up. The claim must be
            // released either way, or a genuine later attempt is locked out forever.
            self.settle(task_id, kind, operation_key);
            return Err(error);
        }
        if !persist_before_launch {
            // The host still has to tell us the terminal is up before the first input goes
            // in; until then the claim stays open, which is what makes a retried request a
            // duplicate rather than a second process.
            self.settle(task_id, kind, operation_key);
        }
        Ok(applied)
    }

    /// Release whatever attempt of this task reached the given run number. An event knows
    /// the generation, not the caller's idempotency key.
    fn settle_by_generation(&self, task_id: &str, generation: u64) {
        self.ledger(|ledger| {
            for kind in [OperationKind::Start, OperationKind::Resume] {
                ledger.settle_by_generation(task_id, kind, generation);
            }
            let _ = ledger.persist(&self.context);
        });
    }

    fn settle(&self, task_id: &str, kind: OperationKind, operation_key: &str) {
        self.ledger(|ledger| {
            ledger.settle(task_id, kind, operation_key);
            let _ = ledger.persist(&self.context);
        });
    }

    /// A spawn that failed before any process existed: undo the status this attempt wrote,
    /// and report the reason. `abandon_launch` mirrors `reportLaunchFailure` →
    /// `onFailed` → `updateTaskStatus(failed, reason)`, which is what the client does.
    fn abandon_launch(
        &self,
        record: &Record,
        failure: ExecutorFailure,
        kind: OperationKind,
        operation_key: &str,
        persist: Persist,
    ) -> TaskError {
        let project_id = record
            .str_field("projectId")
            .unwrap_or_default()
            .to_string();
        let task_id = record.id().unwrap_or_default().to_string();
        let reason = failure.to_string();
        if let Ok(snapshot) = self.load(&project_id) {
            if let Some(current) = Self::find(&snapshot.records, &task_id) {
                let rolled_back =
                    reducer::rollback_mutation(current, record, &prepare_for_launch(record, kind));
                let transition = reducer::apply_status_transition(
                    &[rolled_back],
                    &task_id,
                    TaskStatus::Failed,
                    None,
                    Some(&reason),
                    self.clock().now_ms(),
                );
                // Best effort, and deliberately not allowed to mask the launch error: the
                // caller still has to be told the process did not come up.
                let _ = self.persist_launch(&project_id, &transition.records, persist);
            }
        }
        self.settle(&task_id, kind, operation_key);
        TaskError::Launch(reason)
    }

    /// `handleSwitchTaskConfig` in `src/state/app/taskSwitch.ts` — a manual configuration
    /// change, as the ordering rule it is.
    ///
    /// What the host brings is what only the host can know: the reset snapshot the running
    /// process reported, whether the transcript supports a native resume, and the handoff
    /// text it assembled. What this decides is the order and the failure path:
    /// reset → attribute the session → pick the continuation mechanism → **refuse if there is
    /// nothing to continue from** → persist → only then let the run be started. A persist that
    /// fails rolls back exactly the fields this switch moved, keeping whatever a late hook
    /// wrote in the meantime, and never reaches a spawn.
    pub fn switch_config(
        &self,
        project: &Record,
        task_id: &str,
        operation_key: &str,
        values: &ConfigSwitch,
        observed: &SwitchEvidence,
    ) -> Result<(ConfigSwitchStrategy, Applied), TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        if !is_addressable(project) {
            return Err(TaskError::Unaddressable(project_id));
        }
        let snapshot = self.load(&project_id)?;
        let previous = Self::find(&snapshot.records, task_id)
            .ok_or_else(|| TaskError::NotFound {
                project_id: project_id.clone(),
                task_id: task_id.to_string(),
            })?
            .clone();

        // A switch ends in a process, so it takes an attempt out before it touches anything:
        // retried with the same key it is one attempt, and the second caller learns that
        // before a reset kills the run the first one is waiting on.
        let generation = match self.ledger(|ledger| {
            match ledger.open_claim(task_id, OperationKind::Start, operation_key) {
                Some(existing) => Claimed::Duplicate(existing.clone()),
                None => {
                    let generation = ledger.next_generation(task_id);
                    ledger.claim(
                        task_id,
                        OperationKind::Start,
                        operation_key,
                        generation,
                        self.clock().now_ms(),
                    )
                }
            }
        }) {
            Claimed::New(claim) => claim.generation,
            Claimed::Duplicate(existing) => {
                return Err(TaskError::AlreadyClaimed {
                    task_id: task_id.to_string(),
                    kind: existing.kind.as_str(),
                    claimed_at_ms: existing.claimed_at_ms,
                });
            }
        };
        // Every path below that never reaches a process hands the run number back: a skipped
        // number would make an event for the *next* run look like one for a run that already
        // happened somewhere.
        let release = |service: &Self, _reason: &str| {
            service.ledger(|ledger| {
                ledger.settle(task_id, OperationKind::Start, operation_key);
                ledger.release_generation(task_id, generation);
                let _ = ledger.persist(&service.context);
            });
        };

        // The reset is the first side effect, and it happens *before* any write: a switch
        // that cannot stop the current process changes nothing, exactly as the client's
        // early `return false` does.
        let reset = self.executor.reset(task_id).map_err(|failure| {
            release(self, "reset failed");
            TaskError::Launch(failure.to_string())
        })?;
        let attributed = launch_plan::merge_reset_session(&previous, &reset);
        let owner = session_owner::resolve_session_owner(&attributed, self.families.as_ref());

        let strategy = session_owner::resolve_config_switch_strategy(
            &attributed,
            &values.agent,
            observed.native_resume_supported,
            self.families.as_ref(),
        );
        let resuming = strategy == ConfigSwitchStrategy::Resume
            || (strategy == ConfigSwitchStrategy::Adopt && observed.adopted_session_id.is_some());
        let session_id = match strategy {
            ConfigSwitchStrategy::Resume => observed.session_id.as_deref(),
            ConfigSwitchStrategy::Adopt => observed.adopted_session_id.as_deref(),
            ConfigSwitchStrategy::Handoff => None,
        };
        if !resuming
            && observed
                .handoff_prompt
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            let has_prompt = attributed
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .is_empty();
            if has_prompt {
                release(self, "nothing to continue from");
                // `switchConfigNoContext`: continuing without a transcript, terminal history
                // or a prompt would start an amnesiac session and present it as the same task.
                return Err(TaskError::NoContinuationContext(task_id.to_string()));
            }
        }

        let applied_record = {
            let mut committed = session_owner::apply_resolved_session(
                &attributed,
                &owner,
                session_id,
                observed.adopted_session_path.as_deref(),
            );
            committed
                .0
                .insert("agent".to_string(), Value::String(values.agent.clone()));
            for (key, value) in [
                ("selectedModel", &values.selected_model),
                ("reasoningEffort", &values.reasoning_effort),
                ("speed", &values.speed),
                ("permissionMode", &Some(values.permission_mode.clone())),
            ] {
                match value {
                    Some(text) => {
                        committed
                            .0
                            .insert(key.to_string(), Value::String(text.clone()));
                    }
                    None => {
                        committed.0.remove(key);
                    }
                }
            }
            committed.0.insert(
                "status".to_string(),
                Value::String(
                    serde_json::to_value(TaskStatus::Pending)
                        .expect("a status is a string")
                        .as_str()
                        .expect("a status is a string")
                        .to_string(),
                ),
            );
            committed.0.remove("attentionRequestedAt");
            committed.0.remove("failureReason");
            committed
        };

        let mut records = snapshot.records.clone();
        let position = records
            .iter()
            .position(|row| row.id() == Some(task_id))
            .expect("found above");
        records[position] = applied_record.clone();

        // Core never writes optimistically, so a failed persist has nothing on disk to
        // undo: the record set was never replaced, and the bytes that beat us to it stay
        // exactly as they are. The field-level rollback that the client needs
        // (`reducer::rollback_mutation`) is its own optimistic list, and the parity table
        // proves that rule ported; here the correct action is to hand the refusal back.
        let committed = match self.commit(&project_id, &records) {
            Ok(committed) => committed,
            Err(error) => {
                release(self, "the switch could not be saved");
                return Err(match error {
                    TaskError::Storage(ref inner) if inner.is_stale_write() => {
                        TaskError::PersistFailed(inner.to_string())
                    }
                    other => other,
                });
            }
        };
        if strategy == ConfigSwitchStrategy::Handoff {
            // A handoff starts a *new* run carrying the previous conversation as text, so it
            // acknowledges any cut-off run the same way `start` does.
            let _ = self.acknowledge(task_id);
        }
        let _ = self.ledger(|ledger| ledger.persist(&self.context));
        Ok((
            strategy,
            Applied {
                generation: Some(generation),
                ..committed
            },
        ))
    }

    /// Where a hook/exit event lands. One reducer, one owner: this is the only way an
    /// outside observation moves a task, and the generation guard is what keeps a late exit
    /// from settling a task that has already been resumed.
    ///
    /// The writer half. On a platform that has not been handed the writer role this returns
    /// `Err` from [`TaskService::commit`] — which is why the announcement path calls
    /// [`decide_observation`](TaskService::decide_observation) instead (ADR-035).
    pub fn observe(
        &self,
        project_id: &str,
        event: TaskEvent,
    ) -> Result<(EventOutcome, Applied), TaskError> {
        self.apply_observation(project_id, event, Persist::Commit)
    }

    /// ADR-035: the same decision, without the write.
    ///
    /// Which status a run's report produces, whether it is dropped, and whether the task is
    /// therefore still live are decided here and nowhere else — but the bytes stay wherever
    /// the host put them. This is what lets core be the only emitter of `task-status` on a
    /// platform where it is not the writer: the reads (`load_stamped`) need no write
    /// authority, only `save_covering` does.
    ///
    /// The returned [`Applied`] carries the post-decision records with `revision: None` and
    /// `fingerprint: None`, because nothing was written. The in-memory claim *is* settled for
    /// a terminal observation: a run that has reported has reported, and the alternative —
    /// leaving the claim open so a later commit can see it — would let the same run settle a
    /// task twice.
    pub fn decide_observation(
        &self,
        project_id: &str,
        event: TaskEvent,
    ) -> Result<(EventOutcome, Applied), TaskError> {
        self.apply_observation(project_id, event, Persist::DecideOnly)
    }

    fn apply_observation(
        &self,
        project_id: &str,
        event: TaskEvent,
        persist: Persist,
    ) -> Result<(EventOutcome, Applied), TaskError> {
        self.serial(|| self.apply_observation_under_lock(project_id, event, persist))
    }

    /// `apply_observation` 持串行闸后的实体——settle 与变更同闸，观察既不误拒
    /// 也不被陈旧快照覆盖（executor 退出线程走的就是这条路）。
    fn apply_observation_under_lock(
        &self,
        project_id: &str,
        event: TaskEvent,
        persist: Persist,
    ) -> Result<(EventOutcome, Applied), TaskError> {
        let snapshot = self.load(project_id)?;
        let current = self.generation(event.task_id());
        let seen = event.generation();
        if seen < current && !self.is_run_live(event.task_id(), seen) {
            // "Older than the newest number" is not enough to call an event stale, because
            // two runs of one task may be live at once (ADR-017): run 1's exit is real even
            // after run 2 started. What *is* stale is a run that already reported — no claim
            // in flight — or one that never had a process behind it. Applying such an event
            // would knock a live task off `running` on the strength of a world that already
            // ended.
            return Ok((
                EventOutcome::Dropped("stale generation"),
                self.unchanged(snapshot.records.clone(), snapshot.fingerprint.clone()),
            ));
        }
        if seen > current {
            // An event for a run this process has never claimed means the host is ahead of
            // the ledger — which is a wiring bug, not something to guess past.
            return Err(TaskError::FutureGeneration {
                task_id: event.task_id().to_string(),
                seen,
                current,
            });
        }
        // The fourth element is whether ADR-034's guard applies at all, and it is decided by
        // the *source* of the report, not by the status word: ADR-034 decision 2 excludes the
        // host's own changes (launch, cancel, manual completion, dsh's state machine) from the
        // rule, and those are exactly the reports whose runs never appear in core's ledger.
        let (status, attention, reason, guarded) = match &event {
            TaskEvent::Status {
                status,
                attention_requested_at_ms,
                failure_reason,
                origin,
                ..
            } => (
                *status,
                *attention_requested_at_ms,
                failure_reason.clone(),
                origin.is_guarded(),
            ),
            TaskEvent::Exit { code, .. } => {
                // One rule, shared with the unrecorded path (`settled_by_exit`), so the two
                // can never disagree about what a nonzero exit is called.
                let (settled, reason) = settled_by_exit(*code);
                (settled, None, reason, false)
            }
            TaskEvent::TerminalReady { .. } => {
                // First input can go in: the attempt it was waiting for is over, and from
                // here on the status is whatever the run itself reports.
                self.settle_by_generation(event.task_id(), seen);
                return Ok((
                    EventOutcome::Dropped("terminal ready moves no status"),
                    self.unchanged(snapshot.records.clone(), snapshot.fingerprint.clone()),
                ));
            }
        };
        // ADR-034: an observation may not resurrect a task that has no run left. This is the
        // guard the host used to apply at three transcript-watcher call sites via
        // `is_task_active`, and the case the staleness check above does *not* cover: a watcher
        // thread outlives the process it polls (each carries its own deadline), so a line read
        // late arrives with the *same* generation as the run that just settled the row — neither
        // older nor future, therefore admissible above, and without this rule it re-lights a
        // finished card.
        //
        // Deliberately here rather than in `should_ignore_transition`: that table is held
        // byte-equivalent to the client's `applyTaskStatusTransition`, which carries no such rule
        // because the client never sees these events. This is about admitting an observation, not
        // about what a transition means.
        //
        // The test is "does this *task* have any live run", not "is this row terminal".
        // ADR-034's decision 2 limits the rule to late observations, and a row that reads
        // `done`/`cancelled` while a claim is still open is not a late observation — it is
        // either a second run overlapping a settled one (ADR-017) or the cancel window
        // (row already `cancelled`, process still dying), and the host emits unconditionally in
        // both. Gating on the row instead would silently swallow the `running` that starts a
        // re-run of a finished task, which is a user-visible regression dressed up as a guard.
        if guarded && !status.is_terminal() && !self.is_task_live(event.task_id()) {
            return Ok((
                EventOutcome::Dropped("no live run for this task"),
                self.unchanged(snapshot.records.clone(), snapshot.fingerprint.clone()),
            ));
        }
        // One task row carries one status, so with overlapping runs the honest answer to
        // "is the task finished?" is "no, not while another run is live": this run is done
        // (its claim is released below), and the task itself moves back to `running`.
        let reported_terminal = status.is_terminal();
        let overlapping = reported_terminal
            && !self
                .live_other_generations(event.task_id(), seen)
                .is_empty();
        let status = if overlapping {
            TaskStatus::Running
        } else {
            status
        };
        let transition = reducer::apply_status_transition(
            &snapshot.records,
            event.task_id(),
            status,
            attention,
            reason.as_deref(),
            self.clock().now_ms(),
        );
        if reported_terminal {
            // Released here rather than after the write: folding a terminal event into
            // `running` because another run is live usually leaves the row unchanged, and an
            // unchanged row must not keep a finished run's claim open — that would block the
            // same operation key from ever being retried.
            self.settle_by_generation(event.task_id(), seen);
        }
        if !transition.changed {
            return Ok((
                EventOutcome::Applied {
                    status,
                    changed: false,
                    failure_reason: reason.clone(),
                },
                self.unchanged(snapshot.records, snapshot.fingerprint),
            ));
        }
        let outcome = EventOutcome::Applied {
            status,
            changed: true,
            failure_reason: reason.clone(),
        };
        match persist {
            Persist::Commit => Ok((outcome, self.commit(project_id, &transition.records)?)),
            Persist::ViaHost => Ok((
                outcome,
                self.save_via_host(project_id, &transition.records)?,
            )),
            Persist::DecideOnly => Ok((
                outcome,
                Applied {
                    records: transition.records,
                    // ADR-035: "no revision, no fingerprint" is the honest answer here, and it
                    // is a different answer from "the file did not change" — `changed` still
                    // reports what the reducer decided, because that is what the announcement
                    // path is not allowed to re-derive.
                    revision: None,
                    changed: true,
                    fingerprint: None,
                    generation: None,
                },
            )),
        }
    }

    /// The manual "this is finished". A live task's process is stopped and the status is
    /// left for the exit event to settle; a task with no process is moved right now.
    pub fn complete(
        &self,
        project: &Record,
        task_id: &str,
    ) -> Result<(Completion, Option<Applied>), TaskError> {
        self.serial(|| self.complete_under_lock(project, task_id))
    }

    /// `complete` 持串行闸后的实体。
    fn complete_under_lock(
        &self,
        project: &Record,
        task_id: &str,
    ) -> Result<(Completion, Option<Applied>), TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        let snapshot = self.load(&project_id)?;
        let record = Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
            project_id: project_id.clone(),
            task_id: task_id.to_string(),
        })?;
        let kind = launch_plan::completion_for(record, self.families.as_ref());
        let working_directory = launch_plan::process_path(record, project);
        match kind {
            Completion::Dsh | Completion::StopProcess => {
                self.executor
                    .complete(task_id, &working_directory)
                    .map_err(|failure| TaskError::Launch(failure.to_string()))?;
                Ok((kind, None))
            }
            Completion::Nothing => {
                let transition = reducer::apply_status_transition(
                    &snapshot.records,
                    task_id,
                    TaskStatus::Done,
                    None,
                    None,
                    self.clock().now_ms(),
                );
                let applied = self.commit(&project_id, &transition.records)?;
                Ok((kind, Some(applied)))
            }
        }
    }

    /// Stop the process. The status is the exit event's to write, not this call's — the
    /// difference between "we asked" and "it happened".
    pub fn cancel(&self, project: &Record, task_id: &str) -> Result<CancelTargetInfo, TaskError> {
        self.serial(|| self.cancel_under_lock(project, task_id))
    }

    /// `cancel` 持串行闸后的实体。
    fn cancel_under_lock(
        &self,
        project: &Record,
        task_id: &str,
    ) -> Result<CancelTargetInfo, TaskError> {
        let project_id = project.id().unwrap_or_default().to_string();
        let snapshot = self.load(&project_id)?;
        let record = Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
            project_id: project_id.clone(),
            task_id: task_id.to_string(),
        })?;
        let target = launch_plan::cancel_target(record, project, self.families.as_ref());
        self.executor
            .cancel(task_id, &target)
            .map_err(|failure| TaskError::Launch(failure.to_string()))?;
        // Whatever was in flight on this task is now moot: a claim that outlives a cancel
        // would lock the next legitimate start out.
        self.ledger(|ledger| {
            ledger.settle_task(task_id);
            let _ = ledger.persist(&self.context);
        });
        Ok(CancelTargetInfo {
            target: format!("{target:?}"),
        })
    }

    /// `deleteTasks`. Starred rows are skipped, an unresolvable project is refused, and the
    /// transcript on disk is left alone: removing a row from the list is not permission to
    /// delete the work it describes.
    pub fn delete(&self, project_id: &str, task_ids: &[String]) -> Result<Deleted, TaskError> {
        self.delete_with(project_id, task_ids, Persist::Commit)
    }

    fn delete_with(
        &self,
        project_id: &str,
        task_ids: &[String],
        persist: Persist,
    ) -> Result<Deleted, TaskError> {
        let snapshot = self.load(project_id)?;
        let starred = |id: &str| {
            Self::find(&snapshot.records, id)
                .map(|record| {
                    record
                        .get("starred")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        };
        let requested: Vec<String> = task_ids.iter().filter(|id| !starred(id)).cloned().collect();
        // Reported even when the deletion went through: the client counts these to keep its
        // "N tasks deleted" message honest, and a silent skip reads like a failed delete.
        let skipped_starred: Vec<String> =
            task_ids.iter().filter(|id| starred(id)).cloned().collect();
        if requested.is_empty() {
            return Ok(Deleted {
                removed: Vec::new(),
                skipped_starred,
                applied: self.unchanged(snapshot.records.clone(), snapshot.fingerprint),
            });
        }
        let blocked: Vec<String> = requested
            .iter()
            .filter(|id| {
                Self::find(&snapshot.records, id)
                    .map(|record| {
                        crate::tasks::reducer::status_of(record)
                            .map(|status| status.is_live())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if !blocked.is_empty() {
            return Err(TaskError::AlreadyClaimed {
                task_id: blocked.join(", "),
                kind: "running",
                claimed_at_ms: 0,
            });
        }
        let records: Vec<Record> = snapshot
            .records
            .iter()
            .filter(|record| !requested.iter().any(|id| record.id() == Some(id.as_str())))
            .cloned()
            .collect();
        for id in &requested {
            // A deleted task has nothing left to acknowledge.
            self.ledger(|ledger| {
                ledger.settle_task(id);
                ledger.acknowledge(id);
            });
        }
        let applied = self.finish_intent(project_id, records, persist)?;
        // The router must stop answering for ids that no longer exist; this is the only
        // operation that can make a remembered project wrong (ADR-035's `project_of`).
        self.forget_projects(&requested);
        Ok(Deleted {
            removed: requested,
            skipped_starred,
            applied,
        })
    }

    pub fn mutate(
        &self,
        project_id: &str,
        change: impl Fn(&[Record], u64) -> (Vec<Record>, bool),
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        let (records, changed) = change(&snapshot.records, self.clock().now_ms());
        if !changed || records == snapshot.records {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        self.commit(project_id, &records)
    }

    /// ADR-022 决策 2 的元数据入口。这四个操作在 `reducer` 里早就有了，但 ADR-022 第 17 行
    /// 说"core 侧已经有逐 op 的服务"并不成立：没有公开入口，宿主就只能走 `mutate` 自己拼
    /// 闭包，而那等于把"哪个字段谁能写"重新交回宿主记账——正是这条 ADR 否掉的形态。
    ///
    /// 共同形状：先确认这些行真的存在（改一个不存在的任务必须是错误，不是静默无变化），
    /// 再在权威快照上跑 reducer，最后按 `persist` 决定提交还是只交回决定结果。
    fn metadata_edit(
        &self,
        project_id: &str,
        task_id: &str,
        persist: Persist,
        change: impl Fn(&[Record]) -> Vec<Record>,
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
            project_id: project_id.to_string(),
            task_id: task_id.to_string(),
        })?;
        let records = change(&snapshot.records);
        if records == snapshot.records {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        self.finish_intent(project_id, records, persist)
    }

    /// 改名。**空串不是名字**：reducer 会删掉 `name` 键，与旧客户端 `renameTaskInList` 一致，
    /// 否则任务留下一个再也清不掉的空标签。
    pub fn rename(
        &self,
        project_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Applied, TaskError> {
        self.run_rename(project_id, task_id, name, None, Persist::Commit)
    }

    /// 收藏开关。它读的是当前值：取消收藏写的是 `starred: false`，不是"把键删掉回到原点"，
    /// 与旧客户端一致；两次调用因此都是真实的一格 revision。
    pub fn toggle_star(&self, project_id: &str, task_id: &str) -> Result<Applied, TaskError> {
        self.metadata_edit(project_id, task_id, Persist::Commit, |records| {
            reducer::toggle_star(records, task_id)
        })
    }

    /// 归档/取消归档：旧客户端一次点选多行，所以这是一条**批量**意图，ids 里任何一行不存在
    /// 都整条拒绝——只归档一半比全不归档更难解释。
    fn metadata_batch(
        &self,
        project_id: &str,
        task_ids: &[String],
        persist: Persist,
        change: impl Fn(&[Record]) -> (Vec<Record>, bool),
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        for task_id in task_ids {
            Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
                project_id: project_id.to_string(),
                task_id: task_id.clone(),
            })?;
        }
        let (records, changed) = change(&snapshot.records);
        if !changed || records == snapshot.records {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        self.finish_intent(project_id, records, persist)
    }

    pub fn archive(&self, project_id: &str, task_ids: &[String]) -> Result<Applied, TaskError> {
        let now_ms = self.clock().now_ms();
        self.metadata_batch(project_id, task_ids, Persist::Commit, |records| {
            reducer::archive(records, task_ids, now_ms)
        })
    }

    pub fn unarchive(&self, project_id: &str, task_ids: &[String]) -> Result<Applied, TaskError> {
        self.metadata_batch(project_id, task_ids, Persist::Commit, |records| {
            reducer::unarchive(records, task_ids)
        })
    }

    /// 单行、reducer 自己报"改没改"的那一类意图的共同形状：先确认行存在（改一个不存在的任务
    /// 必须是错误，不是静默无变化），再在权威快照上跑 reducer。
    fn metadata_one(
        &self,
        project_id: &str,
        task_id: &str,
        persist: Persist,
        change: impl Fn(&[Record]) -> (Vec<Record>, bool),
    ) -> Result<Applied, TaskError> {
        self.metadata_batch(project_id, &[task_id.to_string()], persist, change)
    }

    /// #21/#9：会话挂接。宿主交的是事实（哪个族、哪个 agent、拿到什么 id/路径），
    /// "清四族只设一族"与"两个都空就不动"由 core 判（ADR-037 决策 1）。
    pub fn attach_session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        session_agent: &str,
        session_id: Option<&str>,
        session_path: Option<&str>,
    ) -> Result<Applied, TaskError> {
        self.metadata_one(project_id, task_id, Persist::Commit, |records| {
            reducer::attach_session(
                records,
                task_id,
                family,
                session_agent,
                session_id,
                session_path,
            )
        })
    }

    pub fn set_worktree(
        &self,
        project_id: &str,
        task_id: &str,
        path: &str,
        branch: &str,
        base_branch: &str,
    ) -> Result<Applied, TaskError> {
        self.metadata_one(project_id, task_id, Persist::Commit, |records| {
            reducer::set_worktree(records, task_id, path, branch, base_branch)
        })
    }

    pub fn discard_worktree(&self, project_id: &str, task_id: &str) -> Result<Applied, TaskError> {
        self.metadata_one(project_id, task_id, Persist::Commit, |records| {
            reducer::discard_worktree(records, task_id)
        })
    }

    pub fn record_diff_stats(
        &self,
        project_id: &str,
        task_id: &str,
        additions: u64,
        deletions: u64,
    ) -> Result<Applied, TaskError> {
        self.metadata_one(project_id, task_id, Persist::Commit, |records| {
            reducer::record_diff_stats(records, task_id, additions, deletions)
        })
    }

    /// 这一行现在归属哪一族的会话路径——`RenameExpect` 第四样比的正是它，且必须从**权威快照**
    /// 算，不然前置又退回"宿主以为自己看到的那份是新的"。
    fn resolved_session_path(&self, record: &Record) -> Option<String> {
        let owner = session_owner::resolve_session_owner(record, self.families.as_ref());
        let fields = session_owner::session_fields(record, owner.family);
        fields.session_path.or(fields.legacy_session_path)
    }

    /// 带前置的改名走这一条：前置不成立 ⇒ 原样返回、`changed: false`、**不报错**，也不烧
    /// revision（ADR-037 决策 4，与旧客户端"静默丢弃过期 LLM 结果"一字不变）。
    fn run_rename(
        &self,
        project_id: &str,
        task_id: &str,
        name: &str,
        expect: Option<&reducer::RenameExpect>,
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        let record = Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
            project_id: project_id.to_string(),
            task_id: task_id.to_string(),
        })?;
        if let Some(want) = expect {
            let current_path = self.resolved_session_path(record);
            if !reducer::rename_precondition_holds(record, want, current_path.as_deref()) {
                return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
            }
        }
        let records = reducer::rename(&snapshot.records, task_id, name);
        if records == snapshot.records {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        self.finish_intent(project_id, records, persist)
    }

    /// #11：启动修复逐项目投递（宿主的扇出循环在宿主）。宿主交出的事实只有"哪些 id 还挂着
    /// 活子进程"；四条判据在 `reducer::normalize_after_boot`，"是不是 dsh 族"由宿主家族表
    /// 回答——那是 ADR-028 的唯一权威，core 只是把它的回答带给规则。
    pub fn normalize_after_boot(
        &self,
        project_id: &str,
        live_task_ids: &[String],
    ) -> Result<Applied, TaskError> {
        self.normalize_after_boot_with(project_id, live_task_ids, Persist::Commit)
    }

    fn normalize_after_boot_with(
        &self,
        project_id: &str,
        live_task_ids: &[String],
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        let families = self.families.clone();
        let (records, changed) = reducer::normalize_after_boot(
            &snapshot.records,
            live_task_ids,
            self.clock().now_ms(),
            &|agent| families.session_family(agent) == ProtocolFamily::Dsh,
        );
        if !changed {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        self.finish_intent(project_id, records, persist)
    }

    /// #9：续跑前的那**一次**写。会话字段与 #21 走同一条 `apply_resolved_session`（清三设一），
    /// 外加 `agent`、`status: pending` 与两个清空——都落在 core 刚读出的权威行上，而不是宿主
    /// 内存里那份可能已过期的整行（ADR-037 决策 3：载荷与今天那一次写等价）。
    pub fn resume_with_session(
        &self,
        project_id: &str,
        task_id: &str,
        agent: &str,
        family: ProtocolFamily,
        session_id: Option<&str>,
        session_path: Option<&str>,
    ) -> Result<Applied, TaskError> {
        self.resume_with_session_with(
            project_id,
            task_id,
            agent,
            family,
            session_id,
            session_path,
            Persist::Commit,
        )
    }

    fn resume_with_session_with(
        &self,
        project_id: &str,
        task_id: &str,
        agent: &str,
        family: ProtocolFamily,
        session_id: Option<&str>,
        session_path: Option<&str>,
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        let position = snapshot
            .records
            .iter()
            .position(|row| row.id() == Some(task_id))
            .ok_or_else(|| TaskError::NotFound {
                project_id: project_id.to_string(),
                task_id: task_id.to_string(),
            })?;
        let record = &snapshot.records[position];
        let owner = session_owner::SessionOwner {
            agent: agent.to_string(),
            family,
            codex_like: family == ProtocolFamily::Codex,
        };
        let mut committed =
            session_owner::apply_resolved_session(record, &owner, session_id, session_path);
        committed
            .0
            .insert("agent".to_string(), Value::String(agent.to_string()));
        committed.0.insert(
            "status".to_string(),
            Value::String(
                serde_json::to_value(TaskStatus::Pending)
                    .expect("a status is a string")
                    .as_str()
                    .expect("a status is a string")
                    .to_string(),
            ),
        );
        committed.0.remove("attentionRequestedAt");
        committed.0.remove("failureReason");
        if &committed == record {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        let mut records = snapshot.records.clone();
        records[position] = committed;
        self.finish_intent(project_id, records, persist)
    }

    /// #12：配置切换的复合写＝一条原子意图。三路合并里 core 提供"当前行"这一份——今天
    /// 它来自宿主内存（`latestTask`），权威化之后它就是磁盘上的那份；宿主只交得出
    /// previous/resolved 的**差**（`taskSwitch.ts:34` 的 `Object.is` 判据搬到 core）。
    pub fn switch_config_write(
        &self,
        project_id: &str,
        task_id: &str,
        agent: &str,
        selected_model: Option<&str>,
        reasoning_effort: Option<&str>,
        speed: Option<&str>,
        permission_mode: &str,
        previous: &SessionSnapshot,
        resolved: &SessionSnapshot,
    ) -> Result<Applied, TaskError> {
        self.switch_config_write_with(
            project_id,
            task_id,
            agent,
            selected_model,
            reasoning_effort,
            speed,
            permission_mode,
            previous,
            resolved,
            Persist::Commit,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn switch_config_write_with(
        &self,
        project_id: &str,
        task_id: &str,
        agent: &str,
        selected_model: Option<&str>,
        reasoning_effort: Option<&str>,
        speed: Option<&str>,
        permission_mode: &str,
        previous: &SessionSnapshot,
        resolved: &SessionSnapshot,
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        let snapshot = self.load(project_id)?;
        Self::find(&snapshot.records, task_id).ok_or_else(|| TaskError::NotFound {
            project_id: project_id.to_string(),
            task_id: task_id.to_string(),
        })?;
        let mut delta: Vec<(&str, Option<Value>)> = Vec::new();
        for (key, before, after) in previous.zip_entries(resolved) {
            if before != after {
                delta.push((key, after));
            }
        }
        delta.push(("agent", Some(Value::String(agent.to_string()))));
        for (key, value) in [
            ("selectedModel", selected_model),
            ("reasoningEffort", reasoning_effort),
            ("speed", speed),
        ] {
            delta.push((key, value.map(|text| Value::String(text.to_string()))));
        }
        delta.push((
            "permissionMode",
            Some(Value::String(permission_mode.to_string())),
        ));
        delta.push((
            "status",
            Some(Value::String(
                serde_json::to_value(TaskStatus::Pending)
                    .expect("a status is a string")
                    .as_str()
                    .expect("a status is a string")
                    .to_string(),
            )),
        ));
        delta.push(("attentionRequestedAt", None));
        delta.push(("failureReason", None));
        let (records, changed) = reducer::apply_field_delta(&snapshot.records, task_id, &delta);
        if !changed {
            return Ok(self.unchanged(snapshot.records, snapshot.fingerprint));
        }
        self.finish_intent(project_id, records, persist)
    }

    /// ADR-035 给观察路径的那一半，搬到 mutation 上：非递交写权的平台上，core 仍然决定结果，
    /// 保存者还是宿主。"没有 revision、没有指纹"是这里的诚实答案，与"文件没变"是两回事。
    pub fn apply_intent(
        &self,
        project_id: &str,
        intent: &TaskIntent,
    ) -> Result<Applied, TaskError> {
        self.run_intent(project_id, intent, Persist::Commit)
    }

    pub fn decide_intent(
        &self,
        project_id: &str,
        intent: &TaskIntent,
    ) -> Result<Applied, TaskError> {
        self.run_intent(project_id, intent, Persist::DecideOnly)
    }

    /// `apply_task_intent` 在非递交平台的那一格（§12）：命令层按 `gateway.mode()` 选这条，
    /// core 决定 + 桥保存，一次调用完成，不再需要"decide 后手接转发"。
    pub fn apply_intent_via_host(
        &self,
        project_id: &str,
        intent: &TaskIntent,
    ) -> Result<Applied, TaskError> {
        self.run_intent(project_id, intent, Persist::ViaHost)
    }

    /// 观察通道的桥版本（#15 接线）：与 `observe` 同一决策，字节交宿主存。
    pub fn observe_via_host(
        &self,
        project_id: &str,
        event: crate::tasks::executor::TaskEvent,
    ) -> Result<(EventOutcome, Applied), TaskError> {
        self.apply_observation(project_id, event, Persist::ViaHost)
    }

    fn run_intent(
        &self,
        project_id: &str,
        intent: &TaskIntent,
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        self.serial(|| match intent {
            TaskIntent::Rename {
                task_id,
                name,
                expect,
            } => self.run_rename(project_id, task_id, name, expect.as_ref(), persist),
            TaskIntent::ToggleStar { task_id } => {
                self.metadata_edit(project_id, task_id, persist, |records| {
                    reducer::toggle_star(records, task_id)
                })
            }
            TaskIntent::Archive { task_ids } => {
                let now_ms = self.clock().now_ms();
                self.metadata_batch(project_id, task_ids, persist, |records| {
                    reducer::archive(records, task_ids, now_ms)
                })
            }
            TaskIntent::Unarchive { task_ids } => {
                self.metadata_batch(project_id, task_ids, persist, |records| {
                    reducer::unarchive(records, task_ids)
                })
            }
            TaskIntent::AttachSession {
                task_id,
                family,
                session_agent,
                session_id,
                session_path,
            } => self.metadata_one(project_id, task_id, persist, |records| {
                reducer::attach_session(
                    records,
                    task_id,
                    *family,
                    session_agent,
                    session_id.as_deref(),
                    session_path.as_deref(),
                )
            }),
            TaskIntent::SetWorktree {
                task_id,
                path,
                branch,
                base_branch,
            } => self.metadata_one(project_id, task_id, persist, |records| {
                reducer::set_worktree(records, task_id, path, branch, base_branch)
            }),
            TaskIntent::DiscardWorktree { task_id } => {
                self.metadata_one(project_id, task_id, persist, |records| {
                    reducer::discard_worktree(records, task_id)
                })
            }
            TaskIntent::RecordDiffStats {
                task_id,
                additions,
                deletions,
            } => self.metadata_one(project_id, task_id, persist, |records| {
                reducer::record_diff_stats(records, task_id, *additions, *deletions)
            }),
            TaskIntent::Create {
                id,
                prompt,
                agent,
                permission_mode,
                immediate,
                selected_model,
                reasoning_effort,
                speed,
                dsh_agent_preset,
            } => {
                let spec = NewTask {
                    id: id.clone(),
                    project_id: project_id.to_string(),
                    prompt: prompt.clone(),
                    agent: agent.clone(),
                    permission_mode: permission_mode.clone(),
                    immediate: *immediate,
                    selected_model: selected_model.clone(),
                    reasoning_effort: reasoning_effort.clone(),
                    speed: speed.clone(),
                    dsh_agent_preset: dsh_agent_preset.clone(),
                    agent_config_id: None,
                    images: Vec::new(),
                    texts: Vec::new(),
                };
                self.insert_new_task(project_id, &spec, persist)
                    .map(|(_record, applied)| applied)
            }
            TaskIntent::Delete { task_ids } => self
                .delete_with(project_id, task_ids, persist)
                .map(|deleted| deleted.applied),
            TaskIntent::NormalizeAfterBoot { live_task_ids } => {
                self.normalize_after_boot_with(project_id, live_task_ids, persist)
            }
            TaskIntent::SwitchConfig {
                task_id,
                agent,
                selected_model,
                reasoning_effort,
                speed,
                permission_mode,
                previous_session,
                resolved_session,
            } => self.switch_config_write_with(
                project_id,
                task_id,
                agent,
                selected_model.as_deref(),
                reasoning_effort.as_deref(),
                speed.as_deref(),
                permission_mode,
                previous_session,
                resolved_session,
                persist,
            ),
            TaskIntent::ResumeWithSession {
                task_id,
                agent,
                family,
                session_id,
                session_path,
            } => self.resume_with_session_with(
                project_id,
                task_id,
                agent,
                *family,
                session_id.as_deref(),
                session_path.as_deref(),
                persist,
            ),
            TaskIntent::EditTodo {
                task_id,
                prompt,
                agent,
                permission_mode,
            } => self.metadata_one(project_id, task_id, persist, |records| {
                reducer::edit_todo(records, task_id, prompt, agent, permission_mode)
            }),
        })
    }

    /// 一条意图的收尾：`Commit` 按 ADR-015 的覆盖基线提交，`DecideOnly` 只把决定后的记录交回去。
    /// （不叫 `settle`：那个名字在本类型上已经是"了结一条账本 claim"，两者不是一回事。）
    fn finish_intent(
        &self,
        project_id: &str,
        records: Vec<Record>,
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        match persist {
            // 删空 ⇒ 删文件（ADR-037 后果 1）。只有把列表删到空的那条路径会走到
            // `commit_removal`——其余意图的结果天然非空，普通保存的空快照仍按 `[]` 落盘。
            Persist::Commit => {
                if records.is_empty() {
                    self.commit_removal(project_id)
                } else {
                    self.commit(project_id, &records)
                }
            }
            // 桥这一侧同样把生成戳记盖上再交出去：宿库存的是 core 决定的**最终字节**，
            // 与 Writer 路径同源；空数组 ⇒ 删文件由宿主的 write_task_records_raw 镜像。
            Persist::ViaHost => self.save_via_host(project_id, &records),
            Persist::DecideOnly => Ok(Applied {
                records,
                revision: None,
                changed: true,
                fingerprint: None,
                generation: None,
            }),
        }
    }

    /// begin() 用的落盘分派：launch 的写必须发生在 spawn 之前（Commit 或桥），
    /// `DecideOnly` 对执行类操作没有意义——"决定了但没人存"不能成为一个进程的开始。
    fn persist_launch(
        &self,
        project_id: &str,
        records: &[Record],
        persist: Persist,
    ) -> Result<Applied, TaskError> {
        match persist {
            Persist::Commit => self.commit(project_id, records),
            Persist::ViaHost => self.save_via_host(project_id, records),
            Persist::DecideOnly => Err(TaskError::Storage(StorageError::WriteFailed {
                path: self.context.paths.data_dir().to_path_buf(),
                reason: format!(
                    "a {project_id} launch cannot be decided without being stored; the executor would spawn over an unpersisted row"
                ),
            })),
        }
    }

    /// `Persist::ViaHost` 的落盘原语：core 决定字节，宿主保存。`revision: None` 是诚实
    /// ——`runtime.json` 的修订与覆盖面属于写者租约，这条路径不持有它；"宿主存好了"
    /// 与"我发布了一格修订"是两件事（§12 蓝图）。没有桥就是错误，绝不静默降级为不写。
    fn save_via_host(&self, project_id: &str, records: &[Record]) -> Result<Applied, TaskError> {
        let stamped = self.stamp_generation_copies(records);
        let saver = self.context.task_saver().ok_or_else(|| {
            TaskError::Storage(StorageError::WriteFailed {
                path: self.context.paths.data_dir().to_path_buf(),
                reason: format!(
                    "no host task saver is installed, so ViaHost cannot store the decision for {project_id}"
                ),
            })
        })?;
        let values: Vec<serde_json::Value> = stamped
            .iter()
            .map(|record| serde_json::Value::Object(record.0.clone()))
            .collect();
        saver
            .save_task_records(project_id, &values)
            .map_err(|reason| StorageError::WriteFailed {
                path: self.context.paths.data_dir().to_path_buf(),
                reason,
            })?;
        Ok(Applied {
            records: stamped,
            revision: None,
            changed: true,
            fingerprint: None,
            generation: None,
        })
    }

    /// 与 `commit` 对偶的"文件不该存在"那一支：覆盖面发布为缺席（`bump_covering(.., None)`），
    /// 宿主的转发保存（`write_task_records_raw`）对空数组本来就删文件——两条平台路径的
    /// 语义差别只能多不能少。不得反向复活：为"确保目录存在"而把删空的目录写回来就是破契约。
    fn commit_removal(&self, project_id: &str) -> Result<Applied, TaskError> {
        let (fingerprint, key) =
            task_storage::remove_covering(&self.context, project_id, Cover::LastCoreWrite)?;
        let bumped = runtime::bump_covering(&self.context, &[(key, fingerprint.clone())])?;
        self.ledger(|ledger| ledger.persist(&self.context))?;
        Ok(Applied {
            records: Vec::new(),
            revision: Some(bumped.metadata.revision),
            changed: true,
            generation: None,
            fingerprint: fingerprint
                .map(|value| serde_json::to_value(value).expect("a fingerprint is a value")),
        })
    }

    /// What a fresh process owes the user before it accepts any request: every start or
    /// resume the last process left unclaimed is now `interrupted`, and nothing is respawned
    /// on anybody's behalf.
    pub fn reconcile(&self, project_id: &str) -> Result<(Vec<String>, Option<Applied>), TaskError> {
        let snapshot = self.load(project_id)?;
        let claims = self.open_claims_all();
        let (records, cut_off) =
            crate::tasks::operations::reconcile(&claims, &snapshot.records, self.clock().now_ms());
        if cut_off.is_empty() {
            return Ok((
                Vec::new(),
                Some(self.unchanged(snapshot.records, snapshot.fingerprint)),
            ));
        }
        let applied = self.commit(project_id, &records)?;
        let reconciled: Vec<String> = cut_off.iter().map(|item| item.task_id.clone()).collect();
        self.ledger(|ledger| {
            for item in &cut_off {
                // The attempt is over; what remains is the user's turn to see it (ADR-018).
                ledger.settle_task(&item.task_id);
                ledger.await_acknowledgement(item.clone());
            }
            let _ = ledger.persist(&self.context);
        });
        Ok((reconciled, Some(applied)))
    }

    /// Which other runs of this task are still in flight. A terminal event from one of them
    /// does not make the task terminal (ADR-017).
    pub fn live_other_generations(&self, task_id: &str, except: u64) -> Vec<u64> {
        self.ledger(|ledger| ledger.live_other_than(task_id, except))
    }

    /// Does this run still have an attempt in flight?
    pub fn is_run_live(&self, task_id: &str, generation: u64) -> bool {
        self.ledger(|ledger| ledger.is_live(task_id, generation))
    }

    /// ADR-036: does this task have **any** run alive in this process? Two sources, unioned —
    /// a claim core itself took, or the host's answer through the port. Only the first covers
    /// a core-launched run, and only the second covers the legacy launch path; using either
    /// one alone silently mis-classifies the other. "Any", not "newest": ADR-025.
    pub fn is_task_live(&self, task_id: &str) -> bool {
        !self.open_claims(task_id).is_empty() || self.live_runs.has_live_run(task_id)
    }

    /// A run reconciliation cut off, waiting to be acknowledged.
    pub fn awaiting_acknowledgement(
        &self,
        task_id: &str,
    ) -> Option<crate::tasks::operations::Acknowledgement> {
        self.ledger(|ledger| ledger.awaiting_acknowledgement(task_id))
    }

    /// "The user has seen this." Returns whether anything was pending.
    pub fn acknowledge(&self, task_id: &str) -> Result<bool, StorageError> {
        let cleared = self.ledger(|ledger| {
            let cleared = ledger.acknowledge(task_id);
            if cleared {
                let _ = ledger.persist(&self.context);
            }
            cleared
        });
        Ok(cleared)
    }

    fn open_claims_all(&self) -> Vec<Claim> {
        self.ledger(|ledger| ledger.claims().to_vec())
    }

    /// The session family a host will have to run against, exposed so the remote-launch
    /// precondition ("this project is on a channel that cannot carry attachments") can be
    /// checked without duplicating the rule.
    pub fn session_owner_of(&self, record: &Record) -> SessionOwner {
        session_owner::resolve_session_owner(record, self.families.as_ref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelTargetInfo {
    pub target: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Deleted {
    pub removed: Vec<String>,
    pub skipped_starred: Vec<String>,
    pub applied: Applied,
}

/// What `start`/`resume` write before a process exists.
fn prepare_for_launch(record: &Record, kind: OperationKind) -> Record {
    let mut prepared = record.clone();
    prepared.0.insert(
        "status".into(),
        Value::String(
            serde_json::to_value(TaskStatus::Pending)
                .expect("a status is a string")
                .as_str()
                .expect("a status is a string")
                .to_string(),
        ),
    );
    prepared.0.remove("attentionRequestedAt");
    if kind == OperationKind::Resume {
        // `handleResumeTask` clears the reason a task stopped for: continuing is a claim
        // that the failure is no longer the last word about it.
        prepared.0.remove("failureReason");
    }
    prepared
}
