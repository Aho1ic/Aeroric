//! 外部宿主的监督层（Task P8-05 Action 1/2/6/9）。
//!
//! 这一层只决定一件事：**谁是业务写者**，然后把它交给别人。规则三条：
//!
//! 1. **先连后起**（Action 9）：socket 上有活人就连过去；绝不 `kill` 再 `spawn`
//!    （ADR-027：租约由持有者释放，本进程连"猜哪个 PID 是旧宿主"都不该做）。
//! 2. **helper 只从可信位置启动**（Action 9）：`<自身 exe 目录>/aeroric-cli` 或开发期
//!    的 target-specific 生成路径，**不搜 `PATH`、不去找 Native Preview 的目录**。
//! 3. **不兼容就要求协调重启**（Action 6）：报 [`SupervisorOutcome::NeedsCoordinatedRestart`]，
//!    而不是杀进程重来。
//!
//! 四个端口（探测/连接/启动/就绪）都可注入，所以判决矩阵可以在没有真进程的情况下逐条
//! 钉住；真进程那一半由 `scripts/native/tauri-host-smoke.mjs` 覆盖（那里的判据是 PID）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::host_client::{host_socket_path, HostClient};

/// helper 的候选来源，**顺序即优先级**。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelperOrigin {
    /// 打包后的宿主：`<exe 目录>/aeroric-cli`（Tauri 的 `externalBin` 落在这里）。
    Bundle(PathBuf),
    /// 开发期：`src-tauri/binaries/aeroric-cli-<triple>`（由
    /// `pnpm native:prepare-tauri-host` 生成并校验过）。
    Generated(PathBuf),
}

impl HelperOrigin {
    pub fn path(&self) -> &Path {
        match self {
            HelperOrigin::Bundle(path) | HelperOrigin::Generated(path) => path,
        }
    }
}

/// 为什么没能连上/起不来。每一种都对应一句用户可读的话。
#[derive(Debug, PartialEq, Eq)]
pub enum SupervisorError {
    HelperMissing { exe_dir: PathBuf },
    SpawnFailed { path: PathBuf, reason: String },
    NotReady { socket: PathBuf, waited_ms: u64 },
    Authenticate { socket: PathBuf, reason: String },
}

impl std::fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupervisorError::HelperMissing { exe_dir } => write!(
                formatter,
                "this build has no host helper next to {exe_dir:?}; run `pnpm native:prepare-tauri-host`"
            ),
            SupervisorError::SpawnFailed { path, reason } => {
                write!(formatter, "cannot start the host helper at {path:?}: {reason}")
            }
            SupervisorError::NotReady { socket, waited_ms } => write!(
                formatter,
                "the host helper did not become reachable within {waited_ms} ms (socket {socket:?})"
            ),
            SupervisorError::Authenticate { socket, reason } => {
                write!(formatter, "could not authenticate to the host at {socket:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for SupervisorError {}

/// 监督层的判决。
#[derive(Debug)]
pub enum SupervisorOutcome {
    /// 已经有一个可用的宿主：连上去，**没有启动任何东西**。
    Attached(HostClient),
    /// 没有活宿主：启动了 helper 并连上它。
    SpawnedAndAttached(HostClient),
    /// 有个进程占着写租约，但它的协议与本进程对不上：**不杀、不抢**，提示协调重启。
    NeedsCoordinatedRestart { detail: String },
}

/// [`HostSupervisor::attach_or_spawn`] 的结果拆成可搬运的形态。
///
/// 为什么要拆：把 `HostClient` 搬出 async 上下文时，`SupervisorOutcome` 会带着它的
/// 枚举形状穿过 `block_on` 的边界，而判决的三个分支在调用方那里各自要接不同的话
/// （连上/刚启动/需协调重启）。拆开之后每个分支自带它需要的载荷。
pub enum SupervisorParts {
    Attached(HostClient),
    SpawnedAndAttached(HostClient),
    NeedsCoordinatedRestart { detail: String },
}

impl SupervisorOutcome {
    /// 拆成 [`SupervisorParts`]（消费自身：判决已经做完，调用方要的是载荷）。
    pub fn into_parts(self) -> SupervisorParts {
        match self {
            SupervisorOutcome::Attached(client) => SupervisorParts::Attached(client),
            SupervisorOutcome::SpawnedAndAttached(client) => {
                SupervisorParts::SpawnedAndAttached(client)
            }
            SupervisorOutcome::NeedsCoordinatedRestart { detail } => {
                SupervisorParts::NeedsCoordinatedRestart { detail }
            }
        }
    }

    #[allow(dead_code, reason = "判决的三个分支在调用方各自接话；本片由用例读")]
    pub fn is_attached(&self) -> bool {
        !matches!(self, SupervisorOutcome::NeedsCoordinatedRestart { .. })
    }
}

/// socket 活死探测。
pub trait LivenessProbe: Send + Sync {
    fn is_live(&self, socket: &Path) -> bool;
}

/// 起 helper（真实现是 `Command::new(helper).args(["--data-dir", …, "serve"])`）。
pub trait HelperSpawner: Send + Sync {
    fn spawn(&self, helper: &Path, data_dir: &Path) -> Result<(), SupervisorError>;
}

/// 等待 helper 就绪。
pub trait ReadinessWaiter: Send + Sync {
    fn wait(&self, socket: &Path, timeout_ms: u64) -> bool;
}

pub struct HostSupervisor {
    exe_dir: PathBuf,
    generated_helper: Option<PathBuf>,
    probe: Arc<dyn LivenessProbe>,
    spawner: Arc<dyn HelperSpawner>,
    waiter: Arc<dyn ReadinessWaiter>,
    readiness_timeout_ms: u64,
    /// helper 缺失时是否**仍然尝试**起一个（打包尚未落地时为 false：宁可 External 失败，
    /// 也不要因为找不到 helper 就退回"自己当写者"）。
    spawn_enabled: bool,
}

impl HostSupervisor {
    pub fn new(
        exe_dir: impl Into<PathBuf>,
        generated_helper: Option<PathBuf>,
        probe: Arc<dyn LivenessProbe>,
        spawner: Arc<dyn HelperSpawner>,
        waiter: Arc<dyn ReadinessWaiter>,
    ) -> Self {
        Self {
            exe_dir: exe_dir.into(),
            generated_helper,
            probe,
            spawner,
            waiter,
            readiness_timeout_ms: 10_000,
            spawn_enabled: true,
        }
    }

    /// 关掉"没有活宿主就自己起一个"。打包还没落地时用这个：起不来 helper 时应当如实
    /// 报告"没有宿主"，而不是回退成第二个写者。
    pub fn without_spawning(mut self) -> Self {
        self.spawn_enabled = false;
        self
    }

    /// helper 落点：**只在这两处找**（Action 9）。
    pub fn resolve_helper(&self) -> Result<HelperOrigin, SupervisorError> {
        let bundled = self.exe_dir.join(helper_file_name());
        if bundled.is_file() {
            return Ok(HelperOrigin::Bundle(bundled));
        }
        if let Some(generated) = &self.generated_helper {
            if generated.is_file() {
                return Ok(HelperOrigin::Generated(generated.clone()));
            }
        }
        Err(SupervisorError::HelperMissing {
            exe_dir: self.exe_dir.clone(),
        })
    }

    /// 判决：先连后起。
    pub async fn attach_or_spawn(
        &self,
        data_dir: &Path,
        client_id: &str,
    ) -> Result<SupervisorOutcome, SupervisorError> {
        let socket = host_socket_path(data_dir);
        if self.probe.is_live(&socket) {
            return match HostClient::connect(data_dir, client_id).await {
                Ok(client) => Ok(SupervisorOutcome::Attached(client)),
                Err(error) => {
                    let reason = error.to_string();
                    if looks_like_version_skew(&reason) {
                        Ok(SupervisorOutcome::NeedsCoordinatedRestart { detail: reason })
                    } else {
                        Err(SupervisorError::Authenticate { socket, reason })
                    }
                }
            };
        }

        if !self.spawn_enabled {
            // 明确不自己起：调用方（`CorePlacement` 的判决）据此把本进程当作
            // "没有外部宿主可用"，而不是偷偷成为写者。
            return Err(SupervisorError::HelperMissing {
                exe_dir: self.exe_dir.clone(),
            });
        }
        let helper = self.resolve_helper()?;
        self.spawner.spawn(helper.path(), data_dir)?;
        if !self.waiter.wait(&socket, self.readiness_timeout_ms) {
            return Err(SupervisorError::NotReady {
                socket,
                waited_ms: self.readiness_timeout_ms,
            });
        }
        match HostClient::connect(data_dir, client_id).await {
            Ok(client) => Ok(SupervisorOutcome::SpawnedAndAttached(client)),
            Err(error) => Err(SupervisorError::Authenticate {
                socket,
                reason: error.to_string(),
            }),
        }
    }
}

/// helper 文件名（打包后不带 target 后缀：Tauri 的 `externalBin` 会剥掉它）。
pub fn helper_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "aeroric-cli.exe"
    } else {
        "aeroric-cli"
    }
}

/// 生成 helper 的路径。`triple` 由调用方**显式**给出（绝不把 runner 的 CPU 当目标 CPU）。
///
/// 生产消费者是 `pnpm native:prepare-tauri-host`（Action 8，下一切片）；本片由用例钉住
/// 命名约定（`<binaries>/aeroric-cli-<triple>`）。
#[allow(dead_code, reason = "prepare-tauri-host 脚本落地前由用例消费")]
pub fn generated_helper_path(binaries_dir: &Path, triple: &str) -> PathBuf {
    let suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    binaries_dir.join(format!("aeroric-cli-{triple}{suffix}"))
}

/// 这句失败像不像"版本/协议对不上"。判据故意窄：认证失败**不算**——建议重启解决不了
/// token 不匹配，把它混进来会让用户按错误的提示去操作。
fn looks_like_version_skew(reason: &str) -> bool {
    let lowered = reason.to_ascii_lowercase();
    [
        "unknown method",
        "method not found",
        "unsupported protocol",
        "protocol version",
        "unexpected field",
        "missing field",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

/// 真实现：带 2s 超时的 UDS 连接尝试（只回答"有没有人 accept"，与宿主侧 probe_liveness
/// 同语义同超时——误判 Dead 会去抢活人的 socket，比拒绝启动危险）。
pub struct UdsLivenessProbe;

impl LivenessProbe for UdsLivenessProbe {
    fn is_live(&self, socket: &Path) -> bool {
        let socket = socket.to_path_buf();
        let attempt = async move {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                tokio::net::UnixStream::connect(&socket),
            )
            .await
            .map(|result| result.is_ok())
            .unwrap_or(false)
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(attempt),
            Err(_) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map(|runtime| runtime.block_on(attempt))
                .unwrap_or(false),
        }
    }
}

/// 真实现：`Command::new(helper)` 起 `serve`。数据根由 argv 显式给出（INV-03：宿主不查
/// `$HOME`），环境继承（它要用 `PATH` 找 agent 可执行文件）。
pub struct CommandHelperSpawner;

impl HelperSpawner for CommandHelperSpawner {
    fn spawn(&self, helper: &Path, data_dir: &Path) -> Result<(), SupervisorError> {
        std::process::Command::new(helper)
            .arg("--data-dir")
            .arg(data_dir)
            .arg("serve")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map(|_child| ())
            .map_err(|error| SupervisorError::SpawnFailed {
                path: helper.to_path_buf(),
                reason: error.to_string(),
            })
    }
}

/// 真实现：轮询 socket 直到活或超时（25ms 一次）。
pub struct PollingWaiter;

impl ReadinessWaiter for PollingWaiter {
    fn wait(&self, socket: &Path, timeout_ms: u64) -> bool {
        let probe = UdsLivenessProbe;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if probe.is_live(socket) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}
