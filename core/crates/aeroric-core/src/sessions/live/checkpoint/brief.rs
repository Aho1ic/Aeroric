//! The handoff brief: a checkpoint rendered for the successor, in a fixed
//! shape. Every line says how it is known (已验证 / 推断 / 未知); nothing here
//! calls a model, so the brief never claims more than the session showed.

use aeroric_protocol::checkpoint::{
    Checkpoint, Epistemic, HandoffReason, IntentState, ObligationState,
};

fn tag(status: Epistemic) -> &'static str {
    match status {
        Epistemic::Verified => "已验证",
        Epistemic::Inferred => "推断",
        Epistemic::Unknown => "未知",
    }
}

fn obligation_tag(state: ObligationState) -> &'static str {
    match state {
        ObligationState::Open => "待做",
        ObligationState::Passed => "已通过",
        ObligationState::Failed => "未通过",
        ObligationState::Unknown => "未知",
    }
}

/// The full brief given to a new session (system layer where the harness
/// takes one, else in front of its first message).
pub fn handoff_brief(cp: &Checkpoint, reason: HandoffReason) -> String {
    let mut out = format!(
        "# 交接清单（Aeroric · {} · 已结束 {} 个回合）\n\n这是你接手的任务此前的进展，由 Aeroric 根据会话记录整理，不是模型总结。每条后面的标签说明它是怎么知道的。\n",
        reason.label(),
        cp.turn
    );
    if let Some(goal) = &cp.goal {
        out.push_str(&format!(
            "\n## 目标\n- {}（{}）\n",
            goal.text,
            tag(goal.status)
        ));
    }
    if !cp.decisions.is_empty() {
        out.push_str("\n## 已确认决定\n");
        for d in &cp.decisions {
            out.push_str(&format!("- {}（{}）\n", d.text, tag(d.status)));
        }
    }
    if !cp.effects.is_empty() {
        out.push_str("\n## 已发生改动（不要重做）\n");
        if cp.effects_folded > 0 {
            out.push_str(&format!(
                "- 另有 {} 条较早的改动未列出\n",
                cp.effects_folded
            ));
        }
        for e in &cp.effects {
            out.push_str(&format!(
                "- 回合 {}：{}（{}）\n",
                e.turn,
                e.summary,
                tag(e.status)
            ));
        }
    }
    let unknown_intents: Vec<_> = cp
        .intents
        .iter()
        .filter(|i| i.state == IntentState::OutcomeUnknown)
        .collect();
    if !cp.unknown.is_empty() || !unknown_intents.is_empty() {
        out.push_str("\n## 结果未知（先核对，再决定是否重做）\n");
        for e in &cp.unknown {
            out.push_str(&format!("- 回合 {}：{}\n", e.turn, e.summary));
        }
        for i in unknown_intents {
            out.push_str(&format!(
                "- 已批准但没有结果：{}（核对方式：{}）\n",
                i.summary, i.verify
            ));
        }
    }
    let denied: Vec<_> = cp
        .intents
        .iter()
        .filter(|i| i.state == IntentState::Denied)
        .collect();
    if !denied.is_empty() {
        out.push_str("\n## 用户拒绝过的操作（不要再试）\n");
        for i in denied {
            out.push_str(&format!("- {}\n", i.summary));
        }
    }
    if !cp.obligations.is_empty() {
        out.push_str("\n## 未完成义务\n");
        for o in &cp.obligations {
            let check = o
                .check
                .as_deref()
                .map(|c| format!("，核销命令：`{c}`"))
                .unwrap_or_default();
            out.push_str(&format!(
                "- [{}] {}{}\n",
                obligation_tag(o.state),
                o.text,
                check
            ));
        }
    }
    if !cp.constraints.is_empty() {
        out.push_str("\n## 约束（必须遵守）\n");
        for c in &cp.constraints {
            out.push_str(&format!("- {}\n", c.text));
        }
    }
    if !cp.branches.is_empty() {
        out.push_str("\n## 侧支结论\n");
        for b in &cp.branches {
            out.push_str(&format!("- {}：{}\n", b.title, b.summary));
        }
    }
    out.push_str("\n已有证据的改动不要重做；宣告完成前，逐条核销「未完成义务」，并说明每条是怎么核对的。完整对话可用 Aeroric 上下文工具 read_session 查看。\n");
    out
}

/// A short reminder put in front of the next message when the session itself
/// stays the same (a model switched in place, after `/compact`). `None` when
/// nothing is unknown or open: the session's own context is enough then.
pub fn status_reminder(cp: &Checkpoint) -> Option<String> {
    if !cp.needs_attention() {
        return None;
    }
    let mut out = String::from("[Aeroric 状态提醒]\n");
    for e in &cp.unknown {
        out.push_str(&format!("- 结果未知，先核对：{}\n", e.summary));
    }
    for i in cp
        .intents
        .iter()
        .filter(|i| i.state == IntentState::OutcomeUnknown)
    {
        out.push_str(&format!("- 已批准但没有结果：{}\n", i.summary));
    }
    for o in cp
        .obligations
        .iter()
        .filter(|o| matches!(o.state, ObligationState::Open | ObligationState::Unknown))
    {
        out.push_str(&format!("- 未完成：{}\n", o.text));
    }
    for c in &cp.constraints {
        out.push_str(&format!("- 约束：{}\n", c.text));
    }
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::session_live::fixture_checkpoint;

    #[test]
    fn the_brief_lists_each_section_with_how_it_is_known() {
        let brief = handoff_brief(&fixture_checkpoint(), HandoffReason::SourceMove);
        let expected = "# 交接清单（Aeroric · 换来源 · 已结束 2 个回合）\n\n这是你接手的任务此前的进展，由 Aeroric 根据会话记录整理，不是模型总结。每条后面的标签说明它是怎么知道的。\n\n## 目标\n- 修复搜索客户端超时（已验证）\n\n## 已发生改动（不要重做）\n- 回合 2：src/search.rs +18 −4（已验证）\n\n## 结果未知（先核对，再决定是否重做）\n- 回合 2：cargo test search_timeout\n\n## 未完成义务\n- [待做] 回归测试覆盖 30s 边界\n\n已有证据的改动不要重做；宣告完成前，逐条核销「未完成义务」，并说明每条是怎么核对的。完整对话可用 Aeroric 上下文工具 read_session 查看。\n";
        assert_eq!(brief, expected);
    }

    #[test]
    fn a_settled_checkpoint_needs_no_reminder() {
        let mut cp = fixture_checkpoint();
        assert!(status_reminder(&cp)
            .unwrap()
            .contains("结果未知，先核对：cargo test search_timeout"));
        cp.unknown.clear();
        cp.obligations[0].state = ObligationState::Passed;
        assert!(status_reminder(&cp).is_none());
    }
}
