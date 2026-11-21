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
