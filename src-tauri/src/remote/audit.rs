//! 远程访问审计日志:连接 / 认证成败 / 配对 / 撤销等安全事件,
//! JSON Lines 追加写入 `~/.aeroric/remote-audit.log`(超过 1MB 轮转为 .1)。
//!
//! 只记录事实不做判断;失败静默(审计不可用不应影响业务链路)。
//! 集成测试用内存态 RemoteState(audit_enabled=false),不会写真实文件。

use serde_json::{json, Value};

use crate::storage::aeroric_dir;

const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// 追加一条审计事件。`detail` 为对象时其字段平铺进日志行。
pub fn log(event: &str, detail: Value) {
    let Ok(dir) = aeroric_dir() else { return };
    let path = dir.join("remote-audit.log");
    // 轮转:超限后 rename 为 .1(覆盖旧 .1),当前文件重新开始
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let _ = std::fs::rename(&path, dir.join("remote-audit.log.1"));
        }
    }
    let mut line = json!({
        "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "event": event,
    });
    if let (Some(obj), Value::Object(extra)) = (line.as_object_mut(), detail) {
        for (key, value) in extra {
            obj.insert(key, value);
        }
    }
    use std::io::Write;
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let _ = writeln!(file, "{line}");
}
