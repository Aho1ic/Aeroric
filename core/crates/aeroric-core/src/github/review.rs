//! D1 second step (2026-11-21): reviewing a snapshot. The PR's head is
//! fetched into `refs/aeroric/pr/<n>/<sha 12>` (the user's branches, index
//! and working tree untouched), checked out detached at
//! `<repo>/.aeroric/pr-reviews/<snapshot id>`, and a read-only session there
//! gets [`prompt`]. Its answer ends in a findings block that [`findings`]
//! checks against the snapshot and the checked-out files. Reviews are kept
//! as drafts beside the snapshots.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aeroric_protocol::github::{PullFinding, PullReview, PullSnapshot, RepoRef};
use serde::Deserialize;

/// Files listed in the prompt; the rest are counted.
const PROMPT_FILES: usize = 120;
/// Comments quoted in the prompt.
const PROMPT_COMMENTS: usize = 20;
const COMMENT_CHARS: usize = 300;
/// Findings kept per review.
pub const MAX_FINDINGS: usize = 50;

/// git with its error said: stdout, or what git wrote to stderr.
fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("运行 git 失败：{e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn short(sha: &str) -> String {
    sha.chars().take(12).collect()
}

/// The ref the head is fetched into.
pub fn head_ref(snapshot: &PullSnapshot) -> String {
    format!(
        "refs/aeroric/pr/{}/{}",
        snapshot.number,
        short(&snapshot.head_sha)
    )
}

fn normalized(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
        .replace(':', "/")
}

/// Where to fetch from: a remote of the repository whose URL is this
/// repository (its credentials are the user's own), else its https URL.
pub fn remote_for(top: &Path, repo: &RepoRef) -> String {
    let want = format!(
        "{}/{}/{}",
        repo.host.to_ascii_lowercase(),
        repo.owner.to_ascii_lowercase(),
        repo.name.to_ascii_lowercase()
    );
    let listed = git(top, &["remote", "-v"]).unwrap_or_default();
    for line in listed.lines() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(url)) = (parts.next(), parts.next()) else {
            continue;
        };
        if normalized(url).ends_with(&want) {
            return name.to_string();
        }
    }
    format!("https://{}/{}/{}.git", repo.host, repo.owner, repo.name)
}

/// Fetch the PR's head into [`head_ref`]; it must be the snapshot's head —
/// a PR pushed to since says so. The base is fetched when missing; without
/// it the review still runs on the head.
pub fn fetch(top: &Path, remote: &str, snapshot: &PullSnapshot) -> Result<String, String> {
    let target = head_ref(snapshot);
    let have = git(
        top,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("{target}^{{commit}}"),
        ],
    );
    if have.as_deref() != Ok(snapshot.head_sha.as_str()) {
        let spec = format!("+refs/pull/{}/head:{target}", snapshot.number);
        git(top, &["fetch", "--no-tags", "--quiet", remote, &spec])
            .map_err(|e| format!("取不到 PR 的代码：{e}"))?;
        let got = git(
            top,
            &[
                "rev-parse",
                "--verify",
                "-q",
                &format!("{target}^{{commit}}"),
            ],
        )
        .map_err(|_| "取到的 PR 代码读不出来".to_string())?;
        if got != snapshot.head_sha {
            let _ = git(top, &["update-ref", "-d", &target]);
            return Err(format!(
                "PR 在快照之后又有了新提交（现在是 {}），先刷新快照再审",
                short(&got)
            ));
        }
    }
    let base = format!("{}^{{commit}}", snapshot.base_sha);
    if !snapshot.base_sha.is_empty() && git(top, &["cat-file", "-e", &base]).is_err() {
        let spec = format!(
            "+refs/heads/{}:refs/aeroric/pr/{}/base",
            snapshot.base_ref, snapshot.number
        );
        let _ = git(top, &["fetch", "--no-tags", "--quiet", remote, &spec]);
    }
    Ok(target)
}

/// The checkout the reviewer works in: detached at the head, made once.
pub fn checkout(top: &Path, snapshot: &PullSnapshot) -> Result<PathBuf, String> {
    let path = top
        .join(".aeroric")
        .join("pr-reviews")
        .join(snapshot.id.replace(['/', '\\'], "_"));
    if git(&path, &["rev-parse", "HEAD"]).as_deref() == Ok(snapshot.head_sha.as_str()) {
        return Ok(path);
    }
    crate::sessions::live::isolated_copy::exclude(top);
    if path.exists() {
        let _ = git(
            top,
            &["worktree", "remove", "--force", &path.to_string_lossy()],
        );
        let _ = std::fs::remove_dir_all(&path);
    }
    git(
        top,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            &path.to_string_lossy(),
            &snapshot.head_sha,
        ],
    )
    .map_err(|e| format!("检出 PR 的代码失败：{e}"))?;
    Ok(path)
}

/// What the reviewer is told: the PR, read only, where to look, and the
/// findings block to end with.
pub fn prompt(snapshot: &PullSnapshot) -> String {
    let s = snapshot;
    let base = if s.merge_base.is_empty() {
        &s.base_sha
    } else {
        &s.merge_base
    };
    let mut out = format!(
        "请审查 GitHub PR #{}「{}」（{}/{}，作者 {}）。\n\
当前目录是这个 PR 的 head 提交 {}（分支 {}），目标分支是 {}。看改动：`git diff {}...HEAD`。\n\
只读：不要改文件、不要提交、不要向 GitHub 发任何东西，结论由 Aeroric 存成草稿。\n\n",
        s.number,
        s.title,
        s.repo.owner,
        s.repo.name,
        s.author,
        short(&s.head_sha),
        s.head_ref,
        s.base_ref,
        short(base)
    );
    out.push_str(&format!(
        "变更文件（{} 个{}）：\n",
        s.files.len(),
        if s.truncated {
            "，列表不完整"
        } else {
            ""
        }
    ));
    for f in s.files.iter().take(PROMPT_FILES) {
        out.push_str(&format!(
            "- {} {} +{} -{}\n",
            f.status, f.path, f.additions, f.deletions
        ));
    }
    if s.files.len() > PROMPT_FILES {
        out.push_str(&format!(
            "- 另有 {} 个，用 git diff --stat 看\n",
            s.files.len() - PROMPT_FILES
        ));
    }
    if !s.checks.is_empty() {
        out.push_str("\n检查：\n");
        for c in &s.checks {
            let result = c.conclusion.clone().unwrap_or_else(|| c.status.clone());
            out.push_str(&format!("- {}：{}\n", c.name, result));
        }
    }
    if !s.comments.is_empty() {
        out.push_str("\n已有评论（别重复提）：\n");
        for c in s.comments.iter().take(PROMPT_COMMENTS) {
            let place = match &c.anchor {
                Some(a) => format!(
                    "{}{}",
                    a.path,
                    a.line.map(|l| format!(":{l}")).unwrap_or_default()
                ),
                None => "整个 PR".into(),
            };
            let body: String = c.body.chars().take(COMMENT_CHARS).collect();
            let old = if c.outdated {
                "（针对旧版本）"
            } else {
                ""
            };
            out.push_str(&format!(
                "- {}{old} @{}：{}\n",
                place,
                c.author,
                body.replace('\n', " ")
            ));
        }
    }
    out.push_str(
        "\n审查完，回答的最后放一个 json 代码块，只列确实需要改的问题：\n\
```json\n{\"findings\": [{\"path\": \"相对仓库根的文件路径\", \"line\": 12, \"severity\": \"high 或 medium 或 low\", \
\"body\": \"问题和怎么改\", \"basis\": \"依据：你看到的代码或检查结果\"}]}\n```\n\
没有问题就写 {\"findings\": []}。\n",
    );
    out
}

#[derive(Deserialize)]
struct RawFindings {
    #[serde(default)]
    findings: Vec<RawFinding>,
}

#[derive(Deserialize)]
struct RawFinding {
    #[serde(default)]
    path: String,
    #[serde(default)]
    line: Option<serde_json::Value>,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    basis: String,
}

/// The last findings block in `answer`: a ```json fence, else the last
/// `{"findings"…}` object.
fn block(answer: &str) -> Option<RawFindings> {
    let fenced = answer.rmatch_indices("```json").find_map(|(at, _)| {
        let rest = &answer[at + 7..];
        let end = rest.find("```")?;
        serde_json::from_str::<RawFindings>(rest[..end].trim()).ok()
    });
    fenced.or_else(|| {
        let at = answer.rfind("{\"findings\"")?;
        let end = answer.rfind('}')?;
        (end > at)
            .then(|| serde_json::from_str::<RawFindings>(&answer[at..=end]).ok())
            .flatten()
    })
}

/// The reviewer's findings, checked: the path must be one of the PR's files
/// (a leading `./` or `/` dropped) and the body said; a line past the end of
/// the file at the head is dropped to no line; an unknown severity is
/// `medium`. `None` when the answer has no findings block.
pub fn findings(
    answer: &str,
    snapshot: &PullSnapshot,
    checkout: &Path,
) -> Option<(Vec<PullFinding>, u32)> {
    let raw = block(answer)?;
    let files: std::collections::HashSet<&str> =
        snapshot.files.iter().map(|f| f.path.as_str()).collect();
    let mut out = Vec::new();
    let mut dropped = 0;
    for f in raw.findings {
        let path = f
            .path
            .trim()
            .trim_start_matches("./")
            .trim_start_matches('/')
            .to_string();
        let known =
            files.contains(path.as_str()) || (snapshot.truncated && checkout.join(&path).is_file());
        if !known || f.body.trim().is_empty() || out.len() >= MAX_FINDINGS {
            dropped += 1;
            continue;
        }
        let line = f
            .line
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
        let lines = std::fs::read_to_string(checkout.join(&path))
            .map(|t| t.lines().count() as u64)
            .ok();
        let line = line.filter(|l| *l >= 1 && lines.is_none_or(|n| *l <= n));
        let severity = match f.severity.trim().to_ascii_lowercase().as_str() {
            s @ ("high" | "medium" | "low") => s.to_string(),
            _ => "medium".into(),
        };
        out.push(PullFinding {
            path,
            line,
            severity,
            body: f.body.trim().to_string(),
            basis: f.basis.trim().to_string(),
        });
    }
    Some((out, dropped))
}

fn reviews_dir(data_dir: &Path, project_id: &str) -> PathBuf {
    let safe: String = project_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_dir.join("github").join(safe).join("reviews")
}

pub fn save(data_dir: &Path, review: &PullReview) -> Result<(), String> {
    let dir = reviews_dir(data_dir, &review.project_id);
    crate::settings::ensure_private_dir(&dir)?;
    let text = serde_json::to_string_pretty(review).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(&dir.join(format!("{}.json", review.id)), &text)
}

/// A project's reviews, newest first; one whose snapshot is superseded is
/// `outdated`.
pub fn list(data_dir: &Path, project_id: &str) -> Vec<PullReview> {
    let Ok(entries) = std::fs::read_dir(reviews_dir(data_dir, project_id)) else {
        return Vec::new();
    };
    let snapshots = super::store::list(data_dir, project_id);
    let mut out: Vec<PullReview> = entries
        .flatten()
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    for r in &mut out {
        r.outdated = snapshots
            .iter()
            .find(|s| s.id == r.snapshot_id)
            .is_some_and(|s| s.superseded);
    }
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
    out
}

pub fn get(data_dir: &Path, project_id: &str, id: &str) -> Option<PullReview> {
    list(data_dir, project_id).into_iter().find(|r| r.id == id)
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
