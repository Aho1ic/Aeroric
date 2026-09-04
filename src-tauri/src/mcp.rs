//! MCP(Model Context Protocol)服务器配置的加载、保存与连通性测试。
//!
//! 配置持久化在 `~/.aeroric/mcp.json`。服务器 env 常承载 API key / token,
//! 因此统一用 `atomic_write_private`(0o600)落盘,与数据库/SSH 凭据一致。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

use crate::storage::{aeroric_dir, atomic_write_private, ensure_private_dir};

const MCP_CONFIG_FILE: &str = "mcp.json";
const MCP_TEST_TIMEOUT_SECS: u64 = 10;
/// stdout 已 EOF 后,继续收集 stderr 诊断信息的最长等待时间。
const MCP_STDERR_DRAIN_MS: u64 = 500;
/// 与 MCP stdio 服务器握手时声明的协议版本。
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
/// 测试失败时回传的 stderr 行数上限,避免把整屏日志塞进 UI。
const MAX_STDERR_LINES: usize = 20;

/// 单个 MCP 服务器定义。`name` 与 `McpSettings::servers` 的 key 重复,
/// 保存时以 key 为准回填,避免前端两处不一致时产生歧义。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct McpServerConfig {
    #[serde(default)]
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 服务器按名称建 map,与 MCP 生态(claude_desktop_config.json 等)的惯例一致。
/// 用 BTreeMap 而非 HashMap:落盘顺序稳定,避免每次保存都重排文件内容。
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct McpSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

// ── Persistence ─────────────────────────────────────────────────────────────

fn mcp_config_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join(MCP_CONFIG_FILE))
}

/// 缺失文件视为「尚未配置」,返回默认值而非报错(首次启动的正常路径)。
fn read_settings_from(path: &Path) -> Result<McpSettings, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(McpSettings::default()),
        Err(err) => return Err(format!("无法读取 MCP 配置: {err}")),
    };
    if content.trim().is_empty() {
        return Ok(McpSettings::default());
    }
    serde_json::from_str(&content).map_err(|err| format!("MCP 配置格式错误: {err}"))
}

fn write_settings_to(path: &Path, settings: &McpSettings) -> Result<(), String> {
    let content = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("无法序列化 MCP 配置: {err}"))?;
    atomic_write_private(path, &content).map_err(|err| format!("无法写入 MCP 配置: {err}"))
}

pub fn load_mcp_settings() -> Result<McpSettings, String> {
    read_settings_from(&mcp_config_path()?)
}

pub fn save_mcp_settings(settings: &McpSettings) -> Result<(), String> {
    let normalized = normalize_settings(settings)?;
    let path = mcp_config_path()?;
    ensure_private_dir(&aeroric_dir()?)?;
    write_settings_to(&path, &normalized)
}

/// 去空白并校验:key(服务器名)与命令非空。
/// key 去空白后可能相撞(如 "a" 与 " a "),此时报错而不是静默丢弃一个。
fn normalize_settings(settings: &McpSettings) -> Result<McpSettings, String> {
    let mut servers = BTreeMap::new();
    for (key, server) in &settings.servers {
        let name = key.trim().to_string();
        let command = server.command.trim().to_string();
        if name.is_empty() {
            return Err("MCP 服务器名称不能为空".to_string());
        }
        if command.is_empty() {
            return Err(format!("MCP 服务器 \"{name}\" 的命令不能为空"));
        }
        let entry = McpServerConfig {
            // name 以 map key 为准,消除 key 与内嵌 name 不一致的歧义。
            name: name.clone(),
            command,
            args: server.args.clone(),
            env: server.env.clone(),
            enabled: server.enabled,
        };
        if servers.insert(name.clone(), entry).is_some() {
            return Err(format!("MCP 服务器名称重复: \"{name}\""));
        }
    }
    Ok(McpSettings {
        enabled: settings.enabled,
        servers,
    })
}

// ── Connectivity test ───────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum McpTestResult {
    Success {
        message: String,
        #[serde(rename = "serverName", skip_serializing_if = "Option::is_none")]
        server_name: Option<String>,
        #[serde(rename = "serverVersion", skip_serializing_if = "Option::is_none")]
        server_version: Option<String>,
        #[serde(rename = "protocolVersion", skip_serializing_if = "Option::is_none")]
        protocol_version: Option<String>,
    },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        stderr: Option<String>,
    },
    Timeout {
        message: String,
    },
}

fn collected_stderr(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n"))
}

fn push_stderr_line(lines: &mut Vec<String>, line: String) {
    if lines.len() < MAX_STDERR_LINES {
        lines.push(line);
    }
}

fn initialize_request() -> String {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "aeroric", "version": env!("CARGO_PKG_VERSION") },
        }
    });
    format!("{request}\n")
}

/// 解析一行 stdout。返回 `Some` 表示这是 initialize 的终局响应。
/// 服务器启动横幅等非 JSON / 无关消息返回 `None` 继续读。
fn interpret_stdout_line(line: &str) -> Option<McpTestResult> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;

    // 必须匹配 jsonrpc: "2.0" 与 id: 1
    if value.get("jsonrpc").and_then(serde_json::Value::as_str) != Some("2.0") {
        return None;
    }
    if value.get("id").and_then(serde_json::Value::as_u64) != Some(1) {
        return None;
    }

    if let Some(error) = value.get("error") {
        let detail = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("未知错误");
        return Some(McpTestResult::Error {
            message: format!("服务器拒绝初始化请求: {detail}"),
            stderr: None,
        });
    }

    let result = value.get("result").and_then(serde_json::Value::as_object);
    if result.is_none() {
        return Some(McpTestResult::Error {
            message: "initialize 响应缺少 result 对象".to_string(),
            stderr: None,
        });
    }

    let server_info = result.and_then(|r| r.get("serverInfo"));
    let text = |value: Option<&serde_json::Value>, key: &str| {
        value
            .and_then(|v| v.get(key))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    Some(McpTestResult::Success {
        message: "MCP 服务器握手成功".to_string(),
        server_name: text(server_info, "name"),
        server_version: text(server_info, "version"),
        protocol_version: result.and_then(|r| {
            r.get("protocolVersion")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        }),
    })
}

fn build_test_command(config: &McpServerConfig) -> Command {
    let mut cmd = Command::new(config.command.trim());
    cmd.args(&config.args);
    // 从 Finder / Dock 启动时 PATH 极简,npx / uvx 类命令会解析失败;
    // 先铺 login shell 环境,再叠加用户为该服务器配置的 env(用户显式配置优先)。
    cmd.envs(crate::app_settings::get_login_shell_env().iter().cloned());
    cmd.envs(&config.env);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::subprocess::configure_terminable_tokio_process_tree(&mut cmd);
    cmd
}

async fn test_mcp_server_internal(config: &McpServerConfig) -> McpTestResult {
    if config.command.trim().is_empty() {
        return McpTestResult::Error {
            message: "命令不能为空".to_string(),
            stderr: None,
        };
    }

    let mut child = match build_test_command(config).spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return McpTestResult::Error {
                message: format!("找不到命令 \"{}\"", config.command.trim()),
                stderr: None,
            };
        }
        Err(err) => {
            return McpTestResult::Error {
                message: format!("无法启动 MCP 服务器: {err}"),
                stderr: None,
            };
        }
    };

    // 三个管道都是上面显式 piped 的,take 理论上必成功;仍避免 unwrap 以防未来改动。
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = crate::subprocess::terminate_tokio_process_tree(&mut child).await;
        return McpTestResult::Error {
            message: "无法获取 MCP 服务器进程管道".to_string(),
            stderr: None,
        };
    };

    let outcome = timeout(
        Duration::from_secs(MCP_TEST_TIMEOUT_SECS),
        handshake(stdin, stdout, stderr),
    )
    .await;

    // stdio 服务器在 stdin EOF 后会自行退出;进程树终止兜底并回收僵尸进程。
    let _ = crate::subprocess::terminate_tokio_process_tree(&mut child).await;

    outcome.unwrap_or(McpTestResult::Timeout {
        message: format!("MCP 服务器在 {MCP_TEST_TIMEOUT_SECS} 秒内未响应初始化请求"),
    })
}

/// 排空 stderr 中已到达的行。stdout 已 EOF 时进程通常即将退出,但 stderr 未必立刻关闭,
/// 因此加一个短超时兜底,避免诊断信息的收集反过来把命令挂住。
async fn drain_stderr(
    stderr_lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStderr>>,
    captured: &mut Vec<String>,
) {
    let _ = timeout(Duration::from_millis(MCP_STDERR_DRAIN_MS), async {
        while let Ok(Some(line)) = stderr_lines.next_line().await {
            push_stderr_line(captured, line);
        }
    })
    .await;
}

async fn handshake(
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
) -> McpTestResult {
    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stderr_lines = BufReader::new(stderr).lines();
    let mut captured_stderr: Vec<String> = Vec::new();

    // 服务器可能在我们写入之前就已经退出。这时 stdin 已经没有读端,write/flush 会拿到
    // BrokenPipe——但「管道断了」不是有用的诊断,真正的原因在服务器的 stderr 里。把这种
    // 情况和 stdout EOF 走同一条路径:排空 stderr 并报告「握手前退出」。
    //
    // 写入能否落进管道缓冲区取决于内核调度,所以 BrokenPipe 是否出现本身就是平台相关的
    // 竞争(Linux 上常见,macOS 上写入通常先进缓冲区),不能作为返回值的分支依据。
    let request = initialize_request();
    let write_result = async {
        stdin.write_all(request.as_bytes()).await?;
        stdin.flush().await
    }
    .await;
    if let Err(err) = write_result {
        if err.kind() != std::io::ErrorKind::BrokenPipe {
            return McpTestResult::Error {
                message: format!("无法发送初始化请求: {err}"),
                stderr: None,
            };
        }
        drain_stderr(&mut stderr_lines, &mut captured_stderr).await;
        return McpTestResult::Error {
            message: "MCP 服务器在完成握手前退出".to_string(),
            stderr: collected_stderr(&captured_stderr),
        };
    }

    // stderr 读完后 next_line() 会立刻返回 Ok(None),若继续 select 会空转打满 CPU,
    // 因此用标志位摘掉该分支。
    let mut stderr_open = true;
    loop {
        tokio::select! {
            line = stdout_lines.next_line() => match line {
                Ok(Some(line)) => {
                    if let Some(mut result) = interpret_stdout_line(&line) {
                        if let McpTestResult::Error { stderr, .. } = &mut result {
                            if stderr_open {
                                drain_stderr(&mut stderr_lines, &mut captured_stderr).await;
                            }
                            *stderr = collected_stderr(&captured_stderr);
                        }
                        return result;
                    }
                }
                Ok(None) => {
                    // stdout 先到 EOF 时,进程写在 stderr 上的失败原因可能还没被 select 读到,
                    // 先排空 stderr 再返回,否则诊断信息会随分支竞争而丢失。
                    if stderr_open {
                        drain_stderr(&mut stderr_lines, &mut captured_stderr).await;
                    }
                    return McpTestResult::Error {
                        message: "MCP 服务器在完成握手前退出".to_string(),
                        stderr: collected_stderr(&captured_stderr),
                    };
                }
                Err(err) => {
                    if stderr_open {
                        drain_stderr(&mut stderr_lines, &mut captured_stderr).await;
                    }
                    return McpTestResult::Error {
                        message: format!("读取 MCP 服务器输出失败: {err}"),
                        stderr: collected_stderr(&captured_stderr),
                    };
                }
            },
            line = stderr_lines.next_line(), if stderr_open => match line {
                Ok(Some(line)) => push_stderr_line(&mut captured_stderr, line),
                _ => stderr_open = false,
            },
        }
    }
}

// ── 运行时接入(Agent 启动) ─────────────────────────────────────────────────
//
// 两个 Agent 家族读 MCP 配置的机制完全不同,都不是 hooks 那套 settings:
//
// - Claude:只认 `--mcp-config <file>`,文件形如 `{"mcpServers": {...}}`。
//   settings.json 没有 `mcpServers` 顶层键(实测经 --settings 传入会被忽略)。
// - Codex:读 `$CODEX_HOME/config.toml` 的 `[mcp_servers.*]`。用 `-c` 覆盖也可以,
//   但 server env 常承载 API key,放进 argv 会被同机 `ps` 读到;因此改写
//   `$CODEX_HOME/<profile>.config.toml` 并用 `-p <profile>` 层叠,机制同样生效
//   且密钥只存在于 0o600 的文件里。
//
// 两个文件都在任务启动时按当前配置重新生成,避免用户改完 MCP 后仍读到旧内容。

/// Aeroric 为 Codex 生成的 MCP profile 名。同名 `.config.toml` 由 `-p` 层叠加载。
pub const CODEX_MCP_PROFILE: &str = "aeroric-mcp";

/// 当前启用、且自身 enabled 的服务器。返回空表示不需要注入任何 MCP 配置。
fn active_servers(settings: &McpSettings) -> Vec<&McpServerConfig> {
    if !settings.enabled {
        return Vec::new();
    }
    settings
        .servers
        .values()
        .filter(|server| server.enabled)
        .collect()
}

fn claude_mcp_config_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("claude-mcp-config.json"))
}

/// 构造 Claude `--mcp-config` 的文件内容。
fn build_claude_mcp_config(servers: &[&McpServerConfig]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for server in servers {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "command".to_string(),
            serde_json::Value::String(server.command.clone()),
        );
        if !server.args.is_empty() {
            entry.insert(
                "args".to_string(),
                serde_json::Value::Array(
                    server
                        .args
                        .iter()
                        .map(|arg| serde_json::Value::String(arg.clone()))
                        .collect(),
                ),
            );
        }
        if !server.env.is_empty() {
            // env 用 BTreeMap 排序落盘,避免每次生成的文件内容顺序抖动。
            let env: serde_json::Map<String, serde_json::Value> = server
                .env
                .iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
                .collect();
            entry.insert("env".to_string(), serde_json::Value::Object(env));
        }
        map.insert(server.name.clone(), serde_json::Value::Object(entry));
    }
    serde_json::json!({ "mcpServers": serde_json::Value::Object(map) })
}

/// 为一次 Claude 启动准备 `--mcp-config` 文件,返回 `None` 表示无需该 flag。
/// env 可能含 API key,沿用 `atomic_write_private`(0o600)。
pub fn claude_mcp_config_path_for_launch() -> Result<Option<PathBuf>, String> {
    let settings = load_mcp_settings()?;
    let servers = active_servers(&settings);
    if servers.is_empty() {
        return Ok(None);
    }
    let path = claude_mcp_config_path()?;
    ensure_private_dir(&aeroric_dir()?)?;
    let raw = serde_json::to_string_pretty(&build_claude_mcp_config(&servers))
        .map_err(|err| format!("无法序列化 Claude MCP 配置: {err}"))?;
    atomic_write_private(&path, &raw).map_err(|err| format!("无法写入 Claude MCP 配置: {err}"))?;
    Ok(Some(path))
}

/// 把字符串写成合法的 TOML basic string 字面量。
fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

/// 构造 Codex profile 的 `[mcp_servers.*]` TOML 文本。
fn build_codex_mcp_toml(servers: &[&McpServerConfig]) -> String {
    let mut out =
        String::from("# 由 Aeroric 生成,请勿手工编辑。内容随 MCP 设置在任务启动时重写。\n");
    for server in servers {
        out.push_str(&format!("\n[mcp_servers.{}]\n", toml_string(&server.name)));
        out.push_str(&format!("command = {}\n", toml_string(&server.command)));
        let args = server
            .args
            .iter()
            .map(|arg| toml_string(arg))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("args = [{args}]\n"));
        if !server.env.is_empty() {
            out.push_str(&format!(
                "\n[mcp_servers.{}.env]\n",
                toml_string(&server.name)
            ));
            for (key, value) in server.env.iter().collect::<BTreeMap<_, _>>() {
                out.push_str(&format!("{} = {}\n", toml_string(key), toml_string(value)));
            }
        }
    }
    out
}

/// 为一次 Codex 启动准备 profile 文件,返回 profile 名(`None` 表示无需 `-p`)。
///
/// `codex_home` 由调用方给出:内建 Codex 用 `~/.codex`,自定义 Agent 用各自的
/// `agent-homes/{id}`,与该 Agent 实际读取的 CODEX_HOME 保持一致。
pub fn codex_mcp_profile_for_launch(codex_home: &Path) -> Result<Option<String>, String> {
    let settings = load_mcp_settings()?;
    let servers = active_servers(&settings);
    if servers.is_empty() {
        return Ok(None);
    }
    let raw = build_codex_mcp_toml(&servers);
    // 先自校验:生成非法 TOML 会让 Codex 启动直接失败,宁可放弃注入。
    toml::from_str::<toml::Value>(&raw)
        .map_err(|err| format!("生成的 Codex MCP profile 不是合法 TOML: {err}"))?;
    ensure_private_dir(codex_home)?;
    let path = codex_home.join(format!("{CODEX_MCP_PROFILE}.config.toml"));
    atomic_write_private(&path, &raw)
        .map_err(|err| format!("无法写入 Codex MCP profile: {err}"))?;
    Ok(Some(CODEX_MCP_PROFILE.to_string()))
}

/// dsh 的 `serverName` 约束:`[A-Za-z0-9_-]{1,32}`。非法字符替换为 `-`,
/// 截断到 32;冲突时追加序号,保证 patch 内唯一。
fn dsh_server_name(raw: &str, used: &mut Vec<String>) -> String {
    let mut name: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    name.truncate(32);
    if name.is_empty() {
        name = "server".to_string();
    }
    let mut candidate = name.clone();
    let mut counter = 2;
    while used.contains(&candidate) {
        let suffix = format!("-{counter}");
        let mut base = name.clone();
        base.truncate(32 - suffix.len());
        candidate = format!("{base}{suffix}");
        counter += 1;
    }
    used.push(candidate.clone());
    candidate
}

/// YAML 双引号字符串(命令/参数/env 可能含任意字符)。
fn yaml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// 构造 dsh 的 MCP patch 覆盖层:每个 server 一个 `dsh-mcp-client` plugin row
/// (insert 操作;row id 带 `aeroric-mcp-` 前缀,与受管/用户 patch 不冲突)。
fn build_dsh_mcp_patch(servers: &[&McpServerConfig]) -> String {
    let mut out = String::from(
        "# 由 Aeroric 生成,请勿手工编辑。内容随 MCP 设置在任务启动时重写。\n- insert:\n",
    );
    let mut used = Vec::new();
    for server in servers {
        let server_name = dsh_server_name(&server.name, &mut used);
        out.push_str(&format!("    - id: aeroric-mcp-{server_name}\n"));
        out.push_str("      name: '@deepseek-ai/dsh-mcp-client'\n");
        out.push_str("      config:\n");
        out.push_str("        transport: stdio\n");
        out.push_str(&format!(
            "        serverName: {}\n",
            yaml_string(&server_name)
        ));
        out.push_str(&format!(
            "        command: {}\n",
            yaml_string(&server.command)
        ));
        if server.args.is_empty() {
            out.push_str("        args: []\n");
        } else {
            out.push_str("        args:\n");
            for arg in &server.args {
                out.push_str(&format!("          - {}\n", yaml_string(arg)));
            }
        }
        if !server.env.is_empty() {
            out.push_str("        env:\n");
            for (key, value) in server.env.iter().collect::<BTreeMap<_, _>>() {
                out.push_str(&format!(
                    "          {}: {}\n",
                    yaml_string(key),
                    yaml_string(value)
                ));
            }
        }
    }
    out
}

/// 为一次 dsh 启动准备 MCP patch 文件,返回 `None` 表示无需注入。
/// env 可能含 API key,写入走 `atomic_write_private`(0o600);文件放在对应
/// agent 的托管 home 内,任务启动时整体重写(server 增删改自然生效)。
pub fn dsh_mcp_patch_for_launch(dsh_home: &Path) -> Result<Option<PathBuf>, String> {
    let settings = load_mcp_settings()?;
    let servers = active_servers(&settings);
    if servers.is_empty() {
        return Ok(None);
    }
    let raw = build_dsh_mcp_patch(&servers);
    ensure_private_dir(dsh_home)?;
    let path = dsh_home.join("aeroric.mcp.patch.yml");
    atomic_write_private(&path, &raw).map_err(|err| format!("无法写入 dsh MCP patch: {err}"))?;
    Ok(Some(path))
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_mcp_settings() -> Result<McpSettings, String> {
    load_mcp_settings()
}

#[tauri::command]
pub fn set_mcp_settings(settings: McpSettings) -> Result<McpSettings, String> {
    save_mcp_settings(&settings)?;
    load_mcp_settings()
}

#[tauri::command]
pub async fn test_mcp_server(config: McpServerConfig) -> Result<McpTestResult, String> {
    Ok(test_mcp_server_internal(&config).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("aeroric-mcp-{label}-{suffix}.json"))
    }

    fn server(name: &str, command: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.to_string(),
            command: command.to_string(),
            args: Vec::new(),
            env: HashMap::new(),
            enabled: true,
        }
    }

    #[test]
    fn builds_dsh_mcp_patch_rows_with_sanitized_server_names() {
        let mut weird = server("我的 server!", "npx");
        weird.args = vec!["-y".to_string(), "mcp-thing".to_string()];
        weird
            .env
            .insert("TOKEN".to_string(), "se\"cret".to_string());
        let plain = server("files", "/usr/bin/files-mcp");
        let patch = build_dsh_mcp_patch(&[&weird, &plain]);
        assert!(patch.contains("- insert:"));
        assert!(patch.contains("name: '@deepseek-ai/dsh-mcp-client'"));
        assert!(patch.contains("transport: stdio"));
        // 非法字符替换为 '-';两个 server 各占一行 row。
        assert!(patch.contains("serverName: \"---server-\""));
        assert!(patch.contains("serverName: \"files\""));
        assert!(patch.contains("command: \"npx\""));
        assert!(patch.contains("- \"mcp-thing\""));
        assert!(patch.contains("\"TOKEN\": \"se\\\"cret\""));
    }

    #[test]
    fn dsh_server_names_stay_unique_after_sanitization() {
        let mut used = Vec::new();
        assert_eq!(dsh_server_name("a b", &mut used), "a-b");
        assert_eq!(dsh_server_name("a-b", &mut used), "a-b-2");
        assert_eq!(dsh_server_name("", &mut used), "server");
        let long = "x".repeat(64);
        assert_eq!(dsh_server_name(&long, &mut used).len(), 32);
    }

    #[test]
    fn missing_file_yields_default_settings() {
        let settings =
            read_settings_from(&temp_path("missing")).expect("missing file is not fatal");
        assert_eq!(settings, McpSettings::default());
    }

    #[test]
    fn empty_file_yields_default_settings() {
        let path = temp_path("empty");
        fs::write(&path, "   \n").expect("write");
        let settings = read_settings_from(&path).expect("empty file is not fatal");
        let _ = fs::remove_file(&path);
        assert_eq!(settings, McpSettings::default());
    }

    #[test]
    fn malformed_file_reports_error() {
        let path = temp_path("malformed");
        fs::write(&path, "{ not json").expect("write");
        let result = read_settings_from(&path);
        let _ = fs::remove_file(&path);
        assert!(result.is_err());
    }

    fn settings_of(servers: Vec<McpServerConfig>) -> McpSettings {
        McpSettings {
            enabled: false,
            servers: servers
                .into_iter()
                .map(|server| (server.name.clone(), server))
                .collect(),
        }
    }

    #[test]
    fn round_trips_settings_through_disk() {
        let path = temp_path("round-trip");
        let mut env = HashMap::new();
        env.insert("TOKEN".to_string(), "secret".to_string());
        let settings = McpSettings {
            enabled: true,
            servers: BTreeMap::from([(
                "files".to_string(),
                McpServerConfig {
                    name: "files".to_string(),
                    command: "npx".to_string(),
                    args: vec![
                        "-y".to_string(),
                        "@modelcontextprotocol/server-filesystem".to_string(),
                    ],
                    env,
                    enabled: false,
                },
            )]),
        };

        write_settings_to(&path, &settings).expect("write settings");
        let loaded = read_settings_from(&path).expect("read settings");
        let _ = fs::remove_file(&path);
        assert_eq!(loaded, settings);
    }

    #[test]
    fn parses_name_keyed_map_without_inline_name() {
        // 前端只在 map key 里给名称,内嵌 name 缺失时应由 normalize 回填。
        let parsed: McpSettings =
            serde_json::from_str(r#"{"enabled":true,"servers":{"files":{"command":"npx"}}}"#)
                .expect("parse");
        let normalized = normalize_settings(&parsed).expect("normalize");
        assert_eq!(normalized.servers["files"].name, "files");
        assert_eq!(normalized.servers["files"].command, "npx");
        assert!(normalized.enabled);
    }

    #[test]
    fn normalize_prefers_map_key_over_inline_name() {
        let settings = McpSettings {
            enabled: false,
            servers: BTreeMap::from([("real-key".to_string(), server("stale-name", "run"))]),
        };
        let normalized = normalize_settings(&settings).expect("normalize");
        assert!(normalized.servers.contains_key("real-key"));
        assert_eq!(normalized.servers["real-key"].name, "real-key");
    }

    #[cfg(unix)]
    #[test]
    fn persisted_config_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("permissions");
        write_settings_to(&path, &McpSettings::default()).expect("write settings");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        let _ = fs::remove_file(&path);
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn normalize_trims_and_defaults_missing_enabled_flag() {
        let parsed: McpSettings =
            serde_json::from_str(r#"{"servers":{"  a  ":{"command":"  run  "}}}"#).expect("parse");
        assert!(parsed.servers["  a  "].enabled, "enabled defaults to true");

        let normalized = normalize_settings(&parsed).expect("normalize");
        assert_eq!(normalized.servers["a"].name, "a");
        assert_eq!(normalized.servers["a"].command, "run");
        assert!(!normalized.enabled, "top-level enabled defaults to false");
    }

    #[test]
    fn normalize_rejects_blank_and_duplicate_entries() {
        let blank_name = settings_of(vec![server("   ", "run")]);
        assert!(normalize_settings(&blank_name).is_err());

        let blank_command = settings_of(vec![server("a", "  ")]);
        assert!(normalize_settings(&blank_command).is_err());

        // "a" 与 " a " 是不同的 map key,但去空白后相撞,必须报错而非静默覆盖。
        let collides_after_trim = settings_of(vec![server("a", "run"), server(" a ", "other")]);
        assert!(normalize_settings(&collides_after_trim).is_err());
    }

    #[test]
    fn interpret_stdout_line_skips_unrelated_output() {
        assert!(interpret_stdout_line("starting server...").is_none());
        assert!(interpret_stdout_line(r#"{"jsonrpc":"2.0","method":"log"}"#).is_none());
        assert!(interpret_stdout_line(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#).is_none());
    }

    // ── 运行时接入 ──────────────────────────────────────────────────────────

    fn enabled_settings(servers: Vec<McpServerConfig>) -> McpSettings {
        McpSettings {
            enabled: true,
            servers: servers
                .into_iter()
                .map(|server| (server.name.clone(), server))
                .collect(),
        }
    }

    #[test]
    fn active_servers_respects_both_enable_switches() {
        // 总开关关闭 → 无论单个服务器状态都不注入
        let mut off = enabled_settings(vec![server("a", "run")]);
        off.enabled = false;
        assert!(active_servers(&off).is_empty());

        // 总开关开启,但服务器自身关闭 → 跳过该服务器
        let mut disabled_server = server("b", "run");
        disabled_server.enabled = false;
        let settings = enabled_settings(vec![server("a", "run"), disabled_server]);
        let active = active_servers(&settings);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "a");
    }

    #[test]
    fn claude_mcp_config_uses_mcp_servers_shape() {
        let mut with_env = server("fs", "npx");
        with_env.args = vec![
            "-y".into(),
            "@modelcontextprotocol/server-filesystem".into(),
        ];
        with_env.env.insert("API_KEY".into(), "secret".into());
        let settings = enabled_settings(vec![with_env]);
        let value = build_claude_mcp_config(&active_servers(&settings));

        let entry = value
            .get("mcpServers")
            .and_then(|servers| servers.get("fs"))
            .expect("server entry under mcpServers");
        assert_eq!(entry.get("command").and_then(|v| v.as_str()), Some("npx"));
        assert_eq!(
            entry
                .get("args")
                .and_then(|v| v.as_array())
                .map(|args| args.len()),
            Some(2)
        );
        assert_eq!(
            entry
                .get("env")
                .and_then(|env| env.get("API_KEY"))
                .and_then(|v| v.as_str()),
            Some("secret")
        );
    }

    #[test]
    fn claude_mcp_config_omits_empty_args_and_env() {
        let settings = enabled_settings(vec![server("bare", "run")]);
        let value = build_claude_mcp_config(&active_servers(&settings));
        let entry = value
            .get("mcpServers")
            .and_then(|servers| servers.get("bare"))
            .expect("server entry");
        assert!(entry.get("args").is_none());
        assert!(entry.get("env").is_none());
    }

    #[test]
    fn codex_mcp_toml_parses_and_keeps_env() {
        let mut with_env = server("fs", "npx");
        with_env.args = vec!["-y".into(), "pkg".into()];
        with_env.env.insert("API_KEY".into(), "secret".into());
        let settings = enabled_settings(vec![with_env]);
        let raw = build_codex_mcp_toml(&active_servers(&settings));

        let parsed = toml::from_str::<toml::Value>(&raw).expect("generated TOML must parse");
        let entry = parsed
            .get("mcp_servers")
            .and_then(|servers| servers.get("fs"))
            .expect("mcp_servers.fs");
        assert_eq!(entry.get("command").and_then(|v| v.as_str()), Some("npx"));
        assert_eq!(
            entry
                .get("args")
                .and_then(|v| v.as_array())
                .map(|args| args.len()),
            Some(2)
        );
        assert_eq!(
            entry
                .get("env")
                .and_then(|env| env.get("API_KEY"))
                .and_then(|v| v.as_str()),
            Some("secret")
        );
    }

    #[test]
    fn codex_mcp_toml_escapes_hostile_names_and_values() {
        // 名称/命令里的引号与反斜杠若不转义会产出非法 TOML 或串改结构。
        let mut hostile = server(r#"we"ird\name"#, r#"C:\path\to\my "server".exe"#);
        hostile.args = vec![r#"--flag="x""#.into()];
        hostile
            .env
            .insert("KEY".into(), "line1\nline2\ttabbed".into());
        let settings = enabled_settings(vec![hostile]);
        let raw = build_codex_mcp_toml(&active_servers(&settings));

        let parsed =
            toml::from_str::<toml::Value>(&raw).expect("hostile input must stay valid TOML");
        let entry = parsed
            .get("mcp_servers")
            .and_then(|servers| servers.get(r#"we"ird\name"#))
            .expect("hostile server name round-trips");
        assert_eq!(
            entry.get("command").and_then(|v| v.as_str()),
            Some(r#"C:\path\to\my "server".exe"#)
        );
        assert_eq!(
            entry
                .get("env")
                .and_then(|env| env.get("KEY"))
                .and_then(|v| v.as_str()),
            Some("line1\nline2\ttabbed")
        );
    }

    /// 直接落盘 profile 文件,复用 `codex_mcp_profile_for_launch` 的写入逻辑但
    /// 不依赖真实 MCP 配置(该函数从 `~/.aeroric/mcp.json` 读,测试不应触碰用户文件)。
    fn write_profile_for_test(codex_home: &Path, settings: &McpSettings) -> Option<PathBuf> {
        let servers = active_servers(settings);
        if servers.is_empty() {
            return None;
        }
        let raw = build_codex_mcp_toml(&servers);
        toml::from_str::<toml::Value>(&raw).expect("valid TOML");
        ensure_private_dir(codex_home).expect("create codex home");
        let path = codex_home.join(format!("{CODEX_MCP_PROFILE}.config.toml"));
        atomic_write_private(&path, &raw).expect("write profile");
        Some(path)
    }

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aeroric-mcp-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ))
    }

    #[test]
    fn codex_profile_file_lands_in_codex_home_and_stays_private() {
        let home = temp_dir("codex-home");
        let mut with_env = server("fs", "npx");
        with_env.env.insert("API_KEY".into(), "secret".into());
        let settings = enabled_settings(vec![with_env]);

        let path = write_profile_for_test(&home, &settings).expect("profile written");
        // 文件名必须与 `-p aeroric-mcp` 解析的 `<profile>.config.toml` 一致。
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("aeroric-mcp.config.toml")
        );
        assert_eq!(path.parent(), Some(home.as_path()));

        let raw = fs::read_to_string(&path).expect("read profile");
        // 密钥只应存在于文件里(0o600),不进 argv。
        assert!(raw.contains("secret"));

        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "profile must not be group/world readable"
            );
        }

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn codex_profile_is_not_written_when_nothing_active() {
        let home = temp_dir("codex-home-empty");
        // 总开关关闭:不写文件,调用方拿到 None 因而不会拼 `-p`。
        let mut off = enabled_settings(vec![server("a", "run")]);
        off.enabled = false;
        assert!(write_profile_for_test(&home, &off).is_none());
        assert!(!home.exists(), "must not create the home dir when idle");
    }

    #[test]
    fn interpret_stdout_line_reads_server_info() {
        let line = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05",
            "serverInfo":{"name":"filesystem","version":"0.6.2"}}}"#;
        match interpret_stdout_line(line).expect("terminal response") {
            McpTestResult::Success {
                server_name,
                server_version,
                protocol_version,
                ..
            } => {
                assert_eq!(server_name.as_deref(), Some("filesystem"));
                assert_eq!(server_version.as_deref(), Some("0.6.2"));
                assert_eq!(protocol_version.as_deref(), Some("2024-11-05"));
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn interpret_stdout_line_surfaces_jsonrpc_error() {
        let line = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"bad request"}}"#;
        match interpret_stdout_line(line).expect("terminal response") {
            McpTestResult::Error { message, .. } => assert!(message.contains("bad request")),
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[test]
    fn stderr_capture_is_bounded() {
        let mut lines = Vec::new();
        for index in 0..(MAX_STDERR_LINES + 10) {
            push_stderr_line(&mut lines, format!("line {index}"));
        }
        assert_eq!(lines.len(), MAX_STDERR_LINES);
        assert_eq!(collected_stderr(&[]), None);
    }

    #[tokio::test]
    async fn missing_command_reports_not_found() {
        let config = server("ghost", "aeroric-nonexistent-mcp-binary");
        match test_mcp_server_internal(&config).await {
            McpTestResult::Error { message, .. } => assert!(message.contains("找不到命令")),
            other => panic!("expected not-found error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn blank_command_is_rejected_before_spawn() {
        let config = server("blank", "   ");
        match test_mcp_server_internal(&config).await {
            McpTestResult::Error { message, .. } => assert!(message.contains("命令不能为空")),
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn server_exiting_before_handshake_reports_stderr() {
        let mut config = server("early-exit", "sh");
        config.args = vec!["-c".to_string(), "echo boom >&2; exit 1".to_string()];
        match test_mcp_server_internal(&config).await {
            McpTestResult::Error { message, stderr } => {
                assert!(message.contains("退出"));
                assert_eq!(stderr.as_deref(), Some("boom"));
            }
            other => panic!("expected early-exit error, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handshake_succeeds_against_stub_server() {
        let mut config = server("stub", "sh");
        // 读一行请求后回一条合法 initialize 响应,验证端到端握手路径。
        config.args = vec![
            "-c".to_string(),
            r#"read -r _line; printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"stub","version":"1.0.0"}}}'; sleep 5"#
                .to_string(),
        ];
        match test_mcp_server_internal(&config).await {
            McpTestResult::Success { server_name, .. } => {
                assert_eq!(server_name.as_deref(), Some("stub"));
            }
            other => panic!("expected success, got {other:?}"),
        }
    }
}
