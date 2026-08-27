//! 系统权限清单:枚举本应用**可能**需要的系统授权,逐项报告当前状态,并提供
//! 单项获取 / 一键获取 / 打开系统设置三种动作。
//!
//! 为什么需要它:agent 在终端里跑任意命令(截屏、控制 UI、读 Desktop 下的项目),
//! 这些调用继承的是 Aeroric 自己的授权身份。缺权限时系统只会静默失败或弹一次用户
//! 当时可能拒掉的框,之后再没有入口。这里把清单显式化。
//!
//! # 一项权限报两个状态,不是一个
//!
//! macOS 把 TCC 判定**缓存在进程里**:用户在系统设置里打开开关后,本进程再查仍是
//! 旧答案。只报一个状态就会出现「设置里显示已开放、面板刷新仍是未获取」——这正是本
//! 模块要消掉的假阴性。所以每项权限报两个:
//!
//! - `system_status`:系统当前记的账。由**新进程**探测(`fresh_probe`),不受本进程
//!   缓存影响,与用户在系统设置里看到的一致。
//! - `process_status`:本进程实际拿到的能力。决定 agent 现在跑命令会不会失败。
//!
//! 面向用户的 `status` 取 `system_status`(与设置一致);两者不一致时置
//! `restart_required`,面板提示重启,而不是谎报未获取。
//!
//! # 平台差异
//!
//! - **macOS**:TCC 逐项前置授权,可在应用内请求、可跳设置面板。
//! - **Windows**:各项隐私开关记在注册表 ConsentStore,可读、可跳 `ms-settings:`;
//!   非打包(Win32)应用无法在应用内触发授权框,所以只给设置入口。
//! - **Linux**:没有应用级前置授权模型。能力由会话类型(X11 / Wayland + portal)、
//!   用户组和沙箱决定,只能**如实报告**,故这些项标 `report_only`,不摆假按钮。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod other;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as imp;
#[cfg(target_os = "macos")]
use self::macos as imp;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use self::other as imp;
#[cfg(target_os = "windows")]
use self::windows as imp;

/// `main` 的前置分支用的 argv 标志:以新进程身份探测权限并把 JSON 打到 stdout。
pub const PROBE_BRIDGE_FLAG: &str = "--probe-system-permissions";

/// 防止探测子进程再去 spawn 探测子进程(理论上不会,但环境变量比推理便宜)。
const PROBE_GUARD_ENV: &str = "AERORIC_PERMISSION_PROBE";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionStatus {
    /// 已授权。
    Granted,
    /// 未授权:包含"被拒绝"和"从未询问"——两者对用户来说是同一个动作。
    NotGranted,
    /// 无法确定:系统没有公开查询接口,或探测会弹框所以没做。
    Unknown,
}

/// 单项权限的当前快照。`id` 之外的文案全在前端 i18n,后端不返回本地化字符串。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemPermission {
    /// 稳定标识,同时用作前端 i18n key 后缀。
    pub id: String,
    /// 面向用户的结论:优先取系统记的账,与系统设置里看到的一致。
    pub status: PermissionStatus,
    /// 系统当前记的账(新进程探测)。探测不可用时与 `process_status` 相同。
    pub system_status: PermissionStatus,
    /// 本进程实际拿到的能力。决定 agent 现在跑命令会不会失败。
    pub process_status: PermissionStatus,
    /// 系统已授权但本进程还没拿到——重启即生效。面板据此提示重启。
    pub restart_required: bool,
    /// 能否在应用内直接触发系统授权流程(弹框或探测)。
    pub can_request_in_app: bool,
    /// 能否跳转到对应的系统设置面板。
    pub can_open_settings: bool,
    /// 能否清除本应用的授权记录重来(macOS `tccutil reset`)。
    pub can_reset: bool,
    /// 授权变更后必须重启本应用才生效。
    pub needs_restart: bool,
    /// 检测本身会触发系统询问,所以默认不探测(目录类权限)。
    pub probe_prompts: bool,
    /// 本平台没有应用级开关,这一项只报告事实、没有可点的动作(Linux)。
    pub report_only: bool,
    /// 检测失败或状态成因的说明。前端原样展示,不参与判定。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 授权记录不可靠的成因。返回机器码而不是文案,由前端 i18n 展示。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IdentityWarning {
    /// ad-hoc / linker 签名:授权绑当次构建的 cdhash。升级后系统设置里开关看着还在,
    /// 实际已失效——本仓库观察到的「设置显示已开放、应用报未获取」的主因。
    UnstableSignature,
    /// 未以 app bundle 运行(`tauri dev`):授权记在终端 / IDE 身上,与安装版不共享。
    NotBundled,
}

/// 系统按什么身份给本应用记授权。用来解释「设置里明明开了」的假阴性。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    /// 系统记账用的主体(macOS: bundle id / 签名 identifier;Windows: exe 路径)。
    pub subject: String,
    /// 代码签名种类:`developer-id` / `adhoc` / `unsigned` / `not-applicable`。
    pub signature: String,
    /// 授权能否跨版本升级保留。
    pub stable_across_updates: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<IdentityWarning>,
}

impl AppIdentity {
    /// 没有代码签名概念的平台(Windows / Linux)用这个。
    // 只被非 macOS 的平台实现调用,编 macOS 时看着像死代码。
    #[allow(dead_code)]
    pub(crate) fn not_applicable(subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            signature: "not-applicable".to_string(),
            stable_across_updates: true,
            warning: None,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemPermissionReport {
    pub platform: String,
    /// 本平台是否有可枚举的权限清单。false 时 `permissions` 为空。
    pub supported: bool,
    pub permissions: Vec<SystemPermission>,
    pub identity: AppIdentity,
    /// 新进程探测是否可用。false 时 `system_status` 退化为 `process_status`。
    pub fresh_probe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fresh_probe_error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GrantAllResult {
    pub report: SystemPermissionReport,
    /// 已在应用内发起过授权流程的权限 id。
    pub requested: Vec<String>,
    /// 只能去系统设置里手工勾选、且当前仍未授权的权限 id。
    pub manual: Vec<String>,
}

/// 权限的静态描述。平台实现提供这张表,通用逻辑只按表驱动。
pub(crate) struct PermissionDescriptor {
    pub(crate) id: &'static str,
    pub(crate) can_request_in_app: bool,
    pub(crate) can_open_settings: bool,
    /// 能否清除授权记录重来。只有 macOS 的 `tccutil reset` 提供了这个能力。
    pub(crate) can_reset: bool,
    pub(crate) needs_restart: bool,
    pub(crate) probe_prompts: bool,
    /// 只报告、无动作(Linux 的会话 / 用户组类能力)。
    pub(crate) report_only: bool,
}

// 这两个构造子分别只被 linux.rs / windows.rs 用到,编另一个平台时看着像死代码。
#[allow(dead_code)]
impl PermissionDescriptor {
    /// 只报告的项目不需要动作按钮;其余项目至少要能请求或能跳设置。
    pub(crate) const fn report_only(id: &'static str) -> Self {
        Self {
            id,
            can_request_in_app: false,
            can_open_settings: false,
            can_reset: false,
            needs_restart: false,
            probe_prompts: false,
            report_only: true,
        }
    }

    /// 只能去系统设置里手工勾选的项目(Windows 的隐私开关、macOS 的 FDA 等)。
    pub(crate) const fn settings_only(id: &'static str, needs_restart: bool) -> Self {
        Self {
            id,
            can_request_in_app: false,
            can_open_settings: true,
            can_reset: false,
            needs_restart,
            probe_prompts: false,
            report_only: false,
        }
    }
}

/// 一次检测的结果:状态 + 可选的说明。
#[derive(Clone, Debug)]
pub(crate) struct PermissionProbe {
    pub(crate) status: PermissionStatus,
    pub(crate) detail: Option<String>,
}

impl PermissionProbe {
    pub(crate) fn plain(status: PermissionStatus) -> Self {
        Self {
            status,
            detail: None,
        }
    }

    /// 状态确定但成因值得解释(Linux 的「X11 会话本就不设限」等)。
    // 只被 linux.rs / windows.rs 用到,编 macOS 时看着像死代码。
    #[allow(dead_code)]
    pub(crate) fn explained(status: PermissionStatus, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: Some(detail.into()),
        }
    }

    pub(crate) fn unknown(detail: impl Into<String>) -> Self {
        Self {
            status: PermissionStatus::Unknown,
            detail: Some(detail.into()),
        }
    }
}

/// 探测子进程与父进程之间的线格式。故意只带这两个字段,便于跨版本容错。
#[derive(Serialize, Deserialize, Clone, Debug)]
struct ProbeRecord {
    status: PermissionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

// ── 会话内探测缓存 ───────────────────────────────────────────────────────────

/// 目录类权限的检测本身会弹系统询问,所以列表刷新时不能重探;但用户点过「检测」拿到
/// 的结果不该被下一次刷新抹回"未知"。这里按会话记住已探得的结论。
fn probe_cache() -> &'static Mutex<HashMap<String, PermissionProbe>> {
    static CACHE: OnceLock<Mutex<HashMap<String, PermissionProbe>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_probe(id: &str) -> Option<PermissionProbe> {
    probe_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(id).cloned())
}

fn remember_probe(id: &str, probe: &PermissionProbe) {
    // Unknown 不值得记:它没有回答任何问题,记下来只会挡住下一次真探测。
    if probe.status == PermissionStatus::Unknown {
        return;
    }
    if let Ok(mut cache) = probe_cache().lock() {
        cache.insert(id.to_string(), probe.clone());
    }
}

// ── 新进程探测 ───────────────────────────────────────────────────────────────

/// 以**新进程**身份重跑一遍检测,拿到不受本进程缓存影响的系统当前状态。
///
/// 为什么必须换进程:macOS 把 TCC 判定缓存在进程里,用户在系统设置里开了开关之后,
/// 本进程再查还是旧答案。子进程由本应用拉起,TCC 的 responsible process 仍是本应用
/// bundle,所以问到的是同一个主体的授权,只是没有那份缓存。
fn fresh_probe(ids: &[&str]) -> Result<HashMap<String, PermissionProbe>, String> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    if std::env::var_os(PROBE_GUARD_ENV).is_some() {
        return Err("refusing to nest permission probe processes".to_string());
    }
    let exe = std::env::current_exe()
        .map_err(|error| format!("Cannot locate own executable: {error}"))?;

    let mut command = std::process::Command::new(exe);
    command
        .arg(PROBE_BRIDGE_FLAG)
        .arg(ids.join(","))
        .env(PROBE_GUARD_ENV, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::subprocess::configure_background_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Cannot start probe process: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Probe process has no stdout".to_string())?;

    // stdout 交给读线程,子进程句柄留在本线程——否则超时时没人能杀它。
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buffer = String::new();
        let mut stdout = stdout;
        let _ = stdout.read_to_string(&mut buffer);
        let _ = sender.send(buffer);
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Probe process timed out".to_string());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(error) => return Err(format!("Cannot wait for probe process: {error}")),
        }
    }

    let output = receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "Probe process produced no output".to_string())?;
    parse_probe_output(&output)
}

/// 子进程只保证 stdout 里**有**那段 JSON;前面可能混进 dylib 加载警告之类的噪声,
/// 所以从第一个 `{` 起解析,而不是要求整段都是 JSON。
fn parse_probe_output(output: &str) -> Result<HashMap<String, PermissionProbe>, String> {
    let start = output
        .find('{')
        .ok_or_else(|| "Probe output is not JSON".to_string())?;
    let records: HashMap<String, ProbeRecord> = serde_json::from_str(&output[start..])
        .map_err(|error| format!("Cannot read probe output: {error}"))?;
    Ok(records
        .into_iter()
        .map(|(id, record)| {
            (
                id,
                PermissionProbe {
                    status: record.status,
                    detail: record.detail,
                },
            )
        })
        .collect())
}

/// `main` 的前置分支:以 `--probe-system-permissions <id,id,...>` 拉起时,只跑检测、
/// 把 JSON 打到 stdout 然后退出,绝不初始化 Tauri(不开窗口、不碰应用状态)。
pub fn try_run_probe_bridge() -> bool {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(PROBE_BRIDGE_FLAG) {
        return false;
    }
    let requested = args.next().unwrap_or_default();
    let records: HashMap<String, ProbeRecord> = requested
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .filter(|id| find_descriptor(id).is_ok())
        .map(|id| {
            let probe = imp::check(id);
            (
                id.to_string(),
                ProbeRecord {
                    status: probe.status,
                    detail: probe.detail,
                },
            )
        })
        .collect();
    // 序列化失败没有兜底可言,打个空对象让父进程报"无法解析"而不是挂住。
    println!(
        "{}",
        serde_json::to_string(&records).unwrap_or_else(|_| "{}".to_string())
    );
    true
}

// ── 合成快照 ─────────────────────────────────────────────────────────────────

/// 把「本进程视角」和「系统视角」两次检测合成一条面向用户的快照。
///
/// `status` 取系统视角:面板要回答的是"设置里到底开没开",这才和用户眼前的系统设置
/// 对得上。进程视角只用来判断要不要提示重启。
fn snapshot(
    descriptor: &PermissionDescriptor,
    process: PermissionProbe,
    system: Option<PermissionProbe>,
) -> SystemPermission {
    let process_status = process.status;
    let system_probe = system.unwrap_or_else(|| process.clone());
    let system_status = system_probe.status;
    // 系统已授权、本进程还没拿到 → 重启即生效。反过来(刚被撤销)不提示重启:
    // 重启只会让本进程一起失去能力,不是用户想要的动作。
    let restart_required = system_status == PermissionStatus::Granted
        && process_status != PermissionStatus::Granted
        && process_status != PermissionStatus::Unknown;

    SystemPermission {
        id: descriptor.id.to_string(),
        status: system_status,
        system_status,
        process_status,
        restart_required,
        can_request_in_app: descriptor.can_request_in_app,
        can_open_settings: descriptor.can_open_settings,
        can_reset: descriptor.can_reset,
        needs_restart: descriptor.needs_restart,
        probe_prompts: descriptor.probe_prompts,
        report_only: descriptor.report_only,
        // 系统视角的说明优先:它才是 `status` 的来源。
        detail: system_probe.detail.or(process.detail),
    }
}

/// 非侵入式检测:`probe_prompts` 的项目不去碰受保护目录,改用本会话已探得的结论,
/// 没有则报 Unknown。
fn check_quiet(descriptor: &PermissionDescriptor) -> PermissionProbe {
    if descriptor.probe_prompts {
        return cached_probe(descriptor.id)
            .unwrap_or_else(|| PermissionProbe::plain(PermissionStatus::Unknown));
    }
    imp::check(descriptor.id)
}

fn find_descriptor(id: &str) -> Result<&'static PermissionDescriptor, String> {
    imp::DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.id == id)
        .ok_or_else(|| format!("Unknown system permission: {id}"))
}

/// 会弹框的项目不进新进程探测——子进程弹的框和父进程一样打扰用户。
fn freshly_probable_ids() -> Vec<&'static str> {
    if !imp::FRESH_PROBE_HELPS {
        return Vec::new();
    }
    imp::DESCRIPTORS
        .iter()
        .filter(|descriptor| !descriptor.probe_prompts)
        .map(|descriptor| descriptor.id)
        .collect()
}

fn build_report(
    permissions: Vec<SystemPermission>,
    fresh_probe_error: Option<String>,
) -> SystemPermissionReport {
    SystemPermissionReport {
        platform: std::env::consts::OS.to_string(),
        supported: imp::SUPPORTED,
        permissions,
        identity: imp::identity(),
        fresh_probe: imp::FRESH_PROBE_HELPS && fresh_probe_error.is_none(),
        fresh_probe_error,
    }
}

fn list_sync() -> SystemPermissionReport {
    let (system, error) = match fresh_probe(&freshly_probable_ids()) {
        Ok(map) => (map, None),
        Err(error) => (HashMap::new(), Some(error)),
    };
    let permissions = imp::DESCRIPTORS
        .iter()
        .map(|descriptor| {
            snapshot(
                descriptor,
                check_quiet(descriptor),
                system.get(descriptor.id).cloned(),
            )
        })
        .collect();
    build_report(permissions, error)
}

/// 单项获取。能在应用内请求的走系统授权流程;只能手工勾选的直接打开系统设置。
/// 两种情况都返回该项的最新快照。
fn request_sync(id: &str) -> Result<SystemPermission, String> {
    let descriptor = find_descriptor(id)?;
    if descriptor.report_only {
        return Err(format!("{id} has no action on this platform"));
    }
    let process = if descriptor.can_request_in_app {
        imp::request(id)
    } else {
        if descriptor.can_open_settings {
            imp::open_settings(id)?;
        }
        check_quiet(descriptor)
    };
    remember_probe(id, &process);
    // 请求之后再问一次系统。目录类权限例外:它的 `process` 就是刚做的真实读取,
    // 再换进程读一遍只会多弹一次框。
    let system = if descriptor.probe_prompts {
        None
    } else {
        probe_one(descriptor.id)
    };
    Ok(snapshot(descriptor, process, system))
}

fn probe_one(id: &'static str) -> Option<PermissionProbe> {
    fresh_probe(&[id]).ok().and_then(|mut map| map.remove(id))
}

/// 一键获取:对所有"未授权且能在应用内请求"的项目依次发起授权流程,其余项目只
/// 刷新状态。不会替用户打开一堆系统设置窗口——那些项目通过 `manual` 返回,由前端
/// 提示用户逐个处理。
fn grant_all_sync() -> GrantAllResult {
    let (mut system, error) = match fresh_probe(&freshly_probable_ids()) {
        Ok(map) => (map, None),
        Err(error) => (HashMap::new(), Some(error)),
    };
    let mut processes = HashMap::new();
    let mut requested = Vec::new();
    let mut manual = Vec::new();

    for descriptor in imp::DESCRIPTORS {
        let current = check_quiet(descriptor);
        let effective = system
            .get(descriptor.id)
            .map_or(current.status, |probe| probe.status);
        // 系统已经记着授权了就别再弹框——此时缺的是重启,不是授权。
        let process = if effective == PermissionStatus::Granted || !descriptor.can_request_in_app {
            current
        } else {
            requested.push(descriptor.id.to_string());
            let probe = imp::request(descriptor.id);
            remember_probe(descriptor.id, &probe);
            probe
        };
        if effective != PermissionStatus::Granted
            && !descriptor.can_request_in_app
            && descriptor.can_open_settings
        {
            manual.push(descriptor.id.to_string());
        }
        processes.insert(descriptor.id, process);
    }

    // 刚弹过框的项目重新问一次系统,否则用户在框里点了"允许"这一轮仍报未授权。
    let re_probe: Vec<&'static str> = imp::DESCRIPTORS
        .iter()
        .filter(|descriptor| {
            !descriptor.probe_prompts && requested.iter().any(|id| id == descriptor.id)
        })
        .map(|descriptor| descriptor.id)
        .collect();
    if let Ok(updated) = fresh_probe(&re_probe) {
        system.extend(updated);
    }

    let permissions = imp::DESCRIPTORS
        .iter()
        .map(|descriptor| {
            let process = processes
                .remove(descriptor.id)
                .unwrap_or_else(|| PermissionProbe::plain(PermissionStatus::Unknown));
            snapshot(descriptor, process, system.get(descriptor.id).cloned())
        })
        .collect();

    // manual 只保留仍未授权的:上一轮可能已经被用户在设置里勾上了。
    GrantAllResult {
        report: build_report(permissions, error),
        requested,
        manual,
    }
}

fn open_settings_sync(id: &str) -> Result<(), String> {
    let descriptor = find_descriptor(id)?;
    if !descriptor.can_open_settings {
        return Err(format!("{id} has no settings pane to open"));
    }
    imp::open_settings(id)
}

/// 清除本应用在系统里的该项授权记录,让下一次请求重新走一遍。
///
/// 为什么需要:ad-hoc 签名的授权绑 cdhash,升级后记录还在但已对不上,系统设置里
/// 开关看着是开的、应用却拿不到权限,且**再点开关也不会重新生效**。只有清掉记录
/// 重新授权才能修好。
fn reset_sync(id: &str) -> Result<SystemPermission, String> {
    let descriptor = find_descriptor(id)?;
    if !descriptor.can_reset {
        return Err(format!("{id} cannot be reset on this platform"));
    }
    imp::reset(id)?;
    if let Ok(mut cache) = probe_cache().lock() {
        cache.remove(id);
    }
    let process = check_quiet(descriptor);
    let system = if descriptor.probe_prompts {
        None
    } else {
        probe_one(descriptor.id)
    };
    Ok(snapshot(descriptor, process, system))
}

// ── 命令 ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_system_permissions() -> Result<SystemPermissionReport, String> {
    tauri::async_runtime::spawn_blocking(list_sync)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn request_system_permission(id: String) -> Result<SystemPermission, String> {
    tauri::async_runtime::spawn_blocking(move || request_sync(&id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn request_all_system_permissions() -> Result<GrantAllResult, String> {
    tauri::async_runtime::spawn_blocking(grant_all_sync)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_system_permission_settings(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_settings_sync(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// 清除该项授权记录。会话缓存一并清掉,下一次检测重新问系统。
#[tauri::command]
pub async fn reset_system_permission(id: String) -> Result<SystemPermission, String> {
    tauri::async_runtime::spawn_blocking(move || reset_sync(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// 重启本应用,让刚授予的权限生效(macOS 的 TCC 判定在进程启动时缓存)。
#[tauri::command]
pub fn restart_app_for_permissions(app: tauri::AppHandle) {
    app.request_restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor_for(id: &str) -> &'static PermissionDescriptor {
        find_descriptor(id).expect("descriptor exists")
    }

    #[test]
    fn descriptor_ids_are_unique_and_non_empty() {
        let mut seen = std::collections::BTreeSet::new();
        for descriptor in imp::DESCRIPTORS {
            assert!(!descriptor.id.is_empty(), "descriptor with empty id");
            assert!(
                seen.insert(descriptor.id),
                "duplicate permission id: {}",
                descriptor.id
            );
        }
    }

    #[test]
    fn actionable_permissions_offer_at_least_one_action() {
        // 一条既不能请求、又打不开设置的权限在界面上是死条目——除非它明确只报告。
        for descriptor in imp::DESCRIPTORS.iter().filter(|d| !d.report_only) {
            assert!(
                descriptor.can_request_in_app || descriptor.can_open_settings,
                "{} exposes no action",
                descriptor.id
            );
        }
    }

    #[test]
    fn report_only_permissions_expose_no_buttons() {
        for descriptor in imp::DESCRIPTORS.iter().filter(|d| d.report_only) {
            assert!(!descriptor.can_request_in_app, "{}", descriptor.id);
            assert!(!descriptor.can_open_settings, "{}", descriptor.id);
            assert!(!descriptor.can_reset, "{}", descriptor.id);
        }
    }

    #[test]
    fn report_marks_support_by_descriptor_table() {
        let report = list_sync();
        assert_eq!(report.platform, std::env::consts::OS);
        assert_eq!(report.supported, imp::SUPPORTED);
        assert_eq!(report.supported, !report.permissions.is_empty());
        assert_eq!(report.permissions.len(), imp::DESCRIPTORS.len());
    }

    #[test]
    fn quiet_check_never_probes_prompting_permissions() {
        for descriptor in imp::DESCRIPTORS.iter().filter(|d| d.probe_prompts) {
            let probe = check_quiet(descriptor);
            // 缓存里可能已有用户显式探测的结果;没有则必须是 Unknown,且都不该弹框。
            assert!(
                cached_probe(descriptor.id).is_some() || probe.status == PermissionStatus::Unknown,
                "{} was probed during a quiet check",
                descriptor.id
            );
        }
    }

    #[test]
    fn unknown_permission_id_is_rejected() {
        assert!(find_descriptor("not-a-real-permission").is_err());
        assert!(request_sync("not-a-real-permission").is_err());
        assert!(open_settings_sync("not-a-real-permission").is_err());
        assert!(reset_sync("not-a-real-permission").is_err());
    }

    #[test]
    fn status_serializes_as_camel_case() {
        let json = serde_json::to_string(&PermissionStatus::NotGranted).expect("serialize");
        assert_eq!(json, "\"notGranted\"");
        let parsed: PermissionStatus = serde_json::from_str("\"granted\"").expect("deserialize");
        assert_eq!(parsed, PermissionStatus::Granted);
    }

    /// 这条是整个修复的核心:系统已授权、本进程还没拿到,必须报「已获取 + 需重启」,
    /// 而不是谎报未获取——那正是用户看到的假阴性。
    #[test]
    fn system_grant_outranks_stale_process_state() {
        let snapshot = snapshot(
            descriptor_for(imp::DESCRIPTORS[0].id),
            PermissionProbe::plain(PermissionStatus::NotGranted),
            Some(PermissionProbe::plain(PermissionStatus::Granted)),
        );
        assert_eq!(snapshot.status, PermissionStatus::Granted);
        assert_eq!(snapshot.system_status, PermissionStatus::Granted);
        assert_eq!(snapshot.process_status, PermissionStatus::NotGranted);
        assert!(snapshot.restart_required);
    }

    /// 反过来(刚被撤销)不提示重启:重启只会让本进程一起失去能力。
    #[test]
    fn revoked_permission_does_not_ask_for_restart() {
        let snapshot = snapshot(
            descriptor_for(imp::DESCRIPTORS[0].id),
            PermissionProbe::plain(PermissionStatus::Granted),
            Some(PermissionProbe::plain(PermissionStatus::NotGranted)),
        );
        assert_eq!(snapshot.status, PermissionStatus::NotGranted);
        assert!(!snapshot.restart_required);
    }

    /// 探测不可用时退化成单一状态,不能凭空冒出一个"需重启"。
    #[test]
    fn missing_system_probe_falls_back_to_process_state() {
        let snapshot = snapshot(
            descriptor_for(imp::DESCRIPTORS[0].id),
            PermissionProbe::plain(PermissionStatus::NotGranted),
            None,
        );
        assert_eq!(snapshot.status, PermissionStatus::NotGranted);
        assert_eq!(snapshot.system_status, PermissionStatus::NotGranted);
        assert!(!snapshot.restart_required);
    }

    /// 进程侧无法判定时不要求重启:那只是"不知道",不是"缺能力"。
    #[test]
    fn unknown_process_state_does_not_ask_for_restart() {
        let snapshot = snapshot(
            descriptor_for(imp::DESCRIPTORS[0].id),
            PermissionProbe::plain(PermissionStatus::Unknown),
            Some(PermissionProbe::plain(PermissionStatus::Granted)),
        );
        assert_eq!(snapshot.status, PermissionStatus::Granted);
        assert!(!snapshot.restart_required);
    }

    #[test]
    fn probe_output_survives_leading_noise() {
        let parsed = parse_probe_output(
            "dyld[123]: some warning\n{\"accessibility\":{\"status\":\"granted\"}}\n",
        )
        .expect("parses");
        assert_eq!(
            parsed.get("accessibility").map(|probe| probe.status),
            Some(PermissionStatus::Granted)
        );
    }

    #[test]
    fn probe_output_without_json_is_an_error() {
        assert!(parse_probe_output("").is_err());
        assert!(parse_probe_output("no json here").is_err());
        assert!(parse_probe_output("{not json}").is_err());
    }

    /// 探测子进程绝不能再 spawn 探测子进程。
    #[test]
    fn nested_probe_is_refused() {
        let guard = std::env::var_os(PROBE_GUARD_ENV);
        // SAFETY: 单线程测试内改环境变量,结束前还原。
        unsafe { std::env::set_var(PROBE_GUARD_ENV, "1") };
        let result = fresh_probe(&["accessibility"]);
        match guard {
            Some(value) => unsafe { std::env::set_var(PROBE_GUARD_ENV, value) },
            None => unsafe { std::env::remove_var(PROBE_GUARD_ENV) },
        }
        assert!(result.is_err());
    }

    #[test]
    fn empty_probe_list_skips_the_subprocess() {
        assert!(fresh_probe(&[]).expect("no-op succeeds").is_empty());
    }

    #[test]
    fn unknown_probes_are_not_cached() {
        remember_probe(
            "cache-test-unknown",
            &PermissionProbe::unknown("no reason to remember"),
        );
        assert!(cached_probe("cache-test-unknown").is_none());
        remember_probe(
            "cache-test-granted",
            &PermissionProbe::plain(PermissionStatus::Granted),
        );
        assert_eq!(
            cached_probe("cache-test-granted").map(|probe| probe.status),
            Some(PermissionStatus::Granted)
        );
    }

    #[test]
    fn identity_reports_a_subject_and_signature_kind() {
        let identity = imp::identity();
        assert!(!identity.subject.is_empty());
        assert!(!identity.signature.is_empty());
    }
}
