//! Task P2-04 acceptance, part 2: the retry and race half.
//!
//! Every test here is the same question in different clothes — "the same thing happened
//! twice; did the world only move once?" — because the client's answer was an object
//! identity comparison plus a callback that deletes itself, and neither of those survives a
//! second window, a timed-out request, or a process that restarts.

use std::sync::Arc;

use aeroric_core::tasks::executor::TaskEvent;
use aeroric_core::tasks::operations::{LedgerState, OperationKind};
use aeroric_core::tasks::service::TaskError;
use serde_json::json;

mod common;
use aeroric_core::tasks::service::Attachments;
use common::{build_service, cleanup, harness, local_project, record, task};

#[test]
fn the_same_attempt_retried_spawns_exactly_one_process() {
    let (root, context, executor) = harness("retry-once", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");

    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("the first attempt");
    let error = service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect_err("the retry of the same attempt");
    assert!(
        matches!(error, TaskError::AlreadyClaimed { kind: "start", .. }),
        "{error:?}"
    );
    assert_eq!(
        executor.launches().len(),
        1,
        "one attempt, one spawn — the second call never reached the executor"
    );
    assert_eq!(
        service.generation("t1"),
        1,
        "the retry cost nothing at all: no new run number was issued"
    );
    cleanup(root);
}

#[test]
fn a_refused_retry_costs_no_generation_so_a_stale_event_stays_stale() {
    // If a refused attempt left its reserved number behind, an event from the *first* run
    // could arrive looking like it belonged to the future, and the guard that drops late
    // exits would let it through.
    let (root, context, executor) = harness("generation", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("run 1");
    assert_eq!(service.generation("t1"), 1);
    for _ in 0..3 {
        assert!(matches!(
            service.start(
                &project,
                "t1",
                "op-1",
                true,
                None,
                false,
                Attachments::default()
            ),
            Err(TaskError::AlreadyClaimed { .. })
        ));
    }
    assert_eq!(
        service.generation("t1"),
        1,
        "three refusals, one generation"
    );
    let error = service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 2,
                code: Some(0),
            },
        )
        .expect_err("no run 2 was ever issued");
    assert!(
        matches!(
            error,
            TaskError::FutureGeneration {
                seen: 2,
                current: 1,
                ..
            }
        ),
        "{error:?}"
    );
    // A run the ledger never handed out is a wiring fault, so it is reported rather than
    // quietly dropped like an old one: swallowing it would hide an executor that invents
    // its own generation numbers.
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(executor.tasks_path()).expect("file"))
            .expect("array");
    assert_eq!(rows[0].get("status"), Some(&json!("pending")));
    cleanup(root);
}

#[test]
fn two_windows_starting_the_same_task_at_once_are_serialised_by_the_claim() {
    // Two `TaskService` values over one directory cannot both hold the lease, so this models
    // the only case that really happens: one process, two requests, second refused.
    let (root, context, executor) = harness("two-windows", &[task("t1", "p1", "todo")]);
    let service = Arc::new(build_service(context, &executor));
    let project = local_project("p1", "/repo");
    let mut handles = Vec::new();
    for _ in 0..4 {
        let service = Arc::clone(&service);
        let project = project.clone();
        handles.push(std::thread::spawn(move || {
            service.start(
                &project,
                "t1",
                "op-1",
                true,
                None,
                false,
                Attachments::default(),
            )
        }));
    }
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("thread"))
        .collect();
    let ok = results.iter().filter(|r| r.is_ok()).count();
    let refused = results
        .iter()
        .filter(|r| matches!(r, Err(TaskError::AlreadyClaimed { .. })))
        .count();
    assert_eq!(ok, 1, "exactly one of four concurrent requests proceeds");
    assert_eq!(
        refused, 3,
        "and the other three are told the attempt is in flight"
    );
    assert_eq!(executor.launches().len(), 1);
    cleanup(root);
}

#[test]
fn a_claim_survives_the_process_so_a_restart_cannot_double_spawn() {
    let (root, context, executor) = harness("restart", &[task("t1", "p1", "todo")]);
    let project = local_project("p1", "/repo");
    {
        let service = build_service(context, &executor);
        service
            .start(
                &project,
                "t1",
                "op-1",
                true,
                None,
                false,
                Attachments::default(),
            )
            .expect("started, still waiting for the terminal");
        assert!(!service.open_claims("t1").is_empty());
    }
    // A new process over the same directory: the ledger is on disk, so the open attempt is
    // still open and a retry is still a retry.
    let service = build_service(common::reacquire(&root), &executor);
    let error = service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect_err("the previous process never finished this attempt");
    assert!(
        matches!(error, TaskError::AlreadyClaimed { .. }),
        "{error:?}"
    );
    let state = aeroric_core::tasks::operations::load(service.context())
        .expect("ledger")
        .found()
        .expect("written");
    assert_eq!(
        state.claims.len(),
        1,
        "and the file holds one claim, not two"
    );
    cleanup(root);
}

#[test]
fn cancelling_reopens_the_task_for_a_genuine_new_attempt() {
    let (root, context, executor) = harness("cancel-then-start", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("run 1");
    assert!(matches!(
        service.start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default()
        ),
        Err(TaskError::AlreadyClaimed { .. })
    ));
    service.cancel(&project, "t1").expect("cancelled");
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("run 2 is a new intent, not a retry");
    assert_eq!(executor.launches().len(), 2);
    assert_eq!(service.generation("t1"), 2);
    cleanup(root);
}

#[test]
fn cancel_and_complete_are_not_the_same_request() {
    // Both stop a process; only one of them is the user saying "it's finished". The state
    // machine must not confuse a stop request with a settle.
    let (root, context, executor) = harness("cancel-vs-complete", &[task("t1", "p1", "running")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 0,
                status: aeroric_protocol::task::TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
            },
        )
        .ok();
    service.cancel(&project, "t1").expect("cancel");
    assert_eq!(
        executor.call_log(),
        vec!["cancel:t1".to_string()],
        "cancel asks the runtime to stop and writes no status at all"
    );
    let tasks_file = executor.tasks_path();
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&tasks_file).expect("file")).expect("array");
    assert_eq!(rows[0].get("status"), Some(&json!("running")));
    service.complete(&project, "t1").expect("complete");
    assert_eq!(executor.call_log().last().unwrap(), "complete:t1");
    cleanup(root);
}

#[test]
fn a_late_terminal_ready_from_an_older_run_settles_nothing() {
    // The first-input gate is a one-shot per attempt. An old ready event that cleared the
    // current attempt's claim would let a duplicate spawn through the door it just opened.
    let (root, context, executor) = harness("late-ready", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("run 1");
    service.cancel(&project, "t1").expect("cancel 1");
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("run 2");
    assert_eq!(service.open_claims("t1").len(), 1);
    service
        .observe(
            "p1",
            TaskEvent::TerminalReady {
                task_id: "t1".into(),
                generation: 1,
            },
        )
        .expect("dropped as stale");
    assert_eq!(
        service.open_claims("t1").len(),
        1,
        "run 1's ready edge may not release run 2's claim"
    );
    service
        .observe(
            "p1",
            TaskEvent::TerminalReady {
                task_id: "t1".into(),
                generation: 2,
            },
        )
        .expect("current run is ready");
    assert!(service.open_claims("t1").is_empty());
    cleanup(root);
}

#[test]
fn the_record_copy_is_a_mirror_and_never_the_authority() {
    // The copy exists so an exported or synced project still shows which run it was on
    // (ADR-016). It must not be able to *decide* anything: a row that arrives claiming
    // generation 7, in a directory whose ledger has never seen the task, still gets run 0,
    // and an event for the run the copy advertises is refused rather than believed.
    let row = json!({
        "id": "t1", "projectId": "p1", "prompt": "go", "agent": "claude",
        "status": "todo", "permissionMode": "ask", "createdAt": 1i64, "generation": "7"
    });
    let (root, context, executor) = harness("generation-copy", &[record(row)]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    assert_eq!(
        service.generation("t1"),
        0,
        "the ledger has never seen this task, so no run has happened here"
    );
    let error = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 7,
                status: aeroric_protocol::task::TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
            },
        )
        .expect_err("an event for a run this process never issued is a wiring fault");
    assert!(
        matches!(
            error,
            TaskError::FutureGeneration {
                seen: 7,
                current: 0,
                ..
            }
        ),
        "{error:?}"
    );
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("run 0 is ours to start");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(executor.tasks_path()).expect("file"))
            .expect("array");
    assert_eq!(
        rows[0].get("generation"),
        Some(&json!("1")),
        "the lying copy was rewritten to the authority, not negotiated with"
    );
    cleanup(root);
}

#[test]
fn an_empty_ledger_file_is_written_once_and_read_back_as_empty() {
    // A project with no in-flight attempts still has to be distinguishable from "the ledger
    // is unreadable", because the second answer forbids every spawn in the directory.
    let (root, context, executor) = harness("empty-ledger", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("started");
    service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 1,
                code: Some(0),
            },
        )
        .expect("settled");
    let state = aeroric_core::tasks::operations::load(service.context())
        .expect("readable")
        .found()
        .expect("the ledger file exists once anything has been claimed");
    assert!(state.claims.is_empty(), "the attempt settled");
    assert_eq!(state.generations.get("t1"), Some(&1));
    // And the file is the core-exclusive one, not a field inside the client's own files.
    assert!(service.context().paths.task_operations_file().exists());
    let bytes =
        std::fs::read_to_string(service.context().paths.runtime_metadata_file()).expect("meta");
    assert!(
        !bytes.contains("task-operations"),
        "runtime.json stays a counter, not a copy of the ledger"
    );
    let settings = std::fs::read_to_string(service.context().paths.settings_file());
    assert!(
        settings.is_err() || !settings.unwrap().contains("claims"),
        "the ledger never lands in the client's settings file"
    );
    let _ = LedgerState::default();
    let _ = OperationKind::Start;
    cleanup(root);
}

#[test]
fn two_distinct_operation_keys_on_one_task_both_reach_the_executor() {
    // ADR-017: the retry identity is the caller's operation key, not core's run counter, so
    // "same attempt twice" and "two attempts of the same task" are different questions with
    // different answers. What stays true under both is that each run carries its own
    // generation and only its own events move it.
    let (root, context, executor) = harness("double-open", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    service
        .start(
            &project,
            "t1",
            "op-a",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("first attempt");
    service
        .start(
            &project,
            "t1",
            "op-b",
            true,
            None,
            false,
            Attachments::default(),
        )
        .expect("a second, deliberately different attempt");
    assert_eq!(executor.launches().len(), 2, "two windows, two processes");
    assert_eq!(
        service.generation("t1"),
        2,
        "and each got its own run number"
    );
    assert_eq!(service.open_claims("t1").len(), 2, "both are in flight");

    // One of them ending must not be read as the task ending.
    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 2,
                status: aeroric_protocol::task::TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
            },
        )
        .expect("run 2 reports");
    assert_eq!(
        service.open_claims("t1").len(),
        2,
        "run 2 becoming running does not retire run 1's claim"
    );
    service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 2,
                code: Some(0),
            },
        )
        .expect("run 2 exits");
    assert_eq!(
        service.open_claims("t1").len(),
        1,
        "run 2's claim settled; run 1's is still open and still owns its own events"
    );
    let read = || -> serde_json::Value {
        serde_json::from_str::<Vec<serde_json::Value>>(
            &std::fs::read_to_string(executor.tasks_path()).expect("file"),
        )
        .expect("array")[0]
            .clone()
    };
    assert_eq!(
        read().get("status"),
        Some(&json!("running")),
        "ADR-017: a task row carries one status, so while run 1 is live run 2 ending may not \
         claim the task is finished"
    );
    assert_eq!(
        read().get("completedAt"),
        None,
        "and it must not stamp an end time the task has not reached"
    );

    // The last run ending is the task ending.
    service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 1,
                code: Some(1),
            },
        )
        .expect("run 1 exits");
    assert_eq!(read().get("status"), Some(&json!("failed")));
    assert!(
        read().get("completedAt").is_some(),
        "now the task really has ended"
    );
    assert!(
        read().get("failureReason").is_some(),
        "the last run's reason becomes the task's reason: {read}",
        read = read()
    );
    assert!(service.open_claims("t1").is_empty(), "both claims released");
    cleanup(root);
}
