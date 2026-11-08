//! R2-3b (2026-11-05): a session run in an isolated copy of its project's
//! directory — a detached git worktree at `<repo>/.aeroric/worktrees/<task>`
//! — whose changes the user brings back to the original directory hunk by
//! hunk.
//!
//! The copy starts from the original directory as it is (uncommitted edits
//! and new files included, ignored files not), and that tree is its *base*:
//! "relative to the original directory" is base → the copy now. Bringing a
//! hunk back applies it to the original working tree only (never the index,
//! never a commit); where the original directory moved on, it is a conflict.
//! Resolving merges the original directory's newer content into the copy
//! (`git merge-file`, markers where both sides changed) and moves the base
//! forward, so afterwards only the agent's own work is left to bring back.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aeroric_protocol::isolated_copy::CopyRecord;

use super::now_ms;
use super::turn_changes::{git, git_bytes, snapshot_tree};

/// A copy with nothing left to bring back is removed after this long.
pub const KEEP_CLEARED_MS: u64 = 30 * 24 * 3600 * 1000;
const REF_ROOT: &str = "refs/aeroric/copies";

/// The repository root of `project`, when it can have a copy: a local git
/// repository with at least one commit.
pub fn check(project: &Path) -> Result<PathBuf, String> {
    let top = git(project, &["rev-parse", "--show-toplevel"], None)
        .map(|s| PathBuf::from(s.trim()))
        .ok_or("项目不是 git 仓库，不能用独立副本")?;
    git(&top, &["rev-parse", "--verify", "-q", "HEAD"], None)
        .ok_or("仓库还没有任何提交，不能用独立副本")?;
    Ok(top)
}

/// Make `task_id`'s copy of `project`; the record (kept under `dir`) and the
/// working directory inside the copy matching `project`'s place in its repo.
pub fn create(dir: &Path, project: &Path, task_id: &str) -> Result<(CopyRecord, PathBuf), String> {
    let top = check(project)?;
    exclude(&top);
    let (_, base) = snapshot_tree(&top).ok_or("读不到原目录")?;
    let path = top.join(".aeroric").join("worktrees").join(safe(task_id));
    if path.exists() {
        return Err("这个会话的副本目录已经存在".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let path_text = path.to_string_lossy().to_string();
    git(
        &top,
        &["worktree", "add", "--detach", &path_text, "HEAD"],
        None,
    )
    .ok_or("建副本失败（git worktree add）")?;
    // The original directory's uncommitted edits and new files.
    let patch = git_bytes(&top, &["diff", "--binary", "--no-renames", "HEAD", &base])
        .ok_or("读不到原目录未提交的改动")?;
    if !patch.is_empty() && !apply(&path, &patch, false) {
        let _ = git(&top, &["worktree", "remove", "--force", &path_text], None);
        return Err("把未提交的改动复制进副本失败".into());
    }
    let _ = git(&top, &["update-ref", &base_ref(task_id), &base], None);
    let record = CopyRecord {
        task_id: task_id.to_string(),
        path: path_text,
        origin: top.to_string_lossy().to_string(),
        base,
        created_ms: now_ms(),
        cleared_ms: None,
    };
    save(dir, &record);
    let inside = project
        .canonicalize()
        .ok()
        .zip(top.canonicalize().ok())
        .and_then(|(p, t)| p.strip_prefix(t).ok().map(Path::to_path_buf))
        .unwrap_or_default();
    Ok((record, path.join(inside)))
}

/// Apply a patch in `top`'s working tree (never the index); `reverse` undoes it.
pub fn apply(top: &Path, patch: &[u8], reverse: bool) -> bool {
    let mut args = vec![
        "-C".to_string(),
        top.to_string_lossy().to_string(),
        "apply".into(),
    ];
    if reverse {
        args.push("-R".into());
    }
    args.push("--whitespace=nowarn".into());
    args.push("-".into());
    let Ok(mut child) = Command::new("git")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(patch);
    }
    child.wait().is_ok_and(|s| s.success())
}

/// What resolving did: the files merged in, and those left with markers.
pub struct Resolved {
    pub synced: Vec<String>,
    pub conflicted: Vec<String>,
}

/// Merge what the original directory changed since the base into the copy,
/// then move the base to the original directory as it is now.
pub fn resolve(dir: &Path, task_id: &str) -> Result<Resolved, String> {
    let mut record = load(dir, task_id).ok_or("这个会话不在独立副本里")?;
    let origin = PathBuf::from(&record.origin);
    let copy = PathBuf::from(&record.path);
    let (_, now) = snapshot_tree(&origin).ok_or("读不到原目录")?;
    let raw = git(
        &origin,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            &record.base,
            &now,
        ],
        None,
    )
    .ok_or("读不到原目录的改动")?;
    let mut out = Resolved {
        synced: Vec::new(),
        conflicted: Vec::new(),
    };
    for path in raw.split('\0').filter(|p| !p.is_empty()) {
        let base = blob(&origin, &record.base, path);
        let theirs = std::fs::read(origin.join(path)).ok();
        let target = copy.join(path);
        let mine = std::fs::read(&target).ok();
        let next = if mine == base {
            theirs
        } else if theirs == base {
            continue;
        } else {
            let (merged, clean) = merge(&mine, &base, &theirs)?;
            if !clean {
                out.conflicted.push(path.to_string());
            }
            Some(merged)
        };
        match next {
            Some(bytes) => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&target, bytes).map_err(|e| format!("{path}：{e}"))?;
            }
            None => {
                let _ = std::fs::remove_file(&target);
            }
        }
        out.synced.push(path.to_string());
    }
    record.base = now;
    let _ = git(
        &origin,
        &["update-ref", &base_ref(task_id), &record.base],
        None,
    );
    save(dir, &record);
    Ok(out)
}

/// The message prefilled for the agent after resolving.
pub fn prompt(record: &CopyRecord, resolved: &Resolved) -> String {
    let list = |files: &[String]| {
        let shown: Vec<&str> = files.iter().take(20).map(String::as_str).collect();
        let more = files.len().saturating_sub(shown.len());
        if more > 0 {
            format!("{} 等 {} 个文件", shown.join("、"), files.len())
        } else {
            shown.join("、")
        }
    };
    let mut text = format!(
        "原目录（{}）在副本起点之后改了这些文件：{}。已把原目录的新内容合进副本。",
        record.origin,
        list(&resolved.synced)
    );
    if resolved.conflicted.is_empty() {
        text.push_str("请检查合并后的内容是否仍符合你的改动意图，完成后回复。");
    } else {
        text.push_str(&format!(
            "这些文件里有冲突标记（<<<<<<< 副本 / >>>>>>> 原目录）：{}。请保留双方意图把它们整理好，删掉冲突标记，完成后回复。",
            list(&resolved.conflicted)
        ));
    }
    text
}

/// Remove the copy and everything kept for it.
pub fn remove(dir: &Path, task_id: &str) -> Result<(), String> {
    let record = load(dir, task_id).ok_or("这个会话不在独立副本里")?;
    let origin = PathBuf::from(&record.origin);
    if Path::new(&record.path).exists() {
        git(
            &origin,
            &["worktree", "remove", "--force", &record.path],
            None,
        )
        .ok_or("删除副本失败（git worktree remove）")?;
    }
    let _ = git(&origin, &["update-ref", "-d", &base_ref(task_id)], None);
    let _ = std::fs::remove_file(file(dir, task_id));
    Ok(())
}

/// Remove copies with nothing left to bring back for [`KEEP_CLEARED_MS`].
pub fn prune(dir: &Path, now: u64) -> Vec<String> {
    list(dir)
        .into_iter()
        .filter(|r| {
            r.cleared_ms
                .is_some_and(|at| now.saturating_sub(at) > KEEP_CLEARED_MS)
        })
        .filter(|r| remove(dir, &r.task_id).is_ok())
        .map(|r| r.task_id)
        .collect()
}

/// Note whether anything is left to bring back (for [`prune`]).
pub fn set_cleared(dir: &Path, task_id: &str, cleared: bool) {
    let Some(mut record) = load(dir, task_id) else {
        return;
    };
    let next = if cleared {
        record.cleared_ms.or(Some(now_ms()))
    } else {
        None
    };
    if next != record.cleared_ms {
        record.cleared_ms = next;
        save(dir, &record);
    }
}

pub fn load(dir: &Path, task_id: &str) -> Option<CopyRecord> {
    let bytes = std::fs::read(file(dir, task_id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn list(dir: &Path) -> Vec<CopyRecord> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect()
}

fn save(dir: &Path, record: &CopyRecord) {
    let _ = std::fs::create_dir_all(dir);
    let path = file(dir, &record.task_id);
    let tmp = path.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec_pretty(record) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn file(dir: &Path, task_id: &str) -> PathBuf {
    dir.join(format!("{}.json", safe(task_id)))
}

/// `/.aeroric/` in the repository's own exclude file, so the original
/// directory's snapshots never see the copies. The user's `.gitignore` is
/// left alone.
fn exclude(top: &Path) {
    let Some(rel) = git(top, &["rev-parse", "--git-path", "info/exclude"], None) else {
        return;
    };
    let path = top.join(rel.trim());
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.lines().any(|l| l.trim() == "/.aeroric/") {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let sep = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let _ = std::fs::write(&path, format!("{text}{sep}/.aeroric/\n"));
}

/// `path` in `tree`, or `None` when it is not there.
fn blob(top: &Path, tree: &str, path: &str) -> Option<Vec<u8>> {
    git(
        top,
        &["rev-parse", "--verify", "-q", &format!("{tree}:{path}")],
        None,
    )?;
    git_bytes(top, &["cat-file", "blob", &format!("{tree}:{path}")])
}

/// Three-way merge of whole files (missing = empty); (result, clean).
fn merge(
    mine: &Option<Vec<u8>>,
    base: &Option<Vec<u8>>,
    theirs: &Option<Vec<u8>>,
) -> Result<(Vec<u8>, bool), String> {
    let tmp = std::env::temp_dir().join(format!("aeroric-merge-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let write = |name: &str, bytes: &Option<Vec<u8>>| {
        let p = tmp.join(name);
        std::fs::write(&p, bytes.as_deref().unwrap_or_default()).map(|_| p)
    };
    let result = (|| {
        let (m, b, t) = (
            write("mine", mine).map_err(|e| e.to_string())?,
            write("base", base).map_err(|e| e.to_string())?,
            write("theirs", theirs).map_err(|e| e.to_string())?,
        );
        let out = Command::new("git")
            .args([
                "merge-file",
                "-p",
                "-L",
                "副本",
                "-L",
                "起点",
                "-L",
                "原目录",
            ])
            .arg(&m)
            .arg(&b)
            .arg(&t)
            // Run outside any repository: in a checkout git cannot resolve
            // (a worktree made on another machine) it exits 128 (C 批).
            .current_dir(&tmp)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        match out.status.code() {
            Some(0) => Ok((out.stdout, true)),
            Some(n) if n > 0 && n < 128 => Ok((out.stdout, false)),
            _ => Err("合并失败（git merge-file）".to_string()),
        }
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn base_ref(task_id: &str) -> String {
    format!("{REF_ROOT}/{}/base", safe(task_id))
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
#[path = "isolated_copy_tests.rs"]
mod tests;
