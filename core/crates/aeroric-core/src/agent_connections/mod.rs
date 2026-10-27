//! Agent connections, model catalog, pools and agent configurations (B1, Q23–Q34).
//!
//! See `docs/backlog.md` §4.5 for the decisions this
//! encodes. Persistence is `agent-connections.json` in the data dir; credentials
//! are in the keychain, never in the file.

pub mod fetch;
pub mod launch;
pub mod legacy_cleanup;
pub mod migration;
pub mod model;
pub mod official;
pub mod routing;
pub mod service;
pub mod store;

#[cfg(test)]
mod tests;
