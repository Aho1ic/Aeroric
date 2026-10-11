//! Slice 100: redis face over live UDS. Without a real redis server the
//! machine-testable surface is the parameter decode, the connection refusal
//! shape, and the wired registry — the normalize/adapter/safety-guard logic
//! is unit-tested in core `database::redis_face` (8 migrated tests).

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
        &Harness::request("redis-request", method, params),
    )
    .await
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "redis").await);
    (reader, writer)
}

#[tokio::test(flavor = "multi_thread")]
async fn redis_methods_are_served_and_decode_their_params() {
    let host = Harness::start("redis-served").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // 缺连接 → ensure_connected 的 IoError（参数 decode 已通过才算到达业务）。
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_redis_list_databases",
            json!({ "connectionId": "missing" }),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(!message.is_empty());

    // 缺参数 → InvalidArgument（decode 先于业务）。
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_redis_get_value",
            json!({ "connectionId": "c", "db": 0 }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("keyRaw"), "{message}");

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_redis_scan_keys",
            json!({ "connectionId": "c", "cursor": 0 }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("db"), "{message}");

    // create_key 的 request 对象 decode。
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_redis_create_key",
            json!({ "request": { "connectionId": "c", "db": 0 } }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("keyRaw"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn all_eighteen_redis_methods_are_registered() {
    let host = Harness::start("redis-registered").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // 18 条全部不在「未注册」状态：缺参数回 InvalidArgument 而不是
    // UnsupportedCapability（后者＝宿主不提供该方法）。
    let methods = [
        ("database.dbx_redis_list_databases", json!({})),
        ("database.dbx_redis_scan_keys", json!({})),
        ("database.dbx_redis_get_value", json!({})),
        ("database.dbx_redis_load_more", json!({})),
        ("database.dbx_redis_set_value", json!({})),
        ("database.dbx_redis_delete_key", json!({})),
        ("database.dbx_redis_set_ttl", json!({})),
        ("database.dbx_redis_create_key", json!({})),
        ("database.dbx_redis_hash_del", json!({})),
        ("database.dbx_redis_hash_set", json!({})),
        ("database.dbx_redis_list_remove", json!({})),
        ("database.dbx_redis_list_push", json!({})),
        ("database.dbx_redis_list_set", json!({})),
        ("database.dbx_redis_set_remove", json!({})),
        ("database.dbx_redis_set_add", json!({})),
        ("database.dbx_redis_zrem", json!({})),
        ("database.dbx_redis_zadd", json!({})),
        ("database.dbx_redis_execute_command", json!({})),
    ];
    for (method, params) in methods {
        let reply = call(&mut reader, &mut writer, method, params).await;
        let message = failure(reply, ErrorCode::InvalidArgument);
        assert!(
            !message.contains("does not offer"),
            "{method} must be registered"
        );
    }
}
