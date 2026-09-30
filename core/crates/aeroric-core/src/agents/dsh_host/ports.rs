//! `DshHostPorts` —— dsh 启动编排上收 core 时的宿主端口簇（P3-05 kernel 片，
//! grill [dsh] **D2＝端口簇**，补记二百六十四）。
//!
//! 形状规则（与 ADR-036 `LiveRuns` 同模板：core 问、host 答，端口实现不得回调
//! core）：
//! - **零 async_trait**：core 无 async-trait 依赖，两个天生异步的端口
//!   （[`DshHostPorts::ensure_webui`]、[`DshHostPorts::start_stream`]）以
//!   [`BoxFuture`] 返回保持对象安全；其余端口全同步。
//! - **宿主 IPC channel（`ipc::Channel`）不进 core**：输出通道经 [`OutputSink`] 擦除，
//!   宿主在 thin 调用点用 `Channel::send` 包一层。活跃会话表
//!   （`DshWebUiManager::active_sessions`）仍归宿主——core 只把 sink 交回去。
//! - **进程与文件地盘不伪装成纯逻辑**：dsh-web 进程的 ensure（端口 4）与
//!   终端历史截断（端口 3）是宿主 I/O，端口化而非上收。
//!
//! 逐端口出处＝宿主 `run_dsh_task_core`（P3-05 kernel 片上收前的行位）对应段；
//! 编排顺序的保真由 executor.rs 的 verbatim 迁移保证，本文件只定义接触面。

use std::sync::Arc;

pub use futures_util::future::BoxFuture;

/// 终端输出通道擦除：宿主把 `Channel<String>::send` 包成闭包。
/// 语义＝「尽力投递」：Channel 已关闭时静默丢弃（与宿主
/// `send_terminal_text_for_task` 对 `send` 返回值的处理一致）。
pub type OutputSink = Arc<dyn Fn(String) + Send + Sync>;

/// [`DshHostPorts::register_session`] 的事实包：core 解析出 session 路径后，
/// 登记归宿主（`session_dsh::register_dsh_session_with_preset` 带 app handle
/// 与 emit，属宿主地盘）。generation 是 `RunKey` 的 core 侧可表达部分。
#[derive(Debug, Clone)]
pub struct DshSessionRegistration {
    pub task_id: String,
    pub generation: u64,
    pub session_id: String,
    pub session_path: String,
    pub resolved_preset: Option<String>,
}

/// [`DshHostPorts::start_stream`] 的请求：宿主用 base_url 重建
/// `DshApiClient` 并开 `session/follow`＋`session/control` 双流消费环
/// （`consume_task_session_stream` 留在宿主——它与 terminal projection/审批
/// emit 的耦合面在 P3-05 后续片处理，kernel 片不拆）。
#[derive(Clone)]
pub struct DshStreamRequest {
    pub task_id: String,
    pub session_id: String,
    pub base_url: String,
    pub sink: OutputSink,
}

// `OutputSink` 是闭包，Debug 只能标注存在性（与宿主 Channel 的调试形状一致）。
impl std::fmt::Debug for DshStreamRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DshStreamRequest")
            .field("task_id", &self.task_id)
            .field("session_id", &self.session_id)
            .field("base_url", &self.base_url)
            .field("sink", &"<OutputSink>")
            .finish()
    }
}

/// dsh 启动编排需要宿主回答的全部问题。方法名与宿主原调用点一一对应，
/// 参数表刻意保持「宿主段删剩什么就传什么」，不做提前抽象。
pub trait DshHostPorts: Send + Sync {
    /// 宿主 `DshWebUiManager::task_lifecycle_lock`：同 task 的 run/prompt/
    /// cancel 互斥。返回 Arc 让 core 持 guard 跨 `.await`。
    fn lifecycle_lock(&self, task_id: &str) -> Arc<tokio::sync::Mutex<()>>;

    /// 开跑前的任务级清账（宿主五连：`clear_completed_task`、
    /// `cancelled_tasks.remove`、`reasoning_folds.remove`、`clear_terminal_wrap`、
    /// `clear_internal_command`）——同一事务语义，合成一个端口。
    fn clear_prerun_state(&self, task_id: &str);

    /// 全新会话清终端历史（宿主 `storage::truncate_task_terminal_history` +
    /// `terminal_hub::reset_for_truncate`）。恢复会话不调用（判定留在
    /// executor 的 `session_id.is_none()` 分支，与原文一致）。
    fn truncate_history_for_new_session(&self, task_id: &str);

    /// 确保该 agent 的 dsh-web 进程活着并返回 base_url（宿主
    /// `ensure_dsh_webui`：进程 spawn/端口占用/config 写，全宿主地盘）。
    /// Err 文案逐字＝宿主 `ensure_dsh_webui` 的错误链 + "DSH Web URL is
    /// unavailable"。
    fn ensure_webui(&self, agent: &str) -> BoxFuture<'_, Result<String, String>>;

    /// 宿主 `TaskManager::runs.register`：ADR-034 存活判定的账在宿主。
    fn register_run(&self, task_id: &str, generation: u64);

    /// 宿主 `active_sessions.insert`：sink 由宿主存进 `ActiveDshSession`
    /// （字段类型从 `Channel<String>` 改为本端口的 [`OutputSink`]，kernel 片
    /// 的宿主侧机械项）。
    fn register_active_session(
        &self,
        task_id: &str,
        session_id: &str,
        base_url: &str,
        sink: OutputSink,
    );

    /// 宿主 `remember_session_host`：任务表清空后会话详情找回实例。
    fn remember_session_host(&self, session_id: &str, base_url: &str);

    /// 宿主 `session_dsh::dsh_session_path_for`：路径解析读宿主发现面
    /// （home/项目目录映射），返回 None 时 executor 报
    /// "Could not resolve the DSH session log path"。
    fn session_path(&self, agent: &str, project_path: &str, session_id: &str) -> Option<String>;

    /// 宿主 `register_dsh_session_with_preset`（带 app handle/emit 的登记面）。
    fn register_session(&self, facts: &DshSessionRegistration);

    /// 宿主 `sync_terminal_cols`：流开始前把当前终端列数推给 dsh-web。
    fn sync_terminal_cols(&self, task_id: &str);

    /// 宿主 `start_task_session_stream`：双流消费环启动（含 events_open
    /// oneshot 握手），Err＝流打开失败。
    fn start_stream(&self, request: DshStreamRequest) -> BoxFuture<'_, Result<(), String>>;

    /// 宿主 `expect_internal_command`：`/permission` 结果行抑制登记。
    fn expect_internal_command(&self, task_id: &str, kind: &str);

    /// 宿主 `clear_internal_command`：权限命令不可用时撤销抑制。
    fn clear_internal_command(&self, task_id: &str);

    /// 宿主 `send_terminal_text_for_task`：echo 行投递（带 task 存活校验的
    /// 语义在宿主 impl 内，端口只转发）。
    fn send_terminal(&self, task_id: &str, sink: &OutputSink, text: &str);

    /// 宿主 `core_adapter::report_task_status[_with_reason]`：状态上报的
    /// reason 为 None 时走无 reason 版本（两函数在宿主本就同链）。
    fn report_status(&self, task_id: &str, status: &str, reason: Option<&str>);

    /// 宿主 `retire_dsh_runs`：失败路径的任务级结算（ADR-025 规则 2）。
    fn retire_runs(&self, task_id: &str);
}
