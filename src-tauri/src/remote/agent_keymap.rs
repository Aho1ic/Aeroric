//! 审批动作 → agent TUI 按键序列映射。
//!
//! 独立成模块的原因(见 plan M3):Claude/Codex 的审批按键随 TUI 版本可能漂移,
//! 集中在这里便于调整;映射失败的兜底永远是手机切终端 tab 手工按键。
//!
//! 当前映射依据:
//! - Claude Code 权限对话:按数字选项立即生效(1 = Yes/允许),Esc = 拒绝并可补充说明。
//! - Codex 审批提示:y = 允许,n = 拒绝(如版本变化,可先用 action=keys 自定义序列过渡)。

pub(crate) struct KeyStep {
    /// 写入 PTY 的字节(UTF-8 文本,含控制字符)。
    pub data: String,
    /// 写入前等待毫秒数(部分 TUI 的菜单需要两次写入间隔,预留)。
    pub delay_ms: u64,
}

pub(crate) enum RespondAction {
    Approve,
    Deny,
    /// 手机端直接指定按键序列(兜底通道,不经映射)。
    Keys(String),
}

/// action 字符串 → RespondAction。`keys` 需要配套的序列参数。
pub(crate) fn parse_action(action: &str, keys: Option<&str>) -> Result<RespondAction, String> {
    match action {
        "approve" => Ok(RespondAction::Approve),
        "deny" => Ok(RespondAction::Deny),
        "keys" => {
            let keys = keys.unwrap_or("");
            if keys.is_empty() {
                return Err("Action 'keys' requires a non-empty keys param".to_string());
            }
            if keys.len() > 64 {
                return Err("Keys sequence too long (max 64 bytes)".to_string());
            }
            Ok(RespondAction::Keys(keys.to_string()))
        }
        other => Err(format!("Unknown respond action: {other}")),
    }
}

pub(crate) fn keys_for(codex_like: bool, action: RespondAction) -> Vec<KeyStep> {
    let step = |data: &str| KeyStep {
        data: data.to_string(),
        delay_ms: 0,
    };
    match (codex_like, action) {
        (_, RespondAction::Keys(keys)) => vec![KeyStep {
            data: keys,
            delay_ms: 0,
        }],
        (false, RespondAction::Approve) => vec![step("1")],
        (false, RespondAction::Deny) => vec![step("\x1b")],
        (true, RespondAction::Approve) => vec![step("y")],
        (true, RespondAction::Deny) => vec![step("n")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(codex_like: bool, action: RespondAction) -> String {
        keys_for(codex_like, action)
            .into_iter()
            .map(|s| s.data)
            .collect()
    }

    #[test]
    fn claude_mapping_uses_numeric_approve_and_esc_deny() {
        assert_eq!(joined(false, RespondAction::Approve), "1");
        assert_eq!(joined(false, RespondAction::Deny), "\x1b");
    }

    #[test]
    fn codex_mapping_uses_yes_no_keys() {
        assert_eq!(joined(true, RespondAction::Approve), "y");
        assert_eq!(joined(true, RespondAction::Deny), "n");
    }

    #[test]
    fn custom_keys_pass_through_unmapped() {
        assert_eq!(joined(true, RespondAction::Keys("2\r".into())), "2\r");
        assert_eq!(joined(false, RespondAction::Keys("a".into())), "a");
    }

    #[test]
    fn parse_action_validates_input() {
        assert!(matches!(
            parse_action("approve", None),
            Ok(RespondAction::Approve)
        ));
        assert!(matches!(
            parse_action("deny", None),
            Ok(RespondAction::Deny)
        ));
        assert!(matches!(
            parse_action("keys", Some("y\r")),
            Ok(RespondAction::Keys(_))
        ));
        assert!(parse_action("keys", None).is_err());
        assert!(parse_action("keys", Some("")).is_err());
        assert!(parse_action("keys", Some(&"x".repeat(65))).is_err());
        assert!(parse_action("nuke", None).is_err());
    }
}
