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
    PlannedFile, SkippedFile, TurnAction, TurnPlan, TurnRecord, TurnState,
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
        let Some(record) = self.get(task_id, turn_id) else {
            return blocked("没有这一轮的快照");
        };
        if running {
            return blocked("这一轮结束后可撤销");
        }
        match (action, record.state) {
            (TurnAction::Undo, TurnState::Pending | TurnState::Accepted) => {}
            (TurnAction::Restore, TurnState::Undone) => {}
            (TurnAction::Undo, _) => return blocked("这一轮已经撤销了"),
            (TurnAction::Restore, _) => return blocked("这一轮没有撤销过"),
        }
        if record.head_before != record.head_after {
            let commit = record
                .head_after
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(7)
                .collect::<String>();
            return blocked(&format!(
                "这一轮提交了 {commit}，撤销会改写历史，请自己处理"
            ));
        }
        let top = PathBuf::from(&record.top);
        let (from, to) = sides(&record, action);
        for tree in [from, to] {
            if git(&top, &["cat-file", "-e", &format!("{tree}^{{tree}}")], None).is_none() {
                return blocked("快照已经被清理，无法撤销");
            }
        }
        let Some(raw) = git(
            &top,
            &["diff", "--name-only", "-z", "--no-renames", from, to],
            None,
        ) else {
            return blocked("读不到这一轮的快照");
        };
        let mut plan = TurnPlan::default();
        for path in raw.split('\0').filter(|p| !p.is_empty()) {
            let expected = blob(&top, from, path);
            if current(&top, path) == expected {
                plan.files.push(PlannedFile {
                    path: path.to_string(),
                    delete: blob(&top, to, path).is_none(),
                });
            } else {
                plan.skipped.push(SkippedFile {
                    path: path.to_string(),
                    reason: SKIP_CHANGED.into(),
                });
            }
        }
        plan
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
        let plan = self.plan(task_id, turn_id, action, running);
        if let Some(reason) = &plan.blocked {
            return Err(reason.clone());
        }
        let record = self.get(task_id, turn_id).ok_or("没有这一轮的快照")?;
        let top = PathBuf::from(&record.top);
        let (_, to) = sides(&record, action);
        for file in &plan.files {
            write_back(&top, to, &file.path, file.delete)
                .map_err(|e| format!("{}：{e}", file.path))?;
        }
        self.update(task_id, turn_id, |r| {
            match action {
                TurnAction::Undo => {
                    r.state = TurnState::Undone;
                    r.undone_ms = Some(now_ms());
                }
                TurnAction::Restore => {
                    r.state = TurnState::Pending;
                    r.undone_ms = None;
                }
            }
            Ok(())
        })?;
        Ok(plan)
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
    match action {
        TurnAction::Undo => format!(
            "用户撤销了一轮改动，这些文件已恢复到那一轮之前：{list}。不要假设那些改动还在。"
        ),
        TurnAction::Restore => format!("用户撤回了撤销，这些文件回到了那一轮的结果：{list}。"),
    }
}

fn blocked(reason: &str) -> TurnPlan {
    TurnPlan {
        blocked: Some(reason.to_string()),
        ..TurnPlan::default()
    }
}

/// (what the files should be now, what they become).
fn sides(record: &TurnRecord, action: TurnAction) -> (&str, &str) {
    match action {
        TurnAction::Undo => (&record.after, &record.before),
        TurnAction::Restore => (&record.before, &record.after),
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
