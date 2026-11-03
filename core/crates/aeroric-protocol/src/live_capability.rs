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
    DelegateChild,
    Reviewer,
    /// The agent acknowledges what was delivered (R1-2).
    Receipts,
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
