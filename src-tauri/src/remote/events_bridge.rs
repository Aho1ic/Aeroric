//! 桌面事件 → 远程客户端推送桥。
//!
//! Tauri 2 的 `Emitter::emit` 同时投递给 webview 与 Rust 侧 `listen_any`
//! 监听器,因此无需改动任何现有 emit 点:服务启动时挂监听、停止时卸载,
//! 把白名单事件原样转发给全部已认证连接。

use serde_json::Value;
use tauri::{AppHandle, Listener, Manager, Runtime};

use super::RemoteState;

/// 转发给手机的桌面事件白名单。M2/M3 按需扩充(终端流走独立二进制通道,不在此列)。
const FORWARDED_EVENTS: &[&str] = &["task-status", "task-session"];

pub fn attach<R: Runtime>(app: &AppHandle<R>) -> Vec<tauri::EventId> {
    FORWARDED_EVENTS
        .iter()
        .map(|&event_name| {
            let clients = app.state::<RemoteState>().clients.clone();
            let approvals = app.state::<RemoteState>().approvals.clone();
            let event_log = app.state::<RemoteState>().event_log.clone();
            app.listen_any(event_name, move |event| {
                let Ok(data) = serde_json::from_str::<Value>(event.payload()) else {
                    return;
                };
                if event_name == "task-status" {
                    let task_id = data.get("task_id").and_then(Value::as_str);
                    let status = data.get("status").and_then(Value::as_str);
                    if let Some(task_id) = task_id {
                        if status != Some("input_required") || data.get("approval").is_none() {
                            approvals.clear(task_id);
                        }
                    }
                }
                // 先入事件日志(重连 watermark 补发源),推送 envelope 携带 seq
                let seq = event_log.append(event_name, data.clone());
                clients.broadcast_push(event_name, Some(seq), &data);
            })
        })
        .collect()
}
