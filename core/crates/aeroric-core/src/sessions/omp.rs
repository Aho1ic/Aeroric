//! omp 族的路径层（P3-04 第二片的 omp 半边）。
//!
//! 逐字迁移自宿主 `src-tauri/src/session_omp.rs` 的 `omp_project_dir_name`、
//! `omp_session_id_from_file_name`，以及允许根的两段收集。编码规则镜像 omp 上游
//! `session-paths.ts`（home 相对 `-<rel>`、temp `-tmp<rel>`、否则 `--<absolute>--`，
//! `/ \ :` → `-`）——手写约束 5 要求「同一 cwd 两侧算出同一目录名」的用例，
//! 金样在 `tests/session_discovery.rs`。
//!
//! **权限边界（手写约束 2 ／ AGENTS.md『Native OMP session history boundary』）**：
//! core 把托管根与外部历史根分成两个函数，外部那一支（`~/.omp/agent/sessions` 与
//! `~/.omp/profiles/*/agent/sessions`）是**旧客户端现存行为**，由宿主自行组合调用；
//! 新 core 消费方只许 `~/.aeroric/agent-homes/**`（capability 默认 false，开启需
//! 满足 ADR-001 §5 的评审）。既不借迁移静默扩权限，也不删旧客户端行为。

use std::fs;
use std::path::{Path, PathBuf};

/// omp 的 cwd→会话目录名编码。`home` 由调用方注入；`None` 时跳过 home 相对分支。
pub fn omp_project_dir_name(home: Option<&Path>, project_path: &Path) -> String {
    let canonical_cwd = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    let home = home.map(|home| home.canonicalize().unwrap_or_else(|_| home.to_path_buf()));
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .unwrap_or(std::env::temp_dir());

    let encode_relative = |prefix: &str, relative: &Path| -> String {
        let encoded = relative.to_string_lossy().replace(['/', '\\', ':'], "-");
        format!("{prefix}{encoded}")
    };
    let encode_absolute = |absolute: &Path| -> String {
        let stripped = absolute
            .to_string_lossy()
            .trim_start_matches(['/', '\\'])
            .replace(['/', '\\', ':'], "-");
        format!("--{stripped}--")
    };

    if let Some(home) = home.as_ref() {
        if let Ok(relative) = canonical_cwd.strip_prefix(home) {
            if relative.as_os_str().is_empty() {
                return "-".to_string();
            }
            return encode_relative("-", relative);
        }
    }
    if let Ok(relative) = canonical_cwd.strip_prefix(&temp_root) {
        if relative.as_os_str().is_empty() {
            return "-tmp".to_string();
        }
        return encode_relative("-tmp", relative);
    }
    encode_absolute(&canonical_cwd)
}

/// 从文件名提取会话 id:`<ISO时间戳>_<uuidv7>.jsonl` 的 `_` 之后一段。
pub fn omp_session_id_from_file_name(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let stem = file_name.strip_suffix(".jsonl")?;
    let (_, id) = stem.split_once('_')?;
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

/// 托管根的会话目录：`<home>/.aeroric/agent-homes/*\/sessions`。
///
/// 宿主原文**不做 `is_dir()` 过滤**（自定义档案 home 在注册时未必已建 sessions），
/// 逐字保留——加过滤会让「先算根、后建目录」的时序变脆。
pub fn omp_managed_session_roots(home: &Path) -> Vec<PathBuf> {
    let root = home.join(".aeroric").join("agent-homes");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path().join("sessions"))
        .collect()
}

/// 外部历史根：用户直接跑系统 omp 的 `~/.omp/agent/sessions`，以及 profile 作用域的
/// `<home>/.omp/profiles/<name>/agent/sessions`（上游 `dirs.ts::resolveActiveAgentDirOverride`
/// 让 profile 优先于 `PI_CODING_AGENT_DIR`）。只放行 `<profile>/agent/sessions`
/// 这一层，不是整个 `~/.omp`。
///
/// **这条支是旧客户端行为，新 core 消费方默认不得调用**（capability=false，
/// AGENTS.md『Native OMP session history boundary』；开启需 ADR-001 §5 评审）。
pub fn omp_external_history_session_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![home.join(".omp").join("agent").join("sessions")];
    if let Ok(entries) = fs::read_dir(home.join(".omp").join("profiles")) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                roots.push(entry.path().join("agent").join("sessions"));
            }
        }
    }
    roots
}

// ── 读侧与解析（第三片，2026-09-29 补记二百四十三）──────────────────────────
// 逐字迁移自宿主 `src-tauri/src/session_omp.rs`，行为零改动。

use serde_json::Value;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};

use crate::sessions::SessionContent;
use crate::sessions::SessionMessage;

/// 读取 omp 会话文件头(`{type:"session",id,...}` 行)的会话 id。
/// 只流式读前 4 行:title 槽固定 256 字节,header 必然在其中,避免整读大文件。
pub fn read_omp_session_header(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().take(4) {
        let Ok(line) = line else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("session") {
            return value.get("id").and_then(Value::as_str).map(str::to_string);
        }
    }
    None
}

/// 解析 omp 会话 JSONL 行为 Aeroric 的会话消息。
///
/// 布局:首行 256 字节 title 槽、次行 header(`{type:"session",...}`)、之后为
/// append-only entry 树。entry 里的 `message` 对象与 omp RPC 的 AgentMessage 同构:
/// - user:content 为字符串或 `[{type:"text",...},{type:"image",...}]` 数组
/// - assistant:content 数组含 `{type:"text"|"thinking"|"toolCall",...}`
/// - toolResult:content 数组(文本项拼接为输出)
///
/// compaction/model_usage 等非消息 entry 与空行跳过;entry 树按文件线性序展示
/// (omp 的分支树在 Aeroric 中平铺,与 claude/codex 解析的处理一致)。
pub fn parse_omp_session_lines<S: AsRef<str>>(lines: &[S]) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    for line in lines {
        let line = line.as_ref();
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        let message_id = value.get("id").and_then(Value::as_str).map(str::to_string);
        let content = match role {
            "user" => parse_omp_user_content(message),
            "assistant" => parse_omp_assistant_content(message),
            "toolResult" => parse_omp_tool_result_content(message),
            // developer 等其它角色按原样跳过,避免会话视图出现无法渲染的条目。
            _ => continue,
        };
        if content.is_empty() {
            continue;
        }
        messages.push(SessionMessage {
            role: role.to_string(),
            content,
            message_id,
        });
    }
    Ok(messages)
}

fn parse_omp_user_content(message: &Value) -> Vec<SessionContent> {
    match message.get("content") {
        // 空字符串/空文本项不渲染空气泡。
        Some(Value::String(text)) if !text.trim().is_empty() => {
            vec![SessionContent::Text { text: text.clone() }]
        }
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
                    (!text.trim().is_empty()).then_some(SessionContent::Text {
                        text: text.to_string(),
                    })
                }
                // 图片附件只保留占位元数据,不内联 base64。
                Some("image") => Some(SessionContent::Attachment {
                    name: "image".to_string(),
                    media_type: item
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png")
                        .to_string(),
                    source: "inline-image".to_string(),
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_omp_assistant_content(message: &Value) -> Vec<SessionContent> {
    let Some(Value::Array(items)) = message.get("content") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
                (!text.trim().is_empty()).then_some(SessionContent::Text {
                    text: text.to_string(),
                })
            }
            Some("thinking") => {
                let thinking = item
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                (!thinking.trim().is_empty()).then_some(SessionContent::Thinking {
                    thinking: thinking.to_string(),
                })
            }
            Some("toolCall") => {
                let input = match item.get("arguments") {
                    Some(Value::String(raw)) => raw.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                Some(SessionContent::ToolUse {
                    id: item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string(),
                    input,
                })
            }
            _ => None,
        })
        .collect()
}

fn parse_omp_tool_result_content(message: &Value) -> Vec<SessionContent> {
    let tool_call_id = message
        .get("toolCallId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let output = match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                Some("text") => item.get("text").and_then(Value::as_str).map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    // 空输出不渲染空 result 卡片;返回空 vec 让外层把整条消息过滤掉。
    if output.trim().is_empty() {
        return Vec::new();
    }
    vec![SessionContent::ToolResult {
        id: tool_call_id,
        output,
    }]
}

/// 在一个会话目录里挑 mtime 晚于 `since_ms` 的最新 `.jsonl`。
///
/// `claimed` 非空时跳过其中的路径。会话恢复入口按时间兜底发现,不该过滤(那条路径本来就是
/// 它自己的);watcher 必须过滤,否则同项目下的第二个任务会认领第一个任务的会话。
///
/// 接目录而不接 `(agent, project_path)`:目录解析要读真实 home,接进来就没法在测试里
/// 摆出「一条已认领 + 一条更旧的未认领」这个局面,而那正是要防的那个 bug。
pub fn newest_omp_session_in_dir(
    dir: &Path,
    since_ms: u128,
    claimed: Option<&HashSet<String>>,
) -> Option<(String, PathBuf)> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        if claimed.is_some_and(|claimed| claimed.contains(path.to_string_lossy().as_ref())) {
            continue;
        }
        // 单个条目元数据异常只跳过,不能让整个发现流程返回 None。
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(modified_ms) = modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
        else {
            continue;
        };
        if modified_ms <= since_ms {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_time, _)| modified > *best_time)
        {
            best = Some((modified, path));
        }
    }
    let (_, path) = best?;
    let session_id = omp_session_id_from_file_name(&path)?;
    Some((session_id, path))
}
