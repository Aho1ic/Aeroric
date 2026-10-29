//! Projects as core commands (Task P2-03).
//!
//! Two layers, and the split is the point: [`mutations`] holds pure functions that must
//! produce the same list the shipping client's TypeScript produces from the same input
//! (pinned by `contracts/native/v1/fixtures/projects.json`, which both sides are tested
//! against), and [`service`] is the only thing that touches disk — one data directory, one
//! writer, one revision per real change.
//!
//! Nothing in here knows about windows, emitters, or React listeners. The rail shape a
//! client draws is derived from [`service::Rail`], never the other way round.

pub mod mutations;
pub mod service;
pub mod tags;

/// Re-exported so a caller compares locations with the same key the wire uses.
pub use aeroric_protocol::ProjectTarget;
pub use mutations::{
    apply_order, assign_group, clear_group, derive_project_name, group_of, group_projects_for_rail,
    merge_group_names, normalize_group_name, normalize_group_names, normalize_order,
    normalize_remote_path, normalize_wsl_path, remove, rename, rename_group, resolve_location,
    set_avatar, set_pinned, sort_for_rail, ssh_path, toggle_hidden, toggle_pinned, touch,
    upsert_local, upsert_ssh, upsert_wsl, visible_in_groups, wsl_path, GroupView, Upserted,
    UNGROUPED,
};
pub use service::{Applied, GroupConfig, ProjectError, ProjectService, Rail, RailGroup};
