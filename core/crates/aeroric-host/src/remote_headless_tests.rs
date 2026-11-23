//! D4 phase 2: a phone's answer to a structured session's request is told
//! what the agent got, through the driver's settlement (A2).

use std::sync::Arc;
use std::time::Duration;

use aeroric_core::sessions::live::receipts::PermissionReceipts;
use aeroric_core::sessions::live::{
    ConfigChange, Decision, DriverError, LiveStatusSink, SessionDriver, SessionState, SlashCommand,
};
use aeroric_protocol::session_live::{PermissionSendState, SessionUpdate, SessionUpdateKind};
use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use parking_lot::Mutex;

use super::answer_structured;

struct Nothing;
impl LiveStatusSink for Nothing {
    fn status(&self, _: &str, _: &str, _: u64, _: TaskStatus, _: Option<String>) {}
    fn session(&self, _: &str, _: &str, _: ProtocolFamily, _: &str, _: &str, _: Option<&str>) {}
}

#[derive(Clone, Copy)]
enum Then {
    Written,
    NotWritten,
    Ended,
    Busy,
    Gone,
    Silent,
}

/// Answers as a driver would: takes the answer, then publishes the
/// settlement through the sink chain a moment later (off its own thread).
struct Fake {
    then: Then,
    receipts: Arc<PermissionReceipts>,
    answered: Mutex<Vec<String>>,
}

impl SessionDriver for Fake {
    fn family(&self) -> ProtocolFamily {
        ProtocolFamily::Claude
    }
    fn attach(&self) -> Result<(), DriverError> {
        Ok(())
    }
    fn prompt(&self, _: String) -> Result<(), DriverError> {
        Ok(())
    }
    fn interrupt(&self) -> Result<(), DriverError> {
        Ok(())
    }
    fn respond(&self, request_id: &str, decision: Decision) -> Result<(), DriverError> {
        self.answered.lock().push(decision.wire().to_string());
        let kind = match self.then {
            Then::Busy => return Err(DriverError::AlreadyAnswered(request_id.into())),
            Then::Gone => return Err(DriverError::UnknownRequest(request_id.into())),
            Then::Silent => return Ok(()),
            Then::Written => SessionUpdateKind::PermissionResolved {
                id: request_id.into(),
                decision: decision.wire().into(),
            },
            Then::NotWritten => SessionUpdateKind::PermissionSendFailed {
                id: request_id.into(),
                state: PermissionSendState::Failed,
                reason: "审批没送出去：写不进 Claude 的输入".into(),
            },
            Then::Ended => SessionUpdateKind::PermissionSendFailed {
                id: request_id.into(),
                state: PermissionSendState::Ended,
                reason: "审批没送出去：这次请求已随连接结束".into(),
            },
        };
        let receipts = Arc::clone(&self.receipts);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            receipts.update(&SessionUpdate {
                task_id: "t1".into(),
                seq: 1,
                at_ms: 1,
                kind,
            });
        });
        Ok(())
    }
    fn configure(&self, _: ConfigChange) -> Result<(), DriverError> {
        Ok(())
    }
    fn command(&self, _: SlashCommand) -> Result<(), DriverError> {
        Ok(())
    }
    fn close(&self) -> Result<(), DriverError> {
        Ok(())
    }
    fn snapshot(&self) -> SessionState {
        SessionState::default()
    }
    fn idle_since_ms(&self) -> Option<u64> {
        None
    }
    fn suspend(&self) {}
    fn rebind(&self, _: u64) {}
}

async fn answer(then: Then, approve: bool) -> (serde_json::Value, Vec<String>) {
    let receipts = PermissionReceipts::new(Arc::new(Nothing));
    let driver = Fake {
        then,
        receipts: Arc::clone(&receipts),
        answered: Mutex::default(),
    };
    let reply = answer_structured(
        &driver,
        &receipts,
        "t1",
        "p1",
        approve,
        Duration::from_millis(300),
    )
    .await;
    let answered = driver.answered.lock().clone();
    (reply, answered)
}

/// Each way the driver can settle becomes what the phone is told; the
/// answer is sent once, as approve or deny.
#[tokio::test]
async fn the_phone_is_told_what_the_agent_got() {
    let (reply, sent) = answer(Then::Written, true).await;
    assert_eq!(reply["accepted"], true);
    assert_eq!(reply["outcome"], "resolved");
    assert_eq!(sent, ["allowOnce"]);

    let (reply, sent) = answer(Then::Written, false).await;
    assert_eq!(reply["outcome"], "resolved");
    assert_eq!(sent, ["deny"]);

    let (reply, _) = answer(Then::NotWritten, true).await;
    assert_eq!(reply["accepted"], false);
    assert_eq!(reply["outcome"], "sendFailed");
    assert!(reply["reason"].as_str().unwrap().contains("写不进"));

    let (reply, _) = answer(Then::Ended, true).await;
    assert_eq!(reply["outcome"], "ended");

    let (reply, _) = answer(Then::Busy, true).await;
    assert_eq!(reply["outcome"], "answeredElsewhere");
    let (reply, _) = answer(Then::Gone, true).await;
    assert_eq!(reply["outcome"], "answeredElsewhere");

    let (reply, sent) = answer(Then::Silent, true).await;
    assert_eq!(reply["accepted"], false);
    assert_eq!(reply["outcome"], "unconfirmed");
    assert_eq!(sent.len(), 1, "never sent again");
}

/// D4 step 8 (2026-11-23): a task with a live session lists its open
/// requests for the phone; one without a session, or with none open, is
/// left as it was.
#[test]
fn a_live_sessions_open_requests_ride_on_its_task_record() {
    use aeroric_protocol::session_live::PendingPermission;
    use serde_json::json;
    let pending = |id: &str, subagent: Option<&str>| PendingPermission {
        id: id.into(),
        kind: "command".into(),
        summary: format!("run {id}"),
        tool_call_id: None,
        options: vec!["allowOnce".into(), "deny".into()],
        subagent: subagent.map(str::to_string),
    };
    let mut records = json!([
        {"id": "t1", "name": "a", "approval": {"requestId": "hook-1", "kind": "exec"}},
        {"id": "t2", "name": "b"},
        {"id": "t3", "name": "c"},
    ]);
    super::attach_live_pending(&mut records, |task| match task {
        "t1" => Some(vec![pending("r1", None), pending("r2", Some("reviewer"))]),
        "t2" => Some(vec![]),
        _ => None,
    });
    assert_eq!(
        records[0]["pendingApprovals"],
        json!([
            {"requestId": "r1", "kind": "command", "summary": "run r1"},
            {"requestId": "r2", "kind": "command", "summary": "run r2", "subagent": "reviewer"},
        ])
    );
    assert_eq!(
        records[0]["approval"]["requestId"], "hook-1",
        "the hook approval stays"
    );
    assert!(records[1].get("pendingApprovals").is_none());
    assert!(records[2].get("pendingApprovals").is_none());
}

/// D4 step 8: a live session asking for an approval, or ending a turn, is
/// written as `task-changed` (the phone event bridge pushes it); a message
/// chunk is not.
#[test]
fn approvals_and_turns_are_written_for_the_phone() {
    use aeroric_core::events::{EventLog, EventObserver, LoggedEvent};
    #[derive(Default)]
    struct Seen(Mutex<Vec<(String, String)>>);
    impl EventObserver for Seen {
        fn observed(&self, event: &LoggedEvent) {
            self.0.lock().push((
                event.event.clone(),
                event.data["change"].as_str().unwrap_or("").to_string(),
            ));
        }
    }
    let seen = Arc::new(Seen::default());
    let log = Arc::new(EventLog::default().with_observer(seen.clone()));
    let sink = crate::live::TaskServiceSink::new(Arc::clone(&log));
    let update = |kind: SessionUpdateKind| SessionUpdate {
        task_id: "t1".into(),
        seq: 1,
        at_ms: 1,
        kind,
    };
    sink.update(&update(SessionUpdateKind::PermissionRequest {
        id: "r1".into(),
        tool_call_id: None,
        kind: "command".into(),
        summary: "ls".into(),
        options: vec![],
        subagent: None,
    }));
    sink.update(&update(SessionUpdateKind::MessageChunk {
        message_id: "m".into(),
        role: "assistant".into(),
        text: "hi".into(),
    }));
    sink.update(&update(SessionUpdateKind::PermissionResolved {
        id: "r1".into(),
        decision: "allowOnce".into(),
    }));
    assert_eq!(
        *seen.0.lock(),
        [
            ("task-changed".to_string(), "permissionRequest".to_string()),
            ("task-changed".to_string(), "permissionResolved".to_string()),
        ]
    );
}

/// D4 (2026-11-23): a phone gets the latest messages of a transcript, long
/// blocks cut and marked, attachment data left out, and how many it missed.
#[test]
fn a_phone_gets_the_latest_messages_cut_to_size() {
    use aeroric_core::sessions::{SessionContent, SessionMessage};
    let text = |role: &str, text: &str| SessionMessage {
        role: role.into(),
        content: vec![SessionContent::Text { text: text.into() }],
        message_id: None,
    };
    let mut messages: Vec<SessionMessage> =
        (0..50).map(|i| text("user", &format!("m{i}"))).collect();
    messages.push(SessionMessage {
        role: "assistant".into(),
        content: vec![
            SessionContent::ToolUse {
                id: "c1".into(),
                name: "Bash".into(),
                input: "x".repeat(super::PHONE_BLOCK_CHARS + 10),
            },
            SessionContent::Attachment {
                name: "a.png".into(),
                media_type: "image/png".into(),
                source: "base64...".into(),
            },
            SessionContent::Text {
                text: "完成".into(),
            },
        ],
        message_id: Some("last".into()),
    });

    let reply = super::phone_transcript(messages.clone(), None);
    assert_eq!(reply["available"], true);
    assert_eq!(reply["total"], 51);
    assert_eq!(reply["omitted"], 51 - super::PHONE_MESSAGES_DEFAULT);
    let shown = reply["messages"].as_array().unwrap();
    assert_eq!(shown.len(), super::PHONE_MESSAGES_DEFAULT);
    assert_eq!(
        shown[0]["content"][0]["text"], "m11",
        "the latest, oldest first"
    );
    let last = &shown[shown.len() - 1];
    assert_eq!(last["messageId"], "last");
    let tool = &last["content"][0];
    assert_eq!(tool["type"], "tool_use");
    assert_eq!(
        tool["input"].as_str().unwrap().chars().count(),
        super::PHONE_BLOCK_CHARS
    );
    assert_eq!(tool["truncated"], true);
    assert!(
        last["content"][1].get("source").is_none(),
        "no attachment data"
    );
    assert_eq!(last["content"][1]["truncated"], true);
    assert_eq!(
        last["content"][2],
        serde_json::json!({"type": "text", "text": "完成"})
    );

    let few = super::phone_transcript(messages.clone(), Some(3));
    assert_eq!(few["messages"].as_array().unwrap().len(), 3);
    assert_eq!(few["omitted"], 48);
    let capped = super::phone_transcript(messages.clone(), Some(10_000));
    assert_eq!(capped["messages"].as_array().unwrap().len(), 51);
    assert_eq!(capped["omitted"], 0);
    assert_eq!(
        super::phone_transcript(Vec::new(), Some(0))["messages"],
        serde_json::json!([])
    );
}

/// D4 (2026-11-23): a task's changes are read in its own copy when it has
/// one, and the answer says which tree it was.
#[test]
fn a_task_s_changes_come_from_its_own_copy_when_it_has_one() {
    assert_eq!(
        super::tree_to_read("/p", Some("/p/.aeroric/wt/t1")),
        ("/p/.aeroric/wt/t1".to_string(), "worktree")
    );
    assert_eq!(
        super::tree_to_read("/p", None),
        ("/p".to_string(), "project")
    );
    assert_eq!(
        super::tree_to_read("/p", Some(" ")),
        ("/p".to_string(), "project")
    );
}

/// D4 (2026-11-23): a phone's message to a structured session and its 停止
/// go through this host's own `session.prompt` / `session.interrupt`, called
/// in process; their answer or refusal comes back as it is.
#[tokio::test]
async fn a_host_method_is_called_in_process_as_an_authenticated_client() {
    use aeroric_protocol::envelope::{Frame, Success};
    use aeroric_protocol::error::ErrorCode;
    let seen = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let mut set = crate::dispatcher::ServiceSet::new();
    let record = Arc::clone(&seen);
    set.register(
        "session.prompt",
        Arc::new(move |request, _context| {
            record.lock().push(request.params.clone());
            let id = request.id.clone();
            Box::pin(async move {
                Frame::Success(Success {
                    v: 1,
                    id,
                    ok: true,
                    result: serde_json::json!({"accepted": true, "deliveryId": "d1"}),
                    extra: Default::default(),
                })
            })
        }),
    );
    set.register(
        "session.interrupt",
        Arc::new(|request, _context| {
            let id = request.id.clone();
            Box::pin(async move {
                crate::dispatcher::failure(id, ErrorCode::Busy, "暂停所有 Agent 已打开")
            })
        }),
    );
    let set = Arc::new(set);
    let answer = super::call_in_process(
        &set,
        "session.prompt",
        serde_json::json!({"taskId": "t1", "text": "继续"}),
    )
    .await
    .unwrap();
    assert_eq!(answer["deliveryId"], "d1");
    assert_eq!(
        *seen.lock(),
        [serde_json::json!({"taskId": "t1", "text": "继续"})]
    );
    assert_eq!(
        super::call_in_process(
            &set,
            "session.interrupt",
            serde_json::json!({"taskId": "t1"})
        )
        .await,
        Err("暂停所有 Agent 已打开".to_string())
    );
    assert!(
        super::call_in_process(&set, "session.nothing", serde_json::json!({}))
            .await
            .is_err()
    );
}
