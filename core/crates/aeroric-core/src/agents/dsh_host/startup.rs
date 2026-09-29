//! `dsh web` 启动行的判定与输出缓冲（P3-05 第五片，Action 1 的 `startup` 文件的纯面）。
//!
//! 逐字迁自宿主 `src-tauri/src/dsh_webui/startup.rs`：启动 URL 的**信任闸门**
//! （只收 `dsh web:` 前缀行、必须 http＋loopback＋根路径＋无 userinfo/query/fragment
//! ＋实端口，`url` crate 与宿主同版本）、输出环的 16 KiB 截断（UTF-8 边界安全：按
//! `char_indices` 丢弃、不切半字符），以及把 stderr/stdout 拼进错误文案的三个函数。
//! 进程生命周期（spawn/抽干/健康检查/lifecycle 锁排队）是 async I/O，全部留宿主——
//! 本文件是它们消费的判定与数据面。

use parking_lot::Mutex;
use std::process::ExitStatus;
use std::sync::Arc;
use url::{Host, Url};

/// 输出环上限：只保最近 16 KiB，长堆栈也不会撑爆错误文案。
pub const DSH_WEB_OUTPUT_LIMIT: usize = 16 * 1024;

/// 传给 `dsh` 的子命令参数(在用户配置的 `launch.args` 之后)。
///
/// `--no-open`:我们只把 `dsh web` 当 headless RPC 后端,会话跑在 Aeroric 自己的
/// 终端里。上游 web-app 的 `openBrowser` 默认为 true,不带这个 flag 每次拉起后端
/// 都会顺带弹一个系统浏览器 —— 看起来就像"启动终端跳去了网页版"。
/// `printUrl` 在 web-app 的 cordis.patch.yml 里硬编码 true、与 `openBrowser` 相互
/// 独立,所以这里照常能读到启动 URL 那行。
pub fn dsh_web_command_args() -> [&'static str; 4] {
    ["web", "--port", "0", "--no-open"]
}

#[derive(Default)]
pub struct DshWebStartupOutput {
    pub stdout: String,
    pub stderr: String,
}

pub fn append_bounded_output(target: &mut String, line: &str) {
    target.push_str(line);
    target.push('\n');
    if target.len() <= DSH_WEB_OUTPUT_LIMIT {
        return;
    }
    let remove_at_least = target.len() - DSH_WEB_OUTPUT_LIMIT;
    let split = target
        .char_indices()
        .find_map(|(index, _)| (index >= remove_at_least).then_some(index))
        .unwrap_or(target.len());
    target.drain(..split);
}

pub fn startup_output_detail(output: &Arc<Mutex<DshWebStartupOutput>>) -> String {
    let output = output.lock();
    let mut sections = Vec::new();
    if !output.stderr.trim().is_empty() {
        sections.push(format!("stderr:\n{}", output.stderr.trim()));
    }
    if !output.stdout.trim().is_empty() {
        sections.push(format!("stdout:\n{}", output.stdout.trim()));
    }
    sections.join("\n")
}

pub fn attach_startup_output(error: String, output: &Arc<Mutex<DshWebStartupOutput>>) -> String {
    let detail = startup_output_detail(output);
    if detail.is_empty() || error.contains(&detail) {
        error
    } else {
        format!("{error}\n{detail}")
    }
}

pub fn exited_dsh_web_error(
    status: ExitStatus,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> String {
    let detail = startup_output_detail(output);
    if detail.is_empty() {
        format!("DSH Web exited before becoming ready ({status})")
    } else {
        format!("DSH Web exited before becoming ready ({status})\n{detail}")
    }
}

/// 一行 stdout → 启动 URL（`(base_url, port)`）。
///
/// 非前缀行返回 `Ok(None)`（继续抽干）；前缀行一旦识别，任何不满足闸门的地方都
/// **响亮拒绝**（`Err`）而不是回退默认值——这条 URL 接下来是全部 RPC 与 WS 的
/// base，放行非 loopback／带路径／带凭据的形状等于把会话数据交给任意端点。
pub fn parse_dsh_web_startup_url(line: &str) -> Result<Option<(String, u16)>, String> {
    let Some(value) = line.trim().strip_prefix("dsh web:").map(str::trim) else {
        return Ok(None);
    };
    let parsed = Url::parse(value)
        .map_err(|error| format!("DSH Web reported an invalid startup URL {value:?}: {error}"))?;
    if parsed.scheme() != "http" {
        return Err(format!("DSH Web reported a non-HTTP startup URL: {value}"));
    }
    let host = parsed
        .host()
        .ok_or_else(|| format!("DSH Web startup URL has no host: {value}"))?;
    let loopback = match &host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => std::net::IpAddr::V4(*address).is_loopback(),
        Host::Ipv6(address) => std::net::IpAddr::V6(*address).is_loopback(),
    };
    if !loopback {
        return Err(format!(
            "DSH Web reported a non-loopback startup URL: {value}"
        ));
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(format!(
            "DSH Web reported an unsupported startup URL shape: {value}"
        ));
    }
    let port = parsed
        .port()
        .ok_or_else(|| format!("DSH Web startup URL has no port: {value}"))?;
    if port == 0 {
        return Err("DSH Web reported port 0 instead of its allocated port".to_string());
    }
    let base_url = match host {
        Host::Domain(domain) => format!("http://{domain}:{port}"),
        Host::Ipv4(address) => format!("http://{address}:{port}"),
        Host::Ipv6(address) => format!("http://[{address}]:{port}"),
    };
    Ok(Some((base_url, port)))
}
