//! D5 (2026-11-21): a parent task's work split into a few units with
//! dependencies. Each unit is one ordinary delegation; the plan only adds
//! the order, the limits and why a unit is waiting.

use serde::{Deserialize, Serialize};

use crate::artifacts::ArtifactRef;
use crate::delegation::Workspace;

/// Units per plan.
pub const MAX_UNITS: usize = 8;
/// Units running at once.
pub const MAX_RUNNING: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnitState {
    /// Ready, not started (the plan is not confirmed, or a limit holds it).
    #[default]
    NotStarted,
    WaitingDeps,
    Running,
    /// It reported; its acceptance is not all passed yet.
    AwaitingAcceptance,
    Passed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkUnit {
    pub id: String,
    pub title: String,
    /// What the unit's delegation is asked to do.
    pub brief: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Read-only, writing in the parent's directory (one at a time), or in
    /// an isolated copy.
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Its acceptance criteria (each becomes one of the delegation's).
    #[serde(default)]
    pub acceptance: Vec<String>,
    /// The delegation that runs it, once started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_id: Option<String>,
    /// The prerequisites' outputs it was started with (to tell when they
    /// changed since).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub started_with: Vec<ArtifactRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkPlan {
    pub id: String,
    pub parent_task_id: String,
    pub title: String,
    pub units: Vec<WorkUnit>,
    /// The user confirmed it (an agent may only propose one). Nothing starts
    /// before.
    #[serde(default)]
    pub confirmed: bool,
    pub created_ms: u64,
}

/// One unit as it stands now, in one sentence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitStatus {
    pub id: String,
    pub state: UnitState,
    /// Why it is where it is ("等「调研」通过验收").
    pub reason: String,
    /// The prerequisites' outputs changed after it started: its input is
    /// out of date (the user decides whether to redo it).
    #[serde(default)]
    pub inputs_stale: bool,
}
