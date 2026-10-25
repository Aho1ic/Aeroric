//! The host's on-demand gateway (B1 step 4b, Q23/Q29).
//!
//! One `local_router` per host process, started the first time a task pinned to an
//! agent configuration launches. It listens on a free loopback port chosen at start
//! (never the legacy 15721 default, which cc-switch also uses), and only accepts
//! requests carrying a live task token (see `local_router::task_tokens`).
//!
//! The router runs on its own process-lifetime runtime, the same pattern as
//! `push_host::push_runtime`: the executor that needs it is synchronous and may be
//! called from inside another runtime, where `block_on` would panic.

use std::sync::OnceLock;

use aeroric_core::local_router::{LocalRouterState, RouterRuntimeConfig, RouterUpstreams};
use parking_lot::Mutex;

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("aeroric-gateway")
            .build()
            .unwrap_or_else(|error| panic!("cannot start the gateway runtime: {error}"))
    })
}

struct Gateway {
    state: LocalRouterState,
    /// `http://127.0.0.1:<port>` once the router is listening.
    base_url: Mutex<Option<String>>,
}

static GATEWAY: OnceLock<Gateway> = OnceLock::new();

fn gateway(data_dir: &std::path::Path) -> &'static Gateway {
    GATEWAY.get_or_init(|| Gateway {
        state: LocalRouterState::with_database_path(data_dir.join("gateway-usage.sqlite")),
        base_url: Mutex::new(None),
    })
}

fn free_loopback_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("cannot reserve a loopback port: {e}"))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|e| e.to_string())
}

/// Start the gateway if it is not running; return its base URL.
pub fn ensure_running(data_dir: &std::path::Path) -> Result<String, String> {
    let gateway = gateway(data_dir);
    let mut base_url = gateway.base_url.lock();
    if let Some(url) = base_url.as_ref() {
        let port = url.rsplit(':').next().and_then(|p| p.parse::<u16>().ok());
        if port.is_some_and(aeroric_core::local_router::is_listening_on) {
            return Ok(url.clone());
        }
    }
    let port = free_loopback_port()?;
    let config = RouterRuntimeConfig::new("127.0.0.1", port, false, RouterUpstreams::default())
        .with_config_data_dir(data_dir);
    // A short-lived thread so `block_on` never runs inside the caller's runtime.
    let info = std::thread::scope(|scope| {
        scope
            .spawn(|| runtime().block_on(gateway.state.start(config)))
            .join()
            .map_err(|_| "gateway start panicked".to_string())
    })?
    .map_err(|e| format!("cannot start the gateway: {}", e.message))?;
    *base_url = Some(info.base_url.clone());
    Ok(info.base_url)
}
