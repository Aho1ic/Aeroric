//! D1 (2026-11-21): a GitHub pull request pinned as a snapshot to review —
//! read only; nothing is written to GitHub.

use serde::{Deserialize, Serialize};

/// A repository, by the number GitHub gives it as well as its name, so a
/// rename does not make it another repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRef {
    /// `github.com`, or a GitHub Enterprise host.
    pub host: String,
    pub owner: String,
    pub name: String,
    /// GitHub's numeric id.
    pub repo_id: u64,
}

/// One changed file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullFile {
    pub path: String,
    /// `added`, `modified`, `removed`, `renamed`…
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
}

/// One check run on the head commit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullCheck {
    pub name: String,
    /// `queued`, `in_progress`, `completed`.
    pub status: String,
    /// When completed: `success`, `failure`, `neutral`, `cancelled`, `skipped`…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub url: String,
}

/// Where a review comment sits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentAnchor {
    pub path: String,
    /// `LEFT` (the old side) or `RIGHT`.
    #[serde(default)]
    pub side: String,
    /// `None` when GitHub no longer places it on a line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    pub commit_sha: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullComment {
    pub author: String,
    pub body: String,
    /// A review comment's place; `None` for a comment on the whole PR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<CommentAnchor>,
    /// Its commit is not the snapshot's head (针对旧版本).
    #[serde(default)]
    pub outdated: bool,
    #[serde(default)]
    pub at: String,
}

/// A pull request as it was when fetched.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullSnapshot {
    /// `<owner>-<name>-<number>-<head sha 12>`: the same head fetched again
    /// is the same snapshot.
    pub id: String,
    pub project_id: String,
    pub repo: RepoRef,
    pub number: u64,
    pub title: String,
    pub author: String,
    /// `open`, `closed`; merged shows as closed with `merged`.
    pub state: String,
    #[serde(default)]
    pub merged: bool,
    pub url: String,
    pub head_ref: String,
    pub base_ref: String,
    pub head_sha: String,
    pub base_sha: String,
    #[serde(default)]
    pub merge_base: String,
    pub fetched_ms: u64,
    pub files: Vec<PullFile>,
    pub checks: Vec<PullCheck>,
    pub comments: Vec<PullComment>,
    /// Files, checks or comments past their cap were left out.
    #[serde(default)]
    pub truncated: bool,
    /// Set when listed: a newer snapshot of the same PR has another head
    /// (PR 已更新).
    #[serde(default)]
    pub superseded: bool,
}
