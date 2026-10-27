//! Manual smoke against a real `omp` binary (ADR-043 phase two). Ignored by
//! default: run with `AERORIC_OMP_SMOKE_HOME=<agent dir with models.yml>`
//! pointing at an offline fake model, e.g.
//! `cargo test -p aeroric-core --test omp_rpc_smoke -- --ignored --nocapture`.

use std::sync::Arc;
use std::time::Duration;

use aeroric_core::sessions::live::codex::rpc::Transport;
use aeroric_core::sessions::live::omp::driver::{
    OmpLauncher, OmpSession, OmpSessionSpec, OmpStart,
};
use aeroric_core::sessions::live::update::SessionUpdate;
use aeroric_core::sessions::live::{Decision, LiveStatusSink, SessionDriver, SharedLog};
use aeroric_core::tasks::{ProtocolFamily, TaskStatus};
use parking_lot::Mutex;

#[derive(Default)]
struct Sink {
    statuses: Mutex<Vec<TaskStatus>>,
    sessions: Mutex<Vec<(String, Option<String>)>>,
    updates: Mutex<Vec<String>>,
}

impl Sink {
    fn last_status(&self) -> Option<TaskStatus> {
        self.statuses.lock().last().copied()
    }
    fn update_types(&self) -> Vec<String> {
        self.updates.lock().clone()
    }
}

impl LiveStatusSink for Sink {
    fn status(&self, _: &str, _: &str, _: u64, status: TaskStatus, _: Option<String>) {
        self.statuses.lock().push(status);
    }
    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, id: &str, path: Option<&str>) {
        self.sessions
            .lock()
            .push((id.to_string(), path.map(str::to_string)));
    }
    fn update(&self, update: &SessionUpdate) {
        let wire = serde_json::to_value(update).unwrap();
        let mut kind = wire["type"].as_str().unwrap_or("").to_string();
        if kind == "unknown" {
            kind = format!("unknown:{}", wire["kind"].as_str().unwrap_or(""));
        }
        self.updates.lock().push(kind);
    }
}

async fn until(what: &str, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
#[ignore = "needs a real omp and an offline fake model"]
async fn a_real_omp_runs_a_turn_with_an_approval() {
    let Ok(home) = std::env::var("AERORIC_OMP_SMOKE_HOME") else {
        return;
    };
    let program = std::env::var("AERORIC_OMP_SMOKE_BIN").unwrap_or_else(|_| "omp".into());
    let cwd = std::env::temp_dir().join(format!("omp-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let launcher: OmpLauncher = Arc::new(move |start: &OmpStart| {
        let mut command = tokio::process::Command::new(&program);
        command
            .args(aeroric_core::agents::launch::omp_rpc_args(
                &start.permission,
                &start.cwd,
                Some("fake/fake-1"),
                None,
                start.resume.as_deref(),
            ))
            .env("PI_CODING_AGENT_DIR", &home)
            .env("HOME", &home)
            .env_remove("PI_PROFILE")
            .env_remove("OMP_PROFILE");
        Transport::spawn(command)
            .map(|(transport, guard)| (transport, Some(guard)))
            .map_err(|e| e.to_string())
    });
    let sink = Arc::new(Sink::default());
    let session = OmpSession::start(
        OmpSessionSpec {
            task_id: "smoke".into(),
            project_id: "p".into(),
            generation: 1,
            agent: "omp".into(),
            cwd: cwd.to_string_lossy().into_owned(),
            permission: "ask".into(),
            ..Default::default()
        },
        launcher,
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        SharedLog::new("smoke"),
        &tokio::runtime::Handle::current(),
    );
    session.prompt("run echo hi".into()).unwrap();
    let waiting = Arc::clone(&sink);
    until("approval", move || {
        waiting.last_status() == Some(TaskStatus::InputRequired)
    })
    .await;
    let pending = session.snapshot().pending;
    println!("pending: {pending:?}");
    assert_eq!(pending[0].kind, "bash");
    session
        .respond(&pending[0].id, Decision::AllowForSession)
        .unwrap();
    let done = Arc::clone(&sink);
    until("done", move || done.last_status() == Some(TaskStatus::Done)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("updates: {:?}", sink.update_types());
    println!("sessions: {:?}", sink.sessions.lock());
    for kind in [
        "turnStarted",
        "toolCall",
        "permissionRequest",
        "permissionResolved",
        "toolCallUpdate",
        "messageCompleted",
        "turnEnded",
    ] {
        assert!(
            sink.update_types().iter().any(|t| t == kind),
            "missing {kind}"
        );
    }
    for kind in sink.update_types() {
        assert!(
            !kind.starts_with("unknown:"),
            "an omp frame this build does not map: {kind}"
        );
    }
    assert!(session.snapshot().session_id.is_some());
    session.close().unwrap();
}
