//! E5-4 (2026-11-19): the delivery queue's invariants, checked after every
//! step instead of at the end of one story:
//!
//! 1. **Once**: a delivered item stays delivered; a source has at most one
//!    item on its way; content proven delivered is not sent again on its own.
//! 2. **No orphans**: after a restart nothing reads as sending or accepted,
//!    since no driver holds it any more.
//! 3. **Expired is not done**: a lost, timed-out or unreadable item is never
//!    delivered without evidence, and never sent again unless the user says.
//! 4. **Rebuilt**: what is on disk reads back as the same items; replaying an
//!    item's events rebuilds it exactly.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::state::{step, Event, NO_ANSWER};
use super::*;

const EVENTS: &[Event<'static>] = &[
    Event::Send,
    Event::Receipt(ReceiptOutcome::Accepted, None),
    Event::Receipt(ReceiptOutcome::Delivered, None),
    Event::Receipt(ReceiptOutcome::Rejected, Some("Agent 拒收")),
    Event::TurnStarted,
    Event::SendFailed {
        reason: "会话打不开",
        blocked: false,
    },
    Event::SendFailed {
        reason: "没有登录",
        blocked: true,
    },
    Event::Lost(None),
    Event::Lost(Some(NO_ANSWER)),
    Event::Recorded(true),
    Event::Recorded(false),
    Event::Unreadable,
    Event::Stop,
    Event::Restarted,
    Event::Resume,
    Event::Discard,
    Event::Resend,
    Event::Retry,
];

/// What only a person does.
fn by_user(event: Event<'_>) -> bool {
    matches!(
        event,
        Event::Stop | Event::Resume | Event::Discard | Event::Resend
    )
}

fn fresh() -> Delivery {
    Delivery {
        id: "dv-1".into(),
        task_id: "p".into(),
        kind: DeliveryKind::Delegation,
        mode: DeliveryMode::AfterTurn,
        label: "委派结果".into(),
        text: "结论".into(),
        state: DeliveryState::Waiting,
        attempts: 0,
        reason: None,
        evidence: None,
        blocked: false,
        source_ref: Some("delegation:p/dg-1".into()),
        created_ms: 1,
        updated_ms: 1,
    }
}

/// Every state the machine can reach, every event in each: each applied
/// step keeps the invariants, each refused one changes nothing.
#[test]
fn every_reachable_step_of_the_state_machine_keeps_the_invariants() {
    let mut seen = HashSet::new();
    let mut todo = VecDeque::from([(fresh(), Vec::<usize>::new())]);
    let mut steps = 0;
    while let Some((item, path)) = todo.pop_front() {
        let key = format!(
            "{:?}/{}/{:?}/{}/{}",
            item.state,
            item.blocked,
            item.evidence,
            item.reason.is_some(),
            item.attempts.min(3)
        );
        if !seen.insert(key) {
            continue;
        }
        // 4: the stored body is the item, and its events rebuild it.
        let body = serde_json::to_string(&item).unwrap();
        assert_eq!(serde_json::from_str::<Delivery>(&body).unwrap(), item);
        let mut replay = fresh();
        for &i in &path {
            assert!(step(&mut replay, EVENTS[i], 5));
        }
        assert_eq!(replay, item, "replaying {path:?}");
        if path.len() >= 8 {
            continue;
        }
        for (i, &event) in EVENTS.iter().enumerate() {
            let mut next = item.clone();
            if !step(&mut next, event, 5) {
                assert_eq!(next, item, "a refused {event:?} changed {:?}", item.state);
                continue;
            }
            steps += 1;
            keeps(&item, event, &next);
            let mut longer = path.clone();
            longer.push(i);
            todo.push_back((next, longer));
        }
    }
    // Every state was reached, not just the first few.
    let states: HashSet<String> = seen
        .iter()
        .map(|k| k.split('/').next().unwrap().to_string())
        .collect();
    assert_eq!(states.len(), 8, "{states:?}");
    assert!(steps > 100, "{steps} steps");
}

fn keeps(before: &Delivery, event: Event<'_>, after: &Delivery) {
    use DeliveryState as S;
    let what = format!("{:?} --{event:?}--> {:?}", before.state, after.state);
    // 1: settled is final.
    assert!(
        !matches!(before.state, S::Delivered | S::Withdrawn),
        "{what}: a settled item moved"
    );
    // A send is the only thing that counts as an attempt.
    let grew = after.attempts - before.attempts;
    assert_eq!(grew, u32::from(matches!(event, Event::Send)), "{what}");
    if after.state == S::Sending {
        assert_eq!(
            before.state,
            S::Waiting,
            "{what}: sent from outside the queue"
        );
    }
    // 3: delivered only on evidence; a loss, a timeout or an unreadable
    // record never counts.
    if after.state == S::Delivered {
        assert!(
            matches!(
                event,
                Event::Receipt(ReceiptOutcome::Delivered, _)
                    | Event::TurnStarted
                    | Event::Recorded(true)
            ),
            "{what}"
        );
        assert!(after.evidence.is_some() && after.reason.is_none(), "{what}");
    }
    // 3: nobody knows whether it arrived, so only the user sends it again;
    // a stop or a failure only the user can fix waits for them too.
    let back = matches!(after.state, S::Waiting | S::Sending);
    if back && (before.state == S::Unknown || before.state == S::Paused || before.blocked) {
        assert!(by_user(event), "{what}: went again on its own");
    }
    assert!(S::Unknown.is_open(), "unknown is not settled");
}

// MARK: the queue

#[derive(Default)]
struct Host {
    busy: Mutex<HashSet<String>>,
    /// (task, item id) of every hand-over.
    sent: Mutex<Vec<(String, String)>>,
}

impl DeliveryHost for Host {
    fn send(&self, task_id: &str, id: &str, _text: &str) -> Result<bool, String> {
        self.sent.lock().push((task_id.into(), id.into()));
        // No receipts: the transport takes it; a turn start proves it.
        Ok(false)
    }
    fn idle(&self, task_id: &str) -> bool {
        !self.busy.lock().contains(task_id)
    }
    fn recorded(&self, _task_id: &str, _text: &str) -> Option<bool> {
        None
    }
}

struct Quiet;
impl LiveStatusSink for Quiet {
    fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
}

fn temp(what: &str) -> PathBuf {
    std::env::temp_dir().join(format!("aeroric-{what}-{}", uuid::Uuid::new_v4().simple()))
}

fn queue_at(dir: &Path, host: &Arc<Host>) -> Arc<Deliveries> {
    let queue = Deliveries::new(Arc::new(Quiet));
    queue.set_dir(dir).unwrap();
    queue.set_host(Arc::clone(host) as Arc<dyn DeliveryHost>);
    queue
}

fn update(task: &str, kind: SessionUpdateKind) -> SessionUpdate {
    SessionUpdate {
        task_id: task.into(),
        seq: 1,
        at_ms: 1,
        kind,
    }
}

fn result(queue: &Deliveries, task: &str) -> Delivery {
    queue
        .enqueue_once(
            task,
            DeliveryKind::Delegation,
            DeliveryMode::AfterTurn,
            "委派结果",
            "结论",
            &format!("delegation:{task}/dg-1"),
        )
        .unwrap()
}

fn on_its_way(state: DeliveryState) -> bool {
    matches!(
        state,
        DeliveryState::Waiting
            | DeliveryState::Sending
            | DeliveryState::Accepted
            | DeliveryState::Paused
    )
}

/// 1 and 3 over what the queue holds now.
fn holds(queue: &Deliveries, tasks: &[&str]) {
    for task in tasks {
        let items = queue.list(task);
        for item in &items {
            if item.state == DeliveryState::Delivered {
                assert!(
                    item.evidence.is_some(),
                    "delivered with no evidence: {item:?}"
                );
            }
            let Some(source) = &item.source_ref else {
                continue;
            };
            let same = |d: &&Delivery| d.source_ref.as_ref() == Some(source);
            let moving = items
                .iter()
                .filter(same)
                .filter(|d| on_its_way(d.state))
                .count();
            assert!(moving <= 1, "{source}: {moving} on their way");
            if item.state == DeliveryState::Delivered {
                let again = items.iter().filter(same).any(|d| {
                    d.text == item.text
                        && matches!(d.state, DeliveryState::Waiting | DeliveryState::Paused)
                });
                assert!(!again, "{source}: delivered and still waiting to go again");
            }
        }
    }
}

/// 2 and 4: a host started now over a copy of the files reads every item
/// back; what was out is unknown, the user's own unsent message waits for
/// them, the rest is as it was.
fn rebuilds(queue: &Deliveries, dir: &Path, tasks: &[&str]) {
    let copy = temp("delivery-copy");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::copy(dir.join("delivery.db"), copy.join("delivery.db")).unwrap();
    let again = Deliveries::new(Arc::new(Quiet));
    again.set_dir(&copy).unwrap();
    for task in tasks {
        let now = queue.list(task);
        assert_eq!(
            again.list(task).len(),
            now.len(),
            "{task}: items lost or added"
        );
        for item in now {
            let back = again.get(&item.id).expect("read back");
            let expected = match item.state {
                DeliveryState::Sending | DeliveryState::Accepted => DeliveryState::Unknown,
                DeliveryState::Waiting if item.kind == DeliveryKind::User => DeliveryState::Paused,
                state => state,
            };
            assert_eq!(back.state, expected, "{}: {:?}", item.id, item.state);
            assert_eq!(
                (
                    &back.text,
                    &back.source_ref,
                    back.kind,
                    back.attempts,
                    back.created_ms
                ),
                (
                    &item.text,
                    &item.source_ref,
                    item.kind,
                    item.attempts,
                    item.created_ms
                )
            );
            if expected == item.state {
                assert_eq!(back.evidence, item.evidence);
            }
        }
    }
    std::fs::remove_dir_all(&copy).ok();
}

fn settle() {
    std::thread::sleep(Duration::from_millis(30));
}

fn wait(what: &str, ok: impl Fn() -> bool) {
    let start = std::time::Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn state_of(queue: &Deliveries, id: &str) -> DeliveryState {
    queue.get(id).unwrap().state
}

/// 入队、交出、会话断了、用户重发、旧的那条迟到的回执证明它其实到了：
/// the copy still waiting is withdrawn, so the parent hears it once.
#[test]
fn content_proven_delivered_late_is_not_sent_again_from_the_queue() {
    let dir = temp("delivery");
    let host = Arc::new(Host::default());
    let queue = queue_at(&dir, &host);
    let tasks = ["p"];

    let first = result(&queue, "p");
    holds(&queue, &tasks);
    wait("accepted", || {
        state_of(&queue, &first.id) == DeliveryState::Accepted
    });
    holds(&queue, &tasks);
    rebuilds(&queue, &dir, &tasks);

    // The parent's session dies with it out: nobody knows.
    queue.status(
        "project-1",
        "p",
        1,
        TaskStatus::Failed,
        Some("进程退出".into()),
    );
    assert_eq!(queue.get(&first.id).unwrap().state, DeliveryState::Unknown);
    settle();
    assert_eq!(host.sent.lock().len(), 1, "not sent again on its own");
    holds(&queue, &tasks);
    rebuilds(&queue, &dir, &tasks);

    // The user sends it again while the parent is busy: a new item waits.
    host.busy.lock().insert("p".into());
    let second = result(&queue, "p");
    assert_ne!(second.id, first.id);
    assert_eq!(queue.get(&second.id).unwrap().state, DeliveryState::Waiting);
    holds(&queue, &tasks);
    rebuilds(&queue, &dir, &tasks);

    // The first one's receipt comes late: it did arrive.
    queue.update(&update(
        "p",
        SessionUpdateKind::DeliveryReceipt {
            id: first.id.clone(),
            outcome: ReceiptOutcome::Delivered,
            reason: None,
        },
    ));
    assert_eq!(
        queue.get(&first.id).unwrap().state,
        DeliveryState::Delivered
    );
    let copy = queue.get(&second.id).unwrap();
    assert_eq!(copy.state, DeliveryState::Withdrawn);
    assert_eq!(copy.reason.as_deref(), Some(ALREADY_DELIVERED));
    holds(&queue, &tasks);
    rebuilds(&queue, &dir, &tasks);

    // The parent goes idle: nothing more goes out.
    host.busy.lock().clear();
    queue.update(&update(
        "p",
        SessionUpdateKind::TurnEnded {
            turn_id: "u".into(),
            reason: aeroric_protocol::session_live::TurnEndReason::Completed,
            error: None,
            raw_status: None,
        },
    ));
    settle();
    assert_eq!(host.sent.lock().len(), 1, "the parent heard it once");
}

/// A copy already handed over cannot be called back; only waiting ones go.
#[test]
fn a_copy_already_handed_over_is_left_alone() {
    let dir = temp("delivery");
    let host = Arc::new(Host::default());
    let queue = queue_at(&dir, &host);
    let first = result(&queue, "p");
    wait("accepted", || {
        state_of(&queue, &first.id) == DeliveryState::Accepted
    });
    queue.status("project-1", "p", 1, TaskStatus::Failed, None);
    let second = result(&queue, "p");
    wait("handed over", || {
        state_of(&queue, &second.id) == DeliveryState::Accepted
    });
    queue.update(&update(
        "p",
        SessionUpdateKind::DeliveryReceipt {
            id: first.id.clone(),
            outcome: ReceiptOutcome::Delivered,
            reason: None,
        },
    ));
    assert_eq!(
        queue.get(&second.id).unwrap().state,
        DeliveryState::Accepted
    );
    // A plain queue of different texts from one source is untouched too.
    let other = queue
        .enqueue(
            "p",
            DeliveryKind::Delegation,
            DeliveryMode::AfterTurn,
            "委派结果",
            "另一份结论",
            first.source_ref.as_deref(),
        )
        .unwrap();
    assert!(on_its_way(queue.get(&other.id).unwrap().state));
}

/// A restart in the middle of everything: each item reads back, nothing is
/// left in flight without a driver, nothing is sent again on its own, and
/// what waited is sent by the new host.
#[test]
fn a_restart_leaves_nothing_in_flight_and_sends_nothing_twice() {
    let dir = temp("delivery");
    let host = Arc::new(Host::default());
    let queue = queue_at(&dir, &host);
    let tasks = ["p", "q", "r", "s"];

    // p: delivered. q: out, no answer yet. r: the user's message waiting.
    // s: a result waiting for the turn.
    let delivered = result(&queue, "p");
    wait("accepted", || {
        state_of(&queue, &delivered.id) == DeliveryState::Accepted
    });
    queue.update(&update(
        "p",
        SessionUpdateKind::TurnStarted {
            turn_id: "u".into(),
        },
    ));
    let out = result(&queue, "q");
    wait("out", || {
        state_of(&queue, &out.id) == DeliveryState::Accepted
    });
    host.busy.lock().extend(["r".to_string(), "s".to_string()]);
    let user = queue
        .enqueue(
            "r",
            DeliveryKind::User,
            DeliveryMode::AfterTurn,
            "你的消息",
            "继续",
            None,
        )
        .unwrap();
    let waiting = result(&queue, "s");
    assert_eq!(
        queue.get(&delivered.id).unwrap().state,
        DeliveryState::Delivered
    );
    assert_eq!(queue.get(&out.id).unwrap().state, DeliveryState::Accepted);
    holds(&queue, &tasks);
    rebuilds(&queue, &dir, &tasks);

    // The host restarts for real, with every session idle now.
    let sent_before = host.sent.lock().len();
    host.busy.lock().clear();
    let again = queue_at(&dir, &host);
    wait("what waited went", || host.sent.lock().len() > sent_before);
    settle();
    assert_eq!(again.get(&out.id).unwrap().state, DeliveryState::Unknown);
    assert_eq!(again.get(&user.id).unwrap().state, DeliveryState::Paused);
    assert_eq!(
        again.get(&delivered.id).unwrap().state,
        DeliveryState::Delivered
    );
    let sent: Vec<String> = host.sent.lock()[sent_before..]
        .iter()
        .map(|(_, id)| id.clone())
        .collect();
    assert_eq!(
        sent,
        std::slice::from_ref(&waiting.id),
        "only what waited goes, once"
    );
    for task in tasks {
        assert!(again
            .list(task)
            .iter()
            .all(|d| d.state != DeliveryState::Sending));
    }
    holds(&again, &tasks);
}
