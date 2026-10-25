//! A13: the exported diagnostics report carries the host log tail and facts,
//! with the host token, task tokens, keys and the home path masked.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::maintenance::StartupDegradationLog;
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_host::diagnostic_report_services::{build_report, ReportInputs};

struct LocalProbe;

impl StorageLocationProbe for LocalProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        StorageLocation {
            directory: data_dir.to_path_buf(),
            volume: VolumeKind::Local,
            private_permissions_enforced: true,
        }
    }
}

#[test]
fn the_report_carries_the_log_tail_with_secrets_and_home_masked() {
    let root = PathBuf::from("/tmp").join(format!("ah-diag-{}", std::process::id()));
    let home = root.join("home");
    std::fs::create_dir_all(home.join(".aeroric/logs")).unwrap();
    let paths = AppPaths::from_user_home(&home);
    std::fs::write(
        paths.data_dir().join("host.token"),
        "host-secret-token-1234\n",
    )
    .unwrap();
    let log = format!(
        "old line\n[aeroric-host] gateway token aeroric-task-{} for {}/projects/app\n\
         upstream said: invalid key sk-proj-ABCDEFGH12345678\n\
         echoed host-secret-token-1234\nlast line\n",
        "f".repeat(64),
        home.display()
    );
    std::fs::write(paths.data_dir().join("logs/host.log"), log).unwrap();
    let context = Arc::new(
        CoreContext::acquire(
            paths,
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "diag-test".into(),
                version: "0-test".into(),
            },
            &LocalProbe,
        )
        .unwrap(),
    );
    let degradations = Arc::new(StartupDegradationLog::new());
    degradations.record("local-router", "port busy", ":memory:");
    let (name, text) = build_report(&ReportInputs {
        context,
        service: None,
        degradations,
        capabilities: vec![aeroric_protocol::capability::Capability {
            name: "session.live".into(),
            enabled: true,
            since: Some(1),
            note: None,
        }],
        home: Some(home.clone()),
    });

    assert!(
        name.starts_with("aeroric-diagnostics-") && name.ends_with(".md"),
        "{name}"
    );
    for expected in [
        "## 概况",
        "session.live = on",
        "local-router: port busy",
        "last line",
        "~/projects/app",
    ] {
        assert!(text.contains(expected), "missing {expected}:\n{text}");
    }
    for leaked in [
        "host-secret-token-1234",
        "ffffffffffffffff",
        "ABCDEFGH12345678",
        home.to_string_lossy().as_ref(),
    ] {
        assert!(!text.contains(leaked), "{leaked} leaked:\n{text}");
    }
    std::fs::remove_dir_all(&root).ok();
}
