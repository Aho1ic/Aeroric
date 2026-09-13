//! 内置 agent 家族的权限档 / effort → 启动参数映射表。
//!
//! 这几张表曾分别在 `pty.rs`（本地 PTY）与 `ssh.rs`（远端 shell）各抄一份，
//! 只靠注释互相提醒「两处必须一致」。现在收敛为唯一来源：改任何一档的产物
//! 都只动这里，本地与远端天然同步。
//!
//! 注意家族**归类**（agent id → [`AgentFamily`]）不在这份表里：本地与远端对
//! 未知 agent 的默认族是刻意不同的（本地 Codex、远端 Claude），且远端不感知
//! 自定义 agent 配置，不能共用同一张映射。

/// Aeroric 权限模式 → omp `--approval-mode` 参数值。
pub fn omp_permission_flag(permission_mode: &str) -> Option<&'static str> {
    match permission_mode {
        "ask" => Some("always-ask"),
        "auto_edit" => Some("write"),
        "full_access" => Some("yolo"),
        _ => None,
    }
}

/// Aeroric 统一 effort 词表 → omp thinking level(与前端 OMP_THINKING_LEVEL_MAP
/// 恒等映射一致:omp 原生 7 档透传,ultra 封顶 max)。
pub fn omp_thinking_level(effort: &str) -> Option<&'static str> {
    match effort {
        "off" => Some("off"),
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" => Some("max"),
        "ultra" => Some("max"),
        _ => None,
    }
}

/// Aeroric 权限模式 → dsh `DSH_PERMISSION_MODE` 环境变量值
/// (ask → read-only、auto_edit → workspace-write、full_access → danger-full-access)。
pub fn dsh_permission_mode(permission_mode: &str) -> Option<&'static str> {
    match permission_mode {
        "ask" => Some("read-only"),
        "auto_edit" => Some("workspace-write"),
        "full_access" => Some("danger-full-access"),
        _ => None,
    }
}

/// Aeroric 权限模式 → claude 启动参数(flag 在前、值紧随)。
pub fn claude_permission_args(permission_mode: &str) -> Vec<&'static str> {
    match permission_mode {
        "ask" => vec!["--permission-mode", "default"],
        "auto_edit" => vec!["--permission-mode", "acceptEdits"],
        "full_access" => vec!["--dangerously-skip-permissions"],
        _ => vec![],
    }
}

/// Aeroric 权限模式 → codex 启动参数。`auto_edit` 等价于已弃用的
/// `--full-auto`(codex >= 0.128 已移除该别名):工作区内自动写、越界命令才升级审批。
pub fn codex_permission_args(permission_mode: &str) -> Vec<&'static str> {
    match permission_mode {
        "auto_edit" => vec!["--sandbox", "workspace-write", "-a", "on-request"],
        "full_access" => vec!["--dangerously-bypass-approvals-and-sandbox"],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三档权限模式在四张表上的产物快照。本地(pty)与远端(ssh)共用这些函数,
    /// 这里的断言就是「两侧一致」的契约;改动任何一档都必须是有意的。
    #[test]
    fn permission_tables_are_stable_across_local_and_remote() {
        for mode in ["ask", "auto_edit", "full_access"] {
            // omp:三档都有映射。
            assert!(omp_permission_flag(mode).is_some(), "omp 缺 {mode} 档");
            assert!(dsh_permission_mode(mode).is_some(), "dsh 缺 {mode} 档");
        }
        assert_eq!(omp_permission_flag("ask"), Some("always-ask"));
        assert_eq!(omp_permission_flag("auto_edit"), Some("write"));
        assert_eq!(omp_permission_flag("full_access"), Some("yolo"));

        assert_eq!(dsh_permission_mode("ask"), Some("read-only"));
        assert_eq!(dsh_permission_mode("auto_edit"), Some("workspace-write"));
        assert_eq!(
            dsh_permission_mode("full_access"),
            Some("danger-full-access")
        );

        // claude:ask/full_access 有映射,auto_edit 也走 --permission-mode。
        assert_eq!(
            claude_permission_args("ask"),
            vec!["--permission-mode", "default"]
        );
        assert_eq!(
            claude_permission_args("auto_edit"),
            vec!["--permission-mode", "acceptEdits"]
        );
        assert_eq!(
            claude_permission_args("full_access"),
            vec!["--dangerously-skip-permissions"]
        );

        assert_eq!(
            codex_permission_args("auto_edit"),
            vec!["--sandbox", "workspace-write", "-a", "on-request"]
        );
        assert_eq!(
            codex_permission_args("full_access"),
            vec!["--dangerously-bypass-approvals-and-sandbox"]
        );

        // 未定义档位一律静默不注入,而不是报错。
        assert_eq!(omp_permission_flag("bogus"), None);
        assert_eq!(dsh_permission_mode("bogus"), None);
        assert!(claude_permission_args("bogus").is_empty());
        assert!(codex_permission_args("bogus").is_empty());
    }

    #[test]
    fn omp_thinking_level_maps_identity_with_ultra_cap() {
        assert_eq!(omp_thinking_level("off"), Some("off"));
        assert_eq!(omp_thinking_level("minimal"), Some("minimal"));
        assert_eq!(omp_thinking_level("low"), Some("low"));
        assert_eq!(omp_thinking_level("xhigh"), Some("xhigh"));
        assert_eq!(omp_thinking_level("max"), Some("max"));
        assert_eq!(omp_thinking_level("ultra"), Some("max"));
        assert_eq!(omp_thinking_level("bogus"), None);
    }
}
