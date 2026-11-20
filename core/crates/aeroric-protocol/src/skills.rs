//! C7 (2026-11-20): the skill catalog — every skill a session can be given,
//! whatever stores it — and what one launch of a session mounted.
//!
//! Four sources keep their own storage; each is projected into a
//! [`SkillDefinition`].

use serde::{Deserialize, Serialize};

/// `skills.catalog {projectId?}`: every skill, with which one wins a name.
pub const CATALOG_METHOD: &str = "skills.catalog";
/// `skills.mounted {taskId}`: what each launch of a session mounted.
pub const MOUNTED_METHOD: &str = "skills.mounted";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    /// Shipped inside the App (version = the App's skills bundle).
    Builtin,
    /// Learned from memory and accepted by the user (B5-3b).
    Learned,
    /// In the user's own skill folders, which the agent reads itself.
    User,
    /// Inside a Claude Code plugin the user installed.
    Plugin,
}

/// One skill as the catalog shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDefinition {
    /// The name a session calls it by (a plugin's is `plugin:skill`).
    pub id: String,
    pub name: String,
    pub source: SkillSource,
    /// Readable: `随应用内置`, `v3`, a plugin's version; empty when none.
    pub version: String,
    /// Short hash of its content (the bundle's for a built-in one).
    pub content_hash: String,
    /// Its folder on disk; none for a built-in one, which lives in the App.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub embedded: bool,
    #[serde(default)]
    pub summary: String,
    /// The harnesses that can be given it.
    #[serde(default)]
    pub harnesses: Vec<String>,
    /// Given to sessions (in the project asked about, when one was).
    pub active: bool,
    /// Why not, or where only: 已关闭, 被同名的…盖过, 只在某项目.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Why a launch had a skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MountReason {
    /// A built-in skill, on unless the user turned it off.
    Default,
    /// A learned skill the user accepted (for this project, or every one).
    Accepted,
    /// In the user's own folder: the agent reads it itself.
    UserFolder,
    /// From a Claude Code plugin: Claude Code loads it itself.
    Plugin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountedSkill {
    pub skill_id: String,
    pub source: SkillSource,
    pub version: String,
    pub content_hash: String,
    pub reason: MountReason,
}

/// One launch (start or resume) of a session and the skills it had.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLaunch {
    pub at_ms: u64,
    pub harness: String,
    pub skills: Vec<MountedSkill>,
}
