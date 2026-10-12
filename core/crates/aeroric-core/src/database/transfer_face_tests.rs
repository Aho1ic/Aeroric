//! 切片一百零三：Tauri `transfer.rs` 既有 8 条单测随体上收（自目标拒绝三种
//! 形状、终止状态折叠四条），进度事件测试改钉 sink 收到的 payload 序列化
//! 形状（camelCase wire 与前端 `isTerminalDbxTransferProgress` 的两个字面量）。

use super::{
    reject_self_target_transfer, terminal_status_for_exit, terminal_status_for_fk_result,
    TransferExit, TransferProgressSink, TRANSFER_PROGRESS_TOPIC,
};
use dbx_core::transfer::{TransferProgress, TransferRequest, TransferStatus};

fn request() -> TransferRequest {
    TransferRequest {
        transfer_id: "test".to_string(),
        source_connection_id: "conn".to_string(),
        source_database: "db".to_string(),
        source_schema: "public".to_string(),
        source_catalog: None,
        target_connection_id: "conn".to_string(),
        target_database: "db".to_string(),
        target_schema: "public".to_string(),
        target_catalog: None,
        tables: vec!["users".to_string()],
        create_table: true,
        content: Default::default(),
        objects: Vec::new(),
        mode: Default::default(),
        target_table_name_case: Default::default(),
        ownership_policy: Default::default(),
        batch_size: 100,
    }
}

#[test]
fn rejects_an_identical_source_and_target_object() {
    let error = reject_self_target_transfer(&request()).expect_err("self-target must fail");
    assert!(error.contains("users"));
}

#[test]
fn allows_a_different_database_or_schema() {
    let mut different_database = request();
    different_database.target_database = "other".to_string();
    assert!(reject_self_target_transfer(&different_database).is_ok());

    let mut different_schema = request();
    different_schema.target_schema = "archive".to_string();
    assert!(reject_self_target_transfer(&different_schema).is_ok());
}

#[test]
fn allows_a_different_target_table_or_connection() {
    let mut different_table = request();
    different_table.tables = vec!["Users".to_string()];
    different_table.target_table_name_case = dbx_core::transfer::TransferTableNameCase::Upper;
    assert!(reject_self_target_transfer(&different_table).is_ok());

    let mut different_connection = request();
    different_connection.target_connection_id = "other-conn".to_string();
    assert!(reject_self_target_transfer(&different_connection).is_ok());
}

#[test]
fn transfer_progress_serializes_to_the_frontend_wire_shape() {
    // 前端用 listen("dbx-transfer-progress") 收进度(src/lib/databaseApi.ts:107),
    // 并按 transferId 过滤、按 status/terminal 判终止。事件名由 Tauri 壳与宿主
    // sink 各自钉住;这里钉 payload 形状——拼错一个字段或退回 snake_case,进度条
    // 就永远不动、传输永远显示不出终态。
    struct Collect(std::sync::Mutex<Vec<serde_json::Value>>);
    impl TransferProgressSink for Collect {
        fn report(&self, progress: &TransferProgress) {
            self.0
                .lock()
                .expect("progress sink")
                .push(serde_json::to_value(progress).expect("serializable"));
        }
    }
    let sink = Collect(std::sync::Mutex::new(Vec::new()));
    sink.report(&TransferProgress {
        transfer_id: "transfer-7".to_string(),
        table: "orders".to_string(),
        table_index: 1,
        total_tables: 2,
        rows_transferred: 40,
        total_rows: Some(100),
        status: TransferStatus::Error,
        error: Some("target table is read-only".to_string()),
        terminal: true,
    });
    let events = sink.0.lock().expect("progress sink");
    assert_eq!(events.len(), 1);
    let payload = &events[0];
    assert_eq!(payload["transferId"], "transfer-7");
    assert_eq!(payload["table"], "orders");
    assert_eq!(payload["tableIndex"], 1);
    assert_eq!(payload["totalTables"], 2);
    assert_eq!(payload["rowsTransferred"], 40);
    assert_eq!(payload["totalRows"], 100);
    // isTerminalDbxTransferProgress 比对的是这两个字面量。
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["terminal"], true);
    assert_eq!(payload["error"], "target table is read-only");
}

#[test]
fn reports_done_when_no_deferred_foreign_key_failed() {
    let (status, error) = terminal_status_for_fk_result(&[]);
    assert!(matches!(status, TransferStatus::Done), "got {status:?}");
    assert_eq!(error, None);
}

#[test]
fn folds_foreign_key_failures_into_the_terminal_event() {
    // 前端把 status === "error" 当终止事件,所以外键失败必须走 Error 而不是
    // 另发一条非终止事件,否则用户只会看到"传输完成"、外键却没挂上。
    let (status, error) =
        terminal_status_for_fk_result(&["orders".to_string(), "users".to_string()]);
    assert!(matches!(status, TransferStatus::Error), "got {status:?}");
    let error = error.expect("failed foreign keys must surface an error message");
    assert!(
        error.contains("orders"),
        "message should name the table: {error}"
    );
    assert!(
        error.contains("users"),
        "message should name the table: {error}"
    );
}

#[test]
fn cancelling_never_reports_deferred_foreign_key_failures_as_errors() {
    // 取消时还没轮到的表不存在,指向它们的 ALTER 必然失败。把这个报成 Error
    // 会让每次取消都弹一条假故障。
    let (status, error) = terminal_status_for_exit(
        &TransferExit::Cancelled,
        &["orders".to_string(), "users".to_string()],
    );
    assert!(
        matches!(status, TransferStatus::Cancelled),
        "got {status:?}"
    );
    assert_eq!(error, None);
}

#[test]
fn a_failed_table_keeps_its_own_error_instead_of_the_foreign_key_one() {
    // 原始失败原因比"外键没挂上"更有诊断价值。
    let (status, error) = terminal_status_for_exit(
        &TransferExit::Failed("target table is read-only".to_string()),
        &["orders".to_string()],
    );
    assert!(matches!(status, TransferStatus::Error), "got {status:?}");
    assert_eq!(error.as_deref(), Some("target table is read-only"));
}

#[test]
fn only_a_completed_transfer_turns_foreign_key_failures_into_an_error() {
    let (status, error) = terminal_status_for_exit(&TransferExit::Completed, &[]);
    assert!(matches!(status, TransferStatus::Done), "got {status:?}");
    assert_eq!(error, None);

    let (status, error) =
        terminal_status_for_exit(&TransferExit::Completed, &["orders".to_string()]);
    assert!(matches!(status, TransferStatus::Error), "got {status:?}");
    assert!(error.is_some_and(|message| message.contains("orders")));
}

#[test]
fn the_v1_topic_name_is_pinned() {
    // EventLog 消费者按这个 topic 过滤;拼错会让进度永远查不到。
    assert_eq!(TRANSFER_PROGRESS_TOPIC, "dbx.transfer_progress");
}
