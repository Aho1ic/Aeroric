//! Local protocol v1 DTOs. Filled in by P2-01; the skeleton only proves the
//! crate graph and dependency direction.

/// Crate version string reported during the host handshake.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
