use super::*;
use std::time::Instant;

use aeroric_protocol::checkpoint::Acceptance;
use aeroric_protocol::delegation::{Budget, ContextMode};
use aeroric_protocol::delivery::{DeliveryEvidence, ReceiptOutcome};

use super::super::delivery::DeliveryHost;

struct Quiet;
impl LiveStatusSink for Quiet {
    fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
}

#[derive(Default)]
struct FakeHost {
    parents: Mutex<HashMap<String, ParentInfo>>,
    /// (card, first message) of every child started.
    started: Mutex<Vec<(DelegationCard, String)>>,
    /// (task, text) of every message sent.
    sent: Mutex<Vec<(String, String)>>,
    interrupted: Mutex<Vec<String>>,
    busy: Mutex<HashSet<String>>,
    tokens: Mutex<HashMap<String, u64>>,
    fail_start: Mutex<bool>,
    fail_prompt: Mutex<bool>,
    writers: Mutex<Vec<WriterRef>>,
    /// A1: the parent's driver refuses what the queue hands it.
    refuse: Mutex<Option<String>>,
    /// A1: the parent's driver answers with receipts (else: accepted only).
    receipts: Mutex<bool>,
    /// A1: what the parent's own record says after a lost answer.
    recorded: Mutex<Option<bool>>,
    /// B1: the checkpoints a test watches, and how many obligations the
    /// child already had when `start_child` was called.
    checkpoints: Mutex<Option<Arc<super::super::checkpoint::CheckpointSink>>>,
    obligations_at_start: Mutex<Vec<usize>>,
}

/// A1: the delivery queue's side of the host — the parent's driver.
impl DeliveryHost for FakeHost {
    fn send(&self, task_id: &str, _id: &str, text: &str) -> Result<bool, String> {
        if let Some(reason) = self.refuse.lock().clone() {
            return Err(reason);
        }
        self.sent.lock().push((task_id.into(), text.into()));
        Ok(*self.receipts.lock())
    }
    fn idle(&self, task_id: &str) -> bool {
        !self.busy.lock().contains(task_id)
    }
    fn recorded(&self, _task_id: &str, _text: &str) -> Option<bool> {
        *self.recorded.lock()
    }
}

impl DelegationHost for FakeHost {
    fn writers_beside(&self, _task_id: &str) -> Vec<WriterRef> {
        self.writers.lock().clone()
    }
    fn parent(&self, task_id: &str) -> Option<ParentInfo> {
        self.parents.lock().get(task_id).cloned()
    }
    fn start_child(&self, card: &DelegationCard, prompt: String) -> Result<Child, String> {
        if let (Some(cp), Some(child)) = (self.checkpoints.lock().clone(), &card.child_task_id) {
            let count = cp.get(child).map_or(0, |c| c.obligations.len());
            self.obligations_at_start.lock().push(count);
        }
        if *self.fail_start.lock() {
            return Err("本机没有 Codex".into());
        }
        self.started.lock().push((card.clone(), prompt));
        Ok(Child {
            task_id: card.child_task_id.clone().unwrap(),
            label: "Codex".into(),
            agent: "codex".into(),
            model: None,
        })
    }
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String> {
        if *self.fail_prompt.lock() {
            return Err("会话打不开".into());
        }
        self.sent.lock().push((task_id.into(), text));
        Ok(())
    }
    fn interrupt(&self, task_id: &str) {
        self.interrupted.lock().push(task_id.into());
    }
    fn idle(&self, task_id: &str) -> bool {
        !self.busy.lock().contains(task_id)
    }
    fn tokens(&self, task_id: &str) -> Option<u64> {
        self.tokens.lock().get(task_id).copied()
    }
}

struct Rig {
    host: Arc<FakeHost>,
    d: Arc<Delegations>,
    /// A1: the queue results go through, as the executor wires it.
    queue: Arc<Deliveries>,
    /// Its data directory (`delivery.db`).
    queue_dir: PathBuf,
    seq: Mutex<u64>,
}

fn rig(permission: &str) -> Rig {
    let host = Arc::new(FakeHost::default());
    host.parents.lock().insert(
        "p".into(),
        ParentInfo {
            permission: permission.into(),
            nested: false,
        },
    );
    host.parents.lock().insert(
        "rev".into(),
        ParentInfo {
            permission: "plan".into(),
            nested: true,
        },
    );
    let queue_dir = std::env::temp_dir().join(format!(
        "aeroric-delegation-queue-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let queue = Deliveries::new(Arc::new(Quiet));
    queue.set_dir(&queue_dir).unwrap();
    queue.set_host(Arc::clone(&host) as Arc<dyn DeliveryHost>);
    let d = Delegations::new(Arc::clone(&queue) as Arc<dyn LiveStatusSink>);
    d.set_host(Arc::clone(&host) as Arc<dyn DelegationHost>);
    d.set_watch_every(Duration::from_millis(10));
    d.follow(&queue);
    Rig {
        host,
        d,
        queue,
        queue_dir,
        seq: Mutex::new(0),
    }
}

fn contract(instruction: &str) -> Contract {
    Contract {
        instruction: instruction.into(),
        ..Contract::default()
    }
}

fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ok() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl Rig {
    fn push(&self, task: &str, kind: SessionUpdateKind) {
        let mut seq = self.seq.lock();
        *seq += 1;
        self.d.update(&SessionUpdate {
            task_id: task.into(),
            seq: *seq,
            at_ms: 0,
            kind,
        });
    }
    fn card(&self, id: &str) -> DelegationCard {
        self.d
            .list("p")
            .into_iter()
            .find(|c| c.delegation_id == id)
            .unwrap()
    }
    /// Start and wait for the child to run; its task id.
    fn running(&self, c: Contract, origin: Origin) -> (String, String) {
        let card = self.d.start("p", c, origin, "p").unwrap();
        let id = card.delegation_id.clone();
        wait("child started", || {
            self.card(&id).state == DelegationState::Running
        });
        (id.clone(), self.card(&id).child_task_id.unwrap())
    }
    /// One whole turn of `task` saying `text`, then its settle.
    fn answer(&self, task: &str, turn: &str, text: &str, diff: Option<TurnDiff>, checking: bool) {
        self.answer_after(task, turn, &[], text, diff, checking);
    }

    /// A turn that ran `commands` before answering.
    fn answer_after(
        &self,
        task: &str,
        turn: &str,
        commands: &[&str],
        text: &str,
        diff: Option<TurnDiff>,
        checking: bool,
    ) {
        self.push(
            task,
            SessionUpdateKind::TurnStarted {
                turn_id: turn.into(),
            },
        );
        for command in commands {
            self.push(
                task,
                SessionUpdateKind::ToolCall {
                    id: format!("c-{command}"),
                    kind: aeroric_protocol::session_live::ToolKind::Command,
                    title: (*command).into(),
                    input: serde_json::json!({ "command": command }),
                },
            );
        }
        self.push(
            task,
            SessionUpdateKind::MessageCompleted {
                message_id: format!("m-{turn}"),
                text: Some(text.into()),
            },
        );
        self.push(
            task,
            SessionUpdateKind::TurnEnded {
                turn_id: turn.into(),
                reason: TurnEndReason::Completed,
                error: None,
                raw_status: None,
            },
        );
        self.d.turn_settled(TurnSettled {
            task_id: task.into(),
            turn_id: turn.into(),
            reason: TurnEndReason::Completed,
            error: None,
            diff,
            checking,
        });
    }
    fn sent_to(&self, task: &str) -> Vec<String> {
        self.host
            .sent
            .lock()
            .iter()
            .filter(|(t, _)| t == task)
            .map(|(_, s)| s.clone())
            .collect()
    }
}

use super::super::turn_changes::TurnDiff;

fn one_file() -> TurnDiff {
    TurnDiff {
        top: "/w".into(),
        before: String::new(),
        after: String::new(),
        files: vec![super::super::update::TurnFileChange {
            path: "src/a.rs".into(),
            additions: Some(3),
            deletions: Some(1),
        }],
        additions: 3,
        deletions: 1,
    }
}

#[test]
fn permissions_decay_and_read_only_means_plan() {
    let rig = rig("ask");
    let above = Contract {
        permission: Some("full_access".into()),
        workspace: Some(Workspace::Shared),
        ..contract("改")
    };
    assert!(matches!(
        rig.d.start("p", above, Origin::User, "user").unwrap_err(),
        DelegationError::PermissionAbove { .. }
    ));
    let read_only_writes = Contract {
        permission: Some("ask".into()),
        workspace: Some(Workspace::ReadOnly),
        ..contract("查")
    };
    assert_eq!(
        rig.d
            .start("p", read_only_writes, Origin::User, "user")
            .unwrap_err(),
        DelegationError::ReadOnlyWrites
    );
    // A role asking for more than the parent has is clamped, not refused.
    let (resolved, role) = rig
        .d
        .resolve(
            &Contract {
                role: Some("实现者".into()),
                ..contract("改超时")
            },
            "ask",
        )
        .unwrap();
    assert_eq!(role.unwrap().id, "implementer");
    assert_eq!(resolved.permission, "ask");
    // R2-3b: writable roles work in an isolated copy by default.
    assert_eq!(resolved.workspace, Workspace::Copy);
    assert_eq!(resolved.context_mode, ContextMode::Brief);
    let (resolved, _) = rig
        .d
        .resolve(
            &Contract {
                role: Some("researcher".into()),
                ..contract("查")
            },
            "full_access",
        )
        .unwrap();
    assert_eq!(
        (resolved.permission.as_str(), resolved.workspace),
        ("plan", Workspace::ReadOnly)
    );
    assert_eq!(
        rig.d
            .start("p", contract("  "), Origin::User, "user")
            .unwrap_err(),
        DelegationError::EmptyInstruction
    );
    assert!(matches!(
        rig.d
            .start(
                "p",
                Contract {
                    role: Some("无此角色".into()),
                    ..contract("x")
                },
                Origin::User,
                "user"
            )
            .unwrap_err(),
        DelegationError::UnknownRole(_)
    ));
    assert_eq!(
        rig.d
            .start("rev", contract("x"), Origin::Session, "rev")
            .unwrap_err(),
        DelegationError::Nested
    );
    let refused = rig
        .d
        .audit(None)
        .iter()
        .filter(|e| e.action == AuditAction::Refuse)
        .count();
    assert_eq!(refused, 5);
    assert!(rig.host.started.lock().is_empty());
}

#[test]
fn only_one_writer_at_a_time() {
    let rig = rig("auto_edit");
    let writer = || Contract {
        workspace: Some(Workspace::Shared),
        permission: Some("auto_edit".into()),
        ..contract("改")
    };
    rig.running(writer(), Origin::User);
    assert_eq!(
        rig.d
            .start("p", writer(), Origin::User, "user")
            .unwrap_err(),
        DelegationError::SecondWriter
    );
    // Readers are not limited.
    rig.running(contract("查一"), Origin::User);
    rig.running(contract("查二"), Origin::User);
}

#[test]
fn the_agents_delegations_come_back_in_one_message_when_all_are_in() {
    let rig = rig("ask");
    let (a, child_a) = rig.running(
        Contract {
            acceptance: vec![Acceptance {
                text: "给出示例".into(),
                check: None,
            }],
            context_refs: vec!["src/client.rs".into()],
            ..contract("查 reqwest 的超时")
        },
        Origin::Session,
    );
    let (b, child_b) = rig.running(contract("查 hyper 的超时"), Origin::Session);
    let first = rig.host.started.lock()[0].1.clone();
    assert!(first.starts_with(prompt::DELEGATE_MARKER), "{first}");
    assert!(first.contains("查 reqwest 的超时") && first.contains("- src/client.rs"));
    assert!(first.contains("给出示例") && first.contains("只读"));

    rig.answer(&child_a, "t1", "用 connect_timeout", None, false);
    wait("a reported", || {
        rig.card(&a).state == DelegationState::Reported
    });
    let card = rig.card(&a);
    assert_eq!(card.result.as_deref(), Some("用 connect_timeout"));
    assert!(!card.evidence, "no diff, nothing checked");
    std::thread::sleep(Duration::from_millis(30));
    assert!(rig.sent_to("p").is_empty(), "b is still running");

    rig.answer(&child_b, "t1", "hyper 没有总超时", Some(one_file()), false);
    wait("returned", || !rig.sent_to("p").is_empty());
    let sent = rig.sent_to("p");
    assert_eq!(sent.len(), 1);
    assert!(sent[0].starts_with(prompt::RESULT_MARKER));
    assert!(sent[0].contains("2 项") && sent[0].contains("hyper 没有总超时"));
    assert!(rig.card(&b).evidence && rig.card(&b).files == 1);
    assert!(rig.card(&a).sent && rig.card(&b).sent);
    let actions: Vec<AuditAction> = rig.d.audit(Some("p")).iter().map(|e| e.action).collect();
    assert_eq!(
        actions,
        [
            AuditAction::Start,
            AuditAction::Start,
            AuditAction::Report,
            AuditAction::Report,
            AuditAction::Send,
            AuditAction::Send
        ]
    );
}

#[test]
fn a_busy_parent_gets_the_results_when_its_turn_ends() {
    let rig = rig("ask");
    rig.host.busy.lock().insert("p".into());
    let (id, child) = rig.running(contract("查"), Origin::Session);
    rig.answer(&child, "t1", "结论", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    std::thread::sleep(Duration::from_millis(30));
    assert!(rig.sent_to("p").is_empty());
    rig.host.busy.lock().clear();
    rig.push(
        "p",
        SessionUpdateKind::TurnEnded {
            turn_id: "u9".into(),
            reason: TurnEndReason::Completed,
            error: None,
            raw_status: None,
        },
    );
    wait("returned", || rig.sent_to("p").len() == 1);
}

#[test]
fn the_users_own_delegations_wait_for_the_user_to_send() {
    let rig = rig("ask");
    let (id, child) = rig.running(contract("查"), Origin::User);
    assert_eq!(
        rig.d.send("p", &id, "user").unwrap_err(),
        DelegationError::NothingToSend
    );
    rig.answer(&child, "t1", "结论", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    std::thread::sleep(Duration::from_millis(30));
    assert!(rig.sent_to("p").is_empty());
    rig.d.send("p", &id, "user").unwrap();
    wait("sent", || rig.sent_to("p").len() == 1);
    assert_eq!(
        rig.d.send("p", &id, "user").unwrap_err(),
        DelegationError::NothingToSend
    );
}

#[test]
fn a_child_with_checks_reports_once_they_finish() {
    let rig = rig("ask");
    let (id, child) = rig.running(contract("改"), Origin::User);
    rig.answer(&child, "t1", "改好了", Some(one_file()), true);
    assert_eq!(rig.card(&id).state, DelegationState::Checking);
    rig.push(
        &child,
        SessionUpdateKind::AutoCheck {
            turn_id: "t1".into(),
            state: "passed".into(),
            trigger: None,
            note: None,
            input_revision: None,
            items: Vec::new(),
        },
    );
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    assert_eq!(rig.card(&id).result.as_deref(), Some("改好了"));
}

#[test]
fn over_budget_interrupts_and_asks_the_user() {
    let rig = rig("ask");
    let (id, child) = rig.running(
        Contract {
            budget: Some(Budget {
                minutes: None,
                tokens: Some(1000),
            }),
            ..contract("查")
        },
        Origin::Session,
    );
    rig.host.tokens.lock().insert(child.clone(), 1500);
    wait("over budget", || {
        rig.card(&id).state == DelegationState::OverBudget
    });
    assert_eq!(*rig.host.interrupted.lock(), vec![child.clone()]);
    assert_eq!(rig.card(&id).tokens, Some(1500));
    assert_eq!(rig.d.attention().len(), 1);
    // Its interrupted turn ending changes nothing.
    rig.push(
        &child,
        SessionUpdateKind::TurnEnded {
            turn_id: "t1".into(),
            reason: TurnEndReason::Interrupted,
            error: None,
            raw_status: None,
        },
    );
    assert_eq!(rig.card(&id).state, DelegationState::OverBudget);
    // The agent still hears that it came back over budget.
    wait("returned", || rig.sent_to("p").len() == 1);
    assert!(rig.sent_to("p")[0].contains("超出预算"));
}

/// R1-1: another session writing here refuses a writer, not a reader, and
/// names who holds the directory; a redo is checked the same way.
#[test]
fn a_directory_someone_else_is_writing_refuses_writers() {
    let rig = rig("full_access");
    let writer = || Contract {
        workspace: Some(Workspace::Shared),
        permission: Some("auto_edit".into()),
        ..contract("改")
    };
    let (id, _child) = rig.running(writer(), Origin::User);
    rig.d.stop("p", &id, "user").unwrap();
    *rig.host.writers.lock() = vec![WriterRef {
        task_id: "other".into(),
        title: "重构会话列表".into(),
    }];
    let refused = rig
        .d
        .start("p", writer(), Origin::User, "user")
        .unwrap_err();
    assert_eq!(
        refused,
        DelegationError::DirectoryBusy("重构会话列表".into())
    );
    assert!(refused.to_string().contains("「重构会话列表」"));
    assert_eq!(
        rig.d.redo("p", &id, "再来", "user").unwrap_err(),
        DelegationError::DirectoryBusy("重构会话列表".into())
    );
    rig.running(contract("查"), Origin::User);
}

#[test]
fn stop_and_redo() {
    let rig = rig("ask");
    let (id, child) = rig.running(contract("查"), Origin::User);
    rig.d.stop("p", &id, "user").unwrap();
    assert_eq!(rig.card(&id).state, DelegationState::Stopped);
    wait("interrupted", || !rig.host.interrupted.lock().is_empty());
    assert_eq!(
        rig.d.stop("p", &id, "user").unwrap_err(),
        DelegationError::NotOpen
    );
    rig.d.redo("p", &id, "把示例补上", "user").unwrap();
    let card = rig.card(&id);
    assert_eq!((card.state, card.reworks), (DelegationState::Running, 1));
    wait("redo sent", || !rig.sent_to(&child).is_empty());
    assert!(rig.sent_to(&child)[0].starts_with(prompt::REDO_MARKER));
    assert!(rig.sent_to(&child)[0].contains("把示例补上"));
    rig.answer(&child, "t2", "补好了", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    assert_eq!(rig.d.stats()[0].reworks, 1);
}

/// CU 2026-11-02 (F3): after a rework the card carries the new answer.
#[test]
fn a_rework_replaces_the_result() {
    let rig = rig("ask");
    let (id, child) = rig.running(contract("查"), Origin::User);
    rig.answer(&child, "t1", "结论 A", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    assert_eq!(rig.card(&id).result.as_deref(), Some("结论 A"));
    rig.d.redo("p", &id, "再查一次", "user").unwrap();
    wait("redo sent", || !rig.sent_to(&child).is_empty());
    rig.answer(&child, "t2", "结论 A2", None, false);
    wait("reported again", || {
        rig.card(&id).state == DelegationState::Reported && rig.card(&id).reworks == 1
    });
    assert_eq!(rig.card(&id).result.as_deref(), Some("结论 A2"));
}

#[test]
fn a_child_that_cannot_start_fails_and_the_agent_hears_it() {
    let rig = rig("ask");
    *rig.host.fail_start.lock() = true;
    let card = rig
        .d
        .start("p", contract("查"), Origin::Session, "p")
        .unwrap();
    wait("failed", || {
        rig.card(&card.delegation_id).state == DelegationState::Failed
    });
    assert_eq!(
        rig.card(&card.delegation_id).error.as_deref(),
        Some("本机没有 Codex")
    );
    wait("returned", || rig.sent_to("p").len() == 1);
    assert!(rig.sent_to("p")[0].contains("本机没有 Codex"));
}

#[test]
fn a_restart_reads_running_delegations_back_as_interrupted() {
    let tmp = std::env::temp_dir().join(format!(
        "aeroric-delegations-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let rig = rig("ask");
    rig.d.set_dir(tmp.clone());
    let (id, _) = rig.running(contract("查"), Origin::User);
    wait("saved", || {
        std::fs::read_to_string(tmp.join("tasks/p.json")).is_ok_and(|s| s.contains("\"running\""))
    });
    let again = Delegations::new(Arc::new(Quiet));
    again.set_dir(tmp.clone());
    let cards = again.list("p");
    assert_eq!(cards[0].delegation_id, id);
    assert_eq!(cards[0].state, DelegationState::Interrupted);
    assert_eq!(again.attention().len(), 1);
    assert_eq!(again.audit(Some("p"))[0].action, AuditAction::Start);
    assert_eq!(again.stats()[0].total, 1);
}

#[test]
fn stats_count_per_harness_model_and_role() {
    let rig = rig("ask");
    let (a, child) = rig.running(
        Contract {
            role: Some("researcher".into()),
            ..contract("查")
        },
        Origin::User,
    );
    rig.answer(&child, "t1", "结论", None, false);
    wait("reported", || {
        rig.card(&a).state == DelegationState::Reported
    });
    rig.running(contract("另一件"), Origin::User);
    let stats = rig.d.stats();
    assert_eq!(stats.len(), 2);
    let researcher = stats
        .iter()
        .find(|r| r.role.as_deref() == Some("researcher"))
        .unwrap();
    assert_eq!((researcher.total, researcher.reported), (1, 1));
}

/// The tracker's settle runs on a thread of its own and can arrive before
/// the turn's end has gone down the sink chain.
#[test]
fn a_settle_that_overtakes_the_turns_end_still_reads_the_text() {
    let rig = rig("ask");
    let (id, child) = rig.running(contract("查"), Origin::User);
    rig.push(
        &child,
        SessionUpdateKind::TurnStarted {
            turn_id: "t1".into(),
        },
    );
    rig.push(
        &child,
        SessionUpdateKind::MessageChunk {
            message_id: "m1".into(),
            role: "assistant".into(),
            text: "先到的结论".into(),
        },
    );
    rig.d.turn_settled(TurnSettled {
        task_id: child.clone(),
        turn_id: "t1".into(),
        reason: TurnEndReason::Completed,
        error: None,
        diff: None,
        checking: false,
    });
    assert_eq!(rig.card(&id).result.as_deref(), Some("先到的结论"));
    rig.push(
        &child,
        SessionUpdateKind::TurnEnded {
            turn_id: "t1".into(),
            reason: TurnEndReason::Completed,
            error: None,
            raw_status: None,
        },
    );
    assert_eq!(rig.card(&id).state, DelegationState::Reported);
}

// MARK: B4-3 probes

fn tool(rig: &Rig, child: &str, title: &str) {
    rig.push(
        child,
        SessionUpdateKind::ToolCall {
            id: format!("c-{title}-{}", *rig.seq.lock()),
            kind: aeroric_protocol::session_live::ToolKind::Command,
            title: title.into(),
            input: serde_json::json!({"command": title}),
        },
    );
}

fn probes(rig: &Rig, id: &str) -> Vec<String> {
    rig.card(id).probes.iter().map(|p| p.kind.clone()).collect()
}

#[test]
fn the_same_call_three_times_or_back_and_forth_is_flagged_once() {
    let rig = rig("ask");
    let (id, child) = rig.running(contract("查"), Origin::Session);
    rig.push(
        &child,
        SessionUpdateKind::TurnStarted {
            turn_id: "t1".into(),
        },
    );
    tool(&rig, &child, "ls");
    tool(&rig, &child, "ls");
    assert!(probes(&rig, &id).is_empty());
    tool(&rig, &child, "ls");
    assert_eq!(probes(&rig, &id), ["repeat"]);
    tool(&rig, &child, "ls");
    assert_eq!(
        probes(&rig, &id),
        ["repeat"],
        "once until the user has seen it"
    );
    rig.push(
        &child,
        SessionUpdateKind::TurnStarted {
            turn_id: "t2".into(),
        },
    );
    tool(&rig, &child, "cat a");
    tool(&rig, &child, "cat b");
    tool(&rig, &child, "cat a");
    assert_eq!(probes(&rig, &id), ["repeat"]);
    tool(&rig, &child, "cat b");
    assert_eq!(probes(&rig, &id), ["repeat", "oscillation"]);
    // A probe never stops the work.
    assert_eq!(rig.card(&id).state, DelegationState::Running);
    assert_eq!(rig.d.attention().len(), 1);
    rig.d.ack_probes("p", &id).unwrap();
    assert!(rig.d.attention().is_empty());
}

#[test]
fn most_of_the_budget_gone_with_nothing_accepted_is_flagged() {
    let rig = rig("ask");
    let (id, child) = rig.running(
        Contract {
            budget: Some(Budget {
                minutes: None,
                tokens: Some(1000),
            }),
            ..contract("查")
        },
        Origin::User,
    );
    rig.host.tokens.lock().insert(child, 900);
    wait("near budget", || probes(&rig, &id) == ["nearBudget"]);
    assert_eq!(rig.card(&id).state, DelegationState::Running);
}

#[test]
fn done_without_the_criteria_or_without_evidence_is_flagged() {
    let rig = rig("ask");
    let checkpoints = super::super::checkpoint::CheckpointSink::new(Arc::new(Quiet));
    rig.d.set_checkpoints(Arc::downgrade(&checkpoints));
    let (id, child) = rig.running(
        Contract {
            acceptance: vec![Acceptance {
                text: "cargo test 通过".into(),
                check: Some("cargo test".into()),
            }],
            ..contract("改")
        },
        Origin::User,
    );
    wait("obligations", || {
        checkpoints
            .get(&child)
            .is_some_and(|cp| !cp.obligations.is_empty())
    });
    rig.answer(&child, "t1", "改好了", Some(one_file()), false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    // A diff is evidence; the open criterion is not done.
    assert_eq!(probes(&rig, &id), ["unverifiedDone"]);
    let (other, child) = rig.running(contract("查"), Origin::User);
    rig.answer(&child, "t1", "大概是这样", None, false);
    wait("reported", || {
        rig.card(&other).state == DelegationState::Reported
    });
    assert_eq!(probes(&rig, &other), ["noEvidence"]);
    assert!(rig.card(&id).probes[0].detail.contains("还没有自动核对"));
    // A child that read and ran things left traces: not "no evidence".
    let (traced, child) = rig.running(contract("查"), Origin::User);
    rig.answer_after(&child, "t1", &["cat facts.md"], "red=7", None, false);
    wait("reported", || {
        rig.card(&traced).state == DelegationState::Reported
    });
    assert!(probes(&rig, &traced).is_empty());
}

/// C 批 (2026-11-08), A1 (落盘失败): a result that could not even be
/// queued is not sent after all; the card says why and it can be sent again.
#[test]
fn a_result_that_cannot_be_queued_can_be_sent_again() {
    let rig = rig("ask");
    let (id, child) = rig.running(contract("查"), Origin::User);
    rig.answer(&child, "t1", "结论", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    let db = rusqlite::Connection::open(rig.queue_dir.join("delivery.db")).unwrap();
    db.execute_batch("DROP TABLE deliveries").unwrap();
    rig.d.send("p", &id, "user").unwrap();
    let card = rig.card(&id);
    assert!(!card.sent);
    assert!(
        card.return_delivery.is_none(),
        "no record, nothing to follow"
    );
    assert!(
        card.error
            .as_deref()
            .is_some_and(|e| e.starts_with("没送到父会话：没能排上队")),
        "{card:?}"
    );
    assert!(rig.queue.list("p").is_empty());
    db.execute_batch(
        "CREATE TABLE deliveries (
            id TEXT PRIMARY KEY, task_id TEXT NOT NULL, state TEXT NOT NULL,
            updated_ms INTEGER NOT NULL, body TEXT NOT NULL)",
    )
    .unwrap();
    rig.d.send("p", &id, "user").unwrap();
    wait("sent", || rig.sent_to("p").len() == 1);
    let card = rig.card(&id);
    assert!(card.sent);
    assert!(card.error.is_none(), "排上队之后不再说没送到");
    assert!(card.return_delivery.is_some());
}

// MARK: A1 — the result card follows its delivery

/// A reported user delegation, not yet sent; its probes seen, so only its
/// delivery can make it need the user.
fn reported(rig: &Rig) -> String {
    let (id, child) = rig.running(contract("查"), Origin::User);
    rig.answer(&child, "t1", "结论", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    rig.d.ack_probes("p", &id).unwrap();
    assert!(!rig.card(&id).needs_user());
    id
}

fn follow_state(rig: &Rig, id: &str) -> Option<DeliveryState> {
    rig.card(id).return_delivery.map(|r| r.state)
}

/// 完成条件：the card says delivered only when its record is delivered,
/// with the record's evidence.
#[test]
fn a_card_is_delivered_only_with_its_records_evidence() {
    let rig = rig("ask");
    let id = reported(&rig);
    rig.d.send("p", &id, "user").unwrap();
    wait("accepted", || {
        follow_state(&rig, &id) == Some(DeliveryState::Accepted)
    });
    let card = rig.card(&id);
    let follow = card.return_delivery.unwrap();
    assert_eq!(follow.evidence, None, "taken, not yet proven");
    let record = rig.queue.get(&follow.id).unwrap();
    assert_eq!(
        record.source_ref,
        Some(source_ref("p", std::slice::from_ref(&id)))
    );
    assert_eq!(record.kind, DeliveryKind::Delegation);
    rig.push(
        "p",
        SessionUpdateKind::TurnStarted {
            turn_id: "u2".into(),
        },
    );
    wait("delivered", || {
        follow_state(&rig, &id) == Some(DeliveryState::Delivered)
    });
    let follow = rig.card(&id).return_delivery.unwrap();
    let record = rig.queue.get(&follow.id).unwrap();
    assert_eq!(record.state, DeliveryState::Delivered);
    assert_eq!(follow.evidence, Some(DeliveryEvidence::TurnStarted));
    assert_eq!(follow.evidence, record.evidence);
    assert_eq!(
        rig.d.send("p", &id, "user").unwrap_err(),
        DelegationError::NothingToSend
    );
}

/// 入队成功后驱动拒收: the card says it did not arrive and why; sending it
/// again sends the same delivery, so nothing reaches the parent twice.
#[test]
fn a_refused_result_says_why_and_goes_again_as_the_same_delivery() {
    let rig = rig("ask");
    let id = reported(&rig);
    *rig.host.refuse.lock() = Some("会话打不开".into());
    rig.d.send("p", &id, "user").unwrap();
    wait("failed", || {
        follow_state(&rig, &id) == Some(DeliveryState::Failed)
    });
    let card = rig.card(&id);
    let first = card.return_delivery.clone().unwrap();
    assert_eq!(first.reason.as_deref(), Some("会话打不开"));
    assert!(!card.needs_user(), "it says why and offers a resend");

    // The driver takes it but the agent answers no.
    *rig.host.refuse.lock() = None;
    *rig.host.receipts.lock() = true;
    rig.d.send("p", &id, "user").unwrap();
    wait("handed over", || rig.sent_to("p").len() == 1);
    assert_eq!(rig.card(&id).return_delivery.unwrap().id, first.id);
    rig.push(
        "p",
        SessionUpdateKind::DeliveryReceipt {
            id: first.id.clone(),
            outcome: ReceiptOutcome::Rejected,
            reason: Some("Agent 拒收".into()),
        },
    );
    wait("refused", || {
        follow_state(&rig, &id) == Some(DeliveryState::Failed)
    });
    assert_eq!(
        rig.card(&id).return_delivery.unwrap().reason.as_deref(),
        Some("Agent 拒收")
    );

    rig.d.send("p", &id, "user").unwrap();
    wait("handed over again", || rig.sent_to("p").len() == 2);
    rig.push(
        "p",
        SessionUpdateKind::DeliveryReceipt {
            id: first.id.clone(),
            outcome: ReceiptOutcome::Delivered,
            reason: None,
        },
    );
    wait("delivered", || {
        follow_state(&rig, &id) == Some(DeliveryState::Delivered)
    });
    assert_eq!(rig.queue.list("p").len(), 1, "one delivery throughout");
    assert_eq!(
        rig.card(&id).return_delivery.unwrap().evidence,
        Some(DeliveryEvidence::Protocol)
    );
}

/// 接收后断线 + 过期事件: taken, then the parent's session died — nobody
/// knows; it needs the user. Sent again it follows a new delivery, and a
/// late receipt of the old one does not move it.
#[test]
fn a_lost_answer_needs_the_user_and_a_late_receipt_of_the_old_delivery_is_ignored() {
    let rig = rig("ask");
    let id = reported(&rig);
    rig.d.send("p", &id, "user").unwrap();
    wait("accepted", || {
        follow_state(&rig, &id) == Some(DeliveryState::Accepted)
    });
    let old = rig.card(&id).return_delivery.unwrap().id;
    rig.d.status(
        "project-1",
        "p",
        1,
        TaskStatus::Failed,
        Some("进程退出".into()),
    );
    wait("unknown", || {
        follow_state(&rig, &id) == Some(DeliveryState::Unknown)
    });
    let card = rig.card(&id);
    assert!(card.sent, "handed over, not taken back");
    assert!(card.needs_user());
    assert!(rig.d.attention().iter().any(|c| c.delegation_id == id));

    rig.d.send("p", &id, "user").unwrap();
    let new = rig.card(&id).return_delivery.unwrap();
    assert_ne!(new.id, old, "a new delivery; the unknown one stays");
    assert_eq!(rig.queue.get(&old).unwrap().state, DeliveryState::Unknown);
    wait("accepted again", || {
        follow_state(&rig, &id) == Some(DeliveryState::Accepted)
    });
    assert!(!rig.card(&id).needs_user());

    rig.push(
        "p",
        SessionUpdateKind::DeliveryReceipt {
            id: old.clone(),
            outcome: ReceiptOutcome::Delivered,
            reason: None,
        },
    );
    assert_eq!(rig.queue.get(&old).unwrap().state, DeliveryState::Delivered);
    std::thread::sleep(Duration::from_millis(30));
    let card = rig.card(&id);
    assert_eq!(card.return_delivery.as_ref().unwrap().id, new.id);
    assert_eq!(follow_state(&rig, &id), Some(DeliveryState::Accepted));

    // A rework leaves the delivery behind too.
    rig.d.redo("p", &id, "", "user").unwrap();
    assert!(rig.card(&id).return_delivery.is_none());
}

/// 连点两次发送: one delivery.
#[test]
fn sending_twice_queues_one_delivery() {
    let rig = rig("ask");
    rig.host.busy.lock().insert("p".into());
    let id = reported(&rig);
    rig.d.send("p", &id, "user").unwrap();
    assert_eq!(
        rig.d.send("p", &id, "user").unwrap_err(),
        DelegationError::NothingToSend
    );
    // Even past the card's own guard, the queue gives the same one back.
    let card = rig.card(&id);
    rig.d.hand_over("p", vec![card.clone()]);
    rig.d.hand_over("p", vec![card]);
    let list = rig.queue.list("p");
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].state, DeliveryState::Waiting);
    assert_eq!(rig.card(&id).return_delivery.unwrap().id, list[0].id);
    assert!(rig.sent_to("p").is_empty(), "the parent is busy: it waits");
}

/// 宿主在入队后、送达前重启: the card reads its delivery back from the
/// queue — unknown at first, delivered once the parent's record shows it.
#[test]
fn after_a_restart_the_card_takes_its_deliverys_state_from_the_queue() {
    let tmp = std::env::temp_dir().join(format!(
        "aeroric-delegations-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let rig = rig("ask");
    rig.d.set_dir(tmp.clone());
    let id = reported(&rig);
    rig.d.send("p", &id, "user").unwrap();
    wait("accepted", || {
        follow_state(&rig, &id) == Some(DeliveryState::Accepted)
    });
    let delivery = rig.card(&id).return_delivery.unwrap().id;
    wait("saved", || {
        std::fs::read_to_string(tmp.join("tasks/p.json")).is_ok_and(|s| s.contains("\"accepted\""))
    });

    // The host restarts.
    let queue = Deliveries::new(Arc::new(Quiet));
    queue.set_dir(&rig.queue_dir).unwrap();
    let again = Delegations::new(Arc::clone(&queue) as Arc<dyn LiveStatusSink>);
    again.set_dir(tmp.clone());
    again.follow(&queue);
    let card = again.list("p").remove(0);
    let follow = card.return_delivery.clone().unwrap();
    assert_eq!(follow.id, delivery);
    assert_eq!(follow.state, DeliveryState::Unknown, "the answer was lost");
    assert!(card.needs_user());

    *rig.host.recorded.lock() = Some(true);
    queue.set_host(Arc::clone(&rig.host) as Arc<dyn DeliveryHost>);
    let follow = again.list("p").remove(0).return_delivery.unwrap();
    assert_eq!(follow.state, DeliveryState::Delivered);
    assert_eq!(follow.evidence, Some(DeliveryEvidence::Transcript));
    assert!(again.attention().is_empty());
}

/// A card whose delivery the queue no longer has (and that had not
/// settled) is unknown, not sent.
#[test]
fn a_card_whose_delivery_is_gone_is_unknown() {
    let tmp = std::env::temp_dir().join(format!(
        "aeroric-delegations-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let rig = rig("ask");
    rig.d.set_dir(tmp.clone());
    rig.host.busy.lock().insert("p".into());
    let id = reported(&rig);
    rig.d.send("p", &id, "user").unwrap();
    wait("saved", || {
        std::fs::read_to_string(tmp.join("tasks/p.json"))
            .is_ok_and(|s| s.contains("returnDelivery"))
    });
    let empty = std::env::temp_dir().join(format!(
        "aeroric-delegation-queue-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let queue = Deliveries::new(Arc::new(Quiet));
    queue.set_dir(&empty).unwrap();
    let again = Delegations::new(Arc::clone(&queue) as Arc<dyn LiveStatusSink>);
    again.set_dir(tmp);
    again.follow(&queue);
    let follow = again.list("p").remove(0).return_delivery.unwrap();
    assert_eq!(follow.state, DeliveryState::Unknown);
    assert_eq!(follow.reason.as_deref(), Some(RECORD_GONE));
}

/// The agent's own results go back in one delivery that names every card.
#[test]
fn a_batch_is_one_delivery_every_card_follows() {
    let rig = rig("ask");
    let (a, child_a) = rig.running(contract("查 A"), Origin::Session);
    let (b, child_b) = rig.running(contract("查 B"), Origin::Session);
    rig.answer(&child_a, "t1", "A 的结论", None, false);
    rig.answer(&child_b, "t1", "B 的结论", None, false);
    wait("returned", || rig.sent_to("p").len() == 1);
    wait("both follow", || {
        follow_state(&rig, &a) == Some(DeliveryState::Accepted)
            && follow_state(&rig, &b) == Some(DeliveryState::Accepted)
    });
    let one = rig.card(&a).return_delivery.unwrap().id;
    assert_eq!(rig.card(&b).return_delivery.unwrap().id, one);
    let record = rig.queue.get(&one).unwrap();
    assert_eq!(
        record.source_ref,
        Some(source_ref("p", &[b.clone(), a.clone()]))
    );
    assert_eq!(record.label, "委派结果 · 2 项");
}

/// 2026-11-13 Smart Friend: what `ask_expert` asks for.
fn expert_contract(question: &str) -> Contract {
    Contract {
        instruction: prompt::expert_question(question),
        role: Some(EXPERT_ROLE.into()),
        context_mode: Some(ContextMode::Brief),
        permission: Some("plan".into()),
        workspace: Some(Workspace::ReadOnly),
        ..Contract::default()
    }
}

#[test]
fn an_expert_answer_taken_by_the_tool_is_whole_and_not_sent_again() {
    let rig = rig("ask");
    // The parent waits on the tool call: its turn is running.
    rig.host.busy.lock().insert("p".into());
    let (id, child) = rig.running(expert_contract("为什么 loss 不降"), Origin::Session);
    assert_eq!(
        rig.d.take_expert_answer("p", &id).unwrap(),
        ExpertAnswer::Pending
    );
    assert!(rig.host.started.lock()[0].1.contains("为什么 loss 不降"));
    let long = "学习率太大。".repeat(400);
    rig.answer(&child, "t1", &long, None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    match rig.d.take_expert_answer("p", &id).unwrap() {
        ExpertAnswer::Answered { text, .. } => {
            assert_eq!(text, long, "the whole answer, not the card's cut");
            assert!(text.chars().count() > MAX_RESULT_CHARS);
        }
        other => panic!("{other:?}"),
    }
    assert!(rig.card(&id).sent);
    rig.host.busy.lock().clear();
    rig.push(
        "p",
        SessionUpdateKind::TurnEnded {
            turn_id: "u1".into(),
            reason: TurnEndReason::Completed,
            error: None,
            raw_status: None,
        },
    );
    std::thread::sleep(Duration::from_millis(30));
    assert!(rig.sent_to("p").is_empty(), "the tool already gave it back");
}

#[test]
fn an_expert_that_could_not_start_fails_the_tool_and_is_not_sent_again() {
    let rig = rig("ask");
    rig.host.busy.lock().insert("p".into());
    *rig.host.fail_start.lock() = true;
    let card = rig
        .d
        .start("p", expert_contract("怎么办"), Origin::Session, "p")
        .unwrap();
    wait("failed", || {
        rig.card(&card.delegation_id).state == DelegationState::Failed
    });
    assert_eq!(
        rig.d.take_expert_answer("p", &card.delegation_id).unwrap(),
        ExpertAnswer::Failed("本机没有 Codex".into())
    );
    assert!(rig.card(&card.delegation_id).sent);
}

/// Smart Friend eval (2026-11-13): the gateway refused the expert's model
/// and the tool only said the turn did not end normally.
#[test]
fn an_expert_whose_turn_failed_tells_the_tool_why() {
    let rig = rig("ask");
    rig.host.busy.lock().insert("p".into());
    let (id, child) = rig.running(expert_contract("怎么办"), Origin::Session);
    let refused =
        "unexpected status 403 Forbidden: model \"any/claude-opus-5-5\" is set not to be served";
    rig.d.turn_settled(TurnSettled {
        task_id: child,
        turn_id: "t1".into(),
        reason: TurnEndReason::Failed,
        error: Some(refused.into()),
        diff: None,
        checking: false,
    });
    wait("failed", || rig.card(&id).state == DelegationState::Failed);
    assert_eq!(
        rig.d.take_expert_answer("p", &id).unwrap(),
        ExpertAnswer::Failed(format!("子会话的回合没有正常结束：{refused}"))
    );
}

#[test]
fn a_turn_counts_only_its_own_expert_questions() {
    let rig = rig("ask");
    rig.push(
        "p",
        SessionUpdateKind::TurnStarted {
            turn_id: "u1".into(),
        },
    );
    rig.running(expert_contract("一"), Origin::Session);
    rig.running(expert_contract("二"), Origin::Session);
    rig.running(contract("查一下"), Origin::Session);
    assert_eq!(rig.d.expert_asks_this_turn("p"), 2);
    std::thread::sleep(Duration::from_millis(5));
    rig.push(
        "p",
        SessionUpdateKind::TurnStarted {
            turn_id: "u2".into(),
        },
    );
    assert_eq!(rig.d.expert_asks_this_turn("p"), 0);
}

#[test]
fn the_expert_is_kept_until_cleared_and_its_role_only_reads() {
    let tmp =
        std::env::temp_dir().join(format!("aeroric-expert-{}", uuid::Uuid::new_v4().simple()));
    let rig = rig("ask");
    assert!(rig.d.expert().is_none());
    assert!(rig.d.set_expert(None).is_err(), "nowhere to keep it");
    rig.d.set_dir(tmp.clone());
    let choice = ExpertChoice {
        configuration_id: "agent-config:opus".into(),
        agent: "claude".into(),
        model: Some("opus".into()),
        label: "Claude 官方 · opus".into(),
    };
    rig.d.set_expert(Some(choice.clone())).unwrap();
    assert_eq!(rig.d.expert(), Some(choice));
    rig.d.set_expert(None).unwrap();
    rig.d.set_expert(None).unwrap();
    assert!(rig.d.expert().is_none());
    let role = rig
        .d
        .roles()
        .into_iter()
        .find(|r| r.id == EXPERT_ROLE)
        .unwrap();
    assert_eq!(
        (role.permission.as_str(), role.workspace),
        ("plan", Workspace::ReadOnly)
    );
    assert_eq!(role.context_mode, ContextMode::Brief);
    let _ = std::fs::remove_dir_all(tmp);
}

fn criteria(n: usize) -> Contract {
    Contract {
        acceptance: (1..=n)
            .map(|i| Acceptance {
                text: format!("条件 {i}"),
                check: Some(format!("test {i}")),
            })
            .collect(),
        ..contract("改")
    }
}

/// B1: the criteria are the child's obligations before its first input.
#[test]
fn obligations_are_installed_before_the_child_starts() {
    let rig = rig("ask");
    let checkpoints = super::super::checkpoint::CheckpointSink::new(Arc::new(Quiet));
    rig.d.set_checkpoints(Arc::downgrade(&checkpoints));
    *rig.host.checkpoints.lock() = Some(checkpoints.clone());
    let (_, child) = rig.running(criteria(2), Origin::User);
    assert_eq!(*rig.host.obligations_at_start.lock(), [2]);
    let ids: Vec<String> = checkpoints
        .get(&child)
        .unwrap()
        .obligations
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, ["a1", "a2"]);
}

/// B1: obligations that could not be written stop the child from starting.
#[test]
fn a_child_whose_obligations_cannot_be_saved_does_not_start() {
    let rig = rig("ask");
    let checkpoints = super::super::checkpoint::CheckpointSink::new(Arc::new(Quiet));
    // A file where the checkpoint directory should be: every write fails.
    let blocked =
        std::env::temp_dir().join(format!("aeroric-b1-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&blocked, b"not a directory").unwrap();
    checkpoints.set_dir(blocked.join("checkpoints"));
    rig.d.set_checkpoints(Arc::downgrade(&checkpoints));
    let card = rig.d.start("p", criteria(1), Origin::Session, "p").unwrap();
    wait("failed", || {
        rig.card(&card.delegation_id).state == DelegationState::Failed
    });
    let error = rig.card(&card.delegation_id).error.unwrap_or_default();
    assert!(error.starts_with("没能启动：验收条件没存下来"), "{error}");
    assert!(rig.host.started.lock().is_empty(), "no child was started");
    let child = card.child_task_id.unwrap();
    assert!(checkpoints
        .get(&child)
        .is_none_or(|c| c.obligations.is_empty()));
    wait("returned", || rig.sent_to("p").len() == 1);
    let _ = std::fs::remove_file(&blocked);
}

/// B1: a child that fails to start leaves no obligations under its id,
/// which a retry may use again.
#[test]
fn a_child_that_fails_to_start_leaves_no_obligations() {
    let rig = rig("ask");
    let checkpoints = super::super::checkpoint::CheckpointSink::new(Arc::new(Quiet));
    rig.d.set_checkpoints(Arc::downgrade(&checkpoints));
    *rig.host.checkpoints.lock() = Some(checkpoints.clone());
    *rig.host.fail_start.lock() = true;
    let card = rig.d.start("p", criteria(2), Origin::Session, "p").unwrap();
    wait("failed", || {
        rig.card(&card.delegation_id).state == DelegationState::Failed
    });
    assert_eq!(
        *rig.host.obligations_at_start.lock(),
        [2],
        "installed first"
    );
    let child = card.child_task_id.unwrap();
    assert!(checkpoints
        .get(&child)
        .is_none_or(|c| c.obligations.is_empty()));
}

/// B3: a criterion carries the check that passed it and the tree it saw;
/// when a file it rested on changes, the pass is stale and the acceptance
/// no longer counts as passed. A hand-settled one rests on the user.
#[test]
fn a_passed_criterion_has_its_evidence_and_goes_stale_when_files_change() {
    use crate::sessions::live::turn_changes::snapshot_tree;
    use aeroric_protocol::checkpoint::EvidenceKind;
    use aeroric_protocol::session_live::{AutoCheckItem, TurnFileChange};
    use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};

    let rig = rig("ask");
    let checkpoints = super::super::checkpoint::CheckpointSink::new(Arc::new(Quiet));
    rig.d.set_checkpoints(Arc::downgrade(&checkpoints));
    let ledger: Arc<super::super::turn_ledger::TurnLedger> = Arc::default();
    let (id, child) = rig.running(criteria(1), Origin::User);

    let dir = std::env::temp_dir().join(format!("aeroric-b3-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap()
            .status
            .success());
    };
    git(&["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "0\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    let dir = dir.canonicalize().unwrap();
    ledger.set_dir(dir.with_extension("turns"));
    rig.d.set_ledger(Arc::clone(&ledger));
    // The child's turn wrote a.txt.
    let (top, before) = snapshot_tree(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "1\n").unwrap();
    let (_, after) = snapshot_tree(&dir).unwrap();
    let at = crate::sessions::live::now_ms();
    ledger.record(
        &child,
        TurnRecord {
            turn_id: "u1".into(),
            started_ms: at,
            ended_ms: at + 1,
            top: top.to_string_lossy().into(),
            before,
            after: after.clone(),
            head_before: None,
            head_after: None,
            files: vec![TurnFileChange {
                path: "a.txt".into(),
                additions: Some(1),
                deletions: Some(1),
            }],
            additions: 1,
            deletions: 1,
            state: TurnState::Pending,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
            check: None,
        },
    );
    // The auto check passed a1 after that turn.
    std::thread::sleep(Duration::from_millis(5));
    checkpoints.update(&SessionUpdate {
        task_id: child.clone(),
        seq: 7,
        at_ms: crate::sessions::live::now_ms(),
        kind: SessionUpdateKind::AutoCheck {
            turn_id: "u1".into(),
            state: "passed".into(),
            items: vec![AutoCheckItem {
                kind: "tests".into(),
                profile: "acceptance:a1".into(),
                label: "test 1".into(),
                state: "passed".into(),
                summary: String::new(),
                failures: Vec::new(),
                output_tail: String::new(),
                exit_code: None,
                issues: None,
                duration_ms: None,
                optional: false,
                truncated: false,
            }],
            trigger: Some("fileChanges".into()),
            note: None,
            input_revision: Some(after.clone()),
        },
    });
    // Read again on request (the parent agent, the app).
    rig.d.refresh_acceptance("p");
    let card = rig.card(&id);
    let a = &card.acceptance[0];
    assert_eq!(a.state, ObligationState::Passed);
    assert_eq!(a.evidence.len(), 1);
    assert_eq!(a.evidence[0].kind, EvidenceKind::AutoCheck);
    assert_eq!(a.evidence[0].detail, "u1");
    assert_eq!(a.checked_revision.as_deref(), Some(after.as_str()));
    assert!(!a.stale);
    assert!(card.acceptance_passed());

    // Someone edits a.txt afterwards: the pass no longer describes the file.
    std::fs::write(dir.join("a.txt"), "by hand\n").unwrap();
    rig.d.refresh_acceptance("p");
    let card = rig.card(&id);
    assert!(card.acceptance[0].stale, "{:?}", card.acceptance);
    assert!(!card.acceptance_passed());

    // Settled by hand: the evidence is the user's confirmation.
    assert!(checkpoints.set_obligation(&child, "a1", ObligationState::Passed));
    let a = &checkpoints.get(&child).unwrap().obligations[0];
    assert_eq!(a.evidence[0].kind, EvidenceKind::UserConfirmation);
    assert_eq!(a.checked_revision, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// B6 step 2: a child's result names each file it delivered by
/// `artifact_id@version`, so the parent reads exactly that version.
#[test]
fn a_childs_result_names_its_deliverables_by_version() {
    use crate::sessions::live::artifacts::ArtifactRegistry;
    use crate::sessions::live::turn_changes::snapshot_tree;
    use aeroric_protocol::session_live::TurnFileChange;
    use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};

    let rig = rig("ask");
    let ledger: Arc<super::super::turn_ledger::TurnLedger> = Arc::default();
    let registry = Arc::new(ArtifactRegistry::new(Arc::clone(&ledger)));
    rig.d.set_artifacts(Arc::clone(&registry));
    let (id, child) = rig.running(contract("写一份报告"), Origin::User);

    let dir = std::env::temp_dir().join(format!("aeroric-b6-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap()
            .status
            .success());
    };
    git(&["init", "-q"]);
    std::fs::write(dir.join("keep.txt"), "0\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    let dir = dir.canonicalize().unwrap();
    // The child's turn wrote report.md and a scratch file.
    let (top, before) = snapshot_tree(&dir).unwrap();
    std::fs::write(dir.join("report.md"), "# 结论\n").unwrap();
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    std::fs::write(dir.join("tmp/x.txt"), "x").unwrap();
    let (_, after) = snapshot_tree(&dir).unwrap();
    let at = crate::sessions::live::now_ms();
    let record = TurnRecord {
        turn_id: "u1".into(),
        started_ms: at,
        ended_ms: at + 1,
        top: top.to_string_lossy().into(),
        before,
        after,
        head_before: None,
        head_after: None,
        files: ["report.md", "tmp/x.txt"]
            .iter()
            .map(|p| TurnFileChange {
                path: (*p).into(),
                additions: Some(1),
                deletions: Some(0),
            })
            .collect(),
        additions: 2,
        deletions: 0,
        state: TurnState::Pending,
        undone_ms: None,
        undo_group: None,
        state_before_undo: None,
        check: None,
    };
    ledger.record(&child, record.clone());
    registry.register(&child, &record);

    rig.answer(&child, "u1", "报告写好了", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    let card = rig.card(&id);
    assert_eq!(card.artifacts.len(), 1, "{:?}", card.artifacts);
    let named = &card.artifacts[0];
    assert_eq!((named.path.as_str(), named.version), ("report.md", 1));
    let text = prompt::results(std::slice::from_ref(&card));
    assert!(
        text.contains(&format!("- report.md：{}@1", named.artifact_id)),
        "{text}"
    );
    // The parent reads that very version.
    let read = registry
        .read(&child, &named.artifact_id, Some(named.version), 0, None)
        .unwrap();
    assert_eq!(read.content, "# 结论\n");
    std::fs::remove_dir_all(&dir).ok();
}

// MARK: E5-4 invariants

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// E5-4: what holds between the cards and the queue after any step. A card
/// in progress names its child; a card's return delivery is the queue's
/// record as it stands, never more; every delegation item in the queue
/// belongs to a card; and a host started over copies of both files reads
/// the same cards back (those still open as interrupted).
fn card_invariants(rig: &Rig, cards_dir: &std::path::Path) -> Result<(), String> {
    let cards = rig.d.list("p");
    let items = rig.queue.list("p");
    for card in &cards {
        if card.state.is_open() && card.child_task_id.is_none() {
            return Err(format!("{}: in progress with no child", card.delegation_id));
        }
        if let Some(follow) = &card.return_delivery {
            let item = rig
                .queue
                .get(&follow.id)
                .ok_or(format!("{}: its delivery is gone", card.delegation_id))?;
            if (follow.state, follow.evidence) != (item.state, item.evidence) {
                return Err(format!(
                    "{}: card says {:?}/{:?}, queue {:?}/{:?}",
                    card.delegation_id, follow.state, follow.evidence, item.state, item.evidence
                ));
            }
        }
    }
    for item in items.iter().filter(|d| d.kind == DeliveryKind::Delegation) {
        let source = item.source_ref.as_deref().unwrap_or_default();
        if !cards.iter().any(|c| source.contains(&c.delegation_id)) {
            return Err(format!("{}: a result no card owns", item.id));
        }
    }
    // Rebuilt from what is on disk.
    let copy = std::env::temp_dir().join(format!("aeroric-e5-{}", uuid::Uuid::new_v4().simple()));
    copy_tree(cards_dir, &copy.join("cards"));
    copy_tree(&rig.queue_dir, &copy.join("queue"));
    let queue = Deliveries::new(Arc::new(Quiet));
    queue
        .set_dir(&copy.join("queue"))
        .map_err(|e| e.to_string())?;
    let again = Delegations::new(Arc::clone(&queue) as Arc<dyn LiveStatusSink>);
    again.set_dir(copy.join("cards"));
    again.follow(&queue);
    let back = again.list("p");
    std::fs::remove_dir_all(&copy).ok();
    if back.len() != cards.len() {
        return Err(format!("{} cards read back of {}", back.len(), cards.len()));
    }
    for card in &cards {
        let b = back
            .iter()
            .find(|c| c.delegation_id == card.delegation_id)
            .ok_or("a card was not read back")?;
        let expected = if card.state.is_open() {
            DelegationState::Interrupted
        } else {
            card.state
        };
        if b.state != expected || b.child_task_id != card.child_task_id {
            return Err(format!(
                "{}: read back as {:?}",
                card.delegation_id, b.state
            ));
        }
        let ids = |c: &DelegationCard| c.return_delivery.as_ref().map(|r| r.id.clone());
        if ids(b) != ids(card) {
            return Err(format!("{}: follows another delivery", card.delegation_id));
        }
        if let Some(follow) = &b.return_delivery {
            let record = queue.get(&follow.id).map(|d| d.state);
            if record != Some(follow.state) {
                return Err(format!(
                    "{}: read back {:?}, record {record:?}",
                    card.delegation_id, follow.state
                ));
            }
        }
    }
    Ok(())
}

/// Wait for the cards' file to catch up, then insist.
fn hold(rig: &Rig, cards_dir: &std::path::Path, step: &str) {
    let start = Instant::now();
    loop {
        match card_invariants(rig, cards_dir) {
            Ok(()) => return,
            Err(error) if start.elapsed() > Duration::from_secs(3) => {
                panic!("after {step}: {error}")
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// E5-4: one delegation from start to a result proven delivered late, the
/// invariants checked after every step; the parent hears the result once.
#[test]
fn a_delegation_keeps_its_invariants_from_start_to_a_late_proven_delivery() {
    let tmp = std::env::temp_dir().join(format!(
        "aeroric-e5-cards-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let rig = rig("ask");
    rig.d.set_dir(tmp.clone());
    let turn_ended = |rig: &Rig| {
        rig.push(
            "p",
            SessionUpdateKind::TurnEnded {
                turn_id: "u".into(),
                reason: TurnEndReason::Completed,
                error: None,
                raw_status: None,
            },
        )
    };

    let (id, child) = rig.running(contract("查"), Origin::User);
    hold(&rig, &tmp, "the child started");
    rig.answer(&child, "t1", "结论", None, false);
    wait("reported", || {
        rig.card(&id).state == DelegationState::Reported
    });
    rig.d.ack_probes("p", &id).unwrap();
    hold(&rig, &tmp, "the child reported");

    rig.host.busy.lock().insert("p".into());
    rig.d.send("p", &id, "user").unwrap();
    hold(&rig, &tmp, "queued behind a busy parent");
    rig.host.busy.lock().clear();
    turn_ended(&rig);
    wait("accepted", || {
        follow_state(&rig, &id) == Some(DeliveryState::Accepted)
    });
    hold(&rig, &tmp, "handed over");
    let first = rig.card(&id).return_delivery.unwrap().id;

    // The parent's session dies: nobody knows whether it arrived.
    rig.d.status(
        "project-1",
        "p",
        1,
        TaskStatus::Failed,
        Some("进程退出".into()),
    );
    wait("unknown", || {
        follow_state(&rig, &id) == Some(DeliveryState::Unknown)
    });
    hold(&rig, &tmp, "the answer was lost");

    // The user sends it again while the parent is busy; then the first
    // one's receipt shows it had arrived.
    rig.host.busy.lock().insert("p".into());
    rig.d.send("p", &id, "user").unwrap();
    hold(&rig, &tmp, "sent again");
    rig.push(
        "p",
        SessionUpdateKind::DeliveryReceipt {
            id: first.clone(),
            outcome: ReceiptOutcome::Delivered,
            reason: None,
        },
    );
    wait("delivered", || {
        follow_state(&rig, &id) == Some(DeliveryState::Delivered)
    });
    hold(&rig, &tmp, "proven delivered late");
    assert_eq!(rig.card(&id).return_delivery.unwrap().id, first);
    assert_eq!(
        rig.d.send("p", &id, "user").unwrap_err(),
        DelegationError::NothingToSend,
        "not a third time"
    );

    rig.host.busy.lock().clear();
    turn_ended(&rig);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(rig.sent_to("p").len(), 1, "the parent heard it once");
    std::fs::remove_dir_all(&tmp).ok();
}

/// D5: a plan starts nothing until confirmed; then its free units start as
/// delegations, a dependent starts when its prerequisite reported, with that
/// prerequisite named in its brief; after a restart the plan reads back from
/// the plan and the cards alone.
#[test]
fn a_confirmed_plan_starts_its_units_in_order() {
    use aeroric_protocol::work_plan::{UnitState, WorkPlan, WorkUnit};
    let tmp = std::env::temp_dir().join(format!("aeroric-plans-{}", uuid::Uuid::new_v4().simple()));
    let rig = rig("auto_edit");
    rig.d.set_dir(tmp.clone());
    let unit = |id: &str, deps: &[&str]| WorkUnit {
        id: id.into(),
        title: format!("单元{id}"),
        brief: format!("做 {id}"),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        ..WorkUnit::default()
    };
    let draft = WorkPlan {
        parent_task_id: "p".into(),
        title: "拆开做".into(),
        units: vec![unit("a", &[]), unit("b", &["a"]), unit("c", &[])],
        ..WorkPlan::default()
    };
    let cyclic = WorkPlan {
        units: vec![unit("x", &["y"]), unit("y", &["x"])],
        ..draft.clone()
    };
    assert!(rig.d.save_plan(cyclic, 1).unwrap_err().contains("有环"));

    let saved = rig.d.save_plan(draft, 1).unwrap();
    let id = saved.plan.id.clone();
    assert!(!saved.plan.confirmed);
    assert_eq!(saved.statuses[0].reason, "计划还没确认");
    assert!(
        rig.host.started.lock().is_empty(),
        "nothing before the user confirms"
    );

    let view = rig.d.confirm_plan("p", &id).unwrap();
    assert_eq!(rig.host.started.lock().len(), 2, "a and c");
    let status = |v: &super::plans::PlanView, u: &str| {
        v.statuses.iter().find(|s| s.id == u).unwrap().clone()
    };
    assert_eq!(status(&view, "b").reason, "等「单元a」通过验收");
    let a = view.plan.units[0].delegation_id.clone().unwrap();
    wait("a running", || {
        rig.card(&a).state == DelegationState::Running
    });
    assert!(rig
        .d
        .save_plan(view.plan.clone(), 2)
        .unwrap_err()
        .contains("已经确认"));

    // a reports (no criteria: reporting is passing): b starts, told about a.
    let a_child = rig.card(&a).child_task_id.unwrap();
    rig.answer(&a_child, "t1", "a 做完了", None, false);
    wait("b started", || rig.host.started.lock().len() == 3);
    let (card, prompt) = rig.host.started.lock()[2].clone();
    assert!(
        card.contract.instruction.starts_with("做 b"),
        "{}",
        card.contract.instruction
    );
    assert!(prompt.contains("做 b"));
    let view = rig.d.plans("p").remove(0);
    assert_eq!(status(&view, "a").state, UnitState::Passed);
    assert_eq!(status(&view, "b").state, UnitState::Running);

    // Asking again starts nothing twice.
    rig.d.advance_plans("p");
    assert_eq!(rig.host.started.lock().len(), 3);

    // A restart: the plan and the cards say where things stand (the
    // running ones were interrupted by it).
    wait("saved", || {
        std::fs::read_to_string(tmp.join("tasks/p.json")).is_ok()
    });
    let again = Delegations::new(Arc::new(Quiet));
    again.set_dir(tmp.clone());
    let view = again.plans("p").remove(0);
    assert_eq!(status(&view, "a").state, UnitState::Passed);
    assert_eq!(status(&view, "b").state, UnitState::Failed);
    assert_eq!(status(&view, "b").reason, "失败了，等你处理");
}
