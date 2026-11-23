use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{Admit, CommandLedger, KEEP_MS, STALE, STALE_TASK};
use crate::remote::auth::AuthOutcome;
use crate::remote::server::dispatch_once;
use crate::remote::services::{RemoteHostHandle, RemoteTerminalStreams};
use crate::remote::state::{OutboundSender, RemoteState};

fn cell(admit: Admit) -> Arc<tokio::sync::OnceCell<Result<Value, String>>> {
    match admit {
        Admit::Once(c) => c,
        Admit::Run => panic!("ran as before"),
        Admit::Refuse(why) => panic!("refused: {why}"),
    }
}

#[test]
fn a_command_id_names_one_command_per_device() {
    let ledger = CommandLedger::default();
    let cancel = json!({"projectId": "p1", "taskId": "t1", "commandId": "c1", "hostEpoch": "e1"});
    assert!(
        matches!(
            ledger.admit("d1", "tasks.list", &json!({"commandId": "x"}), "e1", 1),
            Admit::Run
        ),
        "reads pass"
    );
    assert!(
        matches!(
            ledger.admit("d1", "task.cancel", &json!({"taskId": "t1"}), "e1", 1),
            Admit::Run
        ),
        "no id: as before"
    );

    let first = cell(ledger.admit("d1", "task.cancel", &cancel, "e1", 1));
    let again = cell(ledger.admit("d1", "task.cancel", &cancel, "e1", 2));
    assert!(Arc::ptr_eq(&first, &again), "the same command");
    let other_device = cell(ledger.admit("d2", "task.cancel", &cancel, "e1", 2));
    assert!(!Arc::ptr_eq(&first, &other_device));
    let changed = json!({"projectId": "p1", "taskId": "t2", "commandId": "c1"});
    assert!(
        matches!(ledger.admit("d1", "task.cancel", &changed, "e1", 3), Admit::Refuse(w) if w.contains("另一条命令"))
    );
    assert!(
        matches!(ledger.admit("d1", "task.cancel", &cancel, "e0", 3), Admit::Refuse(w) if w.starts_with(STALE))
    );
    let bad = json!({"commandId": "x".repeat(200)});
    assert!(matches!(
        ledger.admit("d1", "task.cancel", &bad, "e1", 3),
        Admit::Refuse(_)
    ));

    let later = cell(ledger.admit("d1", "task.cancel", &cancel, "e1", 2 + KEEP_MS));
    assert!(!Arc::ptr_eq(&first, &later), "kept for a while only");
    assert_eq!(ledger.kept("d1"), 1);
    ledger.forget_device("d1");
    assert_eq!(ledger.kept("d1"), 0);
}

/// Counts what reaches the Mac; cancelling takes a moment.
#[derive(Default)]
struct Counting {
    cancels: AtomicUsize,
    /// What `project.writeFile` reached the Mac, and what was audited.
    writes: AtomicUsize,
    audits: parking_lot::Mutex<Vec<String>>,
    /// D4 step 6: what `load_project_tasks` answers.
    tasks: parking_lot::Mutex<Value>,
}

#[async_trait]
impl RemoteHostHandle for Counting {
    fn desktop_version(&self) -> String {
        "test".into()
    }
    fn emit(&self, _: &str, _: Value) {}
    fn audit_log(&self, event: &str, _: Value) {
        self.audits.lock().push(event.to_string());
    }
    async fn load_projects(&self) -> Result<Value, String> {
        Ok(json!([]))
    }
    async fn set_project_pinned(&self, _: &str, _: bool) -> Result<(), String> {
        Ok(())
    }
    async fn load_project_tasks(&self, _: &str) -> Result<Value, String> {
        let tasks = self.tasks.lock().clone();
        Ok(if tasks.is_null() { json!([]) } else { tasks })
    }
    async fn task_input(&self, _: &str, _: &str) -> Result<Value, String> {
        Err("no".into())
    }
    async fn task_respond(&self, _: &str, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn task_cancel(&self, _: &str, task_id: &str) -> Result<Value, String> {
        let n = self.cancels.fetch_add(1, Ordering::SeqCst) + 1;
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(json!({"taskId": task_id, "status": "cancelled", "n": n}))
    }
    async fn task_complete(&self, _: &str, _: &str) -> Result<Value, String> {
        Err("no".into())
    }
    async fn task_request(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn session_messages(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn project_files(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn project_read_file(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn project_write_file(&self, _: Value) -> Result<Value, String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"available": true, "ok": true}))
    }
    async fn git_changes(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn git_diff(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn agent_config_list(&self) -> Result<Value, String> {
        Err("no".into())
    }
    async fn agent_config_save(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn agent_config_detect_models(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn agent_config_create(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn agents_list(&self) -> Result<Value, String> {
        Err("no".into())
    }
    async fn agents_models(&self, _: &str) -> Result<Value, String> {
        Err("no".into())
    }
    async fn stats_summary(&self) -> Result<Value, String> {
        Err("no".into())
    }
    fn terminal_streams(&self, _: OutboundSender) -> Box<dyn RemoteTerminalStreams> {
        struct None_;
        impl RemoteTerminalStreams for None_ {
            fn handle_frame(&mut self, _: &[u8]) {}
            fn abort_all(&mut self) {}
        }
        Box::new(None_)
    }
    async fn push_register(&self, _: Value) -> Result<Value, String> {
        Err("no".into())
    }
    async fn push_status(&self) -> Result<Value, String> {
        Err("no".into())
    }
    async fn push_test(&self) -> Result<Value, String> {
        Err("no".into())
    }
    async fn relay_diagnostics(&self) -> Result<Value, String> {
        Err("no".into())
    }
    async fn device_revoked(&self, _: &str) -> Result<Value, String> {
        Ok(json!({}))
    }
}

fn paired(state: &RemoteState) -> String {
    let mut auth = state.auth.lock();
    let invite = auth.create_invite().unwrap();
    match auth
        .begin_pairing("127.0.0.1".parse().unwrap(), &invite, Some("手机"))
        .unwrap()
    {
        AuthOutcome::Paired { device_id, .. } => device_id,
        AuthOutcome::ProvisionalPaired {
            device_id,
            pairing_id,
            ..
        } => {
            auth.confirm_pairing(&pairing_id).unwrap();
            device_id
        }
        _ => panic!("not paired"),
    }
}

/// Sent three times, two at once: it reaches the Mac once and every send
/// gets that result. Based on another host start: refused. From a revoked
/// device: refused, its results gone.
#[tokio::test]
async fn a_command_sent_again_reaches_the_mac_once() {
    let state = RemoteState::new_in_memory();
    let mac = Counting::default();
    let device = paired(&state);
    let epoch = state.host_epoch.clone();
    let cancel = json!({"projectId": "p1", "taskId": "t1", "commandId": "c1", "hostEpoch": epoch});

    let (a, b) = tokio::join!(
        dispatch_once(&state, &mac, &device, "task.cancel", cancel.clone()),
        dispatch_once(&state, &mac, &device, "task.cancel", cancel.clone()),
    );
    let c = dispatch_once(&state, &mac, &device, "task.cancel", cancel.clone()).await;
    assert_eq!(mac.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(a.clone().unwrap()["n"], 1);
    assert_eq!(a, b);
    assert_eq!(a, c);

    let stale = json!({"projectId": "p1", "taskId": "t1", "commandId": "c2", "hostEpoch": "the-last-start"});
    let refused = dispatch_once(&state, &mac, &device, "task.cancel", stale)
        .await
        .unwrap_err();
    assert!(refused.starts_with(STALE), "{refused}");
    assert_eq!(mac.cancels.load(Ordering::SeqCst), 1);

    // A different command, a new id: it runs.
    let next = json!({"projectId": "p1", "taskId": "t2", "commandId": "c3", "hostEpoch": epoch});
    dispatch_once(&state, &mac, &device, "task.cancel", next)
        .await
        .unwrap();
    assert_eq!(mac.cancels.load(Ordering::SeqCst), 2);

    state.auth.lock().revoke(&device).unwrap();
    state.commands.forget_device(&device);
    let gone = dispatch_once(&state, &mac, &device, "task.cancel", cancel)
        .await
        .unwrap_err();
    assert!(gone.contains("撤销"), "{gone}");
    assert_eq!(mac.cancels.load(Ordering::SeqCst), 2);
    assert_eq!(state.commands.kept(&device), 0);
}

#[tokio::test]
async fn hello_says_which_host_start_this_is() {
    let state = RemoteState::new_in_memory();
    let other = RemoteState::new_in_memory();
    assert_ne!(state.host_epoch, other.host_epoch, "a new one each start");
    let hello = crate::remote::rpc::dispatch(&state, &Counting::default(), "hello", json!({}))
        .await
        .unwrap();
    assert_eq!(hello["hostEpoch"], state.host_epoch.as_str());
    assert!(hello["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("commands.idempotent")));
}

/// D4 step 6 (2026-11-23): a command names the run of its task it was
/// decided on. Matching: run. Run again since: refused, nothing reaches the
/// Mac. The same `commandId` resent after the task ran again: the first
/// result. The task gone: refused.
#[tokio::test]
async fn a_command_based_on_an_earlier_run_of_its_task_is_refused() {
    let state = RemoteState::new_in_memory();
    let mac = Counting::default();
    *mac.tasks.lock() = json!([{"id": "t1", "projectId": "p1", "generation": "3"}]);
    let device = paired(&state);
    let epoch = state.host_epoch.clone();
    let based = |id: &str, generation: Value| json!({"projectId": "p1", "taskId": "t1", "commandId": id, "hostEpoch": epoch, "basedOnGeneration": generation});

    let ran = dispatch_once(
        &state,
        &mac,
        &device,
        "task.cancel",
        based("c1", json!("3")),
    )
    .await
    .unwrap();
    assert_eq!(ran["n"], 1);

    let stale = dispatch_once(&state, &mac, &device, "task.cancel", based("c2", json!(2)))
        .await
        .unwrap_err();
    assert!(stale.starts_with(STALE_TASK), "{stale}");
    assert!(
        stale.contains("第 3 次") && stale.contains("第 2 次"),
        "{stale}"
    );
    assert_eq!(
        mac.cancels.load(Ordering::SeqCst),
        1,
        "a refused command never runs"
    );

    *mac.tasks.lock() = json!([{"id": "t1", "projectId": "p1", "generation": "4"}]);
    let again = dispatch_once(
        &state,
        &mac,
        &device,
        "task.cancel",
        based("c1", json!("3")),
    )
    .await
    .unwrap();
    assert_eq!(again, ran, "a resend keeps the first verdict");

    *mac.tasks.lock() = json!([]);
    let gone = dispatch_once(
        &state,
        &mac,
        &device,
        "task.cancel",
        based("c3", json!("4")),
    )
    .await
    .unwrap_err();
    assert!(
        gone.starts_with(STALE_TASK) && gone.contains("不在了"),
        "{gone}"
    );

    // Older records have no generation: the command passes.
    *mac.tasks.lock() = json!([{"id": "t1", "projectId": "p1"}]);
    assert!(dispatch_once(
        &state,
        &mac,
        &device,
        "task.cancel",
        based("c4", json!("9"))
    )
    .await
    .is_ok());
}

/// backlog §4.4-8 (2026-11-23): a paired device cannot write a project file
/// unless this Mac turned remote file writes on; the refusal is audited and
/// nothing reaches the handle. Reading stays open.
#[tokio::test]
async fn remote_file_writes_are_off_until_the_mac_turns_them_on() {
    let state = RemoteState::new_in_memory();
    let mac = Counting::default();
    let write = json!({"projectId": "p1", "path": "/demo/a.txt", "content": "x"});
    let refused = crate::remote::rpc::dispatch(&state, &mac, "project.writeFile", write.clone())
        .await
        .unwrap_err();
    assert!(
        refused.starts_with(crate::remote::rpc::FILE_WRITES_OFF),
        "{refused}"
    );
    assert_eq!(mac.writes.load(Ordering::SeqCst), 0);
    assert_eq!(*mac.audits.lock(), ["remote-file-write-refused"]);
    assert!(
        !crate::remote::config::RemoteConfig::default().allow_file_writes,
        "off by default"
    );
    let old: crate::remote::config::RemoteConfig =
        serde_json::from_value(json!({"enabled": true, "port": 6790})).unwrap();
    assert!(!old.allow_file_writes, "a config written before stays off");

    let told = crate::remote::rpc::rpc_capabilities(&state);
    assert!(
        !told.as_array().unwrap().contains(&json!("files.write")),
        "not offered while off"
    );
    assert!(told.as_array().unwrap().contains(&json!("tasks.interrupt")));

    state.config.lock().allow_file_writes = true;
    crate::remote::rpc::dispatch(&state, &mac, "project.writeFile", write)
        .await
        .unwrap();
    assert_eq!(mac.writes.load(Ordering::SeqCst), 1);
    assert!(crate::remote::rpc::rpc_capabilities(&state)
        .as_array()
        .unwrap()
        .contains(&json!("files.write")));
}

/// D4 (2026-11-23): `task.interrupt` is a command (it changes the Mac), and
/// a host that cannot interrupt says so.
#[tokio::test]
async fn interrupting_a_task_is_a_command_the_host_may_refuse() {
    assert!(super::COMMANDS.contains(&"task.interrupt"));
    let state = RemoteState::new_in_memory();
    let refused = crate::remote::rpc::dispatch(
        &state,
        &Counting::default(),
        "task.interrupt",
        json!({"projectId": "p1", "taskId": "t1"}),
    )
    .await
    .unwrap_err();
    assert!(refused.contains("cannot interrupt"), "{refused}");
    assert!(crate::remote::rpc::dispatch(
        &state,
        &Counting::default(),
        "task.interrupt",
        json!({"projectId": "p1"})
    )
    .await
    .is_err());
}
