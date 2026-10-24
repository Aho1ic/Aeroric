//! `agentConnections.legacyCleanup.plan/apply` over the authenticated UDS.
//!
//! Isolated: temporary data root, MemorySecrets, the production startup entry
//! that runs the legacy migration before the listener is ready. Results are
//! decoded with the strict protocol DTOs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aeroric_core::clock::SystemClock;
use aeroric_core::context::{MemorySecrets, SecretKey, SecretStore};
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_host::dispatcher::HostServices;
use aeroric_host::startup::{
    run_startup_with_secrets_and_legacy_agent_migration, serve, HostOptions, RunningHost,
};
use aeroric_protocol::envelope::{decode_line, encode_line, Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::legacy_cleanup::{
    CandidateState, CleanupReport, APPLY_METHOD, CAPABILITY, PLAN_METHOD,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const SIDECAR_SECRET: &str = "sk-legacy-sidecar-must-not-leak";

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

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Rig {
    root: PathBuf,
    data_dir: PathBuf,
    socket: PathBuf,
    secrets: Arc<MemorySecrets>,
    running: Option<RunningHost>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.shutdown();
        }
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// Two migrated custom Claude Agents, `ds` and `other`, each with a generated
/// wrapper, a credential sidecar and an old keychain entry.
async fn rig(tag: &str) -> Rig {
    let root = PathBuf::from("/tmp").join(format!(
        "ahlc-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let paths = AppPaths::from_user_home(root.join("home"));
    let data_dir = paths.data_dir().to_path_buf();
    std::fs::create_dir_all(data_dir.join("agent-scripts")).unwrap();
    std::fs::create_dir_all(data_dir.join("agent-credentials")).unwrap();
    let secrets = Arc::new(MemorySecrets::default());
    let mut profiles = Vec::new();
    for id in ["ds", "other"] {
        let wrapper = data_dir.join(format!("agent-scripts/{id}.sh"));
        std::fs::write(
            &wrapper,
            "#!/bin/sh\nexport CLAUDE_CONFIG_DIR=x\nCLAUDE_CODE_SESSION_ENV_DIR=y\n",
        )
        .unwrap();
        std::fs::write(
            data_dir.join(format!("agent-credentials/{id}")),
            SIDECAR_SECRET,
        )
        .unwrap();
        secrets
            .set(
                &SecretKey(format!("agent-config:custom:{id}:api_key")),
                SIDECAR_SECRET,
            )
            .unwrap();
        profiles.push(json!({
            "id": id, "label": format!("Agent {id}"), "path": wrapper, "codex_like": false,
            "family": "claude", "config_lang": "shellscript",
            "base_url": format!("https://{id}.invalid/v1"), "api_key": "", "models": ["m-1"]
        }));
    }
    std::fs::write(
        paths.settings_file(),
        serde_json::to_vec_pretty(&json!({"custom_agents": profiles})).unwrap(),
    )
    .unwrap();

    let socket = data_dir.join("host.sock");
    let started = run_startup_with_secrets_and_legacy_agent_migration(
        &HostOptions {
            paths,
            socket_path: socket.clone(),
            ready_fd: None,
        },
        Arc::new(SystemClock),
        &LeaseOwner {
            application: "aeroric-host-legacy-cleanup-test".into(),
            version: "0-test".into(),
        },
        &LocalProbe,
        "build-hash-test",
        "protocol-hash-test",
        Arc::clone(&secrets) as Arc<dyn SecretStore>,
    )
    .await
    .expect("startup with migration");
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
    Rig {
        root,
        data_dir,
        socket,
        secrets,
        running: Some(running),
    }
}

impl Rig {
    async fn session(
        &self,
    ) -> (
        BufReader<tokio::net::unix::OwnedReadHalf>,
        tokio::net::unix::OwnedWriteHalf,
        Value,
    ) {
        let stream = UnixStream::connect(&self.socket).await.expect("connect");
        let (read, mut writer) = stream.into_split();
        let mut reader = BufReader::new(read);
        let token = std::fs::read_to_string(self.data_dir.join("host.token"))
            .unwrap()
            .trim()
            .to_string();
        let hello = match send(
            &mut reader,
            &mut writer,
            "h",
            "handshake",
            json!({"token": token, "clientId": "test"}),
        )
        .await
        {
            Frame::Success(success) => success.result,
            other => panic!("handshake: {other:?}"),
        };
        (reader, writer, hello)
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let (mut reader, mut writer, _) = self.session().await;
        match send(&mut reader, &mut writer, "c", method, params).await {
            Frame::Success(success) => Ok(success.result),
            Frame::Failure(failure) => Err(serde_json::to_value(failure.error).unwrap()),
            other => panic!("unexpected frame {other:?}"),
        }
    }

    async fn report(&self, method: &str, params: Value) -> (CleanupReport, String) {
        let raw = self.call(method, params).await.expect("success");
        let text = raw.to_string();
        let report: CleanupReport = serde_json::from_value(raw).expect("strict DTO");
        (report, text)
    }

    /// Every file under the data root except the live socket/token/lease.
    fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("host.") || name.contains("lock") || name.contains("lease") {
                    continue;
                }
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.insert(path.clone(), std::fs::read(&path).unwrap_or_default());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.data_dir, &mut out);
        out
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

fn state<'a>(report: &'a CleanupReport, source: &str) -> &'a CandidateState {
    &report
        .candidates
        .iter()
        .find(|candidate| candidate.source == source)
        .unwrap_or_else(|| panic!("{source} missing: {report:?}"))
        .state
}

fn code(error: &Value) -> ErrorCode {
    ErrorCode::from_wire(error["code"].as_str().unwrap_or_default())
}

#[tokio::test]
async fn capability_is_advertised_and_plan_is_read_only() {
    let rig = rig("plan").await;
    let (_, _, hello) = rig.session().await;
    assert!(
        hello["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cap| cap["name"] == CAPABILITY && cap["enabled"] == true),
        "{hello}"
    );
    let before = rig.files();
    let secrets_before = rig.secrets.keys();
    let (report, text) = rig.report(PLAN_METHOD, json!({})).await;
    let (again, _) = rig.report(PLAN_METHOD, Value::Null).await;
    assert_eq!(report, again);
    assert_eq!(state(&report, "custom:ds"), &CandidateState::Eligible {});
    assert_eq!(state(&report, "custom:other"), &CandidateState::Eligible {});
    assert_eq!(report.quarantine_batch, None);
    assert_eq!(rig.files(), before, "plan touched the data root");
    assert_eq!(rig.secrets.keys(), secrets_before);
    assert!(!text.contains(SIDECAR_SECRET));
}

#[tokio::test]
async fn apply_cleans_only_the_confirmed_source() {
    let rig = rig("apply").await;
    let (report, text) = rig
        .report(
            APPLY_METHOD,
            json!({"schemaVersion": 1, "sourceIds": ["custom:ds"]}),
        )
        .await;
    assert_eq!(state(&report, "custom:ds"), &CandidateState::Done {});
    assert_eq!(state(&report, "custom:other"), &CandidateState::Eligible {});
    let batch = report.quarantine_batch.clone().expect("a new batch");
    assert!(!batch.contains('/') && !batch.is_empty(), "{batch}");
    assert!(rig
        .data_dir
        .join("legacy-agent-quarantine")
        .join(&batch)
        .is_dir());
    for leaked in [SIDECAR_SECRET, rig.data_dir.to_str().unwrap(), "/tmp/"] {
        assert!(!text.contains(leaked), "reply leaked {leaked}: {text}");
    }

    assert!(!rig.data_dir.join("agent-scripts/ds.sh").exists());
    assert!(!rig.data_dir.join("agent-credentials/ds").exists());
    assert!(rig.data_dir.join("agent-scripts/other.sh").exists());
    assert!(rig.data_dir.join("agent-credentials/other").exists());
    assert!(rig.secrets.peek("agent-config:custom:ds:api_key").is_none());
    assert_eq!(
        rig.secrets
            .peek("agent-config:custom:other:api_key")
            .as_deref(),
        Some(SIDECAR_SECRET)
    );
    let settings: Value =
        serde_json::from_slice(&std::fs::read(rig.data_dir.join("settings.json")).unwrap())
            .unwrap();
    let ids: Vec<_> = settings["custom_agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|profile| profile["id"].clone())
        .collect();
    assert_eq!(ids, [json!("other")]);

    // Idempotent: the same confirmation again changes nothing.
    let after = rig.files();
    let (second, _) = rig
        .report(
            APPLY_METHOD,
            json!({"schemaVersion": 1, "sourceIds": ["custom:ds"]}),
        )
        .await;
    assert_eq!(state(&second, "custom:ds"), &CandidateState::Done {});
    assert_eq!(second.quarantine_batch, None);
    assert_eq!(rig.files(), after);
}

#[tokio::test]
async fn malformed_requests_are_refused_without_side_effects() {
    let rig = rig("refuse").await;
    let before = rig.files();
    for (method, params) in [
        (APPLY_METHOD, json!({"schemaVersion": 1, "sourceIds": []})),
        (
            APPLY_METHOD,
            json!({"schemaVersion": 1, "sourceIds": ["builtin:claude"]}),
        ),
        (
            APPLY_METHOD,
            json!({"schemaVersion": 1, "sourceIds": ["custom:"]}),
        ),
        (
            APPLY_METHOD,
            json!({"schemaVersion": 2, "sourceIds": ["custom:ds"]}),
        ),
        (
            APPLY_METHOD,
            json!({"schemaVersion": 1, "sourceIds": ["custom:ds"], "force": true}),
        ),
        (APPLY_METHOD, json!({"sourceIds": ["custom:ds"]})),
        (APPLY_METHOD, Value::Null),
        (PLAN_METHOD, json!({"sourceIds": ["custom:ds"]})),
    ] {
        let error = rig.call(method, params.clone()).await.expect_err("refused");
        assert_eq!(
            code(&error),
            ErrorCode::InvalidArgument,
            "{method} {params}: {error}"
        );
    }
    assert_eq!(rig.files(), before);
}

#[tokio::test]
async fn a_source_whose_configuration_is_gone_stays_blocked() {
    let rig = rig("blocked").await;
    let path = rig.data_dir.join("agent-connections.json");
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let configs = document["configs"].as_array_mut().unwrap();
    let before_len = configs.len();
    configs.retain(|config| config["label"] != "Agent ds");
    assert_eq!(configs.len() + 1, before_len, "fixture has the ds config");
    std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

    let (plan, _) = rig.report(PLAN_METHOD, json!({})).await;
    assert!(
        matches!(state(&plan, "custom:ds"), CandidateState::Blocked { .. }),
        "{plan:?}"
    );
    let before = rig.files();
    let (applied, _) = rig
        .report(
            APPLY_METHOD,
            json!({"schemaVersion": 1, "sourceIds": ["custom:ds"]}),
        )
        .await;
    assert!(matches!(
        state(&applied, "custom:ds"),
        CandidateState::Blocked { .. }
    ));
    assert_eq!(applied.quarantine_batch, None);
    assert_eq!(rig.files(), before);
}
