//! 其他 Agent 的导入源（2026-10-28）：DeepSeek Harness、OMP、Qoder、Cursor（CLI 与
//! 编辑器）、OpenCode。
//!
//! 与 `import.rs` 的分工：那边管授权、索引与 Claude / Codex；这里只回答三件事——
//! 某家的会话**在哪**（按各家自己的环境变量与发行变体解析，不写死本机路径）、
//! **一个会话的摘要**（标题 / cwd / 时间）、**一个会话的消息**。全部只读：文件只
//! 打开读取，SQLite 以 `SQLITE_OPEN_READ_ONLY` 打开并设 busy_timeout，不建表、不写入。
//!
//! 规则借自 `harness/codeg/src-tauri/src/parsers/*.rs`（只借规则，不搬代码）。

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use md5::{Digest as _, Md5};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

use super::import::{ImportAgent, SourceMarker};
use crate::sessions::discovery::collect_jsonl_files;
use crate::sessions::{SessionContent, SessionMessage};

/// 宿主注入的发现环境：home 与环境变量快照（测试可伪造，core 不读真实进程环境）。
#[derive(Clone, Debug, Default)]
pub struct DiscoveryEnv {
    pub home: PathBuf,
    pub vars: BTreeMap<String, String>,
}

impl DiscoveryEnv {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            vars: BTreeMap::new(),
        }
    }

    /// 只取各家解析规则用得到的变量。
    pub fn with_process_vars(home: &Path) -> Self {
        let mut env = Self::new(home);
        for key in KNOWN_VARS {
            if let Ok(value) = std::env::var(key) {
                env.vars.insert((*key).to_string(), value);
            }
        }
        env
    }

    fn var(&self, key: &str) -> Option<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// `~` / `~/…` 展开到注入的 home。
    fn path_var(&self, key: &str) -> Option<PathBuf> {
        let value = self.var(key)?;
        Some(match value.strip_prefix('~') {
            Some(rest) => self.home.join(rest.trim_start_matches('/')),
            None => PathBuf::from(value),
        })
    }
}

const KNOWN_VARS: &[&str] = &[
    "DSH_HOME",
    "QODER_CONFIG_DIR",
    "QODER_CONFIG_DIR_NAME",
    "CURSOR_CONFIG_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
];

/// Qoder 各发行版的用户目录名（国际版、国内版）；`QODER_CONFIG_DIR_NAME` 另加。
const QODER_VARIANTS: &[&str] = &[".qoder", ".qoder-cn"];

/// 一家 Agent 的候选根目录：第一项是主根（未发现时也用于展示），其余为同一家的
/// 其他存放位置（Qoder 的发行变体、Cursor 的编辑器目录）。只返回存在的附加根。
pub fn resolve_roots(agent: ImportAgent, env: &DiscoveryEnv) -> Vec<PathBuf> {
    let home = &env.home;
    let mut roots = match agent {
        ImportAgent::Claude => vec![home.join(".claude")],
        ImportAgent::Codex => vec![home.join(".codex")],
        ImportAgent::Dsh => vec![env
            .path_var("DSH_HOME")
            .unwrap_or_else(|| home.join(".dsh"))],
        ImportAgent::Omp => vec![home.join(".omp")],
        ImportAgent::Qoder => {
            if let Some(dir) = env.path_var("QODER_CONFIG_DIR") {
                vec![dir]
            } else {
                let mut names: Vec<String> = QODER_VARIANTS.iter().map(|s| s.to_string()).collect();
                if let Some(name) = env.var("QODER_CONFIG_DIR_NAME") {
                    names.insert(0, name.to_string());
                }
                names.dedup();
                names.iter().map(|name| home.join(name)).collect()
            }
        }
        ImportAgent::Cursor => {
            let cli = env
                .path_var("CURSOR_CONFIG_DIR")
                .or_else(|| {
                    env.path_var("XDG_CONFIG_HOME")
                        .map(|dir| dir.join("cursor"))
                })
                .unwrap_or_else(|| home.join(".cursor"));
            vec![
                cli,
                home.join("Library/Application Support/Cursor/User"),
                home.join(".config/Cursor/User"),
            ]
        }
        ImportAgent::OpenCode => vec![env
            .path_var("XDG_DATA_HOME")
            .unwrap_or_else(|| home.join(".local/share"))
            .join("opencode")],
    };
    let primary = roots.remove(0);
    let mut out = vec![primary.clone()];
    out.extend(roots.into_iter().filter(|root| root.is_dir()));
    // A variant that has sessions outranks a primary that has none (only the
    // Qoder China edition installed, only the Cursor editor); the empty primary
    // is then dropped.
    if !has_sessions(agent, &primary) {
        if let Some(index) = out.iter().position(|root| has_sessions(agent, root)) {
            out.swap(0, index);
            let first = out.remove(0);
            out.retain(|root| root.is_dir());
            out.insert(0, first);
        }
    }
    out.dedup();
    out
}

/// 根下是否有这家的会话存放处（`estimate.rootExists`）。
pub fn has_sessions(agent: ImportAgent, root: &Path) -> bool {
    match agent {
        ImportAgent::Claude | ImportAgent::Qoder => root.join("projects").is_dir(),
        ImportAgent::Codex => {
            root.join("sessions").is_dir() || root.join("archived_sessions").is_dir()
        }
        ImportAgent::Dsh => root.join("sessions").is_dir(),
        ImportAgent::Omp => !omp_session_dirs(root).is_empty(),
        ImportAgent::Cursor => {
            cursor_ide_db(root).is_file()
                || root.join("chats").is_dir()
                || root.join("acp-sessions").is_dir()
        }
        ImportAgent::OpenCode => root.join("opencode.db").is_file(),
    }
}

/// 一个会话的定位与（数据库源）预先取得的摘要。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    /// 同一数据库里的哪一行（OpenCode 会话 id、Cursor 编辑器 composerId）。
    pub key: Option<String>,
    /// 数据库行按行内更新时间算；文件源为空（由文件元数据算）。
    pub marker: Option<SourceMarker>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub first_at: Option<String>,
    /// OMP 配置档名（`profiles/<name>`）。
    pub profile: Option<String>,
    /// 来源 Agent 自己已归档。
    pub archived: bool,
}

impl Source {
    fn file(path: PathBuf) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }
}

/// 新增几家的会话清单。Claude / Codex 仍走 `import::collect_sources`。
pub fn collect(agent: ImportAgent, root: &Path) -> Vec<Source> {
    let mut sources = match agent {
        ImportAgent::Dsh => collect_dsh(root),
        ImportAgent::Omp => collect_omp(root),
        ImportAgent::Qoder => {
            let mut files = Vec::new();
            collect_jsonl_files(&root.join("projects"), &mut files);
            files.retain(|path| !path.components().any(|c| c.as_os_str() == "subagents"));
            files.into_iter().map(Source::file).collect()
        }
        ImportAgent::Cursor => {
            if cursor_ide_db(root).is_file() {
                collect_cursor_ide(root)
            } else {
                collect_cursor_cli(root)
            }
        }
        ImportAgent::OpenCode => collect_opencode(root),
        ImportAgent::Claude | ImportAgent::Codex => Vec::new(),
    };
    sources.sort_by(|a, b| a.path.cmp(&b.path).then(a.key.cmp(&b.key)));
    sources
}

/// 摘要；`Ok(None)` = 不是独立会话（子代理、空会话），跳过而不报错。
#[derive(Default)]
pub struct Summary {
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub first_at: Option<String>,
}

pub fn summarize(
    agent: ImportAgent,
    source: &Source,
    projects: &[(String, String)],
) -> Result<Option<Summary>, String> {
    match agent {
        ImportAgent::Dsh => dsh_summary(&source.path),
        ImportAgent::Omp => omp_summary(&source.path),
        ImportAgent::Cursor if source.key.is_none() => cursor_cli_summary(&source.path, projects),
        _ => Ok(Some(Summary {
            cwd: source.cwd.clone(),
            title: source.title.clone(),
            first_at: source.first_at.clone(),
        })),
    }
}

/// 读一个会话的消息（Qoder 与 Claude 同格式，由 `import.rs` 处理）。
pub fn messages(
    agent: ImportAgent,
    path: &Path,
    key: Option<&str>,
) -> Result<Vec<SessionMessage>, String> {
    match (agent, key) {
        (ImportAgent::Dsh, _) => {
            let lines = crate::sessions::dsh::read_dsh_session_lines(path)?;
            crate::sessions::dsh::parse_dsh_session_lines(&lines)
        }
        (ImportAgent::Omp, _) => {
            let (lines, _) = crate::sessions::history::read_session_tail(path)?;
            crate::sessions::omp::parse_omp_session_lines(&lines)
        }
        (ImportAgent::Cursor, None) => cursor_cli_messages(path),
        (ImportAgent::Cursor, Some(id)) => cursor_ide_messages(path, id),
        (ImportAgent::OpenCode, Some(id)) => opencode_messages(path, id),
        _ => Err("unsupported import source".to_string()),
    }
}

// ── 共用 ──────────────────────────────────────────────────────────────────

const TITLE_CHARS: usize = 80;

fn first_line(text: &str) -> Option<String> {
    let line = text.trim().lines().next()?.trim();
    (!line.is_empty()).then(|| line.chars().take(TITLE_CHARS).collect())
}

fn iso_ms(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn file_mtime_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 只读连接：不建库、不迁移；对方正在写时等一会儿而不是报 busy。
/// A key-value column may hold its JSON as TEXT or as BLOB (`cursorDiskKV`).
fn text_at(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(index)? {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => String::from_utf8(bytes.to_vec()).ok(),
        _ => None,
    })
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn text_items(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.as_str()),
                _ => item.get("text").and_then(Value::as_str),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn text_message(role: &str, text: String) -> Option<SessionMessage> {
    (!text.trim().is_empty()).then(|| SessionMessage {
        role: role.to_string(),
        content: vec![SessionContent::Text { text }],
        message_id: None,
    })
}

// ── DeepSeek Harness ─────────────────────────────────────────────────────

/// `sessions/<projectKey>/<sessionDir>/session(.vN).jsonl(.zstd)`。
fn collect_dsh(root: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    let Ok(projects) = fs::read_dir(root.join("sessions")) else {
        return out;
    };
    for project in projects.flatten().filter(|e| e.path().is_dir()) {
        let Ok(sessions) = fs::read_dir(project.path()) else {
            continue;
        };
        for session in sessions.flatten().filter(|e| e.path().is_dir()) {
            if let Some(path) = crate::sessions::dsh::dsh_transcript_in(&session.path()) {
                out.push(Source::file(path));
            }
        }
    }
    out
}

/// 头部：header 的 cwd / createdAt / delegationDepth，标题优先 `session/title`，
/// 否则首条真用户消息（`source.kind == "user"`）。只流式读到够用为止。
fn dsh_summary(path: &Path) -> Result<Option<Summary>, String> {
    let lines = crate::sessions::dsh::read_dsh_head_lines(path, 4_000)?;
    let mut summary = Summary::default();
    let mut user_title = None;
    for (index, line) in lines.iter().enumerate() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let data = value.get("data");
        if index == 0 && kind == "session" {
            if value
                .get("delegationDepth")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
            {
                return Ok(None);
            }
            summary.cwd = value.get("cwd").and_then(Value::as_str).map(str::to_string);
            summary.first_at = value
                .get("createdAt")
                .and_then(Value::as_i64)
                .and_then(iso_ms);
            continue;
        }
        match kind {
            "session/title" => {
                if let Some(title) = data
                    .and_then(|d| d.get("title"))
                    .and_then(Value::as_str)
                    .and_then(first_line)
                {
                    summary.title = Some(title);
                }
            }
            "user/message" if user_title.is_none() => {
                let real = data
                    .and_then(|d| d.pointer("/source/kind"))
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind == "user");
                if real {
                    user_title = first_line(&text_items(data.and_then(|d| d.get("content"))));
                }
            }
            _ => {}
        }
        if summary.title.is_some() {
            break;
        }
    }
    summary.title = summary.title.or(user_title);
    Ok(Some(summary))
}

// ── OMP ──────────────────────────────────────────────────────────────────

/// `<root>/agent/sessions` 与每个 `<root>/profiles/<name>/agent/sessions`。
fn omp_session_dirs(root: &Path) -> Vec<(Option<String>, PathBuf)> {
    let mut dirs = Vec::new();
    let default = root.join("agent").join("sessions");
    if default.is_dir() {
        dirs.push((None, default));
    }
    if let Ok(entries) = fs::read_dir(root.join("profiles")) {
        for entry in entries.flatten() {
            let dir = entry.path().join("agent").join("sessions");
            if dir.is_dir() {
                dirs.push((Some(entry.file_name().to_string_lossy().into_owned()), dir));
            }
        }
    }
    dirs
}

fn collect_omp(root: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    for (profile, dir) in omp_session_dirs(root) {
        let mut files = Vec::new();
        collect_jsonl_files(&dir, &mut files);
        out.extend(files.into_iter().map(|path| Source {
            profile: profile.clone(),
            ..Source::file(path)
        }));
    }
    out
}

/// 首行 title 槽、次行 header（cwd、timestamp、title）；都没有标题时取首条用户消息。
fn omp_summary(path: &Path) -> Result<Option<Summary>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut summary = Summary::default();
    let mut slot_title = None;
    let mut is_session = false;
    for line in BufReader::new(file).lines().take(400) {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("title") => {
                slot_title = value
                    .get("title")
                    .and_then(Value::as_str)
                    .and_then(first_line)
            }
            Some("session") => {
                is_session = true;
                summary.cwd = value.get("cwd").and_then(Value::as_str).map(str::to_string);
                summary.first_at = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                summary.title = value
                    .get("title")
                    .and_then(Value::as_str)
                    .and_then(first_line);
            }
            Some("message") if summary.title.is_none() && slot_title.is_none() => {
                let message = value.get("message");
                if message.and_then(|m| m.get("role")).and_then(Value::as_str) == Some("user") {
                    summary.title = first_line(&text_items(message.and_then(|m| m.get("content"))));
                }
            }
            _ => {}
        }
        if is_session && (summary.title.is_some() || slot_title.is_some()) {
            break;
        }
    }
    if !is_session {
        return Ok(None);
    }
    summary.title = slot_title.or(summary.title);
    Ok(Some(summary))
}

// ── Cursor ───────────────────────────────────────────────────────────────

fn cursor_ide_db(root: &Path) -> PathBuf {
    root.join("globalStorage").join("state.vscdb")
}

/// 数据库文件与它的 WAL 一起算：WAL 里的新写入不改主文件 mtime。
fn db_marker(path: &Path) -> SourceMarker {
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    SourceMarker {
        mtime_ms: file_mtime_ms(path).max(file_mtime_ms(&wal)),
        size: size(path) + size(&wal),
    }
}

/// CLI：`chats/<md5(cwd)>/<uuid>/store.db` 与 `acp-sessions/<uuid>/store.db`。
fn collect_cursor_cli(root: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    if let Ok(groups) = fs::read_dir(root.join("chats")) {
        for group in groups.flatten() {
            let Ok(chats) = fs::read_dir(group.path()) else {
                continue;
            };
            for chat in chats.flatten() {
                let db = chat.path().join("store.db");
                if db.is_file() {
                    out.push(Source {
                        marker: Some(db_marker(&db)),
                        ..Source::file(db)
                    });
                }
            }
        }
    }
    if let Ok(sessions) = fs::read_dir(root.join("acp-sessions")) {
        for session in sessions.flatten() {
            let db = session.path().join("store.db");
            if db.is_file() {
                let meta: Option<Value> = fs::read(session.path().join("meta.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok());
                out.push(Source {
                    marker: Some(db_marker(&db)),
                    cwd: meta
                        .as_ref()
                        .and_then(|m| m.get("cwd"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    title: meta
                        .as_ref()
                        .and_then(|m| m.get("title"))
                        .and_then(Value::as_str)
                        .and_then(first_line),
                    ..Source::file(db)
                });
            }
        }
    }
    out
}

/// `meta` 键 `"0"` 是 hex 编码的 JSON（name、createdAt、latestRootBlobId…）。
fn cursor_cli_meta(conn: &Connection) -> Option<Value> {
    let hex_text: String = conn
        .query_row("SELECT value FROM meta WHERE key = '0'", [], |row| {
            row.get(0)
        })
        .optional()
        .ok()??;
    let bytes = hex::decode(hex_text.trim()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn md5_hex(text: &str) -> String {
    hex::encode(Md5::digest(text.as_bytes()))
}

fn cursor_cli_summary(
    path: &Path,
    projects: &[(String, String)],
) -> Result<Option<Summary>, String> {
    let conn = open_readonly(path)?;
    let meta = cursor_cli_meta(&conn).unwrap_or(Value::Null);
    if meta.get("subagentInfo").is_some_and(|info| !info.is_null()) {
        return Ok(None);
    }
    // chats/<md5(resolve(cwd))>/…：cwd 本身不存，按已打开项目的路径反查。
    let cwd = path
        .parent()
        .and_then(Path::parent)
        .and_then(|group| group.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .and_then(|hash| {
            projects
                .iter()
                .find(|(_, project)| md5_hex(project.trim_end_matches('/')) == hash)
                .map(|(_, project)| project.clone())
        });
    let mut summary = Summary {
        cwd,
        title: meta
            .get("name")
            .and_then(Value::as_str)
            .and_then(first_line),
        first_at: meta
            .get("createdAt")
            .and_then(Value::as_i64)
            .and_then(iso_ms),
    };
    if summary.title.is_none() {
        summary.title = cursor_cli_messages_with(&conn, &meta)
            .ok()
            .and_then(|messages| messages.into_iter().find(|m| m.role == "user"))
            .and_then(|m| match m.content.first() {
                Some(SessionContent::Text { text }) => first_line(text),
                _ => None,
            });
    }
    Ok(Some(summary))
}

fn cursor_cli_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let conn = open_readonly(path)?;
    let meta = cursor_cli_meta(&conn).unwrap_or(Value::Null);
    cursor_cli_messages_with(&conn, &meta)
}

/// 根 blob 是 protobuf：字段 1 重复出现，每个是 32 字节的子 blob id（按对话顺序）；
/// 子 blob 中 JSON 形式的 `{role, content}` 即消息。没有根时按 rowid 顺序读全部 JSON blob。
fn cursor_cli_messages_with(
    conn: &Connection,
    meta: &Value,
) -> Result<Vec<SessionMessage>, String> {
    let blob = |id: &str| -> Option<Vec<u8>> {
        conn.query_row("SELECT data FROM blobs WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .ok()?
    };
    let mut ordered: Vec<Vec<u8>> = Vec::new();
    if let Some(root) = meta
        .get("latestRootBlobId")
        .and_then(Value::as_str)
        .and_then(blob)
    {
        for id in protobuf_child_ids(&root) {
            if let Some(data) = blob(&hex::encode(id)) {
                ordered.push(data);
            }
        }
    }
    if ordered.is_empty() {
        let mut stmt = conn
            .prepare("SELECT data FROM blobs ORDER BY rowid")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|e| e.to_string())?;
        ordered.extend(rows.flatten());
    }
    let mut messages = Vec::new();
    for data in ordered {
        let Ok(value) = serde_json::from_slice::<Value>(&data) else {
            continue;
        };
        let role = value.get("role").and_then(Value::as_str).unwrap_or("");
        let content = value.get("content");
        match role {
            "user" => {
                let text = text_items(content);
                // The CLI wraps the prompt in <user_query>…</user_query>.
                let text = text
                    .split("<user_query>")
                    .nth(1)
                    .and_then(|rest| rest.split("</user_query>").next())
                    .map(str::to_string)
                    .unwrap_or(text);
                messages.extend(text_message("user", text.trim().to_string()));
            }
            "assistant" => {
                let mut parts = Vec::new();
                if let Some(Value::Array(items)) = content {
                    for item in items {
                        match item.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                if let Some(text) = item.get("text").and_then(Value::as_str) {
                                    parts.push(SessionContent::Text {
                                        text: text.to_string(),
                                    });
                                }
                            }
                            Some("reasoning") => {
                                if let Some(text) = item.get("text").and_then(Value::as_str) {
                                    parts.push(SessionContent::Thinking {
                                        thinking: text.to_string(),
                                    });
                                }
                            }
                            Some("tool-call") => parts.push(SessionContent::ToolUse {
                                id: item
                                    .get("toolCallId")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                name: item
                                    .get("toolName")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool")
                                    .to_string(),
                                input: item.get("args").map(Value::to_string).unwrap_or_default(),
                            }),
                            _ => {}
                        }
                    }
                } else if let Some(text) = content.and_then(Value::as_str) {
                    parts.push(SessionContent::Text {
                        text: text.to_string(),
                    });
                }
                if !parts.is_empty() {
                    messages.push(SessionMessage {
                        role: "assistant".to_string(),
                        content: parts,
                        message_id: value.get("id").and_then(Value::as_str).map(str::to_string),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(messages)
}

/// protobuf 里字段 1、长度 32 的定长字节串（子 blob id）。
fn protobuf_child_ids(bytes: &[u8]) -> Vec<&[u8]> {
    let mut ids = Vec::new();
    let mut at = 0;
    let varint = |at: &mut usize| -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *bytes.get(*at)?;
            *at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    };
    while at < bytes.len() {
        let Some(tag) = varint(&mut at) else { break };
        match tag & 7 {
            0 => {
                if varint(&mut at).is_none() {
                    break;
                }
            }
            1 => at += 8,
            5 => at += 4,
            2 => {
                let Some(len) = varint(&mut at) else { break };
                let end = at + len as usize;
                if end > bytes.len() {
                    break;
                }
                if tag >> 3 == 1 && len == 32 {
                    ids.push(&bytes[at..end]);
                }
                at = end;
            }
            _ => break,
        }
    }
    ids
}

/// 编辑器：`globalStorage/state.vscdb` 的 `cursorDiskKV` 里每个 `composerData:<id>` 是一次对话；
/// cwd 来自 `workspaceStorage/<hash>/workspace.json` 的 folder 与该工作区
/// `state.vscdb` 里 `composer.composerData.allComposers` 的对应关系。
fn collect_cursor_ide(root: &Path) -> Vec<Source> {
    let db = cursor_ide_db(root);
    let Ok(conn) = open_readonly(&db) else {
        return Vec::new();
    };
    let folders = cursor_workspace_folders(root);
    let Ok(mut stmt) =
        conn.prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")
    else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, text_at(row, 1)?)))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, value) in rows.flatten() {
        let Some(value) = value.and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
            continue;
        };
        let id = value
            .get("composerId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| key.trim_start_matches("composerData:").to_string());
        let turns = value
            .get("fullConversationHeadersOnly")
            .or_else(|| value.get("conversation"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if turns == 0 {
            continue;
        }
        let created = value.get("createdAt").and_then(Value::as_i64);
        let updated = value
            .get("lastUpdatedAt")
            .and_then(Value::as_i64)
            .or(created)
            .unwrap_or(0);
        out.push(Source {
            path: db.clone(),
            marker: Some(SourceMarker {
                mtime_ms: updated.max(0) as u64,
                size: turns as u64,
            }),
            title: value
                .get("name")
                .and_then(Value::as_str)
                .and_then(first_line),
            cwd: folders.get(&id).cloned(),
            first_at: created.and_then(iso_ms),
            archived: value.get("isArchived").and_then(Value::as_bool) == Some(true),
            key: Some(id),
            profile: None,
        });
    }
    out
}

fn cursor_workspace_folders(root: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Ok(entries) = fs::read_dir(root.join("workspaceStorage")) else {
        return map;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let folder = fs::read(dir.join("workspace.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|v| v.get("folder").and_then(Value::as_str).map(str::to_string))
            .and_then(|uri| file_uri_path(&uri));
        let Some(folder) = folder else { continue };
        let Ok(conn) = open_readonly(&dir.join("state.vscdb")) else {
            continue;
        };
        let data: Option<String> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'composer.composerData'",
                [],
                |row| text_at(row, 0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten();
        let ids = data
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|v| v.get("allComposers").and_then(Value::as_array).cloned())
            .unwrap_or_default();
        for composer in ids {
            if let Some(id) = composer.get("composerId").and_then(Value::as_str) {
                map.insert(id.to_string(), folder.clone());
            }
        }
    }
    map
}

/// `file:///Users/a/b%20c` → `/Users/a/b c`。
fn file_uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// 气泡：`type` 1 = 用户、2 = 助手；新版逐条存 `bubbleId:<composer>:<bubble>`，旧版内联在
/// `conversation[]`。
fn cursor_ide_messages(path: &Path, composer: &str) -> Result<Vec<SessionMessage>, String> {
    let conn = open_readonly(path)?;
    let data: Option<String> = conn
        .query_row(
            "SELECT value FROM cursorDiskKV WHERE key = ?1",
            [format!("composerData:{composer}")],
            |row| text_at(row, 0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    let value: Value = data
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| "Cursor conversation not found".to_string())?;
    let mut bubbles = Vec::new();
    if let Some(headers) = value
        .get("fullConversationHeadersOnly")
        .and_then(Value::as_array)
    {
        for header in headers {
            let Some(bubble) = header.get("bubbleId").and_then(Value::as_str) else {
                continue;
            };
            let text: Option<String> = conn
                .query_row(
                    "SELECT value FROM cursorDiskKV WHERE key = ?1",
                    [format!("bubbleId:{composer}:{bubble}")],
                    |row| text_at(row, 0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .flatten();
            if let Some(bubble) = text.and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                bubbles.push(bubble);
            }
        }
    } else if let Some(inline) = value.get("conversation").and_then(Value::as_array) {
        bubbles.extend(inline.iter().cloned());
    }
    let mut messages = Vec::new();
    for bubble in bubbles {
        let text = bubble
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let role = match bubble.get("type").and_then(Value::as_i64) {
            Some(1) => "user",
            Some(2) => "assistant",
            _ => continue,
        };
        let mut content = Vec::new();
        if let Some(thinking) = bubble.pointer("/thinking/text").and_then(Value::as_str) {
            if !thinking.trim().is_empty() {
                content.push(SessionContent::Thinking {
                    thinking: thinking.to_string(),
                });
            }
        }
        if !text.trim().is_empty() {
            content.push(SessionContent::Text { text });
        }
        if let Some(tool) = bubble.get("toolFormerData").filter(|v| v.is_object()) {
            let id = tool
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            content.push(SessionContent::ToolUse {
                id: id.clone(),
                name: tool
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                input: tool
                    .get("rawArgs")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            });
            if let Some(result) = tool.get("result").and_then(Value::as_str) {
                content.push(SessionContent::ToolResult {
                    id,
                    output: result.to_string(),
                });
            }
        }
        if !content.is_empty() {
            messages.push(SessionMessage {
                role: role.to_string(),
                content,
                message_id: bubble
                    .get("bubbleId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }
    Ok(messages)
}

// ── OpenCode ─────────────────────────────────────────────────────────────

fn has_tables(conn: &Connection, names: &[&str]) -> bool {
    names.iter().all(|name| {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some()
    })
}

/// OpenCode 2 用 `session_v2`；1.x 用 `session`（两者可并存，v2 优先，同 id 不重复）。
/// 只列顶层会话（`parent_id` 为空），子代理会话随父会话显示。
fn collect_opencode(root: &Path) -> Vec<Source> {
    let db = root.join("opencode.db");
    let Ok(conn) = open_readonly(&db) else {
        return Vec::new();
    };
    let mut out: Vec<Source> = Vec::new();
    for (table, archived) in [
        ("session_v2", "time_archived"),
        ("session", "time_archived"),
    ] {
        if !has_tables(&conn, &[table]) {
            continue;
        }
        let archived_col = if column_exists(&conn, table, archived) {
            archived
        } else {
            "NULL"
        };
        let sql = format!(
            "SELECT id, directory, title, time_created, time_updated, {archived_col} FROM {table} WHERE parent_id IS NULL"
        );
        let Ok(mut stmt) = conn.prepare(&sql) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        }) else {
            continue;
        };
        for (id, directory, title, created, updated, archived_at) in rows.flatten() {
            if out
                .iter()
                .any(|known| known.key.as_deref() == Some(id.as_str()))
            {
                continue;
            }
            out.push(Source {
                path: db.clone(),
                marker: Some(SourceMarker {
                    mtime_ms: updated.or(created).unwrap_or(0).max(0) as u64,
                    size: 0,
                }),
                title: title.as_deref().and_then(first_line),
                cwd: directory,
                first_at: created.and_then(iso_ms),
                archived: archived_at.is_some(),
                key: Some(id),
                profile: None,
            });
        }
    }
    out
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    conn.prepare(&format!("SELECT {column} FROM {table} LIMIT 0"))
        .is_ok()
}

fn opencode_messages(path: &Path, session: &str) -> Result<Vec<SessionMessage>, String> {
    let conn = open_readonly(path)?;
    if has_tables(&conn, &["session_message"]) {
        let v2 = opencode_v2_messages(&conn, session)?;
        if !v2.is_empty() {
            return Ok(v2);
        }
    }
    if has_tables(&conn, &["message", "part"]) {
        return opencode_v1_messages(&conn, session);
    }
    Ok(Vec::new())
}

fn opencode_v2_messages(conn: &Connection, session: &str) -> Result<Vec<SessionMessage>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT type, data FROM session_message WHERE session_id = ?1 AND type IN ('user', 'assistant') ORDER BY seq",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    for (kind, data) in rows.flatten() {
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if kind == "user" {
            messages.extend(text_message(
                "user",
                value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ));
            continue;
        }
        let parts = opencode_parts(
            value
                .get("content")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
        if !parts.is_empty() {
            messages.push(SessionMessage {
                role: "assistant".to_string(),
                content: parts,
                message_id: None,
            });
        }
    }
    Ok(messages)
}

/// text / reasoning / tool（`state.input`，结果在 `state.output` 或 `state.content`）。
fn opencode_parts(items: &[Value]) -> Vec<SessionContent> {
    let mut parts = Vec::new();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = item
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.trim().is_empty())
                {
                    parts.push(SessionContent::Text {
                        text: text.to_string(),
                    });
                }
            }
            Some("reasoning") => {
                if let Some(text) = item
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.trim().is_empty())
                {
                    parts.push(SessionContent::Thinking {
                        thinking: text.to_string(),
                    });
                }
            }
            Some("tool") => {
                let id = item
                    .get("id")
                    .or_else(|| item.get("callID"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let name = item
                    .get("name")
                    .or_else(|| item.get("tool"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let state = item.get("state");
                parts.push(SessionContent::ToolUse {
                    id: id.clone(),
                    name,
                    input: state
                        .and_then(|s| s.get("input"))
                        .map(Value::to_string)
                        .unwrap_or_default(),
                });
                let output = state
                    .and_then(|s| s.get("output"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        let text = text_items(state.and_then(|s| s.get("content")));
                        (!text.is_empty()).then_some(text)
                    });
                if let Some(output) = output {
                    parts.push(SessionContent::ToolResult { id, output });
                }
            }
            _ => {}
        }
    }
    parts
}

/// 1.x：`message.data.role` + 按消息归属的 `part.data`。
fn opencode_v1_messages(conn: &Connection, session: &str) -> Result<Vec<SessionMessage>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT m.data, p.data FROM message m LEFT JOIN part p ON p.message_id = m.id \
             WHERE m.session_id = ?1 ORDER BY m.time_created, m.id, p.id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut messages: Vec<SessionMessage> = Vec::new();
    let mut last_message: Option<String> = None;
    for (message, part) in rows.flatten() {
        let Ok(message_value) = serde_json::from_str::<Value>(&message) else {
            continue;
        };
        let role = match message_value.get("role").and_then(Value::as_str) {
            Some("user") => "user",
            Some("assistant") => "assistant",
            _ => continue,
        };
        let part = part.and_then(|p| serde_json::from_str::<Value>(&p).ok());
        let parts = part
            .map(|p| opencode_parts(std::slice::from_ref(&p)))
            .unwrap_or_default();
        if parts.is_empty() {
            continue;
        }
        if last_message.as_deref() == Some(message.as_str()) {
            if let Some(last) = messages.last_mut() {
                last.content.extend(parts);
                continue;
            }
        }
        last_message = Some(message);
        messages.push(SessionMessage {
            role: role.to_string(),
            content: parts,
            message_id: None,
        });
    }
    Ok(messages)
}

#[cfg(test)]
#[path = "import_sources_tests.rs"]
mod tests;
