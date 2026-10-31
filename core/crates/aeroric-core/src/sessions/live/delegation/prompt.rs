//! What the child is told, and what goes back to the parent.

use aeroric_protocol::checkpoint::ObligationState;
use aeroric_protocol::delegation::{DelegationCard, DelegationState, Resolved, Workspace};

pub const DELEGATE_MARKER: &str = "[Aeroric 委派]";
pub const REDO_MARKER: &str = "[Aeroric 退回重做]";
pub const RESULT_MARKER: &str = "[Aeroric 委派结果]";

/// The child's first message. `brief` is the parent's handoff brief (brief
/// mode only); `role_text` the role's standing instruction.
pub fn child(
    contract: &Resolved,
    role_name: Option<&str>,
    role_text: &str,
    brief: Option<&str>,
) -> String {
    let mut out = format!(
        "{DELEGATE_MARKER} 你是主会话委派出来的子会话{}。完成后用一段话汇报：结论、依据、改了什么。不要再委派别的会话。\n",
        role_name.map(|n| format!("（角色：{n}）")).unwrap_or_default()
    );
    if let Some(brief) = brief.filter(|b| !b.trim().is_empty()) {
        out.push_str("\n## 主会话的交接清单\n");
        out.push_str(brief.trim());
        out.push('\n');
    }
    out.push_str("\n## 任务\n");
    if !role_text.trim().is_empty() {
        out.push_str(role_text.trim());
        out.push('\n');
    }
    out.push_str(contract.instruction.trim());
    out.push('\n');
    if !contract.context_refs.is_empty() {
        out.push_str("\n## 先读这些文件\n");
        for path in &contract.context_refs {
            out.push_str(&format!("- {path}\n"));
        }
    }
    out.push_str("\n## 验收条款\n");
    if contract.acceptance.is_empty() {
        out.push_str("（没有可执行的验收条款：回答里必须写清你的依据，否则主会话无法核对。）\n");
    } else {
        for a in &contract.acceptance {
            match &a.check {
                Some(check) => out.push_str(&format!("- {}（核对：`{check}`）\n", a.text)),
                None => out.push_str(&format!("- {}\n", a.text)),
            }
        }
    }
    out.push_str("\n## 边界\n");
    out.push_str(match contract.workspace {
        Workspace::ReadOnly => "- 只读：不要修改任何文件。\n",
        Workspace::Shared => "- 你和主会话共用工作目录；只改任务需要的文件。\n",
    });
    out.push_str(&format!("- 权限档：{}\n", contract.permission));
    out.push_str("- 共享黑板：用 board_read 看主会话和其他子会话记下的目标、假设、决定和问题，用 board_write 记下你的发现和证据；你写的决定只是提议。\n");
    out
}

pub fn redo(note: &str) -> String {
    let note = note.trim();
    if note.is_empty() {
        format!("{REDO_MARKER} 主会话认为结果还不够，请重新检查验收条款后再汇报一次。")
    } else {
        format!("{REDO_MARKER} {note}\n\n改完后按原格式再汇报一次。")
    }
}

fn state_text(card: &DelegationCard) -> &'static str {
    match card.state {
        DelegationState::Reported => "已回报",
        DelegationState::Failed => "失败",
        DelegationState::OverBudget => "超出预算，已打断",
        DelegationState::Stopped => "已停止",
        DelegationState::Interrupted => "被中断",
        _ => "进行中",
    }
}

/// The message that brings results back to the parent.
pub fn results(cards: &[DelegationCard]) -> String {
    let mut out = if cards.len() == 1 {
        format!("{RESULT_MARKER} 你委派的工作回来了。")
    } else {
        format!("{RESULT_MARKER} 你委派的 {} 项工作都回来了。", cards.len())
    };
    out.push_str("结论是子会话的说法，不是已核实的事实；有验收结果的以验收为准。\n");
    for (i, card) in cards.iter().enumerate() {
        let who = match &card.role_name {
            Some(role) => format!("{role} · {}", card.harness_label),
            None => card.harness_label.clone(),
        };
        out.push_str(&format!("\n### {}. {who}：{}\n", i + 1, state_text(card)));
        out.push_str(&format!("指令：{}\n", card.contract.instruction.trim()));
        if card.files > 0 {
            out.push_str(&format!(
                "改动：{} 个文件 +{} −{}\n",
                card.files, card.additions, card.deletions
            ));
        }
        if !card.acceptance.is_empty() {
            let passed = card
                .acceptance
                .iter()
                .filter(|a| a.state == ObligationState::Passed)
                .count();
            out.push_str(&format!("验收：{passed}/{} 通过\n", card.acceptance.len()));
            for a in &card.acceptance {
                let mark = match a.state {
                    ObligationState::Passed => "通过",
                    ObligationState::Failed => "未通过",
                    _ => "未核对",
                };
                out.push_str(&format!("- {}：{mark}\n", a.text));
            }
        }
        if !card.evidence {
            out.push_str("（没有可核对的证据：既没有验收结果，也没有改动）\n");
        }
        if let Some(error) = &card.error {
            out.push_str(&format!("原因：{error}\n"));
        }
        if let Some(result) = &card.result {
            out.push_str("结论：\n");
            out.push_str(result.trim());
            out.push('\n');
        }
        if let Some(child) = &card.child_task_id {
            out.push_str(&format!("（子会话 {child}，可用 read_session 读全文）\n"));
        }
    }
    out
}
