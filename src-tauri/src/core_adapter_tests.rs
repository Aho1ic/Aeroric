//! P2-05's acceptance evidence (plan §Phase 2 · Task P2-05, criterion 2).
//!
//! Everything here is the real thing: the real `LegacyTaskExecutor`, the real launch kernels, a
//! real pseudo-terminal and a real child process. The only fake is the agent, which is a shell
//! script that records each time it runs. That distinction is the point — P2-04 refused to close
//! this criterion with a recording mock executor, because a mock can only prove that a mock was
//! called.
//!
//! Three things being real makes possible, and one it makes necessary:
//!
//! - The host's storage root is redirected for the duration of each scenario
//!   ([`crate::platform::install_scratch_home`]), so no test ever writes the developer's
//!   `~/.aeroric`.
//! - `TaskService` is constructed for real, so core mints the run's generation and the host's
//!   maps are keyed by that same number end to end. That is ADR-020's single-generation rule
//!   proven across the port rather than on one side of it.
//! - The events are counted as the client receives them, so "no double events" is measured.
//! - The launch chain had to become generic over `Runtime` for any of this to compile, since
//!   `tauri::test` only produces `App<MockRuntime>`. That is route B of the §4b fork.
//!
//! The fake agent blocks on a gate file the scenario opens on demand. Without that the child
//! would exit before the test could look at the host's maps, and every mid-run assertion would
//! be a race that happens to pass.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aeroric_core::clock::FixedClock;
use aeroric_core::context::CoreContext;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage::Record;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::{
    AgentFamilies, Applied, NewTask, NoLiveRuns, ProtocolFamily, TaskError, TaskExecutor,
    TaskService,
};
use serde_json::{json, Map, Value};
use tauri::ipc::Channel;
use tauri::{Listener, Manager};

use crate::app_settings::AppSettings;
use crate::core_adapter::SettingsFamilies;
use crate::legacy_task_executor::{LegacyTaskExecutor, OutputSinkMailbox};
use crate::platform::ScratchHome;
use crate::pty::OutputSink;
use crate::run_registry::RunKey;
use crate::TaskManager;

/// The agent every scenario launches. Deliberately not a built-in id: a built-in would resolve to
/// whatever binary happens to be installed on the machine running the tests.
const FAKE_AGENT: &str = "aeroric-fake-agent";

/// The scratch storage root is process-global, so one scenario holds it at a time.
static SCENARIO: Mutex<()> = Mutex::new(());

/// A data directory the core will accept a writer lease on. Production stats the volume; here the
/// answer is fixed and the *lease* is real.
struct LocalVolume;

impl StorageLocationProbe for LocalVolume {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        StorageLocation::new(data_dir, VolumeKind::Local)
    }
}

fn task_manager() -> TaskManager {
    TaskManager {
        pty_masters: parking_lot::Mutex::new(HashMap::new()),
        pty_writers: parking_lot::Mutex::new(HashMap::new()),
        child_handles: parking_lot::Mutex::new(HashMap::new()),
        pending_pty_sizes: parking_lot::Mutex::new(HashMap::new()),
        cancelled_tasks: parking_lot::Mutex::new(HashSet::new()),
        manually_completed_tasks: parking_lot::Mutex::new(HashSet::new()),
        codex_sessions: parking_lot::Mutex::new(HashMap::new()),
        claude_sessions: parking_lot::Mutex::new(HashMap::new()),
        dsh_sessions: parking_lot::Mutex::new(HashMap::new()),
        omp_sessions: parking_lot::Mutex::new(HashMap::new()),
        claimed_session_paths: parking_lot::Mutex::new(HashSet::new()),
        initial_input_signals: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        wsl_active_ids: parking_lot::Mutex::new(HashSet::new()),
        runs: crate::run_registry::RunRegistry::new(),
        codex_rpc: Arc::new(parking_lot::Mutex::new(None)),
    }
}

/// A throwaway `$HOME`, in the same shape every existing test in this crate uses.
struct ScratchRoot {
    path: PathBuf,
}

impl ScratchRoot {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "aeroric-adapter-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).expect("scratch root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// One scenario: a throwaway `$HOME` the host derives `~/.aeroric` from, a core writer over that
/// same directory, an app whose state is a real `TaskManager`, and an agent whose program is a
/// script that reports every execution.
///
/// Field order is load-bearing twice over. Rust drops in declaration order, so the lease, the
/// managed state and every channel go first. Then the scratch-home slot is cleared *before* the
/// directory is deleted: leaving the slot pointing at a removed directory would make the next
/// reader's `settings.json` "absent" and hand it the same default-writes-back that the
/// constructor above works around.
struct Scenario {
    app: tauri::AppHandle<tauri::test::MockRuntime>,
    /// Held so managed state is not torn down while the scenario runs.
    _app: tauri::App<tauri::test::MockRuntime>,
    sinks: Arc<OutputSinkMailbox>,
    service: TaskService,
    project: Record,
    spawns: PathBuf,
    gate: PathBuf,
    events: Arc<Mutex<Vec<(String, Value)>>>,
    task_id: String,
    _scratch: ScratchHome,
    _home: ScratchRoot,
}

impl Scenario {
    fn new(tag: &str) -> Self {
        let home = ScratchRoot::new(tag);
        let data_dir = home.path().join(".aeroric");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        let project_path = home.path().join("project");
        std::fs::create_dir_all(&project_path).expect("project dir");

        let spawns = home.path().join("agent-spawns.log");
        let gate = home.path().join("agent-gate");
        let script = home.path().join("fake-agent.sh");
        write_fake_agent(&script, &spawns, &gate);
        // The whole home is built *before* it is installed, and that order is not cosmetic.
        // `load_settings_unlocked` creates and caches a default settings.json whenever the file
        // is missing, and every other test in this binary reads settings through the same global
        // `home_dir()`. Installing first would let one of those readers materialise a default
        // over this scenario's fixture — measured, not theorised: it showed up as the fake agent
        // resolving to the Codex family.
        write_settings_with_fake_agent(&data_dir, &script);
        let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
        assert_fake_agent_is_claude();

        let app = tauri::test::mock_app();
        app.manage(task_manager());
        // `finalize_task_exit` reaches this through `event_watcher::cleanup_task_events`, so a
        // scenario that runs a real exit has to carry it; `lib.rs` manages the same value.
        app.manage(crate::remote::RemoteState::new());
        let handle = app.handle().clone();

        let context = CoreContext::acquire(
            AppPaths::with_data_dir(home.path(), Some(&data_dir)),
            Arc::new(FixedClock::at(1_700_000_000_000)),
            &LeaseOwner {
                application: "aeroric-adapter-acceptance".into(),
                version: "0".into(),
            },
            &LocalVolume,
        )
        .expect("writer lease");

        let sinks = Arc::new(OutputSinkMailbox::new());
        let executor: Arc<dyn TaskExecutor> = Arc::new(LegacyTaskExecutor::<
            tauri::test::MockRuntime,
        >::new(
            handle.clone(), sinks.clone()
        ));
        // ADR-036: every run in this file is core-minted, so core's own claims answer the
        // liveness question. The port stays `NoLiveRuns` deliberately — a stub that said "all
        // live" would hide any regression in the claim-based half of the guard.
        let service = TaskService::new(
            Arc::new(context),
            Arc::new(SettingsFamilies) as Arc<dyn AgentFamilies>,
            executor,
            Arc::new(NoLiveRuns),
        )
        .expect("service");

        let events = Arc::new(Mutex::new(Vec::new()));
        listen(&handle, &events);

        Scenario {
            app: handle,
            _app: app,
            sinks,
            service,
            project: Record(project_record(&project_path, tag)),
            spawns,
            gate,
            events,
            task_id: format!("task-{tag}"),
            _scratch: scratch,
            _home: home,
        }
    }

    fn create_task(&self) -> Applied {
        self.service
            .create(
                &self.project,
                NewTask {
                    id: self.task_id.clone(),
                    project_id: "p1".into(),
                    prompt: "say something".into(),
                    agent: FAKE_AGENT.into(),
                    permission_mode: "default".into(),
                    immediate: true,
                    selected_model: None,
                    reasoning_effort: None,
                    speed: None,
                    dsh_agent_preset: None,
                    images: vec![],
                    texts: vec![],
                },
            )
            .expect("create")
            .1
    }

    /// `persist_before_launch` is `true` because that is what the shipping submit path does: the
    /// row is on disk before the process exists.
    fn start(&self, operation_key: &str) -> Result<Applied, TaskError> {
        self.sinks
            .park(operation_key, OutputSink::Channel(Channel::new(|_| Ok(()))));
        self.service.start(
            &self.project,
            &self.task_id,
            operation_key,
            true,
            None,
            false,
            Default::default(),
        )
    }

    fn spawn_count(&self) -> usize {
        std::fs::read_to_string(&self.spawns)
            .map(|text| text.lines().count())
            .unwrap_or_default()
    }

    /// Let the held-open child finish.
    fn release_agent(&self) {
        std::fs::write(&self.gate, b"release\n").expect("open gate");
    }

    fn statuses(&self) -> Vec<String> {
        self.emitted("task-status")
            .iter()
            .filter_map(|payload| {
                payload
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    fn emitted(&self, name: &str) -> Vec<Value> {
        self.events
            .lock()
            .expect("event sink")
            .iter()
            .filter(|(event, _)| event == name)
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    /// Wait for the child to settle. A terminal status is the only event that proves the real
    /// process reached its end, which is what makes "exactly one spawn" a claim about a finished
    /// run rather than a snapshot taken too early.
    fn wait_until(&self, what: &str, predicate: impl Fn(&Self) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !predicate(self) {
            if std::time::Instant::now() > deadline {
                panic!(
                    "timed out waiting for {what}; spawns={}, statuses={:?}",
                    self.spawn_count(),
                    self.statuses()
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn wait_for_spawn(&self, expected: usize) {
        let what = format!("{expected} agent spawn(s)");
        self.wait_until(&what, |scenario| scenario.spawn_count() >= expected);
    }

    /// `launch` returns as soon as the kernel is on the async runtime, so every host-side fact
    /// (the three PTY maps, the registry, the `running` event) arrives after this point. Waiting
    /// for the settle event is enough: `finalize_task_exit` takes the handles down *before* it
    /// emits, so a terminal status means the teardown already happened.
    fn wait_for_launch(&self) {
        self.wait_until("the launch to register and report running", |scenario| {
            !scenario.live_runs().is_empty()
                && scenario.statuses().iter().any(|status| status == "running")
        });
        self.wait_for_spawn(1);
    }

    fn wait_for_terminal_status(&self) {
        self.wait_until("a terminal task-status", |scenario| {
            scenario.statuses().iter().any(|status| is_terminal(status))
        });
    }

    /// Weaker on the event side and stronger on the state side than `wait_for_terminal_status`:
    /// `cancel_task_core` emits `cancelled` *before* the exit monitor tears the handles down, so
    /// waiting for the event alone would read a still-registered run as a leak.
    fn wait_for_retirement(&self) {
        self.wait_until("the run to retire", |scenario| {
            scenario.live_runs().is_empty() && scenario.emitted("task-status").len() >= 2
        });
    }

    fn live_runs(&self) -> Vec<RunKey> {
        self.app
            .state::<TaskManager>()
            .runs
            .live_generations(&self.task_id)
            .into_iter()
            .map(|generation| RunKey::minted(&self.task_id, generation))
            .collect()
    }

    /// HashMap iteration order is not a fact about the code under test, so every key list this
    /// file compares is sorted before it is compared.
    fn writer_keys(&self) -> Vec<RunKey> {
        let mut keys: Vec<RunKey> = self
            .app
            .state::<TaskManager>()
            .pty_writers
            .lock()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn child_keys(&self) -> Vec<RunKey> {
        let mut keys: Vec<RunKey> = self
            .app
            .state::<TaskManager>()
            .child_handles
            .lock()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn has_live_run(&self) -> bool {
        self.app.state::<TaskManager>().has_live_run(&self.task_id)
    }
}

/// The digest of a project's task table — the only honest answer to "was this file written".
fn table_sha(service: &TaskService, project_id: &str) -> String {
    service
        .load(project_id)
        .expect("load")
        .fingerprint
        .expect("a written table has a fingerprint")
        .sha256
}

/// `Applied` carries the cover as a value, so the two sides of a comparison have to meet on the
/// digest rather than on a path label.
fn applied_sha(applied: &Applied) -> String {
    applied
        .fingerprint
        .as_ref()
        .and_then(|value| value.get("sha256"))
        .and_then(Value::as_str)
        .expect("a committed write reports its cover")
        .to_string()
}

fn is_terminal(status: &str) -> bool {
    matches!(status, "done" | "failed" | "cancelled")
}

/// The agent reports its own execution and then waits for the gate, so the spawn count is a fact
/// about the child rather than about what the parent believes it started, and the test gets a
/// process it can inspect while it is still alive.
///
/// It counts only a process that owns a terminal. The launch path also probes the configured
/// binary (`--version`, through `hooks::usable_for` and `resolve_native_initial_prompt`), and
/// those probes capture stdout through a pipe. Two reasons that distinction matters, both
/// measured rather than assumed here: an undiscriminating script scores a probe as a launch, and
/// a script that then waits for the gate **deadlocks the launch**, because the version probe is
/// awaited synchronously on the way to `register_pty_handles`.
fn write_fake_agent(script: &Path, marker: &Path, gate: &Path) {
    let body = format!(
        "#!/bin/sh\nif [ ! -t 1 ]; then exit 0; fi\n         printf 'spawn\\n' >> '{}'\nwhile [ ! -f '{}' ]; do sleep 0.05; done\nexit 0\n",
        marker.to_string_lossy(),
        gate.to_string_lossy()
    );
    std::fs::write(script, body).expect("write fake agent");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

/// The rest of the file is `AppSettings`'s own serialization, so a renamed settings field cannot
/// quietly turn this fixture into a different product. The one profile row is spelled out because
/// the test must know exactly what it declared — and [`assert_fake_agent_is_claude`] then checks
/// the host reads it that way, instead of letting the test agree with itself.
fn write_settings_with_fake_agent(data_dir: &Path, script: &Path) {
    let mut settings = serde_json::to_value(AppSettings::default()).expect("settings shape");
    settings["custom_agents"] = json!([{
        "id": FAKE_AGENT,
        "label": "Fake",
        "path": script.to_string_lossy(),
        "codex_like": false,
        "family": "",
        "config_lang": "shellscript",
        "base_url": "",
        "api_key": "",
        "models": [],
        "enable_1m_context": false,
        "disable_artifact_tool": false,
        "enable_chat_completions_proxy": false,
        "bridge_python_path": "",
        "username": "",
        "password": "",
    }]);
    std::fs::write(
        data_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).expect("settings"),
    )
    .expect("write settings");
}

/// A custom agent with no explicit family and `codex_like: false` launches as Claude family,
/// which is the branch this scenario means to exercise. If the host reads the row differently,
/// say so here rather than asserting against a process that took another path.
fn assert_fake_agent_is_claude() {
    assert_eq!(
        SettingsFamilies.launch_family(FAKE_AGENT),
        ProtocolFamily::Claude,
        "the fixture agent must launch through the claude branch"
    );
    assert_eq!(
        SettingsFamilies.session_family(FAKE_AGENT),
        ProtocolFamily::Claude,
        "…and write a claude-format transcript"
    );
}

fn project_record(path: &Path, tag: &str) -> Map<String, Value> {
    let mut record = Map::new();
    record.insert("id".into(), json!("p1"));
    record.insert("name".into(), json!(format!("project-{tag}")));
    record.insert("path".into(), json!(path.to_string_lossy()));
    record
}

fn listen(
    handle: &tauri::AppHandle<tauri::test::MockRuntime>,
    sink: &Arc<Mutex<Vec<(String, Value)>>>,
) {
    for name in ["task-status", "task-session"] {
        let sink = sink.clone();
        let event_name = name.to_string();
        handle.listen(name, move |event| {
            let payload = serde_json::from_str::<Value>(event.payload()).unwrap_or(Value::Null);
            sink.lock()
                .expect("event sink")
                .push((event_name.clone(), payload));
        });
    }
}

/// Criterion 2, first clause: one start produces one process, one run identity on both sides of
/// the port, and one write sequence.
#[test]
fn one_start_runs_one_process_under_the_generation_core_minted() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("one-spawn");
    let created = scenario.create_task();
    assert!(
        created.revision.is_some(),
        "creating a row bumps the revision"
    );
    let sha_after_create = table_sha(&scenario.service, "p1");

    let applied = scenario
        .start("op-1")
        .expect("the real executor accepts a launch it has a channel for");
    scenario.wait_for_launch();

    // Core minted the number and the executor carried it into the host's maps: the run is
    // addressable as (task, 1), and not under the bare task id the pre-ADR-020 code used.
    assert_eq!(scenario.service.generation(&scenario.task_id), 1);
    assert_eq!(
        scenario.live_runs(),
        vec![RunKey::minted(&scenario.task_id, 1)],
        "the registry holds exactly the generation core minted"
    );
    assert_eq!(
        scenario.writer_keys(),
        vec![RunKey::minted(&scenario.task_id, 1)],
        "the PTY writer is keyed by the run, not by the task id"
    );
    assert!(
        !scenario
            .writer_keys()
            .contains(&RunKey::unminted(&scenario.task_id)),
        "a launch that reached core must never be addressable as unminted"
    );
    assert!(scenario.has_live_run());
    assert_eq!(
        scenario
            .statuses()
            .iter()
            .filter(|s| *s == "running")
            .count(),
        1,
        "one start must not emit `running` twice"
    );

    scenario.release_agent();
    scenario.wait_for_retirement();

    assert_eq!(scenario.spawn_count(), 1, "one start, one process");
    assert_eq!(
        scenario.writer_keys(),
        Vec::<RunKey>::new(),
        "the exit must retire the run's handles"
    );
    assert_eq!(scenario.live_runs(), Vec::<RunKey>::new());
    assert!(
        !scenario.has_live_run(),
        "no run of this task is live any more"
    );
    // ADR-023's whole point is that one event has one emitter. Until the sink is wired, the
    // legacy sites are the only ones, so the exact sequence this run owes is known: the fake agent
    // exits 0 without ever producing a transcript, which the Claude branch settles as `done`.
    // A second emitter for either event lands here as a longer list, not as a passing test.
    assert_eq!(
        scenario.statuses(),
        vec!["running".to_string(), "done".to_string()],
        "one launch, exactly one start event and one settle event"
    );
    assert!(
        scenario
            .emitted("task-session")
            .iter()
            .all(
                |payload| payload.get("task_id").and_then(Value::as_str) == Some(&scenario.task_id)
            ),
        "a session event must name the task it belongs to"
    );
    // "One write sequence", made precise: creating the row wrote the table once, starting it
    // wrote it again, and the run itself added nothing. A third digest would mean the host is
    // still saving task state behind core's back, which is what ADR-022 hands over.
    let reloaded = scenario.service.load("p1").expect("reload");
    assert_eq!(reloaded.records.len(), 1);
    let sha_after_launch = reloaded.fingerprint.expect("table").sha256;
    assert_ne!(
        sha_after_launch, sha_after_create,
        "the start must have written the table"
    );
    assert_eq!(
        sha_after_launch,
        applied_sha(&applied),
        "nothing may write the task table again after the launch"
    );
    assert_ne!(
        applied.revision, created.revision,
        "and the revision it bumped is a different one"
    );
}

/// ADR-017 + ADR-025 over the real executor: a second start of the same task with a *different*
/// operation key is a second run, and the two do not overwrite each other.
#[test]
fn a_second_operation_starts_a_second_run_without_replacing_the_first() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("two-runs");
    scenario.create_task();

    scenario.start("op-a").expect("first launch");
    scenario.wait_for_launch();
    scenario
        .start("op-b")
        .expect("a second operation is a second launch");
    scenario.wait_for_spawn(2);

    assert_eq!(
        scenario.service.generation(&scenario.task_id),
        2,
        "core minted a second generation"
    );
    let first = RunKey::minted(&scenario.task_id, 1);
    let second = RunKey::minted(&scenario.task_id, 2);
    assert_eq!(scenario.writer_keys(), vec![first.clone(), second.clone()]);
    assert_eq!(
        scenario
            .app
            .state::<TaskManager>()
            .resource_run(&scenario.task_id),
        Some(second.clone()),
        "ADR-025: a resource call addresses the newest registered run"
    );
    assert_eq!(
        scenario
            .app
            .state::<TaskManager>()
            .runs
            .intent_keys(&scenario.task_id),
        vec![first, second],
        "ADR-025: an intent reaches every live run"
    );

    scenario.release_agent();
    scenario.wait_for_terminal_status();
    assert_eq!(scenario.spawn_count(), 2);
}

/// ADR-017's mechanism, checked where it is claimed: the same operation key arriving twice is
/// one attempt. Without this the "spawn once" rule has no test behind it.
#[test]
fn replaying_the_operation_key_spawns_nothing_and_saves_nothing() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("replay");
    scenario.create_task();

    scenario.start("op-1").expect("first attempt");
    scenario.wait_for_launch();
    let sha_after_first_attempt = table_sha(&scenario.service, "p1");

    let error = scenario
        .start("op-1")
        .expect_err("the same key is the same attempt, whoever sent it");
    assert!(
        matches!(error, TaskError::AlreadyClaimed { .. }),
        "{error:?}"
    );
    assert_eq!(
        scenario.spawn_count(),
        1,
        "a replay must not fund a second process"
    );
    assert_eq!(
        scenario.service.generation(&scenario.task_id),
        1,
        "and it must not reserve a generation either"
    );
    assert_eq!(
        table_sha(&scenario.service, "p1"),
        sha_after_first_attempt,
        "…nor write the table a second time"
    );
    // The refusal happened before the port was entered, so the first run's channel is untouched
    // and the rejected attempt's is still sitting where its caller left it. Reclaiming it is the
    // command wrapper's job once the writer role is handed over; parking and never discarding
    // would strand one channel per rejected click.
    assert_eq!(
        scenario.sinks.parked(),
        1,
        "the rejected attempt parked exactly one channel"
    );
    assert!(
        scenario.sinks.discard("op-1"),
        "and the caller can still get it back"
    );

    scenario.release_agent();
    scenario.wait_for_terminal_status();
}

/// The other half of criterion 2: `cancel` is an intent, so it must reach every live run and
/// leave nothing registered behind.
#[test]
fn cancel_is_an_intent_over_the_real_executor() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("cancel");
    scenario.create_task();
    scenario.start("op-1").expect("launch");
    scenario.wait_for_launch();
    assert!(!scenario.child_keys().is_empty());

    scenario
        .service
        .cancel(&scenario.project, &scenario.task_id)
        .expect("cancel accepted");
    scenario.wait_for_retirement();

    assert_eq!(scenario.live_runs(), Vec::<RunKey>::new());
    assert_eq!(scenario.writer_keys(), Vec::<RunKey>::new());
    assert_eq!(scenario.child_keys(), Vec::<RunKey>::new());
    assert!(
        !scenario.has_live_run(),
        "a cancelled task must not still look live"
    );
}

/// §4.5.5: criterion 2 is only honest with a failure in it, because "the run never came up" is
/// the branch that has no caller to return to once the kernel is on the async runtime.
///
/// This injects at the port rather than inside the kernel: the operation parked a sink that is
/// not a terminal channel, so `LegacyTaskExecutor::launch` refuses synchronously. What is under
/// test is what core then does with a spawn that never produced a process — `abandon_launch`
/// must record the failure and release the claim, or the task would sit `running` with nothing
/// behind it and every later attempt would be treated as a duplicate of one that never happened.
#[test]
fn a_launch_that_refuses_at_the_port_is_recorded_and_releases_the_claim() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("refused-launch");
    scenario.create_task();
    scenario.sinks.park(
        "op-bad",
        OutputSink::Event {
            event_name: "not-a-terminal",
            id_key: "task_id",
        },
    );

    let error = scenario
        .service
        .start(
            &scenario.project,
            &scenario.task_id,
            "op-bad",
            true,
            None,
            false,
            Default::default(),
        )
        .expect_err("an agent process needs the channel it was started with");
    assert!(
        matches!(error, TaskError::Launch(_)),
        "a spawn refusal reaches the caller as a launch failure, got {error:?}"
    );
    let reason = error.to_string();
    assert!(
        reason.contains("op-bad"),
        "and it names the operation that went wrong: {reason}"
    );

    assert_eq!(
        scenario.spawn_count(),
        0,
        "a refusal must not start anything"
    );
    assert_eq!(
        scenario.live_runs(),
        Vec::<RunKey>::new(),
        "nothing registered, because nothing was launched"
    );
    // What the refusal becomes on the way out is the interesting part. `abandon_launch` mirrors
    // `reportLaunchFailure` → `onFailed` → `updateTaskStatus(failed, reason)`, so the attempt is
    // *recorded* rather than rolled back to `pending`: a task whose process never came up reads as
    // failed, which is what the shipping client already shows.
    let reloaded = scenario.service.load("p1").expect("reload");
    let row = reloaded
        .records
        .iter()
        .find(|row| row.id() == Some(scenario.task_id.as_str()))
        .expect("the row is still there");
    assert_eq!(
        row.get("status").and_then(Value::as_str),
        Some("failed"),
        "a spawn that never produced a process must leave a visible failure, not a running row"
    );
    assert!(
        scenario.service.open_claims(&scenario.task_id).is_empty(),
        "and release the claim, or the next legitimate start is locked out"
    );

    // The claim being free is what makes a real retry work; prove it rather than assert it.
    scenario
        .start("op-good")
        .expect("a later attempt is not locked out");
    scenario.wait_for_launch();
    assert_eq!(scenario.spawn_count(), 1);
    scenario.release_agent();
    scenario.wait_for_retirement();
}

/// `install_core_services` is what `run()` calls on every launch, and until now nothing had
/// ever called it: a wiring function that is never exercised is exactly the kind of code that
/// can be wrong in ways no unit test can see (wrong directory, forgotten state, a panic on the
/// startup path). This drives it once, over a scratch home.
#[test]
fn startup_wiring_installs_the_gateway_and_the_service_over_the_scratch_home() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("startup-wiring");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    std::fs::create_dir_all(home.path().join(".aeroric")).expect("data dir");

    let app = tauri::test::mock_app();
    crate::core_adapter::install_core_services(app.handle(), "0-test");

    let expected = home.path().join(".aeroric");
    {
        let gateway = app.state::<crate::core_adapter::CoreGateway>();
        assert_eq!(
            gateway.data_dir(),
            expected,
            "the gateway must resolve its directory through `platform::home_dir()`, not a guess"
        );
        assert_eq!(
            gateway.is_writer(),
            cfg!(target_os = "macos"),
            "ADR-021: only macOS takes the role, and the lease attempt must not even happen elsewhere"
        );
    }
    assert!(
        app.try_state::<TaskService>().is_some(),
        "no `TaskService` in managed state means the wiring is decorative"
    );
    assert!(
        app.try_state::<Arc<crate::legacy_task_executor::OutputSinkMailbox>>()
            .is_some(),
        "the channel mailbox has to be shared with the command that starts a task"
    );

    // Second instance over the same directory. Only meaningful where a lease was being held at
    // all — on other platforms both are `HandoverDeferred` by design (ADR-021), and asserting a
    // read-only refusal there would test the wrong thing.
    if cfg!(target_os = "macos") {
        let second = tauri::test::mock_app();
        crate::core_adapter::install_core_services(second.handle(), "0-test");
        let gateway = second.state::<crate::core_adapter::CoreGateway>();
        assert!(
            !gateway.is_writer(),
            "the first instance still holds the lease; this one must not write"
        );
        assert!(
            gateway
                .notice()
                .is_some_and(|notice| notice.contains("aeroric")),
            "a refusal the UI cannot explain is not a refusal: {:?}",
            gateway.notice()
        );
    }

    drop(scratch);
}

/// ADR-027 decisions 1 and 3, checked on the channel the user actually sees.
///
/// "Read-only" is only a real state if it reaches `list_startup_degradations`: the gateway's
/// `mode()` field is invisible to a user, and the ADR is explicit that reusing
/// `StartupDegradation` is not the same as satisfying it. Two things are pinned here — that a
/// refused lease is *reported*, and that what it names is the real data directory rather than a
/// relocated one (decision 3 forbids the temp-directory fallback, because two divergent data
/// sets that both look healthy is the worst available outcome).
#[test]
fn a_refused_lease_is_reported_and_names_the_directory_it_will_not_write() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("adr027-reporting");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let data_dir = home.path().join(".aeroric");
    std::fs::create_dir_all(&data_dir).expect("data dir");

    let lease_records = || -> Vec<crate::startup_diagnostics::StartupDegradation> {
        crate::startup_diagnostics::list_startup_degradations()
            .into_iter()
            .filter(|entry| entry.component == "storage-lease")
            .collect()
    };

    // ADR-021 × ADR-027 as one pair: the *first* instance records nothing on either platform.
    // On macOS it holds the lease; elsewhere it never even tries, and `HandoverDeferred` is
    // deliberately not a degradation — reporting "this platform has not been handed the role yet"
    // would hand every Windows and Linux user a warning about a fault that does not exist.
    let before = lease_records().len();
    let first = tauri::test::mock_app();
    crate::core_adapter::install_core_services(first.handle(), "0-test");
    let is_writer = first
        .state::<crate::core_adapter::CoreGateway>()
        .is_writer();
    assert_eq!(
        lease_records().len(),
        before,
        "an uncontested start adds no storage-lease record, whether it is the writer or deferred"
    );
    assert_eq!(is_writer, cfg!(target_os = "macos"));

    if !is_writer {
        drop(scratch);
        return;
    }

    // Only now is there a refusal to report. `record` is append-only and process-global, so a
    // count delta is the only honest shape for "this instance reported exactly one".
    let second = tauri::test::mock_app();
    crate::core_adapter::install_core_services(second.handle(), "0-test");
    let gateway = second.state::<crate::core_adapter::CoreGateway>();
    assert!(
        !gateway.is_writer(),
        "the first instance still holds the lease"
    );
    assert_eq!(
        gateway.data_dir(),
        data_dir,
        "the refusal is about *this* directory"
    );

    let reported = lease_records();
    assert_eq!(
        reported.len() - before,
        1,
        "exactly one new storage-lease record for the refused instance"
    );
    let entry = reported.last().expect("the record just added");
    // Exact equality, not a substring: "the fallback mentions the scratch root" would still pass
    // if the host had quietly moved the data elsewhere and left the old name in the message.
    assert_eq!(
        entry.fallback,
        data_dir.display().to_string(),
        "ADR-027 decision 3: the refusal names the directory it will not write — there is no          relocated copy for it to point at"
    );
    assert!(
        entry.reason.contains("held by another process"),
        "the reason is core's own words, so the holder hint survives: {:?}",
        entry.reason
    );
    // `scratch` drops before `home` by declaration order, which is what clears the global slot
    // before the directory is deleted.
}

/// ADR-036: the service `install_core_services` actually builds has to answer the liveness
/// question by asking the host, and the host's answer must count a run core never minted a
/// generation for — which is **every** run on the shipping client until the writer switch lands.
///
/// The scenarios above all use `NoLiveRuns`, so they cannot catch this seam being wrong: with a
/// stub that always answers "no" they stay green (their runs are core-minted, so the claim half
/// of the union carries them). This is the one test that drives the wired pair.
#[test]
fn the_wired_service_asks_the_host_about_a_run_core_never_minted() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("adr036-liveness");
    let _scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    std::fs::create_dir_all(home.path().join(".aeroric")).expect("data dir");

    let app = tauri::test::mock_app();
    app.manage(task_manager());
    crate::core_adapter::install_core_services(app.handle(), "0-test");

    let manager = app.state::<TaskManager>();
    let service = app.state::<TaskService>();

    assert!(
        !service.is_task_live("legacy-task"),
        "nothing is running yet: the port must not be a rubber stamp"
    );

    // Exactly what the legacy launch path leaves behind: a run in the host's registry with
    // generation 0, and no core claim anywhere.
    manager
        .runs
        .register("legacy-task", crate::run_registry::RunKey::UNMINTED);
    assert!(
        service.is_task_live("legacy-task"),
        "ADR-036: an unminted run is still a live run, or deleting the legacy emit sites \
         silences every status the product path reports"
    );

    assert!(
        manager
            .runs
            .retire("legacy-task", crate::run_registry::RunKey::UNMINTED),
        "and the retirement is heard by the same table"
    );
    assert!(
        !service.is_task_live("legacy-task"),
        "a run that ended must stop holding observations up"
    );
}
