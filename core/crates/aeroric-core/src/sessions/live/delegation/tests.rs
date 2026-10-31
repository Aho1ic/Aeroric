use super::*;
use std::time::Instant;

use aeroric_protocol::checkpoint::Acceptance;
use aeroric_protocol::delegation::{Budget, ContextMode};

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
}

impl DelegationHost for FakeHost {
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
    let d = Delegations::new(Arc::new(Quiet));
    d.set_host(Arc::clone(&host) as Arc<dyn DelegationHost>);
    d.set_watch_every(Duration::from_millis(10));
    Rig {
        host,
        d,
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
    assert_eq!(resolved.workspace, Workspace::Shared);
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
