//! P7-03 Action 1：端口（core 独占；ListeningPort 形状与 PID 身份判定
//! ——与 `src-tauri/src/ports.rs` 同源语义 verbatim 上收）。
use serde::Serialize;

/// 监听端口（与 Tauri `ListeningPort` 字段对齐）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    pub port: u16,
    pub address: String,
    pub protocol: String,
    pub pid: u32,
    pub process_name: String,
    pub url: String,
}

/// 端口释放前 PID 身份判定（Action：避免 PID 重用误杀——stop 前比对进程名，
/// 不匹配即拒绝）。
pub fn pid_matches_process(
    pid: u32,
    expected_process_name: &str,
    actual_process_name: &str,
) -> bool {
    let expected = expected_process_name.to_lowercase();
    let actual = actual_process_name.to_lowercase();
    let _ = pid;
    // 完全匹配或 expected 是 actual 的后缀（路径形式）。
    actual == expected || actual.ends_with(&format!("/{expected}"))
}

/// stop 判定（Action 3：port owner/stop 显式区分——stop 是破坏操作，需
/// PID 身份确认；owner 查询是只读）。
pub fn can_stop_port(owner_confirmed: bool, pid_identity_matches: bool) -> Result<(), String> {
    if !pid_identity_matches {
        return Err("PID identity mismatch (possible PID reuse); refusing to stop".to_string());
    }
    if !owner_confirmed {
        return Err("port stop requires explicit owner confirmation".to_string());
    }
    Ok(())
}
