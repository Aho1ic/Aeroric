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
