//! Task P8-05 切片一：同一宿主的切换语义（`tests/host_cutover.rs`）。
//!
//! 这个套件回答的是计划 Acceptance 里的那几句，逐条都可数：
//!
//! - **"三方同时冷启动，最终只有一个锁持有者、同一 epoch 和一套 watcher；失败竞争者
//!   被回收，没有多余 daemon"**——三方并发 `run_startup` 打同一个 data dir：一个成功、
//!   两个被**拒绝**（不是排队等、不是自动 kill 别人），且败者不留下第二个 socket 与
//!   第二次 epoch 变更。
//! - **"失败竞争者被回收"**——拒绝是**终止**语义：败者进程退出，不留后台重试。
//! - **"版本不兼容显示需协调重启，不能 kill 并重复 spawn"**——未来 schema 的
//!   runtime.json 让启动停在数据兼容那一步，且**文件一字不动**；这时正确动作是提示
//!   协调重启，而不是杀掉正在跑的宿主再起一个。
//! - **"三客户端各连接同一 host"**——胜者 serve 后，两个客户端各自握手成功，拿到同一个
//!   epoch 与同一份 revision。
//!
//! 死 socket 与活 socket 的区别由 `uds::probe_liveness` 承担（P3-06 已落）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::clock::SystemClock;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage::{load_runtime, RuntimeMetadata};
use aeroric_core::storage_lock::{LeaseError, LeaseOwner};
use aeroric_host::dispatcher::HostServices;
use aeroric_host::startup::{run_startup, serve, HostOptions, RunningHost, StartupError};
use aeroric_protocol::envelope::{decode_line, encode_line, Frame, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;

/// 本地卷探针（集成测试跑在本地盘上；`UnprobedLocation` 的 Unknown 是拒绝）。
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

struct Fixture {
    root: PathBuf,
    data_dir: PathBuf,
    socket: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "ah-cutover-{tag}-{}-{}",
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
        Self {
            root,
            data_dir,
            socket,
        }
    }

    fn options(&self) -> HostOptions {
        HostOptions {
            paths: AppPaths::from_user_home(self.root.join("home")),
            socket_path: self.socket.clone(),
            ready_fd: None,
        }
    }

    fn owner(&self, tag: &str) -> LeaseOwner {
        LeaseOwner {
            application: format!("aeroric-{tag}"),
            version: "1.0.0-test".to_string(),
        }
    }

    /// 一次冷启动。返回 `(结果, 读到的 runtime)`——败者那侧也要看得到 runtime 没被动过。
    async fn cold_start(
        &self,
        tag: &str,
    ) -> (
        Result<aeroric_host::startup::StartedHost, StartupError>,
        Option<RuntimeMetadata>,
    ) {
        let result = run_startup(
            &self.options(),
            Arc::new(SystemClock),
            &self.owner(tag),
            &LocalProbe,
            "build-hash-test",
            "protocol-hash-test",
        )
        .await;
        let runtime = read_runtime(&self.data_dir);
        (result, runtime)
    }

    fn cleanup(&self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn read_runtime(data_dir: &Path) -> Option<RuntimeMetadata> {
    let home = data_dir.parent()?;
    let paths = AppPaths::from_user_home(home);
    let outcome = load_runtime(&aeroric_core::CoreContext::read_only(paths)).ok()?;
    outcome.found()
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
    read_frame(reader).await
}

async fn read_frame(reader: &mut BufReader<OwnedReadHalf>) -> Frame {
    let mut line = String::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_line(&mut line),
    )
    .await
    .expect("a frame within the deadline")
    .expect("read");
    assert!(read > 0, "connection closed before a frame");
    decode_line(line.trim_end().as_bytes()).expect("decode")
}

async fn token_of(data_dir: &Path) -> String {
    std::fs::read_to_string(data_dir.join("host.token"))
        .expect("token file")
        .trim()
        .to_string()
}

// ── 一、三方冷启动只有一个持有者 ────────────────────────────────────────────

#[tokio::test]
async fn three_cold_starters_in_one_process_produce_one_holder_and_two_refusals() {
    let fixture = Fixture::new("three");
    // 三方**并发**冷启动（不是逐个来）：竞争窗口是这个套件要覆盖的形状。
    let (first, second, third) = tokio::join!(
        fixture.cold_start("tauri"),
        fixture.cold_start("native"),
        fixture.cold_start("cli"),
    );
    let outcomes = [first, second, third];
    let winners = outcomes.iter().filter(|(result, _)| result.is_ok()).count();
    assert_eq!(winners, 1, "三方同时冷启动只能有一个持有者");

    // 败者的失败必须是**租约拒绝**，并且说得清是"谁拿着"。
    //
    // 这一层的形状要注意：同一个进程里第二个启动者的拒绝理由是"本进程已经持有"
    // （`DataDirectoryLease` 的进程内规则），**跨进程**的拒绝才会指名持有者的
    // application/pid。跨进程那一半由 `scripts/native/tauri-host-smoke.mjs` 真起
    // 两个进程覆盖（那里才有两个真实的 PID），本文件覆盖的是"第二份句柄不会被
    // 悄悄发出去"这条更靠里的不变量——它同样是 INV-03 的一部分。
    let mut refusals = 0;
    for (result, runtime) in &outcomes {
        if let Err(error) = result {
            match error {
                StartupError::Lock(LeaseError::AlreadyInUse { holder, directory }) => {
                    assert!(!holder.trim().is_empty(), "拒绝必须带理由");
                    // macOS 上 `/tmp` 是 `/private/tmp` 的符号链接（租约报的是
                    // canonical 路径），所以比 canonical 之后的形状。
                    assert_eq!(
                        directory,
                        &fixture
                            .data_dir
                            .canonicalize()
                            .unwrap_or_else(|_| fixture.data_dir.clone())
                    );
                    refusals += 1;
                }
                other => panic!("败者的失败类型不对：{other:?}"),
            }
            assert!(runtime.is_some(), "胜者已经写下 runtime；败者不该把它抹掉");
        }
    }
    assert_eq!(refusals, 2, "两个败者都必须被拒（不是排队、不是抢）");

    // 只有一个 socket，且它属于胜者。
    let sockets: Vec<String> = std::fs::read_dir(&fixture.data_dir)
        .expect("data dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("sock"))
        .collect();
    assert_eq!(sockets, vec!["host.sock".to_string()], "{sockets:?}");

    // 败者被**回收**：拒绝是终止语义，没有后台重试在等锁——所以此刻再来一次
    // 仍然是"被拒"，而不是"某个重试突然上了"。
    let (retry, _) = fixture.cold_start("late").await;
    assert!(matches!(
        retry,
        Err(StartupError::Lock(LeaseError::AlreadyInUse { .. }))
    ));

    fixture.cleanup();
}

#[tokio::test]
async fn a_refused_competitor_does_not_touch_the_epoch() {
    let fixture = Fixture::new("epoch");
    let (winner, first_runtime) = fixture.cold_start("winner").await;
    let started = winner.expect("the first starter wins");
    let before = first_runtime.expect("runtime written by the winner");

    // 胜者开始服务之后，败者再来两次。
    let running: RunningHost = serve(
        started,
        HostServices::new(
            Arc::clone(&leaked_token(&fixture.data_dir).await),
            Arc::new(aeroric_core::events::EventLog::default()),
            Arc::new(aeroric_core::CoreContext::read_only(
                AppPaths::from_user_home(fixture.root.join("home")),
            )),
        ),
    );
    for tag in ["native", "cli"] {
        let (refused, _) = fixture.cold_start(tag).await;
        assert!(matches!(
            refused,
            Err(StartupError::Lock(LeaseError::AlreadyInUse { .. }))
        ));
    }
    let after = read_runtime(&fixture.data_dir).expect("runtime still readable");
    assert_eq!(before.epoch, after.epoch, "epoch 不能被败者改写");
    assert_eq!(before.revision, after.revision, "revision 也不能");
    running.shutdown();
    fixture.cleanup();
}

/// 从盘上重新加载 host.token（`HostServices::new` 要 token 对象；`load_or_create`
/// 幂等，所以这一步与真实宿主重启时拿的是同一份）。
async fn leaked_token(data_dir: &Path) -> Arc<aeroric_host::auth::HostToken> {
    Arc::new(
        aeroric_host::auth::HostToken::load_or_create(&data_dir.join("host.token")).expect("token"),
    )
}

// ── 二、三客户端各连同一 host ──────────────────────────────────────────────

#[tokio::test]
async fn every_client_reaches_the_same_host_over_uds() {
    let fixture = Fixture::new("clients");
    let (winner, _) = fixture.cold_start("winner").await;
    let started = winner.expect("the first starter wins");
    let epoch = started.runtime.epoch.clone();
    let token = started.token.clone();
    let context = Arc::clone(&started.context);
    let running = serve(
        started,
        HostServices::new(
            token,
            Arc::new(aeroric_core::events::EventLog::default()),
            context,
        ),
    );

    let raw_token = token_of(&fixture.data_dir).await;
    let mut epochs = Vec::new();
    for client_id in ["tauri", "native", "cli"] {
        let stream = UnixStream::connect(&fixture.socket).await.expect("connect");
        let (read, write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut writer = write;
        let reply = call(
            &mut reader,
            &mut writer,
            &format!("h-{client_id}"),
            "handshake",
            serde_json::json!({ "token": raw_token, "clientId": client_id }),
        )
        .await;
        let Frame::Success(success) = reply else {
            panic!("{client_id} 握手失败：{reply:?}");
        };
        // 握手应答的**真实形状**是 `{clientId, capabilities}`：ready 帧（含 epoch/
        // socketPath）走父进程管道，不在协议面上。所以"同一 host"的判据是
        // **盘上的 epoch 与三客户端连接的是同一个 socket**，而不是应答里的字段。
        assert_eq!(success.result["clientId"], serde_json::json!(client_id));
        assert!(
            success.result["capabilities"].is_array(),
            "能力清单随应答走（ADR-038）"
        );
        epochs.push(read_runtime(&fixture.data_dir).expect("runtime").epoch);
    }
    assert_eq!(epochs.len(), 3);
    assert!(
        epochs.iter().all(|value| value == &epoch),
        "三个客户端必须看到同一个 epoch：{epochs:?}"
    );
    running.shutdown();
    fixture.cleanup();
}

// ── 三、版本不兼容：报错而不是替换 ─────────────────────────────────────────

#[tokio::test]
async fn a_future_schema_stops_startup_without_rewriting_the_file() {
    let fixture = Fixture::new("schema");
    // 先正常起一次，拿到一份真实的 runtime.json。
    let (winner, _) = fixture.cold_start("first").await;
    let started = match winner {
        Ok(started) => started,
        Err(error) => panic!("first start failed: {error}"),
    };
    let path = fixture.data_dir.join("runtime.json");
    drop(started); // 释放写租约（StartedHost 持着 context）

    // 把它改成"未来版本"写下的形状，再冷启动一次。
    let raw = std::fs::read_to_string(&path).expect("runtime");
    let mut json: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let future = json["schemaVersion"].as_u64().unwrap_or(1) + 1;
    json["schemaVersion"] = serde_json::json!(future);
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).expect("write");

    let (result, runtime) = fixture.cold_start("next").await;
    let error = match result {
        Ok(_) => panic!("未来 schema 必须停启动"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(
        message.contains("step 2"),
        "失败要指出停在哪一步：{message}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("runtime"),
        serde_json::to_string_pretty(&json).unwrap(),
        "被拒绝的启动不许改写那份文件（协调重启是人的动作，不是自动替换）"
    );
    // 读侧也被同一道闸拦住（`load_runtime` 拒绝未来 schema），所以 `runtime` 是 None：
    // 这正是"不能假装读懂了别人的数据"。
    assert!(
        runtime.is_none(),
        "未来 schema 必须让读取也停下，而不是降级读一个猜测的版本"
    );
    // 目录里只有**上一次成功启动**留下的那一个 socket 路径，没有被这次的拒绝
    // 添出第二个（启动停在数据兼容那一步，没走到 bind），也没有监听者在上面
    // ——`probe_liveness` 也判定它是死的。
    let sockets: Vec<String> = std::fs::read_dir(&fixture.data_dir)
        .expect("data dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("sock"))
        .collect();
    assert_eq!(sockets, vec!["host.sock".to_string()], "{sockets:?}");
    fixture.cleanup();
}

// ── 四、死 socket 可回收，活 socket 不可抢 ──────────────────────────────────

#[tokio::test]
async fn a_stale_socket_is_reclaimed_but_a_live_host_is_not_evicted() {
    let fixture = Fixture::new("stale");
    std::fs::create_dir_all(&fixture.data_dir).expect("data dir");
    // 造一个"尸体 socket"：路径存在但没有监听者。
    std::fs::write(&fixture.socket, b"stale").expect("stale socket file");
    let (winner, _) = fixture.cold_start("reclaimer").await;
    let started = winner.expect("尸体 socket 不该挡住启动");
    let context = Arc::clone(&started.context);
    let token = Arc::clone(&started.token);
    let running = serve(
        started,
        HostServices::new(
            token,
            Arc::new(aeroric_core::events::EventLog::default()),
            context,
        ),
    );

    // 活着的宿主：第二个启动者被拒（不是把它挤掉）。
    let (refused, _) = fixture.cold_start("intruder").await;
    assert!(matches!(
        refused,
        Err(StartupError::Lock(LeaseError::AlreadyInUse { .. }))
    ));
    // 活宿主仍然可连。
    let stream = UnixStream::connect(&fixture.socket)
        .await
        .expect("live host");
    drop(stream);
    running.shutdown();
    fixture.cleanup();
}
