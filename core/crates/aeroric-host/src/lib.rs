//! Host library. `aeroric-cli serve` is the process entry point; there is
//! deliberately no second daemon. Startup order is lock -> data compat ->
//! runtime -> listener -> ready (P3-06).

pub mod auth;
pub mod codec;
pub mod diagnostics;
pub mod dispatcher;
pub mod executor;
pub mod families;
pub mod notebook_services;
pub mod remote_headless;
pub mod services;
pub mod startup;
pub mod subscriptions;
pub mod uds;

pub fn is_library_only() -> bool {
    true
}
