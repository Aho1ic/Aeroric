//! Host library. `aeroric-cli serve` is the process entry point; there is
//! deliberately no second daemon. Startup order is lock -> data compat ->
//! runtime -> listener -> ready (P3-06).

pub fn is_library_only() -> bool {
    true
}
