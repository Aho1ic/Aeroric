//! R1-3a (2026-11-03): what each agent family can do where it runs, and
//! which logins were refused (frontier-roadmap Q-S11).
//!
//! The table is exhaustive per family, place and feature; a picker greys
//! out what is not supported and says why, offers what is degraded with a
//! note, and marks what nobody has checked yet as such.

use serde::{Deserialize, Serialize};

use crate::task::ProtocolFamily;

pub const CAPABILITIES_METHOD: &str = "session.capabilities";
/// Logins the agent refused, newest first; with `taskId`, only that task's.
pub const LOGIN_STATE_METHOD: &str = "login.state";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Place {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    PermAsk,
    PermAutoEdit,
    PermFullAccess,
    /// Changing the permission while the session runs.
    PermChange,
    PlanMode,
    Effort,
    Compact,
    Review,
    Goal,
    /// A message during a turn reaching that turn.
    Steer,
    /// Aeroric can start a session of this family as a delegated child
    /// (`delegate`, `#` delegation) and route its result back to the
    /// parent. Not "can delegate itself": that needs `ContextTools`.
    DelegateChild,
    /// Aeroric can start a session of this family as a reviewer.
    Reviewer,
    /// The agent acknowledges what was delivered (R1-2).
    Receipts,
    /// A read-only session (`plan`: discussion members, read-only
    /// delegations) is held read-only by the harness itself, not by the
    /// user refusing each write (A4, 2026-11-17).
    StrictReadOnly,
    /// C9 (2026-11-20): the session is given Aeroric's context tools
    /// (memory, Wiki, board, delegation…) at start. H1: the tools surface.
    ContextTools,
    /// H1 (2026-11-20): the surfaces through which a harness uses Aeroric
    /// (`Feature::SURFACES`). A system layer for the opening (memory
    /// profile, handoff brief).
    SurfaceInstructions,
    /// Built-in and learned skills Aeroric mounts at start.
    SurfaceSkills,
    /// The harness's own hooks or events, used to add context at set
    /// points without the model having to remember.
    SurfaceEvents,
    /// The harness's own subagents, seen in the session.
    SurfaceSubagents,
    /// The `aeroric-cli` command line, for a harness with neither MCP nor
    /// a system layer.
    SurfaceCli,
}

impl Feature {
    pub const ALL: &'static [Feature] = &[
        Feature::PermAsk,
        Feature::PermAutoEdit,
        Feature::PermFullAccess,
        Feature::PermChange,
        Feature::PlanMode,
        Feature::Effort,
        Feature::Compact,
        Feature::Review,
        Feature::Goal,
        Feature::Steer,
        Feature::DelegateChild,
        Feature::Reviewer,
        Feature::Receipts,
        Feature::StrictReadOnly,
        Feature::ContextTools,
        Feature::SurfaceInstructions,
        Feature::SurfaceSkills,
        Feature::SurfaceEvents,
        Feature::SurfaceSubagents,
        Feature::SurfaceCli,
    ];

    /// H1: the surfaces, in the order the session info lists them.
    pub const SURFACES: &'static [Feature] = &[
        Feature::SurfaceInstructions,
        Feature::ContextTools,
        Feature::SurfaceSkills,
        Feature::SurfaceEvents,
        Feature::SurfaceSubagents,
        Feature::SurfaceCli,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SupportState {
    Yes,
    No,
    /// Offered, but it works differently; `note` says how.
    Degraded,
    /// Not checked yet: offered, marked as unconfirmed.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEntry {
    pub feature: Feature,
    pub state: SupportState,
    /// Why not, how it differs, or who enforces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilitiesParams {
    pub family: ProtocolFamily,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
    /// C8 (2026-11-20): with a task, the reply also carries the tools its
    /// latest launch had (`tools`, absent when none was kept); H1: and its
    /// surfaces (`surfaces`, a [`SurfaceLaunch`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// H1 (2026-11-20): what one launch (start or resume) of a session
/// really had on each surface. A `Yes` rests on what happened at that
/// launch (the driver took the opening, the context server, the skills
/// mounted) or, for a surface the launch does not exercise, on the
/// family's table, which tests tie to the drivers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceLaunch {
    pub at_ms: u64,
    pub harness: String,
    pub place: Place,
    /// One entry per `Feature::SURFACES`, in that order.
    pub surfaces: Vec<CapabilityEntry>,
}

/// A login the agent refused (401, signed out…), until a turn under it
/// completes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiredLogin {
    /// Family, place and source: `claude|local|own`.
    pub id: String,
    pub family: ProtocolFamily,
    /// "Claude（本机）".
    pub label: String,
    pub reason: String,
    pub at_ms: u64,
    /// For the agent's own login: what to run to sign in again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// For an Agent configuration: the one to check in Settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_id: Option<String>,
    /// The SSH connection, for a remote login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
}
