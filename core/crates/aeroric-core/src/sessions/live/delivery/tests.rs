use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use aeroric_protocol::delivery::DeliveryEvidence;

use super::*;

struct Quiet;
impl LiveStatusSink for Quiet {
    fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
}

#[derive(Default)]
struct FakeHost {
    busy: Mutex<HashSet<String>>,
    /// (task, id, text) of every hand-over.
    sent: Mutex<Vec<(String, String, String)>>,
    fail: Mutex<Option<String>>,
    no_receipts: Mutex<bool>,
    records: Mutex<HashMap<String, Option<bool>>>,
}

impl DeliveryHost for FakeHost {
    fn send(&self, task_id: &str, id: &str, text: &str) -> Result<bool, String> {
        if let Some(reason) = self.fail.lock().clone() {
            return Err(reason);
        }
        self.sent
            .lock()
            .push((task_id.into(), id.into(), text.into()));
        Ok(!*self.no_receipts.lock())
    }
    fn idle(&self, task_id: &str) -> bool {
        !self.busy.lock().contains(task_id)
    }
    fn recorded(&self, task_id: &str, _text: &str) -> Option<bool> {
        self.records.lock().get(task_id).copied().flatten()
    }
}

fn rig() -> (Arc<Deliveries>, Arc<FakeHost>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("aeroric-delivery-{}", uuid::Uuid::new_v4()));
    let queue = Deliveries::new(Arc::new(Quiet));
    queue.set_dir(&dir).unwrap();
    let host = Arc::new(FakeHost::default());
    queue.set_host(Arc::clone(&host) as Arc<dyn DeliveryHost>);
    (queue, host, dir)
}

fn wait(what: &str, ok: impl Fn() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn update(task: &str, kind: SessionUpdateKind) -> SessionUpdate {
    SessionUpdate {
        task_id: task.into(),
        seq: 1,
        at_ms: 1,
        kind,
    }
}

fn receipt(queue: &Deliveries, task: &str, id: &str, outcome: ReceiptOutcome) {
    queue.update(&update(
        task,
        SessionUpdateKind::DeliveryReceipt {
            id: id.into(),
            outcome,
            reason: None,
        },
    ));
}

fn turn_ended(queue: &Deliveries, task: &str) {
    queue.update(&update(
        task,
        SessionUpdateKind::TurnEnded {
            turn_id: "u".into(),
            reason: aeroric_protocol::session_live::TurnEndReason::Completed,
            error: None,
        },
    ));
}

fn state(queue: &Deliveries, id: &str) -> DeliveryState {
    queue.get(id).unwrap().state
}

fn result(queue: &Deliveries, task: &str, text: &str) -> Delivery {
    queue.enqueue(
        task,
        DeliveryKind::Delegation,
        DeliveryMode::AfterTurn,
        "委派结果",
        text,
        Some("dg-1"),
    )
}

/// Q-S5: ours waits for the turn, goes one at a time, and the agent's
/// answer settles it.
#[test]
fn a_result_waits_for_the_turn_then_goes_one_at_a_time() {
    let (queue, host, _) = rig();
    host.busy.lock().insert("p".into());
    let first = result(&queue, "p", "一");
    let second = result(&queue, "p", "二");
    std::thread::sleep(Duration::from_millis(30));
    assert!(host.sent.lock().is_empty(), "busy: nothing goes out");
    host.busy.lock().clear();
    turn_ended(&queue, "p");
    wait("first sent", || host.sent.lock().len() == 1);
    assert_eq!(state(&queue, &first.id), DeliveryState::Sending);
    assert_eq!(state(&queue, &second.id), DeliveryState::Waiting);
    receipt(&queue, "p", &first.id, ReceiptOutcome::Delivered);
    let done = queue.get(&first.id).unwrap();
    assert_eq!(done.state, DeliveryState::Delivered);
    assert_eq!(done.evidence, Some(DeliveryEvidence::Protocol));
    wait("second sent", || host.sent.lock().len() == 2);
    assert_eq!(host.sent.lock()[1].2, "二");
}

/// Claude: the transport takes it, the echo proves it.
#[test]
fn an_echo_after_acceptance_is_the_evidence() {
    let (queue, host, _) = rig();
    let item = result(&queue, "p", "回显");
    wait("sent", || host.sent.lock().len() == 1);
    receipt(&queue, "p", &item.id, ReceiptOutcome::Accepted);
    assert_eq!(state(&queue, &item.id), DeliveryState::Accepted);
    receipt(&queue, "p", &item.id, ReceiptOutcome::Delivered);
    assert_eq!(
        queue.get(&item.id).unwrap().evidence,
        Some(DeliveryEvidence::Echo)
    );
}

/// A driver without receipts: a turn starting is the proof.
#[test]
fn without_receipts_a_turn_start_proves_it() {
    let (queue, host, _) = rig();
    *host.no_receipts.lock() = true;
    let item = result(&queue, "p", "无回执");
    wait("accepted", || {
        state(&queue, &item.id) == DeliveryState::Accepted
    });
    queue.update(&update(
        "p",
        SessionUpdateKind::TurnStarted {
            turn_id: "u2".into(),
        },
    ));
    let done = queue.get(&item.id).unwrap();
    assert_eq!(done.state, DeliveryState::Delivered);
    assert_eq!(done.evidence, Some(DeliveryEvidence::TurnStarted));
}

/// Q-S6: stop keeps what has not gone out, paused; resume sends it.
#[test]
fn stop_pauses_and_resume_sends() {
    let (queue, host, _) = rig();
    host.busy.lock().insert("p".into());
    let item = result(&queue, "p", "停");
    queue.pause("p");
    assert_eq!(state(&queue, &item.id), DeliveryState::Paused);
    host.busy.lock().clear();
    turn_ended(&queue, "p");
    std::thread::sleep(Duration::from_millis(30));
    assert!(host.sent.lock().is_empty(), "paused stays");
    queue.act(&item.id, Action::Resume).unwrap();
    wait("resumed", || host.sent.lock().len() == 1);
    assert_eq!(
        queue.act(&item.id, Action::Resume),
        Err(DeliveryError::WrongState)
    );
    assert_eq!(
        queue.act("nope", Action::Discard),
        Err(DeliveryError::NotFound)
    );
}

/// Q-S8: one try; a failure only the user can fix is not retried by
/// something new, any other is.
#[test]
fn a_start_failure_is_tried_once_and_blocked_ones_wait() {
    let (queue, host, _) = rig();
    *host.fail.lock() = Some("Claude 未登录".into());
    let blocked = result(&queue, "p", "一");
    wait("failed", || {
        state(&queue, &blocked.id) == DeliveryState::Failed
    });
    assert!(queue.get(&blocked.id).unwrap().blocked);
    *host.fail.lock() = Some("进程意外退出".into());
    let flaky = result(&queue, "p", "二");
    wait("failed", || {
        state(&queue, &flaky.id) == DeliveryState::Failed
    });
    assert!(!queue.get(&flaky.id).unwrap().blocked);
    *host.fail.lock() = None;
    result(&queue, "p", "三");
    wait("retried", || {
        host.sent.lock().iter().any(|(_, _, t)| t == "二")
    });
    assert_eq!(state(&queue, &blocked.id), DeliveryState::Failed);
    queue.act(&blocked.id, Action::Resend).unwrap();
    assert!(!queue.get(&blocked.id).unwrap().blocked);
}

/// Q-S4, Q-S7: what was out when the host stopped is unknown, looked up in
/// the record; what waited goes out on its own; paused stays.
#[test]
fn a_restart_settles_what_was_out_and_sends_what_waited() {
    let (queue, host, dir) = rig();
    host.busy.lock().insert("p".into());
    host.busy.lock().insert("q".into());
    let out = result(&queue, "r", "发出去了");
    wait("sent", || host.sent.lock().len() == 1);
    let lost = result(&queue, "s", "丢了");
    wait("sent", || host.sent.lock().len() == 2);
    let waiting = result(&queue, "p", "等着");
    let paused = result(&queue, "q", "停了");
    queue.pause("q");
    drop(queue);

    let again = Deliveries::new(Arc::new(Quiet));
    again.set_dir(&dir).unwrap();
    assert_eq!(state(&again, &out.id), DeliveryState::Unknown);
    let host = Arc::new(FakeHost::default());
    host.records.lock().insert("r".into(), Some(true));
    host.records.lock().insert("s".into(), None);
    again.set_host(Arc::clone(&host) as Arc<dyn DeliveryHost>);
    let found = again.get(&out.id).unwrap();
    assert_eq!(found.state, DeliveryState::Delivered);
    assert_eq!(found.evidence, Some(DeliveryEvidence::Transcript));
    assert_eq!(state(&again, &lost.id), DeliveryState::Unknown);
    assert_eq!(
        again
            .attention()
            .iter()
            .map(|d| d.id.clone())
            .collect::<Vec<_>>(),
        vec![lost.id.clone()]
    );
    wait("waiting sent", || {
        host.sent.lock().iter().any(|(_, id, _)| *id == waiting.id)
    });
    assert_eq!(state(&again, &paused.id), DeliveryState::Paused);
    assert!(again.act(&lost.id, Action::Resend).is_ok());
}

#[test]
fn the_state_machine_refuses_what_does_not_apply() {
    let mut d = Delivery {
        id: "d".into(),
        task_id: "t".into(),
        kind: DeliveryKind::Review,
        mode: DeliveryMode::AfterTurn,
        label: String::new(),
        text: "x".into(),
        state: DeliveryState::Delivered,
        attempts: 1,
        reason: None,
        evidence: None,
        blocked: false,
        source_ref: None,
        created_ms: 0,
        updated_ms: 0,
    };
    for event in [
        Event::Send,
        Event::Stop,
        Event::Discard,
        Event::Resend,
        Event::Lost,
    ] {
        assert!(!step(&mut d, event, 5), "{event:?}");
    }
    d.state = DeliveryState::Unknown;
    assert!(
        !step(&mut d, Event::Send, 5),
        "unknown is never resent by itself"
    );
    assert!(step(&mut d, Event::Discard, 6));
    assert_eq!(d.state, DeliveryState::Withdrawn);
    assert!(user_fixable("Error: 401 Unauthorized"));
    assert!(!user_fixable("broken pipe"));
}

/// CU 2026-11-02 (F1): the real order — the turn's end arrives while the
/// driver still reads busy; the settled status after it sends.
#[test]
fn a_result_goes_once_the_session_has_settled_after_its_turn() {
    let (queue, host, _) = rig();
    host.busy.lock().insert("p".into());
    let first = result(&queue, "p", "一");
    let second = result(&queue, "p", "二");
    turn_ended(&queue, "p");
    std::thread::sleep(Duration::from_millis(30));
    assert!(host.sent.lock().is_empty(), "still busy at the turn's end");
    host.busy.lock().clear();
    queue.status("proj", "p", 1, TaskStatus::Done, None);
    wait("first sent", || host.sent.lock().len() == 1);
    // Delivered: the turn it started is running.
    host.busy.lock().insert("p".into());
    receipt(&queue, "p", &first.id, ReceiptOutcome::Delivered);
    turn_ended(&queue, "p");
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        host.sent.lock().len(),
        1,
        "the second waits for the next turn"
    );
    host.busy.lock().clear();
    queue.status("proj", "p", 1, TaskStatus::Done, None);
    wait("second sent", || host.sent.lock().len() == 2);
    assert_eq!(state(&queue, &second.id), DeliveryState::Sending);
}
