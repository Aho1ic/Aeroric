//! `CliPtyExecutor` 的 fake-agent 打通判据（P3-07 b1.1：`run` 到 settled 的
//! 执行体本体；补记二百九十一实现清单第 ⑤ 条）。
//! 全链走 `TaskService`（create→start→executor.launch→真 PTY→exit→observe），
//! 断言三件：状态到 done、字节环有输出、句柄账退场。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::projects::ProjectService;
use aeroric_core::runtime::registry::TaskRegistry;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_core::tasks::{
    executor::HostWindowFacts, AgentFamilies, Attachments, NewTask, ProtocolFamily, TaskService,
};
use aeroric_core::terminal::pty::TerminalService;
use aeroric_core::terminal::stream::TerminalIdentity;
use aeroric_host::executor::{CliPtyExecutor, RegistryLiveRuns};

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

/// fake agent 的 family：claude 形状（permission args 对 sh 脚本是惰性参数）。
struct FakeFamilies;

impl AgentFamilies for FakeFamilies {
    fn launch_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Claude
    }
    fn session_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Claude
    }
    fn is_builtin(&self, _agent: &str) -> bool {
        false
    }
    fn native_resume_family(&self, _agent: &str) -> ProtocolFamily {
        ProtocolFamily::Claude
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch_home() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-exec-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.aeroric")).expect("home");
    root
}

fn write_fake_agent(root: &Path) -> PathBuf {
    let script = root.join("fake-agent.sh");
    std::fs::write(&script, "#!/bin/sh\necho HELLO-FROM-FAKE-AGENT\nexit 0\n").expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    script
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fake_agent_launch_settles_done_and_retires_its_handles() {
    let root = scratch_home();
    let script = write_fake_agent(&root);
    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "exec-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
        )
        .expect("writer context"),
    );
    // settings.yaml：一个 custom agent 指向 fake 脚本。
    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec_pretty(&serde_json::json!({
            "custom_agents": [{ "id": "fake", "path": script.to_string_lossy(), "args": [] }]
        }))
        .expect("settings"),
    )
    .expect("write settings");

    let registry = Arc::new(TaskRegistry::default());
    let terminal = Arc::new(TerminalService::new());
    let log = Arc::new(EventLog::default());
    let executor = CliPtyExecutor::new(
        Arc::clone(&context),
        Arc::clone(&registry),
        Arc::clone(&terminal),
        Arc::clone(&log),
    );
    let service = Arc::new(
        TaskService::new(
            Arc::clone(&context),
            Arc::new(FakeFamilies),
            executor.clone() as Arc<_>,
            Arc::new(RegistryLiveRuns::new(Arc::clone(&registry))),
        )
        .expect("task service"),
    );
    executor.attach(&service);

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
                agent: "fake".to_string(),
                permission_mode: "ask".to_string(),
                immediate: true,
                selected_model: None,
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
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
            HostWindowFacts {
                terminal_size: Some((80, 24)),
                dsh: None,
            },
        )
        .expect("start task");

    // 轮询到 done（fake 脚本秒退；5 s 上限防永挂）。
    let mut status = String::new();
    for _ in 0..100 {
        let stamped = service.load("project-1").expect("load tasks");
        status = stamped
            .records
            .first()
            .and_then(|record| record.0.get("status"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        if status == "done" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        status, "done",
        "fake agent 的退出必须经 observe 结算为 done"
    );

    // 字节环里有脚本的输出（attach/events 的读点就是它）。
    let identity = TerminalIdentity::new("task-1", 1);
    let chunk = terminal
        .read_from(&identity, 0, 4096)
        .expect("ring must still hold the run");
    let text = String::from_utf8_lossy(&chunk.bytes).to_string();
    assert!(
        text.contains("HELLO-FROM-FAKE-AGENT"),
        "字节环没收到脚本输出: {text:?}"
    );

    // 句柄账退场：child 表与 runs 台账都不再认这条任务。
    assert!(
        registry.child_for("task-1").is_none(),
        "exit 后 child 句柄必须退场"
    );
    assert!(
        registry.runs.live_generations("task-1").is_empty(),
        "exit 后 runs 台账必须 retire"
    );

    drop(service);
    drop(context);
    std::fs::remove_dir_all(&root).ok();
}
