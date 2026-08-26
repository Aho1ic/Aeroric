//! 启动期降级记录。
//!
//! 背景:`run()` 里原先有三处 `.expect()`——DBX 状态、local router 状态、
//! `Builder::build()`。前两处的失败原因几乎总是同一个(`~/.aeroric` 不可写),
//! 而 `.expect()` 会在窗口出现之前 panic:用户看到图标闪一下就没了,没有日志、
//! 没有弹窗,也无法把问题报给我们。
//!
//! 现在前两处改成降级启动(退临时目录,再退内存库),把原因记在这里,前端启动后
//! 查一次并弹告警横幅。第三处没有降级空间(应用壳都没建起来),改成原生弹窗 +
//! stderr,至少让失败可见。

use std::sync::{Mutex, OnceLock};

use serde::Serialize;

/// 一条降级记录。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupDegradation {
    /// 组件标识,前端据此选文案(如 `dbx-state`、`local-router`)。
    pub component: String,
    /// 原始失败原因,原样透出以便用户自查与报错。
    pub reason: String,
    /// 实际退到了哪儿(临时目录路径,或 `:memory:`)。
    pub fallback: String,
}

fn records() -> &'static Mutex<Vec<StartupDegradation>> {
    static RECORDS: OnceLock<Mutex<Vec<StartupDegradation>>> = OnceLock::new();
    RECORDS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 记一条降级。启动期调用,失败不影响主流程。
pub(crate) fn record(component: &str, reason: impl Into<String>, fallback: impl Into<String>) {
    let entry = StartupDegradation {
        component: component.to_string(),
        reason: reason.into(),
        fallback: fallback.into(),
    };
    // 同时打到 stderr:前端还没起来时崩了,日志里也留得下线索。
    eprintln!(
        "[aeroric] startup degraded: {} — {} (fallback: {})",
        entry.component, entry.reason, entry.fallback
    );
    if let Ok(mut guard) = records().lock() {
        guard.push(entry);
    }
}

/// 前端启动后查一次;空数组表示一切正常。
#[tauri::command]
pub fn list_startup_degradations() -> Vec<StartupDegradation> {
    records()
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_accumulate_and_are_readable() {
        record("test-component", "disk full", "/tmp/x");
        let all = list_startup_degradations();
        assert!(all.iter().any(|entry| entry.component == "test-component"
            && entry.reason == "disk full"
            && entry.fallback == "/tmp/x"));
    }

    /// 序列化字段名必须是 camelCase,否则前端读到 undefined 而不报错——
    /// 那种失败最难查,所以钉死。
    #[test]
    fn serializes_as_camel_case() {
        let json = serde_json::to_string(&StartupDegradation {
            component: "dbx-state".into(),
            reason: "boom".into(),
            fallback: ":memory:".into(),
        })
        .expect("serialize");
        assert!(json.contains("\"component\""), "{json}");
        assert!(json.contains("\"fallback\""), "{json}");
    }
}
