//! 逐步启动：lock → data compat → runtime → listener → ready（P3-06 Action 1）。
//!
//! 五步的顺序是**契约**不是风格：拿到写租约之前不碰数据（两个 host 同时
//! 读 runtime.json 再先后写会互相覆盖）；数据兼容性（schema 版本）没结论
//! 之前不建 listener（ready 了却只会拒绝所有请求，比不 ready 更坏）；
//! listener 没绑上不许报 ready（supervisor 拿到 ready 就开始发 RPC）。
//!
//! 第 1 步复用 [`CoreContext::acquire`]：租约与卷探测是一体的（宿主
//! `core_adapter` 走同一入口，INV-03 的「core 不查环境」由注入的 probe 兑现），
//! host 不再自己调 `DataDirectoryLease::acquire` 造第二条锁路径。
//!
//! `ready` 经父进程管道（`ready_fd`）发一行 JSON——supervisor 的握手正面半边
//! （Action 4「验证现存 listener 的握手」由 `uds::probe_liveness` 的反面承担：
//! 活人拒启、尸体清绑）。

use std::io::Write as _;
use std::os::unix::io::RawFd;
use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::clock::Clock;
use aeroric_core::context::{CoreContext, MemorySecrets, SecretStore};
use aeroric_core::location::StorageLocationProbe;
use aeroric_core::paths::AppPaths;
use aeroric_core::storage::{bump_revision, load_runtime, RuntimeMetadata};
use aeroric_core::storage_lock::{LeaseError, LeaseOwner};
use serde::Serialize;

use crate::uds::{bind_listener, UdsError};

/// 启动参数。全部由调用方注入（INV-03：core/host 不查 `$HOME`）。
pub struct HostOptions {
    pub paths: AppPaths,
    pub socket_path: PathBuf,
    /// 父进程管道的写端（`None` = 无人等 ready，跳过第 5 步的写出）。
    pub ready_fd: Option<RawFd>,
}

/// supervisor metadata 的 ready 帧载荷（Action 4 的字段集；build/protocol
/// 两个 hash 由调用方注入，host 库不猜自己是谁编的）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyInfo {
    pub pid: u32,
    pub started_at_ms: u64,
    pub epoch: String,
    pub revision: String,
    pub build_hash: String,
    pub protocol_hash: String,
    pub data_dir_hash: String,
    pub socket_path: String,
}

/// 启动完成后的持有物：context（内含写租约，drop 即释放）、listener 与 token。
/// context 是 `Arc`：serve 循环与每个 handler 共享同一份（写租约只有一份）。
pub struct StartedHost {
    pub context: Arc<CoreContext>,
    pub listener: tokio::net::UnixListener,
    pub runtime: RuntimeMetadata,
    pub socket_path: PathBuf,
    pub token: std::sync::Arc<crate::auth::HostToken>,
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("step 1 (lock): {0}")]
    Lock(#[from] LeaseError),
    #[error("step 2 (data compat): {0}")]
    DataCompat(String),
    #[error("step 3 (runtime): {0}")]
    Runtime(String),
    #[error("step 4 (listener): {0}")]
    Listener(#[from] UdsError),
    #[error("step 5 (ready): {0}")]
    Ready(String),
}

/// 五步全序执行；任一步失败即停，错误带步骤名（supervisor 的 diagnostics
/// 按这个前缀分类，不靠文案子串猜）。
/// Raise the soft open-file limit to `min(hard, 10240)` (R1, cc-switch
/// 04d0ebf5). An app started from Finder or as a login item inherits launchd's
/// soft limit of 256; the gateway holds a client socket and an upstream
/// connection per request, and the PTYs live here too, so a busy session would
/// otherwise hit EMFILE and fail to open settings or SQLite. The hard limit is
/// left alone; failure is only reported. Returns the soft limit in effect.
#[cfg(unix)]
#[allow(clippy::unnecessary_cast, reason = "rlim_t is not u64 on every unix")]
pub fn raise_open_file_limit() -> Option<u64> {
    const WANT: libc::rlim_t = 10240;
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit/setrlimit only read and write the struct we pass.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return None;
    }
    let target = WANT.min(limit.rlim_max);
    if limit.rlim_cur < target {
        let raised = libc::rlimit {
            rlim_cur: target,
            rlim_max: limit.rlim_max,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } == 0 {
            limit.rlim_cur = target;
        } else {
            eprintln!(
                "aeroric-host: could not raise the open-file limit from {}",
                limit.rlim_cur
            );
        }
    }
    Some(limit.rlim_cur as u64)
}

#[cfg(not(unix))]
pub fn raise_open_file_limit() -> Option<u64> {
    None
}

pub async fn run_startup(
    options: &HostOptions,
    clock: Arc<dyn Clock>,
    owner: &LeaseOwner,
    probe: &dyn StorageLocationProbe,
    build_hash: &str,
    protocol_hash: &str,
) -> Result<StartedHost, StartupError> {
    run_startup_with_secrets(
        options,
        clock,
        owner,
        probe,
        build_hash,
        protocol_hash,
        Arc::new(MemorySecrets::default()),
    )
    .await
}

/// Explicit secret-store composition. The library startup above is safe for
/// embedding/tests and deliberately does not migrate legacy Agent settings.
/// Changing HOME cannot isolate an OS credential service.
pub async fn run_startup_with_secrets(
    options: &HostOptions,
    clock: Arc<dyn Clock>,
    owner: &LeaseOwner,
    probe: &dyn StorageLocationProbe,
    build_hash: &str,
    protocol_hash: &str,
    secrets: Arc<dyn SecretStore>,
) -> Result<StartedHost, StartupError> {
    run_startup_with_secrets_inner(
        options,
        clock,
        owner,
        probe,
        build_hash,
        protocol_hash,
        secrets,
        false,
    )
    .await
}

/// Production composition: run the non-destructive legacy Agent migration
/// after data compatibility succeeds and before the listener can become ready.
pub async fn run_startup_with_secrets_and_legacy_agent_migration(
    options: &HostOptions,
    clock: Arc<dyn Clock>,
    owner: &LeaseOwner,
    probe: &dyn StorageLocationProbe,
    build_hash: &str,
    protocol_hash: &str,
    secrets: Arc<dyn SecretStore>,
) -> Result<StartedHost, StartupError> {
    run_startup_with_secrets_inner(
        options,
        clock,
        owner,
        probe,
        build_hash,
        protocol_hash,
        secrets,
        true,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_startup_with_secrets_inner(
    options: &HostOptions,
    clock: Arc<dyn Clock>,
    owner: &LeaseOwner,
    probe: &dyn StorageLocationProbe,
    build_hash: &str,
    protocol_hash: &str,
    secrets: Arc<dyn SecretStore>,
    migrate_legacy_agent_settings: bool,
) -> Result<StartedHost, StartupError> {
    // 1. lock —— 写租约（连卷探测），失败即「另一个写者持有着」，绝不排队等。
    let context = Arc::new(
        CoreContext::acquire(options.paths.clone(), clock, owner, probe)?
            .with_platform(std::sync::Arc::new(crate::platform_adapter::HostOsServices))
            .with_secrets(secrets),
    );

    // 2. data compat —— schema 版本判定（load 自带 UnsupportedSchema/Corrupt 拒绝）。
    let outcome = load_runtime(&context).map_err(|e| StartupError::DataCompat(e.to_string()))?;

    // 3. runtime —— 兼容结论之上取元数据；首启（Absent）由 host 创建
    //    （bump_revision 是唯一的 runtime.json 写入口，epoch 在那一步定下）。
    let runtime = match outcome.found() {
        Some(runtime) => runtime,
        None => {
            let bumped =
                bump_revision(&context).map_err(|e| StartupError::Runtime(e.to_string()))?;
            bumped.metadata
        }
    };

    // Only the production composition opts into this one-way copy. Keep it
    // before token/listener/ready so a failed migration cannot expose a host
    // that has announced readiness with incomplete connection data.
    if migrate_legacy_agent_settings {
        aeroric_core::agent_connections::migration::migrate_legacy_agent_settings(&context)
            .map_err(StartupError::DataCompat)?;
        migrate_omp_plaintext_keys(options.paths.data_dir());
    }

    // 3.5 token —— 握手判据的载体在 listener 之前落位：绑上端口却没有 token，
    //      第一个连接的 handshake 只会得到「文件不存在」，那是自造拒绝面。
    //      `load_or_create` 幂等（存在即读），重启不换 token。
    let token_path = options.paths.data_dir().join("host.token");
    let token = crate::auth::HostToken::load_or_create(&token_path)
        .map_err(|e| StartupError::Runtime(e.to_string()))?;

    // 4. listener —— 路径上限 + 残留探活 + bind + 0600。
    let listener = bind_listener(&options.socket_path).await?;

    // 5. ready —— 父进程管道一行 JSON；写失败＝supervisor 已经不在了，
    //    按启动失败处理（僵而不 ready 会让 supervisor 永远等）。
    if let Some(fd) = options.ready_fd {
        let info = ReadyInfo {
            pid: std::process::id(),
            started_at_ms: context.clock.now_ms(),
            epoch: runtime.epoch.clone(),
            revision: runtime.revision.clone(),
            build_hash: build_hash.to_string(),
            protocol_hash: protocol_hash.to_string(),
            data_dir_hash: lease_identity_hash(&context),
            socket_path: options.socket_path.to_string_lossy().into_owned(),
        };
        let line = serde_json::to_string(&info).map_err(|e| StartupError::Ready(e.to_string()))?;
        let mut pipe = std::fs::File::from_raw_fd_owned(fd);
        pipe.write_all(line.as_bytes())
            .and_then(|()| pipe.write_all(b"\n"))
            .map_err(|e| StartupError::Ready(e.to_string()))?;
    }

    Ok(StartedHost {
        context,
        listener,
        runtime,
        socket_path: options.socket_path.clone(),
        token: std::sync::Arc::new(token),
    })
}

/// cc-switch 复查 D2：Aeroric 早先在 omp 档案的 models.yml 里写了明文 apiKey，
/// 启动时把它们换成变量名。尽力而为：失败只记日志，不挡启动（omp 照旧能读明文）。
fn migrate_omp_plaintext_keys(data_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(data_dir.join("agent-homes")) else {
        return;
    };
    for entry in entries.flatten() {
        let home = entry.path();
        if !home.is_dir() {
            continue;
        }
        match aeroric_core::settings::omp_home::migrate_plaintext_models_yml(&home) {
            Ok(true) => eprintln!(
                "aeroric-host: replaced the plaintext omp apiKey in {}",
                home.join("models.yml").display()
            ),
            Ok(false) => {}
            Err(error) => eprintln!(
                "aeroric-host: cannot migrate {}: {error}",
                home.join("models.yml").display()
            ),
        }
    }
}

/// data-dir 的标识哈希（canonical 路径的 sha256 前 16 位十六进制；
/// supervisor 用它区分「同一个数据目录的旧 listener」与「别的目录的活 host」）。
fn lease_identity_hash(context: &CoreContext) -> String {
    use sha2::Digest as _;
    let canonical = std::fs::canonicalize(context.paths.data_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| context.paths.data_dir().to_string_lossy().into_owned());
    let digest = sha2::Sha256::digest(canonical.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// `FromRawFd` 的显式小封装：ready_fd 的所有权在这里接管（写完由 Drop 关闭）。
trait FromRawFdOwned: Sized {
    fn from_raw_fd_owned(fd: RawFd) -> Self;
}

impl FromRawFdOwned for std::fs::File {
    fn from_raw_fd_owned(fd: RawFd) -> Self {
        // SAFETY: 调用方（`aeroric-cli serve` 的父进程协议）保证 fd 有效且
        // 本进程持有其所有权；ready 写完即由 Drop 关闭，无双关路径。
        unsafe { <std::fs::File as std::os::unix::io::FromRawFd>::from_raw_fd(fd) }
    }
}

// ── serve 循环（第四片接线：accept → peer-UID → 连接循环）────────────────

/// 运行中 host 的关停把手。`shutdown()` 之后 listener 关闭、租约随
/// `StartedHost` 的 drop 释放——**watcher 取消与 storage flush 的完整退出
/// 序是第五片（shutdown.rs）的地盘，这里只立 listener/lease 这一层**。
pub struct RunningHost {
    shutdown: tokio::sync::oneshot::Sender<()>,
}

impl RunningHost {
    pub fn shutdown(self) {
        let _ = self.shutdown.send(());
    }
}

/// 启动 accept 循环（当前 tokio runtime 上 spawn）。每条连接三道闸的顺序
/// 是设计：**peer-UID 先于任何协议字节**——未认证来源不许读到任何协议行为，
/// 连拒绝帧都不发（直接 drop），免得给探测者一个 oracle。
pub fn serve(started: StartedHost, services: Arc<crate::dispatcher::HostServices>) -> RunningHost {
    let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let StartedHost {
            context,
            listener,
            runtime: _runtime,
            socket_path,
            token: _token,
        } = started;
        let _ = &context;
        // 活跃连接计数：shutdown 的 drain 判据。计数归零或 5 s 超时——
        // 超时后**照样**走清理（僵连接不许拖住租约释放，让位比等它诚实）。
        let live = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        loop {
            tokio::select! {
                _ = &mut rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, _addr)) = accepted else { continue };
                    if crate::auth::peer_uid_matches(&stream).is_err() {
                        drop(stream);
                        continue;
                    }
                    let (read, write) = stream.into_split();
                    let context = std::sync::Arc::new(crate::dispatcher::CallContext {
                        services: Arc::clone(&services.services),
                        connection: Arc::new(tokio::sync::Mutex::new(
                            crate::subscriptions::Connection::new(),
                        )),
                    });
                    live.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let live_join = Arc::clone(&live);
                    tokio::spawn(async move {
                        crate::dispatcher::serve_connection(
                            tokio::io::BufReader::new(read),
                            write,
                            context,
                        )
                        .await;
                        live_join.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    });
                }
            }
        }
        // 退出序（Action 5 的 host 侧当前射程：无自有子进程、无独立 storage
        // 写缓冲——flush 归写者路径的原子写；watcher 取消归连接 drain）：
        // 1) 不再 accept（上面已 break）；2) drain 活跃连接或超时；
        // 3) listener 关闭＋租约释放；4) socket 文件清理（正常退出不留残留，
        //    crash 残留由下次启动的探活路径处理——两条路都有主）。
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if live.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                break;
            }
            if tokio::time::timeout_at(
                deadline,
                tokio::time::sleep(std::time::Duration::from_millis(20)),
            )
            .await
            .is_err()
            {
                break;
            }
        }
        drop(listener);
        drop(context);
        std::fs::remove_file(&socket_path).ok();
    });
    RunningHost { shutdown: tx }
}
