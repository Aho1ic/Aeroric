//! 切片一百零五：宿主 task 通知接 EventLog 的端到端判据。fake agent 跑到
//! done 后，`events.since` 必须交付 "task-status"，且载荷与 Embedded 窗口
//! （TauriEventSink→core `window_payload`）逐字节同形——桌面事件泵把这条
//! 原样透传给前端（切片一百零四的透传路径）。泵落地前宿主 wired 路径不公告，
//! External 模式的任务状态到不了前端。

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
use aeroric_host::executor::{CliPtyExecutor, RegistryLiveRuns};
use serde_json::{json, Value};

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_settled_task_announces_task_status_to_the_event_log() {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-taskev-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("home/.aeroric")).expect("home");
    let script = root.join("fake-agent.sh");
    std::fs::write(&script, "#!/bin/sh\necho HELLO\nexit 0\n").expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let paths = AppPaths::from_user_home(root.join("home"));
    let context = Arc::new(
        CoreContext::acquire(
            paths.clone(),
            Arc::new(aeroric_core::clock::SystemClock),
            &LeaseOwner {
                application: "taskev-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
        )
        .expect("writer context"),
    );
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
    assert_eq!(status, "done", "fake agent 必须经 observe 结算为 done");

    // EventLog 交付 "task-status"，载荷与 Embedded 窗口同形（core
    // `window_payload`）：exit 结算的 done 公告在（snake_case task_id、
    // failure_reason 只在 failed 出现、无诊断键）。范围如实留档：start 的
    // running 公告在宿主 task 服务装配面（本切片不扩），调用方响应本身
    // 已带起点状态。
    // 公告由 exit 线程在状态落盘之后追加(observe 与 EventLog 是两个动作),
    // 所以单次快照会**与写入竞态**:实测同一二进制 5 次里 4 次读到空日志。
    // 机器判据必须是「5 s 内到达」而不是「此刻已在」,上限与上面状态轮询同款。
    let (mut events, mut reset) = (Vec::new(), false);
    for _ in 0..100 {
        let (snapshot, _latest, saw_reset) = log.since(0);
        reset = saw_reset;
        if snapshot.iter().any(|event| event.event == "task-status") {
            events = snapshot;
            break;
        }
        events = snapshot;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(!reset);
    let statuses: Vec<Value> = events
        .iter()
        .filter(|event| event.event == "task-status")
        .map(|event| event.data.clone())
        .collect();
    assert!(
        statuses
            .iter()
            .any(|payload| payload == &json!({"task_id": "task-1", "status": "done"})),
        "exit 公告缺失: {statuses:?}"
    );
    assert!(
        statuses
            .iter()
            .all(|payload| payload.get("serviceUnwired").is_none()),
        "wired 路径不得走诊断形状: {statuses:?}"
    );

    drop(service);
    drop(context);
    std::fs::remove_dir_all(&root).ok();
}
