//! Task P2-04 acceptance, part 1: the state machine, the ordering of a launch, and the
//! two content-fingerprint counterexamples P2-02 handed down.
//!
//! The claims being tested are all about *sequence*, which is why every one of them reads
//! the fake executor's log or re-reads the file from inside the call. "It returned Ok" is
//! not evidence that the record reached the disk before the process did.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::storage::atomic::StorageError;
use aeroric_core::storage::{runtime, Cover, Fingerprint};
use aeroric_core::tasks::executor::{HostWindowFacts, ObservationOrigin, TaskEvent};
use aeroric_core::tasks::service::{
    Attachments, EventOutcome, NewTask, SessionSnapshot, TaskError, TaskIntentKind,
    TASK_INTENT_VOCABULARY,
};
use aeroric_core::tasks::{
    launch_plan, operations, reducer, session_owner, NoLiveRuns, TaskIntent, TaskNotification,
};
use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use serde_json::{json, Value};

mod common;
use common::{
    build_service, build_service_with_liveness, cleanup, harness, local_project, reader, record,
    task, Families, StubLiveness,
};

/// The harness puts the managed data directory at `root/managed`, exactly the way
/// `AppPaths::with_data_dir` does. Spelled once here so a test never re-derives the layout
/// (and got it wrong the first time, which is the argument for naming it).
fn tasks_json(root: &Path) -> PathBuf {
    root.join("managed")
        .join("projects")
        .join("p1")
        .join("tasks.json")
}

fn rows(tasks_file: &PathBuf) -> Vec<Value> {
    std::fs::read_to_string(tasks_file)
        .map(|text| serde_json::from_str(&text).expect("tasks.json is an array"))
        .unwrap_or_default()
}

#[test]
fn the_whole_nine_by_nine_table_has_exactly_one_ignored_transition() {
    // The client's guard is one clause long. Pinning "and only that one" is the point: a
    // second clause added here would silently freeze real state changes, and a dropped one
    // would let a late hook drag a detached task back to running.
    let all = [
        TaskStatus::Todo,
        TaskStatus::Pending,
        TaskStatus::Running,
        TaskStatus::InputRequired,
        TaskStatus::Detached,
        TaskStatus::Interrupted,
        TaskStatus::Done,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ];
    let mut ignored = Vec::new();
    for from in all {
        for to in all {
            let records = vec![record(json!({"id": "t", "status": from}))];
            let outcome = reducer::apply_status_transition(&records, "t", to, None, None, 1_000);
            let effective = if outcome.changed {
                match outcome.task.as_ref().and_then(reducer::status_of) {
                    Some(seen) => seen,
                    None => from,
                }
            } else {
                from
            };
            if effective != to {
                ignored.push((from, to, effective));
            }
        }
    }
    assert_eq!(
        ignored,
        vec![
            (
                TaskStatus::Detached,
                TaskStatus::Running,
                TaskStatus::Detached
            ),
            (
                TaskStatus::Detached,
                TaskStatus::InputRequired,
                TaskStatus::Detached
            ),
        ],
        "every other pair must land on the status it was told to"
    );
}

#[test]
fn a_terminal_entry_stamps_once_and_a_second_terminal_event_keeps_the_first_time() {
    let (root, context, executor) = harness("terminal-once", &[task("t1", "p1", "running")]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);

    service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 0,
                code: Some(0),
            },
        )
        .expect("settled");
    let first = rows(&tasks_file);
    let completed_at = first[0].get("completedAt").expect("stamped").clone();

    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 0,
                status: TaskStatus::Cancelled,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Runtime,
            },
        )
        .expect("cancelled");
    let second = rows(&tasks_file);
    assert_eq!(second[0].get("status"), Some(&json!("cancelled")));
    assert_eq!(
        second[0].get("completedAt"),
        Some(&completed_at),
        "the end time belongs to the event that first ended it"
    );
    cleanup(root);
}

#[test]
fn a_record_is_on_disk_before_the_executor_is_asked_to_spawn() {
    // The ordering rule in one assertion: at the moment `launch` was called, `tasks.json`
    // already held the task in `pending`. If a spawn ever moves ahead of the save, the
    // process exists with nothing describing it — and a crash cannot be reconciled.
    let (root, context, executor) = harness("persist-first", &[task("t1", "p1", "todo")]);
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
            HostWindowFacts::default(),
        )
        .expect("started");
    assert_eq!(executor.call_log(), vec!["launch:t1".to_string()]);
    let at_call = executor
        .disk_at_call(0)
        .expect("the file existed at the call");
    let rows = at_call.as_array().expect("an array");
    assert_eq!(rows[0]["status"], json!("pending"));
    cleanup(root);
}

#[test]
fn a_persist_that_refuses_starts_no_process_at_all() {
    // The counterexample P2-02 required: the legacy client wrote the file after we read it.
    let (root, context, executor) = harness("legacy-first", &[task("t1", "p1", "todo")]);
    let tasks_file = tasks_json(&root);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");

    // The baseline is the last byte this core published, so the sequence under test is
    // "core wrote, then the legacy client wrote". Core's very first write over a file it has
    // never published adopts what is there — see
    // `a_file_core_has_never_published_is_adopted_rather_than_refused_forever`.
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("core's first write");
    // Close the first attempt the way a live host would: the terminal came up, so the
    // attempt is over and a later start is genuinely new. Without this the second call is
    // refused as an in-flight duplicate — a different guarantee, pinned in
    // tests/task_idempotency.rs — and the cover check is never reached.
    service
        .observe(
            "p1",
            TaskEvent::TerminalReady {
                task_id: "t1".into(),
                generation: 1,
            },
        )
        .expect("attempt 1 settled");
    let foreign = rows(&tasks_file);
    let mut edited = foreign.clone();
    edited[0]["prompt"] = json!("edited elsewhere");
    std::fs::write(&tasks_file, serde_json::to_vec_pretty(&edited).unwrap())
        .expect("foreign write");

    let error = service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect_err("the cover is gone");
    assert!(
        matches!(error, TaskError::PersistFailed(_)),
        "{error:?} — a stale cover is a refusal, not a guess"
    );
    assert_eq!(
        executor.call_log(),
        vec!["launch:t1".to_string()],
        "the first write spawned once; the refused one spawned nothing more"
    );
    assert_eq!(
        rows(&tasks_file),
        edited,
        "the foreign content is left exactly as found"
    );
    cleanup(root);
}

#[test]
fn content_hand_back_by_hand_is_a_conflict_rather_than_a_replay() {
    // The second counterexample: a human restores an older copy of the file. A revision
    // number alone cannot see this — the counter is still ahead and the bytes are not ours.
    let (root, context, executor) = harness("restored", &[task("t1", "p1", "todo")]);
    let tasks_file = tasks_json(&root);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");

    let before = std::fs::read(&tasks_file).expect("original bytes");
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("first start");
    // Close the first attempt the way a live host would: the terminal came up, so the
    // attempt is over and a later start is genuinely new. Without this the second call is
    // refused as an in-flight duplicate — a different guarantee, pinned in
    // tests/task_idempotency.rs — and the cover check is never reached.
    service
        .observe(
            "p1",
            TaskEvent::TerminalReady {
                task_id: "t1".into(),
                generation: 1,
            },
        )
        .expect("attempt 1 settled");
    let after = std::fs::read(&tasks_file).expect("written bytes");
    assert_ne!(before, after, "the start really changed the file");

    std::fs::write(&tasks_file, &before).expect("a human restored the old copy");
    let error = service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect_err("the restored bytes are not what this writer last published");
    assert!(matches!(error, TaskError::PersistFailed(_)), "{error:?}");
    assert_eq!(
        std::fs::read(&tasks_file).expect("still there"),
        before,
        "and the refusal must not overwrite what the user just restored"
    );
    cleanup(root);
}

#[test]
fn the_fingerprint_is_published_with_the_revision_and_reports_itself_as_stale_write() {
    let (root, context, executor) = harness("published", &[task("t1", "p1", "todo")]);
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
            HostWindowFacts::default(),
        )
        .expect("started");

    let key = "projects/p1/tasks.json".to_string();
    let meta = runtime::load(service.context())
        .expect("metadata")
        .found()
        .expect("runtime.json exists");
    assert_eq!(meta.schema_version, runtime::RUNTIME_SCHEMA_VERSION);
    assert_eq!(meta.revision, "1", "one real change, one step");
    assert!(
        meta.content_fingerprints.contains_key(&key),
        "{:?}",
        meta.content_fingerprints.keys()
    );
    let recorded = meta.content_fingerprints[&key]
        .as_ref()
        .expect("the write produced bytes");
    let on_disk = std::fs::read(tasks_json(&root)).expect("file");
    assert_eq!(recorded, &Fingerprint::of(&on_disk));

    // ADR-011: this refusal is a `staleWrite`, which is `REVISION_CONFLICT` on the wire —
    // the client's own "reload, do not retry" path.
    let error = task_storage_diverge(&service, &project);
    assert!(error.is_stale_write(), "{error:?}");
    cleanup(root);
}

fn task_storage_diverge(
    service: &aeroric_core::tasks::service::TaskService,
    _project: &aeroric_core::storage::compat::Record,
) -> StorageError {
    // Force the conflict at the storage boundary so the mapping is what is under test, not
    // the service's error type.
    let path = service
        .context()
        .paths
        .tasks_file("p1")
        .expect("a valid id");
    std::fs::write(&path, b"[]").expect("foreign write");
    aeroric_core::storage::tasks::save_covering(service.context(), "p1", &[], Cover::LastCoreWrite)
        .expect_err("the last core write is no longer on disk")
}

#[test]
fn a_launch_failure_marks_the_task_failed_and_frees_the_attempt() {
    let (root, context, executor) = harness("spawn-fails", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = tasks_json(&root);
    executor.arm_failure("launch");

    let error = service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect_err("the process did not come up");
    assert!(matches!(error, TaskError::Launch(_)), "{error:?}");
    let row = &rows(&tasks_file)[0];
    assert_eq!(row.get("status"), Some(&json!("failed")));
    assert_eq!(
        row.get("failureReason"),
        Some(&json!(
            "the process could not be started (permission denied (os error 13))"
        )),
        "the reason the executor gave reaches the user"
    );
    assert!(row.get("completedAt").is_some(), "failed is terminal");

    // A genuine second attempt is allowed: a claim that never produced a process must not
    // lock the task out forever.
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("retry after a real failure");
    assert_eq!(executor.launches().len(), 2);
    cleanup(root);
}

#[test]
fn resume_refuses_to_invent_a_session_and_writes_nothing() {
    let (root, context, executor) = harness("no-session", &[task("t1", "p1", "interrupted")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = tasks_json(&root);
    let before = std::fs::read(&tasks_file).expect("seeded");

    // The host could not resolve a session id. An empty one is not "no session" to a CLI —
    // it is a fresh conversation with the user's old prompt pasted into it.
    let error = service
        .resume(&project, "t1", "", "op-r1", true, None)
        .expect_err("a blank session is refused");
    assert!(matches!(error, TaskError::NoSession(_)), "{error:?}");
    assert!(executor.call_log().is_empty(), "no process");
    assert_eq!(std::fs::read(&tasks_file).expect("unchanged"), before);
    assert_eq!(service.generation("t1"), 0, "and no generation was burned");
    cleanup(root);
}

#[test]
fn a_dsh_resume_is_planned_without_the_original_prompt() {
    let dsh = record(json!({
        "id": "t1", "projectId": "p1", "prompt": "build it", "agent": "dsh",
        "status": "interrupted", "permissionMode": "ask", "sessionFamily": "dsh",
        "dshSessionId": "d-1", "dshAgentPreset": "standard", "createdAt": 1i64
    }));
    let (root, context, executor) = harness("dsh-resume", &[dsh]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    service
        .resume(&project, "t1", "d-1", "op-r1", true, None)
        .expect("resumed");
    let call = &executor.call_log()[0];
    assert_eq!(call, "resume:t1");
    assert_eq!(executor.calls()[0].what, "resume");
    assert!(
        executor.calls()[0].detail.contains("prompt=None"),
        "reconnecting must not replay the message: {}",
        executor.calls()[0].detail
    );
    cleanup(root);
}

#[test]
fn start_resume_and_cancel_all_work_with_no_listener_attached() {
    // The acceptance line this test exists for: nothing in the business path asks whether
    // a view is watching. The harness attaches no observer to the service at all — the
    // events below are pushed by the test in the way the host's observer would.
    let (root, context, executor) = harness("headless", &[task("t1", "p1", "todo")]);
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
            HostWindowFacts::default(),
        )
        .expect("started");
    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Runtime,
            },
        )
        .expect("running");
    service
        .cancel(&project, "t1")
        .expect("cancelled — the request does not depend on a listener");
    assert_eq!(
        executor.call_log(),
        vec!["launch:t1".to_string(), "cancel:t1".to_string()]
    );
    assert_eq!(
        executor.calls()[1].detail,
        format!(
            "{:?}",
            aeroric_core::tasks::executor::CancelTarget::Local {
                project_path: "/repo".into()
            }
        )
    );

    // Resume needs a session the host resolved. The state change is written *through* the
    // service: a hand edit would be, correctly, a foreign write that the next save refuses.
    service
        .mutate("p1", |rows, _now| {
            let mut next = rows.to_vec();
            let mut row = next[0].clone();
            row.0.insert("claudeSessionId".to_string(), json!("c-9"));
            row.0.insert("sessionFamily".to_string(), json!("claude"));
            next[0] = row;
            (next, true)
        })
        .expect("session recorded");
    service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 1,
                code: Some(1),
            },
        )
        .expect("run 1 ends");
    service
        .resume(&project, "t1", "c-9", "op-r1", true, None)
        .expect("resumed headless");
    assert_eq!(executor.call_log().last().unwrap(), "resume:t1");
    cleanup(root);
}

#[test]
fn a_late_exit_from_the_previous_run_does_not_move_the_task() {
    let (root, context, executor) = harness("late-exit", &[task("t1", "p1", "interrupted")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = tasks_json(&root);

    // Run 1 is the one that ended before; the user resumes, which is run 2.
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("run 1");
    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Runtime,
            },
        )
        .expect("running");
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
            HostWindowFacts::default(),
        )
        .expect("run 2 is a new attempt");
    let running_after = rows(&tasks_file)[0]
        .get("status")
        .cloned()
        .expect("a status");
    assert_eq!(running_after, json!("pending"));

    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Failed,
                attention_requested_at_ms: None,
                failure_reason: Some("the old process died".into()),
                origin: ObservationOrigin::Runtime,
            },
        )
        .expect("dropped");
    let after = rows(&tasks_file)[0].clone();
    assert_eq!(
        after.get("status"),
        Some(&json!("pending")),
        "run 1's failure may not settle run 2"
    );
    assert_eq!(
        after.get("failureReason"),
        None,
        "and it may not leave a reason behind either"
    );

    // The current run's own exit does land.
    let (outcome, _) = service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 2,
                code: Some(1),
            },
        )
        .expect("settled");
    assert_eq!(
        outcome,
        EventOutcome::Applied {
            status: TaskStatus::Failed,
            changed: true,
            // The reason the exit gave has to ride along in the outcome: the sink is not
            // allowed to reconstruct it, and `failed` without it is a payload regression the
            // frontend would swallow silently.
            failure_reason: Some("the process exited with code 1".into())
        }
    );
    assert_eq!(
        rows(&tasks_file)[0].get("failureReason"),
        Some(&json!("the process exited with code 1"))
    );
    cleanup(root);
}

#[test]
fn a_terminal_ready_settles_the_attempt_that_was_waiting_for_input() {
    // With `persist_before_launch` the claim stays open on purpose: the host has not said
    // the terminal can take the first message yet. That window is exactly where a retry
    // must be refused, and TerminalReady is what closes it.
    let (root, context, executor) = harness("first-input", &[task("t1", "p1", "todo")]);
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
            HostWindowFacts::default(),
        )
        .expect("started");
    assert!(
        !service.open_claims("t1").is_empty(),
        "waiting for the terminal"
    );
    service
        .observe(
            "p1",
            TaskEvent::TerminalReady {
                task_id: "t1".into(),
                generation: 1,
            },
        )
        .expect("ready");
    assert!(service.open_claims("t1").is_empty());
    cleanup(root);
}

#[test]
fn completion_is_three_way_and_only_the_third_one_writes() {
    let live = task("live", "p1", "running");
    let dsh_row = record(json!({
        "id": "dshr", "projectId": "p1", "prompt": "x", "agent": "dsh",
        "status": "detached", "permissionMode": "ask", "sessionFamily": "dsh", "createdAt": 1i64
    }));
    let settled = task("settled", "p1", "interrupted");
    let (root, context, executor) = harness(
        "complete",
        &[live.clone(), dsh_row.clone(), settled.clone()],
    );
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = tasks_json(&root);

    let (kind, applied) = service.complete(&project, "live").expect("live");
    assert_eq!(kind, launch_plan::Completion::StopProcess);
    assert!(
        applied.is_none(),
        "the exit event settles it, not this call"
    );
    assert_eq!(executor.call_log(), vec!["complete:live".to_string()]);

    let (kind, applied) = service.complete(&project, "dshr").expect("dsh");
    assert_eq!(
        kind,
        launch_plan::Completion::Dsh,
        "dsh has its own channel"
    );
    assert!(applied.is_none());
    assert_eq!(executor.call_log().len(), 2);

    let (kind, applied) = service.complete(&project, "settled").expect("settled");
    assert_eq!(kind, launch_plan::Completion::Nothing);
    assert!(
        applied.expect("a write").changed,
        "no process, so the status is ours to set"
    );
    let rows = rows(&tasks_file);
    assert_eq!(rows[2].get("status"), Some(&json!("done")));
    assert!(rows[2].get("completedAt").is_some());
    assert_eq!(
        rows[0].get("status"),
        Some(&json!("running")),
        "and the live task is untouched by a call that only asked the backend"
    );
    cleanup(root);
}

#[test]
fn deletion_skips_starred_rows_refuses_live_ones_and_never_touches_the_worktree() {
    let mut starred = task("star", "p1", "done");
    starred.0.insert("starred".to_string(), json!(true));
    let mut live = task("live", "p1", "running");
    live.0
        .insert("worktreePath".to_string(), json!("/repo/.wt/live"));
    let done = task("gone", "p1", "done");
    let (root, context, executor) = harness("delete", &[starred, live, done]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);
    let seeded = std::fs::read(&tasks_file).expect("seeded");

    let refused = service
        .delete("p1", &["live".to_string()])
        .expect_err("a running task is not deleted from under its process");
    assert!(
        matches!(refused, TaskError::AlreadyClaimed { .. }),
        "{refused:?}"
    );
    assert_eq!(std::fs::read(&tasks_file).expect("untouched"), seeded);

    let result = service
        .delete("p1", &["star".to_string(), "gone".to_string()])
        .expect("deletable");
    assert_eq!(result.removed, vec!["gone".to_string()]);
    assert_eq!(result.skipped_starred, vec!["star".to_string()]);
    let rows = rows(&tasks_file);
    assert_eq!(rows.len(), 2, "star and live both remain");
    assert!(rows.iter().any(|row| row.get("id") == Some(&json!("live"))));
    assert!(
        executor.call_log().is_empty(),
        "no cancel, no worktree removal: those are the host's side effects, and deleting a \
         row must not imply deleting the code"
    );
    cleanup(root);
}

#[test]
fn an_unresolved_start_becomes_interrupted_on_the_next_boot_without_being_restarted() {
    // Simulate the crash: write a claim, drop the service, build a fresh one over the same
    // directory. Whatever the previous process left open is now reconciled, and nothing
    // spawned on our own initiative.
    let (root, context, executor) = harness("crash", &[task("t1", "p1", "pending")]);
    let first = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    first
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("started");
    assert!(
        !first.open_claims("t1").is_empty(),
        "still waiting for the terminal"
    );
    // Dropping the service is the crash: the lease goes with it, and the ledger file does not.
    drop(first);
    let spawned_so_far = executor.launches().len();
    let restarted = build_service(common::reacquire(&root), &executor);
    let (reconciled, applied) = restarted.reconcile("p1").expect("reconciled");
    assert_eq!(reconciled, vec!["t1".to_string()]);
    assert!(applied.expect("a write").changed);
    assert_eq!(
        executor.launches().len(),
        spawned_so_far,
        "reconciliation never spawns"
    );
    let rows = rows(&tasks_json(&root));
    assert_eq!(rows[0].get("status"), Some(&json!("interrupted")));
    assert_eq!(
        rows[0].get("completedAt"),
        None,
        "interrupted carries no end time (ADR-018)"
    );
    // ADR-018: the user has not seen this yet, so continuing where it stopped is not
    // offered until it is acknowledged...
    let resume_error = restarted
        .resume(&project, "t1", "c-old", "op-r", true, None)
        .expect_err("a cut-off run needs an acknowledgement before it is resumed");
    assert!(
        matches!(resume_error, TaskError::AwaitingAcknowledgement { .. }),
        "{resume_error:?}"
    );
    assert!(
        restarted.awaiting_acknowledgement("t1").is_some(),
        "and the pending flag is readable by the host, so it can be shown"
    );
    // ...but starting over is itself that acknowledgement.
    restarted
        .start(
            &project,
            "t1",
            "op-fresh",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("a fresh run is allowed through");
    assert!(restarted.awaiting_acknowledgement("t1").is_none());
    // A second crash is reconciled and gated the same way.
    restarted
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 2,
                code: Some(1),
            },
        )
        .expect("run 2 fails");
    restarted
        .resume(&project, "t1", "c-old", "op-r2", true, None)
        .expect("no pending acknowledgement: the user already saw this task");
    // The resume itself is an attempt in flight, so close it the way a live host would
    // before asserting that a second boot has nothing left to reconcile.
    restarted
        .observe(
            "p1",
            TaskEvent::TerminalReady {
                task_id: "t1".into(),
                generation: 3,
            },
        )
        .expect("run 3's terminal is up");
    assert_eq!(
        executor.launches().len(),
        spawned_so_far + 1,
        "only the fresh start spawned"
    );
    // The claim is released, so a second boot has nothing left to reconcile.
    let (again, applied) = restarted.reconcile("p1").expect("second pass");
    assert!(again.is_empty());
    assert!(!applied.expect("nothing to write").changed);
    cleanup(root);
}

#[test]
fn a_task_family_is_resolved_once_and_the_plan_follows_it() {
    // The launch channel and the session fields must agree, or a codex task gets a claude
    // `--resume`. This is the seam between session_owner and launch_plan.
    let mixed = record(json!({
        "id": "t1", "projectId": "p1", "agent": "codex", "prompt": "go",
        "status": "todo", "claudeSessionId": "stale", "codexSessionId": "x-1",
        "sessionFamily": "codex", "permissionMode": "ask", "createdAt": 1i64
    }));
    let (root, context, executor) = harness("family", std::slice::from_ref(&mixed));
    let service = build_service(context, &executor);
    let owner = service.session_owner_of(&mixed);
    assert_eq!(owner.family, ProtocolFamily::Codex);
    let planned = launch_plan::launch_request(
        &mixed,
        &local_project("p1", "/repo"),
        &Families,
        "op-family",
        1,
        vec![],
        vec![],
        false,
        None,
        None,
    )
    .expect("planned");
    assert_eq!(planned.session_id.as_deref(), Some("x-1"));
    assert_eq!(planned.family, ProtocolFamily::Codex);
    assert!(
        !session_owner::can_native_resume(&mixed, "dsh", &Families),
        "a cross-family switch is not a resume"
    );
    cleanup(root);
}

#[test]
fn the_ledger_file_is_core_exclusive_and_a_corrupt_one_stops_the_service_from_writing() {
    // If `task-operations.json` cannot be read, spawn-once cannot be guaranteed. The safe
    // answer is to refuse, not to start with an empty ledger and double-spawn.
    let (root, context, _executor) = harness("ledger-corrupt", &[task("t1", "p1", "todo")]);
    std::fs::write(
        context.paths.task_operations_file(),
        b"{\"claims\": \"not a list\"}",
    )
    .expect("corrupt");
    // A ledger that cannot be read is not an empty ledger: constructing the service would
    // be a promise that no start is in flight, and that promise is what permits a spawn.
    let error = operations::load(&context).expect_err("unreadable");
    let reason = match error {
        StorageError::Corrupt { reason, .. } => reason,
        other => panic!("{other:?}"),
    };
    assert!(reason.contains("sequence"), "{reason}");

    // A ledger that cannot be read is not an empty ledger: constructing the service would
    // be a promise that no start is in flight, and that promise is what permits a spawn.
    let runner: Arc<dyn aeroric_core::tasks::executor::TaskExecutor> = _executor.clone();
    let failure = aeroric_core::tasks::service::TaskService::new(
        Arc::new(context),
        Arc::new(Families),
        runner,
        Arc::new(NoLiveRuns),
    );
    match failure {
        Err(StorageError::Corrupt { .. }) => {}
        Err(other) => panic!("the ledger error must reach the caller, not become {other:?}"),
        Ok(_) => panic!("a corrupt ledger must not produce a usable service"),
    }
    cleanup(root);
}

#[test]
fn an_unknown_project_id_is_refused_before_any_file_is_touched() {
    let (root, context, executor) = harness("bad-id", &[]);
    let service = build_service(context, &executor);
    let error = service
        .load("../escape")
        .expect_err("a task list is not a path traversal primitive");
    assert!(
        matches!(error, TaskError::Storage(StorageError::InvalidId(_))),
        "{error:?}"
    );
    assert_eq!(executor.call_log(), Vec::<String>::new());
    cleanup(root);
}

#[test]
fn mutating_helpers_keep_their_own_idempotence() {
    let (root, context, executor) = harness("mutate", &[task("t1", "p1", "done")]);
    let service = build_service(context, &executor);
    let revision_before = runtime::load(service.context())
        .expect("metadata")
        .found()
        .map(|meta| meta.revision)
        .unwrap_or_else(|| "0".to_string());
    let applied = service
        .mutate("p1", |rows, now| {
            reducer::archive(rows, &["t1".to_string()], now)
        })
        .expect("archived");
    assert!(applied.changed);
    let again = service
        .mutate("p1", |rows, now| {
            reducer::archive(rows, &["t1".to_string()], now)
        })
        .expect("already archived");
    assert!(
        !again.changed,
        "a second archive of the same task is not a change"
    );
    assert_eq!(
        again.revision, None,
        "and an unchanged mutation must not advance the counter from nil"
    );
    let revision_after = runtime::load(service.context())
        .expect("metadata")
        .found()
        .expect("the archive wrote metadata")
        .revision;
    assert_ne!(
        revision_after, revision_before,
        "a real mutation advances the counter"
    );
    let (again, applied) = {
        let applied = service
            .mutate("p1", |rows, _now| (rows.to_vec(), false))
            .expect("no-op");
        (
            runtime::load(service.context())
                .expect("metadata")
                .found()
                .expect("still there")
                .revision,
            applied,
        )
    };
    assert_eq!(
        again, revision_after,
        "and a no-op leaves it exactly where it was"
    );
    assert!(!applied.changed);
    cleanup(root);
}

#[test]
fn a_status_with_no_change_writes_nothing_and_burns_no_revision() {
    let (root, context, executor) = harness("no-write", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = tasks_json(&root);
    // The run has to be live: ADR-034 drops a non-terminal report for a task with no run left,
    // and this test is about the *other* half — an admitted report that changes nothing.
    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("run 1");
    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Runtime,
            },
        )
        .expect("the run starts");
    let before = std::fs::read(&tasks_file).expect("started");
    let (outcome, applied) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Runtime,
            },
        )
        .expect("same status");
    assert_eq!(
        outcome,
        EventOutcome::Applied {
            status: TaskStatus::Running,
            changed: false,
            failure_reason: None
        }
    );
    assert!(!applied.changed);
    assert_eq!(std::fs::read(&tasks_file).expect("untouched"), before);
    cleanup(root);
}

#[test]
fn the_new_task_record_matches_the_shape_the_client_writes() {
    let (root, context, executor) = harness("create", &[]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let (created, applied) = service
        .create(
            &project,
            NewTask {
                id: "new-1".into(),
                project_id: "p1".into(),
                prompt: "   ".into(),
                agent: "dsh".into(),
                permission_mode: "ask".into(),
                immediate: false,
                selected_model: Some("m".into()),
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
                agent_config_id: None,
                images: vec![],
                texts: vec![],
            },
        )
        .expect("created");
    assert!(applied.changed);
    assert_eq!(created.get("status"), Some(&json!("todo")), "not immediate");
    assert_eq!(
        created.get("name"),
        Some(&json!("dsh")),
        "a blank prompt falls back to the agent label"
    );
    assert_eq!(
        created.get("dshAgentPreset"),
        Some(&json!("standard")),
        "dsh defaults its preset"
    );
    assert_eq!(created.get("createdAt"), Some(&json!(1_700_000_000_000i64)));
    let rows = rows(&tasks_json(&root));
    assert_eq!(rows[0].get("id"), Some(&json!("new-1")), "newest first");
    assert!(
        rows[0].get("reasoningEffort").is_none(),
        "an unset field is absent, not null"
    );

    // A second `create` under the same id is refused rather than silently duplicated.
    let error = service
        .create(
            &project,
            NewTask {
                id: "new-1".into(),
                ..NewTask {
                    id: String::new(),
                    project_id: "p1".into(),
                    prompt: "x".into(),
                    agent: "claude".into(),
                    permission_mode: "ask".into(),
                    immediate: true,
                    selected_model: None,
                    reasoning_effort: None,
                    speed: None,
                    dsh_agent_preset: None,
                    agent_config_id: None,
                    images: vec![],
                    texts: vec![],
                }
            },
        )
        .expect_err("one id, one row");
    assert!(
        matches!(error, TaskError::DuplicateId(ref id) if id == "new-1"),
        "{error:?}"
    );
    cleanup(root);
}

#[test]
fn an_immediate_task_is_created_and_spawned_with_the_same_commit_as_its_record() {
    let (root, context, executor) = harness("create-now", &[]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let (_created, _) = service
        .create(
            &project,
            NewTask {
                id: "immediate".into(),
                project_id: "p1".into(),
                prompt: "go".into(),
                agent: "claude".into(),
                permission_mode: "ask".into(),
                immediate: true,
                selected_model: None,
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
                agent_config_id: None,
                images: vec![],
                texts: vec![],
            },
        )
        .expect("created as pending");
    // `create` only writes. Starting is a separate call so the two ordering guarantees stay
    // testable on their own — but the record must already be `pending` when it lands.
    let rows = rows(&tasks_json(&root));
    assert_eq!(rows[0].get("status"), Some(&json!("pending")));
    service
        .start(
            &project,
            "immediate",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("spawned");
    assert_eq!(executor.call_log(), vec!["launch:immediate".to_string()]);
    assert_eq!(service.generation("immediate"), 1);
    cleanup(root);
}

#[test]
fn a_file_core_has_never_published_is_adopted_rather_than_refused_forever() {
    // The other side of the same rule, and the reason the rule is not "the disk must match
    // a table entry": a directory arriving from the shipping client has no entry at all, and
    // refusing there would mean the migration could never start.
    let (root, context, executor) = harness("adopt", &[task("t1", "p1", "todo")]);
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
            HostWindowFacts::default(),
        )
        .expect("the first core write adopts the legacy bytes");
    // Close the first attempt the way a live host would: the terminal came up, so the
    // attempt is over and a later start is genuinely new. Without this the second call is
    // refused as an in-flight duplicate — a different guarantee, pinned in
    // tests/task_idempotency.rs — and the cover check is never reached.
    service
        .observe(
            "p1",
            TaskEvent::TerminalReady {
                task_id: "t1".into(),
                generation: 1,
            },
        )
        .expect("attempt 1 settled");

    // From here on the same foreign edit *is* a conflict, which is what makes the adoption a
    // one-time handover rather than a permanent blind spot.
    let tasks_file = executor.tasks_path();
    let mut edited = rows(&tasks_file);
    edited[0]["prompt"] = json!("legacy again");
    std::fs::write(&tasks_file, serde_json::to_vec_pretty(&edited).unwrap()).unwrap();
    let error = service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect_err("now core has a baseline to defend");
    assert!(matches!(error, TaskError::PersistFailed(_)), "{error:?}");
    cleanup(root);
}

#[test]
fn a_config_switch_that_cannot_be_saved_rolls_back_and_never_reaches_a_process() {
    // Action 2's "配置切换失败回滚". The interesting part is not that it fails, it is what
    // survives: only the fields this switch moved go back, and a session id the running
    // agent wrote in the meantime is kept.
    let row = json!({
        "id": "t1", "projectId": "p1", "prompt": "go", "agent": "claude",
        "status": "running", "permissionMode": "ask", "createdAt": 1i64,
        "claudeSessionId": "c-old", "sessionFamily": "claude", "name": "keep me"
    });
    let (root, context, executor) = harness("switch-rollback", &[record(row)]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = executor.tasks_path();

    let switch_to_codex = aeroric_core::tasks::service::ConfigSwitch {
        agent: "codex".into(),
        permission_mode: "full_access".into(),
        selected_model: Some("gpt-5".into()),
        reasoning_effort: None,
        speed: None,
    };
    let same_family = aeroric_core::tasks::service::ConfigSwitch {
        agent: "claude_gpt55".into(),
        permission_mode: "full_access".into(),
        selected_model: Some("gpt-5".into()),
        reasoning_effort: None,
        speed: None,
    };
    let evidence = aeroric_core::tasks::service::SwitchEvidence {
        native_resume_supported: true,
        session_id: Some("c-old".into()),
        ..Default::default()
    };
    executor.arm_failure("reset");
    let error = service
        .switch_config(&project, "t1", "op-s1", &switch_to_codex, &evidence)
        .expect_err("the running process could not be stopped");
    assert!(matches!(error, TaskError::Launch(_)), "{error:?}");
    assert_eq!(
        executor.call_log(),
        vec!["reset:t1".to_string()],
        "the only side effect is the reset that failed: no launch, no write"
    );
    let untouched = rows(&tasks_file);
    assert_eq!(
        untouched[0].get("agent"),
        Some(&json!("claude")),
        "nothing was written"
    );
    executor.clear_failure("reset");

    // A clean switch lands first, so core now owns a published cover for this file. The
    // strategy is **handoff**, not resume (ADR-029): `claude_gpt55` reads Claude-format
    // transcripts but its CLI is codex-style, so `--resume` cannot pick the session up where
    // it stands. An earlier version of this test asserted `Resume` because the parity fixture
    // row it was written against was transcribed wrong; the parity table flipped the same two
    // conclusions when that row was fixed.
    let (strategy, applied) = service
        .switch_config(
            &project,
            "t1",
            "op-s3",
            &same_family,
            &aeroric_core::tasks::service::SwitchEvidence {
                native_resume_supported: true,
                session_id: Some("c-old".into()),
                ..Default::default()
            },
        )
        .expect("switched");
    assert_eq!(
        strategy,
        aeroric_core::tasks::session_owner::ConfigSwitchStrategy::Handoff
    );
    assert!(applied.changed);
    assert_eq!(
        applied.generation,
        Some(1),
        "the switch took an attempt out"
    );
    let landed = rows(&tasks_file);
    assert_eq!(landed[0].get("agent"), Some(&json!("claude_gpt55")));
    assert_eq!(landed[0].get("permissionMode"), Some(&json!("full_access")));
    assert_eq!(landed[0].get("status"), Some(&json!("pending")));

    // Retrying the same switch key is refused, exactly like a retried start.
    let repeat = service
        .switch_config(&project, "t1", "op-s3", &same_family, &evidence)
        .expect_err("the same operation key is one attempt");
    assert!(
        matches!(repeat, TaskError::AlreadyClaimed { .. }),
        "{repeat:?}"
    );

    // Now the persist is what fails: the bytes moved under us mid-switch.
    let mut edited = rows(&tasks_file);
    edited[0]["name"] = json!("edited elsewhere");
    std::fs::write(&tasks_file, serde_json::to_vec_pretty(&edited).unwrap()).unwrap();
    let error = service
        .switch_config(&project, "t1", "op-s4", &switch_to_codex, &evidence)
        .expect_err("the cover is gone");
    assert!(matches!(error, TaskError::PersistFailed(_)), "{error:?}");
    let after = rows(&tasks_file);
    assert_eq!(
        after[0].get("name"),
        Some(&json!("edited elsewhere")),
        "and the foreign edit is what the refusal preserved"
    );
    assert_eq!(
        runtime::load(service.context())
            .expect("meta")
            .found()
            .expect("written")
            .revision,
        "1",
        "one switch landed and cost one revision; neither the duplicate key nor the refused \
         retry cost anything"
    );
    assert!(
        executor
            .call_log()
            .iter()
            .all(|call| !call.starts_with("launch")),
        "and it reached no process: {:?}",
        executor.call_log()
    );
    assert_eq!(
        service.open_claims("t1").len(),
        1,
        "only the switch that landed holds an attempt; the refused one gave its number back"
    );
    assert_eq!(
        after[0].get("agent"),
        Some(&json!("claude_gpt55")),
        "the switch that had already landed is not undone by the one that failed"
    );
    cleanup(root);
}

#[test]
fn a_switch_with_nothing_to_continue_from_is_refused_before_any_write() {
    // `switchConfigNoContext`: no transcript, no terminal history, no prompt. Accepting this
    // would open a blank session and present it as the same conversation.
    let row = json!({
        "id": "t1", "projectId": "p1", "prompt": "   ", "agent": "claude",
        "status": "running", "permissionMode": "ask", "createdAt": 1i64
    });
    let (root, context, executor) = harness("switch-no-context", &[record(row)]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = executor.tasks_path();
    let before = std::fs::read(&tasks_file).expect("seeded");
    let error = service
        .switch_config(
            &project,
            "t1",
            "op-s",
            &aeroric_core::tasks::service::ConfigSwitch {
                agent: "codex".into(),
                permission_mode: "ask".into(),
                selected_model: None,
                reasoning_effort: None,
                speed: None,
            },
            &aeroric_core::tasks::service::SwitchEvidence::default(),
        )
        .expect_err("there is nothing to carry over");
    assert!(
        matches!(error, TaskError::NoContinuationContext(_)),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&tasks_file).expect("unchanged"), before);
    cleanup(root);
}

#[test]
fn a_discarded_worktree_cannot_be_resumed_even_when_a_session_id_is_offered() {
    // The rule lives in a component today (`RunningView.tsx:724,741`): the button is simply
    // not rendered. A hidden button is not a rule, so the core refuses it as well.
    let row = json!({
        "id": "t1", "projectId": "p1", "prompt": "go", "agent": "claude",
        "status": "interrupted", "permissionMode": "ask", "createdAt": 1i64,
        "claudeSessionId": "c-1", "sessionFamily": "claude",
        "worktreePath": "/w", "worktreeDiscarded": true
    });
    let (root, context, executor) = harness("discarded-resume", &[record(row)]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = executor.tasks_path();
    let before = std::fs::read(&tasks_file).expect("seeded");
    let error = service
        .resume(&project, "t1", "c-1", "op-r", true, None)
        .expect_err("the tree the session belongs to is gone");
    assert!(
        matches!(error, TaskError::WorktreeDiscarded(_)),
        "{error:?}"
    );
    assert!(executor.call_log().is_empty(), "no process");
    assert_eq!(std::fs::read(&tasks_file).expect("untouched"), before);
    assert_eq!(service.generation("t1"), 0, "and no attempt was claimed");
    // Starting over is still allowed: a new run in a new tree is not a resume.
    service
        .start(
            &project,
            "t1",
            "op-new",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("a fresh run is not a continuation");
    cleanup(root);
}

#[test]
fn a_remote_project_refuses_attachments_and_a_worktree_before_anything_moves() {
    // `taskLifecycle.ts:283-296` refuses these in the submit handler today, which leaves the
    // core free to accept a payload the channel cannot carry. The refusal is a channel limit,
    // so it belongs on the path that starts the process.
    let row = record(json!({
        "id": "t1", "projectId": "p2", "prompt": "go", "agent": "claude",
        "status": "todo", "permissionMode": "ask", "createdAt": 1i64
    }));
    let ssh_project = record(json!({
        "id": "p2", "path": "ssh:c1:/srv/app", "name": "app",
        "location": { "kind": "ssh", "connectionId": "c1", "remotePath": "/srv/app" }
    }));
    let (root, context, executor) = harness("remote-shape", &[]);
    let p2_tasks = context.paths.tasks_file("p2").unwrap();
    std::fs::create_dir_all(p2_tasks.parent().unwrap()).unwrap();
    std::fs::write(
        &p2_tasks,
        serde_json::to_vec_pretty(&Value::Array(vec![Value::Object(row.0.clone())])).unwrap(),
    )
    .unwrap();
    let service = build_service(context, &executor);
    let error = service
        .start(
            &ssh_project,
            "t1",
            "op-r",
            true,
            None,
            false,
            Attachments {
                images: vec!["shot.png".to_string()],
                texts: vec![],
            },
            HostWindowFacts::default(),
        )
        .expect_err("ssh cannot carry an attachment");
    assert!(
        matches!(
            &error,
            TaskError::UnsupportedOnRemote {
                kind: "ssh",
                what: "attachments",
            }
        ),
        "{error:?}"
    );
    assert!(
        executor.call_log().is_empty(),
        "refused before the reset or launch"
    );

    // The same channel refuses a worktree, because the worktree is a local git object.
    let with_worktree = record(json!({
        "id": "t2", "projectId": "p2", "prompt": "go", "agent": "claude",
        "status": "todo", "permissionMode": "ask", "createdAt": 1i64,
        "worktreePath": "/repo/.wt/t2"
    }));
    std::fs::write(
        service.context().paths.tasks_file("p2").unwrap(),
        serde_json::to_vec_pretty(&Value::Array(vec![Value::Object(with_worktree.0.clone())]))
            .unwrap(),
    )
    .unwrap();
    service.load("p2").expect("re-read");
    let error = service
        .start(
            &ssh_project,
            "t2",
            "op-w",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect_err("a remote project has no worktree to run in");
    assert!(
        matches!(
            &error,
            TaskError::UnsupportedOnRemote {
                what: "a worktree",
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        service.generation("t2"),
        0,
        "and no attempt number was burned"
    );
    cleanup(root);
}

/// ADR-034: an observation from a run that has already reported must not resurrect the task.
///
/// This is the accident the host's `is_task_active` gate exists to prevent, and the reason it
/// cannot simply be deleted with the eight emit sites: codex/claude transcript watchers keep
/// polling after the process is gone (`spawn_status_session_watcher` and the lazy-attach loop
/// each carry their own deadline), so a line read late can arrive *after* the exit event that
/// settled the row. The guard in `observe` catches an older *generation*, but this observation
/// carries the same generation as the run that just finished — which is exactly the case the
/// legacy gate covered and the current conjunct does not.
#[test]
fn a_late_observation_from_a_run_that_already_reported_does_not_resurrect_the_task() {
    let (root, context, executor) = harness("late-observation", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let project = local_project("p1", "/repo");
    let tasks_file = tasks_json(&root);

    service
        .start(
            &project,
            "t1",
            "op-1",
            true,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("run 1");
    service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Transcript,
            },
        )
        .expect("running");
    // The counter-case ADR-034 demands: while the run is live, the same kind of observation
    // must be applied, or the rule could "drop" everything and still look green.
    assert_eq!(
        rows(&tasks_file)[0].get("status"),
        Some(&json!("running")),
        "a live run's observations are not late"
    );

    service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 1,
                code: Some(0),
            },
        )
        .expect("the run settled");
    assert_eq!(
        rows(&tasks_file)[0].get("status"),
        Some(&json!("done")),
        "a clean exit settles"
    );

    let fingerprint_before = service
        .load("p1")
        .expect("load")
        .fingerprint
        .expect("written")
        .sha256;
    let (outcome, applied) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::InputRequired,
                attention_requested_at_ms: Some(1_700_000_099_000),
                failure_reason: None,
                origin: ObservationOrigin::Transcript,
            },
        )
        .expect("a late observation is answered, not an error");

    assert!(
        matches!(outcome, EventOutcome::Dropped(_)),
        "the run already reported; this line belongs to a world that ended: {outcome:?}"
    );
    assert!(!applied.changed, "and nothing was written");
    assert_eq!(
        rows(&tasks_file)[0].get("status"),
        Some(&json!("done")),
        "the card must not start spinning again"
    );
    assert_eq!(
        service
            .load("p1")
            .expect("reload")
            .fingerprint
            .expect("written")
            .sha256,
        fingerprint_before,
        "a dropped observation may not even rewrite the file"
    );
    cleanup(root);
}

/// ADR-035: the announcement path has to work on an install that was never handed the writer
/// role, because that is every non-macOS install today (ADR-021). If the *decision* needed the
/// write, deleting the legacy emit sites there would have silenced every `task-status` — the
/// exact outcome ADR-033 §4 forbids.
#[test]
fn a_host_that_was_never_handed_the_writer_role_still_gets_a_decision_and_writes_nothing() {
    let (root, _writer, executor) = harness("decide-only", &[task("t1", "p1", "running")]);
    let tasks_file = tasks_json(&root);
    let service = build_service(reader(&root), &executor);

    let fingerprint_before = service
        .load("p1")
        .expect("reading needs no lease")
        .fingerprint
        .expect("the seed wrote bytes")
        .sha256;

    let (outcome, applied) = service
        .decide_observation(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 0,
                code: Some(0),
            },
        )
        .expect("a reader can decide");

    assert_eq!(
        outcome,
        EventOutcome::Applied {
            status: TaskStatus::Done,
            changed: true,
            failure_reason: None
        },
        "the reducer ran — that is the whole point of splitting it from the commit"
    );
    assert!(applied.revision.is_none(), "nothing was written");
    assert!(applied.fingerprint.is_none(), "no cover was bought");
    assert_eq!(
        rows(&tasks_file)[0].get("status"),
        Some(&json!("running")),
        "the host's bytes stay whatever the host left there"
    );
    assert_eq!(
        service
            .load("p1")
            .expect("reload")
            .fingerprint
            .expect("still seeded")
            .sha256,
        fingerprint_before,
        "a decision may not even touch the file"
    );

    // Positive control. Without it "decide_observation wrote nothing" could be true because
    // *nothing* on a read-only context ever writes, which is not the property anyone needs.
    let refused = service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 0,
                code: Some(0),
            },
        )
        .expect_err("the writer entry point must still refuse");
    assert!(
        matches!(refused, TaskError::Storage(_)),
        "expected a storage refusal, got {refused:?}"
    );
    assert!(
        !root.join("task-operations.json").exists(),
        "settling a claim in memory may not create the ledger file either"
    );
    cleanup(root);
}

/// ADR-034's boundary is "no live run left", not "the row reads terminal". The two differ in
/// exactly one place, and it is a place the shipping client reaches today: a run is still
/// alive while the row already says `cancelled` (the client's save path wrote that; the
/// process has not finished dying), and the host emits unconditionally during that window.
/// Gating on the row would swallow the `running` that starts a re-run of a finished card.
#[test]
fn a_late_report_from_a_still_live_run_is_admitted_even_though_the_row_reads_cancelled() {
    let (root, context, executor) = harness("relight-while-live", &[task("t1", "p1", "todo")]);
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
            HostWindowFacts::default(),
        )
        .expect("run 1");

    // The client's own write, not core's: `mutate` commits without settling the run's claim,
    // which is precisely the state a cancel leaves behind.
    service
        .mutate("p1", |records, _| {
            let mut records = records.to_vec();
            for row in &mut records {
                if row.get("id").and_then(Value::as_str) == Some("t1") {
                    row.0.insert("status".into(), json!("cancelled"));
                }
            }
            (records, true)
        })
        .expect("the host's status write");

    let (outcome, _) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Transcript,
            },
        )
        .expect("answered, not refused");

    assert!(
        matches!(outcome, EventOutcome::Applied { .. }),
        "the run is live, so this is not a late observation: {outcome:?}"
    );

    // And the same report with nothing live behind it must still be dropped, or the previous
    // assertion only proves the rule got weaker.
    service
        .observe(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 1,
                code: Some(0),
            },
        )
        .expect("the run settles");
    let (dropped, _) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 1,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Transcript,
            },
        )
        .expect("answered");
    assert!(
        matches!(dropped, EventOutcome::Dropped(_)),
        "with no run left, the same report is exactly the accident ADR-034 exists for: {dropped:?}"
    );
    cleanup(root);
}

/// ADR-036: the run the *host* started has no core claim, ever — and on the shipping client
/// that is every run, because the product path still launches through the legacy commands. So
/// the liveness question must genuinely reach the port. Two services over the same seeded row,
/// differing only in what the port answers.
#[test]
fn a_legacy_runs_report_is_held_up_by_the_port_and_by_nothing_else() {
    let (root, context, executor) = harness("port-live", &[task("t1", "p1", "done")]);
    let live = StubLiveness::live(&["t1"]);
    let service = build_service_with_liveness(context, &executor, live.clone());

    let (outcome, _) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 0,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Transcript,
            },
        )
        .expect("answered");
    assert!(
        matches!(outcome, EventOutcome::Applied { .. }),
        "the host says a run is alive, so this is not a late observation: {outcome:?}"
    );
    assert!(
        live.asks() > 0,
        "the guard has to actually ask; a port nobody calls passes every test"
    );
    assert_eq!(
        rows(&tasks_json(&root))[0].get("status"),
        Some(&json!("running")),
        "and the answer is applied"
    );
    cleanup(root);

    let (root, context, executor) = harness("port-dead", &[task("t1", "p1", "done")]);
    let service = build_service(context, &executor);
    let (dropped, _) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 0,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Transcript,
            },
        )
        .expect("answered");
    assert!(
        matches!(dropped, EventOutcome::Dropped(_)),
        "with nothing alive behind it, the same report is the accident ADR-034 exists for: {dropped:?}"
    );
    assert_eq!(
        rows(&tasks_json(&root))[0].get("status"),
        Some(&json!("done")),
        "the counter-example writes nothing"
    );
    cleanup(root);
}

/// ADR-035's router: the 28 legacy report sites carry a task id and nothing else
/// (`emit_task_status(app, task_id, status)`), while every core mutation — including the
/// `decide_observation` those sites are about to call — is addressed by project. Two projects
/// have to exist on disk for this to mean anything: with one, "always answer the only project"
/// passes without resolving anything.
#[test]
fn an_observation_can_be_routed_from_a_task_id_alone() {
    let (root, context, executor) = harness("router", &[task("t1", "p1", "todo")]);
    std::fs::write(
        context.paths.projects_file(),
        serde_json::to_vec(&json!([
            {"id": "p1", "path": "/repo", "name": "repo"},
            {"id": "p2", "path": "/other", "name": "other"},
        ]))
        .expect("project list"),
    )
    .expect("write projects.json");
    let p2_dir = root.join("managed").join("projects").join("p2");
    std::fs::create_dir_all(&p2_dir).expect("p2 dir");
    std::fs::write(
        p2_dir.join("tasks.json"),
        serde_json::to_vec(&vec![Value::Object(task("t2", "p2", "todo").0)]).expect("p2 rows"),
    )
    .expect("write p2 tasks");

    let service = build_service(context, &executor);

    assert_eq!(
        service.project_of("t1").expect("scan"),
        Some("p1".to_string()),
        "the row's own project, found through the project list"
    );
    assert_eq!(
        service.project_of("t2").expect("scan"),
        Some("p2".to_string()),
        "and not the first project it happened to look at"
    );
    assert_eq!(
        service.project_of("ghost").expect("scan"),
        None,
        "no row is an honest none, never a guess — writing one task into two projects' files \
         is the divergence ADR-027 refuses"
    );

    // The scan is O(projects) file reads, so the answer is remembered; that is only legal
    // because `delete` is the one operation that can retire an id.
    service
        .delete("p2", &["t2".to_string()])
        .expect("delete t2");
    assert_eq!(
        service.project_of("t2").expect("rescan"),
        None,
        "a deleted task must not keep its router entry, or a late report re-creates the row's \
         home in a table that no longer holds it"
    );
    cleanup(root);
}

/// ADR-035's announcement rule lives in core so no adapter can re-derive it: admitted ⇒
/// announce (even with nothing changed), dropped ⇒ silent. A sink that decided this itself
/// would be the second authority ADR-023 exists to prevent.
#[test]
fn the_announcement_rule_is_part_of_the_outcome_not_of_the_sink() {
    let admitted_unchanged = EventOutcome::Applied {
        status: TaskStatus::Running,
        changed: false,
        failure_reason: None,
    };
    assert_eq!(
        admitted_unchanged.notification_for("t1"),
        Some(TaskNotification::status("t1", TaskStatus::Running)),
        "25 of the 28 legacy sites emit unconditionally today; folding changed:false into \
         'say nothing' would be a behaviour change inside a handover"
    );
    let admitted_failed = EventOutcome::Applied {
        status: TaskStatus::Failed,
        changed: true,
        failure_reason: Some("the process exited with code 2".into()),
    };
    assert_eq!(
        admitted_failed.notification_for("t1"),
        Some(TaskNotification::status_with_reason(
            "t1",
            TaskStatus::Failed,
            "the process exited with code 2"
        )),
        "and the reason arrives already attached, not reconstructed downstream"
    );
    assert_eq!(
        EventOutcome::Dropped("no live run for this task").notification_for("t1"),
        None,
        "the only silence is the one core decided, with a reason a host can log"
    );
}

#[test]
fn a_rename_buys_a_revision_and_an_empty_name_drops_the_key() {
    let (root, context, executor) = harness("meta-rename", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);

    let applied = service
        .rename("p1", "t1", "重构登录")
        .expect("rename is a core-side read-modify-write (ADR-022)");
    assert!(applied.changed, "a real edit changed the row");
    assert!(applied.revision.is_some(), "and it bought a revision");
    assert_eq!(rows(&tasks_file)[0].get("name"), Some(&json!("重构登录")));

    // 空串不是名字：键要消失，而不是留下一个清不掉的空标签（与旧客户端 renameTaskInList 一致）。
    let cleared = service
        .rename("p1", "t1", "")
        .expect("clearing is a write too");
    assert!(cleared.changed);
    assert_eq!(
        rows(&tasks_file)[0].get("name"),
        None,
        "the key must go away entirely"
    );
    cleanup(root);
}

#[test]
fn starring_twice_flips_the_flag_and_advances_the_revision_both_times() {
    let (root, context, executor) = harness("meta-star", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);

    let first = service.toggle_star("p1", "t1").expect("star");
    assert_eq!(rows(&tasks_file)[0].get("starred"), Some(&json!(true)));
    let second = service.toggle_star("p1", "t1").expect("unstar");
    // 两次都推进 revision：unstar 是一次真实写入，不是"撤销回原字节"。
    assert_eq!(rows(&tasks_file)[0].get("starred"), Some(&json!(false)));
    assert_ne!(
        first.revision, second.revision,
        "an unstar must not reuse the star's revision"
    );
    cleanup(root);
}

#[test]
fn archiving_a_batch_writes_once_and_re_archiving_writes_nothing() {
    let (root, context, executor) = harness(
        "meta-archive",
        &[task("t1", "p1", "done"), task("t2", "p1", "failed")],
    );
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);
    let ids = vec!["t1".to_string(), "t2".to_string()];

    let applied = service.archive("p1", &ids).expect("archive both");
    assert!(
        applied.changed && applied.revision.is_some(),
        "the first archive is a real write"
    );
    let archived = rows(&tasks_file);
    assert!(
        archived.iter().all(|row| row.get("archivedAt").is_some()),
        "one intent covers every named row: {archived:?}"
    );

    let again = service
        .archive("p1", &ids)
        .expect("re-archive is not an error");
    assert!(
        !again.changed && again.revision.is_none(),
        "an already-archived batch must not burn a revision"
    );
    assert_eq!(rows(&tasks_file), archived, "and must not touch the bytes");

    service.unarchive("p1", &ids).expect("unarchive");
    assert!(
        rows(&tasks_file)
            .iter()
            .all(|row| row.get("archivedAt").is_none()),
        "unarchive clears every named row"
    );
    cleanup(root);
}

#[test]
fn a_metadata_edit_on_a_missing_row_is_an_error_not_a_quiet_no_change() {
    let (root, context, executor) = harness("meta-missing", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);

    let error = service
        .rename("p1", "ghost", "x")
        .expect_err("editing a row that is not there must fail loudly");
    assert!(
        matches!(error, TaskError::NotFound { .. }),
        "expected NotFound, got {error:?}"
    );
    // 批量同理：只归档一半比全不归档更难解释。
    let partial = service
        .archive("p1", &["t1".to_string(), "ghost".to_string()])
        .expect_err("one missing id rejects the whole intent");
    assert!(
        matches!(partial, TaskError::NotFound { .. }),
        "expected NotFound, got {partial:?}"
    );
    assert!(
        rows(&tasks_file)[0].get("archivedAt").is_none(),
        "the rejected batch must not have written the row that did exist"
    );
    cleanup(root);
}

#[test]
fn a_read_only_host_cannot_rename_even_though_the_op_is_public() {
    let (root, _writer, executor) = harness("meta-readonly", &[task("t1", "p1", "todo")]);
    let service = build_service(reader(&root), &executor);
    let tasks_file = tasks_json(&root);

    let error = service
        .rename("p1", "t1", "不该落盘")
        .expect_err("ADR-021: no writer role, no write");
    assert!(
        matches!(error, TaskError::Storage(_)),
        "expected the same storage refusal the observation path gives, got {error:?}"
    );
    assert_eq!(
        rows(&tasks_file)[0].get("name"),
        None,
        "the op being callable is not the same as the row being written"
    );
    cleanup(root);
}

#[test]
fn deciding_an_intent_returns_the_new_rows_without_touching_the_file() {
    // ADR-035 搬到 mutation 上：非递交写权的平台上，core 照样决定结果，保存者仍是宿主。
    let (root, context, executor) = harness("intent-decide", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);
    let bytes_before = std::fs::read(&tasks_file).expect("seeded bytes");

    let decided = service
        .decide_intent(
            "p1",
            &TaskIntent::Rename {
                task_id: "t1".into(),
                name: "决定但未落盘".into(),
                expect: None,
            },
        )
        .expect("deciding needs no lease");

    assert!(decided.changed, "the reducer really did change the row");
    assert!(
        decided.revision.is_none() && decided.fingerprint.is_none(),
        "no write means no revision — and that is a different answer from 'nothing changed'"
    );
    assert_eq!(
        decided.records[0].get("name"),
        Some(&json!("决定但未落盘")),
        "the caller must get the decided row back, not just a verdict"
    );
    assert_eq!(
        std::fs::read(&tasks_file).expect("still readable"),
        bytes_before,
        "byte-identical: a decision is not a save"
    );
    cleanup(root);
}

#[test]
fn a_reader_can_decide_an_intent_it_cannot_apply() {
    let (root, _writer, executor) = harness("intent-reader", &[task("t1", "p1", "todo")]);
    let service = build_service(reader(&root), &executor);
    let tasks_file = tasks_json(&root);
    let star = TaskIntent::ToggleStar {
        task_id: "t1".into(),
    };

    let decided = service
        .decide_intent("p1", &star)
        .expect("ADR-021 withholds the writer role, not the decision");
    assert_eq!(decided.records[0].get("starred"), Some(&json!(true)));

    let refused = service
        .apply_intent("p1", &star)
        .expect_err("but the same intent must not be written here");
    assert!(
        matches!(refused, TaskError::Storage(_)),
        "expected the same storage refusal as every other write, got {refused:?}"
    );
    assert_eq!(
        rows(&tasks_file)[0].get("starred"),
        None,
        "the refused path wrote nothing, including no ledger"
    );
    cleanup(root);
}

#[test]
fn an_intent_and_the_named_op_it_carries_produce_the_same_bytes() {
    // 宿主将来只有一个入口（apply_intent）。它必须与逐 op 入口逐字节等价，否则"一条命令
    // 一个 op"的迁移会把同一件事做出两种结果，而对账只能看见其中一种。
    let (root_a, context_a, executor_a) = harness("intent-equiv-a", &[task("t1", "p1", "todo")]);
    let service_a = build_service(context_a, &executor_a);
    service_a
        .rename("p1", "t1", "同一个改动")
        .expect("named op");
    service_a
        .archive("p1", &["t1".to_string()])
        .expect("named batch op");

    let (root_b, context_b, executor_b) = harness("intent-equiv-b", &[task("t1", "p1", "todo")]);
    let service_b = build_service(context_b, &executor_b);
    service_b
        .apply_intent(
            "p1",
            &TaskIntent::Rename {
                task_id: "t1".into(),
                name: "同一个改动".into(),
                expect: None,
            },
        )
        .expect("intent");
    service_b
        .apply_intent(
            "p1",
            &TaskIntent::Archive {
                task_ids: vec!["t1".to_string()],
            },
        )
        .expect("intent");

    let mut before = rows(&tasks_json(&root_a));
    let mut after = rows(&tasks_json(&root_b));
    for row in before.iter_mut().chain(after.iter_mut()) {
        if let Some(obj) = row.as_object_mut() {
            // createdAt/completedAt 由时钟固定，两边同种子；这里只比"改了什么"。
            obj.remove("archivedAt");
        }
    }
    assert_eq!(before, after, "two entry points, one result");
    // 但两边都必须真的写了 archivedAt，否则把键删掉也能"相等"。
    assert!(
        rows(&tasks_json(&root_a))[0].get("archivedAt").is_some(),
        "the equivalence assertion above is only meaningful if the archive really landed"
    );
    cleanup(root_a);
    cleanup(root_b);
}

/// ADR-022 决策 1 的下一步是"JS 交 `op` + 载荷"，所以 `TaskIntent` 的线上形状从此是契约而不是
/// 实现细节：钉在这里，总比让保存队列在运行时吃一个反序列化错误再让人去猜约定。
#[test]
fn an_intent_crosses_the_wire_as_op_plus_camel_case_payload() {
    let encoded = serde_json::to_value(TaskIntent::Rename {
        task_id: "t1".to_string(),
        name: "重命名".to_string(),
        expect: None,
    })
    .expect("serializable");
    assert_eq!(
        encoded,
        json!({"op": "rename", "taskId": "t1", "name": "重命名"}),
        "`op` is the discriminator and fields are camelCase — the vocabulary the queue will send"
    );

    let batch: TaskIntent =
        serde_json::from_value(json!({"op": "unarchive", "taskIds": ["t1", "t2"]}))
            .expect("deserializable");
    assert_eq!(
        batch,
        TaskIntent::Unarchive {
            task_ids: vec!["t1".to_string(), "t2".to_string()]
        },
        "a batch intent decodes from the same vocabulary it encodes to"
    );

    // 反例：蛇形 op 名、Pascal 判别式、蛇形字段都必须被拒——否则宿主与 core 会各自发明一套，
    // 而第三套是 Swift 那一边。
    for wrong in [
        json!({"op": "toggle_star", "taskId": "t1"}),
        json!({"op": "ToggleStar", "taskId": "t1"}),
        json!({"op": "toggleStar", "task_id": "t1"}),
    ] {
        assert!(
            serde_json::from_value::<TaskIntent>(wrong.clone()).is_err(),
            "{wrong} must not decode: the vocabulary is one shape, not three"
        );
    }
}

/// ADR-037 词汇表的"一处定义＋门禁可数"那一半：13 条、9/4 的分类、以及**三条 op 名**
/// （serde 判别式、手写的 `op_name`、`TASK_INTENT_VOCABULARY` 表）必须是同一个拼法。
/// 三条各自长出来的那天，就是"宿主一套、core 一套、Swift 一套"重演的那天。
#[test]
fn the_intent_vocabulary_is_thirteen_names_nine_metadata_and_four_state() {
    let samples: Vec<TaskIntent> = vec![
        TaskIntent::Rename {
            task_id: "t".into(),
            name: "n".into(),
            expect: None,
        },
        TaskIntent::ToggleStar {
            task_id: "t".into(),
        },
        TaskIntent::Archive {
            task_ids: vec!["t".into()],
        },
        TaskIntent::Unarchive {
            task_ids: vec!["t".into()],
        },
        TaskIntent::AttachSession {
            task_id: "t".into(),
            family: ProtocolFamily::Claude,
            session_agent: "claude".into(),
            session_id: None,
            session_path: None,
        },
        TaskIntent::SetWorktree {
            task_id: "t".into(),
            path: "/wt".into(),
            branch: "b".into(),
            base_branch: "main".into(),
        },
        TaskIntent::DiscardWorktree {
            task_id: "t".into(),
        },
        TaskIntent::RecordDiffStats {
            task_id: "t".into(),
            additions: 1,
            deletions: 2,
        },
        TaskIntent::Create {
            id: "n".into(),
            prompt: "p".into(),
            agent: "claude".into(),
            permission_mode: "ask".into(),
            immediate: false,
            selected_model: None,
            reasoning_effort: None,
            speed: None,
            dsh_agent_preset: None,
        },
        TaskIntent::Delete {
            task_ids: vec!["t".into()],
        },
        TaskIntent::NormalizeAfterBoot {
            live_task_ids: vec!["t".into()],
        },
        TaskIntent::SwitchConfig {
            task_id: "t".into(),
            agent: "codex".into(),
            selected_model: None,
            reasoning_effort: None,
            speed: None,
            permission_mode: "ask".into(),
            previous_session: SessionSnapshot::default(),
            resolved_session: SessionSnapshot::default(),
        },
        TaskIntent::ResumeWithSession {
            task_id: "t".into(),
            agent: "codex".into(),
            family: ProtocolFamily::Codex,
            session_id: None,
            session_path: None,
        },
        TaskIntent::EditTodo {
            task_id: "t".into(),
            prompt: "p".into(),
            agent: "claude".into(),
            permission_mode: "ask".into(),
        },
    ];
    assert_eq!(
        samples.len(),
        14,
        "词汇表规模是 14 条（ADR-037 的 13 + Q72 修正案 EditTodo），钉死"
    );
    let metadata = TASK_INTENT_VOCABULARY
        .iter()
        .filter(|(_, kind)| *kind == TaskIntentKind::Metadata)
        .count();
    let state = TASK_INTENT_VOCABULARY
        .iter()
        .filter(|(_, kind)| *kind == TaskIntentKind::State)
        .count();
    assert_eq!(
        (metadata, state),
        (10, 4),
        "ADR-026 的分账是元数据 10 / 状态 4；任何一侧偷偷多一条都会让'冲突时能不能重放'变成口头账"
    );
    let mut names: Vec<&str> = TASK_INTENT_VOCABULARY
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), total, "表里不许有重名");

    for sample in &samples {
        let encoded = serde_json::to_value(sample).expect("serializable");
        let wire_op = encoded
            .get("op")
            .and_then(Value::as_str)
            .expect("op discriminator");
        assert_eq!(
            wire_op,
            sample.op_name(),
            "serde 判别式与 op_name() 必须同拼法（{sample:?}）"
        );
        assert!(
            TASK_INTENT_VOCABULARY
                .iter()
                .any(|(name, kind)| *name == wire_op && kind == &sample.kind()),
            "每个变体都必须在表里、且分类一致（{wire_op}）"
        );
    }
    assert_eq!(
        samples
            .iter()
            .filter(|s| s.kind() == TaskIntentKind::Metadata)
            .count(),
        10,
        "14 个样本按 kind() 数出来也得是 10/4"
    );
    // 契约那一头的同一份事实：protocol 的名单（`contracts --write` 的拼写来源）必须与
    // core 的表逐条同名同分类——两条名单各长一份的那天，就是 TS/Swift 拿到第三份的那天。
    let protocol_list = aeroric_protocol::task_intents();
    assert_eq!(protocol_list.len(), TASK_INTENT_VOCABULARY.len());
    for (name, kind) in TASK_INTENT_VOCABULARY {
        let wire_kind = match kind {
            TaskIntentKind::Metadata => "metadata",
            TaskIntentKind::State => "state",
        };
        assert!(
            protocol_list
                .iter()
                .any(|(pn, pk)| *pn == name && *pk == wire_kind),
            "core 的 ({name}, {wire_kind}) 在 protocol 名单里找不到同名同分类的条目"
        );
    }
}

/// #3/#20：`Create` 意图走的必须是具名 `create` 的**同一个**行构造器——三族钉法、
/// 空 prompt 用 agent 名、`immediate ⇒ pending`。两份构造就是两种未来。
#[test]
fn a_create_intent_pins_three_families_at_the_front_of_the_list() {
    let (root, context, executor) = harness("intent-create", &[task("t1", "p1", "todo")]);
    let service = build_service(context, &executor);
    let applied = service
        .apply_intent(
            "p1",
            &TaskIntent::Create {
                id: "new1".into(),
                prompt: "  ".into(),
                agent: "claude_gpt55".into(),
                permission_mode: "ask".into(),
                immediate: false,
                selected_model: Some("m".into()),
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
            },
        )
        .expect("create intent");
    assert!(applied.changed);
    let rows = rows(&tasks_json(&root));
    assert_eq!(rows.len(), 2);
    let fresh = &rows[0];
    assert_eq!(fresh["id"], json!("new1"));
    assert_eq!(fresh["status"], json!("todo"));
    assert_eq!(
        fresh["name"],
        json!("claude_gpt55"),
        "空 prompt ⇒ agent 名当标签"
    );
    assert_eq!(fresh["launchFamily"], json!("codex"));
    assert_eq!(fresh["sessionFamily"], json!("claude"));
    assert_eq!(fresh["resumeFamily"], json!("codex"));
    assert_eq!(fresh["selectedModel"], json!("m"));

    let bytes_before = std::fs::read(tasks_json(&root)).expect("written");
    let refused = service
        .apply_intent(
            "p1",
            &TaskIntent::Create {
                id: "new1".into(),
                prompt: "again".into(),
                agent: "claude".into(),
                permission_mode: "ask".into(),
                immediate: false,
                selected_model: None,
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
            },
        )
        .expect_err("重复 id 必须拒绝，不是静默第二条");
    assert!(matches!(refused, TaskError::DuplicateId(_)), "{refused:?}");
    assert_eq!(
        std::fs::read(tasks_json(&root)).expect("untouched"),
        bytes_before,
        "被拒绝的建行一格 revision 都不烧"
    );
    cleanup(root);
}

/// ADR-037 后果 1：**删空 ⇒ 删文件**。删项目今天就靠这条；core 的写者路径没有它，
/// "core 是唯一发射者"就会变成"core 是唯一留下孤魂 tasks.json 的人"。
#[test]
fn deleting_the_last_row_removes_the_file_and_a_new_create_reopens_it() {
    let (root, context, executor) = harness("intent-delete-empty", &[task("t1", "p1", "done")]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);
    let applied = service
        .apply_intent(
            "p1",
            &TaskIntent::Delete {
                task_ids: vec!["t1".to_string()],
            },
        )
        .expect("delete intent");
    assert!(applied.changed);
    assert!(applied.revision.is_some(), "删空是一次真写");
    assert!(
        applied.fingerprint.is_none(),
        "覆盖面发布为缺席——文件不在了，下一写的合法基线就是'不存在'"
    );
    assert!(!tasks_file.exists(), "删空 ⇒ 删文件");

    let loaded = service.load("p1").expect("load after removal");
    assert!(loaded.records.is_empty());

    service
        .apply_intent(
            "p1",
            &TaskIntent::Create {
                id: "t2".into(),
                prompt: "再来".into(),
                agent: "claude".into(),
                permission_mode: "ask".into(),
                immediate: false,
                selected_model: None,
                reasoning_effort: None,
                speed: None,
                dsh_agent_preset: None,
            },
        )
        .expect("creating after an emptied deletion is a fresh list, not a resurrected one");
    assert_eq!(rows(&tasks_file).len(), 1);
    cleanup(root);
}

/// `Delete` 意图与具名 `delete` 必须共享**同一套规则**（跳收藏、拒在飞）——两份规则
/// 早晚分叉，而 JS 迁移后宿主只会看见其中一份。
#[test]
fn a_delete_intent_and_the_named_delete_produce_the_same_bytes() {
    let seed = vec![
        {
            let mut starred = task("t1", "p1", "done").0;
            starred.insert("starred".into(), json!(true));
            record(Value::Object(starred))
        },
        task("t2", "p1", "done"),
    ];
    let (root_a, context_a, executor_a) = harness("intent-del-a", &seed);
    build_service(context_a, &executor_a)
        .delete("p1", &["t1".to_string(), "t2".to_string()])
        .expect("named delete");
    let (root_b, context_b, executor_b) = harness("intent-del-b", &seed);
    let applied = build_service(context_b, &executor_b)
        .apply_intent(
            "p1",
            &TaskIntent::Delete {
                task_ids: vec!["t1".to_string(), "t2".to_string()],
            },
        )
        .expect("delete intent");
    assert_eq!(
        rows(&tasks_json(&root_a)),
        rows(&tasks_json(&root_b)),
        "一条规则，两个入口，一份字节"
    );
    assert_eq!(rows(&tasks_json(&root_b)).len(), 1, "收藏行被规则跳过");
    assert!(applied.changed);
    cleanup(root_a);
    cleanup(root_b);
}

/// #11：启动修复。宿主只交"哪些 id 还活着"这一个事实；四条判据都在 core，
/// 且第二次跑必须**零写**——重启不烧 revision 是旧客户端今天的行为。
#[test]
fn normalizing_after_boot_applies_the_four_client_rules_and_is_idempotent() {
    let mut dsh = task("t-dsh", "p1", "done").0;
    dsh.insert("agent".into(), json!("dsh"));
    dsh.insert("speed".into(), json!("fast"));
    let (root, context, executor) = harness(
        "intent-boot",
        &[
            task("t-run", "p1", "running"),
            task("t-dead", "p1", "running"),
            task("t-int", "p1", "interrupted"),
            task("t-det", "p1", "detached"),
            record(Value::Object(dsh)),
        ],
    );
    let service = build_service(context, &executor);
    let live = || {
        vec![
            "t-run".to_string(),
            "t-int".to_string(),
            "t-det".to_string(),
        ]
    };
    let applied = service
        .apply_intent(
            "p1",
            &TaskIntent::NormalizeAfterBoot {
                live_task_ids: live(),
            },
        )
        .expect("normalize");
    assert!(applied.changed);
    let rows = rows(&tasks_json(&root));
    let row = |id: &str| {
        rows.iter()
            .find(|r| r["id"] == json!(id))
            .expect("seeded")
            .clone()
    };
    assert_eq!(
        row("t-run")["status"],
        json!("detached"),
        "活孩子 ⇒ detached"
    );
    assert!(row("t-run")
        .get("attentionRequestedAt")
        .is_some_and(Value::is_number));
    assert_eq!(
        row("t-dead")["status"],
        json!("interrupted"),
        "没孩子 ⇒ interrupted"
    );
    assert_eq!(
        row("t-int")["status"],
        json!("detached"),
        "interrupted+活孩子 ⇒ detached"
    );
    assert_eq!(
        row("t-det")["status"],
        json!("detached"),
        "已是 detached 的行原样，不补 attentionRequestedAt"
    );
    assert!(row("t-det").get("attentionRequestedAt").is_none());
    assert_eq!(
        row("t-dsh")["speed"],
        json!("standard"),
        "dsh 的 fast 清成 standard"
    );

    let bytes = std::fs::read(tasks_json(&root)).expect("written");
    let again = service
        .apply_intent(
            "p1",
            &TaskIntent::NormalizeAfterBoot {
                live_task_ids: live(),
            },
        )
        .expect("second pass");
    assert!(!again.changed, "重复启动零写");
    assert_eq!(std::fs::read(tasks_json(&root)).expect("same"), bytes);
    cleanup(root);
}

/// #12：三路合并。**未被点名的键一个不碰**——权威行上别人刚写进去的字段不许被
/// 宿主内存里那份旧行抹掉（这正是整表快照的死法）；配置五项与两个清空则无条件落。
#[test]
fn a_switch_config_intent_merges_only_the_delta_onto_the_authoritative_row() {
    let mut seed = task("t1", "p1", "running").0;
    seed.insert("selectedModel".into(), json!("stale-model"));
    seed.insert("attentionRequestedAt".into(), json!(123));
    seed.insert("failureReason".into(), json!("boom"));
    seed.insert("name".into(), json!("别的窗口刚改的名"));
    let (root, context, executor) = harness("intent-switch", &[record(Value::Object(seed))]);
    let service = build_service(context, &executor);
    let previous = SessionSnapshot::default();
    let resolved = SessionSnapshot {
        claude_session_id: Some("s-new".into()),
        session_agent: Some("claude".into()),
        session_family: Some("claude".into()),
        ..Default::default()
    };
    let applied = service
        .apply_intent(
            "p1",
            &TaskIntent::SwitchConfig {
                task_id: "t1".into(),
                agent: "codex".into(),
                selected_model: None,
                reasoning_effort: None,
                speed: None,
                permission_mode: "bypass".into(),
                previous_session: previous,
                resolved_session: resolved,
            },
        )
        .expect("switch write");
    assert!(applied.changed);
    let row = &rows(&tasks_json(&root))[0];
    assert_eq!(
        row["claudeSessionId"],
        json!("s-new"),
        "previous→resolved 的变化被应用"
    );
    assert_eq!(row["sessionAgent"], json!("claude"));
    assert_eq!(row["sessionFamily"], json!("claude"));
    assert_eq!(row["agent"], json!("codex"));
    assert_eq!(row["permissionMode"], json!("bypass"));
    assert_eq!(row["status"], json!("pending"));
    assert!(
        row.get("selectedModel").is_none(),
        "values.selectedModel 是 undefined ⇒ 键被删，与 JS 落盘一致"
    );
    assert!(row.get("attentionRequestedAt").is_none());
    assert!(row.get("failureReason").is_none());
    assert_eq!(
        row["name"],
        json!("别的窗口刚改的名"),
        "未被点名的键不许被这次写抹掉"
    );
    cleanup(root);
}

/// #9：会话 + 状态一次落定，且与 #21 共用清三设一；重复投递零写。
#[test]
fn resume_with_session_sets_one_family_and_pending_clearing_the_two_fields() {
    let mut seed = task("t1", "p1", "done").0;
    seed.insert("claudeSessionPath".into(), json!("/old.jsonl"));
    seed.insert("codexSessionId".into(), json!("stale-other"));
    seed.insert("attentionRequestedAt".into(), json!(5));
    seed.insert("failureReason".into(), json!("boom"));
    let (root, context, executor) = harness("intent-resume-write", &[record(Value::Object(seed))]);
    let service = build_service(context, &executor);
    let intent = TaskIntent::ResumeWithSession {
        task_id: "t1".into(),
        agent: "codex".into(),
        family: ProtocolFamily::Codex,
        session_id: Some("s1".into()),
        session_path: Some("/new.jsonl".into()),
    };
    let applied = service
        .apply_intent("p1", &intent)
        .expect("resume write intent");
    assert!(applied.changed);
    let row = &rows(&tasks_json(&root))[0];
    assert_eq!(row["codexSessionId"], json!("s1"));
    assert_eq!(row["codexSessionPath"], json!("/new.jsonl"));
    assert!(
        row.get("claudeSessionPath").is_none(),
        "清三设一：claude 的路径不许活过这次挂接"
    );
    assert_eq!(row["sessionCodexLike"], json!(true));
    assert_eq!(row["sessionFamily"], json!("codex"));
    assert_eq!(row["agent"], json!("codex"));
    assert_eq!(row["status"], json!("pending"));
    assert!(row.get("attentionRequestedAt").is_none());
    assert!(row.get("failureReason").is_none());

    let bytes = std::fs::read(tasks_json(&root)).expect("written");
    let again = service
        .apply_intent("p1", &intent)
        .expect("second pass decodes");
    assert!(!again.changed, "同一次续跑写两遍是烧 revision，不是幂等");
    assert_eq!(std::fs::read(tasks_json(&root)).expect("same"), bytes);
    cleanup(root);
}

/// 新四条的线上形状（ADR-022 决策 1 的下一步就是 JS 按这个发）。
#[test]
fn the_new_intents_cross_the_wire_with_their_vocabulary_names() {
    let created = serde_json::to_value(TaskIntent::Create {
        id: "n1".into(),
        prompt: "p".into(),
        agent: "claude".into(),
        permission_mode: "ask".into(),
        immediate: true,
        selected_model: None,
        reasoning_effort: None,
        speed: None,
        dsh_agent_preset: None,
    })
    .expect("serializable");
    assert_eq!(
        created,
        json!({"op": "create", "id": "n1", "prompt": "p", "agent": "claude",
               "permissionMode": "ask", "immediate": true}),
        "None 的可选字段整键缺席，不许发 null——JS 发的就是没这个键"
    );

    let deleted: TaskIntent =
        serde_json::from_value(json!({"op": "delete", "taskIds": ["t1"]})).expect("decodes");
    assert_eq!(deleted.kind(), TaskIntentKind::State);

    let normalized: TaskIntent = serde_json::from_value(json!({
        "op": "normalizeAfterBoot", "liveTaskIds": ["t1", "t2"]
    }))
    .expect("decodes");
    assert_eq!(
        normalized,
        TaskIntent::NormalizeAfterBoot {
            live_task_ids: vec!["t1".to_string(), "t2".to_string()]
        }
    );

    let resumed: TaskIntent = serde_json::from_value(json!({
        "op": "resumeWithSession", "taskId": "t1", "agent": "codex", "family": "codex",
        "sessionId": "s1"
    }))
    .expect("decodes");
    assert_eq!(
        resumed,
        TaskIntent::ResumeWithSession {
            task_id: "t1".into(),
            agent: "codex".into(),
            family: ProtocolFamily::Codex,
            session_id: Some("s1".into()),
            session_path: None,
        }
    );

    assert!(
        serde_json::from_value::<TaskIntent>(json!({
            "op": "switchconfig", "taskId": "t1", "agent": "codex",
            "permissionMode": "ask", "previousSession": {}, "resolvedSession": {}
        }))
        .is_err(),
        "蛇形首字母小写拼错了族名也必须被拒——词汇表只有一种拼法"
    );
}

// ---- §12 步骤一：`Persist::ViaHost` 的宿主桥（core 决定、宿主存字节）----

#[derive(Default)]
struct CapturingSaver {
    saved: std::sync::Mutex<Vec<(String, Vec<Value>)>>,
}

impl aeroric_core::context::HostTaskSaver for CapturingSaver {
    fn save_task_records(&self, project_id: &str, records: &[Value]) -> Result<(), String> {
        self.saved
            .lock()
            .expect("saver mutex")
            .push((project_id.to_string(), records.to_vec()));
        Ok(())
    }
}

fn last_saved(saver: &CapturingSaver) -> Vec<Value> {
    saver
        .saved
        .lock()
        .expect("saver mutex")
        .last()
        .map(|(_, rows)| rows.clone())
        .unwrap_or_default()
}

/// 桥上的意图写：决定生效、字节经桥、盘上的旧文件一格不动、`revision: None`
/// ——宿主存好了与 core 发布了一格修订是两件事（§12）。
#[test]
fn a_via_host_intent_stores_through_the_bridge_and_publishes_no_revision() {
    let (root, _writer, executor) = harness("bridge-intent", &[task("t1", "p1", "todo")]);
    let saver = Arc::new(CapturingSaver::default());
    let bridge: Arc<dyn aeroric_core::context::HostTaskSaver> = saver.clone();
    let service = build_service(reader(&root).with_task_saver(bridge), &executor);
    let tasks_file = tasks_json(&root);
    let bytes_before = std::fs::read(&tasks_file).expect("seeded");

    let applied = service
        .apply_intent_via_host(
            "p1",
            &TaskIntent::Rename {
                task_id: "t1".into(),
                name: "桥上的名字".into(),
                expect: None,
            },
        )
        .expect("the bridge stores what core decided");

    assert!(applied.changed);
    assert!(
        applied.revision.is_none() && applied.fingerprint.is_none(),
        "a ViaHost write is not a publication; claiming a revision would lie"
    );
    let stored = last_saved(&saver);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0]["name"], json!("桥上的名字"));
    assert_eq!(stored[0]["projectId"], json!("p1"));
    assert_eq!(
        std::fs::read(&tasks_file).expect("untouched"),
        bytes_before,
        "core's own file must not move on the bridge path"
    );
    cleanup(root);
}

/// 没装桥就不是"静默不写"而是错误——ADR-021 的平台政策在宿主，机制不许假装成功。
#[test]
fn a_via_host_write_without_a_bridge_is_a_refusal_not_a_silent_skip() {
    let (root, _writer, executor) = harness("bridge-absent", &[task("t1", "p1", "todo")]);
    let service = build_service(reader(&root), &executor);
    let refused = service
        .apply_intent_via_host(
            "p1",
            &TaskIntent::Rename {
                task_id: "t1".into(),
                name: "没人存".into(),
                expect: None,
            },
        )
        .expect_err("no saver means no store, and that is an error");
    assert!(
        matches!(&refused, TaskError::Storage(inner) if inner.to_string().contains("no host task saver")),
        "{refused:?}"
    );
    cleanup(root);
}

/// 桥上的 launch：pending 先经桥存下，进程后起（`persist_before_launch` 的同步语义在
/// 桥这一侧天然成立——桥就是同步写），而 core 的文件一格不动。
#[test]
fn a_via_host_launch_stores_pending_before_the_executor_spawns() {
    let (root, _writer, executor) = harness("bridge-launch", &[task("t1", "p1", "todo")]);
    let saver = Arc::new(CapturingSaver::default());
    let bridge: Arc<dyn aeroric_core::context::HostTaskSaver> = saver.clone();
    let service = build_service(reader(&root).with_task_saver(bridge), &executor);
    let project = local_project("p1", "/repo");

    let applied = service
        .start_via_host(
            &project,
            "t1",
            "op-bridge-1",
            false,
            None,
            false,
            Attachments::default(),
            HostWindowFacts::default(),
        )
        .expect("a reader may launch as long as the host stores its decision");

    assert!(applied.changed);
    assert!(
        applied.revision.is_none(),
        "same honesty as the intent path"
    );
    let stored = last_saved(&saver);
    assert_eq!(
        stored[0]["status"],
        json!("pending"),
        "begin() 的 pending 写在 spawn 前经桥落定"
    );
    assert_eq!(
        stored[0]["generation"],
        json!("1"),
        "generation 戳记与 Writer 路径同源"
    );
    let calls = executor.calls();
    assert_eq!(
        calls.first().map(|call| call.what),
        Some("launch"),
        "the spawn happened - and only after the bridge call above"
    );
    let rows = rows(&tasks_json(&root));
    assert_eq!(
        rows[0]["status"],
        json!("todo"),
        "core's file is untouched; the host holds the truth"
    );
    cleanup(root);
}

/// 桥上的观察（#15 接线的主形态）：决策、宣告、字节交桥——与 `decide_observation` 的
/// "只决定"和 `observe` 的"自己写"三态分明。
#[test]
fn a_via_host_observation_announces_and_hands_the_bytes_to_the_bridge() {
    let (root, _writer, executor) = harness("bridge-observe", &[task("t1", "p1", "running")]);
    let saver = Arc::new(CapturingSaver::default());
    let bridge: Arc<dyn aeroric_core::context::HostTaskSaver> = saver.clone();
    let service = build_service(reader(&root).with_task_saver(bridge), &executor);

    let (outcome, applied) = service
        .observe_via_host(
            "p1",
            TaskEvent::Exit {
                task_id: "t1".into(),
                generation: 0,
                code: Some(0),
            },
        )
        .expect("the decision is a reader's right; the bridge is its hand");

    match outcome {
        EventOutcome::Applied { status, .. } => assert_eq!(status, TaskStatus::Done),
        other => panic!("an exit with code 0 settles: {other:?}"),
    }
    assert!(applied.changed && applied.revision.is_none());
    let stored = last_saved(&saver);
    assert_eq!(stored[0]["status"], json!("done"));
    assert_eq!(
        rows(&tasks_json(&root))[0]["status"],
        json!("running"),
        "core's copy still shows the pre-event truth"
    );
    cleanup(root);
}
/// 桥上的续跑（§12 步骤②c），并且把一条边界钉在盘上：`resume()` **只写状态**，宿主解析出来的
/// 那个 session id **不由它落盘**——那是 `ResumeWithSession` 那条复合意图的职责（Q70：一次复合写＝
/// 一条原子意图）。这条断言写反过一次（本用例第一版就断言了 `claudeSessionId` 会经桥落定，红在
/// `Null`），所以它现在的价值是双向的：既防接线那一刀有人以为 spawn 顺手把会话写了而漏发意图，
/// 也防他把会话写成第二段落盘、把 `ResumeWithSession` 的原子性拆掉。
#[test]
fn a_via_host_resume_stores_the_pending_and_leaves_the_session_to_the_intent() {
    let (root, _writer, executor) = harness("bridge-resume", &[task("t1", "p1", "interrupted")]);
    let saver = Arc::new(CapturingSaver::default());
    let bridge: Arc<dyn aeroric_core::context::HostTaskSaver> = saver.clone();
    let service = build_service(reader(&root).with_task_saver(bridge), &executor);
    let project = local_project("p1", "/repo");

    let applied = service
        .resume_via_host(&project, "t1", "c-77", "op-bridge-r", true, None)
        .expect("a reader may resume as long as the host stores its decision");

    assert!(applied.changed);
    assert!(
        applied.revision.is_none(),
        "the bridge stores bytes; it buys no cover revision"
    );
    let batches = saver.saved.lock().expect("saver mutex").len();
    assert_eq!(
        batches, 1,
        "one decision, one store — not a session write and a status write"
    );
    let stored = last_saved(&saver);
    assert_eq!(stored[0]["status"], json!("pending"));
    assert_eq!(
        stored[0].get("claudeSessionId"),
        None,
        "解析出的 session 不由 `resume()` 落盘——它是 `ResumeWithSession` 的职责（Q70）。\
         这条为 `None` 而存在：写成 `Some` 就是在这里偷偷开出会话的第二段落盘"
    );
    assert_eq!(
        executor.calls().first().map(|call| call.what),
        Some("resume"),
        "the reconnect was asked for, and only after that single write"
    );
    assert_eq!(
        rows(&tasks_json(&root))[0]["status"],
        json!("interrupted"),
        "core's own file is untouched; the host holds the truth"
    );
    cleanup(root);
}

/// 守卫不因为改走桥就松动：空 session 在桥这一侧同样是"不许把旧对话悄悄换成一段新的"
/// （`resume` 里那条 `NoSession` 与 `claudeSessionId` 的来历是同一个理由）。这条用例的**后半句**
/// 才是它存在的全部意义——桥一次都不许被调用。只断言"错误传出去了"会放过另一种事故：错误传出去
/// 之前已经写了一半字节出去。
#[test]
fn the_bridge_resume_refuses_a_blank_session_without_touching_the_bridge() {
    let (root, _writer, executor) =
        harness("bridge-resume-blank", &[task("t1", "p1", "interrupted")]);
    let saver = Arc::new(CapturingSaver::default());
    let bridge: Arc<dyn aeroric_core::context::HostTaskSaver> = saver.clone();
    let service = build_service(reader(&root).with_task_saver(bridge), &executor);
    let project = local_project("p1", "/repo");

    let error = service
        .resume_via_host(&project, "t1", "", "op-bridge-r", true, None)
        .expect_err("the bridge does not get to reinterpret a blank session");
    assert!(matches!(error, TaskError::NoSession(_)), "{error:?}");
    assert!(
        saver.saved.lock().expect("saver mutex").is_empty(),
        "a refused resume stored nothing — not even the rows it had already read"
    );
    assert!(
        executor.call_log().is_empty(),
        "and no process was asked for"
    );
    assert_eq!(service.generation("t1"), 0, "and no generation was burned");
    cleanup(root);
}
