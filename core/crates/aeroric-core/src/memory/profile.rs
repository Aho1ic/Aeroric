//! The text a starting session is handed (Q-M4): the user's profile, then
//! the project's facts, pinned first, cut to `PROFILE_BUDGET_CHARS`.

use aeroric_protocol::memory::{MemoryCategory, MemoryItem, MemoryKind, PROFILE_BUDGET_CHARS};

pub const HEADER: &str = "# Aeroric 记忆（用户确认过的长期信息）";

fn category(item: &MemoryItem) -> &'static str {
    match item.category {
        Some(MemoryCategory::Language) => "[语言] ",
        Some(MemoryCategory::Interaction) => "[习惯] ",
        Some(MemoryCategory::Preference) => "[偏好] ",
        Some(MemoryCategory::Constraint) => "[限制] ",
        None => "",
    }
}

/// `global` and `project` are active items, pinned first then newest.
/// The text and the ids it carries (B5-3a counts them as injected).
pub(super) fn render(
    global: &[MemoryItem],
    project: &[MemoryItem],
) -> Option<(String, Vec<String>)> {
    let mut about: Vec<&MemoryItem> = project
        .iter()
        .chain(global)
        .filter(|item| item.kind == MemoryKind::Profile)
        .collect();
    about.sort_by_key(|item| !item.pinned);
    let mut facts: Vec<&MemoryItem> = project
        .iter()
        .chain(global)
        .filter(|item| item.kind == MemoryKind::Fact)
        .collect();
    facts.sort_by_key(|item| (!item.pinned, item.project_id.is_none()));
    if about.is_empty() && facts.is_empty() {
        return None;
    }
    let mut out = HEADER.to_string();
    let mut ids = Vec::new();
    let mut used = out.chars().count();
    let mut left = 0usize;
    for (title, items) in [("## 关于用户", &about), ("## 事实与约定", &facts)] {
        if items.is_empty() {
            continue;
        }
        let mut wrote_title = false;
        for item in items.iter() {
            let line = format!("\n- {}{}", category(item), item.text);
            let cost = line.chars().count()
                + if wrote_title {
                    0
                } else {
                    title.chars().count() + 1
                };
            if used + cost > PROFILE_BUDGET_CHARS {
                left += 1;
                continue;
            }
            if !wrote_title {
                out.push('\n');
                out.push_str(title);
                wrote_title = true;
            }
            out.push_str(&line);
            ids.push(item.id.clone());
            used += cost;
        }
    }
    if left > 0 {
        out.push_str(&format!(
            "\n（还有 {left} 条未列出，可用 memory_recall 检索）"
        ));
    }
    Some((out, ids))
}
