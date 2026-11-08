//! R2-2 (2026-11-04): a session's changes reviewed hunk by hunk — accept
//! (a mark), reject (the hunk reverse-applied to the working tree only),
//! comment (kept until sent together) — over one turn, since the session
//! was last marked seen, or the whole session.
//!
//! Hunks are named by a hash of their file, header and body, so a hunk keeps
//! its verdict for as long as it reads the same. `git apply -R` without
//! `--index` touches only the working tree; a hunk the tree has moved past
//! does not apply and is marked stale instead of being forced.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aeroric_protocol::change_review::{
    ChangeScope, DiffLine, HunkState, ReviewDiff, ReviewFile, ReviewHunk,
};
use aeroric_protocol::turn_ledger::TurnRecord;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::isolated_copy;
use super::now_ms;
use super::turn_changes::{git, snapshot_tree};
use super::turn_ledger::TurnLedger;

const SEEN_ROOT: &str = "refs/aeroric/seen";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HunkNote {
    state: HunkState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
    #[serde(default)]
    comment_sent: bool,
    path: String,
    line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskNotes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seen_ms: Option<u64>,
    #[serde(default)]
    hunks: HashMap<String, HunkNote>,
}

/// One file of a parsed diff, with what a single-hunk patch needs.
#[derive(Clone, Debug)]
struct Parsed {
    path: String,
    binary: bool,
    /// `diff --git …` through `+++ …`.
    header: String,
    hunks: Vec<ParsedHunk>,
}

#[derive(Clone, Debug)]
struct ParsedHunk {
    header: String,
    /// The body as git wrote it (with `\ No newline` markers).
    body: String,
    lines: Vec<DiffLine>,
}

pub struct ChangeReview {
    ledger: std::sync::Arc<TurnLedger>,
    dir: Mutex<Option<PathBuf>>,
    notes: Mutex<HashMap<String, TaskNotes>>,
    /// R2-3b: where isolated copies are recorded (`<data>/copies`).
    copies: Mutex<Option<PathBuf>>,
}

/// What a reject or an accept did, for the agent's next message.
pub struct Verdict {
    pub state: HunkState,
    pub path: String,
    pub line: u32,
}

impl ChangeReview {
    pub fn new(ledger: std::sync::Arc<TurnLedger>) -> Self {
        Self {
            ledger,
            dir: Mutex::new(None),
            notes: Mutex::default(),
            copies: Mutex::new(None),
        }
    }

    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
        self.notes.lock().clear();
    }

    pub fn set_copies_dir(&self, dir: PathBuf) {
        *self.copies.lock() = Some(dir);
    }

    pub fn copies_dir(&self) -> Option<PathBuf> {
        self.copies.lock().clone()
    }

    /// R2-3b: hunks of the copy not brought back (pending or in conflict);
    /// `None` when the task is not in a copy.
    pub fn origin_pending(&self, task_id: &str) -> Option<u32> {
        let dir = self.copies_dir()?;
        isolated_copy::load(&dir, task_id)?;
        let diff = self.diff(task_id, ChangeScope::Origin, None);
        Some(
            diff.files
                .iter()
                .flat_map(|f| &f.hunks)
                .filter(|h| matches!(h.state, HunkState::Pending | HunkState::Conflict))
                .count() as u32,
        )
    }

    /// R2-3b: bring every pending hunk back; (brought back, in conflict).
    pub fn bring_all(&self, task_id: &str) -> Result<(u32, u32), String> {
        let diff = self.diff(task_id, ChangeScope::Origin, None);
        if let Some(reason) = diff.empty_reason.filter(|_| diff.files.is_empty()) {
            return Err(reason);
        }
        let (mut applied, mut conflicts) = (0, 0);
        for hunk in diff.files.iter().flat_map(|f| &f.hunks) {
            if hunk.state != HunkState::Pending {
                continue;
            }
            match self
                .accept(task_id, ChangeScope::Origin, None, &hunk.id)?
                .state
            {
                HunkState::Applied => applied += 1,
                _ => conflicts += 1,
            }
        }
        Ok((applied, conflicts))
    }

    /// The hunks of `scope` with their verdicts.
    pub fn diff(&self, task_id: &str, scope: ChangeScope, turn_id: Option<&str>) -> ReviewDiff {
        let notes = self.load(task_id);
        let mut out = ReviewDiff {
            scope,
            turn_id: turn_id.map(str::to_string),
            seen_ms: notes.seen_ms,
            unsent: notes
                .hunks
                .values()
                .filter(|n| n.comment.is_some() && !n.comment_sent)
                .count() as u32,
            ..ReviewDiff::default()
        };
        let (top, from, to) = match self.ends(task_id, scope, turn_id) {
            Ok(ends) => ends,
            Err(reason) => {
                out.empty_reason = Some(reason);
                return out;
            }
        };
        let parsed = match parse_range(&top, &from, &to) {
            Some(parsed) => parsed,
            None => {
                out.empty_reason = Some("读不到这段改动".into());
                return out;
            }
        };
        if parsed.is_empty() {
            out.empty_reason = Some("这段范围里没有改动".into());
        }
        out.files = parsed
            .into_iter()
            .map(|file| {
                let additions = count(&file, "+");
                let deletions = count(&file, "-");
                let hunks = file
                    .hunks
                    .iter()
                    .map(|h| {
                        let id = hunk_id(&file.path, h);
                        let note = notes.hunks.get(&id);
                        ReviewHunk {
                            id,
                            header: h.header.clone(),
                            lines: h.lines.clone(),
                            state: note.map(|n| n.state).unwrap_or_default(),
                            comment: note.and_then(|n| n.comment.clone()),
                            comment_sent: note.is_some_and(|n| n.comment_sent),
                        }
                    })
                    .collect();
                ReviewFile {
                    path: file.path,
                    binary: file.binary,
                    additions,
                    deletions,
                    hunks,
                }
            })
            .collect();
        if scope == ChangeScope::Origin {
            if let Some(dir) = self.copies_dir() {
                let left = out
                    .files
                    .iter()
                    .flat_map(|f| &f.hunks)
                    .any(|h| matches!(h.state, HunkState::Pending | HunkState::Conflict));
                isolated_copy::set_cleared(&dir, task_id, !left);
            }
        }
        out
    }

    /// Mark a hunk accepted; a turn whose every hunk is accepted is accepted.
    pub fn accept(
        &self,
        task_id: &str,
        scope: ChangeScope,
        turn_id: Option<&str>,
        hunk: &str,
    ) -> Result<Verdict, String> {
        let (_, file, parsed) = self.find(task_id, scope, turn_id, hunk)?;
        if scope == ChangeScope::Origin {
            // R2-3b: accepting here is bringing it back.
            if file.binary {
                return Err("二进制文件暂不能按段带回".into());
            }
            let dir = self.copies_dir().ok_or("没有副本记录")?;
            let record = isolated_copy::load(&dir, task_id).ok_or("这个会话不在独立副本里")?;
            let patch = format!("{}{}\n{}", file.header, parsed.header, parsed.body);
            let brought = isolated_copy::apply(Path::new(&record.origin), patch.as_bytes(), false);
            return Ok(
                self.set(task_id, hunk, &file, &parsed, turn_id, scope, |n| {
                    n.state = if brought {
                        HunkState::Applied
                    } else {
                        HunkState::Conflict
                    };
                }),
            );
        }
        let verdict = self.set(task_id, hunk, &file, &parsed, turn_id, scope, |n| {
            n.state = HunkState::Accepted;
        });
        if let (ChangeScope::Turn, Some(turn)) = (scope, turn_id) {
            let all = self.diff(task_id, scope, Some(turn));
            let every = all
                .files
                .iter()
                .flat_map(|f| &f.hunks)
                .all(|h| h.state == HunkState::Accepted);
            if every {
                let _ = self.ledger.accept(task_id, turn);
            }
        }
        Ok(verdict)
    }

    /// Reverse-apply one hunk to the working tree; stale when it no longer applies.
    pub fn reject(
        &self,
        task_id: &str,
        scope: ChangeScope,
        turn_id: Option<&str>,
        hunk: &str,
    ) -> Result<Verdict, String> {
        let (top, file, parsed) = self.find(task_id, scope, turn_id, hunk)?;
        if file.binary {
            return Err("二进制文件只能整轮撤销".into());
        }
        let patch = format!("{}{}\n{}", file.header, parsed.header, parsed.body);
        let applied = apply_reverse(&top, &patch);
        Ok(
            self.set(task_id, hunk, &file, &parsed, turn_id, scope, |n| {
                n.state = if applied {
                    HunkState::Rejected
                } else {
                    HunkState::Stale
                };
            }),
        )
    }

    /// Keep (or with empty text, drop) a comment on a hunk.
    pub fn comment(
        &self,
        task_id: &str,
        scope: ChangeScope,
        turn_id: Option<&str>,
        hunk: &str,
        text: &str,
    ) -> Result<(), String> {
        let (_, file, parsed) = self.find(task_id, scope, turn_id, hunk)?;
        let text = text.trim().to_string();
        self.set(task_id, hunk, &file, &parsed, turn_id, scope, |n| {
            n.comment = (!text.is_empty()).then_some(text);
            n.comment_sent = false;
        });
        Ok(())
    }

    /// The unsent comments as one message, handed to `send`; they are
    /// marked sent only when it succeeds (C 批 2026-11-08). `None` when none.
    pub fn take_comments<T>(
        &self,
        task_id: &str,
        send: impl FnOnce(String) -> Result<T, String>,
    ) -> Option<Result<T, String>> {
        let mut notes = self.load(task_id);
        let mut lines: Vec<(String, u32, String)> = notes
            .hunks
            .values()
            .filter(|n| !n.comment_sent)
            .filter_map(|n| Some((n.path.clone(), n.line, n.comment.clone()?)))
            .collect();
        if lines.is_empty() {
            return None;
        }
        lines.sort();
        let body: Vec<String> = lines
            .into_iter()
            .map(|(path, line, text)| format!("- {path}:{line} — {text}"))
            .collect();
        let sent = send(format!("请按这些审查意见修改：\n{}", body.join("\n")));
        if sent.is_ok() {
            for note in notes.hunks.values_mut() {
                if note.comment.is_some() {
                    note.comment_sent = true;
                }
            }
            self.store(task_id, notes);
        }
        Some(sent)
    }

    /// Snapshot the working tree as what the user has now seen.
    pub fn mark_seen(&self, task_id: &str) -> Result<u64, String> {
        let top = self.top(task_id).ok_or("这个会话还没有改动")?;
        let (_, tree) = snapshot_tree(&top).ok_or("读不到工作区")?;
        git(&top, &["update-ref", &seen_ref(task_id), &tree], None).ok_or("记不下已看")?;
        let mut notes = self.load(task_id);
        let now = now_ms();
        notes.seen_ms = Some(now);
        self.store(task_id, notes);
        Ok(now)
    }

    /// Hunks rejected in `turn`, for its header.
    pub fn rejected(&self, task_id: &str, turn_id: &str) -> u32 {
        self.load(task_id)
            .hunks
            .values()
            .filter(|n| n.state == HunkState::Rejected && n.turn_id.as_deref() == Some(turn_id))
            .count() as u32
    }

    fn find(
        &self,
        task_id: &str,
        scope: ChangeScope,
        turn_id: Option<&str>,
        hunk: &str,
    ) -> Result<(PathBuf, Parsed, ParsedHunk), String> {
        let (top, from, to) = self.ends(task_id, scope, turn_id)?;
        let parsed = parse_range(&top, &from, &to).ok_or("读不到这段改动")?;
        for file in parsed {
            if let Some(h) = file.hunks.iter().find(|h| hunk_id(&file.path, h) == hunk) {
                let h = h.clone();
                return Ok((top, file, h));
            }
        }
        Err("这一处改动已经变了，请刷新".into())
    }

    #[allow(clippy::too_many_arguments)]
    fn set(
        &self,
        task_id: &str,
        hunk: &str,
        file: &Parsed,
        parsed: &ParsedHunk,
        turn_id: Option<&str>,
        scope: ChangeScope,
        change: impl FnOnce(&mut HunkNote),
    ) -> Verdict {
        let mut notes = self.load(task_id);
        let note = notes
            .hunks
            .entry(hunk.to_string())
            .or_insert_with(|| HunkNote {
                path: file.path.clone(),
                line: first_line(parsed),
                turn_id: (scope == ChangeScope::Turn)
                    .then(|| turn_id.map(str::to_string))
                    .flatten(),
                ..HunkNote::default()
            });
        change(note);
        let verdict = Verdict {
            state: note.state,
            path: note.path.clone(),
            line: note.line,
        };
        self.store(task_id, notes);
        verdict
    }

    /// (top, from, to) for `scope`.
    fn ends(
        &self,
        task_id: &str,
        scope: ChangeScope,
        turn_id: Option<&str>,
    ) -> Result<(PathBuf, String, String), String> {
        if scope == ChangeScope::Origin {
            let dir = self.copies_dir().ok_or("没有副本记录")?;
            let record = isolated_copy::load(&dir, task_id).ok_or("这个会话不在独立副本里")?;
            let top = PathBuf::from(&record.path);
            let (_, now) = snapshot_tree(&top).ok_or("读不到副本")?;
            return Ok((top, record.base, now));
        }
        let turns = self.ledger.list(task_id);
        if turns.is_empty() {
            return Err("这个会话还没有改动".into());
        }
        if scope == ChangeScope::Turn {
            let turn = turn_id
                .and_then(|id| turns.iter().find(|t| t.turn_id == id))
                .or_else(|| turns.iter().max_by_key(|t| t.ended_ms))
                .ok_or("没有这一轮的快照")?;
            return Ok((
                PathBuf::from(&turn.top),
                turn.before.clone(),
                turn.after.clone(),
            ));
        }
        let first: &TurnRecord = turns
            .iter()
            .min_by_key(|t| t.started_ms)
            .expect("non-empty");
        let top = PathBuf::from(&first.top);
        let (_, now) = snapshot_tree(&top).ok_or("读不到工作区")?;
        let from = match scope {
            ChangeScope::Seen => git(
                &top,
                &["rev-parse", "--verify", "-q", &seen_ref(task_id)],
                None,
            )
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| first.before.clone()),
            _ => first.before.clone(),
        };
        Ok((top, from, now))
    }

    fn top(&self, task_id: &str) -> Option<PathBuf> {
        self.ledger
            .list(task_id)
            .into_iter()
            .next()
            .map(|t| PathBuf::from(t.top))
    }

    fn load(&self, task_id: &str) -> TaskNotes {
        if let Some(n) = self.notes.lock().get(task_id) {
            return n.clone();
        }
        let notes: TaskNotes = self
            .path(task_id)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        self.notes.lock().insert(task_id.to_string(), notes.clone());
        notes
    }

    fn store(&self, task_id: &str, notes: TaskNotes) {
        if let Some(path) = self.path(task_id) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let tmp = path.with_extension("json.tmp");
            if let Ok(bytes) = serde_json::to_vec_pretty(&notes) {
                if std::fs::write(&tmp, bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        }
        self.notes.lock().insert(task_id.to_string(), notes);
    }

    fn path(&self, task_id: &str) -> Option<PathBuf> {
        let dir = self.dir.lock().clone()?;
        Some(dir.join(format!("{}.json", safe(task_id))))
    }
}

/// The agent's note after a rejected hunk.
pub fn reject_note(verdict: &Verdict) -> String {
    format!(
        "用户拒绝了 {} 第 {} 行附近的一处改动，已改回去；不要再按原样改回来。",
        verdict.path, verdict.line
    )
}

fn parse_range(top: &Path, from: &str, to: &str) -> Option<Vec<Parsed>> {
    let raw = git(
        top,
        &[
            "diff",
            "--no-color",
            "--no-renames",
            "--no-ext-diff",
            "-U3",
            from,
            to,
        ],
        None,
    )?;
    Some(parse(&raw))
}

/// A unified diff as git writes it, by file and hunk.
fn parse(raw: &str) -> Vec<Parsed> {
    let mut files: Vec<Parsed> = Vec::new();
    let mut in_header = false;
    let (mut old, mut new) = (0u32, 0u32);
    for line in raw.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        if bare.starts_with("diff --git ") {
            let path = bare
                .rsplit_once(" b/")
                .map(|(_, p)| p.to_string())
                .unwrap_or_default();
            files.push(Parsed {
                path,
                binary: false,
                header: line.to_string(),
                hunks: Vec::new(),
            });
            in_header = true;
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if in_header && !bare.starts_with("@@") {
            if bare.starts_with("Binary files") {
                file.binary = true;
            }
            if let Some(p) = bare.strip_prefix("+++ b/") {
                file.path = p.to_string();
            }
            file.header.push_str(line);
            continue;
        }
        if let Some(rest) = bare.strip_prefix("@@ ") {
            in_header = false;
            let mut nums = rest.split(' ');
            old = start(nums.next().unwrap_or(""));
            new = start(nums.next().unwrap_or(""));
            file.hunks.push(ParsedHunk {
                header: bare.to_string(),
                body: String::new(),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = file.hunks.last_mut() else {
            continue;
        };
        hunk.body.push_str(line);
        let (kind, text) = bare.split_at(bare.len().min(1));
        match kind {
            "+" => {
                hunk.lines.push(DiffLine {
                    kind: "+".into(),
                    text: text.into(),
                    old: None,
                    new: Some(new),
                });
                new += 1;
            }
            "-" => {
                hunk.lines.push(DiffLine {
                    kind: "-".into(),
                    text: text.into(),
                    old: Some(old),
                    new: None,
                });
                old += 1;
            }
            " " | "" => {
                hunk.lines.push(DiffLine {
                    kind: " ".into(),
                    text: text.into(),
                    old: Some(old),
                    new: Some(new),
                });
                old += 1;
                new += 1;
            }
            _ => {}
        }
    }
    files
}

/// `-12,3` → 12.
fn start(range: &str) -> u32 {
    range
        .trim_start_matches(['-', '+'])
        .split(',')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn count(file: &Parsed, kind: &str) -> u64 {
    file.hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.kind == kind)
        .count() as u64
}

fn first_line(hunk: &ParsedHunk) -> u32 {
    hunk.lines
        .iter()
        .find(|l| l.kind != " ")
        .and_then(|l| l.new.or(l.old))
        .unwrap_or(0)
}

/// FNV-1a over path, header and body: stable across runs and builds.
fn hunk_id(path: &str, hunk: &ParsedHunk) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path
        .bytes()
        .chain([0])
        .chain(hunk.header.bytes())
        .chain([0])
        .chain(hunk.body.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("h{hash:016x}")
}

fn apply_reverse(top: &Path, patch: &str) -> bool {
    let dir = std::env::temp_dir().join(format!("aeroric-hunk-{}", uuid::Uuid::new_v4().simple()));
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let file = dir.join("hunk.patch");
    let ok = std::fs::write(&file, patch).is_ok()
        && git(
            top,
            &[
                "apply",
                "-R",
                "--whitespace=nowarn",
                &file.to_string_lossy(),
            ],
            None,
        )
        .is_some();
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

fn seen_ref(task_id: &str) -> String {
    format!("{SEEN_ROOT}/{}", safe(task_id))
}

fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "change_review_tests.rs"]
mod tests;
