//! A05: what the working tree changed during each turn, for any agent family.
//!
//! When a turn starts the session's repository is snapshotted to a tree
//! object through a *temporary copy* of its index (`git add -A` then
//! `git write-tree`): the real index, HEAD and working tree are never touched,
//! and copying the index keeps git's stat cache so unchanged files are not
//! re-hashed. When the turn ends a second snapshot is diffed against the
//! first and a `turnChanges` update is appended to the session's log. Git runs
//! on its own threads so streaming is never held up; a directory that is not
//! a git repository simply produces nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::update::{SessionUpdate, SessionUpdateKind, TurnFileChange};
use super::{now_ms, DriverRegistry, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// Files listed per turn; the totals still count every file.
pub const MAX_FILES: usize = 200;
/// A snapshot that takes longer than this is abandoned (huge untracked trees).
const GIT_TIMEOUT: Duration = Duration::from_secs(20);

type Snapshot = mpsc::Receiver<Option<Tree>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Tree {
    top: PathBuf,
    id: String,
}

/// Wraps the host's sink: every call passes through unchanged; turn
/// boundaries additionally start or finish a snapshot.
pub struct TurnChangeTracker {
    inner: Arc<dyn LiveStatusSink>,
    registry: Arc<DriverRegistry>,
    cwds: Mutex<HashMap<String, PathBuf>>,
    started: Mutex<HashMap<String, Snapshot>>,
}

impl TurnChangeTracker {
    pub fn new(inner: Arc<dyn LiveStatusSink>, registry: Arc<DriverRegistry>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            registry,
            cwds: Mutex::default(),
            started: Mutex::default(),
        })
    }

    /// Where `task_id`'s agent works; set when its session starts.
    pub fn watch(&self, task_id: &str, cwd: &str) {
        self.cwds
            .lock()
            .insert(task_id.to_string(), PathBuf::from(cwd));
    }

    fn turn_started(&self, task_id: &str) {
        let Some(cwd) = self.cwds.lock().get(task_id).cloned() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.started.lock().insert(task_id.to_string(), rx);
        std::thread::spawn(move || {
            let _ = tx.send(snapshot(&cwd));
        });
    }

    fn turn_ended(self: &Arc<Self>, task_id: &str, turn_id: &str) {
        let Some(before) = self.started.lock().remove(task_id) else {
            return;
        };
        let this = Arc::clone(self);
        let task_id = task_id.to_string();
        let turn_id = turn_id.to_string();
        std::thread::spawn(move || {
            let Ok(Some(before)) = before.recv_timeout(GIT_TIMEOUT) else {
                return;
            };
            let Some(after) = snapshot(&before.top) else {
                return;
            };
            let Some(kind) = changes(&before, &after, &turn_id) else {
                return;
            };
            if let Some(log) = this.registry.log(&task_id) {
                let update = log.push(kind, now_ms());
                this.inner.update(&update);
            }
        });
    }
}

/// Holds an `Arc<Self>` for the threads; the sink trait takes `&self`.
pub struct TrackingSink(pub Arc<TurnChangeTracker>);

impl LiveStatusSink for TrackingSink {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        self.0
            .inner
            .status(project_id, task_id, generation, status, reason);
    }

    fn session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        agent: &str,
        session_id: &str,
        session_path: Option<&str>,
    ) {
        self.0
            .inner
            .session(project_id, task_id, family, agent, session_id, session_path);
    }

    fn update(&self, update: &SessionUpdate) {
        match &update.kind {
            SessionUpdateKind::TurnStarted { .. } => self.0.turn_started(&update.task_id),
            SessionUpdateKind::TurnEnded { turn_id, .. } => {
                self.0.turn_ended(&update.task_id, turn_id)
            }
            _ => {}
        }
        self.0.inner.update(update);
    }
}

/// The repository's working tree as a tree id, or `None` when `cwd` is not in
/// a git repository or git fails.
fn snapshot(cwd: &Path) -> Option<Tree> {
    let top = PathBuf::from(git(cwd, &["rev-parse", "--show-toplevel"], None)?.trim());
    let real_index = git(&top, &["rev-parse", "--git-path", "index"], None)?;
    let real_index = top.join(real_index.trim());
    let scratch = tempfile_dir()?;
    let index = scratch.join("index");
    if real_index.is_file() {
        std::fs::copy(&real_index, &index).ok()?;
    }
    let result = (|| {
        git(&top, &["add", "-A", "--", "."], Some(&index))?;
        git(&top, &["write-tree"], Some(&index))
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    Some(Tree {
        top,
        id: result?.trim().to_string(),
    })
}

fn changes(before: &Tree, after: &Tree, turn_id: &str) -> Option<SessionUpdateKind> {
    if before.id == after.id {
        return None;
    }
    let raw = git(
        &before.top,
        &[
            "diff",
            "--numstat",
            "-z",
            "--no-renames",
            &before.id,
            &after.id,
        ],
        None,
    )?;
    let files = parse_numstat_z(&raw);
    if files.is_empty() {
        return None;
    }
    let additions = files.iter().filter_map(|f| f.additions).sum();
    let deletions = files.iter().filter_map(|f| f.deletions).sum();
    let truncated = files.len() > MAX_FILES;
    Some(SessionUpdateKind::TurnChanges {
        turn_id: turn_id.to_string(),
        files: files.into_iter().take(MAX_FILES).collect(),
        additions,
        deletions,
        truncated,
    })
}

/// `git diff --numstat -z`: `<add>\t<del>\t<path>\0` per file; binary files
/// report `-` for both counts.
pub fn parse_numstat_z(raw: &str) -> Vec<TurnFileChange> {
    raw.split('\0')
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let mut parts = entry.splitn(3, '\t');
            let additions = parts.next()?.parse().ok();
            let deletions = parts.next()?.parse().ok();
            let path = parts.next()?.to_string();
            Some(TurnFileChange {
                path,
                additions,
                deletions,
            })
        })
        .collect()
}

fn tempfile_dir() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-turn-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Run git with a deadline; stdout on success.
fn git(cwd: &Path, args: &[&str], index: Option<&Path>) -> Option<String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut out).ok()?;
        Some(out)
    });
    let deadline = Instant::now() + GIT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().ok()??;
                return status.success().then_some(out);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-turn-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        run(&dir, &["init", "-q"]);
        std::fs::write(dir.join("src/a.rs"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        run(&dir, &["add", "-A"]);
        run(&dir, &["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn numstat_parses_text_and_binary_entries() {
        let files = parse_numstat_z("3\t1\tsrc/a.rs\0-\t-\timg.png\0");
        assert_eq!(
            files,
            vec![
                TurnFileChange {
                    path: "src/a.rs".into(),
                    additions: Some(3),
                    deletions: Some(1)
                },
                TurnFileChange {
                    path: "img.png".into(),
                    additions: None,
                    deletions: None
                },
            ]
        );
    }

    #[test]
    fn a_turn_reports_edits_and_new_files_and_leaves_the_repo_untouched() {
        let dir = repo();
        let staged_before = std::fs::read(dir.join(".git/index")).unwrap();
        let before = snapshot(&dir.join("src")).expect("a repository snapshots");
        std::fs::write(dir.join("src/a.rs"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(dir.join("src/new.rs"), "fn x() {}\n").unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target/ignored.o"), "bin").unwrap();
        let after = snapshot(&dir).unwrap();
        let Some(SessionUpdateKind::TurnChanges {
            files,
            additions,
            deletions,
            truncated,
            turn_id,
        }) = changes(&before, &after, "t1")
        else {
            panic!("changes expected");
        };
        assert_eq!(turn_id, "t1");
        assert!(!truncated);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/a.rs", "src/new.rs"], "ignored files stay out");
        assert_eq!((additions, deletions), (3, 1));
        assert_eq!(
            std::fs::read(dir.join(".git/index")).unwrap(),
            staged_before,
            "the real index is not touched"
        );
        let status = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        let status = String::from_utf8_lossy(&status.stdout);
        assert!(
            status.contains("?? src/new.rs"),
            "new file still untracked: {status}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_change_and_no_repository_produce_nothing() {
        let dir = repo();
        let a = snapshot(&dir).unwrap();
        let b = snapshot(&dir).unwrap();
        assert!(changes(&a, &b, "t").is_none());
        let plain = std::env::temp_dir().join(format!(
            "aeroric-turn-plain-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&plain).unwrap();
        assert!(snapshot(&plain).is_none());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&plain).ok();
    }
}
