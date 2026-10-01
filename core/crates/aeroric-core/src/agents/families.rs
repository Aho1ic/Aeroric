//! agent → family 三判定的**唯一实现**（P3-07 b1.0：自宿主 `SettingsFamilies`
//! 纯化上收，补记二百八十八）。
//!
//! 判定只吃四个字段：agent 名 + custom profile 的 `family`/`codex_like`
//! （内建表在代码里）。数据从哪来不归本文件管——Tauri 宿主喂它的全局
//! settings 缓存，CLI host 喂 `storage::load_settings` 的读数；**两个注入点、
//! 一份判定**，不留第二份真相。
//!
//! 语义逐字保真（含两条镜像规则）：
//! - `claude_gpt55`：session 是 Claude 格式、launch/resume 是 codex 能力（ADR-029）；
//! - unknown agent：`agent == "claude" ? Claude : Codex`——镜像
//!   `src/agents.ts:170-178`，「不改行为」就是照抄它。

use crate::tasks::ProtocolFamily;

/// custom profile 的判定输入（宿主从 `AgentLaunchSpec` 或 settings record 提取）。
#[derive(Debug, Clone, Copy)]
pub struct FamilyProfile<'a> {
    pub family: &'a str,
    pub codex_like: bool,
}

/// 内建的 session 家族表；`claude_gpt55` 在格式上是 Claude。
pub fn built_in_session_family(agent: &str) -> Option<ProtocolFamily> {
    match agent {
        "claude" | "claude_gpt55" => Some(ProtocolFamily::Claude),
        "codex" => Some(ProtocolFamily::Codex),
        "dsh" => Some(ProtocolFamily::Dsh),
        "omp" => Some(ProtocolFamily::Omp),
        _ => None,
    }
}

/// ADR-029 的内建 resume 能力表：`claude_gpt55` 是 codex 能力（客户端拿
/// `agentOption("claude_gpt55").family` 和记录家族比对，所以 `claude` 任务
/// 切到它是 handoff 不是 `--resume`）。
pub fn built_in_native_resume_family(agent: &str) -> Option<ProtocolFamily> {
    match agent {
        "claude_gpt55" => Some(ProtocolFamily::Codex),
        _ => built_in_session_family(agent),
    }
}

pub fn is_builtin(agent: &str) -> bool {
    built_in_session_family(agent).is_some()
}

/// `AgentFamily::parse` 的 core 形：认四个字符串，其余 None（调用方回退）。
pub fn parse_family(value: &str) -> Option<ProtocolFamily> {
    match value.trim() {
        "claude" => Some(ProtocolFamily::Claude),
        "codex" => Some(ProtocolFamily::Codex),
        "dsh" => Some(ProtocolFamily::Dsh),
        "omp" => Some(ProtocolFamily::Omp),
        _ => None,
    }
}

/// custom profile 的 session 家族：显式 `family` 优先，否则 legacy 的
/// `codex_like` 布尔。与 `config_bundles.rs:100-108` 同序。
pub fn custom_session_family(profile: FamilyProfile<'_>) -> ProtocolFamily {
    parse_family(profile.family).unwrap_or(if profile.codex_like {
        ProtocolFamily::Codex
    } else {
        ProtocolFamily::Claude
    })
}

/// 两边都不认识的 agent。
pub fn unknown_session_family(agent: &str) -> ProtocolFamily {
    if agent == "claude" {
        ProtocolFamily::Claude
    } else {
        ProtocolFamily::Codex
    }
}

/// launch 家族：dsh/omp 直接由显式 family 决定；claude/codex 之间看
/// `codex_like`——`pty.rs:1625,1808` 就是拿这个 flag 在 `build_codex_cmd` 与
/// `build_claude_cmd` 之间二选一，flag 即家族。
/// `profile` 是 `get_agent_launch_spec(agent)` 的两个字段（内建 agent 由调用方
/// 传它的内建 spec 投影）。
pub fn launch_family(profile: FamilyProfile<'_>) -> ProtocolFamily {
    match parse_family(profile.family) {
        Some(ProtocolFamily::Dsh) => ProtocolFamily::Dsh,
        Some(ProtocolFamily::Omp) => ProtocolFamily::Omp,
        _ if profile.codex_like => ProtocolFamily::Codex,
        _ => ProtocolFamily::Claude,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                built_in_session_family(agent),
                Some(expected),
                "built-in {agent}"
            );
            assert!(is_builtin(agent), "{agent} must be built-in");
        }
        assert!(!is_builtin("my-codex"));
        // ADR-029 的那半边：格式 Claude、能力 codex。
        assert_eq!(
            built_in_native_resume_family("claude_gpt55"),
            Some(ProtocolFamily::Codex)
        );
        assert_eq!(
            built_in_native_resume_family("claude"),
            Some(ProtocolFamily::Claude)
        );
    }

    #[test]
    fn custom_profile_precedence_is_explicit_family_then_codex_like() {
        assert_eq!(
            custom_session_family(FamilyProfile {
                family: " dsh ",
                codex_like: true
            }),
            ProtocolFamily::Dsh,
            "显式 family 赢，且两侧空白被 trim（与宿主 parse 前一致）"
        );
        assert_eq!(
            custom_session_family(FamilyProfile {
                family: "",
                codex_like: true
            }),
            ProtocolFamily::Codex
        );
        assert_eq!(
            custom_session_family(FamilyProfile {
                family: "nonsense",
                codex_like: false
            }),
            ProtocolFamily::Claude,
            "认不出的 family 落 legacy 布尔"
        );
    }

    #[test]
    fn launch_family_reads_the_flag_for_claude_codex_and_names_for_dsh_omp() {
        assert_eq!(
            launch_family(FamilyProfile {
                family: "omp",
                codex_like: true
            }),
            ProtocolFamily::Omp,
            "omp 名字优先于 flag"
        );
        assert_eq!(
            launch_family(FamilyProfile {
                family: "claude",
                codex_like: true
            }),
            ProtocolFamily::Codex,
            "claude/codex 之间 flag 就是答案（pty.rs 二选一的镜像）"
        );
        assert_eq!(
            launch_family(FamilyProfile {
                family: "",
                codex_like: false
            }),
            ProtocolFamily::Claude
        );
    }

    #[test]
    fn unknown_agents_mirror_the_client_rule() {
        assert_eq!(unknown_session_family("claude"), ProtocolFamily::Claude);
        assert_eq!(
            unknown_session_family("anything-else"),
            ProtocolFamily::Codex
        );
    }
}
