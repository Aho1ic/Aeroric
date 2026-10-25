//! A14: the host holds a power assertion while a task is pending or running,
//! releases it when none is, and never holds it when the setting is off.
//! A fake assertion stands in for `caffeinate`, so the test never touches the
//! machine's power state.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::TaskService;
use aeroric_core::terminal::pty::TerminalService;
use aeroric_host::executor::{CliPtyExecutor, RegistryLiveRuns};
use aeroric_host::keep_awake::{HeldAssertion, KeepAwake, PowerAssertion};
use serde_json::json;

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

/// Counts live holds: +1 on hold, −1 when the held value is dropped.
#[derive(Default)]
struct FakePower {
    held: Arc<AtomicUsize>,
}

struct FakeHeld(Arc<AtomicUsize>);

impl HeldAssertion for FakeHeld {
    fn alive(&mut self) -> bool {
        true
    }
}

impl Drop for FakeHeld {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl PowerAssertion for FakePower {
    fn hold(&self) -> Result<Box<dyn HeldAssertion>, String> {
        self.held.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeHeld(Arc::clone(&self.held))))
    }
}

fn write_tasks(paths: &AppPaths, status: &str) {
    let dir = paths.data_dir().join("projects").join("p1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tasks.json"),
        serde_json::to_vec_pretty(&json!([
            { "id": "t1", "projectId": "p1", "prompt": "x", "agent": "claude",
              "permissionMode": "ask", "status": status },
            { "id": "t2", "projectId": "p1", "prompt": "y", "agent": "codex",
              "permissionMode": "ask", "status": "done" }
        ]))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn the_assertion_follows_running_tasks_and_the_setting() {
    let root = PathBuf::from("/tmp").join(format!("ah-keepawake-{}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.aeroric")).unwrap();
    let paths = AppPaths::from_user_home(root.join("home"));
    std::fs::write(
        paths.data_dir().join("projects.json"),
        serde_json::to_vec(&json!([{ "id": "p1", "name": "p1", "path": "/tmp/p1" }])).unwrap(),
    )
    .unwrap();
    write_tasks(&paths, "running");
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "keep-awake-test".into(),
                version: "0-test".into(),
            },
            &LocalProbe,
        )
        .unwrap(),
    );
    let registry = Arc::new(TaskRegistry::default());
    let log = Arc::new(EventLog::default());
    let executor = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::clone(&registry),
        Arc::new(TerminalService::new()),
        Arc::clone(&log),
    );
    let families = Arc::new(aeroric_host::families::SettingsFamiliesCli::new(
        Arc::clone(&context),
    ));
    let service = Arc::new(
        TaskService::new(
            Arc::clone(&context),
            families as Arc<_>,
            executor as Arc<_>,
            Arc::new(RegistryLiveRuns::new(registry)),
        )
        .unwrap(),
    );
    let power = Arc::new(FakePower::default());
    let held = Arc::clone(&power.held);
    let keep = KeepAwake::start(Arc::clone(&context), service, log, power);

    keep.tick(true);
    assert_eq!(held.load(Ordering::SeqCst), 1, "a running task holds it");
    let state = keep.state();
    assert!(state.enabled && state.active);
    assert_eq!(state.active_tasks, 1);
    keep.tick(true);
    assert_eq!(held.load(Ordering::SeqCst), 1, "held once, not per tick");

    write_tasks(&paths, "input_required");
    keep.tick(true);
    assert_eq!(
        held.load(Ordering::SeqCst),
        0,
        "waiting for the user releases it"
    );
    assert!(!keep.state().active);

    write_tasks(&paths, "pending");
    keep.tick(true);
    assert_eq!(held.load(Ordering::SeqCst), 1, "a pending task holds it");

    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec(&json!({ "keep_awake_while_running": false })).unwrap(),
    )
    .unwrap();
    keep.tick(true);
    assert_eq!(held.load(Ordering::SeqCst), 0, "turning it off releases it");
    let state = keep.state();
    assert!(!state.enabled && !state.active);
    assert_eq!(state.active_tasks, 1);

    drop(keep);
    std::fs::remove_dir_all(&root).ok();
}
