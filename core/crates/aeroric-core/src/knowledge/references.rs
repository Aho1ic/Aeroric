//! Q-R6 (2026-11-13): the reference library — local folders of code the user
//! collected, indexed read-only so an agent can see how others solved a
//! problem before writing its own. One list for every project.
//!
//! `<data>/references/list.json` holds the folders; each has
//! `<data>/references/<id>/index.db`: the files it read (size and mtime, so a
//! rebuild only reads what changed), 40-line chunks in an FTS5 index over the
//! CJK-segmented text, and top-level symbols from the code index's parsers.
//! Aeroric never writes into a reference folder and never pulls it.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use super::code_index;
use crate::notebook::rag::cjk;

const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const CHUNK_LINES: usize = 40;
/// Hits one search returns, across every folder.
pub const SEARCH_LIMIT: usize = 12;
/// Lines one read returns at most.
pub const READ_LIMIT: u32 = 400;
const SKIP_DIRS: [&str; 12] = [
    ".git",
    "target",
    "node_modules",
    "build",
    "dist",
    "out",
    ".build",
    "DerivedData",
    "__pycache__",
    ".venv",
    "venv",
    "Pods",
];
const TEXT_EXTENSIONS: &[&str] = &[
    "rs", "swift", "py", "pyi", "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "go", "java",
    "kt", "kts", "scala", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "cu", "cuh", "m", "mm", "cs",
    "rb", "php", "lua", "dart", "zig", "jl", "r", "sh", "bash", "zsh", "sql", "proto", "vue",
    "svelte", "md", "mdx", "rst", "toml", "yaml", "yml", "json", "cmake", "gradle",
];
const TEXT_NAMES: &[&str] = &["Makefile", "Dockerfile", "CMakeLists.txt", "Justfile"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceError {
    NotAFolder(String),
    Duplicate(String),
    Unknown(String),
    OutsideFolder,
    NotAFile(String),
    Storage(String),
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAFolder(path) => write!(f, "{path} 不是一个能读的文件夹"),
            Self::Duplicate(name) => write!(f, "这个文件夹已经在参考代码库里（{name}）"),
            Self::Unknown(id) => write!(f, "没有参考代码库 {id}"),
            Self::OutsideFolder => write!(f, "路径要在参考代码库文件夹里面"),
            Self::NotAFile(path) => write!(f, "{path} 不是一个能读的文本文件"),
            Self::Storage(why) => write!(f, "参考代码库存储出错：{why}"),
        }
    }
}

impl std::error::Error for ReferenceError {}

impl From<rusqlite::Error> for ReferenceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<std::io::Error> for ReferenceError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    pub added_ms: u64,
}

/// A folder as the settings list shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceStatus {
    #[serde(flatten)]
    pub reference: Reference,
    pub files: u64,
    pub symbols: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_ms: Option<u64>,
    /// More files than [`MAX_FILES`]: the rest are not searched.
    pub truncated: bool,
    pub indexing: bool,
    /// The folder changed since the last index (checked when asked).
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceHit {
    pub repo_id: String,
    pub repo: String,
    pub path: String,
    pub line: u32,
    /// `symbol` (a definition's name matched) or `text`.
    pub kind: String,
    pub excerpt: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexReport {
    pub files: u64,
    pub read: u64,
    pub removed: u64,
    pub truncated: bool,
}

/// The library: the list, and which folders are being indexed now.
#[derive(Default)]
pub struct References {
    dir: Mutex<Option<PathBuf>>,
    indexing: Mutex<HashSet<String>>,
    errors: Mutex<HashMap<String, String>>,
    /// Serialises changes to `list.json`.
    lock: Mutex<()>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn short_hash(text: &str) -> String {
    Sha1::digest(text.as_bytes())[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    crate::runtime::subprocess::configure_background_command(&mut cmd);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn is_text_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    TEXT_NAMES.contains(&name)
        || name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| TEXT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The folder's text files (git's view when it is a repository, so
/// `.gitignore` holds), sorted, and whether the list was cut.
pub fn list_files(dir: &Path) -> (Vec<String>, bool) {
    let mut files: Vec<String> = match git(dir, &["ls-files", "-co", "--exclude-standard", "-z"]) {
        Some(out) => out
            .split('\0')
            .filter(|p| !p.is_empty() && is_text_path(p))
            .map(str::to_string)
            .collect(),
        None => {
            let mut files = Vec::new();
            let mut stack = vec![dir.to_path_buf()];
            while let Some(current) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&current) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let Ok(kind) = entry.file_type() else {
                        continue;
                    };
                    if kind.is_dir() {
                        if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                            stack.push(path);
                        }
                    } else if kind.is_file() {
                        if let Ok(rel) = path.strip_prefix(dir) {
                            let rel = rel.to_string_lossy().replace('\\', "/");
                            if is_text_path(&rel) {
                                files.push(rel);
                            }
                        }
                    }
                }
                if files.len() > MAX_FILES {
                    break;
                }
            }
            files
        }
    };
    files.sort();
    let truncated = files.len() > MAX_FILES;
    files.truncate(MAX_FILES);
    (files, truncated)
}

fn stamp(meta: &std::fs::Metadata) -> (u64, u64) {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    (meta.len(), modified)
}

/// Changes when a listed file is added, removed or written.
pub fn fingerprint(dir: &Path) -> String {
    let (files, _) = list_files(dir);
    let mut hasher = Sha1::new();
    for rel in &files {
        hasher.update(rel.as_bytes());
        if let Ok(meta) = std::fs::metadata(dir.join(rel)) {
            let (size, modified) = stamp(&meta);
            hasher.update(format!("|{size}|{modified}\n").as_bytes());
        }
    }
    hasher.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn open_index(path: &Path) -> Result<Connection, ReferenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY, size INTEGER NOT NULL,
             mtime INTEGER NOT NULL, lines INTEGER NOT NULL);
         CREATE VIRTUAL TABLE IF NOT EXISTS chunks USING fts5(path UNINDEXED, start UNINDEXED,
             body, tokenize = '{}');
         CREATE TABLE IF NOT EXISTS symbols(name TEXT NOT NULL, kind TEXT NOT NULL,
             path TEXT NOT NULL, line INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS symbols_name ON symbols(name COLLATE NOCASE);
         CREATE INDEX IF NOT EXISTS symbols_path ON symbols(path);
         CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        cjk::FTS_TOKENIZER
    ))?;
    Ok(conn)
}

fn meta(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), ReferenceError> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn forget_file(conn: &Connection, path: &str) -> Result<(), ReferenceError> {
    conn.execute("DELETE FROM chunks WHERE path = ?1", [path])?;
    conn.execute("DELETE FROM symbols WHERE path = ?1", [path])?;
    conn.execute("DELETE FROM files WHERE path = ?1", [path])?;
    Ok(())
}

/// Build or bring up to date `index` for `dir`; only changed files are read.
pub fn index_folder(dir: &Path, index: &Path) -> Result<IndexReport, ReferenceError> {
    if !dir.is_dir() {
        return Err(ReferenceError::NotAFolder(dir.display().to_string()));
    }
    let mut conn = open_index(index)?;
    let (files, truncated) = list_files(dir);
    let known: HashMap<String, (u64, u64)> = {
        let mut statement = conn.prepare("SELECT path, size, mtime FROM files")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, u64>(1)?, r.get::<_, u64>(2)?),
            ))
        })?;
        rows.collect::<Result<_, _>>()?
    };
    let listed: HashSet<&str> = files.iter().map(String::as_str).collect();
    let tx = conn.transaction()?;
    let mut report = IndexReport {
        truncated,
        ..IndexReport::default()
    };
    for path in known.keys().filter(|p| !listed.contains(p.as_str())) {
        forget_file(&tx, path)?;
        report.removed += 1;
    }
    for rel in &files {
        let full = dir.join(rel);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let now = stamp(&meta);
        report.files += 1;
        if known.get(rel) == Some(&now) {
            continue;
        }
        forget_file(&tx, rel)?;
        let Ok(bytes) = std::fs::read(&full) else {
            continue;
        };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        for (n, chunk) in lines.chunks(CHUNK_LINES).enumerate() {
            let body = chunk.join("\n");
            if body.trim().is_empty() {
                continue;
            }
            tx.execute(
                "INSERT INTO chunks(path, start, body) VALUES (?1, ?2, ?3)",
                params![
                    rel,
                    (n * CHUNK_LINES + 1) as i64,
                    format!("{rel}\n{}", cjk::segment(&body))
                ],
            )?;
        }
        for symbol in code_index::file_symbols(rel, &bytes) {
            tx.execute(
                "INSERT INTO symbols(name, kind, path, line) VALUES (?1, ?2, ?3, ?4)",
                params![symbol.name, symbol.kind, rel, symbol.line],
            )?;
        }
        tx.execute(
            "INSERT INTO files(path, size, mtime, lines) VALUES (?1, ?2, ?3, ?4)",
            params![rel, now.0, now.1, lines.len() as i64],
        )?;
        report.read += 1;
    }
    set_meta(&tx, "fingerprint", &fingerprint(dir))?;
    set_meta(&tx, "indexed_ms", &now_ms().to_string())?;
    set_meta(&tx, "truncated", if truncated { "1" } else { "0" })?;
    tx.commit()?;
    Ok(report)
}

/// The line of `path` (1-based, from `start`, within one chunk) that best
/// matches `words`, and its text.
fn best_line(dir: &Path, path: &str, start: u32, words: &[String]) -> (u32, String) {
    let text = std::fs::read_to_string(dir.join(path)).unwrap_or_default();
    let chunk: Vec<(u32, &str)> = text
        .lines()
        .enumerate()
        .skip(start.saturating_sub(1) as usize)
        .take(CHUNK_LINES)
        .map(|(i, l)| (i as u32 + 1, l))
        .collect();
    let found = chunk
        .iter()
        .find(|(_, l)| {
            let lower = l.to_lowercase();
            words.iter().any(|w| lower.contains(w))
        })
        .or_else(|| chunk.iter().find(|(_, l)| !l.trim().is_empty()))
        .copied()
        .unwrap_or((start, ""));
    (found.0, found.1.trim().chars().take(200).collect())
}

impl References {
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
    }

    fn root(&self) -> Result<PathBuf, ReferenceError> {
        self.dir
            .lock()
            .clone()
            .ok_or_else(|| ReferenceError::Storage("参考代码库目录还没设置".into()))
    }

    fn index_path(&self, id: &str) -> Result<PathBuf, ReferenceError> {
        Ok(self.root()?.join(id).join("index.db"))
    }

    pub fn list(&self) -> Vec<Reference> {
        let Ok(root) = self.root() else {
            return Vec::new();
        };
        std::fs::read(root.join("list.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, list: &[Reference]) -> Result<(), ReferenceError> {
        let root = self.root()?;
        std::fs::create_dir_all(&root)?;
        let tmp = root.join("list.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(list).map_err(|e| ReferenceError::Storage(e.to_string()))?,
        )?;
        std::fs::rename(tmp, root.join("list.json"))?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Reference, ReferenceError> {
        self.list()
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| ReferenceError::Unknown(id.to_string()))
    }

    /// Register a folder; the caller indexes it afterwards.
    pub fn add(
        &self,
        path: &str,
        name: Option<&str>,
        purpose: Option<&str>,
    ) -> Result<Reference, ReferenceError> {
        let _guard = self.lock.lock();
        let folder = std::fs::canonicalize(path)
            .ok()
            .filter(|p| p.is_dir())
            .ok_or_else(|| ReferenceError::NotAFolder(path.to_string()))?;
        let folder_text = folder.to_string_lossy().into_owned();
        let mut list = self.list();
        if let Some(existing) = list.iter().find(|r| r.path == folder_text) {
            return Err(ReferenceError::Duplicate(existing.name.clone()));
        }
        let name = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| folder_text.clone())
            });
        let slug: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let reference = Reference {
            id: format!(
                "{}-{}",
                if slug.is_empty() { "ref" } else { &slug },
                short_hash(&folder_text)
            ),
            name,
            path: folder_text,
            purpose: purpose
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string),
            added_ms: now_ms(),
        };
        list.push(reference.clone());
        self.save(&list)?;
        Ok(reference)
    }

    /// Forget a folder and its index; the folder itself is left alone.
    pub fn remove(&self, id: &str) -> Result<(), ReferenceError> {
        let _guard = self.lock.lock();
        let mut list = self.list();
        let before = list.len();
        list.retain(|r| r.id != id);
        if list.len() == before {
            return Err(ReferenceError::Unknown(id.to_string()));
        }
        self.save(&list)?;
        let _ = std::fs::remove_dir_all(self.root()?.join(id));
        self.errors.lock().remove(id);
        Ok(())
    }

    /// Mark `id` as being indexed; false when it already is.
    pub fn begin_index(&self, id: &str) -> bool {
        self.indexing.lock().insert(id.to_string())
    }

    /// Index `id` now (blocking); call [`Self::begin_index`] first.
    pub fn index(&self, id: &str) -> Result<IndexReport, ReferenceError> {
        let result = (|| {
            let reference = self.get(id)?;
            let index = self.index_path(id)?;
            // `begin_index` keeps this the folder's only writer.
            index_folder(Path::new(&reference.path), &index)
        })();
        self.indexing.lock().remove(id);
        match &result {
            Ok(_) => {
                self.errors.lock().remove(id);
            }
            Err(error) => {
                self.errors.lock().insert(id.to_string(), error.to_string());
            }
        }
        result
    }

    pub fn status(&self, reference: Reference) -> ReferenceStatus {
        let index = self.index_path(&reference.id).ok().filter(|p| p.exists());
        let conn = index.and_then(|p| Connection::open(p).ok());
        let count = |sql: &str| {
            conn.as_ref()
                .and_then(|c| c.query_row(sql, [], |r| r.get::<_, u64>(0)).ok())
                .unwrap_or(0)
        };
        let stored = conn.as_ref().and_then(|c| meta(c, "fingerprint"));
        let indexed_ms = conn
            .as_ref()
            .and_then(|c| meta(c, "indexed_ms"))
            .and_then(|v| v.parse().ok());
        let indexing = self.indexing.lock().contains(&reference.id);
        let stale =
            !indexing && stored.is_some_and(|f| f != fingerprint(Path::new(&reference.path)));
        ReferenceStatus {
            files: count("SELECT COUNT(*) FROM files"),
            symbols: count("SELECT COUNT(*) FROM symbols"),
            indexed_ms,
            truncated: conn.as_ref().and_then(|c| meta(c, "truncated")).as_deref() == Some("1"),
            indexing,
            stale,
            error: self.errors.lock().get(&reference.id).cloned(),
            reference,
        }
    }

    pub fn statuses(&self) -> Vec<ReferenceStatus> {
        self.list().into_iter().map(|r| self.status(r)).collect()
    }

    /// Definitions named like a query word first, then text chunks by
    /// relevance, over one folder (`repo` is its id or name) or all.
    pub fn search(
        &self,
        query: &str,
        repo: Option<&str>,
    ) -> Result<Vec<ReferenceHit>, ReferenceError> {
        let list: Vec<Reference> = match repo.map(str::trim).filter(|r| !r.is_empty()) {
            Some(want) => {
                let found: Vec<Reference> = self
                    .list()
                    .into_iter()
                    .filter(|r| r.id == want || r.name.eq_ignore_ascii_case(want))
                    .collect();
                if found.is_empty() {
                    return Err(ReferenceError::Unknown(want.to_string()));
                }
                found
            }
            None => self.list(),
        };
        let words: Vec<String> = query
            .split_whitespace()
            .map(str::to_lowercase)
            .filter(|w| !w.is_empty())
            .collect();
        let expression = cjk::match_expression(query).map(|e| e.replace(" AND ", " OR "));
        let mut symbols = Vec::new();
        let mut texts: Vec<(f64, ReferenceHit)> = Vec::new();
        for reference in &list {
            let Ok(index) = self.index_path(&reference.id) else {
                continue;
            };
            if !index.exists() {
                continue;
            }
            let conn = Connection::open(&index)?;
            let dir = Path::new(&reference.path);
            for word in &words {
                let mut statement = conn.prepare(
                    "SELECT kind, path, line FROM symbols WHERE name = ?1 COLLATE NOCASE LIMIT 5",
                )?;
                let rows = statement.query_map([word], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, u32>(2)?,
                    ))
                })?;
                for (kind, path, line) in rows.flatten() {
                    let (_, text) = best_line(dir, &path, line, std::slice::from_ref(word));
                    symbols.push(ReferenceHit {
                        repo_id: reference.id.clone(),
                        repo: reference.name.clone(),
                        path,
                        line,
                        kind: format!("symbol:{kind}"),
                        excerpt: text,
                    });
                }
            }
            let Some(expression) = &expression else {
                continue;
            };
            let mut statement = conn.prepare(
                "SELECT path, start, bm25(chunks) FROM chunks WHERE chunks MATCH ?1
                 ORDER BY bm25(chunks) LIMIT ?2",
            )?;
            let rows = statement.query_map(params![expression, SEARCH_LIMIT as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })?;
            for (path, start, score) in rows.flatten() {
                let (line, excerpt) = best_line(dir, &path, start.max(1) as u32, &words);
                texts.push((
                    score,
                    ReferenceHit {
                        repo_id: reference.id.clone(),
                        repo: reference.name.clone(),
                        path,
                        line,
                        kind: "text".into(),
                        excerpt,
                    },
                ));
            }
        }
        texts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut hits = symbols;
        hits.extend(texts.into_iter().map(|(_, hit)| hit));
        let mut seen = HashSet::new();
        hits.retain(|h| seen.insert((h.repo_id.clone(), h.path.clone(), h.line)));
        hits.truncate(SEARCH_LIMIT);
        Ok(hits)
    }

    /// Lines `start..=end` (1-based) of a file inside folder `repo` (its id or
    /// name), numbered. A path that leaves the folder is refused.
    pub fn read(
        &self,
        repo: &str,
        path: &str,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Result<String, ReferenceError> {
        let reference = self
            .list()
            .into_iter()
            .find(|r| r.id == repo || r.name.eq_ignore_ascii_case(repo))
            .ok_or_else(|| ReferenceError::Unknown(repo.to_string()))?;
        let relative = Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(ReferenceError::OutsideFolder);
        }
        let root = std::fs::canonicalize(&reference.path)
            .map_err(|_| ReferenceError::NotAFolder(reference.path.clone()))?;
        let full = std::fs::canonicalize(root.join(relative))
            .map_err(|_| ReferenceError::NotAFile(path.to_string()))?;
        if !full.starts_with(&root) {
            return Err(ReferenceError::OutsideFolder);
        }
        let meta = std::fs::metadata(&full).map_err(|_| ReferenceError::NotAFile(path.into()))?;
        if !meta.is_file() || meta.len() > 4 * MAX_FILE_BYTES {
            return Err(ReferenceError::NotAFile(path.to_string()));
        }
        let bytes = std::fs::read(&full)?;
        if bytes.iter().take(8192).any(|b| *b == 0) {
            return Err(ReferenceError::NotAFile(path.to_string()));
        }
        let text = String::from_utf8_lossy(&bytes);
        let total = text.lines().count() as u32;
        let start = start.unwrap_or(1).max(1);
        let end = end
            .unwrap_or(start + 199)
            .max(start)
            .min(start + READ_LIMIT - 1)
            .min(total.max(1));
        let body: Vec<String> = text
            .lines()
            .enumerate()
            .skip(start as usize - 1)
            .take((end + 1).saturating_sub(start) as usize)
            .map(|(i, line)| format!("{}: {line}", i + 1))
            .collect();
        let more = if end < total {
            format!(
                "\n（共 {total} 行，还有 {} 行，用 start={} 接着读）",
                total - end,
                end + 1
            )
        } else {
            String::new()
        };
        Ok(format!(
            "{} · {path} 第 {start}–{end} 行\n{}{more}",
            reference.name,
            body.join("\n")
        ))
    }
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod tests;
