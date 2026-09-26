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
    AgentFamilies, Applied, NewTask, NoLiveRuns, ObservationOrigin, ProtocolFamily, TaskError,
    TaskEvent, TaskExecutor, TaskIntent, TaskRuntimeObserver, TaskService, TaskStatus,
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
    service: Arc<TaskService>,
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
        let service = Arc::new(service);
        // The router (`TaskService::project_of`) resolves an id through the project list, which
        // is the same `projects.json` the client writes. Without it every observation in these
        // scenarios would land on the no-row path and the reducer's guards would never be
        // reached — a green channel that never asked the question.
        let project = Record(project_record(&project_path, tag));
        std::fs::write(
            service.context().paths.projects_file(),
            serde_json::to_vec(&json!([Value::Object(project.0.clone())])).expect("encode"),
        )
        .expect("projects.json");

        let events = Arc::new(Mutex::new(Vec::new()));
        listen(&handle, &events);

        Scenario {
            app: handle,
            _app: app,
            sinks,
            service,
            project,
            spawns,
            gate,
            events,
            task_id: format!("task-{tag}"),
            _scratch: scratch,
            _home: home,
        }
    }

    /// The real observation channel (ADR-033 §3): `install_core_services` builds this pair on
    /// every launch, so the acceptance scenarios drive the same type the product will. The
    /// write half routes on the start-up mode, so a scenario asks for the mode it means.
    fn observer_in(
        &self,
        mode: crate::core_adapter::CoreMode,
    ) -> crate::core_adapter::CoreRuntimeObserver<tauri::test::MockRuntime> {
        crate::core_adapter::CoreRuntimeObserver::new(
            Arc::clone(&self.service),
            Arc::new(crate::core_adapter::TauriEventSink::new(self.app.clone())),
            mode,
        )
    }

    /// The product observer on this machine's platform: macOS takes the lease, so the
    /// writer half is the live branch here.
    fn observer(&self) -> crate::core_adapter::CoreRuntimeObserver<tauri::test::MockRuntime> {
        self.observer_in(crate::core_adapter::CoreMode::Writer)
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
        app.try_state::<Arc<TaskService>>().is_some(),
        "no `TaskService` in managed state means the wiring is decorative"
    );
    assert!(
        app.try_state::<Arc<dyn aeroric_core::tasks::TaskRuntimeObserver>>()
            .is_some(),
        "the observation channel is what replaces the 28 emit sites; if it is not managed,          the handover has nowhere to go"
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
    let service = app.state::<Arc<TaskService>>();

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

    // The port answers `is_task_active`'s whole question, not just the registry's half: a
    // transcript watcher can be alive because the session file was found, with no pty run
    // registered at all. A narrower answer would drop those statuses, and nothing else here
    // would notice.
    manager.claude_sessions.lock().insert(
        crate::run_registry::RunKey::unminted("legacy-task"),
        crate::session::ClaudeSessionInfo {
            session_id: "s-1".into(),
            session_path: "/tmp/legacy-task.jsonl".into(),
            is_placeholder: false,
        },
    );
    assert!(
        service.is_task_live("legacy-task"),
        "a real session entry keeps the task active even with no live run"
    );
    manager
        .claude_sessions
        .lock()
        .values_mut()
        .for_each(|info| info.is_placeholder = true);
    assert!(
        !service.is_task_live("legacy-task"),
        "but a lazy-attach placeholder must not: that is `is_task_active`'s rule, and the          port inherits it rather than inventing a simpler one"
    );
}

/// ADR-033 §3's channel, end to end, on the writer side of ADR-021: a host-side fact enters
/// through `TaskRuntimeObserver`, core's reducer decides **and commits** (the store cutover
/// made core the writer of the task table), and the sink is the only thing that emits. The
/// committed row is what a second instance would read — the file is the proof, not a return
/// value.
#[test]
fn a_writer_observer_commits_its_decision_and_announces_exactly_once() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("observer-announce");
    scenario.create_task();
    scenario.start("op-1").expect("start");
    scenario.wait_for_launch();

    let sha_before = table_sha(&scenario.service, "p1");
    let before = scenario.emitted("task-status").len();

    scenario.observer().observe(TaskEvent::Status {
        task_id: scenario.task_id.clone(),
        generation: 1,
        status: TaskStatus::InputRequired,
        attention_requested_at_ms: Some(1_700_000_099_000),
        failure_reason: None,
        origin: ObservationOrigin::Transcript,
    });

    let after = scenario.emitted("task-status");
    assert_eq!(
        after.len(),
        before + 1,
        "one admitted observation is exactly one event; a second would be the loop ADR-023 \
         exists to prevent. statuses seen: {:?}",
        scenario.statuses()
    );
    assert_eq!(
        after.last().and_then(|payload| payload.get("status")),
        Some(&json!("input_required")),
        "and it is the status core decided, spelled the way the client listens for it"
    );
    assert_ne!(
        table_sha(&scenario.service, "p1"),
        sha_before,
        "the writer half of the observer must land the decision on the disk the next \
         instance reads; announcing without committing would leave the shipped client's \
         file contradicting what the UI shows"
    );
    let rows = scenario
        .service
        .load("p1")
        .expect("the committed table is readable")
        .records;
    assert!(
        rows.iter().any(|row| {
            row.id() == Some(scenario.task_id.as_str())
                && row.get("status").and_then(Value::as_str) == Some("input_required")
                && row.get("attentionRequestedAt").is_some()
        }),
        "and the committed row is the observation, not the pending row `start` wrote"
    );

    // The drop half, reported through the same channel rather than assumed: the host's own
    // exit path emits `done` and settles nothing in core, so core still holds generation 1's
    // claim until an observation says otherwise. That is the state ADR-034's guard reads.
    scenario.observer().observe(TaskEvent::Exit {
        task_id: scenario.task_id.clone(),
        generation: 1,
        code: Some(0),
    });
    assert!(
        scenario.service.open_claims(&scenario.task_id).is_empty(),
        "a terminal observation must settle the run it reports, or nothing is ever late"
    );
    scenario.release_agent();
    scenario.wait_for_retirement();

    let settled = scenario.emitted("task-status").len();
    scenario.observer().observe(TaskEvent::Status {
        task_id: scenario.task_id.clone(),
        generation: 1,
        status: TaskStatus::Running,
        attention_requested_at_ms: None,
        failure_reason: None,
        origin: ObservationOrigin::Transcript,
    });
    assert_eq!(
        scenario.emitted("task-status").len(),
        settled,
        "ADR-034's guard is what makes the handover safe; the sink may not re-light a settled \
         card. If `project_of` had failed to route, this would have announced anyway and the \
         channel would look wider than it is. statuses seen: {:?}",
        scenario.statuses()
    );
}

/// The read-only side of the same channel (ADR-027): a refused instance decides and
/// announces — its UI must still hear the task's life — but writes nothing anywhere,
/// bridge included. `decide_observation` is exactly the write-free half of the split.
#[test]
fn a_read_only_observer_announces_without_writing() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("observer-readonly");
    scenario.create_task();
    scenario.start("op-1").expect("start");
    scenario.wait_for_launch();

    let sha_before = table_sha(&scenario.service, "p1");
    let before = scenario.emitted("task-status").len();

    scenario
        .observer_in(crate::core_adapter::CoreMode::ReadOnlyBusy)
        .observe(TaskEvent::Status {
            task_id: scenario.task_id.clone(),
            generation: 1,
            status: TaskStatus::InputRequired,
            attention_requested_at_ms: Some(1_700_000_099_000),
            failure_reason: None,
            origin: ObservationOrigin::Transcript,
        });

    assert_eq!(
        scenario.emitted("task-status").len(),
        before + 1,
        "read-only is not silent: the instance's own UI still gets the event"
    );
    assert_eq!(
        table_sha(&scenario.service, "p1"),
        sha_before,
        "and ADR-027 holds at the channel level: not one byte reaches the disk, even though \
         core holds a writer lease in this scenario — the mode, not the lease, decides"
    );
    scenario.release_agent();
    scenario.wait_for_retirement();
}

/// ADR-033 §4's floor: an observation core cannot route is still announced. Silence here would
/// be the "quietly stop emitting to make the count look good" outcome that ADR forbids, and it
/// would be invisible to every other check.
#[test]
fn an_unroutable_observation_announces_rather_than_going_quiet() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scenario = Scenario::new("observer-unroutable");
    scenario.create_task();
    scenario.start("op-1").expect("start");
    scenario.wait_for_launch();

    let before = scenario.emitted("task-status").len();
    scenario.observer().observe(TaskEvent::Exit {
        task_id: "a-task-core-never-heard-of".into(),
        generation: 0,
        code: Some(3),
    });
    let after = scenario.emitted("task-status");
    assert_eq!(
        after.len(),
        before + 1,
        "no row means no guard applies, not no event"
    );
    let payload = after.last().cloned().expect("one event");
    assert_eq!(payload.get("status"), Some(&json!("failed")));
    assert_eq!(
        payload.get("failure_reason").and_then(Value::as_str),
        Some("the process exited with code 3"),
        "the reason comes from core's one exit rule, the same one `observe` uses — not from \
         the host re-deriving it"
    );
    scenario.release_agent();
    scenario.wait_for_retirement();
}

/// The host entry point for ADR-022: JS hands over one intent, never a whole-table snapshot.
///
/// Asserted on both sides of the ADR-021 line rather than only macOS, because the two branches
/// must produce the *same file* — what differs is only who wrote it. The unknown key is the
/// point: a host that re-serializes through its own `Task` struct would drop it (that is exactly
/// how `launchFamily` would have been erased), so the write must forward core's bytes.
#[test]
fn an_intent_is_executed_by_whichever_side_holds_the_writer_role() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("intent-entry");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let data = home.path().join(".aeroric");
    std::fs::create_dir_all(&data).expect("data dir");
    std::fs::create_dir_all(data.join("projects/p1")).expect("project dir");
    let tasks_file = data.join("projects/p1/tasks.json");
    std::fs::write(
        &tasks_file,
        json!([{
            "id": "t1", "projectId": "p1", "prompt": "do it", "agent": "claude",
            "permissionMode": "ask", "status": "todo", "createdAt": 1,
            "launchFamily": "claude", "aFieldTheHostHasNotDeclared": "keep me"
        }])
        .to_string(),
    )
    .expect("seed tasks");

    let app = tauri::test::mock_app();
    crate::core_adapter::install_core_services(app.handle(), "0-test");
    let writer = app.state::<crate::core_adapter::CoreGateway>().is_writer();

    let outcome = crate::core_adapter::apply_task_intent_core(
        app.handle(),
        "p1",
        &TaskIntent::Rename {
            task_id: "t1".into(),
            name: "改在 core 里".into(),
            expect: None,
        },
    )
    .expect("an intent must be executable on either side of the handover line");

    assert!(outcome.changed, "the row really changed");
    assert_eq!(
        outcome.committed_by_core, writer,
        "`committedByCore` must say who wrote, not merely that something was written"
    );
    assert_eq!(
        outcome.revision.is_some(),
        writer,
        "only core buying a cover revision may report one; a host-side save has no revision to show"
    );
    assert_eq!(
        outcome.tasks[0]["name"],
        json!("改在 core 里"),
        "the response is the authoritative snapshot the store should adopt"
    );

    let on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(&tasks_file).expect("written file"))
            .expect("valid json");
    assert_eq!(
        on_disk[0]["name"],
        json!("改在 core 里"),
        "the intent landed"
    );
    assert_eq!(
        on_disk[0]["aFieldTheHostHasNotDeclared"],
        json!("keep me"),
        "a key no side declares must survive the write — forwarding core's bytes is what keeps \
         the handover from silently deleting fields the host cannot name"
    );
    assert_eq!(on_disk[0]["launchFamily"], json!("claude"));
    drop(scratch);
}

/// 延迟递交写权那一格，测在它测得到的地方：`writer_handover_allowed()` 是 `cfg!`，macOS 上永远
/// 进不去 `CoreMode::HandoverDeferred` 那条臂。能测的是臂的**体**——core 决定、宿主按字节存。
///
/// 这条曾经走 `decide_intent` + 宿主侧转发。换成桥以后断言更硬了：桥会把 generation 副本对齐到
/// 账本编号（`commit` 也做，`DecideOnly` 不做），所以"延迟平台存下的字节"与"写者存下的字节"是
/// 同一套逻辑的产物，而不是两套各自实现的近似。未知键仍然是重点：宿主一旦过自己的 `Task` 结构体，
/// 它就没了（计划补记六十就是这个坑）。
#[test]
fn the_bridge_stores_a_deferring_cores_decision_verbatim() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("intent-bridge");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let data = home.path().join(".aeroric");
    std::fs::create_dir_all(&data).expect("data dir");
    std::fs::create_dir_all(data.join("projects/p1")).expect("project dir");
    let tasks_file = data.join("projects/p1/tasks.json");
    let seeded = json!([{
        "id": "t1", "projectId": "p1", "prompt": "do it", "agent": "claude",
        "permissionMode": "ask", "status": "todo", "createdAt": 1,
        "launchFamily": "codex", "aFieldTheHostHasNotDeclared": "keep me"
    }])
    .to_string();
    std::fs::write(&tasks_file, &seeded).expect("seed tasks");

    let app = tauri::test::mock_app();
    let executor: Arc<dyn TaskExecutor> =
        Arc::new(LegacyTaskExecutor::<tauri::test::MockRuntime>::new(
            app.handle().clone(),
            Arc::new(OutputSinkMailbox::new()),
        ));
    let service = TaskService::new(
        Arc::new(crate::core_adapter::deferred_context(
            AppPaths::with_data_dir(home.path(), Some(&data)),
        )),
        Arc::new(SettingsFamilies) as Arc<dyn AgentFamilies>,
        executor,
        Arc::new(NoLiveRuns),
    )
    .expect("a deferring context still builds a service");

    let applied = service
        .apply_intent_via_host(
            "p1",
            &TaskIntent::ToggleStar {
                task_id: "t1".into(),
            },
        )
        .expect("the host bridge stores what core decided");

    assert!(applied.changed, "the row really changed");
    assert!(
        applied.revision.is_none() && applied.fingerprint.is_none(),
        "a host-side save buys no cover revision — reporting one would invent a ledger"
    );
    let on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(&tasks_file).expect("written file"))
            .expect("valid json");
    let decided: Vec<Value> = applied
        .records
        .iter()
        .map(|record| Value::Object(record.0.clone()))
        .collect();
    assert_eq!(
        on_disk,
        Value::Array(decided),
        "the file must be exactly what core decided, not a host-side re-derivation of it"
    );
    assert_eq!(on_disk[0]["starred"], json!(true));
    assert_eq!(
        on_disk[0]["aFieldTheHostHasNotDeclared"],
        json!("keep me"),
        "the whole reason this step forwards bytes instead of `Task` values"
    );
    assert_eq!(on_disk[0]["launchFamily"], json!("codex"));
    drop(scratch);
}

/// 装不装桥，就是"延迟递交写权"与"只读"之间唯一的差别——所以这条差别只能有一处定义。
///
/// 两种只读用的都是 `CoreContext::read_only` 本身，拿不到桥，于是 `ViaHost` 在它们身上是错误
/// 而不是"照写"（ADR-027：租约在另一个活实例手里时，一个字节都不许动）。反向的那半句由 core 的
/// `a_via_host_write_without_a_bridge_is_a_refusal_not_a_silent_skip` 守住；这里守的是宿主要装。
#[test]
fn only_a_deferring_context_is_offered_a_route_to_the_disk() {
    let home = ScratchRoot::new("bridge-presence");
    let paths = AppPaths::with_data_dir(home.path(), Some(&home.path().join(".aeroric")));
    assert!(
        crate::core_adapter::deferred_context(paths.clone())
            .task_saver()
            .is_some(),
        "'core decides, the host stores' is exactly the saver being installed"
    );
    assert!(
        CoreContext::read_only(paths).task_saver().is_none(),
        "a read-only instance must have no door out to the disk at all"
    );
}

/// ADR-027's "read-only" claim, tested where it is actually at risk: a write entry point.
///
/// A second instance over the same directory must refuse an intent outright and leave the file
/// byte-identical. Without this, "read-only" would only be a UI statement while the intent path
/// happily saved — the exact failure mode ADR-027 exists to prevent.
#[test]
fn a_read_only_instance_refuses_an_intent_and_writes_nothing() {
    if !cfg!(target_os = "macos") {
        // Off macOS both instances are `HandoverDeferred` by design (ADR-021), so there is no
        // lease holder to be refused. Asserting a refusal here would test the wrong mechanism.
        return;
    }
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("intent-readonly");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let data = home.path().join(".aeroric");
    std::fs::create_dir_all(&data).expect("data dir");
    std::fs::create_dir_all(data.join("projects/p1")).expect("project dir");
    let tasks_file = data.join("projects/p1/tasks.json");
    let seeded = json!([{
        "id": "t1", "projectId": "p1", "prompt": "do it", "agent": "claude",
        "permissionMode": "ask", "status": "todo", "createdAt": 1
    }])
    .to_string();
    std::fs::write(&tasks_file, &seeded).expect("seed tasks");

    let first = tauri::test::mock_app();
    crate::core_adapter::install_core_services(first.handle(), "0-test");
    assert!(
        first
            .state::<crate::core_adapter::CoreGateway>()
            .is_writer(),
        "the first instance must hold the lease for this test to mean anything"
    );

    let second = tauri::test::mock_app();
    crate::core_adapter::install_core_services(second.handle(), "0-test");
    let error = crate::core_adapter::apply_task_intent_core(
        second.handle(),
        "p1",
        &TaskIntent::ToggleStar {
            task_id: "t1".into(),
        },
    )
    .expect_err("a read-only instance must not save");
    assert!(
        error.contains("只读"),
        "the refusal has to be explainable to the user, got: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(&tasks_file).expect("still there"),
        seeded,
        "refusing must mean refusing: not one byte written"
    );
    drop(scratch);
}

/// `launch_via_core` 的三条用例共用的播种：`projects.json` 里有 p1，`projects/p1/tasks.json`
/// 里有那一行。写在 `install_core_services` **之后**是刻意的——`projects.json` 的路径由 service
/// 自己的 context 给出，测试再算一遍就是第二个真相源（ADR-027 反对的就是这个）。
fn seed_launchable_task(service: &TaskService, tasks_file: &Path, row: Value) {
    std::fs::create_dir_all(tasks_file.parent().expect("project dir")).expect("project dir");
    std::fs::write(
        tasks_file,
        serde_json::to_vec(&json!([row])).expect("encode tasks"),
    )
    .expect("tasks.json");
    std::fs::write(
        service.context().paths.projects_file(),
        serde_json::to_vec(&json!([project_record(
            tasks_file
                .parent()
                .expect("project dir")
                .parent()
                .expect("data dir"),
            "launch"
        )]))
        .expect("encode projects"),
    )
    .expect("projects.json");
}

/// 解析不出项目就不能递交，而这条错误路径**必须在寄存之前**：channel 一旦寄出去又没人来取，
/// 它就永久留在 mailbox 里（`parked()` 这个计数存在就是为了发现这种残留）。
#[test]
fn a_launch_that_cannot_find_its_project_parks_no_channel() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("launch-no-project");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let app = tauri::test::mock_app();
    crate::core_adapter::install_core_services(app.handle(), "0-test");

    let error = crate::core_adapter::launch_via_core(
        app.handle(),
        "t-missing",
        OutputSink::Channel(Channel::new(|_| Ok(()))),
        false,
        None,
        false,
        aeroric_core::tasks::Attachments::default(),
        None,
        None,
    )
    .expect_err("a task no project owns cannot be launched through core");
    assert!(
        error.contains("t-missing"),
        "the refusal has to name the task the user was looking at, got: {error}"
    );
    assert_eq!(
        app.state::<Arc<crate::legacy_task_executor::OutputSinkMailbox>>()
            .parked(),
        0,
        "nothing was handed out, so nothing may be left parked"
    );
    drop(scratch);
}

/// 失败注入（§12 验收点名的那一次）：执行器的 SSH 分支在**派发之前**同步解析连接，查不到
/// 就按名拒绝，所以这条用例不需要任何真二进制，却能同时打中三件事——错误要传回调用方、寄出的
/// channel 要收回来、那一行要落成 failed。第三件是 `abandon_launch` 的补偿语义：今天由
/// `finalize_task_exit` 负责的事，接线的同时就得由 core 负责，否则"进程没起来"在盘上会留下
/// 一行 pending。dsh 分支曾经是这条注入的宿主，接线后它按名分发了 `run_dsh_task_core`；
/// 同步拒绝的最后一种形状就是这条远端连接解析。
#[test]
fn a_launch_the_executor_refuses_lands_failed_and_gives_the_channel_back() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("launch-refused");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let data = home.path().join(".aeroric");
    std::fs::create_dir_all(&data).expect("data dir");
    let tasks_file = data.join("projects/p1/tasks.json");

    let app = tauri::test::mock_app();
    crate::core_adapter::install_core_services(app.handle(), "0-test");
    let service = app.state::<Arc<TaskService>>();
    seed_launchable_task(
        &service,
        &tasks_file,
        json!({
            "id": "t1", "projectId": "p1", "prompt": "do it", "agent": "codex",
            "permissionMode": "ask", "status": "todo", "createdAt": 1
        }),
    );
    // 项目本身是 SSH 目标，而那个 connection id 在宿主里不存在——连接解析是内核之前
    // 唯一能同步拒绝的分支， refusal 因此带着执行器的原话回到调用方。
    std::fs::write(
        service.context().paths.projects_file(),
        serde_json::to_vec(&json!([{
            "id": "p1",
            "name": "project-launch",
            "path": "/remote/repo",
            "location": {"kind": "ssh", "connectionId": "conn-missing", "remotePath": "/remote/repo"},
        }]))
        .expect("encode projects"),
    )
    .expect("projects.json");

    let error = crate::core_adapter::launch_via_core(
        app.handle(),
        "t1",
        OutputSink::Channel(Channel::new(|_| Ok(()))),
        false,
        None,
        false,
        aeroric_core::tasks::Attachments::default(),
        None,
        None,
    )
    .expect_err(
        "an unknown ssh connection refuses by name, and that refusal must not be swallowed",
    );
    assert!(
        error.contains("conn-missing"),
        "the launch error must carry the executor's own words, got: {error}"
    );
    assert_eq!(
        app.state::<Arc<crate::legacy_task_executor::OutputSinkMailbox>>()
            .parked(),
        0,
        "the executor refused before taking the channel, so the entry had to take it back out"
    );

    let on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(&tasks_file).expect("written file"))
            .expect("valid json");
    assert_eq!(
        on_disk[0]["status"],
        json!("failed"),
        "a launch that never produced a process is not left reading pending"
    );
    assert!(
        on_disk[0]["failureReason"]
            .as_str()
            .unwrap_or_default()
            .contains("conn-missing"),
        "the reason the user sees has to be the reason the executor gave: {on_disk}"
    );
    drop(scratch);
}

/// 只读实例的启动：与意图入口那条拒绝用例同一个口径，但它多测一件那边没有的事——**拒绝发生在
/// 寄存之后**，所以 channel 必须被拿回来。留在 mailbox 里的那个 channel 不是"什么都没发生"，
/// 它是那个终端从此收不到任何字节的原因。
#[test]
fn a_read_only_instance_refuses_a_launch_and_returns_the_channel() {
    if !cfg!(target_os = "macos") {
        // Off macOS both instances are `HandoverDeferred` by design (ADR-021) and both may
        // launch through the bridge, so there is no refusal to observe here.
        return;
    }
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("launch-readonly");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let data = home.path().join(".aeroric");
    std::fs::create_dir_all(&data).expect("data dir");
    let tasks_file = data.join("projects/p1/tasks.json");
    let seeded = json!({
        "id": "t1", "projectId": "p1", "prompt": "do it", "agent": "codex",
        "permissionMode": "ask", "status": "todo", "createdAt": 1
    });

    let first = tauri::test::mock_app();
    crate::core_adapter::install_core_services(first.handle(), "0-test");
    let service = first.state::<Arc<TaskService>>();
    seed_launchable_task(&service, &tasks_file, seeded.clone());
    assert!(
        first
            .state::<crate::core_adapter::CoreGateway>()
            .is_writer(),
        "the first instance must hold the lease for this test to mean anything"
    );

    let second = tauri::test::mock_app();
    crate::core_adapter::install_core_services(second.handle(), "0-test");
    let error = crate::core_adapter::launch_via_core(
        second.handle(),
        "t1",
        OutputSink::Channel(Channel::new(|_| Ok(()))),
        false,
        None,
        false,
        aeroric_core::tasks::Attachments::default(),
        None,
        None,
    )
    .expect_err("a read-only instance must not start a process over a row it cannot save");
    assert!(
        error.contains("只读"),
        "the refusal has to be explainable: {error}"
    );
    assert_eq!(
        second
            .state::<Arc<crate::legacy_task_executor::OutputSinkMailbox>>()
            .parked(),
        0,
        "refusing after parking is exactly the case where a channel could be leaked"
    );
    assert_eq!(
        on_disk_rows(&tasks_file)[0]["status"],
        json!("todo"),
        "refusing must mean refusing: the row still reads todo"
    );
    drop(scratch);
}

fn on_disk_rows(tasks_file: &Path) -> Vec<Value> {
    serde_json::from_str(&std::fs::read_to_string(tasks_file).expect("readable"))
        .expect("valid json array")
}

/// 守卫在宿主这一侧同样成立，而这条用例的重点是它**发生在寄存之后**：空 session 是 core 的
/// `resume` 拒的，那时 channel 已经寄出去了，所以入口必须自己取回来。`launch` 那条同类用例正好
/// 相反（解析不出项目 ⇒ 寄存之前就失败，`parked()` 本来就是 0）——两条合起来才把"寄存前失败"和
/// "寄存后失败"这两种错误路径都钉住，而只有后者会漏下取不回来的窗口。
#[test]
fn a_resume_with_no_session_is_refused_after_parking_and_writes_nothing() {
    let _serial = SCENARIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = ScratchRoot::new("resume-blank");
    let scratch = crate::platform::install_scratch_home(home.path().to_path_buf());
    let data = home.path().join(".aeroric");
    std::fs::create_dir_all(&data).expect("data dir");
    let tasks_file = data.join("projects/p1/tasks.json");

    let app = tauri::test::mock_app();
    crate::core_adapter::install_core_services(app.handle(), "0-test");
    let service = app.state::<Arc<TaskService>>();
    seed_launchable_task(
        &service,
        &tasks_file,
        json!({
            "id": "t1", "projectId": "p1", "prompt": "do it", "agent": "claude",
            "permissionMode": "ask", "status": "interrupted", "createdAt": 1
        }),
    );
    let seeded = std::fs::read_to_string(&tasks_file).expect("seeded bytes");

    let error = crate::core_adapter::resume_via_core(
        app.handle(),
        "t1",
        "",
        OutputSink::Channel(Channel::new(|_| Ok(()))),
        true,
        None,
    )
    .expect_err("a blank session id is not the same as resume-with-no-session");
    assert!(
        error.contains("no session"),
        "core's own words must reach the caller, got: {error}"
    );
    assert_eq!(
        app.state::<Arc<crate::legacy_task_executor::OutputSinkMailbox>>()
            .parked(),
        0,
        "the channel was parked before core looked at the session, so the entry had to take it back"
    );
    assert_eq!(
        std::fs::read_to_string(&tasks_file).expect("still readable"),
        seeded,
        "refusing means refusing: not one byte, and no half-written session either"
    );
    drop(scratch);
}
