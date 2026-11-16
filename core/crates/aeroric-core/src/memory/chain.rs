//! B6 (2026-11-13): 代码版本链 (frontier-roadmap J6, §6.1). Every turn the
//! R2 ledger recorded is split, with no model, into blocks of branch × file
//! × definition: the `git diff -U0` hunks between the turn's two snapshot
//! trees, each put under the innermost definition that spans it
//! (`code_index::file_definitions`, private and nested ones included). The text is kept here, so the chain
//! outlives the ledger's pinned snapshots; undone turns stay, marked.
//!
//! The chain is never searched: `memory_recall` shows a few blocks under a
//! memory whose evidence names the session that made them.

use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};

use aeroric_protocol::memory::{
    CodeChange, MemoryItem, CHAIN_DIFF_CHARS, CHAIN_KEEP_MS, CHAIN_KEEP_PER_PROJECT,
};
use aeroric_protocol::turn_ledger::TurnRecord;

use super::{Memory, MemoryError};
use crate::knowledge::code_index;

/// Blocks kept from one turn (a formatter run over the whole tree).
const MAX_BLOCKS: usize = 200;

fn git(top: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// The branch checked out now, `None` when HEAD is detached.
pub fn branch(top: &Path) -> Option<String> {
    let name = String::from_utf8(git(top, &["rev-parse", "--abbrev-ref", "HEAD"])?).ok()?;
    let name = name.trim();
    (!name.is_empty() && name != "HEAD").then(|| name.to_string())
}

/// The commit checked out now.
pub fn head(top: &Path) -> Option<String> {
    let id = String::from_utf8(git(top, &["rev-parse", "HEAD"])?).ok()?;
    Some(id.trim().to_string()).filter(|id| !id.is_empty())
}

/// A path as `git diff` writes it after `--- a/` or `+++ b/`.
fn diff_path(raw: &str) -> Option<String> {
    let raw = raw.trim_end();
    if raw == "/dev/null" {
        return None;
    }
    let raw = raw
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(raw);
    let path = raw
        .strip_prefix("a/")
        .or_else(|| raw.strip_prefix("b/"))
        .unwrap_or(raw);
    Some(path.to_string())
}

/// `@@ -a,b +c,d @@` → `(a, c)`.
fn hunk_starts(header: &str) -> Option<(u32, u32)> {
    let mut parts = header.split_whitespace().skip(1);
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let start = |s: &str| s.split(',').next()?.parse::<u32>().ok();
    Some((start(old)?, start(new)?))
}

/// One hunk as parsed: where it starts on each side and its lines.
struct Hunk {
    old_start: u32,
    new_start: u32,
    text: String,
    additions: u32,
    deletions: u32,
}

struct FileDiff {
    path: String,
    /// The file is gone after the turn: definitions come from before.
    deleted: bool,
    binary: bool,
    hunks: Vec<Hunk>,
}

fn parse(diff: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_path: Option<String> = None;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            old_path = None;
            files.push(FileDiff {
                path: String::new(),
                deleted: false,
                binary: false,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("--- ") {
            old_path = diff_path(rest);
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            match diff_path(rest) {
                Some(path) => file.path = path,
                None => {
                    file.deleted = true;
                    file.path = old_path.clone().unwrap_or_default();
                }
            }
        } else if line.starts_with("Binary files ") {
            file.binary = true;
            if file.path.is_empty() {
                // `Binary files a/x and b/x differ`
                file.path = line
                    .split(" and ")
                    .nth(1)
                    .and_then(|r| r.strip_suffix(" differ"))
                    .and_then(diff_path)
                    .unwrap_or_default();
            }
        } else if line.starts_with("@@") {
            if let Some((old_start, new_start)) = hunk_starts(line) {
                file.hunks.push(Hunk {
                    old_start,
                    new_start,
                    text: format!("{line}\n"),
                    additions: 0,
                    deletions: 0,
                });
            }
        } else if let Some(hunk) = file.hunks.last_mut() {
            if line.starts_with('+') {
                hunk.additions += 1;
            } else if line.starts_with('-') {
                hunk.deletions += 1;
            }
            if hunk.text.len() < CHAIN_DIFF_CHARS * 4 {
                hunk.text.push_str(line);
                hunk.text.push('\n');
            }
        }
    }
    files.retain(|f| !f.path.is_empty());
    files
}

/// The innermost definition spanning `line`.
fn enclosing(definitions: &[code_index::Definition], line: u32) -> Option<String> {
    let line = line.max(1);
    definitions
        .iter()
        .filter(|d| d.line <= line && line <= d.end)
        .max_by_key(|d| d.line)
        .map(|d| d.name.clone())
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// One turn's blocks: its diff, hunk by hunk, under the definitions they
/// fall in. Empty when the snapshots are gone or nothing parsed.
pub fn changes(
    project_id: &str,
    task_id: &str,
    record: &TurnRecord,
    branch: Option<&str>,
) -> Vec<CodeChange> {
    let top = Path::new(&record.top);
    let Some(diff) = git(
        top,
        &[
            "diff",
            "-U0",
            "--no-renames",
            "--no-color",
            "--no-ext-diff",
            &record.before,
            &record.after,
        ],
    ) else {
        return Vec::new();
    };
    let diff = String::from_utf8_lossy(&diff);
    let mut out: Vec<CodeChange> = Vec::new();
    for file in parse(&diff) {
        let block = |symbol: Option<String>| CodeChange {
            project_id: project_id.to_string(),
            branch: branch.map(str::to_string),
            path: file.path.clone(),
            symbol,
            task_id: task_id.to_string(),
            turn_id: record.turn_id.clone(),
            at_ms: record.ended_ms,
            additions: 0,
            deletions: 0,
            state: record.state,
            diff: String::new(),
        };
        if file.binary || file.hunks.is_empty() {
            out.push(block(None));
            continue;
        }
        let tree = if file.deleted {
            &record.before
        } else {
            &record.after
        };
        let source = git(top, &["show", &format!("{tree}:{}", file.path)]).unwrap_or_default();
        let symbols = code_index::file_definitions(&file.path, &source);
        let mut mine: Vec<CodeChange> = Vec::new();
        for hunk in &file.hunks {
            let at = if file.deleted {
                hunk.old_start
            } else {
                hunk.new_start
            };
            let symbol = enclosing(&symbols, at);
            let index = match mine.iter().position(|c| c.symbol == symbol) {
                Some(index) => index,
                None => {
                    mine.push(block(symbol));
                    mine.len() - 1
                }
            };
            let change = &mut mine[index];
            change.additions += hunk.additions;
            change.deletions += hunk.deletions;
            change.diff.push_str(&hunk.text);
        }
        out.extend(mine);
    }
    for change in &mut out {
        change.diff = cut(&change.diff, CHAIN_DIFF_CHARS);
    }
    out.truncate(MAX_BLOCKS);
    out
}

/// Record one ledger turn of a project that uses memory (at the turn's
/// end, or later from the ledger). The branch is known only while the
/// turn's last commit is still checked out.
pub fn record_turn(memory: &Memory, project_id: &str, task_id: &str, record: &TurnRecord) {
    if !memory.project_enabled(project_id) {
        return;
    }
    let top = Path::new(&record.top);
    let branch = match (&record.head_after, head(top)) {
        (Some(was), Some(now)) if *was == now => branch(top),
        (None, None) => branch(top),
        _ => None,
    };
    let found = changes(project_id, task_id, record, branch.as_deref());
    if !found.is_empty() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        let _ = memory.record_changes(task_id, &record.turn_id, &found, now);
    }
}

impl Memory {
    /// Keep one turn's blocks (replacing what it had), then let the
    /// project's changes older than `CHAIN_KEEP_MS`, or beyond
    /// `CHAIN_KEEP_PER_PROJECT`, go.
    pub fn record_changes(
        &self,
        task_id: &str,
        turn_id: &str,
        changes: &[CodeChange],
        now_ms: u64,
    ) -> Result<(), MemoryError> {
        self.with(|store| {
            store.put_changes(task_id, turn_id, changes)?;
            if let Some(project) = changes.first().map(|c| c.project_id.as_str()) {
                store.prune_changes(
                    project,
                    now_ms.saturating_sub(CHAIN_KEEP_MS),
                    CHAIN_KEEP_PER_PROJECT,
                )?;
            }
            Ok(())
        })
    }

    /// The turn's blocks are kept already.
    pub fn has_changes(&self, task_id: &str, turn_id: &str) -> bool {
        self.with(|store| store.has_changes(task_id, turn_id))
            .unwrap_or(false)
    }

    /// The newest blocks of the sessions an item's evidence names.
    pub fn changes_for(&self, item: &MemoryItem, limit: usize) -> Vec<CodeChange> {
        let mut seen = HashSet::new();
        let tasks: Vec<String> = item
            .evidence
            .iter()
            .filter_map(|e| e.task_id.clone())
            .filter(|t| seen.insert(t.clone()))
            .collect();
        if tasks.is_empty() {
            return Vec::new();
        }
        self.with(|store| store.changes_of(&tasks, limit))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use aeroric_protocol::memory::{MemoryDraft, MemoryKind};
    use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};

    use super::{changes, enclosing, parse};
    use crate::knowledge::code_index::Definition;
    use crate::memory::{evidence, Memory};

    fn sh(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// A repository with one file before and after a turn, as trees.
    fn turn(before: &str, after: &str) -> (std::path::PathBuf, TurnRecord) {
        let dir = std::env::temp_dir().join(format!("aeroric-chain-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["init", "-q", "-b", "main"]);
        let tree = |text: &str| {
            std::fs::write(dir.join("lib.rs"), text).unwrap();
            sh(&dir, &["add", "-A"]);
            sh(&dir, &["write-tree"])
        };
        let (b, a) = (tree(before), tree(after));
        sh(
            &dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "x",
            ],
        );
        let record = TurnRecord {
            turn_id: "u1".into(),
            started_ms: 1,
            ended_ms: 2,
            top: dir.to_string_lossy().to_string(),
            before: b,
            after: a,
            head_before: None,
            head_after: None,
            files: Vec::new(),
            additions: 0,
            deletions: 0,
            state: TurnState::Undone,
            undone_ms: Some(3),
            undo_group: None,
            state_before_undo: None,
            check: None,
        };
        (dir, record)
    }

    #[test]
    fn hunks_go_under_the_definition_above_them() {
        let before =
            "fn alpha() {\n    1\n}\n\nimpl Ledger {\n    fn beta(&self) {\n        2\n    }\n}\n";
        let after = "fn alpha() {\n    10\n}\n\nimpl Ledger {\n    fn beta(&self) {\n        20\n        21\n    }\n}\n";
        let (dir, record) = turn(before, after);
        let found = changes("p1", "t1", &record, Some("main"));
        let names: Vec<_> = found.iter().map(|c| c.symbol.clone()).collect();
        assert_eq!(
            names,
            vec![Some("alpha".into()), Some("Ledger::beta".into())],
            "private functions and methods count"
        );
        assert_eq!((found[1].additions, found[1].deletions), (2, 1));
        assert!(found[1].diff.contains("+        21"));
        assert_eq!(found[0].state, TurnState::Undone, "an undone turn stays");
        assert_eq!(found[0].branch.as_deref(), Some("main"));
        assert_eq!(super::branch(&dir).as_deref(), Some("main"));
    }

    #[test]
    fn the_diff_parser_keeps_deleted_and_binary_files() {
        let diff = "diff --git a/old.rs b/old.rs\ndeleted file mode 100644\n--- a/old.rs\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-fn gone() {}\n-\ndiff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n";
        let files = parse(diff);
        assert_eq!(files.len(), 2);
        assert!(files[0].deleted);
        assert_eq!(files[0].path, "old.rs");
        assert_eq!(files[0].hunks[0].deletions, 2);
        assert!(files[1].binary);
        assert_eq!(files[1].path, "logo.png");
        let definitions = [
            Definition {
                name: "Outer".into(),
                line: 5,
                end: 20,
            },
            Definition {
                name: "Outer::inner".into(),
                line: 8,
                end: 12,
            },
        ];
        assert_eq!(enclosing(&definitions, 3), None, "above every definition");
        assert_eq!(enclosing(&definitions, 10).as_deref(), Some("Outer::inner"));
        assert_eq!(enclosing(&definitions, 15).as_deref(), Some("Outer"));
        assert_eq!(enclosing(&definitions, 30), None, "after every definition");
    }

    #[test]
    fn a_memory_shows_the_changes_of_its_sessions_and_old_ones_go() {
        let dir = std::env::temp_dir().join(format!("aeroric-chain-m-{}", uuid::Uuid::new_v4()));
        let memory = Memory::default();
        memory.set_dir(dir).unwrap();
        let item = memory
            .remember(
                MemoryDraft {
                    project_id: Some("p1".into()),
                    kind: MemoryKind::Fact,
                    category: None,
                    text: "超时改用 connect_timeout".into(),
                    supersedes: None,
                },
                evidence(Some("t1"), None, None, "x", 1),
                1,
            )
            .unwrap();
        let change = |turn: &str, at: u64| aeroric_protocol::memory::CodeChange {
            project_id: "p1".into(),
            branch: None,
            path: "ssh.rs".into(),
            symbol: Some("connect".into()),
            task_id: "t1".into(),
            turn_id: turn.into(),
            at_ms: at,
            additions: 1,
            deletions: 1,
            state: TurnState::Pending,
            diff: "@@ -1 +1 @@".into(),
        };
        let day = 86_400_000;
        memory
            .record_changes("t1", "u1", &[change("u1", 10 * day)], 10 * day)
            .unwrap();
        memory
            .record_changes("t1", "u2", &[change("u2", 20 * day)], 20 * day)
            .unwrap();
        assert!(memory.has_changes("t1", "u1"));
        let shown = memory.changes_for(&item, 3);
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].turn_id, "u2", "newest first");
        // Recording a turn again replaces its blocks.
        memory
            .record_changes("t1", "u2", &[change("u2", 20 * day)], 20 * day)
            .unwrap();
        assert_eq!(memory.changes_for(&item, 3).len(), 2);
        // 180 days on, the first turn's blocks go.
        memory
            .record_changes("t1", "u3", &[change("u3", 195 * day)], 195 * day)
            .unwrap();
        let left: Vec<_> = memory
            .changes_for(&item, 9)
            .into_iter()
            .map(|c| c.turn_id)
            .collect();
        assert_eq!(left, vec!["u3", "u2"]);
        let none = memory
            .remember(
                MemoryDraft {
                    project_id: None,
                    kind: MemoryKind::Profile,
                    category: None,
                    text: "回复用中文".into(),
                    supersedes: None,
                },
                evidence(None, None, None, "x", 1),
                1,
            )
            .unwrap();
        assert!(
            memory.changes_for(&none, 3).is_empty(),
            "no session, no chain"
        );
    }
}
