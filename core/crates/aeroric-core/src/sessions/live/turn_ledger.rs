//! R2-1 (2026-11-04): the turns that changed files, per task, so each can be
//! accepted, undone, or its undo taken back.
//!
//! A turn's two snapshot trees are pinned under `refs/aeroric/turns/…` in
//! its repository so `git gc` keeps them; at most [`KEEP_PER_REPO`] turns
//! and [`KEEP_MS`] are kept per repository. Undo writes files from the
//! snapshot straight into the working tree: the index, HEAD and every branch
//! stay as they are. A file whose content is no longer what the turn left
//! (a later turn, or the user, changed it) is skipped, never overwritten.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aeroric_protocol::turn_ledger::{
    PlannedFile, SkippedFile, TurnAction, TurnCheck, TurnPlan, TurnRecord, TurnState,
};
use parking_lot::Mutex;

use super::now_ms;
use super::turn_changes::{git, git_bytes};

pub const KEEP_PER_REPO: usize = 50;
pub const KEEP_MS: u64 = 30 * 24 * 3600 * 1000;
const REF_ROOT: &str = "refs/aeroric/turns";

pub const SKIP_CHANGED: &str = "这一轮之后又改过";

#[derive(Default)]
pub struct TurnLedger {
    dir: Mutex<Option<PathBuf>>,
    tasks: Mutex<HashMap<String, Vec<TurnRecord>>>,
}

impl TurnLedger {
    /// Where the ledger is kept (`<data>/turns`); unset keeps it in memory.
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
        self.tasks.lock().clear();
    }

    pub fn list(&self, task_id: &str) -> Vec<TurnRecord> {
        self.load(task_id)
    }

    pub fn get(&self, task_id: &str, turn_id: &str) -> Option<TurnRecord> {
        self.load(task_id)
            .into_iter()
            .find(|r| r.turn_id == turn_id)
    }

    /// A turn that changed files: pin its snapshots, keep it, and let the
    /// repository's oldest turns go.
    pub fn record(&self, task_id: &str, record: TurnRecord) {
        let top = PathBuf::from(&record.top);
        for (side, tree) in [("before", &record.before), ("after", &record.after)] {
            let name = ref_name(task_id, &record.turn_id, side);
            let _ = git(&top, &["update-ref", &name, tree], None);
        }
        let mut list = self.load(task_id);
        list.retain(|r| r.turn_id != record.turn_id);
        list.push(record.clone());
        self.store(task_id, list);
        self.prune(&top, now_ms());
    }

    /// R5-1: how the project's checks ended on `turn_id`; a turn the ledger
    /// does not hold (it changed no files) is left alone.
    pub fn set_check(&self, task_id: &str, turn_id: &str, check: TurnCheck) {
        let mut list = self.load(task_id);
        let Some(record) = list.iter_mut().find(|r| r.turn_id == turn_id) else {
            return;
        };
        record.check = Some(check);
        self.store(task_id, list);
    }

    pub fn accept(&self, task_id: &str, turn_id: &str) -> Result<TurnRecord, String> {
        self.update(task_id, turn_id, |r| match r.state {
            TurnState::Pending => {
                r.state = TurnState::Accepted;
                Ok(())
            }
            _ => Err("这一轮不是待验收".to_string()),
        })
    }

    /// What `action` would do now; `running` blocks it.
    pub fn plan(
        &self,
        task_id: &str,
        turn_id: &str,
        action: TurnAction,
        running: bool,
    ) -> TurnPlan {
        self.planned(task_id, turn_id, action, running).plan
    }

    /// R2-3a: the turns `action` on `turn_id` covers, oldest first — the
    /// turn alone, every later one not undone yet (`UndoFrom`), or the batch
    /// it was undone with (`Restore`).
    fn members(
        &self,
        task_id: &str,
        record: &TurnRecord,
        action: TurnAction,
    ) -> Result<Vec<TurnRecord>, String> {
        let undone = record.state == TurnState::Undone;
        match action {
            TurnAction::Undo | TurnAction::UndoFrom if undone => {
                return Err("这一轮已经撤销了".into())
            }
            TurnAction::Restore if !undone => return Err("这一轮没有撤销过".into()),
            _ => {}
        }
        let mut list = self.load(task_id);
        list.sort_by_key(|r| r.started_ms);
        let members: Vec<TurnRecord> = match action {
            TurnAction::Undo => vec![record.clone()],
            TurnAction::UndoFrom => list
                .into_iter()
                .filter(|r| r.started_ms >= record.started_ms && r.state != TurnState::Undone)
                .collect(),
            TurnAction::Restore => match &record.undo_group {
                Some(group) => list
                    .into_iter()
                    .filter(|r| r.undo_group.as_deref() == Some(group))
                    .collect(),
                None => vec![record.clone()],
            },
        };
        Ok(members)
    }

    fn planned(&self, task_id: &str, turn_id: &str, action: TurnAction, running: bool) -> Planned {
        let Some(record) = self.get(task_id, turn_id) else {
            return Planned::blocked("没有这一轮的快照");
        };
        if running {
            return Planned::blocked("这一轮结束后可撤销");
        }
        let members = match self.members(task_id, &record, action) {
            Ok(members) => members,
            Err(reason) => return Planned::blocked(&reason),
        };
        let batch = members.len() > 1;
        for member in &members {
            if member.head_before != member.head_after {
                let commit = member
                    .head_after
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(7)
                    .collect::<String>();
                let which = if batch { "其中一轮" } else { "这一轮" };
                return Planned::blocked(&format!(
                    "{which}提交了 {commit}，撤销会改写历史，请自己处理"
                ));
            }
        }
        let top = PathBuf::from(&record.top);
        // Each path: the first and the last member that changed it.
        let mut touched: Vec<(String, usize, usize)> = Vec::new();
        for (i, member) in members.iter().enumerate() {
            for tree in [&member.before, &member.after] {
                if git(&top, &["cat-file", "-e", &format!("{tree}^{{tree}}")], None).is_none() {
                    return Planned::blocked("快照已经被清理，无法撤销");
                }
            }
            let Some(raw) = git(
                &top,
                &[
                    "diff",
                    "--name-only",
                    "-z",
                    "--no-renames",
                    &member.before,
                    &member.after,
                ],
                None,
            ) else {
                return Planned::blocked("读不到这一轮的快照");
            };
            for path in raw.split('\0').filter(|p| !p.is_empty()) {
                match touched.iter_mut().find(|(p, _, _)| p == path) {
                    Some(entry) => entry.2 = i,
                    None => touched.push((path.to_string(), i, i)),
                }
            }
        }
        let mut out = Planned {
            plan: TurnPlan {
                turns: members.iter().map(|m| m.turn_id.clone()).collect(),
                ..TurnPlan::default()
            },
            writes: Vec::new(),
            top: top.clone(),
            members: members.iter().map(|m| m.turn_id.clone()).collect(),
        };
        for (path, first, last) in touched {
            // Undo: from the last turn's result back to how the first found it.
            let (expected, target) = match action {
                TurnAction::Restore => (&members[first].before, &members[last].after),
                _ => (&members[last].after, &members[first].before),
            };
            if current(&top, &path) == blob(&top, expected, &path) {
                let delete = blob(&top, target, &path).is_none();
                out.plan.files.push(PlannedFile {
                    path: path.clone(),
                    delete,
                });
                out.writes.push((path, target.clone(), delete));
            } else {
                out.plan.skipped.push(SkippedFile {
                    path,
                    reason: SKIP_CHANGED.into(),
                });
            }
        }
        out
    }

    /// Carry out `action`: write each planned file from the other snapshot.
    /// Returns what was done (a blocked plan does nothing).
    pub fn apply(
        &self,
        task_id: &str,
        turn_id: &str,
        action: TurnAction,
        running: bool,
    ) -> Result<TurnPlan, String> {
        let planned = self.planned(task_id, turn_id, action, running);
        if let Some(reason) = &planned.plan.blocked {
            return Err(reason.clone());
        }
        for (path, tree, delete) in &planned.writes {
            write_back(&planned.top, tree, path, *delete).map_err(|e| format!("{path}：{e}"))?;
        }
        let group = (planned.members.len() > 1).then(|| planned.members[0].clone());
        let now = now_ms();
        let mut list = self.load(task_id);
        for r in list
            .iter_mut()
            .filter(|r| planned.members.contains(&r.turn_id))
        {
            match action {
                TurnAction::Undo | TurnAction::UndoFrom => {
                    r.state_before_undo = Some(r.state);
                    r.state = TurnState::Undone;
                    r.undone_ms = Some(now);
                    r.undo_group = group.clone();
                }
                TurnAction::Restore => {
                    r.state = r.state_before_undo.take().unwrap_or(TurnState::Pending);
                    r.undone_ms = None;
                    r.undo_group = None;
                }
            }
        }
        self.store(task_id, list);
        Ok(planned.plan)
    }

    fn update(
        &self,
        task_id: &str,
        turn_id: &str,
        change: impl FnOnce(&mut TurnRecord) -> Result<(), String>,
    ) -> Result<TurnRecord, String> {
        let mut list = self.load(task_id);
        let record = list
            .iter_mut()
            .find(|r| r.turn_id == turn_id)
            .ok_or("没有这一轮的快照")?;
        change(record)?;
        let out = record.clone();
        self.store(task_id, list);
        Ok(out)
    }

    /// Keep the newest [`KEEP_PER_REPO`] turns of `top` that are younger
    /// than [`KEEP_MS`]; the rest lose their refs and their records.
    fn prune(&self, top: &Path, now: u64) {
        let Some(dir) = self.dir.lock().clone() else {
            return;
        };
        let top_text = top.to_string_lossy().to_string();
        let mut all: Vec<(String, TurnRecord)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(task) = name.strip_suffix(".json") else {
                    continue;
                };
                for record in self.load(task) {
                    if record.top == top_text {
                        all.push((task.to_string(), record));
                    }
                }
            }
        }
        all.sort_by(|a, b| b.1.ended_ms.cmp(&a.1.ended_ms));
        let dropped: Vec<(String, String)> = all
            .iter()
            .enumerate()
            .filter(|(i, (_, r))| *i >= KEEP_PER_REPO || now.saturating_sub(r.ended_ms) > KEEP_MS)
            .map(|(_, (task, r))| (task.clone(), r.turn_id.clone()))
            .collect();
        for (task, turn) in dropped {
            for side in ["before", "after"] {
                let _ = git(
                    top,
                    &["update-ref", "-d", &ref_name(&task, &turn, side)],
                    None,
                );
            }
            let mut list = self.load(&task);
            list.retain(|r| r.turn_id != turn);
            self.store(&task, list);
        }
    }

    fn load(&self, task_id: &str) -> Vec<TurnRecord> {
        if let Some(list) = self.tasks.lock().get(task_id) {
            return list.clone();
        }
        let list: Vec<TurnRecord> = self
            .path(task_id)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        self.tasks.lock().insert(task_id.to_string(), list.clone());
        list
    }

    fn store(&self, task_id: &str, list: Vec<TurnRecord>) {
        if let Some(path) = self.path(task_id) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let tmp = path.with_extension("json.tmp");
            if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
                if std::fs::write(&tmp, bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        }
        self.tasks.lock().insert(task_id.to_string(), list);
    }

    fn path(&self, task_id: &str) -> Option<PathBuf> {
        let dir = self.dir.lock().clone()?;
        Some(dir.join(format!("{}.json", safe(task_id))))
    }
}

/// A task's ledger as kept under `dir`, read without a live executor (R3-3a
/// costs every session's turns, live or not).
pub fn read_at(dir: &Path, task_id: &str) -> Vec<TurnRecord> {
    std::fs::read(dir.join(format!("{}.json", safe(task_id))))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The note for the agent's next message after an undo (or taking it back).
pub fn note(action: TurnAction, plan: &TurnPlan) -> String {
    let files: Vec<&str> = plan
        .files
        .iter()
        .map(|f| f.path.as_str())
        .take(20)
        .collect();
    let more = plan.files.len().saturating_sub(files.len());
    let list = if more > 0 {
        format!("{} 等 {} 个文件", files.join("、"), plan.files.len())
    } else {
        files.join("、")
    };
    let turns = plan.turns.len().max(1);
    match action {
        TurnAction::Undo | TurnAction::UndoFrom if turns > 1 => format!(
            "用户撤销了最近 {turns} 轮改动，这些文件已恢复到那几轮之前：{list}。不要假设那些改动还在。"
        ),
        TurnAction::Undo | TurnAction::UndoFrom => format!(
            "用户撤销了一轮改动，这些文件已恢复到那一轮之前：{list}。不要假设那些改动还在。"
        ),
        TurnAction::Restore if turns > 1 => {
            format!("用户撤回了 {turns} 轮的撤销，这些文件回到了那几轮的结果：{list}。")
        }
        TurnAction::Restore => format!("用户撤回了撤销，这些文件回到了那一轮的结果：{list}。"),
    }
}

/// A plan and what carrying it out writes: (path, tree to take it from, delete).
struct Planned {
    plan: TurnPlan,
    writes: Vec<(String, String, bool)>,
    top: PathBuf,
    members: Vec<String>,
}

impl Planned {
    fn blocked(reason: &str) -> Self {
        Planned {
            plan: TurnPlan {
                blocked: Some(reason.to_string()),
                ..TurnPlan::default()
            },
            writes: Vec::new(),
            top: PathBuf::new(),
            members: Vec::new(),
        }
    }
}

/// The blob id of `path` in `tree`, or `None` when it is not there.
fn blob(top: &Path, tree: &str, path: &str) -> Option<String> {
    git(
        top,
        &["rev-parse", "--verify", "-q", &format!("{tree}:{path}")],
        None,
    )
    .map(|s| s.trim().to_string())
}

/// The blob id the working tree's `path` would have; `None` when absent.
fn current(top: &Path, path: &str) -> Option<String> {
    let full = top.join(path);
    std::fs::symlink_metadata(&full).ok()?;
    git(top, &["hash-object", "--", path], None).map(|s| s.trim().to_string())
}

fn write_back(top: &Path, tree: &str, path: &str, delete: bool) -> Result<(), String> {
    let full = top.join(path);
    if delete {
        return match std::fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    let mode = git(top, &["ls-tree", tree, "--", path], None)
        .and_then(|line| line.split_whitespace().next().map(str::to_string))
        .unwrap_or_else(|| "100644".into());
    let bytes = git_bytes(top, &["cat-file", "blob", &format!("{tree}:{path}")])
        .ok_or("读不到快照里的内容")?;
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if mode == "120000" {
        let _ = std::fs::remove_file(&full);
        #[cfg(unix)]
        std::os::unix::fs::symlink(String::from_utf8_lossy(&bytes).to_string(), &full)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    std::fs::write(&full, &bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let exec = mode == "100755";
        if let Ok(meta) = std::fs::metadata(&full) {
            let mut perms = meta.permissions();
            let bits = perms.mode();
            perms.set_mode(if exec { bits | 0o111 } else { bits & !0o111 });
            let _ = std::fs::set_permissions(&full, perms);
        }
    }
    Ok(())
}

fn ref_name(task_id: &str, turn_id: &str, side: &str) -> String {
    format!("{REF_ROOT}/{}/{}/{side}", safe(task_id), safe(turn_id))
}

/// Ref- and file-name safe.
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
#[path = "turn_ledger_tests.rs"]
mod tests;
