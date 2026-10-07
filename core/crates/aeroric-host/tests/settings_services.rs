//! P8-05 切片九：设置面宿主服务的判据。
//!
//! 三条判据，每条对应用户可见的后果：
//!
//! 1. **单字段更新保留其余键**——`settings.json` 是**一整份 map**（core 的刻意选择，
//!    因为旧客户端会把"读不出"当成"没有设置"然后用默认值覆盖用户文件）。所以更新一个
//!    字段时，用户不认识的键必须活着；用例往文件里塞一个"第三方键"再更新，断言它还在。
//! 2. **缺文件与空对象等价，坏文件不是**——前两者都返 `{}`；坏文件必须报错而**不是**
//!    被这次更新覆盖（覆盖＝把用户的设置换成一行 JSON）。
//! 3. **表与注册点一致**——`FIELD_UPDATES` 的每一行都必须有一个同名的显式注册点
//!    （注册点写成字面量才扫得到；"加了一行表却忘了注册"会变成静默缺口）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::clock::SystemClock;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_host::dispatcher::HostServices;
use aeroric_host::settings_services::field_update_methods;
use aeroric_host::startup::{run_startup, serve, HostOptions, RunningHost};
use aeroric_protocol::envelope::{decode_line, encode_line, Frame, Request};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
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
            "ahs9-{tag}-{}-{}",
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
                application: "aeroric-host-settings-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
            "build-hash-test",
            "protocol-hash-test",
        )
        .await
        .expect("startup");
        let token = std::sync::Arc::clone(&started.token);
        let context = std::sync::Arc::clone(&started.context);
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

    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    fn write_settings(&self, value: serde_json::Value) {
        std::fs::write(
            self.settings_path(),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .expect("write settings");
    }

    fn read_settings(&self) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(self.settings_path()).expect("read"))
            .expect("json")
    }

    async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let stream = UnixStream::connect(&self.socket).await.expect("connect");
        let (read, write) = stream.into_split();
        (BufReader::new(read), write)
    }

    async fn handshake(&self, reader: &mut BufReader<OwnedReadHalf>, writer: &mut OwnedWriteHalf) {
        let token = std::fs::read_to_string(self.data_dir.join("host.token"))
            .expect("token")
            .trim()
            .to_string();
        let reply = call(
            reader,
            writer,
            "h",
            "handshake",
            json!({ "token": token, "clientId": "test" }),
        )
        .await;
        assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
    }

    fn cleanup(&mut self) {
        if let Some(running) = self.running.take() {
            running.shutdown();
        }
        std::fs::remove_dir_all(&self.root).ok();
    }
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    method: &str,
    params: serde_json::Value,
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
    .expect("a reply within the deadline")
    .expect("read");
    decode_line(reply.trim_end().as_bytes()).expect("decode")
}

fn result(frame: &Frame) -> &serde_json::Value {
    match frame {
        Frame::Success(success) => &success.result,
        other => panic!("expected success, got {other:?}"),
    }
}

#[tokio::test]
async fn loading_settings_distinguishes_absent_from_corrupt() {
    let mut rig = Rig::start("load").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;

    // 缺文件 ⇒ 空对象（不是错误）。
    let absent = call(
        &mut reader,
        &mut writer,
        "1",
        "settings.load_app_settings",
        json!({}),
    )
    .await;
    assert_eq!(result(&absent), &json!({}));

    // 坏文件 ⇒ 报错，**不是**空对象（空对象会被下一次保存写成"用户没有设置"）。
    std::fs::write(rig.settings_path(), b"{ this is not json").expect("corrupt");
    let corrupt = call(
        &mut reader,
        &mut writer,
        "2",
        "settings.load_app_settings",
        json!({}),
    )
    .await;
    assert!(matches!(corrupt, Frame::Failure(_)), "{corrupt:?}");
    rig.cleanup();
}

#[tokio::test]
async fn a_field_update_keeps_every_other_key() {
    let mut rig = Rig::start("update").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    rig.write_settings(json!({
        "send_shortcut": "enter",
        "third_party_key": { "kept": true },
        "custom_agents": [{ "id": "fake" }]
    }));

    let updated = call(
        &mut reader,
        &mut writer,
        "1",
        "settings.update_proxy_settings",
        json!({ "proxySettings": { "url": "http://127.0.0.1:7890" } }),
    )
    .await;
    let body = result(&updated);
    assert_eq!(
        body["proxy_settings"]["url"],
        json!("http://127.0.0.1:7890")
    );

    // 磁盘上的其余键一个都不能少——这是"整份 map"设计的存在理由。
    let on_disk = rig.read_settings();
    assert_eq!(on_disk["third_party_key"], json!({ "kept": true }));
    assert_eq!(on_disk["custom_agents"][0]["id"], json!("fake"));
    assert_eq!(on_disk["send_shortcut"], json!("enter"));

    // 缺参数 ⇒ 明确 InvalidArgument，且**磁盘不动**。
    let before = rig.read_settings();
    let missing = call(
        &mut reader,
        &mut writer,
        "2",
        "settings.update_proxy_settings",
        json!({}),
    )
    .await;
    assert!(matches!(missing, Frame::Failure(_)), "{missing:?}");
    assert_eq!(rig.read_settings(), before);
    rig.cleanup();
}

#[tokio::test]
async fn a_corrupt_file_is_never_overwritten_by_an_update() {
    let mut rig = Rig::start("corrupt").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    std::fs::write(rig.settings_path(), b"{ broken").expect("corrupt");
    let bytes_before = std::fs::read(rig.settings_path()).unwrap();

    let reply = call(
        &mut reader,
        &mut writer,
        "1",
        "settings.update_auto_cleanup_settings",
        json!({ "autoCleanupSettings": { "enabled": true } }),
    )
    .await;
    assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
    assert_eq!(
        std::fs::read(rig.settings_path()).unwrap(),
        bytes_before,
        "坏文件必须原样留着，等用户自己去修"
    );
    rig.cleanup();
}

#[tokio::test]
async fn every_field_update_row_has_a_registered_method() {
    let mut rig = Rig::start("table").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    rig.write_settings(json!({}));

    for (index, row) in field_update_methods().iter().enumerate() {
        // 缺参数时若"方法不存在"，错误会是 NotFound/InvalidArgument 之外的形状；
        // 这里用 "方法在集合里" 这条更强的前置：给一个空对象，只看它**不是**方法缺失。
        let reply = call(
            &mut reader,
            &mut writer,
            &format!("t{index}"),
            row.method,
            json!({ row.param: null }),
        )
        .await;
        match reply {
            Frame::Failure(failure) => assert!(
                failure.error.message.contains("Missing param")
                    || failure.error.message.contains("Invalid")
                    || failure.error.message.contains("object"),
                "{} 已注册，但拒绝方式不是参数问题：{:?}",
                row.method,
                failure.error
            ),
            Frame::Success(_) => {
                // `null` 也是合法值（例如把某个设置清空），此时必须真写进去了。
                assert!(rig.read_settings().get(row.key).is_some(), "{}", row.method);
            }
            other => panic!("{}: unexpected frame {other:?}", row.method),
        }
    }
    rig.cleanup();
}
