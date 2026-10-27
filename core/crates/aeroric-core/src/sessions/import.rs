//! 已有会话导入（UI 改版 M4）：Claude / Codex 本机会话的**授权后只读索引**。
//!
//! 规则：
//! - 没有授权记录的 Agent，一个文件都不读（`scan` 只遍历授权根）；`estimate` 只做目录遍历计数，
//!   不打开文件、不写索引。
//! - 源文件只读打开；索引只存元数据，不复制会话内容（M4 D2）。打开会话时按 `source_path` 现读。
//! - 会话 id 由 Agent 与源路径确定性派生，重复扫描不重复。
//! - 增量：marker（mtime + size）未变的条目不再读取。
//! - home 由宿主注入（`core-discovers-no-real-home`）；默认根 = `<home>/.claude`、`<home>/.codex`。

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sessions::codex::is_injected_context;
use crate::sessions::discovery::{collect_jsonl_files, collect_session_files};
use crate::sessions::parser::parse_session_lines;
use crate::sessions::SessionMessage;
use crate::storage::atomic::{write_atomic, FileMode};

/// 单个会话文件的读取上限：超过即标 `too_large`，不读内容。
pub const MAX_SESSION_BYTES: u64 = 32 * 1024 * 1024;
/// 取 cwd / 标题时最多读的头部字节数。新版 Codex 在首条真提示词之前先写
/// session_meta、developer 指令、AGENTS.md 与 world_state，常过 100 KB，
/// 所以窗口放到 1 MiB；读到标题即停。
const HEAD_BYTES: u64 = 1024 * 1024;
/// 标题算法的版本。索引里旧版本的条目即便文件未变也重算一次标题。
const TITLE_REV: u32 = 2;
/// 头部单行上限：更长的行（超大附件）跳过不解析。
const HEAD_LINE_BYTES: u64 = 16 * 1024 * 1024;
const TITLE_CHARS: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportAgent {
    Claude,
    Codex,
}

impl ImportAgent {
    pub const ALL: [ImportAgent; 2] = [ImportAgent::Claude, ImportAgent::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            ImportAgent::Claude => "claude",
            ImportAgent::Codex => "codex",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "claude" => Some(ImportAgent::Claude),
            "codex" => Some(ImportAgent::Codex),
            _ => None,
        }
    }

    /// 默认根目录（宿主注入 home）。
    pub fn default_root(self, home: &Path) -> PathBuf {
        match self {
            ImportAgent::Claude => home.join(".claude"),
            ImportAgent::Codex => home.join(".codex"),
        }
    }

    /// 根目录下真正存放会话的子目录。
    fn sessions_dir(self, root: &Path) -> PathBuf {
        match self {
            ImportAgent::Claude => root.join("projects"),
            ImportAgent::Codex => root.join("sessions"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportGrant {
    pub agent: ImportAgent,
    pub root: PathBuf,
    pub granted_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMarker {
    pub mtime_ms: u64,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSession {
    pub id: String,
    pub agent: ImportAgent,
    pub source_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_at: Option<String>,
    pub last_at_ms: u64,
    pub marker: SourceMarker,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default)]
    pub too_large: bool,
    /// Archived by the user: off the project list, under 已归档 (2026-10-27).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    /// Which title algorithm produced `title` (see `TITLE_REV`).
    #[serde(default)]
    pub title_rev: u32,
}

/// The user's choices over the index (2026-10-27): sessions left out of the
/// import or removed from Aeroric, and archived ones. Source files are never
/// touched; this only decides what Aeroric shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPrefs {
    #[serde(default)]
    pub excluded: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub archived: std::collections::BTreeSet<String>,
}

/// One session found on disk for the import picker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSession {
    #[serde(flatten)]
    pub session: ImportedSession,
    /// Ticked by default: not left out before.
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportEstimate {
    pub root_exists: bool,
    pub sessions: u64,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub unchanged: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImportError {
    #[error("no import grant for {0}")]
    NotGranted(&'static str),
    #[error("no imported session with id {0}")]
    UnknownSession(String),
    #[error("session file is larger than {MAX_SESSION_BYTES} bytes")]
    TooLarge,
    #[error("{0}")]
    Io(String),
}

/// `<data>/imports/` 下的授权与索引。
pub struct ImportStore {
    dir: PathBuf,
}

impl ImportStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("imports"),
        }
    }

    fn grants_path(&self) -> PathBuf {
        self.dir.join("grants.json")
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    fn prefs_path(&self) -> PathBuf {
        self.dir.join("prefs.json")
    }

    pub fn prefs(&self) -> ImportPrefs {
        read_json(&self.prefs_path()).unwrap_or_default()
    }

    fn save_prefs(&self, prefs: &ImportPrefs) -> Result<(), ImportError> {
        write_json(&self.prefs_path(), prefs)
    }

    /// What Aeroric shows: the index without left-out or removed sessions,
    /// archived ones flagged.
    pub fn visible(&self) -> Vec<ImportedSession> {
        let prefs = self.prefs();
        self.index()
            .into_iter()
            .filter(|session| !prefs.excluded.contains(&session.id))
            .map(|mut session| {
                session.archived = prefs.archived.contains(&session.id);
                session
            })
            .collect()
    }

    pub fn set_archived(&self, id: &str, archived: bool) -> Result<(), ImportError> {
        if !self.index().iter().any(|session| session.id == id) {
            return Err(ImportError::UnknownSession(id.to_string()));
        }
        let mut prefs = self.prefs();
        if archived {
            prefs.archived.insert(id.to_string());
        } else {
            prefs.archived.remove(id);
        }
        self.save_prefs(&prefs)
    }

    /// Take a session out of Aeroric (its file stays where the agent keeps it).
    pub fn remove(&self, id: &str) -> Result<(), ImportError> {
        if !self.index().iter().any(|session| session.id == id) {
            return Err(ImportError::UnknownSession(id.to_string()));
        }
        let mut prefs = self.prefs();
        prefs.archived.remove(id);
        prefs.excluded.insert(id.to_string());
        self.save_prefs(&prefs)
    }

    /// Every session under `roots` for the picker, without saving anything.
    /// Known, unchanged files are not re-read.
    pub fn preview(
        &self,
        roots: &[(ImportAgent, PathBuf)],
        projects: &[(String, String)],
    ) -> Vec<PreviewSession> {
        let previous = self.index();
        let prefs = self.prefs();
        let mut out = Vec::new();
        for (agent, root) in roots {
            for path in collect_sources(*agent, root) {
                let Some(marker) = marker_of(&path) else {
                    continue;
                };
                let id = session_id(*agent, &path);
                let session = match previous
                    .iter()
                    .find(|s| s.id == id && s.marker == marker && s.title_rev == TITLE_REV)
                {
                    Some(known) => {
                        let mut known = known.clone();
                        known.project_id = assign_project(known.cwd.as_deref(), projects);
                        known
                    }
                    None => match describe(*agent, &path, id, marker, projects) {
                        Ok(session) => session,
                        Err(_) => continue,
                    },
                };
                out.push(PreviewSession {
                    selected: !prefs.excluded.contains(&session.id),
                    session,
                });
            }
        }
        out.sort_by(|a, b| b.session.last_at_ms.cmp(&a.session.last_at_ms));
        out
    }

    /// The picker's answer: `selected` are imported, every other `offered` id
    /// is left out (until picked again).
    pub fn choose(&self, offered: &[String], selected: &[String]) -> Result<(), ImportError> {
        let mut prefs = self.prefs();
        for id in offered {
            if selected.contains(id) {
                prefs.excluded.remove(id);
            } else {
                prefs.excluded.insert(id.clone());
                prefs.archived.remove(id);
            }
        }
        self.save_prefs(&prefs)
    }

    pub fn grants(&self) -> Vec<ImportGrant> {
        read_json(&self.grants_path()).unwrap_or_default()
    }

    pub fn index(&self) -> Vec<ImportedSession> {
        read_json(&self.index_path()).unwrap_or_default()
    }

    fn save_grants(&self, grants: &[ImportGrant]) -> Result<(), ImportError> {
        write_json(&self.grants_path(), grants)
    }

    fn save_index(&self, index: &[ImportedSession]) -> Result<(), ImportError> {
        write_json(&self.index_path(), index)
    }

    /// 写入（或替换）一个 Agent 的授权。不触发扫描；调用方随后调用 `scan`。
    pub fn grant(&self, agent: ImportAgent, root: PathBuf, now_ms: u64) -> Result<(), ImportError> {
        let mut grants = self.grants();
        grants.retain(|grant| grant.agent != agent);
        grants.push(ImportGrant {
            agent,
            root,
            granted_at_ms: now_ms,
        });
        self.save_grants(&grants)
    }

    /// 撤销：删授权与该 Agent 的全部索引条目；源文件不动。
    pub fn revoke(&self, agent: ImportAgent) -> Result<(), ImportError> {
        let mut grants = self.grants();
        grants.retain(|grant| grant.agent != agent);
        self.save_grants(&grants)?;
        let mut index = self.index();
        index.retain(|session| session.agent != agent);
        self.save_index(&index)
    }

    /// 只遍历已授权的根；marker 未变的条目不读文件。`projects` 为 (id, 路径)。
    pub fn scan(&self, projects: &[(String, String)]) -> Result<ScanReport, ImportError> {
        let grants = self.grants();
        let previous = self.index();
        let mut report = ScanReport::default();
        let mut next = Vec::new();
        for grant in &grants {
            for path in collect_sources(grant.agent, &grant.root) {
                let Some(marker) = marker_of(&path) else {
                    continue;
                };
                let id = session_id(grant.agent, &path);
                match previous.iter().find(|session| session.id == id) {
                    Some(old) if old.marker == marker && old.title_rev == TITLE_REV => {
                        let mut kept = old.clone();
                        kept.project_id = assign_project(kept.cwd.as_deref(), projects);
                        next.push(kept);
                        report.unchanged += 1;
                    }
                    old => match describe(grant.agent, &path, id, marker, projects) {
                        Ok(session) => {
                            if old.is_some() {
                                report.updated += 1;
                            } else {
                                report.added += 1;
                            }
                            next.push(session);
                        }
                        Err(error) => report.errors.push(format!("{}: {error}", path.display())),
                    },
                }
            }
        }
        report.removed = previous
            .iter()
            .filter(|old| grants.iter().any(|g| g.agent == old.agent))
            .filter(|old| !next.iter().any(|session| session.id == old.id))
            .count() as u64;
        next.sort_by(|a, b| b.last_at_ms.cmp(&a.last_at_ms).then(a.id.cmp(&b.id)));
        self.save_index(&next)?;
        Ok(report)
    }

    /// 读一个已导入会话的消息（只读、带体积上限）。授权已撤销的会话不可读。
    pub fn messages(
        &self,
        id: &str,
    ) -> Result<(ImportedSession, Vec<SessionMessage>), ImportError> {
        let session = self
            .index()
            .into_iter()
            .find(|session| session.id == id)
            .ok_or_else(|| ImportError::UnknownSession(id.to_string()))?;
        if !self
            .grants()
            .iter()
            .any(|grant| grant.agent == session.agent)
        {
            return Err(ImportError::NotGranted(session.agent.as_str()));
        }
        let size = fs::metadata(&session.source_path)
            .map_err(|e| ImportError::Io(e.to_string()))?
            .len();
        if size > MAX_SESSION_BYTES {
            return Err(ImportError::TooLarge);
        }
        let mut text = String::new();
        File::open(&session.source_path)
            .and_then(|mut file| file.read_to_string(&mut text))
            .map_err(|e| ImportError::Io(e.to_string()))?;
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let mut messages = Vec::new();
        parse_session_lines(&lines, session.agent == ImportAgent::Codex, &mut messages);
        Ok((session, messages))
    }
}

/// dry-run：只遍历目录与 metadata，不打开文件、不写索引，不需要授权。
pub fn estimate(agent: ImportAgent, root: &Path) -> ImportEstimate {
    let mut estimate = ImportEstimate {
        root_exists: agent.sessions_dir(root).is_dir(),
        ..ImportEstimate::default()
    };
    for path in collect_sources(agent, root) {
        if let Some(marker) = marker_of(&path) {
            estimate.sessions += 1;
            estimate.bytes += marker.size;
            estimate.latest_ms = Some(estimate.latest_ms.unwrap_or(0).max(marker.mtime_ms));
        }
    }
    estimate
}

/// 会话文件清单。Claude 跳过 `subagents/` 下的子代理转录（它们不是独立会话）。
pub fn collect_sources(agent: ImportAgent, root: &Path) -> Vec<PathBuf> {
    let dir = agent.sessions_dir(root);
    let mut files = Vec::new();
    match agent {
        ImportAgent::Claude => {
            collect_jsonl_files(&dir, &mut files);
            files.retain(|path| !path.components().any(|c| c.as_os_str() == "subagents"));
        }
        ImportAgent::Codex => collect_session_files(&dir, &mut files),
    }
    files.sort();
    files
}

/// 确定性 id：`imp_` + sha256(agent \n path) 前 16 字节。
pub fn session_id(agent: ImportAgent, path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(agent.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("imp_{hex}")
}

/// cwd 等于项目路径或位于其下 → 该项目（取最长匹配）；否则未归类。
pub fn assign_project(cwd: Option<&str>, projects: &[(String, String)]) -> Option<String> {
    let cwd = Path::new(cwd?);
    projects
        .iter()
        .filter(|(_, path)| !path.is_empty() && cwd.starts_with(Path::new(path)))
        .max_by_key(|(_, path)| path.len())
        .map(|(id, _)| id.clone())
}

fn marker_of(path: &Path) -> Option<SourceMarker> {
    let metadata = fs::metadata(path).ok()?;
    let mtime_ms = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some(SourceMarker {
        mtime_ms,
        size: metadata.len(),
    })
}

fn describe(
    agent: ImportAgent,
    path: &Path,
    id: String,
    marker: SourceMarker,
    projects: &[(String, String)],
) -> Result<ImportedSession, String> {
    let fallback_title = path
        .file_stem()
        .map(|stem| fallback_title(agent, &stem.to_string_lossy()))
        .unwrap_or_else(|| id.clone());
    // The head is read even for an oversized file: its title and cwd are in
    // the first lines, only the full history is off limits.
    let head = read_head(agent, path)?;
    Ok(ImportedSession {
        project_id: assign_project(head.cwd.as_deref(), projects),
        id,
        agent,
        source_path: path.to_path_buf(),
        cwd: head.cwd,
        title: head.title.unwrap_or(fallback_title),
        first_at: head.first_at,
        last_at_ms: marker.mtime_ms,
        marker,
        too_large: marker.size > MAX_SESSION_BYTES,
        archived: false,
        title_rev: TITLE_REV,
    })
}

#[derive(Default)]
struct Head {
    cwd: Option<String>,
    title: Option<String>,
    first_at: Option<String>,
}

/// 读头部取 cwd、首条用户文本与首个时间戳：只在前 `HEAD_BYTES` 内**开始**
/// 的行，但每行读完整（≤ `HEAD_LINE_BYTES`）——首条提示词里贴了截图时，这一行
/// 常有几 MB，截断就解析不了（2026-10-27）。
fn read_head(agent: ImportAgent, path: &Path) -> Result<Head, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut head = Head::default();
    let mut consumed: u64 = 0;
    let mut buf = Vec::new();
    while consumed < HEAD_BYTES {
        buf.clear();
        let read = (&mut reader)
            .take(HEAD_LINE_BYTES)
            .read_until(b'\n', &mut buf)
            .map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        consumed += read as u64;
        if !buf.ends_with(b"\n") && read as u64 == HEAD_LINE_BYTES {
            // A line past the cap cannot be parsed; skip the rest of it.
            loop {
                buf.clear();
                let more = (&mut reader)
                    .take(HEAD_LINE_BYTES)
                    .read_until(b'\n', &mut buf)
                    .map_err(|e| e.to_string())?;
                consumed += more as u64;
                if more == 0 || buf.ends_with(b"\n") {
                    break;
                }
            }
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&buf) else {
            continue;
        };
        if head.first_at.is_none() {
            head.first_at = value
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
        match agent {
            ImportAgent::Claude => {
                if head.cwd.is_none() {
                    head.cwd = value
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                }
                if head.title.is_none()
                    && value.get("type").and_then(|v| v.as_str()) == Some("user")
                {
                    head.title = value
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(user_text);
                }
            }
            ImportAgent::Codex => {
                let payload = value.get("payload");
                match value.get("type").and_then(|v| v.as_str()) {
                    Some("session_meta") if head.cwd.is_none() => {
                        head.cwd = payload
                            .and_then(|p| p.get("cwd"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                    }
                    Some("event_msg") if head.title.is_none() => {
                        if payload.and_then(|p| p.get("type")).and_then(|v| v.as_str())
                            == Some("user_message")
                        {
                            head.title = payload.and_then(|p| p.get("message")).and_then(user_text);
                        }
                    }
                    Some("response_item") if head.title.is_none() => {
                        if payload.and_then(|p| p.get("role")).and_then(|v| v.as_str())
                            == Some("user")
                        {
                            head.title = payload.and_then(|p| p.get("content")).and_then(user_text);
                        }
                    }
                    _ => {}
                }
            }
        }
        if head.cwd.is_some() && head.title.is_some() && head.first_at.is_some() {
            break;
        }
    }
    Ok(head)
}

/// A session with no prompt of its own (a sub-agent thread, an aborted
/// start): "Codex 会话 · 2026-08-01 21:20" from `rollout-2026-08-01T21-20-22-<uuid>`
/// rather than the raw file name.
fn fallback_title(agent: ImportAgent, stem: &str) -> String {
    let name = match agent {
        ImportAgent::Claude => "Claude Code 会话",
        ImportAgent::Codex => "Codex 会话",
    };
    let stamp = stem.strip_prefix("rollout-").and_then(|rest| {
        let date = rest.get(0..10)?;
        let time = rest.get(11..16)?.replace('-', ":");
        (rest.as_bytes().get(10) == Some(&b'T')).then(|| format!("{date} {time}"))
    });
    match stamp {
        Some(stamp) => format!("{name} · {stamp}"),
        None => name.to_string(),
    }
}

/// 用户文本的首行；跳过 agent 注入的上下文块（命令包装、环境上下文、
/// AGENTS.md 指令，见 `is_injected_context`）。
fn user_text(content: &serde_json::Value) -> Option<String> {
    let candidates: Vec<&str> = match content {
        serde_json::Value::String(text) => vec![text.as_str()],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
            .collect(),
        _ => Vec::new(),
    };
    candidates
        .into_iter()
        .map(str::trim)
        .find(|text| !text.is_empty() && !is_injected_context(text))
        .and_then(|text| text.lines().next())
        .map(|line| line.chars().take(TITLE_CHARS).collect())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), ImportError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| ImportError::Io(e.to_string()))?;
    write_atomic(path, &bytes, FileMode::Private).map_err(|e| ImportError::Io(e.to_string()))
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
