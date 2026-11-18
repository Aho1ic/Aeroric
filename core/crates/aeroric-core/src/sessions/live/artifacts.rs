//! B6 (2026-11-18): what each task delivered. A completed turn that changed
//! files registers every file it added or changed as a new version of that
//! file's artifact. A version is the blob the turn's snapshot holds, pinned
//! under `refs/aeroric/artifacts/…` so `git gc` keeps it after the turn
//! ledger lets the turn go; nothing is copied.
//!
//! Undone, superseded and the checks a version went through are worked out
//! from the turn ledger when read, never stored.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_protocol::artifacts::{
    Artifact, ArtifactContent, ArtifactKind, ArtifactPage, ArtifactVersion, ValidationRecord, GONE,
    READ_DEFAULT, READ_MAX,
};
use aeroric_protocol::checkpoint::{EvidenceKind, EvidenceRef};
use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};
use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::turn_changes::{git, git_bytes};
use super::turn_ledger::{safe, TurnLedger};

/// A task keeps at most this many artifacts; the ones changed longest ago go.
pub const MAX_ARTIFACTS: usize = 500;
/// An artifact keeps at most this many versions; the oldest go.
pub const MAX_VERSIONS: usize = 20;
const PAGE: usize = 100;
const REF_ROOT: &str = "refs/aeroric/artifacts";

pub const NOT_GIT: &str = "这个目录不是 Git 仓库，无法记录交付物的版本";
pub const NO_VERSION: &str = "没有这个版本";
pub const NO_ARTIFACT: &str = "没有这个交付物";

/// Folders whose files are scratch, never deliverables.
const SCRATCH_DIRS: &[&str] = &[
    "tmp",
    "temp",
    ".tmp",
    ".cache",
    "__pycache__",
    "node_modules",
    ".pytest_cache",
    ".mypy_cache",
];
const SCRATCH_SUFFIXES: &[&str] = &[".swp", ".tmp", ".orig", ".rej", "~"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    id: String,
    path: String,
    kind: ArtifactKind,
    created_ms: u64,
    /// The repository's top directory.
    top: String,
    #[serde(default)]
    excluded: bool,
    #[serde(default)]
    removed: bool,
    /// Oldest first.
    versions: Vec<StoredVersion>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredVersion {
    version: u32,
    turn_id: String,
    /// When its turn started: checks of turns from then on saw it, until
    /// the next version stood.
    started_ms: u64,
    blob: String,
    size: u64,
    at_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Book {
    artifacts: Vec<Stored>,
    #[serde(default)]
    dropped: u32,
}

pub struct ArtifactRegistry {
    ledger: Arc<TurnLedger>,
    dir: Mutex<Option<PathBuf>>,
    tasks: Mutex<HashMap<String, Book>>,
}

impl ArtifactRegistry {
    pub fn new(ledger: Arc<TurnLedger>) -> Self {
        Self {
            ledger,
            dir: Mutex::default(),
            tasks: Mutex::default(),
        }
    }

    /// Where the registry is kept (`<data>/artifacts`); unset keeps it in
    /// memory.
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
        self.tasks.lock().clear();
    }

    /// A completed turn that changed files: each file it added or changed
    /// is a new version; a file it deleted is marked removed.
    pub fn register(&self, task_id: &str, record: &TurnRecord) {
        let top = PathBuf::from(&record.top);
        let paths: Vec<&str> = record
            .files
            .iter()
            .map(|f| f.path.as_str())
            .filter(|p| counts(p))
            .collect();
        if paths.is_empty() {
            return;
        }
        // Git could not list the tree: say nothing rather than call every
        // file deleted.
        let Some(found) = blobs(&top, &record.after, &paths) else {
            return;
        };
        let undone = self.undone_turns(task_id);
        self.update(task_id, |book| {
            for path in paths {
                let at = book
                    .artifacts
                    .iter()
                    .position(|a| a.path == path && a.top == record.top);
                let Some((blob, size)) = found.get(path) else {
                    if let Some(i) = at {
                        book.artifacts[i].removed = true;
                    }
                    continue;
                };
                let i = at.unwrap_or_else(|| {
                    book.artifacts.push(Stored {
                        id: format!("art-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
                        path: path.to_string(),
                        kind: kind_of(path),
                        created_ms: record.ended_ms,
                        top: record.top.clone(),
                        ..Default::default()
                    });
                    book.artifacts.len() - 1
                });
                let artifact = &mut book.artifacts[i];
                artifact.removed = false;
                if let Some(last) = artifact.versions.last_mut() {
                    // The same turn told twice: it is still one version.
                    if last.turn_id == record.turn_id {
                        last.blob = blob.clone();
                        last.size = *size;
                        pin(&top, task_id, &artifact.id, last.version, blob);
                        continue;
                    }
                    if last.blob == *blob && !undone.contains(&last.turn_id) {
                        continue;
                    }
                }
                let version = artifact.versions.last().map_or(1, |v| v.version + 1);
                pin(&top, task_id, &artifact.id, version, blob);
                artifact.versions.push(StoredVersion {
                    version,
                    turn_id: record.turn_id.clone(),
                    started_ms: record.started_ms,
                    blob: blob.clone(),
                    size: *size,
                    at_ms: record.ended_ms,
                });
                while artifact.versions.len() > MAX_VERSIONS {
                    let old = artifact.versions.remove(0);
                    unpin(&top, task_id, &artifact.id, old.version);
                }
            }
            if book.artifacts.len() > MAX_ARTIFACTS {
                book.artifacts
                    .sort_by_key(|a| std::cmp::Reverse(latest_ms(a)));
                for gone in book.artifacts.drain(MAX_ARTIFACTS..) {
                    let top = PathBuf::from(&gone.top);
                    for v in &gone.versions {
                        unpin(&top, task_id, &gone.id, v.version);
                    }
                    book.dropped += 1;
                }
            }
        });
    }

    /// One page of `task_id`'s artifacts, the latest changed first.
    pub fn list(&self, task_id: &str, cursor: Option<&str>) -> ArtifactPage {
        let book = self.load(task_id);
        let mut all: Vec<&Stored> = book.artifacts.iter().collect();
        all.sort_by(|a, b| latest_ms(b).cmp(&latest_ms(a)).then(a.path.cmp(&b.path)));
        let start = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
        let turns = self.ledger.list(task_id);
        let artifacts: Vec<Artifact> = all
            .iter()
            .skip(start)
            .take(PAGE)
            .map(|a| view(task_id, a, &turns))
            .collect();
        let end = start + artifacts.len();
        ArtifactPage {
            artifacts,
            next_cursor: (end < all.len()).then(|| end.to_string()),
            dropped: book.dropped,
            unavailable: None,
        }
    }

    /// Every version of one artifact, newest first.
    pub fn versions(&self, task_id: &str, artifact_id: &str) -> Option<Vec<ArtifactVersion>> {
        let book = self.load(task_id);
        let artifact = book.artifacts.iter().find(|a| a.id == artifact_id)?;
        let mut versions = versions_of(artifact, &self.ledger.list(task_id));
        versions.reverse();
        Some(versions)
    }

    /// A slice of one version's bytes, from the repository's object store.
    /// `version` unset reads the one that stands now.
    pub fn read(
        &self,
        task_id: &str,
        artifact_id: &str,
        version: Option<u32>,
        offset: u64,
        limit: Option<u64>,
    ) -> Result<ArtifactContent, String> {
        let book = self.load(task_id);
        let artifact = book
            .artifacts
            .iter()
            .find(|a| a.id == artifact_id)
            .ok_or(NO_ARTIFACT)?;
        let turns = self.ledger.list(task_id);
        let wanted = match version {
            Some(n) => artifact.versions.iter().find(|v| v.version == n),
            None => {
                let current = current_index(artifact, &turns);
                current.map(|i| &artifact.versions[i])
            }
        }
        .ok_or(NO_VERSION)?;
        let bytes = git_bytes(
            Path::new(&artifact.top),
            &["cat-file", "blob", &wanted.blob],
        )
        .ok_or(GONE)?;
        Ok(slice(artifact, wanted, &bytes, offset, limit))
    }

    /// The user says `artifact_id` does (not) count as a deliverable.
    pub fn exclude(&self, task_id: &str, artifact_id: &str, excluded: bool) -> Result<(), String> {
        self.update(task_id, |book| {
            let artifact = book
                .artifacts
                .iter_mut()
                .find(|a| a.id == artifact_id)
                .ok_or(NO_ARTIFACT)?;
            artifact.excluded = excluded;
            Ok(())
        })
    }

    fn undone_turns(&self, task_id: &str) -> Vec<String> {
        self.ledger
            .list(task_id)
            .into_iter()
            .filter(|r| r.state == TurnState::Undone)
            .map(|r| r.turn_id)
            .collect()
    }

    /// Read, change and save `task_id`'s book under one lock.
    fn update<R>(&self, task_id: &str, change: impl FnOnce(&mut Book) -> R) -> R {
        let path = self.path(task_id);
        let mut tasks = self.tasks.lock();
        let book = tasks
            .entry(task_id.to_string())
            .or_insert_with(|| read_book(path.as_deref()));
        let result = change(book);
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(bytes) = serde_json::to_vec_pretty(&*book) {
                if let Err(error) = crate::storage::atomic::write_atomic(
                    &path,
                    &bytes,
                    crate::storage::atomic::FileMode::Private,
                ) {
                    eprintln!("artifacts: saving {task_id} failed: {error}");
                }
            }
        }
        result
    }

    fn load(&self, task_id: &str) -> Book {
        let path = self.path(task_id);
        self.tasks
            .lock()
            .entry(task_id.to_string())
            .or_insert_with(|| read_book(path.as_deref()))
            .clone()
    }

    fn path(&self, task_id: &str) -> Option<PathBuf> {
        self.dir
            .lock()
            .as_ref()
            .map(|d| d.join(format!("{}.json", safe(task_id))))
    }
}

fn read_book(path: Option<&Path>) -> Book {
    path.and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Whether `cwd` can keep versions at all.
pub fn unavailable(cwd: &Path) -> Option<String> {
    git(cwd, &["rev-parse", "--show-toplevel"], None)
        .is_none()
        .then(|| NOT_GIT.to_string())
}

fn latest_ms(a: &Stored) -> u64 {
    a.versions.last().map_or(a.created_ms, |v| v.at_ms)
}

fn undone(turns: &[TurnRecord], turn_id: &str) -> bool {
    turns
        .iter()
        .any(|r| r.turn_id == turn_id && r.state == TurnState::Undone)
}

/// The newest version whose turn was not undone.
fn current_index(artifact: &Stored, turns: &[TurnRecord]) -> Option<usize> {
    artifact
        .versions
        .iter()
        .rposition(|v| !undone(turns, &v.turn_id))
}

fn view(task_id: &str, a: &Stored, turns: &[TurnRecord]) -> Artifact {
    let current = current_index(a, turns).and_then(|i| versions_of(a, turns).into_iter().nth(i));
    Artifact {
        id: a.id.clone(),
        task_id: task_id.to_string(),
        path: a.path.clone(),
        kind: a.kind,
        created_ms: a.created_ms,
        latest_version: a.versions.last().map_or(0, |v| v.version),
        excluded: a.excluded,
        removed: a.removed,
        current,
    }
}

/// Oldest first, with what the ledger says about each.
fn versions_of(a: &Stored, turns: &[TurnRecord]) -> Vec<ArtifactVersion> {
    let standing: Vec<bool> = a
        .versions
        .iter()
        .map(|v| !undone(turns, &v.turn_id))
        .collect();
    let current = standing.iter().rposition(|s| *s);
    a.versions
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let withdrawn = !standing[i];
            // It stood until the next version that was not undone.
            let until = a.versions[i + 1..]
                .iter()
                .zip(&standing[i + 1..])
                .find(|(_, s)| **s)
                .map(|(next, _)| next.started_ms);
            let checks = if withdrawn {
                Vec::new()
            } else {
                let mut seen: Vec<&TurnRecord> = turns
                    .iter()
                    .filter(|r| r.state != TurnState::Undone && r.started_ms >= v.started_ms)
                    .filter(|r| until.is_none_or(|end| r.started_ms < end))
                    .filter(|r| r.check.is_some())
                    .collect();
                seen.sort_by_key(|r| r.started_ms);
                seen.into_iter()
                    .filter_map(|r| {
                        let check = r.check.as_ref()?;
                        Some(ValidationRecord {
                            evidence: EvidenceRef {
                                kind: EvidenceKind::AutoCheck,
                                seq: None,
                                turn: None,
                                detail: r.turn_id.clone(),
                            },
                            verdict: if check.passed { "passed" } else { "failed" }.into(),
                            summary: check.summary.clone(),
                            incomplete: check.incomplete,
                        })
                    })
                    .collect()
            };
            ArtifactVersion {
                artifact_id: a.id.clone(),
                version: v.version,
                turn_id: v.turn_id.clone(),
                blob: v.blob.clone(),
                size: v.size,
                at_ms: v.at_ms,
                withdrawn,
                superseded: !withdrawn && current.is_some_and(|c| i < c),
                checks,
            }
        })
        .collect()
}

/// At most `limit` bytes from `offset`. A text file is cut on a character
/// boundary and sent as text; anything else as base64.
fn slice(
    artifact: &Stored,
    version: &StoredVersion,
    bytes: &[u8],
    offset: u64,
    limit: Option<u64>,
) -> ArtifactContent {
    let size = bytes.len();
    let start = (offset as usize).min(size);
    let limit = limit.unwrap_or(READ_DEFAULT).clamp(1, READ_MAX) as usize;
    let mut end = start.saturating_add(limit).min(size);
    let text = std::str::from_utf8(bytes).ok();
    let (encoding, content) = match text {
        Some(text) if text.is_char_boundary(start) => {
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            ("utf8", text[start..end].to_string())
        }
        _ => (
            "base64",
            base64::engine::general_purpose::STANDARD.encode(&bytes[start..end]),
        ),
    };
    ArtifactContent {
        artifact_id: artifact.id.clone(),
        version: version.version,
        blob: version.blob.clone(),
        size: size as u64,
        offset: start as u64,
        encoding: encoding.into(),
        content,
        next_offset: (end < size).then_some(end as u64),
    }
}

/// Whether a changed path is a deliverable: not in a scratch folder, not a
/// scratch file. Ignored files never reach the snapshot to begin with.
pub fn counts(path: &str) -> bool {
    let mut parts = path.split('/');
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == ".DS_Store" || SCRATCH_SUFFIXES.iter().any(|s| name.ends_with(s)) {
        return false;
    }
    !parts.any(|p| SCRATCH_DIRS.contains(&p))
}

pub fn kind_of(path: &str) -> ArtifactKind {
    let ext = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rs" | "swift" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "go" | "java" | "kt"
        | "c" | "h" | "cc" | "cpp" | "hpp" | "m" | "mm" | "rb" | "php" | "cs" | "sh" | "zsh"
        | "bash" | "lua" | "sql" | "css" | "scss" | "html" | "vue" | "svelte" | "dart"
        | "scala" | "zig" => ArtifactKind::Code,
        "md" | "markdown" | "txt" | "rst" | "adoc" | "pdf" | "doc" | "docx" | "tex" | "rtf"
        | "pptx" | "key" => ArtifactKind::Document,
        "json" | "jsonl" | "csv" | "tsv" | "yaml" | "yml" | "toml" | "xml" | "parquet" | "xlsx"
        | "db" | "sqlite" => ArtifactKind::Data,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "heic" | "bmp" | "ico" | "tiff" => {
            ArtifactKind::Image
        }
        _ => ArtifactKind::Other,
    }
}

/// `path → (blob, size)` for the regular files among `paths` in `tree`;
/// a path missing here was deleted. Symlinks and submodules are skipped.
fn blobs(top: &Path, tree: &str, paths: &[&str]) -> Option<HashMap<String, (String, u64)>> {
    // Literal: a name with `*` or `[` in it is that name, not a pattern.
    let specs: Vec<String> = paths.iter().map(|p| format!(":(literal){p}")).collect();
    let mut args = vec!["ls-tree", "-z", "-l", "--full-tree", tree, "--"];
    args.extend(specs.iter().map(String::as_str));
    git(top, &args, None).map(|raw| parse_ls_tree(&raw))
}

/// `<mode> <type> <object> <size>\t<path>\0` per entry.
fn parse_ls_tree(raw: &str) -> HashMap<String, (String, u64)> {
    raw.split('\0')
        .filter_map(|entry| {
            let (meta, path) = entry.split_once('\t')?;
            let mut fields = meta.split_whitespace();
            let mode = fields.next()?;
            let kind = fields.next()?;
            let object = fields.next()?;
            let size = fields.next()?.parse().ok()?;
            (kind == "blob" && mode != "120000")
                .then(|| (path.to_string(), (object.to_string(), size)))
        })
        .collect()
}

fn ref_name(task_id: &str, artifact_id: &str, version: u32) -> String {
    format!(
        "{REF_ROOT}/{}/{}/v{version}",
        safe(task_id),
        safe(artifact_id)
    )
}

fn pin(top: &Path, task_id: &str, artifact_id: &str, version: u32, blob: &str) {
    let name = ref_name(task_id, artifact_id, version);
    let _ = git(top, &["update-ref", &name, blob], None);
}

fn unpin(top: &Path, task_id: &str, artifact_id: &str, version: u32) {
    let name = ref_name(task_id, artifact_id, version);
    let _ = git(top, &["update-ref", "-d", &name], None);
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;
