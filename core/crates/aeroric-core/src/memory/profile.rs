//! The text a starting session is handed (Q-M4): the user's profile, then
//! the project's facts, pinned first, cut to `PROFILE_BUDGET_CHARS`.

use aeroric_protocol::memory::{MemoryCategory, MemoryItem, MemoryKind, PROFILE_BUDGET_CHARS};

pub const HEADER: &str = "# Aeroric 记忆（用户确认过的长期信息）";
/// What every profile starts with (H3: the context bill finds it by this).
pub const HEADER_PREFIX: &str = "# Aeroric 记忆";

fn category(item: &MemoryItem) -> &'static str {
    match item.category {
        Some(MemoryCategory::Language) => "[语言] ",
        Some(MemoryCategory::Interaction) => "[习惯] ",
        Some(MemoryCategory::Preference) => "[偏好] ",
        Some(MemoryCategory::Constraint) => "[限制] ",
        None => "",
    }
}

/// B6 second batch: some profile or fact item does not fit the budget (the
/// cue for 画像压缩).
pub(super) fn overflows(global: &[MemoryItem], project: &[MemoryItem]) -> bool {
    let shown = render(global, project).map_or(0, |(_, ids)| ids.len());
    let all = project
        .iter()
        .chain(global)
        .filter(|i| matches!(i.kind, MemoryKind::Profile | MemoryKind::Fact))
        .count();
    shown < all
}

/// E8 (2026-11-22): what a starting session is handed, measured — the
/// characters it uses against [`PROFILE_BUDGET_CHARS`], the ids carried and
/// the ids the budget left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub text: Option<String>,
    pub used_chars: usize,
    pub carried: Vec<String>,
    pub left_out: Vec<String>,
}

/// `global` and `project` are active items, pinned first then newest.
/// The text and the ids it carries (B5-3a counts them as injected).
pub(super) fn render(
    global: &[MemoryItem],
    project: &[MemoryItem],
) -> Option<(String, Vec<String>)> {
    let load = measure(global, project);
    load.text.map(|text| (text, load.carried))
}

/// [`render`], with what it left out and the characters it used.
pub(super) fn measure(global: &[MemoryItem], project: &[MemoryItem]) -> Load {
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
    // H7: when the budget runs out, the least used facts are the ones left
    // out (stable: newest first among equals).
    facts.sort_by_key(|item| {
        (
            !item.pinned,
            item.project_id.is_none(),
            std::cmp::Reverse(item.stats.helpful + item.stats.retrieved),
        )
    });
    if about.is_empty() && facts.is_empty() {
        return Load::default();
    }
    let mut out = HEADER.to_string();
    let mut ids = Vec::new();
    let mut used = out.chars().count();
    let mut left_ids = Vec::new();
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
                left_ids.push(item.id.clone());
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
    if !left_ids.is_empty() {
        out.push_str(&format!(
            "\n（还有 {} 条未列出，可用 memory_recall 检索）",
            left_ids.len()
        ));
    }
    Load {
        used_chars: out.chars().count(),
        text: Some(out),
        carried: ids,
        left_out: left_ids,
    }
}
