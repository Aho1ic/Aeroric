//! The Tauri host's entry into the native core (Task P2-05).
//!
//! This module owns exactly one decision at start-up — *may this process be the business
//! writer of this data directory* — and then hands the answer to everyone else. It is
//! deliberately not the place where task logic lives: `TaskService` decides what a write
//! means, `LegacyTaskExecutor` decides how a process starts, and this file only decides
//! whether we are allowed to be the one doing either.
//!
//! Two ADRs shape it:
//!
//! - **ADR-021**: the writer role is handed over on macOS only. Windows/Linux keep the
//!   shipping client as sole writer until P7, because that is where the product is
//!   actually tested. Crucially the *platform* judgement lives here, not in core —
//!   `StorageLocation::writer_eligible()` keeps answering only "is this location worthy",
//!   and core stays free of `#[cfg(target_os)]` politics.
//! - **ADR-027**: a second instance that cannot take the lease starts **read-only** and
//!   says so. It never steals the lease (a lease is released by its holder, and the plan
//!   forbids killing a foreign PID), and it never falls back to a temp directory — two
//!   divergent data sets that both look healthy is the one outcome worse than refusing.
//!
//! The mode decision is split out as [`decide_mode`] so the platform × lease-outcome
//! matrix is testable without touching a real filesystem.
//!
//! `install_core_services` runs from `lib.rs`'s setup hook, so the gateway is constructed on the
//! product path and the writer lease **is** taken on macOS from this point. That is deliberate and
//! it is not yet the writer switch: `storage.rs` still saves the task table, and nothing routes a
//! mutation through `TaskService`. What changes is that the refusal is now real — a second instance
//! starts read-only and says so — and that core's cover fingerprints start recording what the
//! legacy save path does to the same files, which is the counter-example ADR-011 and acceptance #3
//! ask for. Items still unused by the product path carry a targeted `dead_code` allowance at the
//! item, never a module-wide one: an allow whose stated reason has become false is worse than a
//! warning.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::{
    tasks::{
        AgentFamilies, LiveRuns, ObservationOrigin, ProtocolFamily, TaskEvent, TaskEventSink,
        TaskNotification, TaskRuntimeObserver, TaskService, TaskSessionFacts, TaskStatus,
    },
    AppPaths, Clock, CoreContext, LeaseError, LeaseOwner, SystemClock,
};

use crate::startup_diagnostics;
use crate::storage_location::PlatformLocationProbe;

/// Why this process is or is not the writer. Every variant is a *statement*, and the
/// three non-`Writer` ones each carry a different user-visible consequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreMode {
    /// We hold the lease and may write.
    Writer,
    /// ADR-021: this platform has not been handed the role yet. Not a fault, and the
    /// shipping client is *still* writing — so unlike the variants below nothing is
    /// degraded, the core is simply a reader here.
    HandoverDeferred,
    /// ADR-027: another live Aeroric process holds the lease.
    ReadOnlyBusy,
    /// ADR-010: this location was refused (unprobeable volume, network mount, no
    /// permission receipt). Reported, because a silent refusal here would look like a
    /// working installation that never saves.
    ReadOnlyLocation,
}

impl CoreMode {
    #[allow(
        dead_code,
        reason = "读它的是 CoreGateway 上的同名转发，那条路要等第一个走 core 的写入口"
    )]
    pub(crate) fn is_writer(self) -> bool {
        self == CoreMode::Writer
    }

    /// The component name the frontend matches on to pick its wording.
    pub(crate) fn diagnostics_component(self) -> Option<&'static str> {
        match self {
            CoreMode::ReadOnlyBusy => Some("storage-lease"),
            CoreMode::ReadOnlyLocation => Some("storage-location"),
            _ => None,
        }
    }
}

/// 业务面由**谁**拥有（Task P8-05 Action 2 的互斥选择）。
///
/// 这是比 [`CoreMode`] 更外一层的判断：`CoreMode` 说"本进程能不能写"，
/// `CorePlacement` 说"业务面归谁"。两者必须**同时**成立才不会出现两个写者：
/// `External` 时本进程连租约都不去取（取了就是第二个写者），`Embedded` 时才走
/// `CoreMode` 那套。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CorePlacement {
    /// 本进程拥有业务面：起 watcher / 用量索引 / 笔记调度 / local router，并按
    /// [`CoreMode`] 决定能不能写。今天的产品路径就是这一档。
    Embedded,
    /// 另一个进程（host helper）拥有业务面：本进程只连它，**不取租约、不起任何后台
    /// 服务**（Action 2："同进程不双启动 watcher、remote server、notebook scheduler、
    /// local router"）。
    External,
}

/// 外部宿主的可用形态。分成三档而不是一个 bool，因为"没有宿主但有 helper"与
/// "没有宿主也没有 helper"对下一步的动作完全不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostAvailability {
    /// socket 上有活人：连它。
    Live,
    /// 没有宿主，但有可启动的 helper：**本片仍走 Embedded**（打包落地前，helper 的
    /// 存在不能成为"默认外置"的理由——`pnpm native:prepare-tauri-host` 还没进产品
    /// 流程，而每一次启动都去 spawn 一个宿主是行为变更，不是接线）。
    AbsentButHelperPresent,
    /// 没有宿主也没有 helper：Embedded（今天的默认）。
    Absent,
}

/// 判决要读的两件事：有没有活宿主、以及连不连得上。
///
/// 拆成端口是为了让"External 时不取租约""连不上就只读而不是自立"这两条**能在没有真
/// 宿主的情况下断言**——它们正是最贵的那两个错误。
pub(crate) trait PlacementProbe {
    fn availability(&self, data_dir: &Path) -> HostAvailability;
    /// 连宿主；`Ok((client, spawned))` 里 `spawned` 表示"这次是它被我们启动的"。
    #[allow(clippy::type_complexity)]
    fn attach(&self, data_dir: &Path) -> Result<(crate::host_client::HostClient, bool), String>;
}

/// 生产实现：探活 + 经 `HostSupervisor` 连接。
///
/// **本片不 spawn**（`without_spawning`）：打包尚未落地，而"每次启动都去起一个宿主"是
/// 行为变更，不是接线。所以 `attach` 只在真有活宿主时成功，否则报错并由调用方如实只读。
pub(crate) struct PlacementProbeImpl {
    pub(crate) exe_dir: PathBuf,
    pub(crate) generated_helper: Option<PathBuf>,
    pub(crate) allow_spawn: bool,
}

impl Default for PlacementProbeImpl {
    fn default() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let generated_helper = std::env::var("AERORIC_HOST_HELPER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        Self {
            exe_dir,
            generated_helper,
            allow_spawn: false,
        }
    }
}

impl PlacementProbe for PlacementProbeImpl {
    fn availability(&self, data_dir: &Path) -> HostAvailability {
        let socket = crate::host_client::host_socket_path(data_dir);
        let live = crate::host_supervisor::LivenessProbe::is_live(
            &crate::host_supervisor::UdsLivenessProbe,
            &socket,
        );
        if live {
            return HostAvailability::Live;
        }
        let helper = crate::host_supervisor::HostSupervisor::new(
            self.exe_dir.clone(),
            self.generated_helper.clone(),
            Arc::new(crate::host_supervisor::UdsLivenessProbe),
            Arc::new(crate::host_supervisor::CommandHelperSpawner),
            Arc::new(crate::host_supervisor::PollingWaiter),
        )
        .resolve_helper();
        match helper {
            Ok(_) if self.allow_spawn => HostAvailability::AbsentButHelperPresent,
            Ok(_) => HostAvailability::AbsentButHelperPresent,
            Err(_) => HostAvailability::Absent,
        }
    }

    fn attach(&self, data_dir: &Path) -> Result<(crate::host_client::HostClient, bool), String> {
        let supervisor = crate::host_supervisor::HostSupervisor::new(
            self.exe_dir.clone(),
            self.generated_helper.clone(),
            Arc::new(crate::host_supervisor::UdsLivenessProbe),
            Arc::new(crate::host_supervisor::CommandHelperSpawner),
            Arc::new(crate::host_supervisor::PollingWaiter),
        );
        let supervisor = if self.allow_spawn {
            supervisor
        } else {
            supervisor.without_spawning()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let outcome = runtime
            .block_on(supervisor.attach_or_spawn(data_dir, "tauri"))
            .map_err(|error| error.to_string())?
            .into_parts();
        match outcome {
            crate::host_supervisor::SupervisorParts::Attached(client) => Ok((client, false)),
            crate::host_supervisor::SupervisorParts::SpawnedAndAttached(client) => {
                Ok((client, true))
            }
            crate::host_supervisor::SupervisorParts::NeedsCoordinatedRestart { detail } => {
                Err(format!("需要协调重启：{detail}"))
            }
        }
    }
}

/// 纯判决：只有"macOS 且已经有活宿主"才外置。
///
/// 保守是刻意的：`External` 会让本进程放弃写权与后台服务，这个方向的误判代价是
/// **界面没有数据**（可观察、可回退）；反方向的误判代价是**两个写者**（不可观察、
/// 会丢数据）。所以判据只认"已经在跑的宿主"，不认"理论上可以起一个"。
pub(crate) fn decide_placement(handover_allowed: bool, host: HostAvailability) -> CorePlacement {
    if !handover_allowed {
        return CorePlacement::Embedded;
    }
    match host {
        HostAvailability::Live => CorePlacement::External,
        HostAvailability::AbsentButHelperPresent | HostAvailability::Absent => {
            CorePlacement::Embedded
        }
    }
}

/// The lease attempt, reduced to what the mode decision needs. Keeping this separate from
/// `LeaseError` is what lets the whole matrix be tested without creating a real directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Acquisition {
    #[allow(
        dead_code,
        reason = "生产路径在 Ok(acquisition) 分支里直接给 CoreMode::Writer；该变体只被 decide_mode 的四格矩阵用例构造"
    )]
    Acquired,
    HeldByAnother,
    Refused,
}

/// ADR-021: the one place that knows which platforms have been handed the writer role.
/// P7-01/P7-02 is where this stops being platform-conditional.
pub(crate) fn writer_handover_allowed() -> bool {
    cfg!(target_os = "macos")
}

/// Pure decision behind [`CoreGateway::startup`]: hand-over policy first, then whatever
/// the lease said. Order matters — on a deferred platform we never even attempt the
/// lease, so a Windows install cannot fail in a new way just because this code landed.
pub(crate) fn decide_mode(handover_allowed: bool, acquisition: Acquisition) -> CoreMode {
    if !handover_allowed {
        return CoreMode::HandoverDeferred;
    }
    match acquisition {
        Acquisition::Acquired => CoreMode::Writer,
        Acquisition::HeldByAnother => CoreMode::ReadOnlyBusy,
        Acquisition::Refused => CoreMode::ReadOnlyLocation,
    }
}

/// [`aeroric_core::context::HostTaskSaver`] 的宿主实现：core 决定的记录**原样**落盘。
///
/// 为什么不复用 `save_project_tasks`：那条路把每一行过一遍宿主的 `Task` 结构体，而 `Task` 只声明
/// 它认识的键，serde 静默丢弃其余的——`launchFamily`/`resumeFamily` 就是这么被抹掉的（计划补记六十，
/// 补记六十六用变异复现）。只要宿主还是保存者，它就必须按字节转发，否则"谁写盘"的答案会偷偷变成
/// "谁能表示什么"。空数组 ⇒ 删文件由 `write_task_records_raw` 镜像（ADR-037 后果 1）。
struct StorageTaskBridge;

impl aeroric_core::context::HostTaskSaver for StorageTaskBridge {
    fn save_task_records(
        &self,
        project_id: &str,
        records: &[serde_json::Value],
    ) -> Result<(), String> {
        crate::storage::write_task_records_raw(project_id, records)
    }
}

/// 延迟递交写权那一站的 context：core 没有写者租约（所以只读），但装着一根把字节存回去的桥。
///
/// 抽成函数不是为了整洁。`writer_handover_allowed()` 是 `cfg!`，macOS 上那条分支永远走不到，
/// 于是唯一可测的就是"这个 context 长什么样"这件事本身——调用点测不到，函数体测得到。
pub(crate) fn deferred_context(paths: AppPaths) -> CoreContext {
    CoreContext::read_only(paths).with_task_saver(Arc::new(StorageTaskBridge))
}

/// The host's single handle on the core. `None` context is impossible by construction: a
/// non-writer still gets a read-only `CoreContext`, so callers never branch on "did the
/// core come up" — only on `is_writer()`.
pub(crate) struct CoreGateway {
    context: Arc<CoreContext>,
    mode: CoreMode,
    /// Kept because `CoreContext` exposes no data-directory getter: the refusal messages
    /// must name the directory this instance will *not* write.
    data_dir: PathBuf,
    /// One start-up notice core itself produced (a cloud-synced root, ADR-010 decision 1).
    location_warning: Option<String>,
    /// 业务面归谁（P8-05 Action 2）。`External` 时本进程不取租约、不起后台服务。
    placement: CorePlacement,
    /// `External` 且连上了宿主的连接句柄；`None` 表示"外部可用但这次没连上"。
    ///
    /// 保持连接打开而不是连完就丢：握手是每条连接一次的（`mark_authenticated` 拒绝
    /// 二次握手），所以下一切片把 domain 命令转发到宿主时复用这条连接，而不是每次
    /// 重新握一遍。
    /// 宿主连接。用 `tokio::sync::Mutex` 而不是 `std`：请求-应答是异步的，而
    /// `std::sync::MutexGuard` **不能跨 `.await`**（那会让整个 command future 变成非 Send，
    /// Tauri 直接编译不过）。这一条在切片六接线时被编译器当场逮住。
    host: tokio::sync::Mutex<Option<HostSession>>,
    /// `External` 判决的依据（诊断与 UI 要用）。
    host_status: Option<String>,
}

/// 外部宿主的会话：一条已认证的连接。放在 `Mutex` 里是因为 `CoreGateway` 在 managed
/// state 里被共享，而 `HostClient` 需要 `&mut` 做请求-应答。
#[allow(
    dead_code,
    reason = "同上：连接由下一切片消费；`spawned` 供 UI 区分『我起的』与『本来就在』"
)]
pub(crate) struct HostSession {
    pub(crate) client: crate::host_client::HostClient,
    pub(crate) spawned: bool,
}

impl CoreGateway {
    /// Takes the writer role where ADR-021 allows it and the lease is free; otherwise
    /// starts as a reader and records why.
    pub(crate) fn startup(paths: AppPaths, version: &str) -> Self {
        Self::startup_with_probe(paths, version, &PlacementProbeImpl::default())
    }

    /// 与 [`CoreGateway::startup`] 同一条路，但探活可注入。
    ///
    /// 判决必须在**取租约之前**做完：`External` 的整个意思就是"别去取"
    /// （取了就是第二个写者，而租约层只会让后到者只读，两件事的后果不同）。
    pub(crate) fn startup_with_probe(
        paths: AppPaths,
        version: &str,
        probe: &dyn PlacementProbe,
    ) -> Self {
        let data_dir = paths.data_dir().to_path_buf();
        let availability = probe.availability(&data_dir);
        let placement = decide_placement(writer_handover_allowed(), availability);
        if placement == CorePlacement::External {
            return Self::external(paths, probe, availability);
        }
        Self::embedded(paths, version, &data_dir)
    }

    /// 外置：**不取租约、不起后台服务**，只连宿主；连不上就如实只读。
    fn external(
        paths: AppPaths,
        probe: &dyn PlacementProbe,
        availability: HostAvailability,
    ) -> Self {
        let data_dir = paths.data_dir().to_path_buf();
        let mut session = None;
        let status: Option<String>;
        match probe.attach(&data_dir) {
            Ok((client, spawned)) => {
                session = Some(HostSession { client, spawned });
                status = Some(if spawned {
                    "外部宿主：本进程连上了刚启动的 host".to_string()
                } else {
                    "外部宿主：本进程已连上正在运行的 host".to_string()
                });
            }
            Err(error) => {
                // 连不上就只读：**不回退成内嵌写者**（那会变成两个写者）。
                let reason = format!(
                    "检测到外部宿主但未能连上（{error}）；本进程以只读运行，等宿主可用后重启"
                );
                startup_diagnostics::record(
                    "host-attached",
                    reason.clone(),
                    data_dir.display().to_string(),
                );
                status = Some(reason);
            }
        }
        let _ = availability;
        Self {
            context: Arc::new(CoreContext::read_only(paths)),
            mode: CoreMode::ReadOnlyBusy,
            data_dir,
            location_warning: status.clone(),
            placement: CorePlacement::External,
            host: tokio::sync::Mutex::new(session),
            host_status: status,
        }
    }

    fn embedded(paths: AppPaths, version: &str, data_dir: &std::path::Path) -> Self {
        let data_dir = data_dir.to_path_buf();
        if !writer_handover_allowed() {
            // No lease attempt at all: `CoreContext::read_only` touches nothing. The saver this
            // arm installs is the whole difference between "deferred" and "read-only": a host that
            // has not been handed the writer role still lets core decide, and stores those bytes
            // itself (ADR-022 §12). The two refusal modes below deliberately get no bridge.
            return Self {
                context: Arc::new(deferred_context(paths)),
                mode: CoreMode::HandoverDeferred,
                data_dir,
                location_warning: None,
                placement: CorePlacement::Embedded,
                host: tokio::sync::Mutex::new(None),
                host_status: None,
            };
        }

        let owner = LeaseOwner {
            application: "aeroric".to_string(),
            version: version.to_string(),
        };
        let acquisition = CoreContext::acquire(
            paths.clone(),
            Arc::new(SystemClock) as Arc<dyn Clock>,
            &owner,
            &PlatformLocationProbe,
        );

        match acquisition {
            Ok(context) => {
                let location_warning = context.location_warning();
                Self {
                    context: Arc::new(context),
                    mode: CoreMode::Writer,
                    data_dir,
                    location_warning,
                    placement: CorePlacement::Embedded,
                    host: tokio::sync::Mutex::new(None),
                    host_status: None,
                }
            }
            Err(error) => {
                let mode = decide_mode(
                    true,
                    match error {
                        LeaseError::AlreadyInUse { .. } => Acquisition::HeldByAnother,
                        _ => Acquisition::Refused,
                    },
                );
                // core's own `Display` already names the directory and the holder, so the
                // record carries it verbatim rather than a paraphrase.
                let reason = error.to_string();
                if let Some(component) = mode.diagnostics_component() {
                    // `fallback` is the data directory: nothing was relocated, and the
                    // honest value here is *which* directory this instance will not write.
                    startup_diagnostics::record(
                        component,
                        reason.clone(),
                        data_dir.display().to_string(),
                    );
                }
                Self {
                    context: Arc::new(CoreContext::read_only(paths)),
                    mode,
                    data_dir,
                    location_warning: Some(reason),
                    placement: CorePlacement::Embedded,
                    host: tokio::sync::Mutex::new(None),
                    host_status: None,
                }
            }
        }
    }

    /// The start-up entry point: resolve the user home the way the rest of the host does, then
    /// hand the decision to [`CoreGateway::startup`].
    ///
    /// A missing home is a degradation, not a panic. `CoreContext::read_only` touches nothing on
    /// disk — no lease, no directory creation — so an unusable placeholder path is safe to carry
    /// purely for the message, and every later write still refuses through `require_write`.
    pub(crate) fn at_startup(version: &str) -> Self {
        let Some(home) = crate::platform::home_dir() else {
            let reason = "无法定位用户主目录，原生 core 本次启动只读".to_string();
            startup_diagnostics::record("storage-location", reason.clone(), String::new());
            return Self {
                context: Arc::new(CoreContext::read_only(AppPaths::from_user_home(""))),
                mode: CoreMode::ReadOnlyLocation,
                data_dir: PathBuf::from("~"),
                location_warning: Some(reason),
                placement: CorePlacement::Embedded,
                host: tokio::sync::Mutex::new(None),
                host_status: None,
            };
        };
        Self::startup(AppPaths::from_user_home(home), version)
    }

    pub(crate) fn context(&self) -> &Arc<CoreContext> {
        &self.context
    }

    /// The directory this instance will or will not write. ADR-027's persistent read-only
    /// indicator has to name it, and a refusal that names nothing is not an explanation.
    #[allow(
        dead_code,
        reason = "只读的常驻横幅（ADR-027）与它的测试都要这个名字；今天只有 startup_wiring 用例在读"
    )]
    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn mode(&self) -> CoreMode {
        self.mode
    }

    /// 业务面归谁（P8-05 Action 2）。`lib.rs` 走的是 [`CoreGateway::owns_business_plane`]；
    /// 这个取值器给诊断与用例用。
    #[allow(
        dead_code,
        reason = "产品路径读的是 owns_business_plane()；本取值器供诊断与用例"
    )]
    pub(crate) fn placement(&self) -> CorePlacement {
        self.placement
    }

    /// 本进程是否拥有业务面。后台服务（watcher/用量索引/笔记调度/local router/remote）
    /// 只在为真时启动。
    pub(crate) fn owns_business_plane(&self) -> bool {
        self.placement == CorePlacement::Embedded
    }

    /// 外部宿主连接（`External` 且连上时才有）。
    pub(crate) fn host(&self) -> &tokio::sync::Mutex<Option<HostSession>> {
        &self.host
    }

    /// 测试专用：直接造一个带可用宿主连接的 `External` 网关。
    ///
    /// 走这条路而不是 `startup_with_probe`，是因为用例要**先**起脚本化宿主、**再**连它，
    /// 而 `startup_with_probe` 的探针接口要在构造时同步完成连接（异步测试里做不到）。
    /// 判决本身仍然来自 [`decide_placement`]，所以这里不引入第二套语义。
    #[cfg(test)]
    pub(crate) fn with_host_session_for_test(
        paths: AppPaths,
        client: crate::host_client::HostClient,
    ) -> Self {
        let data_dir = paths.data_dir().to_path_buf();
        Self {
            context: Arc::new(CoreContext::read_only(paths)),
            mode: CoreMode::ReadOnlyBusy,
            data_dir,
            location_warning: None,
            placement: CorePlacement::External,
            host: tokio::sync::Mutex::new(Some(HostSession {
                client,
                spawned: false,
            })),
            host_status: Some("外部宿主：本进程已连上正在运行的 host（测试构造）".to_string()),
        }
    }

    /// `External` 判决的来由（诊断/UI 文案）。
    pub(crate) fn host_status(&self) -> Option<&str> {
        self.host_status.as_deref()
    }

    #[allow(
        dead_code,
        reason = "第一个经 core 的写入口就是它的调用方；今天 mutation 仍全走 storage.rs"
    )]
    pub(crate) fn is_writer(&self) -> bool {
        self.mode.is_writer()
    }

    /// What the host owes the user at start-up: the reason we are not writing, and/or the
    /// cloud-sync caveat. `None` means there is genuinely nothing to report.
    #[allow(
        dead_code,
        reason = "与 ADR-010 的云端目录告知同一个出口，接进 UI 时与 mode() 一起用"
    )]
    pub(crate) fn notice(&self) -> Option<&str> {
        self.location_warning.as_deref()
    }

    /// The single gate every mutation in the adapter passes through.
    ///
    /// It exists so "read-only" is one decision rather than N: a command that forgets to
    /// check still cannot write, because `CoreContext::require_write` inside core refuses
    /// without a lease. This function only turns that into a message the UI can act on.
    #[allow(
        dead_code,
        reason = "真正的兜底是 core 的 require_write，本函数只把理由说给人听；第一个经 core 的写入口即调用方"
    )]
    pub(crate) fn require_writer(&self) -> Result<(), String> {
        if self.is_writer() {
            return Ok(());
        }
        let dir = self.data_dir.display();
        Err(match self.mode {
            CoreMode::ReadOnlyBusy => {
                format!("another Aeroric instance is using {dir}; this window is read-only")
            }
            CoreMode::ReadOnlyLocation => {
                format!("{dir} is not a location Aeroric will write to; this window is read-only")
            }
            CoreMode::HandoverDeferred => {
                format!("the native core does not own writes on this platform yet; {dir}")
            }
            CoreMode::Writer => unreachable!("handled by is_writer()"),
        })
    }
}

/// The host's answer to "which family is this agent", asked twice (ADR-028).
///
/// **Launch** must not be a table here. `claude_gpt55` points at a shell script
/// (`app_settings/builtin.rs:196` → `~/.claude/start-gpt55.sh`), and
/// `app_settings/launch_spec.rs:400-406` infers codex-vs-claude from that script's
/// *contents* at run time — so any hardcoded answer here would override a real user's
/// configuration and change what gets executed. Delegating to the same function `pty.rs`
/// selects on is the only version of "equivalent to today" that is actually true.
///
/// **Session** is a table, because that side is not dynamic: `session.rs:557` states the
/// built-in `claude`/`claude_gpt55` keep Claude-format history, and a custom profile
/// carries an explicit `family` whose legacy fallback is `codex_like` — the same
/// precedence `config_bundles.rs:100-108` uses.
/// The host's answer to "which family is this agent", asked twice (ADR-028).
///
/// **Launch is delegated, never tabulated here.** `claude_gpt55` points at a shell script
/// (`app_settings/builtin.rs:196` → `~/.claude/start-gpt55.sh`), and
/// `app_settings/launch_spec.rs:400-406` infers codex-vs-claude from that script's
/// *contents* at run time. A hardcoded table would override a real user's configuration and
/// change which command line actually runs, so this asks the same function `pty.rs` selects
/// on — the only version of "equivalent to today" that is true.
///
/// **Session is a table**, because that half is not dynamic: `session.rs:557` says the
/// built-in `claude`/`claude_gpt55` keep Claude-format history, and a custom profile
/// carries an explicit `family` whose legacy fallback is `codex_like`.
pub(crate) struct SettingsFamilies;

impl SettingsFamilies {
    pub(crate) fn of(family: crate::app_settings::AgentFamily) -> ProtocolFamily {
        use crate::app_settings::AgentFamily as Host;
        match family {
            Host::Claude => ProtocolFamily::Claude,
            Host::Codex => ProtocolFamily::Codex,
            Host::Dsh => ProtocolFamily::Dsh,
            Host::Omp => ProtocolFamily::Omp,
        }
    }

    /// `AgentFamily` 的反向拼写——喂 core 判定的 `FamilyProfile.family` 字段
    /// （b1.0：判定本体在 `aeroric_core::agents::families`，宿主只剩数据源）。
    fn family_str(family: crate::app_settings::AgentFamily) -> &'static str {
        use crate::app_settings::AgentFamily as Host;
        match family {
            Host::Claude => "claude",
            Host::Codex => "codex",
            Host::Dsh => "dsh",
            Host::Omp => "omp",
        }
    }
}

impl AgentFamilies for SettingsFamilies {
    fn launch_family(&self, agent: &str) -> ProtocolFamily {
        let spec = crate::app_settings::get_agent_launch_spec(agent);
        aeroric_core::agents::families::launch_family(
            aeroric_core::agents::families::FamilyProfile {
                family: Self::family_str(spec.family),
                codex_like: spec.codex_like,
            },
        )
    }

    fn session_family(&self, agent: &str) -> ProtocolFamily {
        use aeroric_core::agents::families as f;
        if let Some(built) = f::built_in_session_family(agent) {
            return built;
        }
        match crate::app_settings::load_settings_internal()
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
        {
            Some(profile) => f::custom_session_family(f::FamilyProfile {
                family: &profile.family,
                codex_like: profile.codex_like,
            }),
            None => f::unknown_session_family(agent),
        }
    }

    fn is_builtin(&self, agent: &str) -> bool {
        aeroric_core::agents::families::is_builtin(agent)
    }

    /// ADR-029. `claude_gpt55` is Claude in *format* but codex in *capability*: the client
    /// compares `agentOption("claude_gpt55").family` (= codex) against the record's family, so
    /// a `claude` task switches to it by handoff rather than by `--resume`. 判定表在 core
    /// （b1.0）；custom profile 落 `session_family`（能力与格式一致）。
    fn native_resume_family(&self, agent: &str) -> ProtocolFamily {
        aeroric_core::agents::families::built_in_native_resume_family(agent)
            .unwrap_or_else(|| self.session_family(agent))
    }
}
#[cfg(test)]
mod family_tests {
    use super::SettingsFamilies;
    use aeroric_core::tasks::{AgentFamilies, ProtocolFamily};

    /// The half of ADR-028 that lives in the host: one built-in, two answers. `launch` is
    /// deliberately not exercised here — resolving it loads the user's real settings and may
    /// read their start script from disk, which a unit test has no business doing. That
    /// path is covered by delegating to the very function `pty.rs` selects on.
    #[test]
    fn claude_gpt55_keeps_claude_sessions_even_though_it_is_not_launched_like_claude() {
        for (agent, expected) in [
            ("claude", ProtocolFamily::Claude),
            ("claude_gpt55", ProtocolFamily::Claude),
            ("codex", ProtocolFamily::Codex),
            ("dsh", ProtocolFamily::Dsh),
            ("omp", ProtocolFamily::Omp),
        ] {
            assert_eq!(
                SettingsFamilies.session_family(agent),
                expected,
                "built-in {agent}"
            );
            assert!(
                SettingsFamilies.is_builtin(agent),
                "{agent} must be built-in"
            );
        }
        assert!(!SettingsFamilies.is_builtin("my-codex"));
    }
}

/// The adapter's announcement channel (ADR-023): core decides, this emits.
///
/// Deliberately thin. It carries **no** suppression gate: `is_task_active` was applied at some
/// of the 28 legacy sites and not others, and ADR-034 resolved that by moving the rule into the
/// observation channel (one core rule over "does this task still have a live run"), not into the
/// emitter. This type only knows how to spell a payload; `EventOutcome::notification_for` is the
/// only thing that decides whether to spell it.
pub(crate) struct TauriEventSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

/// 切片一百零四：事件泵的窗口出口。泵已经把 v1 topic 翻译成 legacy 通道名
/// （点号是非法 Tauri 窗口事件名），这里只做一次 `emit`——发不出去也不能让
/// 泵停下（消费者可能不在线，事件已由宿主 EventLog 留底）。
struct WindowEventSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> crate::host_event_pump::EventSink for WindowEventSink<R> {
    fn emit(&self, channel: &str, payload: &serde_json::Value) {
        use tauri::Emitter;
        let _ = self.app.emit(channel, payload);
    }

    fn skipped(&self, event: &str) {
        startup_diagnostics::record(
            "event-pump",
            format!("事件泵丢弃无法投递的事件：{event}"),
            String::new(),
        );
    }
}

impl<R: tauri::Runtime> TauriEventSink<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: tauri::Runtime> TaskEventSink for TauriEventSink<R> {
    fn notify(&self, notification: TaskNotification) {
        use tauri::Emitter;
        // 切片一百零五：载荷构造上收 core（宿主 EventLog 与本窗口共用一份）。
        match &notification {
            TaskNotification::Status { .. } => {
                let _ = self.app.emit("task-status", notification.window_payload());
            }
            TaskNotification::Session(_) => {
                let _ = self.app.emit("task-session", notification.window_payload());
            }
        }
    }
}

/// The runtime -> core observation channel (ADR-033 §3): the single door an outside fact about
/// a task walks through on its way to the reducer, and the reason `TauriEventSink` can be the
/// only emitter (ADR-023).
///
/// Four things get decided here rather than at a call site, each with an ADR behind it:
///
/// * where the row lives - `TaskService::project_of`, because report sites carry a task id only;
/// * whether the observation is admitted at all - core's guards (ADR-034);
/// * what gets announced - `EventOutcome::notification_for` (ADR-035: admitted means emitted,
///   even when nothing changed; only a drop is silent);
/// * whether anything is written - never from here, see the note inside.
///
/// A task this process cannot route still announces itself through
/// `TaskEvent::notification_when_unrecorded`, because ADR-033 §4 forbids turning a handover into
/// silence; that path says so in the log instead of pretending the reducer saw it.
pub(crate) struct CoreRuntimeObserver<R: tauri::Runtime> {
    service: Arc<TaskService>,
    sink: Arc<TauriEventSink<R>>,
    /// Fixed at start-up, when the lease was won or lost: the write half of every
    /// observation routes on it. ADR-021 makes this a platform fact before any event
    /// exists, so it cannot change under a running observer.
    mode: CoreMode,
}

impl<R: tauri::Runtime> CoreRuntimeObserver<R> {
    pub(crate) fn new(
        service: Arc<TaskService>,
        sink: Arc<TauriEventSink<R>>,
        mode: CoreMode,
    ) -> Self {
        Self {
            service,
            sink,
            mode,
        }
    }
}

impl<R: tauri::Runtime> TaskRuntimeObserver for CoreRuntimeObserver<R> {
    fn observe(&self, event: TaskEvent) {
        let task_id = event.task_id().to_string();
        let project = match self.service.project_of(&task_id) {
            Ok(project) => project,
            Err(error) => {
                // A corrupt or unreadable table is not an answer about *this* task, so it takes
                // the same no-row path as a missing row rather than silently swallowing a fact.
                eprintln!("[aeroric] core observer could not resolve {task_id}: {error}");
                None
            }
        };
        let notification = match project {
            Some(project_id) => {
                // The write half routes on the start-up mode, not on `is_writer()` alone:
                // ADR-021's deferred platform hands the decision to core and the *bytes* to
                // the bridge, while ADR-027's refused instance gets a decision and an
                // announcement with no write anywhere — one reducer, three destinations.
                let decided = match self.mode {
                    CoreMode::Writer => self.service.observe(&project_id, event.clone()),
                    CoreMode::HandoverDeferred => {
                        self.service.observe_via_host(&project_id, event.clone())
                    }
                    CoreMode::ReadOnlyBusy | CoreMode::ReadOnlyLocation => {
                        self.service.decide_observation(&project_id, event.clone())
                    }
                };
                match decided {
                    Ok((outcome, _)) => outcome.notification_for(&task_id),
                    Err(error) => {
                        eprintln!(
                            "[aeroric] core observer refused an event for {task_id}: {error}"
                        );
                        None
                    }
                }
            }
            None => event.notification_when_unrecorded(),
        };
        if let Some(notification) = notification {
            self.sink.notify(notification);
        }
    }

    fn report_session(&self, facts: TaskSessionFacts) {
        // No reducer step to run: the six legacy sites announced the path the host had just
        // resolved, and announcing it here keeps that behavior while collapsing the emitter
        // surface to one line. The record's session fields stay the client's until the store
        // cutover (see the port doc).
        self.sink.notify(TaskNotification::Session(facts));
    }
}

/// The door the six `task-session` report sites use. It takes the same path as
/// `report_task_status` — one observer, one sink — because ADR-023's criterion is that each
/// family has exactly one emitter, not that only statuses go through core.
pub(crate) fn report_task_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    facts: TaskSessionFacts,
) {
    use tauri::Manager;
    match app.try_state::<Arc<dyn TaskRuntimeObserver>>() {
        Some(observer) => observer.report_session(facts),
        None => TauriEventSink::new(app.clone()).notify(TaskNotification::Session(facts)),
    }
}

/// The door every legacy report site walks through on its way to core (ADR-023, ADR-033).
///
/// Two decisions are made here rather than at the 22 call sites, and both have a reason:
///
/// * **Which generation reports.** The transcript watchers are task-scoped, not run-scoped —
///   they outlive the process whose transcript they read. Asking "which run is this line from?"
///   is unanswerable there, so the observation carries *the newest registered generation*
///   (ADR-025's addressing rule), which is what the site's own status word always described:
///   what this task is doing right now. Falling back to `UNMINTED` keeps a run core never
///   numbered observable instead of silently out of range.
/// * **What happens when core is not running.** `install_core_services` can fail (a corrupt
///   ledger) and the app still starts, by design. Then the announcement falls back to the same
///   `TauriEventSink`, so no report site ever goes quiet — ADR-033 §4 forbids the silence, and
///   keeping the emit inside the sink means "one emitter" stays literally true even in the
///   degraded mode.
pub(crate) fn report_task_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    status: &str,
) {
    report_task_status_with_reason(app, task_id, status, None);
}

/// A status a *watcher* read rather than a change the host made: transcript lines and hook
/// events. These are the reports ADR-034's late-observation guard exists for, and passing
/// anything else here would be the moment the guard quietly stops applying.
pub(crate) fn report_observed_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    status: &str,
    origin: ObservationOrigin,
) {
    report_status(app, task_id, status, None, origin);
}

/// The same report with the reason the host already had. Sites like `finalize_task_exit` word
/// the reason themselves ("Process exited with code 1", "WSL process exited with code 7"), and
/// that text is user-visible today — so it travels in the event and core passes it through
/// rather than synthesizing its own wording.
pub(crate) fn report_task_status_with_reason<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    status: &str,
    reason: Option<&str>,
) {
    report_status(app, task_id, status, reason, ObservationOrigin::Runtime);
}

fn report_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    status: &str,
    reason: Option<&str>,
    origin: ObservationOrigin,
) {
    use tauri::Manager;
    let Ok(parsed) = serde_json::from_value::<TaskStatus>(serde_json::json!(status)) else {
        // A status word outside the frozen nine is a bug in the caller, not a new spelling.
        eprintln!("[aeroric] report_task_status got unknown status {status:?} for {task_id}");
        return;
    };
    let generation = app
        .state::<crate::TaskManager>()
        .runs
        .resource_key(task_id)
        .map(|run| run.generation())
        .unwrap_or(crate::run_registry::RunKey::UNMINTED);
    let event = TaskEvent::Status {
        task_id: task_id.to_string(),
        generation,
        status: parsed,
        attention_requested_at_ms: None,
        failure_reason: reason.map(str::to_string),
        origin,
    };
    match app.try_state::<Arc<dyn TaskRuntimeObserver>>() {
        Some(observer) => observer.observe(event),
        None => TauriEventSink::new(app.clone()).notify(match reason {
            Some(reason) => TaskNotification::status_with_reason(task_id, parsed, reason),
            None => TaskNotification::status(task_id, parsed),
        }),
    }
}

/// ADR-036: the host's answer to "is any run of this task still alive".
///
/// The answer is `is_task_active`'s *body*, not just the run registry: that gate also counts a
/// codex session entry, and a claude entry that is not a lazy-attach placeholder. Answering with
/// `TaskManager::has_live_run` alone would be narrower than the thing being replaced, and the
/// failure is the silent kind — the transcript watchers would start dropping statuses that ship
/// today. Nothing is copied into core: a mirror would fail by staying alive forever if a single
/// retire path were missed, and the failure would be silent (the gate stops suppressing).
///
/// It must not call back into core, and `has_live_run` only takes the registry's leaf lock, so
/// asking cannot extend a lock chain.
struct HostLiveness<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> LiveRuns for HostLiveness<R> {
    fn has_live_run(&self, task_id: &str) -> bool {
        crate::session::is_task_active(&self.app, task_id)
    }
}

/// Puts the adapter's shared state in front of the app, from `lib.rs`'s setup hook.
///
/// Three rules shape this function, and none of them are stylistic:
///
/// 1. **The gateway is built before anything is managed**, because the executor it hands the
///    service needs an `AppHandle` that only exists by then, and the service needs the *same*
///    `Arc<CoreContext>` the gateway resolved — a second context would be a second answer to
///    "may this process write" (ADR-021).
/// 2. **A failure here must not stop the app from starting.** If the ledger cannot be loaded,
///    the honest outcome is "no core service this run", recorded as a degradation — the same
///    posture `DbxState::new_blocking` takes. Refusing to boot over a corrupt
///    core 独占的账本文件坏了，就 strand 用户整个工作区，而那个文件之后是
///    release can repair.
/// 3. **Managing the service is not the writer switch.** `storage.rs` still saves the task
///    table; nothing routes a *mutation* through here yet. What becomes real is the lease, the
///    read-only refusal, and — since ADR-033/035 — the observation channel: the managed
///    `TaskRuntimeObserver` decides and announces without ever writing, which is exactly why the
///    announcement handover and the store cutover can land separately.
///
/// 返回本进程是否拥有业务面，调用方据此决定起不起后台服务（P8-05 Action 2）。
///
/// 返回 bool 而不是让调用方去读 managed state，是为了让「忘了判」在类型上就不可能：
/// 不起服务的那个分支必须在同一个表达式里写出来。
pub(crate) fn install_core_services<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    version: &str,
) -> bool {
    use aeroric_core::tasks::TaskExecutor;
    use tauri::Manager;

    let gateway = CoreGateway::at_startup(version);
    let context = gateway.context().clone();
    let mode = gateway.mode();
    let owns_business_plane = gateway.owns_business_plane();
    // 切片一百零四：External 模式拉起事件泵（自有第二条连接，不占转发锁）。
    // Embedded 的窗口事件由本进程各发射点直接发，不需要泵。
    if gateway.placement() == CorePlacement::External {
        let data_dir = gateway.data_dir().to_path_buf();
        let sink: std::sync::Arc<dyn crate::host_event_pump::EventSink> =
            std::sync::Arc::new(WindowEventSink { app: app.clone() });
        tauri::async_runtime::spawn(async move {
            match crate::host_client::HostClient::connect(&data_dir, "event-pump").await {
                Ok(client) => {
                    let source =
                        crate::host_event_pump::HostPumpSource::connected(client, &data_dir);
                    crate::host_event_pump::run(source, sink).await
                }
                Err(error) => {
                    // 泵连不上不阻塞启动：宿主可能还没起，窗口事件断路如实记录。
                    startup_diagnostics::record(
                        "event-pump",
                        format!("事件泵未能连接宿主：{error}"),
                        data_dir.display().to_string(),
                    );
                }
            }
        });
    }
    app.manage(gateway);

    let mailbox = Arc::new(crate::legacy_task_executor::OutputSinkMailbox::new());
    app.manage(Arc::clone(&mailbox));

    let executor: Arc<dyn TaskExecutor> = Arc::new(
        crate::legacy_task_executor::LegacyTaskExecutor::new(app.clone(), mailbox),
    );
    let live_runs: Arc<dyn LiveRuns> = Arc::new(HostLiveness { app: app.clone() });
    match TaskService::new(
        context,
        Arc::new(SettingsFamilies) as Arc<dyn AgentFamilies>,
        executor,
        live_runs,
    ) {
        Ok(service) => {
            // Arc, not the bare service: `CoreRuntimeObserver` has to share it, and a second
            // `TaskService` would be a second ledger over the same directory.
            let service = Arc::new(service);
            let observer: Arc<dyn TaskRuntimeObserver> = Arc::new(CoreRuntimeObserver::new(
                Arc::clone(&service),
                Arc::new(TauriEventSink::new(app.clone())),
                mode,
            ));
            app.manage(service);
            app.manage(observer);
        }
        Err(error) => startup_diagnostics::record(
            "core-service",
            format!("原生 core 本次启动不可用：{error}"),
            String::new(),
        ),
    }
    owns_business_plane
}

/// The frozen nine spellings, written out rather than round-tripped through serde: a payload
/// constructor should not need an `expect` to obtain a string that is total by construction.
/// `the_event_words_match_their_serialized_forms` pins this list against the contract's own
/// serialisation, so a new variant cannot drift silently.
#[allow(
    dead_code,
    reason = "载荷构造器随 TauriEventSink 一起接进产品路径；今天由三条形状用例与 TS 侧载荷契约测试守住"
)]
const fn status_word(status: aeroric_core::tasks::TaskStatus) -> &'static str {
    use aeroric_core::tasks::TaskStatus;
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::InputRequired => "input_required",
        TaskStatus::Detached => "detached",
        TaskStatus::Interrupted => "interrupted",
        TaskStatus::Done => "done",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

#[allow(dead_code, reason = "同上")]
const fn family_word(family: aeroric_core::tasks::ProtocolFamily) -> &'static str {
    use aeroric_core::tasks::ProtocolFamily;
    match family {
        ProtocolFamily::Claude => "claude",
        ProtocolFamily::Codex => "codex",
        ProtocolFamily::Dsh => "dsh",
        ProtocolFamily::Omp => "omp",
    }
}

/// Builds the `task-status` payload.
///
/// The key set and spellings are the shipping client's, not a redesign:
/// `src/state/app/useAppTauriEvents.ts` reads `task_id`, `status` and an *optional*
/// `failure_reason`, and the status strings are the frozen nine (`input_required`, not
/// `inputRequired`). `src/test/native-core-adapter.test.ts` re-derives all of it from both
/// sources at gate time, because ADR-023 promises this payload does not change.
///
/// `failure_reason` is added only when present, exactly as the shipping `failed` emits do: a
/// `running` event carries two keys today, so sending `"failure_reason": null` would be a new
/// key on a wire the frontend already parses — and omitting the key on the failed path would
/// take away the text the UI explains the failure with.
#[allow(
    dead_code,
    reason = "随 TauriEventSink 接线；键集与拼写由 core_adapter_tests 与 src/test/native-core-adapter.test.ts 双侧钉住"
)]
#[cfg(test)]
mod sink_tests {
    // 切片一百零五：载荷构造与字词 pin 随实现上收 core `tasks::events`
    // （payload_tests）；这里只保留 wire 形状对账——Tauri 发射面发出的
    // JSON 与 core 构造逐字节同形，防两侧漂移。

    #[test]
    fn the_tauri_sink_shape_is_the_core_shape() {
        use aeroric_core::tasks::{TaskNotification, TaskStatus};
        assert_eq!(
            TaskNotification::status("t1", TaskStatus::InputRequired).window_payload(),
            serde_json::json!({"task_id": "t1", "status": "input_required"})
        );
        assert_eq!(
            TaskNotification::status("t1", TaskStatus::Running).window_payload(),
            serde_json::json!({"task_id": "t1", "status": "running"})
        );
    }
}

/// 一次经 core 的任务写的结果，同时就是前端该采纳的权威快照（Action 3 的"React 接受
/// authoritative snapshot"那一半从这条返回开始）。
///
/// 它原来叫 `TaskIntentOutcome`，只有意图入口用它。启动入口（`launch_via_core`）返回的是同一个
/// 四元组——"这批行、这格修订、有没有变、谁落的盘"——再起一个字段一模一样的结构体就是让
/// "谁写了什么"有两种拼写。TS 侧的接口名不参与序列化，线上形状一字未改。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskWriteOutcome {
    /// 这个项目决定后的全部任务记录。
    pub tasks: Vec<serde_json::Value>,
    /// `None` 意味着这一格没被 advance：要么没变化，要么本平台还没递交写权。
    pub revision: Option<String>,
    pub changed: bool,
    /// 谁落的盘：`true` = core；`false` = 宿主按 core 的决定原样写回。
    pub committed_by_core: bool,
}

fn records_as_values(records: &[aeroric_core::storage::Record]) -> Vec<serde_json::Value> {
    records
        .iter()
        .map(|record| serde_json::Value::Object(record.0.clone()))
        .collect()
}

/// ADR-022 决策 1/2 的宿主入口：前端交一条意图，不再交整表快照。
///
/// 三种情况必须分开，不能只看 `is_writer()`：那样会把"本平台还没递交写权"和"租约在另一个活
/// 实例手里"当成同一件事，而这两个问题对"这个实例能不能写盘"的答案是相反的。
/// - `Writer`：core 提交，覆盖基线按 ADR-015 由它校验。
/// - `HandoverDeferred`（ADR-021）：core 决定并提交给 [`StorageTaskBridge`]——同一个 reducer、
///   同一套 generation 戳记，只是字节由宿主落。这条臂以前自己 `decide_intent` + 转发，与桥是
///   同一件事的两扇门，收敛以后只剩一扇。
/// - `ReadOnlyBusy` / `ReadOnlyLocation`（ADR-027 / ADR-010）：**一个字节都不许写**。这条若放行，
///   "第二个实例只读"就只剩 UI 上的说法。
pub(crate) fn apply_task_intent_core<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    project_id: &str,
    intent: &aeroric_core::tasks::TaskIntent,
) -> Result<TaskWriteOutcome, String> {
    use tauri::Manager;

    let service = app.state::<Arc<TaskService>>();
    let mode = app.state::<CoreGateway>().mode();

    let applied = match mode {
        CoreMode::Writer => service
            .apply_intent(project_id, intent)
            .map_err(|error| error.to_string())?,
        CoreMode::HandoverDeferred => service
            .apply_intent_via_host(project_id, intent)
            .map_err(|error| error.to_string())?,
        CoreMode::ReadOnlyBusy | CoreMode::ReadOnlyLocation => {
            return Err(
                "Aeroric 本次启动是只读的（数据目录被另一个实例持有，或本机不接受它），改动没有保存"
                    .to_string(),
            );
        }
    };

    Ok(TaskWriteOutcome {
        tasks: records_as_values(&applied.records),
        revision: applied.revision,
        changed: applied.changed,
        committed_by_core: mode == CoreMode::Writer,
    })
}

#[tauri::command]
pub(crate) fn apply_task_intent(
    app: tauri::AppHandle,
    project_id: String,
    intent: aeroric_core::tasks::TaskIntent,
) -> Result<TaskWriteOutcome, String> {
    apply_task_intent_core(&app, &project_id, &intent)
}

/// `run_task` 那批命令今天只带得出 `task_id`，而 core 的每个执行入口都按**项目**寻址。
///
/// 答案从盘上来，不从参数来：`project_of` 就是为"站点只有 task_id"造的（ADR-033 §4），拿到的
/// 项目 id 再去 `projects.json` 换成记录。命令参数里那条 `projectPath` 不作数——一份数据只能有
/// 一个真相源（ADR-027），而那条路径连"任务在哪个项目"都回答不了。
fn project_record_for(
    service: &TaskService,
    task_id: &str,
) -> Result<aeroric_core::storage::Record, String> {
    let project_id = service
        .project_of(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("查不到任务 {task_id} 属于哪个项目，所以无法经 core 启动它"))?;
    aeroric_core::storage::projects::load_projects(service.context())
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|record| record.id() == Some(project_id.as_str()))
        .ok_or_else(|| format!("项目 {project_id} 不在 projects.json 里，任务 {task_id} 启动不了"))
}

/// §12 步骤②b 的启动入口：**解析项目 → 寄存 channel → 按 `mode()` 三分递交**。
///
/// **今天还没有生产调用方**。七条命令体要等 ③④ 那一刀才换：#7 的整表快照还在写，先换体就等于让
/// 一次 launch 之后的第一条状态观察被 ADR-015 的覆盖守卫拒掉（`fingerprint.rs` 的 `LastCoreWrite`
/// 只认 core 上次发布的字节，而宿主的整表保存没有任何告知钩子）。入口先行不是拖延——下面三件事
/// 每一件都会**安静地**变成"进程起来了但没人收字节"或"一个永远取不回来的窗口"，而"安静"正是
/// 这个模块一路被门禁要求消除的那类失败：
///
/// 1. **operation key 由宿主发**。§10 复勘量过：八个启动调用点没有一个发 key，而 ADR-017 要的"这一次
///    点击而不是下一次"的身份必须有，它同时是 mailbox 的键。uuid v4 让同一任务的两次并发启动天然
///    落在两个 key 上。
/// 2. **先寄存再递交**。终端 channel 是 webview 对象，进不了 `LaunchRequest`，executor 只能从
///    mailbox 取；反过来做就把"进程已经起来、字节没人收"变成一种可能的结果。
/// 3. **拒绝要把 channel 取回来**。留在 mailbox 里没人认领的 channel 不是"什么都没发生"，它是那个
///    窗口再也收不到输出的原因——`parked()` 这个计数存在就是为了能发现这种残留。
pub(crate) fn launch_via_core<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    sink: crate::pty::OutputSink,
    persist_before_launch: bool,
    prompt_override: Option<String>,
    inject_prompt_into_terminal: bool,
    attachments: aeroric_core::tasks::Attachments,
    terminal_size: Option<(u16, u16)>,
    dsh_window: Option<aeroric_core::tasks::DshWindowFacts>,
) -> Result<TaskWriteOutcome, String> {
    use tauri::Manager;

    let service = app.state::<Arc<TaskService>>();
    let mailbox = app.state::<Arc<crate::legacy_task_executor::OutputSinkMailbox>>();
    let mode = app.state::<CoreGateway>().mode();
    // 解析不出项目时 channel 还没寄出去，所以这条错误路径不需要归还。
    let project = project_record_for(&service, task_id)?;

    let operation_key = uuid::Uuid::new_v4().to_string();
    mailbox.park(&operation_key, sink);

    let window = aeroric_core::tasks::HostWindowFacts {
        terminal_size,
        dsh: dsh_window,
    };
    let started = match mode {
        CoreMode::Writer => service.start(
            &project,
            task_id,
            &operation_key,
            persist_before_launch,
            prompt_override,
            inject_prompt_into_terminal,
            attachments,
            window,
        ),
        CoreMode::HandoverDeferred => service.start_via_host(
            &project,
            task_id,
            &operation_key,
            persist_before_launch,
            prompt_override,
            inject_prompt_into_terminal,
            attachments,
            window,
        ),
        CoreMode::ReadOnlyBusy | CoreMode::ReadOnlyLocation => {
            mailbox.take(&operation_key);
            return Err(
                "Aeroric 本次启动是只读的（数据目录被另一个实例持有，或本机不接受它），任务没有启动"
                    .to_string(),
            );
        }
    };
    let applied = started.map_err(|error| {
        // core 或 executor 失败了：这条 key 上再不会有人来取 channel，留着它就是残留。
        // （`LegacyTaskExecutor` 成功路径已经 take 过一次，take 是幂等的。）
        mailbox.take(&operation_key);
        error.to_string()
    })?;

    Ok(TaskWriteOutcome {
        tasks: records_as_values(&applied.records),
        revision: applied.revision,
        changed: applied.changed,
        committed_by_core: mode == CoreMode::Writer,
    })
}

/// §12 步骤②c 的续跑入口：与 [`launch_via_core`] 同一个三步形状（解析 → 寄存 → 按 mode 三分），
/// 差别只在 core 那条守卫链必须原样生效——空 session 与"上次被截断还没人看过"（ADR-018）在这里
/// 同样是拒绝，桥不给它们开后门。
///
/// 它今天同样**没有生产调用方**。它现在就该存在、而不是等接的时候现场发明的理由很具体：
/// `run_dsh_task` 的"续跑"就是拿解析出的 sessionId 再调**启动**命令（§12 已按台账把这条更正过），
/// 所以三条 `resume_*` 与那条 dsh 分支各自要调哪一具身体，是这两具身体先立起来才答得出的问题。
///
/// 一具身体与另一具的分工也钉在这里：`resume` **只写状态那一段**。宿主解析出的 session id 不由
/// 这条入口落盘——那是 `ResumeWithSession` 那条复合意图的职责（Q70），拆开投两次就会留下"会话已挂、
/// 状态未改"的中间态。
pub(crate) fn resume_via_core<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    task_id: &str,
    session_id: &str,
    sink: crate::pty::OutputSink,
    persist_before_launch: bool,
    terminal_size: Option<(u16, u16)>,
) -> Result<TaskWriteOutcome, String> {
    use tauri::Manager;

    let service = app.state::<Arc<TaskService>>();
    let mailbox = app.state::<Arc<crate::legacy_task_executor::OutputSinkMailbox>>();
    let mode = app.state::<CoreGateway>().mode();
    let project = project_record_for(&service, task_id)?;

    let operation_key = uuid::Uuid::new_v4().to_string();
    mailbox.park(&operation_key, sink);

    let started = match mode {
        CoreMode::Writer => service.resume(
            &project,
            task_id,
            session_id,
            &operation_key,
            persist_before_launch,
            terminal_size,
        ),
        CoreMode::HandoverDeferred => service.resume_via_host(
            &project,
            task_id,
            session_id,
            &operation_key,
            persist_before_launch,
            terminal_size,
        ),
        CoreMode::ReadOnlyBusy | CoreMode::ReadOnlyLocation => {
            mailbox.take(&operation_key);
            return Err(
                "Aeroric 本次启动是只读的（数据目录被另一个实例持有，或本机不接受它），任务没有续跑"
                    .to_string(),
            );
        }
    };
    let applied = started.map_err(|error| {
        mailbox.take(&operation_key);
        error.to_string()
    })?;

    Ok(TaskWriteOutcome {
        tasks: records_as_values(&applied.records),
        revision: applied.revision,
        changed: applied.changed,
        committed_by_core: mode == CoreMode::Writer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> AppPaths {
        let root = std::env::temp_dir().join(format!(
            "aeroric-gateway-{tag}-{}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        AppPaths::with_data_dir(&root, Some(&root.join("data")))
    }

    /// P8-05 Action 2：判决矩阵。**只有"允许移交 + 已经有活宿主"才外置**。
    #[test]
    fn the_placement_matrix_only_goes_external_for_a_live_host_on_macos() {
        assert_eq!(
            decide_placement(true, HostAvailability::Live),
            CorePlacement::External
        );
        // 没有活宿主就没有外置的理由——"理论上可以起一个"不算（那会让每次启动都变成
        // 行为变更，而 helper 打包还没落地）。
        assert_eq!(
            decide_placement(true, HostAvailability::AbsentButHelperPresent),
            CorePlacement::Embedded
        );
        assert_eq!(
            decide_placement(true, HostAvailability::Absent),
            CorePlacement::Embedded
        );
        // 未移交写权的平台（Windows/Linux）永远内嵌：外置是 macOS 的能力，
        // 它没有"D 盘上的 host helper"这回事。
        for host in [
            HostAvailability::Live,
            HostAvailability::AbsentButHelperPresent,
            HostAvailability::Absent,
        ] {
            assert_eq!(decide_placement(false, host), CorePlacement::Embedded);
        }
    }

    /// 探针端口：判决要问的两件事都能被替身喂进去。
    struct FakePlacementProbe {
        availability: HostAvailability,
        attach: Result<bool, String>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl PlacementProbe for FakePlacementProbe {
        fn availability(&self, _data_dir: &Path) -> HostAvailability {
            self.availability
        }

        fn attach(
            &self,
            data_dir: &Path,
        ) -> Result<(crate::host_client::HostClient, bool), String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match &self.attach {
                // 真连一条：判决要的那个"连得上"必须真发生过，否则本用例只是在测替身。
                Ok(spawned) => {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("runtime");
                    let client = runtime
                        .block_on(async {
                            // 起一个只答握手的 UDS 服务端，然后连它。
                            // 数据目录要自己建：`CoreGateway` 在 External 路径上**碰都不碰**
                            // 盘（这也正是本用例要证明的事之一），所以这里没有别人的副作用可借。
                            std::fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
                            let listener = tokio::net::UnixListener::bind(
                                crate::host_client::host_socket_path(data_dir),
                            )
                            .map_err(|error| error.to_string())?;
                            std::fs::write(
                                crate::host_client::host_token_path(data_dir),
                                "t",
                            )
                            .map_err(|error| error.to_string())?;
                            let handle = tokio::spawn(async move {
                                if let Ok((stream, _)) = listener.accept().await {
                                    let (read, mut write) = stream.into_split();
                                    let mut reader =
                                        tokio::io::BufReader::new(read);
                                    let mut line = String::new();
                                    use tokio::io::AsyncBufReadExt as _;
                                    use tokio::io::AsyncWriteExt as _;
                                    let _ = reader.read_line(&mut line).await;
                                    let reply = aeroric_protocol::envelope::encode_line(
                                        &aeroric_protocol::envelope::Frame::Success(
                                            aeroric_protocol::envelope::Success {
                                                v: aeroric_protocol::PROTOCOL_VERSION,
                                                id: "h".to_string(),
                                                ok: true,
                                                result: serde_json::json!({ "clientId": "tauri", "capabilities": [] }),
                                                extra: Default::default(),
                                            },
                                        ),
                                    )
                                    .expect("encode");
                                    let _ = write.write_all(reply.as_bytes()).await;
                                    let _ = write.flush().await;
                                    // 保持连接开着直到客户端读走应答。
                                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                                }
                            });
                            let client =
                                crate::host_client::HostClient::connect(data_dir, "tauri").await;
                            let _ = handle.await;
                            client.map_err(|error| error.to_string())
                        })?;
                    Ok((client, *spawned))
                }
                Err(reason) => Err(reason.clone()),
            }
        }
    }

    /// 外置用例要真绑一个 UDS socket，而 macOS 的 `$TMPDIR` 逼近 `sun_path` 上限
    /// （276 片案底）⇒ 这几条用 `/tmp` 短根，与 host harness 同约定。
    fn short_root_paths(tag: &str) -> AppPaths {
        let root = std::path::PathBuf::from("/tmp").join(format!(
            "aeg-{tag}-{}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        AppPaths::with_data_dir(&root, Some(&root.join("d")))
    }

    /// External 的整个意思就是**别去取租约**：取了就是第二个写者。
    #[test]
    fn external_placement_never_takes_the_lease() {
        let paths = short_root_paths("place");
        let data_dir = paths.data_dir().to_path_buf();
        let probe = FakePlacementProbe {
            availability: HostAvailability::Live,
            attach: Ok(false),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let gateway = CoreGateway::startup_with_probe(paths, "test", &probe);

        assert_eq!(gateway.placement(), CorePlacement::External);
        assert!(!gateway.owns_business_plane(), "外置时不起任何后台服务");
        assert!(!gateway.is_writer(), "外置时本进程不是写者");
        assert!(
            gateway.context().require_write().is_err(),
            "core 也必须独立拒绝：外置时我们连租约都没有"
        );
        assert!(
            gateway.host().blocking_lock().is_some(),
            "连上了就应当留着连接"
        );
        assert_eq!(probe.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        // 租约文件不该被这份 External 启动创建出来。
        let lease_files = std::fs::read_dir(&data_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.contains("lock") || name.contains("lease"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert!(
            lease_files.is_empty(),
            "External 不该产生租约文件：{lease_files:?}"
        );
        std::fs::remove_dir_all(data_dir.parent().unwrap()).ok();
    }

    /// 外置但连不上：**只读 + 如实报告**，绝不回退成内嵌写者（那会变成两个写者）。
    #[test]
    fn a_failed_external_attach_stays_read_only_instead_of_falling_back() {
        let paths = short_root_paths("fail");
        let data_dir = paths.data_dir().to_path_buf();
        let probe = FakePlacementProbe {
            availability: HostAvailability::Live,
            attach: Err("presented token does not match this host's data directory".to_string()),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let gateway = CoreGateway::startup_with_probe(paths, "test", &probe);

        assert_eq!(gateway.placement(), CorePlacement::External);
        assert!(!gateway.is_writer());
        assert!(gateway.host().blocking_lock().is_none());
        let status = gateway.host_status().unwrap_or_default();
        assert!(status.contains("未能连上"), "{status}");
        assert!(status.contains("只读"), "连不上时要说清后果：{status}");
        std::fs::remove_dir_all(data_dir.parent().unwrap()).ok();
    }

    /// 没有活宿主 ⇒ 内嵌（今天的产品路径），而且**不去连**。
    #[test]
    fn an_absent_host_keeps_the_embedded_plane_and_skips_attaching() {
        let paths = short_root_paths("embedded");
        let data_dir = paths.data_dir().to_path_buf();
        let probe = FakePlacementProbe {
            availability: HostAvailability::Absent,
            attach: Err("must not be called".to_string()),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let gateway = CoreGateway::startup_with_probe(paths, "test", &probe);
        assert_eq!(gateway.placement(), CorePlacement::Embedded);
        assert!(gateway.owns_business_plane());
        assert_eq!(
            probe.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "内嵌路径不该去连宿主（没有活人可连，连一次只是浪费 2s 超时）"
        );
        std::fs::remove_dir_all(data_dir.parent().unwrap()).ok();
    }

    #[test]
    fn the_mode_matrix_is_decidable_without_a_filesystem() {
        // ADR-021 × ADR-027 in one table. The first two rows are the load-bearing pair:
        // a deferred platform must not report a fault, and a lease conflict must not be
        // mistaken for one.
        assert_eq!(
            decide_mode(false, Acquisition::Acquired),
            CoreMode::HandoverDeferred,
            "even a free lease must not make Windows a writer yet"
        );
        assert_eq!(
            decide_mode(false, Acquisition::HeldByAnother),
            CoreMode::HandoverDeferred,
            "and it must not look broken either"
        );
        assert_eq!(decide_mode(true, Acquisition::Acquired), CoreMode::Writer);
        assert_eq!(
            decide_mode(true, Acquisition::HeldByAnother),
            CoreMode::ReadOnlyBusy
        );
        assert_eq!(
            decide_mode(true, Acquisition::Refused),
            CoreMode::ReadOnlyLocation
        );
    }

    #[test]
    fn only_the_two_read_only_faults_are_reported_to_the_user() {
        assert_eq!(CoreMode::Writer.diagnostics_component(), None);
        assert_eq!(CoreMode::HandoverDeferred.diagnostics_component(), None);
        assert_eq!(
            CoreMode::ReadOnlyBusy.diagnostics_component(),
            Some("storage-lease")
        );
        assert_eq!(
            CoreMode::ReadOnlyLocation.diagnostics_component(),
            Some("storage-location")
        );
    }

    #[test]
    fn a_free_temp_directory_makes_this_process_the_writer() {
        // The point of prerequisite ①: the probe must be good enough that a normal local
        // directory actually gets a writer. If this ever goes red, the core has quietly
        // lost the ability to write anywhere.
        let (_root, gateway) = writer_on_temp("acquire");
        assert_eq!(
            gateway.mode(),
            CoreMode::Writer,
            "notice: {:?}",
            gateway.notice()
        );
        assert!(gateway.require_writer().is_ok());
    }

    #[test]
    fn a_second_instance_over_the_same_directory_starts_read_only_and_says_so() {
        // ADR-027 end to end, on a real lease. The first holder must stay writable: if
        // the *second* attempt damaged the first one, this is where it shows.
        let paths = temp_paths("double-open");
        let first = CoreGateway::startup(paths.clone(), "test");
        assert_eq!(first.mode(), CoreMode::Writer);

        let second = CoreGateway::startup(paths.clone(), "test");
        assert_eq!(second.mode(), CoreMode::ReadOnlyBusy);
        let error = second.require_writer().unwrap_err();
        assert!(
            error.contains("read-only"),
            "the refusal must name the mode, got: {error}"
        );
        assert!(
            error.contains(&paths.data_dir().display().to_string()),
            "and the directory it will not write: {error}"
        );
        assert!(second.notice().is_some(), "the reason must be reportable");

        let listed = startup_diagnostics::local_degradations();
        assert!(
            listed.iter().any(|d| d.component == "storage-lease"),
            "the lease conflict must reach the start-up list the frontend reads"
        );

        assert!(
            first.require_writer().is_ok(),
            "losing a race for the lease must not degrade the holder"
        );

        // The lease is released by dropping the holder, not by any API here.
        drop(second);
        drop(first);
        let reopened = CoreGateway::startup(paths, "test");
        assert_eq!(
            reopened.mode(),
            CoreMode::Writer,
            "and a fresh start after both are gone must write again"
        );
    }

    #[test]
    fn read_only_mode_cannot_write_even_if_someone_asked_it_to() {
        // `CoreContext::read_only` has no lease, so core itself is the backstop; the
        // gateway must not claim writer-ness on top of it.
        let paths = temp_paths("read-only");
        let data_dir = paths.data_dir().to_path_buf();
        let gateway = CoreGateway {
            context: Arc::new(CoreContext::read_only(paths)),
            mode: CoreMode::ReadOnlyBusy,
            data_dir,
            location_warning: None,
            placement: CorePlacement::Embedded,
            host: tokio::sync::Mutex::new(None),
            host_status: None,
        };
        assert!(!gateway.is_writer());
        assert!(gateway.require_writer().is_err());
        assert!(
            gateway.context().require_write().is_err(),
            "core must refuse independently of the gateway's own flag"
        );
    }

    /// Each test gets its own throwaway directory — nothing here may touch `~/.aeroric`,
    /// which the shipping app is writing on this very machine.
    fn writer_on_temp(tag: &str) -> (AppPaths, CoreGateway) {
        let paths = temp_paths(tag);
        let gateway = CoreGateway::startup(paths.clone(), "test");
        (paths, gateway)
    }

    #[test]
    fn handover_policy_is_one_function_and_is_not_a_stub() {
        // Guards the ADR-021 gate check: the platform judgement must live in exactly one
        // place. If it ever grows branches elsewhere, this still passes — so the real
        // protection is the `tauri-adapter` gate case, which greps for that.
        assert_eq!(writer_handover_allowed(), cfg!(target_os = "macos"));
    }
}
