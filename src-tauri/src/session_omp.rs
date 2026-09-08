//! omp 会话发现与注册。
//!
//! omp 的会话文件是明文 JSONL:`<PI_CODING_AGENT_DIR>/agent/sessions/<encoded-cwd>/<ISO>_<uuidv7>.jsonl`
//! (编码规则见 `omp_project_dir_name`:home 内 `-<relative>`、temp 内 `-tmp<relative>`、
//! 其它 `--<absolute>--`,`/ \ :` 统一替换为 `-`)。文件懒创建:首个 assistant 消息
//! 产出时才落盘,但文件名(含 uuid)在会话创建时就已确定。omp 跑在 PTY 里,Aeroric
//! 拿不到会话文件名,只能靠 `spawn_omp_session_watcher` 扫目录认领。

use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::session::{OmpSessionInfo, SessionContent, SessionMessage};
use crate::TaskManager;

/// 认领一条 omp 会话文件并广播给前端。
///
/// 返回**是否由本次调用认领成功**。`false` 说明这条路径已经属于别的任务 —— 同项目下多个
/// omp 任务会扫到同一个目录,谁先落盘谁先被看到。调用方(watcher)必须据此继续等自己的文件
/// 出现,而不是把别人的会话当成自己的然后退出。
pub(crate) fn register_omp_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    task_id: &str,
    session_id: &str,
    session_path: &Path,
) -> bool {
    let path_string = session_path.to_string_lossy().into_owned();
    {
        let tm = app.state::<TaskManager>();
        let mut claimed = tm.claimed_session_paths.lock();
        // 插入是原子的:即使两个 watcher 在同一瞬间都看到这条未认领的路径,也只有一个能
        // 拿到它。这正是返回值要覆盖的竞态。
        if !claimed.insert(path_string.clone()) {
            return false;
        }
        tm.omp_sessions.lock().insert(
            task_id.to_string(),
            OmpSessionInfo {
                session_id: session_id.to_string(),
                session_path: path_string.clone(),
            },
        );
    }
    let _ = app.emit(
        "task-session",
        serde_json::json!({
            "task_id": task_id,
            "session_id": session_id,
            "session_path": path_string,
            "codex_like": false,
            "family": "omp",
        }),
    );
    true
}

/// omp 会话目录名编码(对应 omp 官方 `session-paths.ts` 的 `getDefaultSessionDirName`):
/// home 内 `-<relative>`、系统 temp 内 `-tmp<relative>`、其余 `--<absolute>--`;
/// `/` `\` `:` 替换为 `-`(18.1.10 无 UTF-16 转义)。
pub(crate) fn omp_project_dir_name(project_path: &Path) -> String {
    let canonical_cwd = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    let home = crate::platform::home_dir().map(|home| home.canonicalize().unwrap_or(home));
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

/// omp 族 agent 的会话根目录(托管 home 优先)。
pub(crate) fn omp_sessions_dir_for(agent: &str) -> Result<PathBuf, String> {
    Ok(crate::omp_home::omp_home_for(agent)?.join("sessions"))
}

/// 会话读取的路径白名单:内建 omp 的托管 home sessions + 用户自己的
/// `~/.omp/agent/sessions`(用户可能手动用系统 omp 在同一项目里跑过会话)。
/// 自定义 omp 档案(home 为 `agent-homes/{id}`)的路径读取走 task 里持久化的
/// session 路径,Phase 7 接入时按 agent 校验。
pub(crate) fn omp_session_allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(managed) = crate::omp_home::omp_home_for("omp") {
        roots.push(managed.join("sessions"));
    }
    // 自定义 omp 档案(Phase 7)的会话在各自 home(agent-homes/{id}/sessions);
    // 校验只拿到 family 拿不到档案 id,放行全部 Aeroric 托管 home 的 sessions
    // ——这些目录都在 ~/.aeroric 隔离目录内,且仅用于 omp 族的会话读取。
    if let Some(root) =
        crate::platform::home_dir().map(|home| home.join(".aeroric").join("agent-homes"))
    {
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                roots.push(entry.path().join("sessions"));
            }
        }
    }
    if let Some(default_dir) = crate::platform::home_dir() {
        roots.push(default_dir.join(".omp").join("agent").join("sessions"));
        roots.extend(omp_profile_session_roots(&default_dir));
    }
    roots
}

/// `<home>/.omp/profiles/<name>/agent/sessions` —— profile 作用域的会话目录。
///
/// 用户在 shell rc 里 export 过 `PI_PROFILE` / `OMP_PROFILE` 时,omp 的 agent dir
/// 变成 `~/.omp/profiles/<name>/agent`(上游 `dirs.ts::resolveActiveAgentDirOverride`
/// 让 profile 优先于 `PI_CODING_AGENT_DIR`),会话就落在那底下而非托管 home。
///
/// 漏掉这一支,历史视图会在 `session.rs` 的 allowed-roots 校验处拒掉真实存在的
/// 会话文件,前端表现为"加载很久之后失败"。
///
/// 只放行 `<profile>/agent/sessions` 这一层,不是整个 `~/.omp`。
fn omp_profile_session_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(entries) = fs::read_dir(home.join(".omp").join("profiles")) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                roots.push(entry.path().join("agent").join("sessions"));
            }
        }
    }
    roots
}

/// 从文件名提取会话 id:`<ISO时间戳>_<uuidv7>.jsonl` 的 `_` 之后一段。
pub(crate) fn omp_session_id_from_file_name(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let stem = file_name.strip_suffix(".jsonl")?;
    let (_, id) = stem.split_once('_')?;
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

/// 解析前端传来的会话引用(`ompSessionId ?? ompSessionPath`,两种形态都可能)
/// 为 `(session_id, path)`。resume 时 `omp_sessions` 已被 `finalize_task_exit`
/// 清空,需要据此立即重新注册,不能等 watcher —— 旧会话文件的 mtime 早于
/// `since_ms`,发现逻辑扫不到它。
pub(crate) fn resolve_omp_session_ref(
    agent: &str,
    project_path: &Path,
    session_ref: &str,
) -> Option<(String, PathBuf)> {
    let session_ref = session_ref.trim();
    if session_ref.is_empty() {
        return None;
    }
    if session_ref.ends_with(".jsonl") {
        let path = PathBuf::from(session_ref);
        if path.is_file() {
            let id = omp_session_id_from_file_name(&path)?;
            return Some((id, path));
        }
        return None;
    }
    let dir = omp_sessions_dir_for(agent)
        .ok()?
        .join(omp_project_dir_name(project_path));
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if omp_session_id_from_file_name(&path).as_deref() == Some(session_ref) {
            return Some((session_ref.to_string(), path));
        }
    }
    None
}

/// 在一个会话目录里挑 mtime 晚于 `since_ms` 的最新 `.jsonl`。
///
/// `claimed` 非空时跳过其中的路径。会话恢复入口按时间兜底发现,不该过滤(那条路径本来就是
/// 它自己的);watcher 必须过滤,否则同项目下的第二个任务会认领第一个任务的会话。
///
/// 接目录而不接 `(agent, project_path)`:目录解析要读真实 home,接进来就没法在测试里
/// 摆出「一条已认领 + 一条更旧的未认领」这个局面,而那正是要防的那个 bug。
fn newest_omp_session_in_dir(
    dir: &Path,
    since_ms: u128,
    claimed: Option<&std::collections::HashSet<String>>,
) -> Option<(String, PathBuf)> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<(SystemTime, PathBuf)> = None;
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
        let Ok(modified_ms) = modified.duration_since(UNIX_EPOCH).map(|d| d.as_millis()) else {
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

fn omp_project_sessions_dir(agent: &str, project_path: &Path) -> Option<PathBuf> {
    Some(
        omp_sessions_dir_for(agent)
            .ok()?
            .join(omp_project_dir_name(project_path)),
    )
}

/// 按 mtime 取最新的会话文件,**不**过滤已认领。会话恢复入口用它。
pub(crate) fn discover_omp_session_since(
    agent: &str,
    project_path: &Path,
    since_ms: u128,
) -> Option<(String, PathBuf)> {
    let dir = omp_project_sessions_dir(agent, project_path)?;
    newest_omp_session_in_dir(&dir, since_ms, None)
}

/// 同上,但跳过已被其他任务认领的路径。watcher 用它。
pub(crate) fn discover_unclaimed_omp_session_since<R: tauri::Runtime>(
    app: &AppHandle<R>,
    agent: &str,
    project_path: &Path,
    since_ms: u128,
) -> Option<(String, PathBuf)> {
    let dir = omp_project_sessions_dir(agent, project_path)?;
    let claimed = {
        let tm = app.state::<TaskManager>();
        let claimed = tm.claimed_session_paths.lock().clone();
        claimed
    };
    newest_omp_session_in_dir(&dir, since_ms, Some(&claimed))
}

/// omp 会话 watcher:会话文件懒创建(首个 assistant 消息才落盘),因此轮询会话
/// 目录直到**自己的**文件出现、任务已被别处认领、或 PTY 进程退出。
///
/// 与 dsh watcher(`session_dsh.rs`)的差别在预算:dsh 是 headless 一次性进程,
/// 文件启动即建,2 分钟固定宽限足够;omp 是交互式 TUI,用户可能空闲很久才发第一
/// 条消息,固定截止会让长会话永远挂不上。所以这里以进程存活为界,并在前 2 分钟
/// 之后把轮询间隔从 500ms 放宽到 5s —— 空闲会话不该每秒扫两次目录。
///
/// 同项目下的多个 omp 任务共用一个会话目录。所以发现要跳过已认领的路径,而认领本身
/// 还可能输给同一瞬间的另一个 watcher —— 那时不能退出,要继续等自己的文件出现。
pub(crate) fn spawn_omp_session_watcher(
    app: AppHandle,
    task_id: String,
    agent: String,
    project_path: PathBuf,
    since_ms: u128,
) {
    thread::spawn(move || {
        for tick in 0u32.. {
            {
                let tm = app.state::<TaskManager>();
                if tm.omp_sessions.lock().contains_key(&task_id) {
                    return;
                }
                if !tm.child_handles.lock().contains_key(&task_id) {
                    // 进程已退出:再发现一次,秒退的短会话也能补挂。这是最后一次机会,
                    // 认领输了也只能收摊 —— 进程都没了,不会再有属于自己的文件出现。
                    if let Some((session_id, path)) =
                        discover_unclaimed_omp_session_since(&app, &agent, &project_path, since_ms)
                    {
                        register_omp_session(&app, &task_id, &session_id, &path);
                    }
                    return;
                }
            }
            if let Some((session_id, path)) =
                discover_unclaimed_omp_session_since(&app, &agent, &project_path, since_ms)
            {
                // 认领成功才收工。失败说明这条路径刚被别的任务拿走,自己的还没落盘。
                if register_omp_session(&app, &task_id, &session_id, &path) {
                    return;
                }
            }
            thread::sleep(if tick < 240 {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(5)
            });
        }
    });
}

/// 读取 omp 会话文件头(`{type:"session",id,...}` 行)的会话 id。
/// 只流式读前 4 行:title 槽固定 256 字节,header 必然在其中,避免整读大文件。
pub(crate) fn read_omp_session_header(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    for line in std::io::BufReader::new(file).lines().take(4) {
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
pub(crate) fn parse_omp_session_lines<S: AsRef<str>>(
    lines: &[S],
) -> Result<Vec<SessionMessage>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// profile 作用域的会话目录必须在放行名单里。
    ///
    /// 用户 shell rc 里 export 了 `PI_PROFILE=sota`,omp 就把会话写到
    /// `~/.omp/profiles/sota/agent/sessions/`。这一支缺失时,历史视图会在
    /// `session.rs` 的 allowed-roots 校验处拒掉真实存在的会话文件——前端只看到
    /// "加载很久后失败",看不出是路径被拒。
    #[test]
    fn profile_scoped_session_dirs_are_allowed_roots() {
        let home = std::env::temp_dir().join(format!("aeroric-omp-roots-{}", uuid::Uuid::new_v4()));
        let profiles = home.join(".omp").join("profiles");
        for name in ["sota", "just", "zzz"] {
            fs::create_dir_all(profiles.join(name).join("agent").join("sessions")).unwrap();
        }
        // 非目录项不得变成 root。
        fs::write(profiles.join("stray.txt"), b"x").unwrap();

        let roots = omp_profile_session_roots(&home);

        for name in ["sota", "just", "zzz"] {
            let expected = profiles.join(name).join("agent").join("sessions");
            assert!(roots.contains(&expected), "missing root for profile {name}");
        }
        assert_eq!(
            roots.len(),
            3,
            "stray file must not become a root: {roots:?}"
        );

        let _ = fs::remove_dir_all(&home);
    }

    /// 没有 profiles 目录时不得 panic,返回空。
    #[test]
    fn absent_profiles_dir_yields_no_roots() {
        let home =
            std::env::temp_dir().join(format!("aeroric-omp-noprof-{}", uuid::Uuid::new_v4()));
        assert!(omp_profile_session_roots(&home).is_empty());
    }

    #[test]
    fn session_id_extracts_from_omp_file_name() {
        let path = Path::new(
            "/home/u/.omp/agent/sessions/-tmp-x/2026-09-06T10-30-00_0198c7a2-7f4a-7d3e-9f2a-3b6c5d4e4f50.jsonl",
        );
        assert_eq!(
            omp_session_id_from_file_name(path).as_deref(),
            Some("0198c7a2-7f4a-7d3e-9f2a-3b6c5d4e4f50")
        );
        assert_eq!(
            omp_session_id_from_file_name(Path::new("garbage.jsonl")),
            None
        );
    }

    fn touch_session(dir: &Path, name: &str, mtime_offset_secs: u64) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).expect("create session");
        // 显式设 mtime 摆出「已认领的那条更新」这个局面。靠写入顺序 + sleep 也能凑出来,
        // 但那样断言就依赖文件系统的时间精度了。
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_000 + mtime_offset_secs);
        file.set_modified(when).expect("set mtime");
        path
    }

    /// **R7 的核心。** 同项目下的第二个 omp 任务不能认领第一个任务的会话文件。
    ///
    /// 只按 mtime 取最新的话,第二个 watcher 会选中已被认领的那条(它更新),然后退出;
    /// 自己的文件随后落盘也不再有人来绑。
    #[test]
    fn discovery_skips_a_session_another_task_already_claimed() {
        let dir = std::env::temp_dir().join(format!("aeroric-omp-claim-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("mkdir");
        let mine = touch_session(&dir, "2026-09-08T10-00-00_mine.jsonl", 10);
        let theirs = touch_session(&dir, "2026-09-08T10-05-00_theirs.jsonl", 20);

        // 不过滤时取到的是更新的那条 —— 也就是别人的。
        assert_eq!(
            newest_omp_session_in_dir(&dir, 0, None).map(|(_, path)| path),
            Some(theirs.clone())
        );

        let claimed: std::collections::HashSet<String> = [theirs.to_string_lossy().into_owned()]
            .into_iter()
            .collect();
        let found = newest_omp_session_in_dir(&dir, 0, Some(&claimed));
        assert_eq!(
            found.as_ref().map(|(_, path)| path.clone()),
            Some(mine),
            "已被认领的那条必须跳过,即使它更新"
        );
        assert_eq!(found.map(|(id, _)| id).as_deref(), Some("mine"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// 全部都被认领时返回 `None` —— watcher 据此继续等,而不是拿别人的顶上。
    #[test]
    fn discovery_yields_nothing_when_every_session_is_claimed() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-omp-allclaim-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("mkdir");
        let only = touch_session(&dir, "2026-09-08T10-00-00_only.jsonl", 10);

        let claimed: std::collections::HashSet<String> =
            [only.to_string_lossy().into_owned()].into_iter().collect();
        assert!(newest_omp_session_in_dir(&dir, 0, Some(&claimed)).is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    /// 恢复入口不过滤:那条路径本来就是它自己的。
    #[test]
    fn recovery_discovery_does_not_filter_claimed_paths() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-omp-recover-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("mkdir");
        let only = touch_session(&dir, "2026-09-08T10-00-00_only.jsonl", 10);

        assert_eq!(
            newest_omp_session_in_dir(&dir, 0, None).map(|(_, path)| path),
            Some(only)
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// 认领一条已属于别人的路径必须返回 `false`,而不是静默成功。
    ///
    /// 静默 `return` 的后果不是「少一次注册」:watcher 拿不到信号,会以为自己绑上了然后
    /// 退出循环,于是自己的会话文件出现之后再也没人来绑。
    #[test]
    fn claiming_a_taken_path_reports_failure() {
        let app = tauri::test::mock_app();
        app.manage(crate::TaskManager {
            pty_masters: parking_lot::Mutex::new(std::collections::HashMap::new()),
            pty_writers: parking_lot::Mutex::new(std::collections::HashMap::new()),
            child_handles: parking_lot::Mutex::new(std::collections::HashMap::new()),
            pending_pty_sizes: parking_lot::Mutex::new(std::collections::HashMap::new()),
            cancelled_tasks: parking_lot::Mutex::new(std::collections::HashSet::new()),
            manually_completed_tasks: parking_lot::Mutex::new(std::collections::HashSet::new()),
            codex_sessions: parking_lot::Mutex::new(std::collections::HashMap::new()),
            claude_sessions: parking_lot::Mutex::new(std::collections::HashMap::new()),
            dsh_sessions: parking_lot::Mutex::new(std::collections::HashMap::new()),
            omp_sessions: parking_lot::Mutex::new(std::collections::HashMap::new()),
            claimed_session_paths: parking_lot::Mutex::new(std::collections::HashSet::new()),
            initial_input_signals: std::sync::Arc::new(parking_lot::Mutex::new(
                std::collections::HashMap::new(),
            )),
            wsl_active_ids: parking_lot::Mutex::new(std::collections::HashSet::new()),
            codex_rpc: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        });
        let handle = app.handle().clone();
        let path = Path::new("/tmp/aeroric-omp-shared/2026-09-08T10-00-00_shared.jsonl");

        assert!(
            register_omp_session(&handle, "task-one", "shared", path),
            "第一个任务应当认领成功"
        );
        assert!(
            !register_omp_session(&handle, "task-two", "shared", path),
            "同一条路径不能被第二个任务再认领一次"
        );

        let tm = handle.state::<crate::TaskManager>();
        assert!(tm.omp_sessions.lock().contains_key("task-one"));
        assert!(
            !tm.omp_sessions.lock().contains_key("task-two"),
            "认领失败的任务不该留下会话绑定"
        );
    }

    #[test]
    fn project_dir_encoding_matches_omp_rules() {
        let root = std::env::temp_dir().join(format!("omp-enc-{}", std::process::id()));
        let project = root.join("同步 demo");
        fs::create_dir_all(&project).unwrap();
        // temp 内 → "-tmp<relative>":仅 / \ : 替换为 "-",空格保留,前缀无附加分隔符
        // (对应 omp encodeRelativeSessionDirName("-tmp", relative))。
        let expected = format!("-tmpomp-enc-{}-同步 demo", std::process::id());
        assert_eq!(omp_project_dir_name(&project), expected);
        // temp 根目录本身(tempRelative === "")→ 裸 "-tmp"。
        assert_eq!(omp_project_dir_name(&std::env::temp_dir()), "-tmp");
        fs::remove_dir_all(&root).ok();
    }

    fn omp_line(json: serde_json::Value) -> String {
        json.to_string()
    }

    #[test]
    fn parses_user_assistant_toolresult_entries() {
        let lines = vec![
            // 首行 title 槽(带空格填充)与 header 行都要跳过。
            omp_line(serde_json::json!({"type":"title","v":1,"title":"demo"})) + "          ",
            omp_line(
                serde_json::json!({"type":"session","version":3,"id":"0198-omp","timestamp":"2026-09-06T00:00:00.000Z","cwd":"/tmp/demo"}),
            ),
            omp_line(serde_json::json!({
                "type":"message","id":"m1","parentId":null,"timestamp":"2026-09-06T00:00:01.000Z",
                "message":{"role":"user","content":"hello omp","timestamp":1}
            })),
            omp_line(serde_json::json!({
                "type":"message","id":"m2","parentId":"m1","timestamp":"2026-09-06T00:00:02.000Z",
                "message":{"role":"assistant","content":[
                    {"type":"thinking","thinking":"think hard"},
                    {"type":"text","text":"hi there"},
                    {"type":"toolCall","id":"call-1","name":"bash","arguments":"{\"cmd\":\"ls\"}"}
                ],"stopReason":"toolUse","usage":{"input":10,"output":5},"timestamp":2}
            })),
            omp_line(serde_json::json!({
                "type":"message","id":"m3","parentId":"m2","timestamp":"2026-09-06T00:00:03.000Z",
                "message":{"role":"toolResult","toolCallId":"call-1","toolName":"bash",
                    "content":[{"type":"text","text":"file.txt"}],"isError":false,"timestamp":3}
            })),
            // 非 message entry(compaction)与空行跳过。
            omp_line(serde_json::json!({"type":"compaction","id":"c1","parentId":"m3"})),
            "   ".to_string(),
        ];
        let messages = parse_omp_session_lines(&lines).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert!(
            matches!(&messages[0].content[0], SessionContent::Text { text } if text == "hello omp")
        );
        assert_eq!(messages[1].message_id.as_deref(), Some("m2"));
        assert!(
            matches!(&messages[1].content[0], SessionContent::Thinking { thinking } if thinking == "think hard")
        );
        assert!(
            matches!(&messages[1].content[1], SessionContent::Text { text } if text == "hi there")
        );
        assert!(
            matches!(&messages[1].content[2], SessionContent::ToolUse { name, id, .. } if name == "bash" && id == "call-1")
        );
        assert!(
            matches!(&messages[2].content[0], SessionContent::ToolResult { id, output } if id == "call-1" && output == "file.txt")
        );
    }

    #[test]
    fn parses_user_content_arrays_with_images() {
        let lines = vec![omp_line(serde_json::json!({
            "type":"message","id":"m1","parentId":null,
            "message":{"role":"user","content":[
                {"type":"text","text":"look at this"},
                {"type":"image","data":"QUJD","mimeType":"image/jpeg"}
            ]}
        }))];

        let messages = parse_omp_session_lines(&lines).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.len(), 2);
        assert!(
            matches!(&messages[0].content[0], SessionContent::Text { text } if text == "look at this")
        );
        assert!(
            matches!(&messages[0].content[1], SessionContent::Attachment { media_type, .. } if media_type == "image/jpeg")
        );
    }

    #[test]
    fn reads_session_header_id_and_ignores_title_slot() {
        let dir = std::env::temp_dir().join(format!("omp-hdr-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2026-09-06T00-00-00_0198c7a2-0000-7d3e-9f2a-3b6c5d4e4f50.jsonl");
        let title_line = r#"{"type":"title","v":1,"title":"t"}"#;
        let padded = format!("{title_line:<256}");
        fs::write(
            &path,
            format!(
                "{padded}\n{}\n",
                omp_line(serde_json::json!({"type":"session","version":3,"id":"0198c7a2-0000-7d3e-9f2a-3b6c5d4e4f50"}))
            ),
        )
        .unwrap();
        assert_eq!(
            read_omp_session_header(&path).as_deref(),
            Some("0198c7a2-0000-7d3e-9f2a-3b6c5d4e4f50")
        );
        fs::remove_dir_all(&dir).ok();
    }
}
