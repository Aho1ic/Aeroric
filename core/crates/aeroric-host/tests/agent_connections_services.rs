//! B1 第 3 步：`agentConnections.*` 宿主方法的判据。
//!
//! 1. 凭据只进钥匙串：文件里没有明文，回包只给 "stored"/"missing"，不回显值。
//! 2. 引用检查：被号池或配置引用的连接不能删；删成功后凭据一并删除（Q27）。
//! 3. 候选查询：按映射名聚合多个渠道（Q25），不可用的给具名原因。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::clock::SystemClock;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_host::dispatcher::HostServices;
use aeroric_host::startup::{run_startup, serve, HostOptions, RunningHost};
use aeroric_protocol::envelope::{decode_line, encode_line, Frame, Request};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

struct LocalProbe;

impl StorageLocationProbe for LocalProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        StorageLocation {
            directory: data_dir.to_path_buf(),
            volume: VolumeKind::Local,
            private_permissions_enforced: true,
        }
    }
}

struct Rig {
    root: PathBuf,
    data_dir: PathBuf,
    socket: PathBuf,
    running: Option<RunningHost>,
}

impl Rig {
    async fn start(tag: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "ahac-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let paths = AppPaths::from_user_home(&home);
        let data_dir = paths.data_dir().to_path_buf();
        let socket = data_dir.join("host.sock");
        let started = run_startup(
            &HostOptions {
                paths,
                socket_path: socket.clone(),
                ready_fd: None,
            },
            Arc::new(SystemClock),
            &LeaseOwner {
                application: "aeroric-host-agent-connections-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
            "build-hash-test",
            "protocol-hash-test",
        )
        .await
        .expect("startup");
        let token = Arc::clone(&started.token);
        let context = Arc::clone(&started.context);
        let running = serve(
            started,
            HostServices::new(
                token,
                Arc::new(aeroric_core::events::EventLog::default()),
                context,
            ),
        );
        Self {
            root,
            data_dir,
            socket,
            running: Some(running),
        }
    }

    fn document(&self) -> String {
        std::fs::read_to_string(self.data_dir.join("agent-connections.json")).unwrap_or_default()
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let stream = UnixStream::connect(&self.socket).await.expect("connect");
        let (read, mut writer) = stream.into_split();
        let mut reader = BufReader::new(read);
        let token = std::fs::read_to_string(self.data_dir.join("host.token"))
            .expect("token")
            .trim()
            .to_string();
        let hello = send(
            &mut reader,
            &mut writer,
            "h",
            "handshake",
            json!({ "token": token, "clientId": "test" }),
        )
        .await;
        assert!(matches!(hello, Frame::Success(_)), "{hello:?}");
        match send(&mut reader, &mut writer, "c", method, params).await {
            Frame::Success(s) => Ok(s.result),
            Frame::Failure(f) => Err(serde_json::to_value(f.error).unwrap()),
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.shutdown();
        }
        std::fs::remove_dir_all(&self.root).ok();
    }
}

async fn send(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) -> Frame {
    let frame = Frame::Request(Request {
        v: 1,
        id: id.to_string(),
        method: method.to_string(),
        params,
        idempotency_key: None,
        extra: Default::default(),
    });
    let line = encode_line(&frame).expect("encode");
    writer.write_all(line.as_bytes()).await.expect("write");
    writer.flush().await.expect("flush");
    let mut reply = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_line(&mut reply),
    )
    .await
    .expect("reply in time")
    .expect("read");
    decode_line(reply.trim_end().as_bytes()).expect("decode")
}

fn connection(id: &str, protocol: &str, upstream: &str) -> Value {
    json!({
        "id": id,
        "label": id,
        "kind": "api_key",
        "vendor": "other",
        "base_url": format!("https://{id}.example.com/v1"),
        "protocols": [protocol],
        "models": [{ "upstream_id": upstream, "mapped_name": "deepseek-v4.1-flash" }],
    })
}

#[tokio::test]
async fn credentials_stay_out_of_the_file_and_the_reply() {
    let rig = Rig::start("cred").await;
    let reply = rig
        .call(
            "agentConnections.putConnection",
            json!({ "connection": connection("a", "messages", "ds-flash"), "credential": "sk-secret-123" }),
        )
        .await
        .expect("put");
    assert_eq!(reply["credentials"]["a"], "stored");
    assert!(!reply.to_string().contains("sk-secret-123"));
    assert!(!rig.document().contains("sk-secret-123"));

    // Empty credential revokes immediately (Q27).
    let reply = rig
        .call(
            "agentConnections.setCredential",
            json!({ "connectionId": "a", "credential": "" }),
        )
        .await
        .expect("revoke");
    assert_eq!(reply["credentials"]["a"], "missing");
}

#[tokio::test]
async fn referenced_connections_cannot_be_deleted() {
    let rig = Rig::start("refs").await;
    rig.call(
        "agentConnections.putConnection",
        json!({ "connection": connection("a", "messages", "ds-flash"), "credential": "k" }),
    )
    .await
    .expect("put");
    rig.call(
        "agentConnections.putPool",
        json!({ "pool": { "id": "p", "label": "p", "members": [{ "connection_id": "a" }] } }),
    )
    .await
    .expect("pool");
    // A config that targets the connection directly blocks the delete: silently
    // retargeting it would change what new tasks run on.
    rig.call(
        "agentConnections.putConfig",
        json!({ "config": {
            "id": "cfg", "label": "cfg", "harness": "claude",
            "target": { "kind": "connection", "connection_id": "a" }
        } }),
    )
    .await
    .expect("config");
    let refused = rig
        .call("agentConnections.deleteConnection", json!({ "id": "a" }))
        .await;
    assert!(refused.is_err(), "{refused:?}");
    assert!(rig.document().contains("\"a\""));

    // Pool membership does not block; the member is dropped with the connection.
    rig.call("agentConnections.deleteConfig", json!({ "id": "cfg" }))
        .await
        .expect("delete config");
    let reply = rig
        .call("agentConnections.deleteConnection", json!({ "id": "a" }))
        .await
        .expect("delete connection");
    assert!(reply["document"]["connections"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(reply["document"]["pools"][0]["members"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(reply["credentials"].as_object().unwrap().len(), 0);
}

#[tokio::test]
async fn candidates_group_channels_by_mapped_name() {
    let rig = Rig::start("route").await;
    for (id, protocol, upstream) in [
        ("a", "messages", "ds-flash-a"),
        ("b", "chat_completions", "ds-flash-b"),
    ] {
        rig.call(
            "agentConnections.putConnection",
            json!({ "connection": connection(id, protocol, upstream), "credential": "k" }),
        )
        .await
        .expect("put");
    }
    rig.call(
        "agentConnections.putConfig",
        json!({ "config": {
            "id": "cfg", "label": "flash", "harness": "claude",
            "target": { "kind": "route" }, "default_model": "deepseek-v4.1-flash"
        } }),
    )
    .await
    .expect("config");

    let reply = rig
        .call("agentConnections.candidates", json!({ "configId": "cfg" }))
        .await
        .expect("candidates");
    assert_eq!(reply["models"], json!(["deepseek-v4.1-flash"]));
    let list = reply["candidates"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{reply}");
    let upstreams: Vec<&str> = list
        .iter()
        .map(|c| c["upstream_model"].as_str().unwrap())
        .collect();
    assert!(upstreams.contains(&"ds-flash-a") && upstreams.contains(&"ds-flash-b"));
    // Chat Completions channel is reachable for Claude only through the gateway.
    let b = list.iter().find(|c| c["connection_id"] == "b").unwrap();
    assert_ne!(b["reach"], json!("direct"), "{b}");

    let missing = rig
        .call(
            "agentConnections.candidates",
            json!({ "configId": "cfg", "model": "no-such-model" }),
        )
        .await
        .expect("candidates");
    assert!(missing["candidates"].as_array().unwrap().is_empty());
    assert_eq!(missing["excluded"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn fallback_models_are_checked_when_a_config_is_saved() {
    let rig = Rig::start("fallback").await;
    rig.call(
        "agentConnections.putConnection",
        json!({ "connection": connection("a", "messages", "ds-flash"), "credential": "k" }),
    )
    .await
    .expect("put");
    let config = |fallbacks: serde_json::Value| {
        json!({ "config": {
            "id": "cfg", "label": "cfg", "harness": "claude",
            "target": { "kind": "route" }, "default_model": "main",
            "fallback_models": fallbacks
        } })
    };
    for bad in [
        json!(["main"]),
        json!(["x", "x"]),
        json!([" "]),
        json!(["a", "b", "c", "d"]),
    ] {
        let refused = rig
            .call("agentConnections.putConfig", config(bad.clone()))
            .await;
        assert!(refused.is_err(), "{bad} should be refused");
    }
    rig.call(
        "agentConnections.putConfig",
        config(json!(["backup-1", "backup-2"])),
    )
    .await
    .expect("config");
    assert!(rig.document().contains("backup-2"));
}

#[tokio::test]
async fn fetch_without_credential_fails_and_keeps_the_catalog() {
    let rig = Rig::start("fetch").await;
    rig.call(
        "agentConnections.putConnection",
        json!({ "connection": connection("a", "messages", "ds-flash") }),
    )
    .await
    .expect("put");
    let before = rig.document();
    let failed = rig
        .call(
            "agentConnections.fetchModels",
            json!({ "connectionId": "a" }),
        )
        .await;
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(rig.document(), before);
}

/// A one-shot model catalog: answers the first request with two models and
/// reports the request it saw.
async fn one_shot_models() -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        let body = r#"{"data":[{"id":"m-a"},{"id":"m-b"}]}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await.unwrap();
        request
    });
    (base, task)
}

/// 2026-10-26: the list can be fetched from what is typed, before saving
/// (cc-switch `fetch_models_for_config`); nothing is written.
#[tokio::test]
async fn models_are_fetched_for_an_unsaved_draft_without_saving_it() {
    let rig = Rig::start("draft-fetch").await;
    let before = rig.document();
    let (base, upstream) = one_shot_models().await;
    let mut draft = connection("new", "chat_completions", "kept-model");
    draft["base_url"] = json!(base);
    let missing = rig
        .call(
            "agentConnections.fetchModels",
            json!({ "connection": draft.clone() }),
        )
        .await;
    assert!(
        missing.is_err(),
        "no typed key and none stored: {missing:?}"
    );

    let reply = rig
        .call(
            "agentConnections.fetchModels",
            json!({ "connection": draft, "credential": "sk-typed" }),
        )
        .await
        .expect("fetch");
    assert_eq!(reply["fetched"], 2);
    assert_eq!(
        reply["models"],
        json!(["m-a", "m-b"]),
        "the raw list, for the picker"
    );
    let ids: Vec<&str> = reply["connection"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["upstream_id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"kept-model") && ids.contains(&"m-a") && ids.contains(&"m-b"),
        "{ids:?}"
    );
    let request = upstream.await.unwrap().to_ascii_lowercase();
    assert!(request.contains("/v1/models"), "{request}");
    assert!(request.contains("sk-typed"));
    assert_eq!(rig.document(), before, "a draft fetch never saves");
}

/// A one-shot Chat Completions upstream: answers the first request with
/// "pong" and reports the Authorization header it saw.
async fn one_shot_chat() -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"pong"}}]}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await.unwrap();
        request
    });
    (base, task)
}

#[tokio::test]
async fn a_test_message_goes_through_the_connection_with_its_key() {
    let rig = Rig::start("probe").await;
    let (base, upstream) = one_shot_chat().await;
    let mut conn = connection("p", "chat_completions", "m-1");
    conn["base_url"] = json!(base);
    rig.call(
        "agentConnections.putConnection",
        json!({ "connection": conn, "credential": "sk-probe" }),
    )
    .await
    .expect("put");
    let reply = rig
        .call(
            "agentConnections.testConnection",
            json!({ "connectionId": "p", "model": "m-1" }),
        )
        .await
        .expect("probe");
    assert_eq!(reply["protocol"], "chat_completions");
    assert_eq!(reply["probe"]["status"], 200);
    assert_eq!(reply["probe"]["excerpt"], "pong");
    let request = upstream.await.unwrap();
    assert!(request.contains("/v1/chat/completions"));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer sk-probe"));
    assert!(request.contains("\"max_tokens\":64"), "room for a reasoning model to answer");
}

#[tokio::test]
async fn status_and_official_logins_are_reported_without_secrets() {
    let rig = Rig::start("status").await;
    let status = rig
        .call("agentConnections.status", json!({}))
        .await
        .expect("status");
    assert!(status["cooldowns"].is_array());
    assert!(status["downgrades"].is_array());
    assert!(status["reasoningStripped"].is_array());
    let official = rig
        .call("agentConnections.officialStatus", json!({}))
        .await
        .expect("official");
    for cli in ["codex", "claude"] {
        assert!(official[cli]["signedIn"].is_boolean(), "{official}");
        assert!(official[cli]["sources"].is_array());
    }
    let wire = official.to_string();
    assert!(!wire.contains("access_token") && !wire.contains("refresh_token"));
}
