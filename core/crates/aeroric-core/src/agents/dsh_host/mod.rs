//! DSH 结构化宿主（P3-05）：webui 宿主面按职责进 core。
//!
//! 第一片＝[`dispatcher`]（host 下行帧→事件名的纯映射面）。后续片按 Action 1 的
//! 名单逐个来（startup／client／state／approvals／questions／attachments／
//! projection）；executor 接线与 legacy 分支删除是最后一片，且**只删已替换的
//! 本地分支**——`legacy_task_executor.rs` 整体不归本 Task（Action 6 原文）。
//!
//! 归属边界与 sessions 同形：**纯判定与映射在 core，事件投递（emit）与宿主句柄地盘、
//! 落盘 I/O 留宿主**；10 条 `dsh-host-*` topic 的接线出口（`mapped` 或
//! `not-forwarded`）在本 Task 收（T1=A 改派，见 `event-topics-pending.md`）。

pub mod approvals;
pub mod attachments;
pub mod client;
pub mod dispatcher;
pub mod projection;
pub mod startup;
pub mod state;
