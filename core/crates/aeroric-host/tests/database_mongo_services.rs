//! Slice 101: mongo face over live UDS. Without a real mongo server the
//! machine-testable surface is parameter decode, connection-refusal shape,
//! and wired registry — normalize/cancel logic unit-tested in core
//! `database::mongo_face`.

use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

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
        &Harness::request("mongo-request", method, params),
    )
    .await
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "mongo").await);
    (reader, writer)
}

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(reply) => reply.result,
        other => panic!("expected success, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mongo_methods_decode_params_and_refuse_without_a_server() {
    let host = Harness::start("mongo-decode").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // 缺参数 → InvalidArgument。
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_mongo_list_collections",
            json!({ "connectionId": "c" }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("database"), "{message}");

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_mongo_insert_document",
            json!({ "connectionId": "c", "database": "d", "collection": "c" }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("docJson"), "{message}");

    // 缺连接 → ensure_connected 的 IoError。
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "database.dbx_mongo_list_databases",
            json!({ "connectionId": "missing" }),
        )
        .await,
        ErrorCode::IoError,
    );
    assert!(!message.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn all_six_mongo_methods_are_registered() {
    let host = Harness::start("mongo-registered").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    for method in [
        "database.dbx_mongo_list_databases",
        "database.dbx_mongo_list_collections",
        "database.dbx_mongo_find_documents",
        "database.dbx_mongo_insert_document",
        "database.dbx_mongo_update_document",
        "database.dbx_mongo_delete_documents",
    ] {
        let reply = call(&mut reader, &mut writer, method, json!({})).await;
        let message = failure(reply, ErrorCode::InvalidArgument);
        assert!(!message.contains("does not offer"), "{method} registered");
    }
}
