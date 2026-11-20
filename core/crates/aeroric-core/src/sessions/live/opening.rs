//! H3 (2026-11-20): the opening a starting session is handed, the same on
//! every harness — what Aeroric offers and what for, the user's memory
//! profile, and where to look (Wiki page titles, role names). Each part has
//! a budget and the whole a cap, so it never crowds the session.
//!
//! The purpose says what the tools are for, not "always check first": a
//! rule the model is told to follow every time is the kind of injection
//! the harness note keeps out until H8 shows it helps.

use super::tool_catalog::Offered;

/// The purpose part's heading; the context bill files it under `instructions`.
pub const PURPOSE_HEADER: &str = "# Aeroric 能做什么";
/// The pointers part's heading; also `instructions` in the bill.
pub const POINTERS_HEADER: &str = "# Aeroric 里能查的";
pub const PURPOSE_BUDGET_CHARS: usize = 480;
pub const POINTERS_BUDGET_CHARS: usize = 600;
/// The memory profile keeps its own budget (`PROFILE_BUDGET_CHARS`); the
/// whole stays under this, the pointers giving way first.
pub const OPENING_BUDGET_CHARS: usize =
    aeroric_protocol::memory::PROFILE_BUDGET_CHARS + PURPOSE_BUDGET_CHARS + POINTERS_BUDGET_CHARS;

/// What the session's context tools are for — one sentence, shared with the
/// context server's MCP `instructions` (Codex shows the model only these
/// and loads a tool family on their word, R4-3), so the two never differ.
pub fn tool_uses(offered: Offered) -> String {
    let mut parts = vec![
        "检索本项目的历史会话",
        "读取当前任务的交接清单",
        "把工作委派给子会话",
        "把本轮范围以外、值得以后做的事提议成用户的待办（todo_propose）",
        "读取任务交付的文件及其各版本（artifact_list、artifact_read）",
    ];
    if offered.memory {
        parts.push("查用户的长期记忆与本项目约定");
    }
    if offered.wiki {
        parts.push("读、搜本项目 Wiki（模块职责与常见坑、设计决定与理由；先读 Wiki 再翻代码更快）");
    }
    if offered.references {
        parts.push("搜、读用户收集的参考代码库（别的仓库怎么实现同类功能）");
    }
    if offered.expert {
        parts.push("遇到难点时请教一个更强的专家模型（ask_expert）");
    }
    format!("Aeroric 上下文工具：{}。", parts.join("、"))
}

/// What Aeroric is and what its tools are for; `with_tools` false for a
/// session that was not given them (a remote one).
pub fn purpose(with_tools: bool, offered: Offered) -> String {
    let what = "Aeroric 是启动这个会话的应用：它记着这个项目以前的会话、用户确认过的长期记忆和项目 Wiki，也管着多个 Agent 共用的黑板和委派。";
    let how = if with_tools {
        format!(
            "需要以前的讨论、项目约定、设计决定，或者想把一块工作交给别人时，可以用它的工具去查、去做；不需要时不必调用。{}",
            tool_uses(offered)
        )
    } else {
        "这个会话没有接入它的工具；需要以前的讨论或项目约定时，请用户在 Aeroric 里查看。"
            .to_string()
    };
    cut(
        &format!("{PURPOSE_HEADER}\n{what}{how}"),
        PURPOSE_BUDGET_CHARS,
    )
}

/// Where to look: the Wiki's page titles and the role library's names,
/// titles only; `None` when there is nothing to point at.
pub fn pointers(wiki_titles: &[String], roles: &[String]) -> Option<String> {
    let mut lines = Vec::new();
    if !wiki_titles.is_empty() {
        lines.push(("项目 Wiki 页（read_wiki 按标题读）：", wiki_titles));
    }
    if !roles.is_empty() {
        lines.push(("委派角色（delegate 时可选）：", roles));
    }
    if lines.is_empty() {
        return None;
    }
    let mut out = POINTERS_HEADER.to_string();
    // Each line gets its share of the budget; what does not fit is counted.
    let share = (POINTERS_BUDGET_CHARS - POINTERS_HEADER.chars().count()) / lines.len();
    for (label, names) in lines {
        out.push('\n');
        out.push_str(&listed(label, names, share));
    }
    Some(out)
}

/// `label` and as many `names` as fit in `budget` characters, then
/// "等 N 个" for the rest.
fn listed(label: &str, names: &[String], budget: usize) -> String {
    let mut line = label.to_string();
    for (at, name) in names.iter().enumerate() {
        let sep = if at == 0 { "" } else { "、" };
        let rest = names.len() - at;
        let tail = format!("等 {rest} 个");
        if line.chars().count()
            + sep.chars().count()
            + name.chars().count()
            + 1
            + tail.chars().count()
            > budget
            && at > 0
        {
            line.push_str(&format!("{sep}{tail}"));
            return line;
        }
        line.push_str(sep);
        line.push_str(name);
    }
    line
}

/// The opening: purpose, then the profile, then the pointers, each a
/// section of its own; the pointers go first when over the cap.
pub fn compose(purpose: &str, profile: Option<&str>, pointers: Option<&str>) -> String {
    let mut parts: Vec<&str> = vec![purpose];
    parts.extend(profile);
    let without = parts.join("\n\n");
    match pointers {
        Some(p) if without.chars().count() + 2 + p.chars().count() <= OPENING_BUDGET_CHARS => {
            format!("{without}\n\n{p}")
        }
        _ => cut(&without, OPENING_BUDGET_CHARS),
    }
}

/// H3 (2026-11-20): a harness with no system layer (DSH) gets the opening
/// in front of its first message, in an envelope (`envelope.rs`) the App
/// shows as "Aeroric 附加的上下文", not as the user's words. The body is
/// escaped, so nothing in it can close the envelope early.
pub const ATTACHED_MARKER: &str = "[Aeroric 附加的上下文]";
pub const ATTACHED_END: &str = "[Aeroric 附加的上下文结束]";

/// `context` in its envelope, then `prompt`.
pub fn attach(context: &str, prompt: &str) -> String {
    attach_because(
        context,
        prompt,
        "这个 Agent 没有系统层，开场内容放在第一条消息前",
    )
}

/// [`attach`] with its own reason (H4: what was found for this message).
pub fn attach_because(context: &str, prompt: &str, why: &str) -> String {
    let opened = super::envelope::wrap(
        ATTACHED_MARKER,
        "Aeroric",
        why,
        super::envelope::Authority::Background,
        context,
    );
    format!("{}\n{ATTACHED_END}\n\n{prompt}", opened.trim_end())
}

/// The envelope's body and the message after it, for a message that
/// starts with one.
pub fn split_attached(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix(ATTACHED_MARKER)?;
    let body_start = rest.find('\n')? + 1;
    let end = rest.find(&format!("\n{ATTACHED_END}"))?;
    let body = rest.get(body_start..end).unwrap_or_default();
    let after = &rest[end + 1 + ATTACHED_END.len()..];
    Some((body, after.trim_start_matches('\n')))
}

/// H3 (2026-11-20): the context bill's split of a text that holds the
/// opening (`text` from [`PURPOSE_HEADER`] on): `(kind, chars)` per part in
/// the bill's kinds — the purpose and the pointers are `instructions`, the
/// profile `memory`, a handoff brief after the `---` `other`. The chars add
/// up to the whole text, separators going with the part before them.
pub fn bill_parts(text: &str) -> Vec<(&'static str, usize)> {
    const BRIEF: &str = "\n\n---\n\n";
    let (opening, brief) = match text.find(BRIEF) {
        Some(at) => (&text[..at + BRIEF.len()], Some(&text[at + BRIEF.len()..])),
        None => (text, None),
    };
    let mut parts: Vec<(&'static str, usize)> = Vec::new();
    let mut rest = opening;
    while !rest.is_empty() {
        // A section runs to the next "\n\n# " heading.
        let from = rest.chars().next().map_or(rest.len(), char::len_utf8);
        let end = rest[from..]
            .find("\n\n# ")
            .map(|at| from + at + 2)
            .unwrap_or(rest.len());
        let (section, tail) = rest.split_at(end);
        let kind = if section.starts_with(crate::memory::profile::HEADER_PREFIX) {
            "memory"
        } else {
            "instructions"
        };
        match parts.last_mut() {
            Some((last, chars)) if *last == kind => *chars += section.chars().count(),
            _ => parts.push((kind, section.chars().count())),
        }
        rest = tail;
    }
    if let Some(brief) = brief.filter(|b| !b.is_empty()) {
        parts.push(("other", brief.chars().count()));
    }
    parts
}

fn cut(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let mut out: String = text.chars().take(budget.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;
