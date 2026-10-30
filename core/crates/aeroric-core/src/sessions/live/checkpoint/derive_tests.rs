use super::*;
use aeroric_protocol::session_live::{AutoCheckItem, PlanEntry, TurnEndReason, TurnFileChange};
use serde_json::json;

fn run(updates: Vec<SessionUpdateKind>) -> Building {
    let mut b = Building::new(Checkpoint::new("t1"));
    for (i, kind) in updates.into_iter().enumerate() {
        b.apply(&SessionUpdate {
            task_id: "t1".into(),
            seq: i as u64 + 1,
            at_ms: 0,
            kind,
        });
    }
    b
}

fn call(id: &str, kind: ToolKind, title: &str) -> SessionUpdateKind {
    SessionUpdateKind::ToolCall {
        id: id.into(),
        kind,
        title: title.into(),
        input: json!({}),
    }
}

fn done(id: &str, status: ToolStatus) -> SessionUpdateKind {
    SessionUpdateKind::ToolCallUpdate {
        id: id.into(),
        status,
        output_delta: None,
        result: None,
    }
}

fn ask(id: &str, call: &str, summary: &str) -> SessionUpdateKind {
    SessionUpdateKind::PermissionRequest {
        id: id.into(),
        tool_call_id: Some(call.into()),
        kind: "command".into(),
        summary: summary.into(),
        options: vec!["allowOnce".into(), "deny".into()],
    }
}

fn resolved(id: &str, decision: &str) -> SessionUpdateKind {
    SessionUpdateKind::PermissionResolved {
        id: id.into(),
        decision: decision.into(),
    }
}

fn end() -> SessionUpdateKind {
    SessionUpdateKind::TurnEnded {
        turn_id: "u".into(),
        reason: TurnEndReason::Completed,
        error: None,
    }
}

#[test]
fn a_finished_command_is_an_effect_with_its_evidence() {
    let b = run(vec![
        call("c1", ToolKind::Command, "cargo test"),
        done("c1", ToolStatus::Completed),
        end(),
    ]);
    let cp = &b.checkpoint;
    assert_eq!(cp.turn, 1);
    assert_eq!(cp.effects.len(), 1);
    assert_eq!(cp.effects[0].summary, "cargo test");
    assert_eq!(cp.effects[0].status, Epistemic::Verified);
    assert_eq!(cp.effects[0].evidence[0].seq, Some(2));
    assert!(cp.unknown.is_empty());
}

#[test]
fn reads_and_searches_are_not_effects() {
    let b = run(vec![
        call("c1", ToolKind::WebSearch, "搜索 reqwest"),
        done("c1", ToolStatus::Completed),
        call("c2", ToolKind::Other, "Read src/main.rs"),
        end(),
    ]);
    assert!(b.checkpoint.effects.is_empty());
    assert!(b.checkpoint.unknown.is_empty());
}

#[test]
fn a_call_without_a_result_when_the_turn_ends_is_unknown() {
    let b = run(vec![
        call("c1", ToolKind::Command, "cargo test search_timeout"),
        end(),
    ]);
    let cp = &b.checkpoint;
    assert_eq!(cp.unknown.len(), 1);
    assert_eq!(cp.unknown[0].status, Epistemic::Unknown);
    assert!(cp.needs_attention());
}

#[test]
fn a_later_run_of_the_same_command_clears_its_unknown() {
    let b = run(vec![
        call("c1", ToolKind::Command, "cargo test"),
        end(),
        call("c2", ToolKind::Command, "cargo test"),
        done("c2", ToolStatus::Completed),
        end(),
    ]);
    assert!(b.checkpoint.unknown.is_empty());
    assert_eq!(b.checkpoint.effects.len(), 1);
}

#[test]
fn an_approval_is_recorded_when_asked_and_saved_at_once() {
    let mut b = Building::new(Checkpoint::new("t1"));
    b.apply(&SessionUpdate {
        task_id: "t1".into(),
        seq: 1,
        at_ms: 0,
        kind: call("c1", ToolKind::Command, "git push"),
    });
    let change = b.apply(&SessionUpdate {
        task_id: "t1".into(),
        seq: 2,
        at_ms: 0,
        kind: ask("p1", "c1", "git push origin fix"),
    });
    assert_eq!(change, Change::Save);
    let intent = &b.checkpoint.intents[0];
    assert_eq!(intent.state, IntentState::Pending);
    assert_eq!(intent.summary, "git push origin fix");
    assert_eq!(intent.verify, "看命令的退出码与输出");
}

#[test]
fn a_denied_approval_is_denied_and_no_effect() {
    let b = run(vec![
        call("c1", ToolKind::Command, "git push"),
        ask("p1", "c1", "git push"),
        resolved("p1", "deny"),
        done("c1", ToolStatus::Declined),
        end(),
    ]);
    let cp = &b.checkpoint;
    assert_eq!(cp.intents[0].state, IntentState::Denied);
    assert!(cp.effects.is_empty());
    assert!(cp.unknown.is_empty());
}

#[test]
fn an_allowed_approval_that_ran_is_executed() {
    let b = run(vec![
        call("c1", ToolKind::Command, "git push"),
        ask("p1", "c1", "git push"),
        resolved("p1", "allowOnce"),
        done("c1", ToolStatus::Completed),
        end(),
    ]);
    let cp = &b.checkpoint;
    assert_eq!(cp.intents[0].state, IntentState::Executed);
    assert_eq!(cp.effects[0].kind, "commit");
    assert_eq!(cp.effects[0].evidence[0].kind, EvidenceKind::Commit);
}

#[test]
fn an_allowed_approval_with_no_result_is_outcome_unknown() {
    let b = run(vec![
        call("c1", ToolKind::Command, "rm -rf build"),
        ask("p1", "c1", "rm -rf build"),
        resolved("p1", "allowOnce"),
        end(),
    ]);
    let cp = &b.checkpoint;
    assert_eq!(cp.intents[0].state, IntentState::OutcomeUnknown);
    assert_eq!(cp.unknown.len(), 1);
}

#[test]
fn the_first_user_message_is_the_goal() {
    let mut b = Building::new(Checkpoint::new("t1"));
    assert_eq!(b.note_prompt("修复搜索超时", None), Change::Changed);
    assert_eq!(b.note_prompt("再加个测试", None), Change::None);
    let goal = b.checkpoint.goal.unwrap();
    assert_eq!(goal.text, "修复搜索超时");
    assert_eq!(goal.status, Epistemic::Verified);
}

#[test]
fn turn_changes_land_on_the_turn_that_just_ended() {
    let b = run(vec![
        end(),
        SessionUpdateKind::TurnChanges {
            turn_id: "u".into(),
            files: vec![
                TurnFileChange {
                    path: "a.rs".into(),
                    additions: Some(3),
                    deletions: Some(1),
                },
                TurnFileChange {
                    path: "b.rs".into(),
                    additions: Some(1),
                    deletions: Some(0),
                },
                TurnFileChange {
                    path: "c.rs".into(),
                    additions: Some(1),
                    deletions: Some(0),
                },
            ],
            additions: 5,
            deletions: 1,
            truncated: false,
        },
    ]);
    let effect = &b.checkpoint.effects[0];
    assert_eq!(effect.turn, 1);
    assert_eq!(effect.summary, "a.rs、b.rs 等 3 个文件 +5 −1");
    assert_eq!(effect.evidence[0].kind, EvidenceKind::TurnChanges);
}

#[test]
fn the_plan_replaces_plan_obligations_only() {
    let mut b = Building::new(Checkpoint::new("t1"));
    b.checkpoint.obligations.push(Obligation {
        id: "a1".into(),
        text: "测试通过".into(),
        check: Some("cargo test".into()),
        state: ObligationState::Open,
        source: ObligationSource::Acceptance,
    });
    let plan = |done: &str| SessionUpdateKind::Plan {
        entries: vec![
            PlanEntry {
                text: "读代码".into(),
                status: done.into(),
            },
            PlanEntry {
                text: "改代码".into(),
                status: "pending".into(),
            },
        ],
    };
    for (seq, kind) in [plan("inProgress"), plan("completed")]
        .into_iter()
        .enumerate()
    {
        b.apply(&SessionUpdate {
            task_id: "t1".into(),
            seq: seq as u64 + 1,
            at_ms: 0,
            kind,
        });
    }
    let o = &b.checkpoint.obligations;
    assert_eq!(o.len(), 3);
    assert_eq!(o[0].id, "a1");
    assert_eq!(o[1].state, ObligationState::Passed);
    assert_eq!(o[2].state, ObligationState::Open);
}

#[test]
fn an_acceptance_check_result_settles_its_obligation() {
    let mut b = Building::new(Checkpoint::new("t1"));
    b.checkpoint.obligations.push(Obligation {
        id: "a1".into(),
        text: "测试通过".into(),
        check: Some("cargo test".into()),
        state: ObligationState::Open,
        source: ObligationSource::Acceptance,
    });
    let change = b.apply(&SessionUpdate {
        task_id: "t1".into(),
        seq: 1,
        at_ms: 0,
        kind: SessionUpdateKind::AutoCheck {
            turn_id: "u".into(),
            state: "passed".into(),
            items: vec![AutoCheckItem {
                kind: "tests".into(),
                profile: "acceptance:a1".into(),
                label: "测试通过".into(),
                state: "passed".into(),
                summary: "通过".into(),
                failures: vec![],
                output_tail: String::new(),
            }],
        },
    });
    assert_eq!(change, Change::Save);
    assert_eq!(b.checkpoint.obligations[0].state, ObligationState::Passed);
}

#[test]
fn effects_beyond_the_cap_fold_into_a_count() {
    let mut kinds = Vec::new();
    for i in 0..(MAX_ENTRIES + 5) {
        kinds.push(call(
            &format!("c{i}"),
            ToolKind::Command,
            &format!("echo {i}"),
        ));
        kinds.push(done(&format!("c{i}"), ToolStatus::Completed));
    }
    let b = run(kinds);
    assert_eq!(b.checkpoint.effects.len(), MAX_ENTRIES);
    assert_eq!(b.checkpoint.effects_folded, 5);
    assert_eq!(b.checkpoint.effects[0].summary, "echo 5");
}

#[test]
fn an_answered_question_is_a_decision() {
    let b = run(vec![
        SessionUpdateKind::Question {
            id: "q1".into(),
            style: "select".into(),
            title: "用哪个测试框架？".into(),
            message: None,
            options: vec![],
            placeholder: None,
            timeout_ms: None,
        },
        SessionUpdateKind::QuestionResolved {
            id: "q1".into(),
            outcome: "answered".into(),
        },
    ]);
    assert_eq!(b.checkpoint.decisions[0].text, "已回答：用哪个测试框架？");
}

#[test]
fn long_text_is_cut() {
    let long = "字".repeat(MAX_TEXT_CHARS + 10);
    assert_eq!(cut(&long).chars().count(), MAX_TEXT_CHARS);
    assert!(cut(&long).ends_with('…'));
}

#[test]
fn a_clarification_turn_becomes_a_card_and_its_goal_is_the_request() {
    let mut b = Building::new(Checkpoint::new("t1"));
    b.note_prompt(
        &crate::sessions::live::checkpoint::clarify::wrap("修复超时"),
        None,
    );
    assert_eq!(b.checkpoint.goal.as_ref().unwrap().text, "修复超时");
    for (seq, kind) in [
        SessionUpdateKind::MessageChunk {
            message_id: "m1".into(),
            role: "assistant".into(),
            text: "```json\n{\"acceptance\": [{\"text\": \"测试通过\", \"check\": \"cargo test\"}]}\n```".into(),
        },
        end(),
    ]
    .into_iter()
    .enumerate()
    {
        b.apply(&SessionUpdate {
            task_id: "t1".into(),
            seq: seq as u64 + 1,
            at_ms: 0,
            kind,
        });
    }
    match b.clarification.take() {
        Some(SessionUpdateKind::Clarification {
            acceptance, raw, ..
        }) => {
            assert_eq!(acceptance[0].text, "测试通过");
            assert_eq!(acceptance[0].check.as_deref(), Some("cargo test"));
            assert_eq!(raw, None);
        }
        other => panic!("{other:?}"),
    }
    // Only the first turn clarifies.
    b.apply(&SessionUpdate {
        task_id: "t1".into(),
        seq: 9,
        at_ms: 0,
        kind: end(),
    });
    assert!(b.clarification.is_none());
}
