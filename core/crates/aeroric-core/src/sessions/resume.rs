//! 会话恢复层（P3-04 第五片）：native resume 判定、会话 id 解析与按时间/prompt 兜底恢复。
//!
//! 逐字迁移自宿主 `src-tauri/src/session.rs` 的 `session_file_supports_native_resume`、
//! `ensure_session_can_be_adopted`、`resolve_session_id_from_file`、`is_uuid_like`、
//! `session_started_at_ms`、`session_matches_project`、`first_user_message`、
//! `recover_session` 与 `RecoveredSession`，行为一行未改。两处刻意的形状变化：
//! ① `RecoveredSession` 字段升 `pub`（宿主命令在迁移后仍要构造，serde `camelCase`
//! 输出逐字节不变）；② `recover_session` 的 `home` 由调用方注入——core 不碰全局
//! home（`core-discovers-no-real-home`），宿主把自己解析出的用户目录注入 `home` 参数，
//! 与第二片发现根的注入是同一关系（补记二百四十二）。
//!
//! 引用解析阶梯（family 字段 → 按 path 读 session id → legacy 字段/path → recover）
//! 的前三跳属宿主/前端；core 只保留这两条判定：**解析不出 session 就拒绝续跑**
//! （[`resolve_session_id_from_file`] 返 `None`、[`ensure_session_can_be_adopted`]
//! 抛既有错误文案）与「paginated Codex 不能收养」。P2-04 移交（Q43）的判据落点。

use crate::sessions::discovery::{
    claude_sessions_dirs_for_project, codex_sessions_roots, collect_jsonl_files,
    collect_session_files_from_roots,
};
use crate::sessions::parser::{parse_session_messages, SESSION_FORMAT_DETECTION_LINES};
use crate::sessions::SessionContent;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredSession {
    pub session_id: String,
    pub session_path: String,
}

/// Codex app-server 目前无法原生恢复 `history_mode = "paginated"` 的会话。
/// 只扫描头部格式探测窗口：`session_meta` 是 Codex rollout 的首部记录，
/// 而旧版文件可能没有 `history_mode`，这两种情况都保持历史的原生 resume 行为。
pub fn session_file_supports_native_resume(path: &Path, is_codex: bool) -> Result<bool, String> {
    if !is_codex {
        return Ok(true);
    }

    let file = File::open(path).map_err(|error| error.to_string())?;
    for line in BufReader::new(file)
        .lines()
        .take(SESSION_FORMAT_DETECTION_LINES)
    {
        let line = line.map_err(|error| error.to_string())?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
            continue;
        }
        let history_mode = value
            .get("payload")
            .and_then(|payload| payload.get("history_mode"))
            .and_then(serde_json::Value::as_str);
        return Ok(!history_mode.is_some_and(|mode| mode.eq_ignore_ascii_case("paginated")));
    }

    Ok(true)
}

pub fn ensure_session_can_be_adopted(path: &Path, is_codex: bool) -> Result<(), String> {
    if session_file_supports_native_resume(path, is_codex)? {
        return Ok(());
    }
    Err("Codex paginated sessions cannot be adopted for native resume yet".to_string())
}

pub fn is_uuid_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts
            .iter()
            .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn resolve_session_id_from_file(path: &Path, is_codex: bool) -> Option<String> {
    let file = File::open(path).ok()?;
    for line in BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take(SESSION_FORMAT_DETECTION_LINES)
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if is_codex {
            let event_type = value.get("type").and_then(|v| v.as_str());
            if event_type == Some("session_meta") {
                if let Some(id) = value
                    .get("payload")
                    .and_then(|p| p.get("id"))
                    .and_then(|v| v.as_str())
                    .filter(|id| !id.trim().is_empty())
                {
                    return Some(id.to_string());
                }
            }
        } else if let Some(id) = value
            .get("sessionId")
            .or_else(|| value.get("session_id"))
            .and_then(|v| v.as_str())
            .filter(|id| !id.trim().is_empty())
        {
            return Some(id.to_string());
        }
    }

    let stem = path.file_stem().and_then(|name| name.to_str())?;
    if !is_codex && is_uuid_like(stem) {
        return Some(stem.to_string());
    }

    if is_codex {
        if let Some(rest) = stem.strip_prefix("rollout-") {
            if let Some(candidate) = rest.get(20..) {
                if is_uuid_like(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }
        if is_uuid_like(stem) {
            return Some(stem.to_string());
        }
    }

    None
}

pub fn session_started_at_ms(path: &Path) -> Option<i64> {
    let file = File::open(path).ok()?;
    for line in BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take(SESSION_FORMAT_DETECTION_LINES)
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let timestamp = value
            .get("timestamp")
            .or_else(|| {
                value
                    .get("payload")
                    .and_then(|payload| payload.get("timestamp"))
            })
            .and_then(serde_json::Value::as_str);
        if let Some(timestamp) = timestamp {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(timestamp) {
                return Some(parsed.timestamp_millis());
            }
        }
    }

    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
}

pub fn session_matches_project(path: &Path, project_path: &str, is_codex: bool) -> bool {
    if !is_codex {
        return true;
    }
    let expected = Path::new(project_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(project_path));
    let Ok(file) = File::open(path) else {
        return false;
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take(SESSION_FORMAT_DETECTION_LINES)
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .filter_map(|value| {
            value
                .get("payload")
                .and_then(|payload| payload.get("cwd"))
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
        })
        .any(|cwd| cwd.canonicalize().unwrap_or(cwd) == expected)
}

pub fn first_user_message(path: &Path, is_codex: bool) -> Option<String> {
    let lines = BufReader::new(File::open(path).ok()?)
        .lines()
        .map_while(Result::ok)
        .take(2_000)
        .collect::<Vec<_>>();
    parse_session_messages(&lines, is_codex)
        .into_iter()
        .find(|message| message.role == "user")
        .and_then(|message| {
            message
                .content
                .into_iter()
                .find_map(|content| match content {
                    SessionContent::Text { text } if !text.trim().is_empty() => Some(text),
                    _ => None,
                })
        })
}

/// 按创建时间（±15 分钟窗）与首条用户消息（`contains` 匹配）兜底恢复会话。
/// `home` 由宿主注入（同第二片）；roots 规则复用 [`crate::sessions::discovery`]。
pub fn recover_session(
    home: Option<&Path>,
    project_path: &str,
    prompt: &str,
    created_at: i64,
    is_codex: bool,
) -> Option<RecoveredSession> {
    let mut files = if is_codex {
        collect_session_files_from_roots(&codex_sessions_roots(home, project_path))
    } else {
        let mut files = Vec::new();
        for root in claude_sessions_dirs_for_project(home, project_path) {
            collect_jsonl_files(&root, &mut files);
        }
        files
    };
    let normalized_prompt = prompt.trim();
    files.retain(|path| session_matches_project(path, project_path, is_codex));

    files
        .into_iter()
        .filter_map(|path| {
            let started_at = session_started_at_ms(&path)?;
            let delta = started_at.abs_diff(created_at);
            if delta > 15 * 60 * 1_000 {
                return None;
            }
            let prompt_match = if normalized_prompt.is_empty() {
                true
            } else {
                first_user_message(&path, is_codex)
                    .map(|message| message.trim().contains(normalized_prompt))
                    .unwrap_or(false)
            };
            if !prompt_match {
                return None;
            }
            let session_id = resolve_session_id_from_file(&path, is_codex)?;
            Some((
                delta,
                RecoveredSession {
                    session_id,
                    session_path: path.to_string_lossy().into_owned(),
                },
            ))
        })
        .min_by_key(|(delta, _)| *delta)
        .map(|(_, recovered)| recovered)
}
