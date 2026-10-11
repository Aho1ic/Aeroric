//! Slice 99: notebook rag_index / rag_context over live UDS. Vault comes from
//! the real register_vault service; embedding never touches a real provider —
//! the index path refuses at probe_dimension against a dead endpoint, the
//! context path serves the no-index shape without any network.

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
        &Harness::request("rag-request", method, params),
    )
    .await
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "rag").await);
    (reader, writer)
}

/// 注册一个真实目录为 vault（走真实的 register_vault 服务）。
async fn seed_vault(host: &Harness) -> String {
    let vault = host.home.join("vault");
    std::fs::create_dir_all(&vault).expect("mkdir vault");
    let (mut reader, mut writer) = authenticated(host).await;
    success(
        call(
            &mut reader,
            &mut writer,
            "notebook.register_vault",
            json!({ "path": vault.to_string_lossy() }),
        )
        .await,
    );
    vault.to_string_lossy().into_owned()
}

fn dead_endpoint_config() -> Value {
    // provider 要发真请求才能探维度——测试钉死端口：连接拒绝就是机器可测的
    // 「provider 连不上」形状，绝不连接真实 embedding 服务。
    json!({
        "provider": "openAi",
        "baseUrl": "http://127.0.0.1:1/v1",
        "model": "text-embedding-3-small",
        "apiKey": ""
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn rag_context_serves_the_no_index_shape_without_any_network() {
    let host = Harness::start("rag-context-empty").await;
    let vault = seed_vault(&host).await;
    let (mut reader, mut writer) = authenticated(&host).await;

    let bundle = success(
        call(
            &mut reader,
            &mut writer,
            "notebook.rag_context",
            json!({
                "vault": vault,
                "query": "hello",
                "config": dead_endpoint_config()
            }),
        )
        .await,
    );
    // 没索引也要出上下文：当前笔记本身仍然是有用的上下文,而这正是「刚装好、
    // 还没建索引」时用户的第一次提问。
    assert_eq!(bundle["vectorsMissing"], json!(true));
    assert_eq!(bundle["degraded"], json!([]));
    // ContextBundle 的 assembled 是 #[serde(flatten)]——字段在顶层（扁平 wire）。
    assert!(bundle["text"].is_string());
    assert!(bundle["citations"].is_array());
    assert_eq!(bundle["truncated"], json!(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn rag_context_refuses_a_current_note_outside_the_vault() {
    let host = Harness::start("rag-context-cross").await;
    let vault = seed_vault(&host).await;
    // 第二个 vault：A vault 的库不得装配 B vault 的笔记。
    let other = host.home.join("other-vault");
    std::fs::create_dir_all(&other).expect("mkdir other");
    let note = other.join("outside.md");
    std::fs::write(&note, "# hello").expect("write note");
    let (mut reader, mut writer) = authenticated(&host).await;
    success(
        call(
            &mut reader,
            &mut writer,
            "notebook.register_vault",
            json!({ "path": other.to_string_lossy() }),
        )
        .await,
    );

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "notebook.rag_context",
            json!({
                "vault": vault,
                "query": "hello",
                "config": dead_endpoint_config(),
                "currentPath": note.to_string_lossy()
            }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert_eq!(message, "Notebook note is outside the given vault");
}

#[tokio::test(flavor = "multi_thread")]
async fn rag_index_refuses_at_probe_against_a_dead_provider() {
    let host = Harness::start("rag-index-dead").await;
    let vault = seed_vault(&host).await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // embed 客户端带退避重试（500ms·2^n），整轮 probe 会超过 harness 的 5s
    // 看门狗——这里手写慢读（60s 上限），断言的是最终拒绝形状。
    let line = aeroric_protocol::envelope::encode_line(&Harness::request(
        "rag-index-1",
        "notebook.rag_index",
        json!({
            "vault": vault,
            "config": dead_endpoint_config(),
            "scope": "all"
        }),
    ))
    .unwrap();
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    writer.write_all(line.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(60), reader.read_until(b'\n', &mut buf))
        .await
        .expect("probe refusal must arrive within 60 s")
        .expect("connection alive");
    let reply = aeroric_protocol::envelope::decode_line(&buf).unwrap();
    let message = failure(reply, ErrorCode::IoError);
    // probe_dimension 的失败文案来自 provider 客户端（连接拒绝），不是 panic、
    // 也不是「成功建了空索引」。
    assert!(!message.is_empty(), "probe failure carries a message");

    // 任务登记随线程结束注销：失败后同一 vault 可以再次发起（不再报 already running）。
    // 第二轮同样带重试退避——慢读。
    let line = aeroric_protocol::envelope::encode_line(&Harness::request(
        "rag-index-2",
        "notebook.rag_index",
        json!({
            "vault": vault,
            "config": dead_endpoint_config()
        }),
    ))
    .unwrap();
    writer.write_all(line.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(60), reader.read_until(b'\n', &mut buf))
        .await
        .expect("second refusal must arrive within 60 s")
        .expect("connection alive");
    // 第二次同样在 probe 失败（IoError），而不是 already-running 拒绝——
    // NativeJobGuard 的 Drop 注销是「重试」路径的前提。
    let second = failure(
        aeroric_protocol::envelope::decode_line(&buf).unwrap(),
        ErrorCode::IoError,
    );
    assert!(!second.contains("already running"), "{second}");
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_rag_params_refuse() {
    let host = Harness::start("rag-malformed").await;
    let vault = seed_vault(&host).await;
    let (mut reader, mut writer) = authenticated(&host).await;

    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "notebook.rag_context",
            json!({ "vault": vault }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("query"));

    let message = failure(
        call(&mut reader, &mut writer, "notebook.rag_index", json!({})).await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("vault"));
}
