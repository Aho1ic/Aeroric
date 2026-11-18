//! R2-1 (2026-11-04): the turns that changed files, per task, so each can be
//! accepted, undone, or its undo taken back.
//!
//! A turn's two snapshot trees are pinned under `refs/aeroric/turns/…` in
//! its repository so `git gc` keeps them; at most [`KEEP_PER_REPO`] turns
//! and [`KEEP_MS`] are kept per repository. Undo writes files from the
//! snapshot straight into the working tree: the index, HEAD and every branch
//! stay as they are. A file whose content is no longer what the turn left
//! (a later turn, or the user, changed it) is skipped, never overwritten.
//! A7 (2026-11-17): a file already as the undo would leave it is in place,
//! not skipped; each write goes through a temporary file and a rename; a
//! half-done undo is reported file by file and finished by trying again.
//! B1-2 (2026-11-18): a path through a symlinked folder is never touched; a
//! file is in place only when its content and its mode both match; a
//! symlink is restored by a rename like a file.

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
/// B1-2: the path leads through a symlink (or something that is not a
/// folder), or out of the project; undo never writes or deletes through it.
pub const SKIP_UNSAFE: &str = "路径经过符号链接或越出项目，没有动它";

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
            if inside(&top, &path).is_err() {
                out.plan.skipped.push(SkippedFile {
                    path,
                    reason: SKIP_UNSAFE.into(),
                });
                continue;
            }
            let now = current(&top, &path);
            let wanted = entry(&top, target, &path);
            if now == wanted {
                // A7: already as the action would leave it — by hand, or by
                // an earlier try that stopped half way.
                out.plan.in_place.push(path);
            } else if now == entry(&top, expected, &path) {
                let delete = wanted.is_none();
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
    /// Returns what was done (a blocked plan does nothing). A7: every file is
    /// tried; when any could not be written the result lists it in `failed`,
    /// `files` holds only what was written, and the turns keep their state —
    /// trying again finds the written ones `in_place` and finishes the rest.
    pub fn apply(
        &self,
        task_id: &str,
        turn_id: &str,
        action: TurnAction,
        running: bool,
    ) -> Result<TurnPlan, String> {
        let mut planned = self.planned(task_id, turn_id, action, running);
        if let Some(reason) = &planned.plan.blocked {
            return Err(reason.clone());
        }
        let mut written = Vec::new();
        for ((path, tree, delete), file) in planned.writes.iter().zip(planned.plan.files.drain(..))
        {
            match write_back(&planned.top, tree, path, *delete) {
                Ok(()) => written.push(file),
                Err(reason) => planned.plan.failed.push(SkippedFile {
                    path: path.clone(),
                    reason,
                }),
            }
        }
        planned.plan.files = written;
        if !planned.plan.failed.is_empty() {
            return Ok(planned.plan);
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
            // B5-3: through the shared atomic writer; a failure is said.
            if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
                if let Err(error) = crate::storage::atomic::write_atomic(
                    &path,
                    &bytes,
                    crate::storage::atomic::FileMode::Private,
                ) {
                    eprintln!("turn ledger: saving {task_id} failed: {error}");
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

/// Whether an undo's result is worth a note to the agent: something is now
/// as the undo leaves it.
pub fn worth_a_note(plan: &TurnPlan) -> bool {
    !plan.files.is_empty() || !plan.in_place.is_empty()
}

/// The note for the agent's next message after an undo (or taking it back).
/// A7: names what was written and what was already in place (a retry after
/// a half-done undo names the first try's files too), and what failed.
pub fn note(action: TurnAction, plan: &TurnPlan) -> String {
    let all: Vec<&str> = plan
        .files
        .iter()
        .map(|f| f.path.as_str())
        .chain(plan.in_place.iter().map(String::as_str))
        .collect();
    let list = names(&all);
    let turns = plan.turns.len().max(1);
    if !plan.failed.is_empty() {
        let failed: Vec<&str> = plan.failed.iter().map(|f| f.path.as_str()).collect();
        let what = if matches!(action, TurnAction::Restore) {
            "撤回撤销"
        } else {
            "撤销改动"
        };
        return format!(
            "用户{what}时只做完了一部分：这些文件已改好：{list}；这些没能改：{}，仍是原来的内容。",
            names(&failed)
        );
    }
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

/// `a、b` — the first 20, then `等 N 个文件`.
fn names(paths: &[&str]) -> String {
    let shown = &paths[..paths.len().min(20)];
    if paths.len() > shown.len() {
        format!("{} 等 {} 个文件", shown.join("、"), paths.len())
    } else {
        shown.join("、")
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

/// What a path holds: its object id and git mode (`100644`, `100755`,
/// `120000`). Content alone is not enough: a `chmod` or a file swapped for a
/// symlink to the same text is a change too.
type Entry = (String, String);

/// `path` in `tree`, or `None` when it is not there.
fn entry(top: &Path, tree: &str, path: &str) -> Option<Entry> {
    let out = git(top, &["ls-tree", "-z", tree, "--", path], None)?;
    let line = out.split('\0').next()?;
    let (meta, _) = line.split_once('\t')?;
    let mut parts = meta.split_whitespace();
    let mode = parts.next()?.to_string();
    let _kind = parts.next()?;
    Some((parts.next()?.to_string(), mode))
}

/// The working tree's `path` as `git add` would record it; `None` when
/// absent. Something there that cannot be hashed (a folder, an unreadable
/// file) matches no snapshot, so it is never mistaken for absent.
fn current(top: &Path, path: &str) -> Option<Entry> {
    let full = top.join(path);
    let meta = std::fs::symlink_metadata(&full).ok()?;
    let unknown = || Some(("?".to_string(), "?".to_string()));
    if meta.file_type().is_symlink() {
        // `hash-object <path>` would read through the link; git records the
        // link's own text.
        let Ok(target) = std::fs::read_link(&full) else {
            return unknown();
        };
        return match hash_bytes(top, target.as_os_str().as_encoded_bytes()) {
            Some(id) => Some((id, "120000".into())),
            None => unknown(),
        };
    }
    if !meta.file_type().is_file() {
        return unknown();
    }
    // With the path's filters, as the snapshot's `git add` applied them.
    let Some(id) = git(top, &["hash-object", "--", path], None) else {
        return unknown();
    };
    #[cfg(unix)]
    let exec = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o100 != 0
    };
    #[cfg(not(unix))]
    let exec = false;
    let mode = if exec { "100755" } else { "100644" };
    Some((id.trim().to_string(), mode.into()))
}

/// The blob id of `bytes`.
fn hash_bytes(top: &Path, bytes: &[u8]) -> Option<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(["hash-object", "--stdin"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(bytes).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `path` under `top`, refused when it is absolute, climbs out, names
/// `.git`, or passes through anything but a real folder: a folder swapped
/// for a symlink after the turn would otherwise take the write (or the
/// delete) outside the project.
fn inside(top: &Path, path: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let parts: Vec<Component> = Path::new(path).components().collect();
    if parts.is_empty() {
        return Err(SKIP_UNSAFE.into());
    }
    let mut here = top.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        match part {
            Component::Normal(name) if !name.eq_ignore_ascii_case(".git") => here.push(name),
            _ => return Err(SKIP_UNSAFE.into()),
        }
        if i + 1 == parts.len() {
            break;
        }
        match std::fs::symlink_metadata(&here) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => return Err(SKIP_UNSAFE.into()),
            // Not there yet: it is created as a real folder.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(here)
}

fn write_back(top: &Path, tree: &str, path: &str, delete: bool) -> Result<(), String> {
    let full = inside(top, path)?;
    if delete {
        return match std::fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    let (_, mode) = entry(top, tree, path).ok_or("读不到快照里的内容")?;
    let bytes = git_bytes(top, &["cat-file", "blob", &format!("{tree}:{path}")])
        .ok_or("读不到快照里的内容")?;
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Again after creating folders: nothing may have slipped a link in.
    let full = inside(top, path)?;
    if mode == "120000" {
        return replace_link(&full, &bytes).map_err(|e| e.to_string());
    }
    replace_file(&full, &bytes, mode == "100755").map_err(|e| e.to_string())
}

/// A temporary name beside `full`.
fn beside(full: &Path) -> PathBuf {
    let parent = full.parent().unwrap_or(Path::new("."));
    let name = full
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    parent.join(format!(
        ".{name}.aeroric-undo-{}",
        uuid::Uuid::new_v4().simple()
    ))
}

/// B1-2: a symlink made beside `full` and renamed over it, so a failure
/// leaves what was there.
fn replace_link(full: &Path, target: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let tmp = beside(full);
        std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(target), &tmp)?;
        let renamed = std::fs::rename(&tmp, full);
        if renamed.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        renamed
    }
    #[cfg(not(unix))]
    {
        let _ = (full, target);
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// A7: `bytes` into a temporary file beside `full`, then renamed over it, so
/// an interrupted write leaves the old content or the new, never half. The
/// file keeps its permission bits (a new one gets the default), with the
/// executable bits as `exec` says. A symlink at `full` is replaced, not
/// written through.
fn replace_file(full: &Path, bytes: &[u8], exec: bool) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = beside(full);
    let kept = std::fs::symlink_metadata(full)
        .ok()
        .filter(|meta| meta.file_type().is_file())
        .map(|meta| meta.permissions());
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = match &kept {
                Some(perms) => perms.clone(),
                None => file.metadata()?.permissions(),
            };
            let bits = perms.mode();
            perms.set_mode(if exec { bits | 0o111 } else { bits & !0o111 });
            file.set_permissions(perms)?;
        }
        #[cfg(not(unix))]
        let _ = (&kept, exec);
        drop(file);
        std::fs::rename(&tmp, full)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
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
