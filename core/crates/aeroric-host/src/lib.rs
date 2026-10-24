//! Host library. `aeroric-cli serve` is the process entry point; there is
//! deliberately no second daemon. Startup order is lock -> data compat ->
//! runtime -> listener -> ready (P3-06).

pub mod adapter_services;
pub mod agent_connections_services;
pub mod auth;
pub mod canvas_services;
pub mod codec;
pub mod connection_services;
pub mod database_services;
pub mod diagnostics;
pub mod dispatcher;
pub mod executor;
pub mod families;
pub mod file_access;
pub mod file_services2;
pub mod gateway;
pub mod ide_services;
pub mod imports_services;
pub mod infrastructure_services;
pub(crate) mod local_targets;
pub mod lsp_services;
pub mod maintenance_services;
pub mod notebook_services;
pub mod platform_adapter;
pub mod push_host;
pub mod remote_headless;
pub mod search_services;
pub mod services;
pub mod settings_services;
pub mod startup;
pub mod submission_services;
pub mod subscriptions;
pub mod uds;

pub fn is_library_only() -> bool {
    true
}
