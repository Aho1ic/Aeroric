//! 宿主服务注入面（P8-03 刀三b）。
//!
//! core 的 server/rpc 只认本文件的两个 trait：WS 服务循环、配对状态机、
//! RPC 调度与校验都在 core；PTY 写入、存储读写、git/files、agent 配置、
//! 桌面事件 emit 与审计落点是宿主能力，经 [`RemoteHostHandle`] 注入。
//! headless 测试与将来的 aeroric-cli serve 用各自的实现（fake / core service
//! 直连），桌面端由 `RemoteHostBridge`（宿主句柄包装）实现一次。

use async_trait::async_trait;

use serde_json::Value;

use crate::remote::state::OutboundSender;

/// 单连接的终端帧处理器（宿主 `TerminalStreams` 的 core 视角）。
#[async_trait]
pub trait RemoteTerminalStreams: Send {
    /// 连接收到 Binary 消息时调用;非终端帧静默忽略。
    fn handle_frame(&mut self, bytes: &[u8]);
    /// 连接退出时停掉全部转发任务并归还 PTY 尺寸租约。
    fn abort_all(&mut self);
}

#[async_trait]
pub trait RemoteHostHandle: Send + Sync {
    // ── 桌面壳信息 ──
    /// `hello`/auth 回包里的 version 字段（宿主 package_info）。
    fn desktop_version(&self) -> String;

    // ── 事件与审计（宿主落点；G19：audit 落点留宿主经注入隔离） ──
    /// 桌面事件总线（remote-device-paired / project-pinned-changed 等）。
    fn emit(&self, event: &str, payload: Value);
    /// 审计日志（宿主 remote-audit.log；调用方负责 `audit_enabled` 门控）。
    fn audit_log(&self, event: &str, payload: Value);

    // ── 存储面（宿主 storage） ──
    /// `projects.list`：projects.json 的序列化快照。
    async fn load_projects(&self) -> Result<Value, String>;
    /// `projects.setPinned`：只改 pinned 字段。
    async fn set_project_pinned(&self, project_id: &str, pinned: bool) -> Result<(), String>;
    /// `tasks.list`/`tasks.get` 的任务面（宿主整表读）。
    async fn load_project_tasks(&self, project_id: &str) -> Result<Value, String>;

    // ── 任务执行面（宿主 TaskManager/pty/core_adapter） ──
    /// `task.input`：bracketed-paste 包裹后写入运行中的 PTY。
    async fn task_input(&self, task_id: &str, text: &str) -> Result<Value, String>;
    /// `task.respond`：审批按键序列（宿主持有 TaskManager writer；action/keys
    /// 解析与审批注册表核对由实现方完成）。
    async fn task_respond(&self, task_id: &str, params: Value) -> Result<Value, String>;
    /// `task.cancel`（SSH 任务宿主侧拒绝语义随宿主实现）。
    async fn task_cancel(&self, project_id: &str, task_id: &str) -> Result<Value, String>;
    /// `task.complete`（按会话族复用桌面完成内核）。
    async fn task_complete(&self, project_id: &str, task_id: &str) -> Result<Value, String>;
    /// `task.create`/`task.resume` 的原生执行器（刀二：直连 core TaskService
    /// ＋launch/resume_via_core；ready 闸与 per-project 串行队列由实现方持有）。
    async fn task_request(&self, payload: Value) -> Result<Value, String>;

    // ── 会话/文件/git/agent 配置（宿主） ──
    async fn session_messages(&self, params: Value) -> Result<Value, String>;
    async fn project_files(&self, params: Value) -> Result<Value, String>;
    async fn project_read_file(&self, params: Value) -> Result<Value, String>;
    async fn project_write_file(&self, params: Value) -> Result<Value, String>;
    async fn git_changes(&self, params: Value) -> Result<Value, String>;
    async fn git_diff(&self, params: Value) -> Result<Value, String>;
    async fn agent_config_list(&self) -> Result<Value, String>;
    async fn agent_config_save(&self, params: Value) -> Result<Value, String>;
    async fn agent_config_detect_models(&self, params: Value) -> Result<Value, String>;
    async fn agent_config_create(&self, params: Value) -> Result<Value, String>;
    async fn agents_list(&self) -> Result<Value, String>;
    async fn agents_models(&self, agent: &str) -> Result<Value, String>;
    async fn stats_summary(&self) -> Result<Value, String>;

    // ── 终端流 ──
    fn terminal_streams(&self, outbound: OutboundSender) -> Box<dyn RemoteTerminalStreams>;
}
