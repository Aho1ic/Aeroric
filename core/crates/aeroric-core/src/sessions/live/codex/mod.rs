//! Codex through `codex app-server` (stdio JSON-RPC), ADR-043.

pub mod driver;
pub mod map;
pub mod rpc;
pub mod server;

#[cfg(test)]
pub(crate) mod fake;
