//! 四个集成测试文件共享的真机 harness：临时 data dir、完整 `run_startup`、
//! `serve` 循环与 UDS 客户端。全部走 `/tmp` 短根（macOS `$TMPDIR` 逼近
//! sun_path 上限，276 片案底）；目录带进程内原子序号（277 片案底）。
//! 完整形状见 MIGRATION_PLAN.md 补记二百七十六/二百七十七。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aeroric_core::clock::SystemClock;
use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::paths::AppPaths;
use aeroric_core::storage_lock::LeaseOwner;
use aeroric_host::dispatcher::HostServices;
use aeroric_host::startup::{run_startup, RunningHost};
use aeroric_protocol::envelope::{decode_line, encode_line, Frame, Request};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 本地卷探针：集成测试在本地盘跑，卷探测给 Local（`UnprobedLocation`
/// 的 Unknown 是**拒绝**，用它等于测不到启动路径）。
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

pub struct Harness {
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub socket_path: PathBuf,
    pub token: String,
    pub services: Arc<HostServices>,
    running: Option<RunningHost>,
}

impl Harness {
    pub async fn start(tag: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!("ah-it-{tag}-{}-{n}", std::process::id()));
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let paths = AppPaths::from_user_home(&home);
        let data_dir = paths.data_dir().to_path_buf();
        let socket_path = data_dir.join("host.sock");
        let started = run_startup(
            &aeroric_host::startup::HostOptions {
                paths,
                socket_path: socket_path.clone(),
                ready_fd: None,
            },
            Arc::new(SystemClock),
            &LeaseOwner {
                application: "aeroric-host-test".to_string(),
                version: "0-test".to_string(),
            },
            &LocalProbe,
            "build-hash-test",
            "protocol-hash-test",
        )
        .await
        .expect("startup must succeed");
        // token 的**明文**只为测试断言而读文件；生产客户端经带外渠道拿它。
        let token = std::fs::read_to_string(data_dir.join("host.token"))
            .expect("token file")
            .trim()
            .to_string();
        let services = HostServices::new(
            Arc::clone(&started.token),
            Arc::new(aeroric_core::events::EventLog::default()),
        );
        let running = Some(aeroric_host::startup::serve(started, Arc::clone(&services)));
        Self {
            home: root,
            data_dir,
            socket_path,
            token,
            services,
            running,
        }
    }

    pub async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .expect("connect");
        let (read, write) = stream.into_split();
        (BufReader::new(read), write)
    }

    /// 发一帧收一帧（响应必须是协议帧，任何 codec 级断连在这里显形为 Err）。
    pub async fn call(
        reader: &mut BufReader<OwnedReadHalf>,
        writer: &mut OwnedWriteHalf,
        request: &Frame,
    ) -> Frame {
        // encode_line 自带行尾换行（双换行会在流里留空行=下一帧 Empty 断连）。
        let line = encode_line(request).expect("encode");
        writer.write_all(line.as_bytes()).await.expect("write");
        writer.flush().await.expect("flush");
        read_one_frame(reader).await
    }

    pub fn request(id: &str, method: &str, params: serde_json::Value) -> Frame {
        Frame::Request(Request {
            v: 1,
            id: id.to_string(),
            method: method.to_string(),
            params,
            idempotency_key: None,
            extra: Default::default(),
        })
    }

    pub async fn handshake(
        reader: &mut BufReader<OwnedReadHalf>,
        writer: &mut OwnedWriteHalf,
        token: &str,
        client_id: &str,
    ) -> Frame {
        Self::call(
            reader,
            writer,
            &Self::request(
                "h-1",
                "handshake",
                serde_json::json!({ "token": token, "clientId": client_id }),
            ),
        )
        .await
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.shutdown();
        }
        std::fs::remove_dir_all(&self.home).ok();
    }
}

/// 读一个 `\n` 定界帧（测试侧的极简 codec；超时 5 s——永挂在这里就是断连
/// 语义没生效，直接 panic 出栈）。
pub async fn read_one_frame(reader: &mut BufReader<OwnedReadHalf>) -> Frame {
    let attempt = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut buf: Vec<u8> = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match reader.read(&mut byte).await {
                Ok(0) => panic!("connection closed before a full frame; buf={buf:?}"),
                Ok(_) => {
                    buf.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(e) => panic!("read error: {e}"),
            }
        }
        decode_line(&buf).expect("response must be a valid frame")
    })
    .await;
    attempt.expect("frame must arrive within 5 s (a hang here means the gate never answered)")
}

/// 断言连接已被 host 切断（读侧 EOF）。
pub async fn assert_closed(reader: &mut BufReader<OwnedReadHalf>) {
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), reader.read(&mut byte))
        .await
        .expect("EOF must arrive within 5 s, not hang");
    assert_eq!(read.expect("read"), 0, "host must close the connection");
}
