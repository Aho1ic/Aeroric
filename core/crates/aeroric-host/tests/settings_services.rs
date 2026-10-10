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
use aeroric_protocol::error::ErrorCode;
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

impl Drop for Rig {
    fn drop(&mut self) {
        self.cleanup();
    }
}

async fn request(rig: &Rig, method: &str, params: serde_json::Value) -> Frame {
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    call(&mut reader, &mut writer, "preference", method, params).await
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

#[tokio::test]
async fn preference_newline_accepts_the_real_wire_and_rejects_ambiguous_or_wrong_types() {
    let rig = Rig::start("newline").await;
    rig.write_settings(json!({"unknown": {"keep": true}}));
    for params in [
        json!({"enabled": false}),
        json!({"enabled": true}),
        json!({"shiftEnterNewline": false}),
        json!({"enabled": true, "shiftEnterNewline": true}),
    ] {
        let expected = params
            .get("enabled")
            .or_else(|| params.get("shiftEnterNewline"))
            .unwrap();
        let reply = request(&rig, "settings.save_shift_enter_newline", params.clone()).await;
        assert_eq!(&result(&reply)["terminal_shift_enter_newline"], expected);
        assert_eq!(rig.read_settings()["unknown"], json!({"keep": true}));
    }
    let before = std::fs::read(rig.settings_path()).unwrap();
    for params in [
        json!({}),
        json!({"enabled": null}),
        json!({"enabled": "false"}),
        json!({"shiftEnterNewline": 0}),
        json!({"enabled": true, "shiftEnterNewline": false}),
    ] {
        let reply = request(&rig, "settings.save_shift_enter_newline", params).await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
}

#[tokio::test]
async fn preference_shortcut_normalizes_like_desktop_without_touching_other_settings() {
    let rig = Rig::start("shortcut").await;
    rig.write_settings(json!({"unknown": 42, "claude_path": "/not/executed"}));
    for (input, expected) in [
        ("enter", "enter"),
        ("mod_enter", "mod_enter"),
        ("anything-else", "mod_enter"),
        (" enter ", "mod_enter"),
    ] {
        let reply = request(
            &rig,
            "settings.save_send_shortcut",
            json!({"sendShortcut": input}),
        )
        .await;
        assert_eq!(result(&reply)["send_shortcut"], expected);
        assert_eq!(rig.read_settings()["unknown"], 42);
        assert_eq!(rig.read_settings()["claude_path"], "/not/executed");
    }
    let before = std::fs::read(rig.settings_path()).unwrap();
    for input in [json!(null), json!(true), json!(12), json!({})] {
        let reply = request(
            &rig,
            "settings.save_send_shortcut",
            json!({"sendShortcut": input}),
        )
        .await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
}

#[tokio::test]
async fn preference_schedules_clamp_fields_preserve_timestamps_and_do_not_create_output_dirs() {
    let rig = Rig::start("schedule").await;
    let out = rig.root.join("not-created/report-output");
    let reply = request(
        &rig,
        "settings.update_auto_cleanup_settings",
        json!({
            "autoCleanupSettings": {
                "enabled": true, "mode": "unknown", "weekday": 99, "hour": 99,
                "interval_days": 0, "retain_days": 65535, "last_run_at": 123456,
            }
        }),
    )
    .await;
    assert_eq!(
        result(&reply)["auto_cleanup_settings"],
        json!({
            "enabled": true, "mode": "weekly", "weekday": 6, "hour": 23,
            "interval_days": 1, "retain_days": 3650, "last_run_at": 123456,
        })
    );
    let reply = request(
        &rig,
        "settings.update_weekly_report_settings",
        json!({
            "weeklyReportSettings": {"week_start_day": 200, "week_end_day": 199, "output_dir": out},
        }),
    )
    .await;
    assert_eq!(
        result(&reply)["weekly_report_settings"],
        json!({
            "week_start_day": 6, "week_end_day": 6, "output_dir": out,
        })
    );
    assert!(
        !out.exists(),
        "saving a preference must not generate a report or mkdir"
    );
    let before = std::fs::read(rig.settings_path()).unwrap();
    for (method, params) in [
        (
            "settings.update_auto_cleanup_settings",
            json!({"autoCleanupSettings": null}),
        ),
        (
            "settings.update_auto_cleanup_settings",
            json!({"autoCleanupSettings": {"enabled": "true"}}),
        ),
        (
            "settings.update_auto_cleanup_settings",
            json!({"autoCleanupSettings": {"weekday": 256}}),
        ),
        (
            "settings.update_weekly_report_settings",
            json!({"weeklyReportSettings": {"output_dir": 42}}),
        ),
        (
            "settings.update_weekly_report_settings",
            json!({"weeklyReportSettings": {"week_start_day": -1}}),
        ),
    ] {
        let reply = request(&rig, method, params).await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
}

#[tokio::test]
async fn preference_updates_refuse_corrupt_or_unreturnable_settings_without_repair_side_effects() {
    let rig = Rig::start("pref-corrupt").await;
    for bytes in [
        b"{ broken".as_slice(),
        br#"{"custom_agents":"not-an-array","unknown":12}"#,
    ] {
        std::fs::write(rig.settings_path(), bytes).unwrap();
        let reply = request(
            &rig,
            "settings.save_send_shortcut",
            json!({"sendShortcut": "enter"}),
        )
        .await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), bytes);
    }
    assert!(!rig.data_dir.join("agent-homes").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preference_updates_share_one_transaction_across_clients() {
    let rig = Arc::new(Rig::start("pref-concurrent").await);
    for _ in 0..8 {
        rig.write_settings(json!({"unknown": "x".repeat(16384)}));
        let start = Arc::new(tokio::sync::Barrier::new(7));
        let mut calls = Vec::new();
        for (method, params) in [
            (
                "settings.save_send_shortcut",
                json!({"sendShortcut": "enter"}),
            ),
            (
                "settings.save_shift_enter_newline",
                json!({"enabled": false}),
            ),
            (
                "settings.update_auto_cleanup_settings",
                json!({"autoCleanupSettings": {"last_run_at": 123}}),
            ),
            (
                "settings.update_weekly_report_settings",
                json!({"weeklyReportSettings": {"week_start_day": 2}}),
            ),
            (
                "settings.update_notebook_embedding_settings",
                json!({"notebookEmbeddingSettings": {"model": "synthetic"}}),
            ),
            (
                "settings.update_proxy_settings",
                json!({"proxySettings": {"url": "http://synthetic.invalid"}}),
            ),
            (
                "settings.update_dsh_reasoning_effort",
                json!({"agent": "dsh", "effort": "max"}),
            ),
        ] {
            let rig = Arc::clone(&rig);
            let start = Arc::clone(&start);
            calls.push(tokio::spawn(async move {
                start.wait().await;
                let reply = request(&rig, method, params).await;
                assert!(matches!(reply, Frame::Success(_)), "{method}: {reply:?}");
            }));
        }
        for call in calls {
            call.await.unwrap();
        }
        let fields = rig.read_settings();
        assert_eq!(fields["unknown"].as_str().unwrap().len(), 16384);
        assert_eq!(fields["send_shortcut"], "enter");
        assert_eq!(fields["terminal_shift_enter_newline"], false);
        assert_eq!(fields["auto_cleanup_settings"]["last_run_at"], 123);
        assert_eq!(fields["weekly_report_settings"]["week_start_day"], 2);
        assert_eq!(fields["notebook_embedding_settings"]["model"], "synthetic");
        assert_eq!(fields["proxy_settings"]["url"], "http://synthetic.invalid");
        assert_eq!(fields["dsh_reasoning_efforts"]["dsh"], "max");
    }
}

#[tokio::test]
async fn preference_embedding_normalizes_without_persisting_inline_keys_or_contacting_a_provider() {
    let rig = Rig::start("embedding").await;
    rig.write_settings(json!({"unknown": {"keep": true}}));
    let reply = request(
        &rig,
        "settings.update_notebook_embedding_settings",
        json!({
            "notebookEmbeddingSettings": {
                "provider": "openAi", "base_url": " \t ", "model": "  ",
                "api_key": "synthetic-inline-key", "apiKey": "synthetic-camel-key",
            },
        }),
    )
    .await;
    assert_eq!(
        result(&reply)["notebook_embedding_settings"],
        json!({
            "provider": "openAi", "base_url": "http://127.0.0.1:11434", "model": "nomic-embed-text",
        })
    );
    assert_eq!(rig.read_settings()["unknown"], json!({"keep": true}));
    let persisted = std::fs::read_to_string(rig.settings_path()).unwrap();
    assert!(!persisted.contains("synthetic-inline-key"));
    assert!(!persisted.contains("synthetic-camel-key"));
    let reply = request(&rig, "settings.update_notebook_embedding_settings", json!({
        "notebookEmbeddingSettings": {
            "provider": "ollama", "base_url": " https://never-contact.invalid/v1/ ", "model": " synthetic ",
        },
    })).await;
    assert_eq!(
        result(&reply)["notebook_embedding_settings"],
        json!({
            "provider": "ollama", "base_url": "https://never-contact.invalid/v1/", "model": "synthetic",
        })
    );
    let before = std::fs::read(rig.settings_path()).unwrap();
    for payload in [
        json!(null),
        json!({"provider": "not-a-provider"}),
        json!({"base_url": false}),
        json!({"model": 42}),
    ] {
        let reply = request(
            &rig,
            "settings.update_notebook_embedding_settings",
            json!({
                "notebookEmbeddingSettings": payload,
            }),
        )
        .await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
    assert!(!rig.data_dir.join("agent-homes").exists());
}

#[tokio::test]
async fn preference_dsh_refuses_a_non_object_effort_map_before_writing() {
    let rig = Rig::start("dsh-corrupt-map").await;
    for effort_map in [json!(false), json!(null), json!(["high"])] {
        rig.write_settings(json!({
            "unknown": {"keep": true}, "dsh_reasoning_efforts": effort_map,
        }));
        let before = std::fs::read(rig.settings_path()).unwrap();
        let reply = request(
            &rig,
            "settings.update_dsh_reasoning_effort",
            json!({"agent": "dsh", "effort": "high"}),
        )
        .await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
    assert!(!rig.data_dir.join("agent-homes").exists());
}

#[tokio::test]
async fn preference_dsh_refuses_an_unreturnable_reply_before_writing() {
    let rig = Rig::start("dsh-corrupt-reply").await;
    rig.write_settings(json!({
        "custom_agents": false, "dsh_reasoning_efforts": {"other": "low"},
    }));
    let before = std::fs::read(rig.settings_path()).unwrap();
    let reply = request(
        &rig,
        "settings.update_dsh_reasoning_effort",
        json!({"agent": "dsh", "effort": " MAX "}),
    )
    .await;
    assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
    assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    assert!(!rig.data_dir.join("agent-homes").exists());
}

#[tokio::test]
async fn preference_proxy_normalizes_complete_replacements_and_refuses_wrong_types() {
    let rig = Rig::start("proxy-normalize").await;
    rig.write_settings(json!({"unknown": {"keep": true}}));
    let reply = request(
        &rig,
        "settings.update_proxy_settings",
        json!({
            "proxySettings": {
                "url": " 127.0.0.1:7890 ", "no_proxy": " localhost, , example.invalid ,localhost ",
                "username": " synthetic-user ", "password": " synthetic-pass ",
            },
        }),
    )
    .await;
    assert_eq!(
        result(&reply)["proxy_settings"],
        json!({
            "url": "http://127.0.0.1:7890", "no_proxy": "localhost,example.invalid,localhost",
            "username": "synthetic-user", "password": "synthetic-pass",
        })
    );
    assert_eq!(rig.read_settings()["unknown"], json!({"keep": true}));
    let before = std::fs::read(rig.settings_path()).unwrap();
    for payload in [
        json!(null),
        json!(false),
        json!("not-an-object"),
        json!({"url": false}),
        json!({"no_proxy": []}),
        json!({"username": 42}),
        json!({"password": null}),
    ] {
        let reply = request(
            &rig,
            "settings.update_proxy_settings",
            json!({
                "proxySettings": payload,
            }),
        )
        .await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
    // Valid complete replacement can repair the exact target, not unrelated data.
    rig.write_settings(json!({"proxy_settings": null, "unknown": true}));
    let reply = request(
        &rig,
        "settings.update_proxy_settings",
        json!({"proxySettings": {}}),
    )
    .await;
    assert_eq!(
        result(&reply)["proxy_settings"],
        json!({
            "url": "", "no_proxy": "", "username": "", "password": "",
        })
    );
    assert_eq!(result(&reply)["unknown"], true);
    assert!(!rig.data_dir.join("agent-homes").exists());
}

#[tokio::test]
async fn preference_proxy_consumes_legacy_state_before_replacement_and_never_revives_it() {
    let rig = Rig::start("proxy-legacy").await;
    let profiles = json!([
        {"id": "first", "label": "First", "path": "/never/run-first",
            "username": "synthetic-old-user", "password": "synthetic-old-pass",
            "future": {"username": "not-a-legacy-field"}},
        {"id": "second", "label": "Second", "path": "/never/run-second",
            "username": "synthetic-second-user", "password": "synthetic-second-pass",
            "future": [1, 2, 3]},
    ]);
    let original = json!({
        "unknown": {"keep": true}, "custom_agents": profiles,
        "proxy_settings": {"url": "http://old-global.invalid", "username": "synthetic-global"},
        "agent_proxy_enabled": {"claude": false, " OMP ": true, "": true, "codex": false},
        "agent_proxy_overrides": {
            " CLAUDE ": {"enabled": true, "url": "http://old-legacy.invalid"},
            "dsh": {"enabled": false, "url": "http://unused.invalid"},
        },
    });
    rig.write_settings(original.clone());
    let reply = request(
        &rig,
        "settings.update_proxy_settings",
        json!({
            "proxySettings": {"url": "http://new.invalid:7890"},
        }),
    )
    .await;
    assert_eq!(
        result(&reply)["agent_proxy_enabled"],
        json!({"claude": true, "omp": true})
    );
    assert!(result(&reply).get("agent_proxy_overrides").is_none());
    let mut expected_profiles = profiles;
    for profile in expected_profiles.as_array_mut().unwrap() {
        let fields = profile.as_object_mut().unwrap();
        fields.remove("username");
        fields.remove("password");
    }
    assert_eq!(result(&reply)["custom_agents"], expected_profiles);
    assert_eq!(
        result(&reply)["proxy_settings"],
        json!({
            "url": "http://new.invalid:7890", "no_proxy": "", "username": "", "password": "",
        })
    );
    let reply = request(
        &rig,
        "settings.update_proxy_settings",
        json!({"proxySettings": {}}),
    )
    .await;
    assert_eq!(
        result(&reply)["proxy_settings"],
        json!({
            "url": "", "no_proxy": "", "username": "", "password": "",
        })
    );
    assert_eq!(result(&reply)["custom_agents"], expected_profiles);
    assert_eq!(result(&reply)["unknown"], json!({"keep": true}));
    // An initial empty replacement must consume legacy fields without
    // reviving them, not just clear a previously migrated configuration.
    rig.write_settings(original);
    let reply = request(
        &rig,
        "settings.update_proxy_settings",
        json!({"proxySettings": {}}),
    )
    .await;
    assert_eq!(
        result(&reply)["proxy_settings"],
        json!({"url": "", "no_proxy": "", "username": "", "password": ""})
    );
    assert_eq!(result(&reply)["custom_agents"], expected_profiles);
    assert_eq!(
        result(&reply)["agent_proxy_enabled"],
        json!({"claude": true, "omp": true})
    );
    assert!(result(&reply).get("agent_proxy_overrides").is_none());
    assert_eq!(result(&reply)["unknown"], json!({"keep": true}));
    assert_eq!(&rig.read_settings(), result(&reply));
    assert!(!rig.data_dir.join("agent-homes").exists());
}

#[tokio::test]
async fn preference_proxy_refuses_corrupt_migration_inputs_or_unreturnable_settings_before_writing()
{
    let rig = Rig::start("proxy-corrupt").await;
    for original in [
        json!({"custom_agents": false}),
        json!({"agent_proxy_overrides": false}),
        json!({"agent_proxy_enabled": {"dsh": "true"}}),
        json!({"custom_agents": [["positional", "Synthetic", "/never/run"]]}),
    ] {
        rig.write_settings(original);
        let before = std::fs::read(rig.settings_path()).unwrap();
        let reply = request(
            &rig,
            "settings.update_proxy_settings",
            json!({
                "proxySettings": {"url": "http://never-contact.invalid"},
            }),
        )
        .await;
        assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
    assert!(!rig.data_dir.join("agent-homes").exists());
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
        "custom_agents": [{ "id": "fake", "label": "Synthetic", "path": "/never/run" }]
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
    let before = std::fs::read(rig.settings_path()).unwrap();

    for (index, row) in field_update_methods().iter().enumerate() {
        // Every listed field now has a typed preference. Null must be rejected
        // as an argument error, not accepted or mistaken for a missing method.
        let reply = call(
            &mut reader,
            &mut writer,
            &format!("t{index}"),
            row.method,
            json!({ row.param: null }),
        )
        .await;
        match reply {
            Frame::Failure(failure) => assert_eq!(
                failure.error.code,
                ErrorCode::InvalidArgument,
                "{}",
                row.method,
            ),
            other => panic!("{}: unexpected frame {other:?}", row.method),
        }
        assert_eq!(std::fs::read(rig.settings_path()).unwrap(), before);
    }
    rig.cleanup();
}

#[tokio::test]
async fn dsh_reasoning_effort_is_validated_and_keeps_other_keys() {
    let mut rig = Rig::start("dsh-effort").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    rig.write_settings(json!({
        "third_party_key": { "kept": true },
        "dsh_reasoning_efforts": { "other": "low" }
    }));

    let updated = call(
        &mut reader,
        &mut writer,
        "1",
        "settings.update_dsh_reasoning_effort",
        json!({ "agent": "dsh", "effort": " MAX " }),
    )
    .await;
    let body = result(&updated);
    assert_eq!(body["dsh_reasoning_efforts"]["dsh"], json!("max"));
    let on_disk = rig.read_settings();
    assert_eq!(on_disk["third_party_key"], json!({ "kept": true }));
    assert_eq!(on_disk["dsh_reasoning_efforts"]["other"], json!("low"));

    let before_invalid = rig.read_settings();
    let invalid = call(
        &mut reader,
        &mut writer,
        "2",
        "settings.update_dsh_reasoning_effort",
        json!({ "agent": "dsh", "effort": "ultra" }),
    )
    .await;
    assert!(matches!(invalid, Frame::Failure(_)), "{invalid:?}");
    assert_eq!(rig.read_settings(), before_invalid);

    let before_non_dsh = rig.read_settings();
    let non_dsh = call(
        &mut reader,
        &mut writer,
        "3",
        "settings.update_dsh_reasoning_effort",
        json!({ "agent": "codex", "effort": "high" }),
    )
    .await;
    assert!(matches!(non_dsh, Frame::Failure(_)), "{non_dsh:?}");
    assert_eq!(rig.read_settings(), before_non_dsh);
    rig.cleanup();
}

#[tokio::test]
async fn agent_config_file_paths_read_and_write_through_configured_path() {
    let mut rig = Rig::start("agent-config").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;

    let config_path = rig.root.join("agent-config.json");
    rig.write_settings(json!({
        "claude_config_path": config_path.to_string_lossy(),
    }));

    let path_reply = call(
        &mut reader,
        &mut writer,
        "1",
        "settings.get_agent_config_file_path",
        json!({ "agent": "claude" }),
    )
    .await;
    assert_eq!(result(&path_reply), &json!(config_path.to_string_lossy()));

    let missing = call(
        &mut reader,
        &mut writer,
        "2",
        "settings.read_agent_config_file",
        json!({ "agent": "claude" }),
    )
    .await;
    assert_eq!(result(&missing), &json!(null));

    let write = call(
        &mut reader,
        &mut writer,
        "3",
        "settings.write_agent_config_file",
        json!({ "agent": "claude", "content": "{\"ok\":true}" }),
    )
    .await;
    assert!(matches!(write, Frame::Success(_)), "{write:?}");
    assert_eq!(
        std::fs::read_to_string(&config_path).unwrap(),
        "{\"ok\":true}"
    );

    let read = call(
        &mut reader,
        &mut writer,
        "4",
        "settings.read_agent_config_file",
        json!({ "agent": "claude" }),
    )
    .await;
    assert_eq!(result(&read), &json!("{\"ok\":true}"));
    rig.cleanup();
}
