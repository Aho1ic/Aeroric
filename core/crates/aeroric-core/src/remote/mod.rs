//! 手机远程连接（Mobile Remote）的**纯逻辑半边**（P8-03 刀一自宿主 `remote/` 上收）。
//!
//! 本模块只收**不依赖 Tauri 句柄**的部分：协议类型与 RPC 版本协商
//! （`protocol`）、E2EE 握手与会话密码（`crypto`/`orca_crypto`）、终端帧编码
//! （`terminal_frames`）与 broadcast hub（`terminal_hub`）、事件环形缓冲
//! （`event_log`）、键位映射（`agent_keymap`）、设备注册表与配对/邀请
//! （`auth`）。落盘原语走 core `settings`（`aeroric_dir` 的读序与宿主同：
//! DATA_DIR → `AERORIC_HOME` → 真实家；宿主测试用 scratch 根隔离）。
//!
//! 分界：WS 服务器循环、Tauri 事件桥（listen_any）、10 条 Tauri 命令、
//! React 中转（task.create/resume 的 forward_task_request）与审计落点留宿主
//! `remote/`（刀二起 RPC dispatch 与状态注册表上收，宿主只留命令壳与桥）。

pub mod agent_keymap;
pub mod auth;
pub mod crypto;
pub mod event_log;
pub mod orca_crypto;
pub mod protocol;
pub mod terminal_frames;
pub mod terminal_hub;
