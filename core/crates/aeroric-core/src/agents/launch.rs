//! 四族命令的纯命令规划(P3-02 Action 1):**计划与执行分离**。
//!
//! [`LaunchPlan`] 是"要把哪个程序以什么参数/环境/工作目录拉起来"的纯数据描述,
//! 不携带任何可执行句柄——宿主把它变成 `portable_pty::CommandBuilder`,CLI 把它
//! 变成 `tokio::process::Command`,两边的 spawn 形状可以不同,argv/env/cwd 必须
//! 逐字节相同(验收 = 迁移前后比较,差异必须在 ADR 中逐项说明)。
//!
//! 本文件的映射表自宿主 `agent_family_maps.rs` 一次性 move(它曾是"本地 PTY 与
//! 远端 shell 各抄一份、靠注释互相同步"的病,收敛为唯一来源后本地与远端天然
//! 同步;现在再前进一步:core 是唯一真相源,宿主 re-export)。

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

/// 一次拉起的完整计划:program/args 之外,还带要设置的环境(有序,后写覆盖先写)、
/// 要**移除**的环境(如 `PI_PROFILE`/`OMP_PROFILE`——omp 的 profile 优先于
/// `PI_CODING_AGENT_DIR`,只设不删会让托管 home 静默失效)与工作目录。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    /// `None` = 继承调用方 cwd(本地 shell/交互路径的现状)。
    pub cwd: Option<std::path::PathBuf>,
    /// 有序环境注入:先到先设、后到覆盖,宿主按序重放。
    pub env: Vec<(String, String)>,
    /// 必须从继承环境里移除的键。
    pub unset_env: Vec<String>,
}

impl LaunchPlan {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Default::default()
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn unset_env(mut self, key: impl Into<String>) -> Self {
        self.unset_env.push(key.into());
        self
    }

    pub fn cwd(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.cwd = Some(path.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三档权限模式在四张表上的产物快照(自宿主 agent_family_maps.rs 随体迁入)。
    /// 本地(pty)与远端(ssh)共用这些函数,这里的断言就是「两侧一致」的契约;
    /// 改动任何一档都必须是有意的。
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
        assert_eq!(
            omp_thinking_level("ultra"),
            Some("max"),
            "ultra 封顶 max,不向上游发送 ultra"
        );
        assert_eq!(omp_thinking_level("bogus"), None);
    }

    /// LaunchPlan 的环境语义:env 有序(后写覆盖先写,宿主按序重放),
    /// unset_env 与 env 互相独立——"先 unset 再设"由调用方按序表达。
    #[test]
    fn a_launch_plan_keeps_env_order_and_separate_unset_list() {
        let plan = LaunchPlan::new("omp")
            .arg("--cwd")
            .arg("/tmp/proj")
            .env("PI_CODING_AGENT_DIR", "/home/managed")
            .unset_env("PI_PROFILE")
            .unset_env("OMP_PROFILE")
            .env("DSH_TELEMETRY_DISABLED", "1")
            .cwd("/tmp/spawn-cwd");

        assert_eq!(plan.program, "omp");
        assert_eq!(plan.args, vec!["--cwd", "/tmp/proj"]);
        assert_eq!(
            plan.env,
            vec![
                (
                    "PI_CODING_AGENT_DIR".to_string(),
                    "/home/managed".to_string()
                ),
                ("DSH_TELEMETRY_DISABLED".to_string(), "1".to_string()),
            ],
            "env 保持注入顺序"
        );
        assert_eq!(plan.unset_env, vec!["PI_PROFILE", "OMP_PROFILE"]);
        assert_eq!(
            plan.cwd.as_deref(),
            Some(std::path::Path::new("/tmp/spawn-cwd"))
        );
    }
}
