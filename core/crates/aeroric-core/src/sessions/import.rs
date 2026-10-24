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

use crate::sessions::discovery::{collect_jsonl_files, collect_session_files};
use crate::sessions::parser::parse_session_lines;
use crate::sessions::SessionMessage;
use crate::storage::atomic::{write_atomic, FileMode};

/// 单个会话文件的读取上限：超过即标 `too_large`，不读内容。
pub const MAX_SESSION_BYTES: u64 = 32 * 1024 * 1024;
/// 取 cwd / 标题时最多读的头部字节数。
const HEAD_BYTES: u64 = 64 * 1024;
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
                    Some(old) if old.marker == marker => {
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
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| id.clone());
    let head = if marker.size > MAX_SESSION_BYTES {
        Head::default()
    } else {
        read_head(agent, path)?
    };
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
    })
}

#[derive(Default)]
struct Head {
    cwd: Option<String>,
    title: Option<String>,
    first_at: Option<String>,
}

/// 读头部（≤ 64 KB）取 cwd、首条用户文本与首个时间戳。
fn read_head(agent: ImportAgent, path: &Path) -> Result<Head, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file.take(HEAD_BYTES));
    let mut head = Head::default();
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
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

/// 用户文本的首行；跳过以 `<` 开头的注入块（命令包装、环境上下文）。
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
        .find(|text| !text.is_empty() && !text.starts_with('<'))
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
