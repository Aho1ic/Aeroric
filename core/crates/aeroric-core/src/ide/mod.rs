//! P6-03：IDE 服务模块（core 独占；LSP/DAP process 由宿主管理，本模块承载
//! 协议形状、parser 子模块与纯转换——offset encoding/位置协议统一在这里，
//! 不让 Swift 各组件自行换算）。
pub mod dap;
pub mod diagnostics;
pub mod diagnostics_face;
pub mod lsp;
pub mod lsp_face;
pub mod lsp_session;
pub mod run_config;
pub mod run_face;
pub mod tests_face;

/// P6-03 Action 1：projectTarget（run/debug/preview 用 projectTarget 而非仅
/// 本地 path——远程/WSL 目标形态）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTarget {
    /// 本地路径（local 目标）或远程路径（ssh/wsl 目标）。
    pub path: String,
    /// 目标形态：local / ssh:<connection_id> / wsl。
    pub flavor: String,
}

impl ProjectTarget {
    pub fn local(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            flavor: "local".to_string(),
        }
    }
}

/// P6-03 Action 1：test_runner 判定（test failure 定位——输出行解析为
/// (file, line) 对）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFailureLocation {
    pub file: String,
    pub line: u32,
}

/// cargo test 失败定位（输出行 → file:line；纯函数）。
pub fn parse_cargo_test_failure_line(line: &str) -> Option<TestFailureLocation> {
    // 形如 "  --> src/main.rs:42:5" 或 "at src/main.rs:42:5"。
    let marker = line
        .trim()
        .trim_start_matches("-->")
        .trim()
        .trim_start_matches("at ");
    let mut parts = marker.rsplitn(3, ':');
    let _col = parts.next()?;
    let line_number: u32 = parts.next()?.parse().ok()?;
    let file = parts.next()?.to_string();
    Some(TestFailureLocation {
        file,
        line: line_number,
    })
}

/// P6-03 Action 1/5：web preview 判定（只允许显式批准的本地/项目 URL——
/// 外部 URL 交系统浏览器；纯决策）。
pub fn is_approved_preview_url(url: &str, approved_hosts: &[String]) -> bool {
    // 只认 http/localhost/项目 host；javascript:/data:/file: 一律拒绝。
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("data:") || lower.starts_with("file:")
    {
        return false;
    }
    let Some(scheme_end) = lower.find("://") else {
        return false;
    };
    let scheme = &lower[..scheme_end];
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let rest = &lower[scheme_end + 3..];
    let host = rest.split(['/']).next().unwrap_or("");
    let host_only = host.split(':').next().unwrap_or("");
    host_only == "localhost"
        || host_only == "127.0.0.1"
        || approved_hosts
            .iter()
            .any(|approved| approved.eq_ignore_ascii_case(host_only))
}
