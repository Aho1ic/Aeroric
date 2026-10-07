//! 启动期降级记录（自 `src-tauri/src/startup_diagnostics.rs` 上收）。
//!
//! 启动链上有些失败（`~/.aeroric` 不可写、DBX 状态打不开）不该让窗口出现之前
//! panic：用户只会看到图标闪一下，没有日志也没有弹窗。降级启动把原因记在这里，
//! UI 起来后查一次并显示横幅。
//!
//! 与旧实现的两处差别，都是为了让宿主测试不再共享进程级可变状态：
//! 旧的是模块级 `OnceLock<Mutex<Vec<..>>>`，这里是普通结构体，由 [`crate::context::CoreContext`]
//! 的持有者决定生命周期；`eprintln!` 留给宿主（写日志是宿主的出口）。

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 一条降级记录。字段名按 camelCase 上线（旧前端读 `component`/`reason`/`fallback`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupDegradation {
    /// 组件标识，UI 据此选文案（如 `dbx-state`、`local-router`）。
    pub component: String,
    /// 原始失败原因，原样透出以便用户自查与报错。
    pub reason: String,
    /// 实际退到了哪儿（临时目录路径，或 `:memory:`）。
    pub fallback: String,
}

impl StartupDegradation {
    pub fn new(
        component: impl Into<String>,
        reason: impl Into<String>,
        fallback: impl Into<String>,
    ) -> Self {
        Self {
            component: component.into(),
            reason: reason.into(),
            fallback: fallback.into(),
        }
    }
}

/// 本次进程的降级账。`list` 返回快照，锁被毒化时退空表而不是 panic——
/// 启动期的账本没有资格让应用起不来。
#[derive(Debug, Default)]
pub struct StartupDegradationLog {
    records: Mutex<Vec<StartupDegradation>>,
}

impl StartupDegradationLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &self,
        component: impl Into<String>,
        reason: impl Into<String>,
        fallback: impl Into<String>,
    ) -> StartupDegradation {
        let entry = StartupDegradation::new(component, reason, fallback);
        if let Ok(mut guard) = self.records.lock() {
            guard.push(entry.clone());
        }
        entry
    }

    pub fn list(&self) -> Vec<StartupDegradation> {
        self.records
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.records
            .lock()
            .map(|guard| guard.is_empty())
            .unwrap_or(true)
    }
}
