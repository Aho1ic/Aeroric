//! Slice 97: execution face + production confirmation loop over live UDS.
//! The confirm request is appended to the host EventLog under the legacy
//! channel name (executor "task-status" precedent); the test reads it back
//! through `events.since` and answers through the real UDS method — the same
//! surfaces the frontend pump slices will use. Scratch SQLite only; no real
//! database, no OS keychain.

use std::time::Duration;

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(reply) => reply.result,
        other => panic!("expected success, got {other:?}"),
    }
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(reply) = reply else {
        panic!("expected failure, got {reply:?}");
    };
    assert_eq!(reply.error.code, code, "{reply:?}");
    reply.error.message
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(
        reader,
        writer,
        &Harness::request("execution-request", method, params),
    )
    .await
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "execution").await);
    (reader, writer)
}

fn connection_body(host: &Harness, id: &str, name: &str, production: bool) -> Value {
    json!({
        "id": id,
        "name": name,
        "dbType": "sqlite",
        "readOnly": false,
        "dbx": {
            "id": id,
            "name": name,
            "db_type": "sqlite",
            "host": host.home.join(format!("{id}.sqlite")).to_string_lossy(),
            "port": 0,
            "username": "",
            "password": "",
            "is_production": production
        },
        "createdAt": 0
    })
}

async fn save_connection(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    body: Value,
) {
    success(
        call(
            reader,
            writer,
            "database.dbx_save_connection",
            json!({ "connection": body }),
        )
        .await,
    );
}

async fn events_watermark(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
) -> u64 {
    let reply = success(call(reader, writer, "events.since", json!({ "after": 0 })).await);
    reply["latestSeq"].as_u64().unwrap_or(0)
}

async fn wait_for_confirm_event(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    mut after: u64,
) -> Value {
    let mut seen: Vec<Value> = Vec::new();
    for _ in 0..200 {
        let reply = success(call(reader, writer, "events.since", json!({ "after": after })).await);
        after = reply["latestSeq"].as_u64().unwrap_or(after);
        if let Some(events) = reply["events"].as_array() {
            for event in events {
                if event["event"] == "dbx-production-confirm-requested" {
                    return event["data"].clone();
                }
            }
            seen.extend(events.iter().cloned());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("confirm request never appeared in the event log; seen={seen:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn assess_target_and_sql_are_served_with_saved_connections() {
    let host = Harness::start("execution-assess").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    save_connection(
        &mut reader,
        &mut writer,
        connection_body(&host, "prod-1", "Production", true),
    )
    .await;

    let reply = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_assess_production_target",
            json!({ "request": { "connectionId": "prod-1", "database": null } }),
        )
        .await,
    );
    // is_production 连接整体都是生产环境：需要确认、库列表为空（整连接语义）。
    assert_eq!(reply["requiresConfirmation"], json!(true));
    assert_eq!(reply["productionDatabases"], json!([]));

    let reply = success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_assess_production_sql",
            json!({ "request": { "connectionId": "prod-1", "database": "app", "sql": "select * from users" } }),
        )
        .await,
    );
    assert_eq!(reply["requiresConfirmation"], json!(false));
    assert_eq!(reply["isMutation"], json!(false));

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_assess_production_target",
            json!({ "request": { "connectionId": "missing", "database": null } }),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(message.contains("Connection config not found"));
}

#[tokio::test(flavor = "multi_thread")]
async fn production_write_confirmation_loop_end_to_end() {
    let host = Harness::start("execution-confirm").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    save_connection(
        &mut reader,
        &mut writer,
        connection_body(&host, "prod-2", "Production", true),
    )
    .await;
    let watermark = events_watermark(&mut reader, &mut writer).await;

    // 发起生产写：确认闸先于连接——请求会挂起等待前端答复。
    let (frame_tx, mut frame_rx) = tokio::sync::oneshot::channel::<Frame>();
    let mut execute = tokio::task::spawn({
        let socket = host.socket_path.clone();
        let token = host.token.clone();
        async move {
            let session = tokio::net::UnixStream::connect(&socket).await.unwrap();
            let (r, mut w) = session.into_split();
            let mut r = tokio::io::BufReader::new(r);
            success(Harness::handshake(&mut r, &mut w, &token, "execute").await);
            // 确认会挂起超过 harness 的 5s 看门狗——这里手写慢读（30s 上限）。
            let line = aeroric_protocol::envelope::encode_line(&Harness::request(
                "exec-1",
                "database.dbx_execute_query",
                json!({ "request": {
                    "connectionId": "prod-2",
                    "sql": "delete from users where id = 1"
                }}),
            ))
            .unwrap();
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
            w.write_all(line.as_bytes()).await.unwrap();
            w.flush().await.unwrap();
            let mut buf = Vec::new();
            tokio::time::timeout(Duration::from_secs(30), r.read_until(b'\n', &mut buf))
                .await
                .expect("confirm refusal must arrive within 30 s")
                .expect("connection alive");
            let frame = aeroric_protocol::envelope::decode_line(&buf).expect("valid frame");
            let _ = frame_tx.send(frame.clone());
            frame
        }
    });

    // 哨兵事件：把 watermark 抬过 since(0) 的「首连不补发」语义（EventLog
    // 设计：after==0 永远返回空）。真实消费者连接时日志通常已有事件；
    // 测试用 harness 暴露的同一 EventLog 垫一条，确认事件即可被 since 读回。
    host.services
        .log
        .append("test-sentinel", json!({ "probe": true }));
    let confirm = tokio::select! {
        frame = &mut frame_rx => panic!("execute returned before confirm: {frame:?}"),
        joined = &mut execute => panic!("execute task settled before confirm: {joined:?}"),
        confirm = wait_for_confirm_event(&mut reader, &mut writer, watermark) => confirm,
    };
    assert_eq!(confirm["connection"], json!("Production"));
    assert_eq!(confirm["sql"], json!("delete from users where id = 1"));
    let request_id = confirm["requestId"]
        .as_str()
        .expect("requestId")
        .to_string();

    // 拒绝：执行方必须拿到逐字拒绝文案，绝不放行。
    success(
        call(
            &mut reader,
            &mut writer,
            "database.respond_dbx_production_confirmation",
            json!({ "requestId": request_id, "approved": false }),
        )
        .await,
    );
    let reply = execute.await.unwrap();
    let message = failure(reply, ErrorCode::IoError);
    assert_eq!(message, "Production database operation cancelled.");
}

#[tokio::test(flavor = "multi_thread")]
async fn respond_unknown_request_id_is_an_error_and_malformed_input_refused() {
    let host = Harness::start("execution-respond").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.respond_dbx_production_confirmation",
            json!({ "requestId": "nope", "approved": true }),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(message.contains("No pending production confirmation"));

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.respond_dbx_production_confirmation",
            json!({ "requestId": "nope" }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("approved"));

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_execute_query",
            json!({}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("request"));

    // 取消在飞查询：注册表里没有也只是 no-op 成功（legacy 语义）。
    success(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_cancel_query",
            json!({ "executionId": "ghost" }),
        )
        .await,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unregistered_database_methods_stay_unsupported() {
    let host = Harness::start("execution-boundary").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // 尚未上收的方法必须保持不可达：宿主不虚报能力（dbx_list_databases 等
    // schema/驱动面属后续切片；本切片只到执行面 11 条）。
    for method in [
        "database.dbx_list_databases",
        "database.dbx_list_schemas",
        "database.dbx_list_objects",
        "database.dbx_get_columns",
        "database.dbx_redis_get_value",
        "database.dbx_mongo_find_documents",
        "database.dbx_start_transfer",
    ] {
        let reply = call(&mut reader, &mut writer, method, json!({})).await;
        assert_eq!(
            failure(reply, ErrorCode::UnsupportedCapability),
            "this host does not offer the requested method",
            "{method}"
        );
    }
}
