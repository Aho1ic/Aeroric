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
        "ask" | "plan" => Some("always-ask"),
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

// ── 四族计划函数:build_*_cmd + add_*_launch_args 的计划半边(P3-02 切片二)──
//
// 每个函数覆盖宿主旧 builder 与 add_* 的全部职责,产出 [`LaunchPlan`];
// 宿主把它机械翻译成 CommandBuilder。run-flow 层(cwd、setup_env 系、
// AERORIC_AGENT_MODEL、extra_env 最后赢家)不属于家族规划,留在宿主。

/// 家族规划的纯数据输入:与宿主 `AgentLaunchSpec` 的 program/args 同形
/// (extra_env 的注入在 run-flow 层、最后赢家,不在这里)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchInput {
    pub program: String,
    pub args: Vec<String>,
}

impl LaunchInput {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
}

/// `omp --mode rpc` (ADR-043 phase two): the structured driver's argv. Unlike
/// [`omp_plan`] it takes no positional prompt (prompts go over the protocol)
/// and always passes `--mode rpc`; environment is the launcher's.
pub fn omp_rpc_args(
    permission_mode: &str,
    project_path: &str,
    model: Option<&str>,
    thinking: Option<&str>,
    resume: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["--mode".to_string(), "rpc".to_string()];
    if let Some(flag) = omp_permission_flag(permission_mode) {
        args.extend(["--approval-mode".to_string(), flag.to_string()]);
    }
    args.extend(["--cwd".to_string(), project_path.to_string()]);
    if let Some(model) = normalize_selected(model) {
        args.extend(["--model".to_string(), model]);
    }
    if let Some(level) = normalize_selected(thinking).and_then(|level| omp_thinking_level(&level)) {
        args.extend(["--thinking".to_string(), level.to_string()]);
    }
    if let Some(resume) = normalize_selected(resume) {
        args.extend(["--resume".to_string(), resume]);
    }
    args
}

fn normalize_selected(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
}

fn toml_table_key(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// TOML 字符串字面量(与 `toml::Value::String` 的序列化对基础字符等价:
/// 转义反斜杠与引号)。effort/模型词表来自受控集合,不会出现控制字符。
fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn claude_plan(
    input: &LaunchInput,
    permission_mode: &str,
    agent: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> LaunchPlan {
    let mut plan = LaunchPlan::new(&input.program).args(input.args.clone());
    for arg in claude_permission_args(permission_mode) {
        plan = plan.arg(arg);
    }
    if agent == "claude" {
        if let Some(model) = normalize_selected(model) {
            plan = plan.arg("--model").arg(model);
        }
    }
    if let Some(effort) = effort {
        plan = plan.arg("--effort").arg(effort);
    }
    plan
}

pub fn codex_plan(
    input: &LaunchInput,
    permission_mode: &str,
    project_path: &str,
    model: Option<&str>,
    effort: Option<&str>,
    speed: Option<&str>,
) -> LaunchPlan {
    let mut plan = LaunchPlan::new(&input.program).args(input.args.clone());
    for arg in codex_permission_args(permission_mode) {
        plan = plan.arg(arg);
    }
    plan = plan.arg("-C").arg(project_path);
    plan = plan.arg("-c").arg(format!(
        "projects.{}.trust_level=\"trusted\"",
        toml_table_key(project_path)
    ));
    if let Some(model) = normalize_selected(model) {
        plan = plan.arg("-m").arg(model);
    }
    if let Some(effort) = effort {
        plan = plan
            .arg("-c")
            .arg(format!("model_reasoning_effort={}", toml_string(effort)));
    }
    if speed == Some("fast") {
        plan = plan.arg("-c").arg("features.fast_mode=true");
        plan = plan.arg("-c").arg("service_tier=\"fast\"");
    }
    plan
}

pub fn dsh_plan(
    input: &LaunchInput,
    permission_mode: &str,
    interactive: bool,
    dsh_home: &std::path::Path,
    patches: &[std::path::PathBuf],
) -> LaunchPlan {
    let mut plan = LaunchPlan::new(&input.program).args(input.args.clone());
    if !interactive {
        plan = plan.arg("--profile").arg("headless");
    }
    if let Some(mode) = dsh_permission_mode(permission_mode) {
        plan = plan.env("DSH_PERMISSION_MODE", mode);
    }
    // 受管 patch 已禁用遥测 row;env 再兜底一层,防 dsh 未来默认值变化。
    plan = plan.env("DSH_TELEMETRY_DISABLED", "1");
    plan = plan.env("DSH_HOME", dsh_home.to_string_lossy().into_owned());
    for patch in patches {
        plan = plan
            .arg("--patch")
            .arg(patch.to_string_lossy().into_owned());
    }
    plan
}

pub fn omp_plan(
    input: &LaunchInput,
    permission_mode: &str,
    agent_env: &[(String, String)],
    project_path: &str,
    model: Option<&str>,
    thinking: Option<&str>,
    resume: Option<&str>,
    positional_prompt: &[String],
) -> LaunchPlan {
    let mut plan = LaunchPlan::new(&input.program).args(input.args.clone());
    if let Some(flag) = omp_permission_flag(permission_mode) {
        plan = plan.arg("--approval-mode").arg(flag);
    }
    for (key, value) in agent_env {
        plan = plan.env(key.clone(), value.clone());
    }
    plan = plan.arg("--cwd").arg(project_path);
    if let Some(model) = normalize_selected(model) {
        plan = plan.arg("--model").arg(model);
    }
    if let Some(level) = normalize_selected(thinking).and_then(|level| omp_thinking_level(&level)) {
        plan = plan.arg("--thinking").arg(level);
    }
    if let Some(resume) = normalize_selected(resume) {
        plan = plan.arg("--resume").arg(resume);
    }
    for arg in positional_prompt {
        plan = plan.arg(arg.clone());
    }
    plan
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
