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
use aeroric_core::tasks::executor::TaskEvent;
use aeroric_core::tasks::service::{Attachments, EventOutcome, NewTask, TaskError};
use aeroric_core::tasks::{launch_plan, operations, reducer, session_owner};
use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use serde_json::{json, Value};

mod common;
use common::{build_service, cleanup, harness, local_project, record, task, Families};

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
        .resume(&project, "t1", "", "op-r1", true)
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
        .resume(&project, "t1", "d-1", "op-r1", true)
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
        .resume(&project, "t1", "c-9", "op-r1", true)
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
            changed: true
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
        .resume(&project, "t1", "c-old", "op-r", true)
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
        .resume(&project, "t1", "c-old", "op-r2", true)
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
        1,
        vec![],
        vec![],
        false,
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
    let failure =
        aeroric_core::tasks::service::TaskService::new(context, Arc::new(Families), runner);
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
    let (root, context, executor) = harness("no-write", &[task("t1", "p1", "running")]);
    let service = build_service(context, &executor);
    let tasks_file = tasks_json(&root);
    let before = std::fs::read(&tasks_file).expect("seeded");
    let (outcome, applied) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 0,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
            },
        )
        .expect("same status");
    assert_eq!(
        outcome,
        EventOutcome::Applied {
            status: TaskStatus::Running,
            changed: false
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

    // A clean switch lands first, so core now owns a published cover for this file. Staying
    // inside the claude family with native resume supported makes it a `resume`, not a
    // handoff: the transcript is one the target CLI can read where it stands.
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
        aeroric_core::tasks::session_owner::ConfigSwitchStrategy::Resume
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
        .resume(&project, "t1", "c-1", "op-r", true)
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
