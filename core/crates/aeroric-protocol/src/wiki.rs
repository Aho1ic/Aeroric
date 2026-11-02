//! B5.5-1 project Wiki (2026-10-31): pages compiled from the project's own
//! notes and what Aeroric learned, kept per project, readable and editable
//! by the user, read by agents (frontier-roadmap §4; Q-W1–Q-W6). A
//! paragraph the user changed is never overwritten by a compile: the
//! compiler opens an issue instead.

use serde::{Deserialize, Serialize};

pub const PAGES_METHOD: &str = "wiki.pages";
pub const PAGE_METHOD: &str = "wiki.page";
pub const COMPILE_METHOD: &str = "wiki.compile";
pub const EDIT_METHOD: &str = "wiki.edit";
pub const REVISIONS_METHOD: &str = "wiki.revisions";
pub const REVERT_METHOD: &str = "wiki.revert";
pub const ISSUES_METHOD: &str = "wiki.issues";
pub const ISSUE_ACTION_METHOD: &str = "wiki.issueAction";
pub const EXPORT_PLAN_METHOD: &str = "wiki.exportPlan";
pub const EXPORT_METHOD: &str = "wiki.export";
pub const SEARCH_METHOD: &str = "wiki.search";

/// `wiki.export`'s folder unless the user picks another.
pub const DEFAULT_EXPORT_DIR: &str = "docs/wiki";
/// Hits `search_wiki` returns.
pub const SEARCH_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WikiPageKind {
    Index,
    Decision,
    Runbook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Note,
    Memory,
    Skill,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub kind: SourceKind,
    /// A note's path relative to the project, `#` its section; a memory id;
    /// a skill name.
    pub target: String,
    pub label: String,
}

/// One paragraph (a `##` section) of a page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiBlock {
    /// Stable across compiles: `note:<path>#<section>`, `memory:<id>`, …
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    /// The user changed (or removed: empty text) it; compiles keep it.
    #[serde(default)]
    pub user_edited: bool,
    /// Hash of what the compiler last produced for this block.
    #[serde(default)]
    pub compiled_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiPage {
    pub slug: String,
    pub kind: WikiPageKind,
    pub title: String,
    /// A decision's `Status:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// A decision's notes folder (`feature`, `architecture`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub blocks: Vec<WikiBlock>,
    pub revision: u32,
    pub updated_ms: u64,
    /// 2026-11-02: the user renamed the page; compiling keeps their title.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub title_edited: bool,
    /// The title the compiler last produced, to notice when its source changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiled_title: Option<String>,
}

/// An issue about a page's title rather than a block.
pub const TITLE_BLOCK_KEY: &str = "title";

/// The page tree's row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiPageSummary {
    pub slug: String,
    pub kind: WikiPageKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub revision: u32,
    pub edited: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevisionReason {
    Compile,
    Edit,
    Revert,
    Issue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiRevision {
    pub slug: String,
    pub n: u32,
    pub reason: RevisionReason,
    pub markdown: String,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WikiIssueKind {
    /// The source of a paragraph the user changed moved on.
    SourceChanged,
    /// The source of a paragraph (or page) the user changed is gone.
    SourceRemoved,
    Agent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiIssue {
    pub id: String,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_key: Option<String>,
    pub kind: WikiIssueKind,
    pub text: String,
    /// What the compiler would have written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    pub open: bool,
    /// `compiler`, `user`, or the session that flagged it.
    pub author: String,
    pub at_ms: u64,
}

/// R1-1: sources the last compile could not read. Their pages stay as they
/// were; only a confirmed removal takes content away.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiSourceGaps {
    /// The project's folder or notes folder could not be read at all.
    #[serde(default)]
    pub all: bool,
    /// Decision pages whose note could not be read.
    #[serde(default)]
    pub slugs: Vec<String>,
}

/// `wiki.exportPlan`: what an export would write.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiExportPlan {
    pub files: Vec<String>,
    /// Those of `files` already there.
    #[serde(default)]
    pub overwrites: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileReport {
    pub added: u32,
    pub updated: u32,
    pub removed: u32,
    pub issues: u32,
    pub pages: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiHit {
    pub slug: String,
    pub title: String,
    pub excerpt: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_camel_case() {
        assert_eq!(
            serde_json::to_value(WikiPageKind::Runbook).unwrap(),
            "runbook"
        );
        assert_eq!(
            serde_json::to_value(WikiIssueKind::SourceChanged).unwrap(),
            "sourceChanged"
        );
    }
}
