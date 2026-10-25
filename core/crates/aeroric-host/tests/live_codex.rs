//! ADR-043 N2: a Codex task started through `TaskService` runs on the
//! structured driver — a scripted `codex app-server` (sh) receives the first
//! prompt as `turn/start`, the turn's end settles the task `done`, and the
//! thread id lands on the record as its Codex session. The PTY executor is
//! wrapped, not replaced: nothing reaches it for this run.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::projects::ProjectService;
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::sessions::live::executor::StructuredExecutor;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::{
    executor::HostWindowFacts, AgentFamilies, Attachments, NewTask, ProtocolFamily, TaskService,
};
use aeroric_core::terminal::pty::TerminalService;
use aeroric_host::executor::{CliPtyExecutor, RegistryLiveRuns};
use aeroric_host::live::{HostRouting, TaskServiceSink};

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

struct CodexFamilies;

impl AgentFamilies for CodexFamilies {
    fn launch_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Codex
    }
    fn session_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Codex
    }
    fn is_builtin(&self, _agent: &str) -> bool {
        false
    }
    fn native_resume_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Codex
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch_home() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-live-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.aeroric")).expect("home");
    root
}

/// A one-turn `codex app-server`: request ids are the client's 1, 2, 3.
fn write_fake_codex(root: &Path) -> PathBuf {
    let script = root.join("fake-codex.sh");
    let turn_file = root.join("turn.json");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
for a in "$@"; do last="$a"; done
if [ "$last" = "--version" ]; then echo "codex-cli 0.162.1"; exit 0; fi
[ "$last" = "app-server" ] || exit 2
read init
echo '{{"id":1,"result":{{}}}}'
read initialized
read start
echo '{{"id":2,"result":{{"thread":{{"id":"th-1","path":"/tmp/th-1.jsonl"}}}}}}'
read turn
echo "$turn" > '{turn}'
echo '{{"id":3,"result":{{"turn":{{"id":"u-1"}}}}}}'
echo '{{"method":"turn/started","params":{{"threadId":"th-1","turn":{{"id":"u-1"}}}}}}'
echo '{{"method":"item/agentMessage/delta","params":{{"threadId":"th-1","itemId":"m-1","delta":"ok"}}}}'
echo '{{"method":"turn/completed","params":{{"threadId":"th-1","turn":{{"id":"u-1","status":"completed"}}}}}}'
cat > /dev/null
"#,
            turn = turn_file.display()
        ),
    )
    .expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    script
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_codex_task_runs_its_first_turn_on_the_structured_driver() {
    let root = scratch_home();
    let script = write_fake_codex(&root);
    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "live-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
        )
        .expect("writer context"),
    );
    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec_pretty(&serde_json::json!({
            "custom_agents": [{ "id": "fake-codex", "path": script.to_string_lossy(), "args": [] }]
        }))
        .expect("settings"),
    )
    .expect("write settings");

    let registry = Arc::new(TaskRegistry::default());
    let log = Arc::new(EventLog::default());
    let pty = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::clone(&registry),
        Arc::new(TerminalService::new()),
        Arc::clone(&log),
    );
    let sink = TaskServiceSink::new(Arc::clone(&log));
    let structured = StructuredExecutor::new(
        pty.clone() as Arc<_>,
        HostRouting::new(Arc::clone(&context)),
        Arc::clone(&sink) as Arc<_>,
        tokio::runtime::Handle::current(),
    );
    let service = Arc::new(
        TaskService::new(
            Arc::clone(&context),
            Arc::new(CodexFamilies),
            structured.clone() as Arc<_>,
            Arc::new(RegistryLiveRuns::new(Arc::clone(&registry))),
        )
        .expect("task service"),
    );
    pty.attach(&service);
    sink.attach(&service);

    let project_dir = root.join("workspace");
    std::fs::create_dir_all(&project_dir).expect("workspace");
    let projects = ProjectService::new(Arc::clone(&context));
    let (upserted, _applied) = projects
        .open_local(
            &project_dir.to_string_lossy(),
            "project-1",
            context.clock.now_ms() as i64,
        )
        .expect("open project");
    let project = upserted.project;
    service
        .create(
            &project,
            NewTask {
                id: "task-1".to_string(),
                project_id: "project-1".to_string(),
                prompt: "hi".to_string(),
                agent: "fake-codex".to_string(),
                permission_mode: "ask".to_string(),
                immediate: true,
                selected_model: None,
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
                agent_config_id: None,
                images: Vec::new(),
                texts: Vec::new(),
            },
        )
        .expect("create task");
    service
        .start(
            &project,
            "task-1",
            "op-1",
            true,
            None,
            false,
            Attachments {
                images: Vec::new(),
                texts: Vec::new(),
            },
            HostWindowFacts::default(),
        )
        .expect("start task");

    let mut record = serde_json::Value::Null;
    for _ in 0..100 {
        let stamped = service.load("project-1").expect("load tasks");
        record = serde_json::to_value(&stamped.records.first().expect("task").0).expect("json");
        if record["status"] == "done" && record["codexSessionId"] == "th-1" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        record["status"], "done",
        "the turn's end settles the task: {record}"
    );
    assert_eq!(
        record["codexSessionId"], "th-1",
        "the thread is the task's Codex session: {record}"
    );

    let turn: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("turn.json")).expect("the fake codex saw turn/start"),
    )
    .expect("turn json");
    assert_eq!(turn["method"], "turn/start");
    assert_eq!(turn["params"]["input"][0]["text"], "hi");
    assert_eq!(turn["params"]["approvalPolicy"], "on-request");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
    assert!(
        registry.child_for("task-1").is_none(),
        "no PTY was opened for this run"
    );
    assert!(structured.registry().get("task-1").is_some());
}
