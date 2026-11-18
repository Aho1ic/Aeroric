//! B6 (2026-11-18): what a task delivered. Every file a completed turn added
//! or changed is an artifact; each turn that changed it again is a new
//! version. A version is a reference to the Git blob the turn's snapshot
//! holds, never a copy; its content is read back from the repository.
//!
//! Whether a version was undone, superseded, or which checks it went
//! through is worked out when it is read, from the turn ledger.

use serde::{Deserialize, Serialize};

use crate::checkpoint::EvidenceRef;

pub const LIST_METHOD: &str = "artifacts.list";
pub const VERSIONS_METHOD: &str = "artifacts.versions";
pub const READ_METHOD: &str = "artifacts.read";
pub const EXCLUDE_METHOD: &str = "artifacts.exclude";

/// When a version's content is gone from the repository.
pub const GONE: &str = "这个版本的内容已不可用";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    Code,
    Document,
    Data,
    Image,
    #[default]
    Other,
}

/// One delivered file of a task, keyed by its path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub task_id: String,
    /// Relative to the repository's top directory.
    pub path: String,
    pub kind: ArtifactKind,
    pub created_ms: u64,
    pub latest_version: u32,
    /// The user said it does not count as a deliverable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub excluded: bool,
    /// A later turn deleted the file; its versions are kept.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
    /// The version that stands now: the newest one not undone (none when
    /// every version was undone). Worked out when read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<ArtifactVersion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVersion {
    pub artifact_id: String,
    /// 1, 2, 3 … in the order the turns made them.
    pub version: u32,
    pub turn_id: String,
    /// The Git blob id; it is also the content's hash.
    pub blob: String,
    pub size: u64,
    pub at_ms: u64,
    /// The turn that made it was undone. Worked out when read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub withdrawn: bool,
    /// A later version (not undone) stands instead. Worked out when read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub superseded: bool,
    /// The project's checks that ran while this version stood. Worked out
    /// when read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<ValidationRecord>,
}

/// One check that saw this version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRecord {
    /// An `autoCheck` reference; `detail` is the turn that ran it.
    pub evidence: EvidenceRef,
    /// `passed` | `failed`.
    pub verdict: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// "结果可能不完整" (B4).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub incomplete: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    /// `artifacts.list`: where the previous page stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// `artifacts.read`: the version (default: the current one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// `artifacts.read`: the first byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// `artifacts.read`: at most this many bytes (capped at [`READ_MAX`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// `artifacts.exclude`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded: Option<bool>,
}

/// One page of a task's artifacts, the latest changed first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPage {
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Artifacts let go to keep the task under its limit: "只列了最近 N 个".
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped: u32,
    /// The task works where versions cannot be kept (not a Git repository).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

pub const READ_DEFAULT: u64 = 64 * 1024;
pub const READ_MAX: u64 = 256 * 1024;

/// A slice of one version's bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactContent {
    pub artifact_id: String,
    pub version: u32,
    pub blob: String,
    pub size: u64,
    pub offset: u64,
    /// `utf8` when the slice is text, else `base64`.
    pub encoding: String,
    pub content: String,
    /// Where the next slice starts; unset at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u64>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}
