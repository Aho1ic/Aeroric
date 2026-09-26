//! spawn 环境的顺序操作表达(P3-02 Action 1 的 env 半边)。
//!
//! 宿主 `setup_env`/`setup_omp_env` 的职责不是"构造命令"而是"按序重放一组环境
//! 操作"。把操作表搬进 core 后,顺序约束从注释变成结构:`combined_omp_env_ops`
//! 返回 terminal 序列**再接** omp 序列的单个列表——宿主不可能把 `PI_PROFILE`
//! 的清除排到 login-shell 合并之前(那样清除会被覆盖,托管 home 静默失效)。
//!
//! 分工:core 决定"要设什么、要删什么、什么顺序";宿主决定"login-shell 环境
//! 从哪来"(I/O)并机械重放。

/// 一条环境操作。`Unset` 在语义上先于同 key 的 `Set` 由调用方的列表顺序表达。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvOp {
    Set(String, String),
    Unset(String),
}

/// omp 的 profile 优先于 `PI_CODING_AGENT_DIR`(上游 `dirs.ts` 的
/// `resolveActiveAgentDirOverride` 是 `activeProfile ? undefined : …`),只要
/// `PI_PROFILE`/`OMP_PROFILE` 有值,托管 home 直接被丢弃。这两个键**必须 Unset**,
/// 而且必须发生在 login-shell 合并**之后**(合并可能把它们带回来)。
pub const OMP_PROFILE_KEYS: &[&str] = &["PI_PROFILE", "OMP_PROFILE"];

/// `setup_env` 的计划半边:颜色键先删(继承来的 NO_COLOR 会让原生 TUI 掉色)、
/// login-shell 合并(跳过颜色键)、LANG/LC_CTYPE 兜底、终端类型与强制真彩。
pub fn terminal_env_ops(login_env: &[(String, String)]) -> Vec<EnvOp> {
    const COLOR_KEYS: [&str; 4] = ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "FORCE_COLOR"];
    let mut ops = Vec::new();
    for key in COLOR_KEYS {
        ops.push(EnvOp::Unset(key.to_string()));
    }
    for (key, value) in login_env {
        if COLOR_KEYS.contains(&key.as_str()) {
            continue;
        }
        ops.push(EnvOp::Set(key.clone(), value.clone()));
    }
    let has = |name: &str| login_env.iter().any(|(k, _)| k == name);
    if !has("LANG") {
        ops.push(EnvOp::Set("LANG".into(), "en_US.UTF-8".into()));
    }
    if !has("LC_CTYPE") {
        ops.push(EnvOp::Set("LC_CTYPE".into(), "en_US.UTF-8".into()));
    }
    ops.push(EnvOp::Set("TERM".into(), "xterm-256color".into()));
    ops.push(EnvOp::Set("COLORTERM".into(), "truecolor".into()));
    ops.push(EnvOp::Set("CLICOLOR".into(), "1".into()));
    ops.push(EnvOp::Set("CLICOLOR_FORCE".into(), "1".into()));
    ops.push(EnvOp::Set("FORCE_COLOR".into(), "3".into()));
    ops
}

/// Aeroric hook 守卫(`aeroric-hook.mjs` / `aeroric-omp-hook.js` 依靠
/// AERORIC_TASK_ID + AERORIC_EVENT_DIR 同时存在才工作;用户在 Aeroric 之外手动
/// 跑 agent 时这些变量缺失,脚本立即 exit 0)。`event_dir` 为 `None` 时(宿主
/// 侧 events 目录不可得)什么都不注入——与旧 `setup_aeroric_env` 的 if-let 一致。
pub fn aeroric_guard_ops(
    task_id: &str,
    agent: &str,
    is_codex: bool,
    event_dir: Option<&str>,
) -> Vec<EnvOp> {
    match event_dir {
        Some(dir) => vec![
            EnvOp::Set("AERORIC_TASK_ID".into(), task_id.to_string()),
            EnvOp::Set("AERORIC_EVENT_DIR".into(), dir.to_string()),
            EnvOp::Set("AERORIC_AGENT".into(), agent.to_string()),
            EnvOp::Set(
                "AERORIC_AGENT_CODEX_LIKE".into(),
                if is_codex { "1" } else { "0" }.into(),
            ),
        ],
        None => Vec::new(),
    }
}

/// `setup_omp_env` 的计划半边:先清 profile 键、再重放托管 home env、最后 hook
/// 守卫。**顺序是语义**:login-shell 可能把 profile 键带回来,所以清除必须在
/// 合并之后——这正是 [`combined_omp_env_ops`] 把两段拼成一个列表的原因。
pub fn omp_env_ops(
    agent_env: &[(String, String)],
    task_id: &str,
    agent: &str,
    is_codex: bool,
    event_dir: Option<&str>,
) -> Vec<EnvOp> {
    let mut ops = Vec::new();
    for key in OMP_PROFILE_KEYS {
        ops.push(EnvOp::Unset(key.to_string()));
    }
    for (key, value) in agent_env {
        ops.push(EnvOp::Set(key.clone(), value.clone()));
    }
    ops.extend(aeroric_guard_ops(task_id, agent, is_codex, event_dir));
    ops
}

/// omp 拉起的完整环境序列:terminal 层在前、omp 层在后,顺序由本函数的结构
/// 保证(INV-06/INV-07 的"setup_omp_env 必须后于 setup_env"从注释变成形状)。
pub fn combined_omp_env_ops(
    login_env: &[(String, String)],
    agent_env: &[(String, String)],
    task_id: &str,
    agent: &str,
    is_codex: bool,
    event_dir: Option<&str>,
) -> Vec<EnvOp> {
    let mut ops = terminal_env_ops(login_env);
    ops.extend(omp_env_ops(agent_env, task_id, agent, is_codex, event_dir));
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_map(ops: &[EnvOp]) -> std::collections::BTreeMap<String, String> {
        let mut map = std::collections::BTreeMap::new();
        for op in ops {
            match op {
                EnvOp::Set(k, v) => {
                    map.insert(k.clone(), v.clone());
                }
                EnvOp::Unset(k) => {
                    map.remove(k);
                }
            }
        }
        map
    }

    #[test]
    fn terminal_ops_drop_inherited_no_color_and_force_truecolor() {
        let login = vec![
            ("NO_COLOR".to_string(), "1".to_string()),
            ("PATH".to_string(), "/usr/bin".to_string()),
        ];
        let ops = terminal_env_ops(&login);
        let map = env_map(&ops);
        assert!(!map.contains_key("NO_COLOR"), "继承的 NO_COLOR 必须被删掉");
        assert_eq!(map.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(map.get("TERM").map(String::as_str), Some("xterm-256color"));
        assert_eq!(map.get("FORCE_COLOR").map(String::as_str), Some("3"));
    }

    #[test]
    fn lang_fallback_only_fires_when_login_env_lacks_it() {
        let without = terminal_env_ops(&[]);
        assert!(without.contains(&EnvOp::Set("LANG".into(), "en_US.UTF-8".into())));
        let with = terminal_env_ops(&[("LANG".to_string(), "zh_CN.UTF-8".to_string())]);
        assert!(!with.contains(&EnvOp::Set("LANG".into(), "en_US.UTF-8".into())));
    }

    /// 顺序即语义:omp ops 拼在 terminal ops **之后**,所以无论 login-shell 带回
    /// 什么 profile,最后的 Unset 一定赢——把它排反了,这个反例就红。
    #[test]
    fn combined_sequence_puts_profile_unset_after_the_login_merge() {
        let login = vec![("PI_PROFILE".to_string(), "sota".to_string())];
        let agent_env = vec![(
            "PI_CODING_AGENT_DIR".to_string(),
            "/managed/omp".to_string(),
        )];
        let ops = combined_omp_env_ops(&login, &agent_env, "t1", "omp", false, None);

        let unset_pos = ops
            .iter()
            .position(|op| op == &EnvOp::Unset("PI_PROFILE".into()))
            .expect("profile unset 必须存在");
        let merge_pos = ops
            .iter()
            .position(|op| matches!(op, EnvOp::Set(k, _) if k == "PI_PROFILE"))
            .expect("login merge 会带回 PI_PROFILE");
        assert!(
            unset_pos > merge_pos,
            "profile 清除必须排在 login-shell 合并之后:unset@{unset_pos} vs merge@{merge_pos}"
        );

        let map = env_map(&ops);
        assert!(!map.contains_key("PI_PROFILE"));
        assert_eq!(
            map.get("PI_CODING_AGENT_DIR").map(String::as_str),
            Some("/managed/omp")
        );
    }

    #[test]
    fn guard_ops_are_all_or_nothing_on_the_event_dir() {
        let present = aeroric_guard_ops("t1", "claude", false, Some("/events/t1"));
        assert_eq!(present.len(), 4);
        assert!(present.contains(&EnvOp::Set("AERORIC_AGENT_CODEX_LIKE".into(), "0".into())));

        let absent = aeroric_guard_ops("t1", "claude", true, None);
        assert!(
            absent.is_empty(),
            "events 目录不可得时什么都不注入(旧 if-let)"
        );
    }
}
