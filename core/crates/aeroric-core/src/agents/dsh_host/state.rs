//! DSH 宿主的状态形状与 reducer（P3-05 第六片，Action 1 的 `state` 文件）。
//!
//! 只收**纯面**：`dsh web` 实例状态的三个 serde 形状（`WebUiStatus`／`DshWebUiState`／
//! `DshRuntimeRecovery`，字符串与字段是前端在网契约）与内部命令回显状态机
//! [`take_internal_command_echo`]——原宿主 manager 方法 `is_internal_command_echo`
//! 的函数体逐字搬来，`&Mutex<HashMap<..>>` 换成 `&mut HashMap<..>`，锁归宿主。
//!
//! 状态机的两条不对称规则是它存在的全部理由（注释原文随迁）：
//! - 第二次同名 `command/run`（`saw_run` 已置）**不吞**——放过去它就是普通回显；
//! - 没配对 run 先到的 `command/done` **不吞也不清登记**——吞了会把真正的引导
//!   命令回显整个漏进终端最前面两行。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebUiStatus {
    Starting,
    Running,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DshWebUiState {
    pub agent: String,
    pub port: u16,
    pub url: Option<String>,
    pub pid: Option<u32>,
    pub status: WebUiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DshRuntimeRecovery {
    pub restarted: bool,
    pub reconnected_sessions: usize,
    pub cancelled_turns: usize,
    pub errors: Vec<String>,
}

/// 一条等待被吞掉的内部命令回显。`command/run` 按命令名匹配,随后的
/// `command/done` 一并吞掉——任务生命周期锁保证这期间不会有用户命令插进来。
#[derive(Debug, Clone)]
pub struct InternalCommandEcho {
    pub name: String,
    pub saw_run: bool,
}

/// 该 `session/event` 帧是否**可能**是内部命令回显（`command/run`/`command/done`）。
/// 宿主方法在取锁**之前**就做这层筛——非 command 帧一次锁都不碰；迁移保这个时序，
/// 预筛函数与状态机分开暴露。[`take_internal_command_echo`] 内部会再筛一次（幂等）。
pub fn is_command_echo_event(payload: &Value) -> bool {
    let Some(event) = payload.get("event") else {
        return false;
    };
    matches!(
        event.get("type").and_then(Value::as_str),
        Some("command/run") | Some("command/done")
    )
}

/// `session/event` 是否是被登记的内部命令回显；是则执行对应转移。
///
/// 逐字保留宿主方法体的全部判定序：先 `event.type` 两臂筛形，再查该任务的登记，
/// `command/run` 要求「没见过 run 且名字配得上」才吞并置位；`command/done` 要求
/// 见过 run 才吞并**删除登记**（一次性生命周期）。
pub fn take_internal_command_echo(
    pending: &mut HashMap<String, InternalCommandEcho>,
    task_id: &str,
    payload: &Value,
) -> bool {
    let Some(event) = payload.get("event") else {
        return false;
    };
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(event_type, "command/run" | "command/done") {
        return false;
    }
    let Some(entry) = pending.get_mut(task_id) else {
        return false;
    };
    if event_type == "command/run" {
        let name = event
            .get("data")
            .and_then(|data| data.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if entry.saw_run || name != entry.name {
            return false;
        }
        entry.saw_run = true;
        return true;
    }
    // 命令的结果行紧跟在 run 之后,吞掉它就结束这条登记。反过来,还没见到
    // 配对的 run 就先到的 done 属于别的命令:吞掉它会连登记一起清掉,真正
    // 的引导命令回显反而会漏进终端。
    if !entry.saw_run {
        return false;
    }
    pending.remove(task_id);
    true
}
