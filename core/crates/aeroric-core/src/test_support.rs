//! Helpers shared by this crate's unit tests.

use std::sync::Once;

/// Raise the open-file soft limit for the test process, once. macOS gives a
/// non-interactive shell 256 descriptors; the router stress tests open many
/// sockets and SQLite files at a time, and every test running beside them
/// then failed with EMFILE. The host does the same at startup
/// (`aeroric-host` `startup::raise_open_file_limit`).
#[cfg(unix)]
#[allow(clippy::unnecessary_cast, reason = "rlim_t is not u64 on every unix")]
pub(crate) fn raise_open_file_limit() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        const WANT: libc::rlim_t = 10240;
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit/setrlimit only read and write the struct we pass.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
            return;
        }
        let target = WANT.min(limit.rlim_max);
        if limit.rlim_cur < target {
            let raised = libc::rlimit {
                rlim_cur: target,
                rlim_max: limit.rlim_max,
            };
            // SAFETY: as above.
            unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) };
        }
    });
}

#[cfg(not(unix))]
pub(crate) fn raise_open_file_limit() {}
