//! Task P8-05 切片七：文件服务第二批的判据（`files.format_file` / 本地历史三条 /
//! `files.copy_paths_to_directory`）。
//!
//! 这里钉的是**服务端的权威校验**，不是功能的 happy path：
//!
//! 1. **只认 `projectId` ＋ 相对路径**：绝对路径与 `..` 都被拒（否则客户端可以读写项目
//!    外的任意文件，而服务端本该是那个"决定能碰哪些文件"的地方）。
//! 2. **本地历史恢复前有兜底快照**（core 的 `restore_entry` 语义），所以"恢复"本身可撤回。
//! 3. **复制不覆盖**：目标同名时拒绝，而不是静默替换——覆盖不可撤回，而重来很便宜。
//! 4. **格式化走的是 core 的同一实现**（非零退出时原文不变），所以这里只断言"失败时原文
//!    没被动过"这条外部可见性质。
//!
//! 真机 pid 之外的一切都在临时目录里（`/tmp` 短根，macOS 的 `sun_path` 上限不讲情面）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::clock::SystemClock;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_host::dispatcher::HostServices;
use aeroric_host::startup::{run_startup, serve, HostOptions, RunningHost};
use aeroric_protocol::envelope::{decode_line, encode_line, Frame, Request};
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
    project_dir: PathBuf,
    running: Option<RunningHost>,
}

impl Rig {
    async fn start(tag: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "ahf2-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let home = root.join("home");
        let project_dir = root.join("project");
        std::fs::create_dir_all(&project_dir).expect("project");
        let paths = AppPaths::from_user_home(&home);
        let data_dir = paths.data_dir().to_path_buf();
        let socket = data_dir.join("host.sock");

        // 项目记录：宿主解析 `projectId` → 根就靠它。
        std::fs::create_dir_all(&data_dir).expect("data dir");
        let record = serde_json::to_vec_pretty(&serde_json::json!([{
            "id": "p1",
            "name": "batch2",
            "path": project_dir.to_string_lossy(),
        }]))
        .expect("record");
        std::fs::write(data_dir.join("projects.json"), record).expect("projects.json");

        let started = run_startup(
            &HostOptions {
                paths,
                socket_path: socket.clone(),
                ready_fd: None,
            },
            Arc::new(SystemClock),
            &LeaseOwner {
                application: "aeroric-host-batch2-test".to_string(),
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
            project_dir,
            running: Some(running),
        }
    }

    async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let stream = UnixStream::connect(&self.socket).await.expect("connect");
        let (read, write) = stream.into_split();
        (BufReader::new(read), write)
    }

    fn write_file(&self, relative: &str, contents: &str) {
        let path = self.project_dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("file");
    }

    fn read_file(&self, relative: &str) -> String {
        std::fs::read_to_string(self.project_dir.join(relative)).expect("read")
    }

    async fn call(
        &self,
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

    async fn handshake(&self, reader: &mut BufReader<OwnedReadHalf>, writer: &mut OwnedWriteHalf) {
        let token = std::fs::read_to_string(self.data_dir.join("host.token"))
            .expect("token")
            .trim()
            .to_string();
        let reply = self
            .call(
                reader,
                writer,
                "h",
                "handshake",
                serde_json::json!({ "token": token, "clientId": "test" }),
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

fn success_result(frame: &Frame) -> &serde_json::Value {
    match frame {
        Frame::Success(success) => &success.result,
        other => panic!("expected success, got {other:?}"),
    }
}

#[tokio::test]
async fn the_second_batch_is_registered_and_project_scoped() {
    let mut rig = Rig::start("scope").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    rig.write_file("src/a.txt", "hello\n");

    // 相对路径 ⇒ 成功。
    let ok = rig
        .call(
            &mut reader,
            &mut writer,
            "1",
            "files.list_local_history",
            serde_json::json!({ "projectId": "p1", "path": "src/a.txt" }),
        )
        .await;
    assert!(matches!(ok, Frame::Success(_)), "{ok:?}");
    assert_eq!(success_result(&ok), &serde_json::json!([]), "还没有历史");

    // 绝对路径 ⇒ 拒绝（客户端不该能指到项目外）。
    let absolute = rig
        .call(
            &mut reader,
            &mut writer,
            "2",
            "files.list_local_history",
            serde_json::json!({ "projectId": "p1", "path": "/etc/hosts" }),
        )
        .await;
    assert!(matches!(absolute, Frame::Failure(_)), "{absolute:?}");

    // `..` ⇒ 拒绝。
    let escape = rig
        .call(
            &mut reader,
            &mut writer,
            "3",
            "files.list_local_history",
            serde_json::json!({ "projectId": "p1", "path": "../../etc/hosts" }),
        )
        .await;
    assert!(matches!(escape, Frame::Failure(_)), "{escape:?}");

    // 不存在的项目 ⇒ NotFound（不是"空结果"）。
    let unknown = rig
        .call(
            &mut reader,
            &mut writer,
            "4",
            "files.list_local_history",
            serde_json::json!({ "projectId": "nope", "path": "src/a.txt" }),
        )
        .await;
    match unknown {
        Frame::Failure(failure) => {
            assert_eq!(
                failure.error.code,
                aeroric_protocol::error::ErrorCode::NotFound
            );
            assert!(
                failure.error.message.contains("nope"),
                "{:?}",
                failure.error
            );
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
    rig.cleanup();
}

#[tokio::test]
async fn local_history_round_trips_and_restore_is_undoable() {
    let mut rig = Rig::start("history").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    rig.write_file("src/b.txt", "first\n");

    // 先手动把 v1 记进历史（快照只在格式化/恢复这些路径上自动产生）。
    // 布局：`<root>/.aeroric/local-history/<相对路径目录>/<entryId>.txt`——注意历史目录
    // 以**被快照的那个文件**为名再开一层（`history_dir_for_relative_path`）。
    // 条目 id 是**数字时间戳**（core 的 `entry_id_timestamp_ms` 会把认不出时间戳的名字
    // 直接过滤掉——那不是漏读，是"这条记录看不懂就不当成历史"）。
    let snapshot_dir = rig.project_dir.join(".aeroric/local-history/src/b.txt");
    std::fs::create_dir_all(&snapshot_dir).expect("history dir");
    std::fs::write(snapshot_dir.join("1700000000000.txt"), "first\n").expect("snapshot");
    rig.write_file("src/b.txt", "second\n");

    let listed = rig
        .call(
            &mut reader,
            &mut writer,
            "1",
            "files.list_local_history",
            serde_json::json!({ "projectId": "p1", "path": "src/b.txt" }),
        )
        .await;
    let entries = success_result(&listed)
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(entries.len(), 1, "{listed:?}");
    assert_eq!(entries[0]["relativePath"], serde_json::json!("src/b.txt"));

    let read = rig
        .call(
            &mut reader,
            &mut writer,
            "2",
            "files.read_local_history_entry",
            serde_json::json!({ "projectId": "p1", "path": "src/b.txt", "entryId": "1700000000000" }),
        )
        .await;
    assert_eq!(
        success_result(&read)["content"],
        serde_json::json!("first\n")
    );

    // 恢复：文件回到 v1，且**恢复前先兜底快照**（所以历史里多了一条，恢复可撤回）。
    let restored = rig
        .call(
            &mut reader,
            &mut writer,
            "3",
            "files.restore_local_history_entry",
            serde_json::json!({ "projectId": "p1", "path": "src/b.txt", "entryId": "1700000000000" }),
        )
        .await;
    assert!(matches!(restored, Frame::Success(_)), "{restored:?}");
    assert_eq!(rig.read_file("src/b.txt"), "first\n");

    let after = rig
        .call(
            &mut reader,
            &mut writer,
            "4",
            "files.list_local_history",
            serde_json::json!({ "projectId": "p1", "path": "src/b.txt" }),
        )
        .await;
    let entries = success_result(&after)
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        entries.len(),
        2,
        "恢复前的兜底快照让恢复本身可撤回：{after:?}"
    );

    // 不存在的条目是明确错误，不是空内容。
    let missing = rig
        .call(
            &mut reader,
            &mut writer,
            "5",
            "files.read_local_history_entry",
            serde_json::json!({ "projectId": "p1", "path": "src/b.txt", "entryId": "ghost" }),
        )
        .await;
    assert!(matches!(missing, Frame::Failure(_)), "{missing:?}");
    rig.cleanup();
}

#[tokio::test]
async fn copy_refuses_to_overwrite_and_stays_inside_the_project() {
    let mut rig = Rig::start("copy").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    rig.write_file("src/c.txt", "payload\n");
    rig.write_file("out/c.txt", "existing\n");

    let refused = rig
        .call(
            &mut reader,
            &mut writer,
            "1",
            "files.copy_paths_to_directory",
            serde_json::json!({ "projectId": "p1", "paths": ["src/c.txt"], "targetDir": "out" }),
        )
        .await;
    match refused {
        Frame::Failure(failure) => assert!(
            failure.error.message.contains("overwrite"),
            "要说明是拒绝覆盖：{:?}",
            failure.error
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        rig.read_file("out/c.txt"),
        "existing\n",
        "拒绝时目标一字不动"
    );

    // 目标目录不存在 ⇒ 建出来再复制。
    let copied = rig
        .call(
            &mut reader,
            &mut writer,
            "2",
            "files.copy_paths_to_directory",
            serde_json::json!({ "projectId": "p1", "paths": ["src/c.txt"], "targetDir": "fresh" }),
        )
        .await;
    assert!(matches!(copied, Frame::Success(_)), "{copied:?}");
    assert_eq!(rig.read_file("fresh/c.txt"), "payload\n");

    // 源在项目外 ⇒ 拒绝（`..`）。
    let escape = rig
        .call(
            &mut reader,
            &mut writer,
            "3",
            "files.copy_paths_to_directory",
            serde_json::json!({ "projectId": "p1", "paths": ["../outside.txt"], "targetDir": "fresh" }),
        )
        .await;
    assert!(matches!(escape, Frame::Failure(_)), "{escape:?}");

    // 目标在项目外 ⇒ 同样拒绝。
    let target_outside = rig
        .call(
            &mut reader,
            &mut writer,
            "4",
            "files.copy_paths_to_directory",
            serde_json::json!({ "projectId": "p1", "paths": ["src/c.txt"], "targetDir": "../escape" }),
        )
        .await;
    assert!(
        matches!(target_outside, Frame::Failure(_)),
        "{target_outside:?}"
    );
    assert!(
        !rig.root.join("escape").exists(),
        "拒绝时不该真的建出项目外的目录"
    );
    rig.cleanup();
}

#[tokio::test]
async fn format_keeps_the_original_when_the_formatter_fails() {
    let mut rig = Rig::start("format").await;
    let (mut reader, mut writer) = rig.connect().await;
    rig.handshake(&mut reader, &mut writer).await;
    // `.txt` 没有配置 formatter ⇒ core 报"No formatter is configured"，文件必须一字不动。
    rig.write_file("src/d.txt", "keep me\n");

    let failed = rig
        .call(
            &mut reader,
            &mut writer,
            "1",
            "files.format_file",
            serde_json::json!({ "projectId": "p1", "path": "src/d.txt" }),
        )
        .await;
    assert!(matches!(failed, Frame::Failure(_)), "{failed:?}");
    assert_eq!(rig.read_file("src/d.txt"), "keep me\n", "失败时原文不变");
    rig.cleanup();
}
