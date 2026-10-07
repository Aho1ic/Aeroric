//! domain 命令的转发面（Task P8-05 Action 1 的后半）。
//!
//! 形状只有一个函数：[`dispatch`]。它决定一条 domain 命令**由谁执行**：
//!
//! - `Embedded`（今天的产品默认）→ 走本进程的本地实现，行为与过去完全一致。
//! - `External`（已经有别的进程当宿主）→ **必须**经宿主；宿主说"没有这个方法"时
//!   **报错**，绝不悄悄回退成本地执行。
//!
//! 第二条是这一层存在的全部理由。回退看起来更"稳"，实际是把两个写者问题从"启动时
//! 一次"扩散成"每条命令一次"：宿主没实现的方法一旦本地跑，就同时存在两个写同一份
//! 数据的进程——正是 INV-03 要排除的形状，而且它不可观察（两边都不报错）。
//!
//! 调用方写法（迁移期）：
//!
//! ```ignore
//! dispatch(app, "files.read_dir", params, || local_read_dir(params)).await
//! ```
//!
//! `local` 是个闭包，所以**没被迁移的命令一行都不用改**——它们只是永远走 `Embedded`
//! 分支。覆盖率因此可以一条一条地涨，而每一步都有可数的判据。

use serde_json::Value;
use tauri::Manager;

use crate::core_adapter::{CoreGateway, CorePlacement};

/// 转发面拿不到宿主时的失败形状。刻意分成两类：**能力缺失**（宿主在，但没这个方法）
/// 与**传输失败**（宿主不可达/连接断了）。前者要的是协调升级，后者要的是等它回来——
/// 给用户的动作完全不同，所以不能合并成一句话。
#[derive(Debug, PartialEq, Eq)]
pub enum DispatchError {
    /// 宿主活着但不服务这个方法。
    NotServed { method: String, detail: String },
    /// 宿主这条路上出问题了（连接、握手、超时）。
    Transport { method: String, detail: String },
    /// 外置但没有可用连接（启动时没连上，或连接已丢）。
    NoSession { method: String },
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::NotServed { method, detail } => write!(
                formatter,
                "外部宿主不提供 {method}（{detail}）；需要**协调重启/升级**宿主，而不是在本进程里另跑一份"
            ),
            DispatchError::Transport { method, detail } => {
                write!(formatter, "与外部宿主的连接失败（{method}）：{detail}")
            }
            DispatchError::NoSession { method } => write!(
                formatter,
                "当前为 external 模式但没有可用的宿主连接（{method}）：本进程不自行执行，请等宿主可用"
            ),
        }
    }
}

impl std::error::Error for DispatchError {}

/// 宿主对"未知方法"的回答长什么样。
///
/// 判据要**窄**：只有确实表示"这个方法不存在"的码/文案才算 NotServed。把
/// `InvalidArgument`（参数写错）也归进来会让一条笔误变成"请升级宿主"。
pub fn looks_like_method_missing(code: &str, message: &str) -> bool {
    let lowered_message = message.to_ascii_lowercase();
    let lowered_code = code.to_ascii_lowercase();
    lowered_code.contains("unsupportedcapability")
        || lowered_message.contains("unknown method")
        || lowered_message.contains("method not found")
        || lowered_message.contains("no such method")
}

/// 判决 + 执行。`local` 只在 `Embedded` 时被求值（**External 下连 future 都不会被
/// poll**，这正是"不回退"的机器形状：本地实现没有机会跑）。
pub async fn dispatch<F>(
    app: &tauri::AppHandle<impl tauri::Runtime>,
    method: &str,
    params: Value,
    local: F,
) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    let gateway = app.try_state::<CoreGateway>().ok_or_else(|| {
        // 没有网关＝core 服务没装上：本地路径仍然可跑（旧客户端行为），但要说清
        // 这不是外置判决的结果。
        "core 网关未安装，无法判定 external/embedded".to_string()
    })?;
    route_for(gateway.inner(), method, params, local).await
}

/// 与 [`dispatch`] 同一逻辑，但不依赖 Tauri 的 `AppHandle`（用例与将来的 CLI 复用）。
pub async fn route_for<F>(
    gateway: &CoreGateway,
    method: &str,
    params: Value,
    local: F,
) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    if gateway.placement() == CorePlacement::Embedded {
        return local.await;
    }
    let outcome = forward(gateway, method, params).await;
    outcome.map_err(|error| error.to_string())
}

/// 只做转发的那一半（调用方已判定 external）。返回 `(route, value)` 便于用例断言。
pub async fn forward(
    gateway: &CoreGateway,
    method: &str,
    params: Value,
) -> Result<Value, DispatchError> {
    // 异步锁：请求-应答跨 await，`std::sync` 的 guard 会让 command future 非 Send。
    let mut guard = gateway.host().lock().await;
    let Some(session) = guard.as_mut() else {
        return Err(DispatchError::NoSession {
            method: method.to_string(),
        });
    };
    match session.client.request(method, params).await {
        Ok(value) => Ok(value),
        Err(error) => {
            let text = error.to_string();
            if looks_like_method_missing(&text, &text) {
                Err(DispatchError::NotServed {
                    method: method.to_string(),
                    detail: text,
                })
            } else {
                Err(DispatchError::Transport {
                    method: method.to_string(),
                    detail: text,
                })
            }
        }
    }
}
