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
            },
        );
        self.d.turn_settled(TurnSettled {
            task_id: task.into(),
            turn_id: turn.into(),
            reason: TurnEndReason::Completed,
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
