//! 系统权限清单:枚举本应用**可能**需要的系统授权,逐项报告当前状态,并提供
//! 单项获取 / 一键获取 / 打开系统设置三种动作。
//!
//! 为什么需要它:agent 在终端里跑任意命令(截屏、控制 UI、读 Desktop 下的项目),
//! 这些调用继承的是 Aeroric 自己的 TCC 身份。缺权限时系统只会静默失败或弹一次
//! 用户当时可能拒掉的框,之后再没有入口。这里把清单显式化。
//!
//! 只有 macOS 有逐项授权模型(TCC)。Windows / Linux 不需要这类前置授权,
//! `list_system_permissions` 在那些平台返回 `supported: false` 的空报告——面板本身
//! 只在 macOS 注册,但命令在任何平台都能安全调用(手机远程端也会打到这里)。
//!
//! 检测一律**不弹框**:目录类权限例外,读取受保护目录会触发系统询问,所以它们默认
//! 报 `Unknown`,只在用户显式点"获取"时才探测(见 `probe_prompts`)。

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

#[cfg(target_os = "macos")]
use self::macos as imp;
#[cfg(not(target_os = "macos"))]
use self::other as imp;

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
    pub status: PermissionStatus,
    /// 能否在应用内直接触发系统授权流程(弹框或探测)。
    pub can_request_in_app: bool,
    /// 能否跳转到对应的系统设置面板。
    pub can_open_settings: bool,
    /// 授权变更后必须重启本应用才生效。
    pub needs_restart: bool,
    /// 检测本身会触发系统询问,所以默认不探测(目录类权限)。
    pub probe_prompts: bool,
    /// 检测失败的原因(如自动化目标未运行)。前端原样展示,不参与判定。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemPermissionReport {
    pub platform: String,
    /// 本平台是否有逐项授权模型。false 时 `permissions` 为空。
    pub supported: bool,
    pub permissions: Vec<SystemPermission>,
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
    pub(crate) needs_restart: bool,
    pub(crate) probe_prompts: bool,
}

/// 一次检测的结果:状态 + 可选的失败说明。
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

    pub(crate) fn unknown(detail: impl Into<String>) -> Self {
        Self {
            status: PermissionStatus::Unknown,
            detail: Some(detail.into()),
        }
    }
}

fn snapshot(descriptor: &PermissionDescriptor, probe: PermissionProbe) -> SystemPermission {
    SystemPermission {
        id: descriptor.id.to_string(),
        status: probe.status,
        can_request_in_app: descriptor.can_request_in_app,
        can_open_settings: descriptor.can_open_settings,
        needs_restart: descriptor.needs_restart,
        probe_prompts: descriptor.probe_prompts,
        detail: probe.detail,
    }
}

/// 非侵入式检测:`probe_prompts` 的项目直接报 Unknown,不去碰受保护目录。
fn check_quiet(descriptor: &PermissionDescriptor) -> PermissionProbe {
    if descriptor.probe_prompts {
        return PermissionProbe::plain(PermissionStatus::Unknown);
    }
    imp::check(descriptor.id)
}

fn build_report(permissions: Vec<SystemPermission>) -> SystemPermissionReport {
    SystemPermissionReport {
        platform: std::env::consts::OS.to_string(),
        supported: imp::SUPPORTED,
        permissions,
    }
}

fn find_descriptor(id: &str) -> Result<&'static PermissionDescriptor, String> {
    imp::DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.id == id)
        .ok_or_else(|| format!("Unknown system permission: {id}"))
}

fn list_sync() -> SystemPermissionReport {
    let permissions = imp::DESCRIPTORS
        .iter()
        .map(|descriptor| snapshot(descriptor, check_quiet(descriptor)))
        .collect();
    build_report(permissions)
}

/// 单项获取。能在应用内请求的走系统授权流程;只能手工勾选的直接打开系统设置。
/// 两种情况都返回该项的最新快照。
fn request_sync(id: &str) -> Result<SystemPermission, String> {
    let descriptor = find_descriptor(id)?;
    let probe = if descriptor.can_request_in_app {
        imp::request(id)
    } else {
        if descriptor.can_open_settings {
            imp::open_settings(id)?;
        }
        imp::check(id)
    };
    Ok(snapshot(descriptor, probe))
}

/// 一键获取:对所有"未授权且能在应用内请求"的项目依次发起授权流程,其余项目只
/// 刷新状态。不会替用户打开一堆系统设置窗口——那些项目通过 `manual` 返回,由前端
/// 提示用户逐个处理。
fn grant_all_sync() -> GrantAllResult {
    let mut permissions = Vec::with_capacity(imp::DESCRIPTORS.len());
    let mut requested = Vec::new();
    let mut manual = Vec::new();

    for descriptor in imp::DESCRIPTORS {
        let current = check_quiet(descriptor);
        let probe = if current.status == PermissionStatus::Granted {
            current
        } else if descriptor.can_request_in_app {
            requested.push(descriptor.id.to_string());
            imp::request(descriptor.id)
        } else {
            current
        };
        if probe.status != PermissionStatus::Granted && !descriptor.can_request_in_app {
            manual.push(descriptor.id.to_string());
        }
        permissions.push(snapshot(descriptor, probe));
    }

    GrantAllResult {
        report: build_report(permissions),
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

/// 重启本应用,让刚授予的权限生效(macOS 的 TCC 判定在进程启动时缓存)。
#[tauri::command]
pub fn restart_app_for_permissions(app: tauri::AppHandle) {
    app.request_restart();
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_permission_offers_at_least_one_action() {
        // 一条既不能请求、又打不开设置的权限在界面上是死条目。
        for descriptor in imp::DESCRIPTORS {
            assert!(
                descriptor.can_request_in_app || descriptor.can_open_settings,
                "{} exposes no action",
                descriptor.id
            );
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
            assert_eq!(
                probe.status,
                PermissionStatus::Unknown,
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
    }

    #[test]
    fn status_serializes_as_camel_case() {
        let json = serde_json::to_string(&PermissionStatus::NotGranted).expect("serialize");
        assert_eq!(json, "\"notGranted\"");
        let parsed: PermissionStatus = serde_json::from_str("\"granted\"").expect("deserialize");
        assert_eq!(parsed, PermissionStatus::Granted);
    }
}
