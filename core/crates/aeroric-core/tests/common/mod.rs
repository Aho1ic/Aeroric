//! The harness both P2-04 test files share: a writer over a throwaway directory, a
//! family table, and an executor that *records* what it was asked to do.
//!
//! The executor is the point. Every acceptance claim in this task is about ordering and
//! counting — was the record on disk before the process existed, did one attempt spawn one
//! process, did a late exit move anything — and none of that is observable from a return
//! value. So each call appends to a log, a failure can be armed in advance, and a test can
//! ask "what actually happened, in what order".

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::storage::compat::Record;
use aeroric_core::tasks::executor::{
    CancelTarget, ExecutorFailure, LaunchRequest, LiveRuns, NoLiveRuns, ResetOutcome,
    ResumeRequest, TaskEvent, TaskExecutor,
};
use aeroric_core::tasks::service::TaskService;
use aeroric_core::tasks::session_owner::AgentFamilies;
use aeroric_core::{AppPaths, CoreContext, FixedClock, LeaseOwner};
use aeroric_protocol::task::ProtocolFamily;
use serde_json::{json, Value};

struct LocalVolume;

impl StorageLocationProbe for LocalVolume {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        StorageLocation::new(data_dir, VolumeKind::Local)
    }
}

pub fn writer(tag: &str) -> (PathBuf, CoreContext) {
    let root = std::env::temp_dir().join(format!(
        "aeroric-tasks-{tag}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::remove_dir_all(&root).ok();
    let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
    let context = CoreContext::acquire(
        paths,
        Arc::new(FixedClock::at(1_700_000_000_000)),
        &LeaseOwner {
            application: "test".into(),
            version: "0".into(),
        },
        &LocalVolume,
    )
    .expect("lease");
    (root, context)
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

pub fn record(value: Value) -> Record {
    Record(value.as_object().expect("an object").clone())
}

pub fn local_project(id: &str, path: &str) -> Record {
    record(json!({"id": id, "path": path, "name": "repo"}))
}

pub fn task(id: &str, project_id: &str, status: &str) -> Record {
    record(json!({
        "id": id, "projectId": project_id, "prompt": "do it", "agent": "claude",
        "status": status, "permissionMode": "ask", "createdAt": 1_700_000_000_000i64
    }))
}

/// The four built-ins plus one custom profile, so "same family, different home" is a case
/// that can actually be written down.
#[derive(Clone, Default)]
pub struct Families;

impl AgentFamilies for Families {
    /// Mirrors the shipping client, both halves (ADR-028): `claude_gpt55` keeps
    /// Claude-format sessions but is started with codex-style arguments, because
    /// `src/agents.ts` gives it `codexLike: true` and `pty.rs` picks `build_codex_cmd`
    /// from that flag.
    fn launch_family(&self, agent: &str) -> ProtocolFamily {
        match agent {
            "claude" => ProtocolFamily::Claude,
            "claude_gpt55" | "codex" => ProtocolFamily::Codex,
            "dsh" => ProtocolFamily::Dsh,
            "omp" => ProtocolFamily::Omp,
            _ => self.session_family(agent),
        }
    }
    fn session_family(&self, agent: &str) -> ProtocolFamily {
        match agent {
            "claude" | "claude_gpt55" => ProtocolFamily::Claude,
            "codex" => ProtocolFamily::Codex,
            "dsh" => ProtocolFamily::Dsh,
            "omp" => ProtocolFamily::Omp,
            "my-claude" => ProtocolFamily::Claude,
            _ => ProtocolFamily::Claude,
        }
    }
    fn is_builtin(&self, agent: &str) -> bool {
        matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh" | "omp")
    }
    /// ADR-029: Claude transcripts, codex-style CLI. `claude` → `claude_gpt55` therefore is
    /// *not* a native resume, which is what the shipping client actually concludes.
    fn native_resume_family(&self, agent: &str) -> ProtocolFamily {
        match agent {
            "claude_gpt55" => ProtocolFamily::Codex,
            other => self.session_family(other),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub what: &'static str,
    pub task_id: String,
    pub detail: String,
}

/// What a test asserts on: the ordered log plus the state of the record *at the moment the
/// executor was called*, which is the only way to prove "persisted before spawned".
#[derive(Clone, Debug)]
pub struct Seen {
    pub calls: Vec<Call>,
    /// Read back from disk inside the call, so a spawn that raced ahead of the save is
    /// caught by the test rather than argued about.
    pub disk_at_call: Vec<Option<Value>>,
}

pub struct FakeExecutor {
    inner: Mutex<Seen>,
    /// `what` → fail that call once.
    fail: Mutex<Vec<&'static str>>,
    project_id: Mutex<String>,
    tasks_file: Mutex<PathBuf>,
    events: Mutex<Vec<TaskEvent>>,
}

impl FakeExecutor {
    pub fn new(tasks_file: PathBuf, project_id: &str) -> Arc<Self> {
        Arc::new(FakeExecutor {
            inner: Mutex::new(Seen {
                calls: Vec::new(),
                disk_at_call: Vec::new(),
            }),
            fail: Mutex::new(Vec::new()),
            project_id: Mutex::new(project_id.to_string()),
            tasks_file: Mutex::new(tasks_file),
            events: Mutex::new(Vec::new()),
        })
    }

    pub fn arm_failure(&self, what: &'static str) {
        self.fail.lock().unwrap().push(what);
    }

    pub fn clear_failure(&self, what: &'static str) {
        self.fail.lock().unwrap().retain(|item| *item != what);
    }

    fn should_fail(&self, what: &'static str) -> bool {
        let mut guard = self.fail.lock().unwrap();
        if let Some(at) = guard.iter().position(|item| *item == what) {
            guard.remove(at);
            return true;
        }
        false
    }

    pub fn calls(&self) -> Vec<Call> {
        self.inner.lock().unwrap().calls.clone()
    }

    pub fn call_log(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .map(|call| format!("{}:{}", call.what, call.task_id))
            .collect()
    }

    pub fn launches(&self) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|call| call.what == "launch")
            .collect()
    }

    pub fn disk_at_call(&self, index: usize) -> Option<Value> {
        self.inner
            .lock()
            .unwrap()
            .disk_at_call
            .get(index)
            .cloned()
            .flatten()
    }

    /// Snapshot of `tasks.json` as the executor saw it — captured inside the call.
    pub fn disk_snapshot(&self) -> Option<Value> {
        read_tasks(&self.tasks_file.lock().unwrap())
    }

    fn note(&self, what: &'static str, task_id: &str, detail: String) {
        let snapshot = read_tasks(&self.tasks_file.lock().unwrap());
        let mut guard = self.inner.lock().unwrap();
        guard.calls.push(Call {
            what,
            task_id: task_id.to_string(),
            detail,
        });
        guard.disk_at_call.push(snapshot);
    }

    /// Stand in for the runtime: the host's observer is what forwards these to
    /// `TaskService::observe`, so a test decides whether an event reaches the reducer.
    pub fn emit(&self, event: TaskEvent) {
        self.events.lock().unwrap().push(event);
    }

    pub fn emitted(&self) -> Vec<TaskEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Where this project's records live, taken from the context that wrote them so a test
    /// never re-derives the layout.
    pub fn tasks_path(&self) -> PathBuf {
        self.tasks_file.lock().unwrap().clone()
    }

    pub fn project_id(&self) -> String {
        self.project_id.lock().unwrap().clone()
    }
}

fn read_tasks(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

impl TaskExecutor for FakeExecutor {
    fn launch(&self, request: &LaunchRequest) -> Result<(), ExecutorFailure> {
        let detail = format!(
            "gen={} cwd={} prompt={:?} dsh={}",
            request.generation,
            match &request.working_directory {
                aeroric_core::tasks::WorkingDirectory::Local { path } => path.clone(),
                aeroric_core::tasks::WorkingDirectory::Ssh { remote_path, .. } =>
                    remote_path.clone(),
                aeroric_core::tasks::WorkingDirectory::Wsl { path, .. } => path.clone(),
            },
            request
                .prompt_override
                .clone()
                .unwrap_or_else(|| request.prompt.clone()),
            request.family == ProtocolFamily::Dsh,
        );
        self.note("launch", &request.task_id, detail);
        if self.should_fail("launch") {
            return Err(ExecutorFailure::Spawn {
                reason: "permission denied (os error 13)".into(),
            });
        }
        Ok(())
    }

    fn resume(&self, request: &ResumeRequest) -> Result<(), ExecutorFailure> {
        self.note(
            "resume",
            &request.task_id,
            format!(
                "gen={} session={} prompt={:?}",
                request.generation, request.session_id, request.prompt
            ),
        );
        if self.should_fail("resume") {
            return Err(ExecutorFailure::Spawn {
                reason: "the session file is gone".into(),
            });
        }
        Ok(())
    }

    fn cancel(&self, task_id: &str, target: &CancelTarget) -> Result<(), ExecutorFailure> {
        self.note("cancel", task_id, format!("{target:?}"));
        if self.should_fail("cancel") {
            return Err(ExecutorFailure::Stop {
                reason: "no such process".into(),
            });
        }
        Ok(())
    }

    fn complete(&self, task_id: &str, working_directory: &str) -> Result<(), ExecutorFailure> {
        self.note("complete", task_id, working_directory.to_string());
        if self.should_fail("complete") {
            return Err(ExecutorFailure::Stop {
                reason: "the task is not running".into(),
            });
        }
        Ok(())
    }

    fn reset(&self, task_id: &str) -> Result<ResetOutcome, ExecutorFailure> {
        self.note("reset", task_id, String::new());
        if self.should_fail("reset") {
            return Err(ExecutorFailure::Stop {
                reason: "the process is not registered".into(),
            });
        }
        Ok(ResetOutcome::default())
    }
}

/// A directory seeded with `initial` as `projects/p1/tasks.json`, plus the executor that
/// will be asked to run things in it.
pub fn harness(tag: &str, initial: &[Record]) -> (PathBuf, CoreContext, Arc<FakeExecutor>) {
    let (root, context) = writer(tag);
    let tasks_file = context.paths.tasks_file("p1").expect("path");
    std::fs::create_dir_all(tasks_file.parent().unwrap()).expect("project dir");
    std::fs::write(
        &tasks_file,
        serde_json::to_vec_pretty(&Value::Array(
            initial
                .iter()
                .map(|row| Value::Object(row.0.clone()))
                .collect(),
        ))
        .unwrap(),
    )
    .expect("seed");
    let executor = FakeExecutor::new(tasks_file, "p1");
    (root, context, executor)
}

pub fn build_service(context: CoreContext, executor: &Arc<FakeExecutor>) -> TaskService {
    build_service_with_liveness(context, executor, Arc::new(NoLiveRuns))
}

/// ADR-036's seam under test: some hosts must be able to answer "this task is still running"
/// for a run core never started, so the port has to be injectable rather than hard-wired.
pub fn build_service_with_liveness(
    context: CoreContext,
    executor: &Arc<FakeExecutor>,
    live_runs: Arc<dyn LiveRuns>,
) -> TaskService {
    let families: Arc<dyn AgentFamilies> = Arc::new(Families);
    let runner: Arc<dyn TaskExecutor> = executor.clone();
    // `build_service` still takes the context by value so the 20-odd callers that got one from
    // `writer()`/`reacquire()` stay as they are; the Arc is the service's sharing rule, not
    // theirs.
    TaskService::new(Arc::new(context), families, runner, live_runs).expect("service")
}

/// A `LiveRuns` that answers for exactly the ids a test names, and counts how many times core
/// actually asked. The counter is the point: without it a port that is never consulted still
/// passes every assertion.
#[derive(Default)]
pub struct StubLiveness {
    live: Mutex<Vec<String>>,
    asked: std::sync::atomic::AtomicUsize,
}

impl StubLiveness {
    pub fn live(ids: &[&str]) -> Arc<Self> {
        Arc::new(StubLiveness {
            live: Mutex::new(ids.iter().map(|id| id.to_string()).collect()),
            asked: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    pub fn asks(&self) -> usize {
        self.asked.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl LiveRuns for StubLiveness {
    fn has_live_run(&self, task_id: &str) -> bool {
        self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|id| id == task_id)
    }
}

pub fn cleanup(root: PathBuf) {
    std::fs::remove_dir_all(root).ok();
}

/// The same directory as seen by a process that was **never handed the writer role**
/// (ADR-021: that is every non-macOS install today). No lease is attempted, so
/// `require_write` fails and `load_*` still work — which is precisely the split ADR-035
/// builds the announcement path on.
pub fn reader(root: &Path) -> CoreContext {
    let paths = AppPaths::with_data_dir(root, Some(&root.join("managed")));
    CoreContext::read_only(paths).with_clock(Arc::new(FixedClock::at(1_700_000_005_000)))
}

/// Re-open a directory the way a fresh process would: same paths, new lease, new epoch.
/// This is how the crash-recovery tests prove that nothing important lived only in the
/// first service's memory.
pub fn reacquire(root: &Path) -> CoreContext {
    let paths = AppPaths::with_data_dir(root, Some(&root.join("managed")));
    CoreContext::acquire(
        paths,
        Arc::new(FixedClock::at(1_700_000_005_000)),
        &LeaseOwner {
            application: "test".into(),
            version: "0".into(),
        },
        &LocalVolume,
    )
    .expect("lease")
}
