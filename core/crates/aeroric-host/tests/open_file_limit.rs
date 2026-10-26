//! R1: the host raises its soft open-file limit at start (cc-switch 04d0ebf5).

#[cfg(unix)]
#[test]
#[allow(clippy::unnecessary_cast, reason = "rlim_t is not u64 on every unix")]
fn the_soft_open_file_limit_is_raised_towards_10240() {
    let soft = aeroric_host::startup::raise_open_file_limit().expect("getrlimit works");
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit only writes the struct we pass.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    assert_eq!(soft, limit.rlim_cur as u64);
    assert!(limit.rlim_cur >= 10240u64.min(limit.rlim_max as u64) as libc::rlim_t);
}
