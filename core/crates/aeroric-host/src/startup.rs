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
use aeroric_core::context::CoreContext;
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

/// 启动完成后的持有物：context（内含写租约，drop 即释放）与 listener。
pub struct StartedHost {
    pub context: CoreContext,
    pub listener: tokio::net::UnixListener,
    pub runtime: RuntimeMetadata,
    pub socket_path: PathBuf,
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
pub async fn run_startup(
    options: &HostOptions,
    clock: Arc<dyn Clock>,
    owner: &LeaseOwner,
    probe: &dyn StorageLocationProbe,
    build_hash: &str,
    protocol_hash: &str,
) -> Result<StartedHost, StartupError> {
    // 1. lock —— 写租约（连卷探测），失败即「另一个写者持有着」，绝不排队等。
    let context = CoreContext::acquire(options.paths.clone(), clock, owner, probe)?;

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
    })
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
