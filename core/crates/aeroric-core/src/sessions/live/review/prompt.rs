//! What the reviewer and the writer are told. The reviewer sees the turn's
//! diff, the accepted criteria and the constraints — never the writer's
//! conversation (Q-B4).

use aeroric_protocol::review::{Decision, Finding, ReviewCard, Severity};

use super::TurnDiff;

/// The first line of a reviewer prompt.
pub const REVIEW_MARKER: &str = "[Aeroric 审查]";
/// The first line of the findings sent back to the writer.
pub const REPLY_MARKER: &str = "[Aeroric 审查意见]";
/// A diff longer than this is not inlined; the reviewer gets the command.
pub const MAX_INLINE_DIFF: usize = 60_000;

pub struct Context<'a> {
    pub round: u32,
    pub acceptance: &'a [String],
    pub constraints: &'a [String],
    pub checks: Option<&'a str>,
    /// The diff text when it fits [`MAX_INLINE_DIFF`].
    pub diff_text: Option<&'a str>,
    /// The previous round, with the writer's answers.
    pub previous: Option<&'a ReviewCard>,
}

pub fn reviewer(diff: &TurnDiff, cx: &Context<'_>) -> String {
    let mut out = format!(
        "{REVIEW_MARKER} 第 {} 轮\n你是代码审查者。只读：不要修改任何文件，不要运行有副作用的命令。你看不到写代码的会话的对话，只根据下面的改动和要求审查。\n",
        cx.round
    );
    list(&mut out, "验收条款", cx.acceptance);
    list(&mut out, "约束", cx.constraints);
    if let Some(checks) = cx.checks {
        out.push_str(&format!("\n## 自动检查\n{checks}\n"));
    }
    if let Some(previous) = cx.previous {
        out.push_str("\n## 上一轮你的意见与对方的答复\n");
        for f in &previous.findings {
            out.push_str(&format!("- {}：{}", line(f), answer(f)));
            out.push('\n');
        }
        out.push_str("这一轮只看新的改动，以及上一轮被采纳但还没改好的问题；被驳回的意见不要重复提，除非新改动让它更严重。\n");
    }
    let git = format!(
        "git -C {} diff {} {}",
        shell_quote(&diff.top.to_string_lossy()),
        diff.before,
        diff.after
    );
    out.push_str(&format!(
        "\n## 改动（{} 个文件，+{} −{}）\n",
        diff.files.len(),
        diff.additions,
        diff.deletions
    ));
    match cx.diff_text {
        Some(text) => out.push_str(&format!("```diff\n{text}\n```\n")),
        None => {
            for f in &diff.files {
                out.push_str(&format!("- {}\n", f.path));
            }
            out.push_str(&format!(
                "改动太长没有内联。用 `{git} -- <文件>` 查看每个文件。\n"
            ));
        }
    }
    out.push_str(&format!(
        "（完整改动也可以用 `{git}` 查看。）\n
最后只输出一个 JSON 代码块：
```json
{{\"verdict\": \"pass 或 changes\", \"summary\": \"一句话总评\",
 \"findings\": [{{\"id\": \"r1\", \"severity\": \"blocker|major|minor\", \"file\": \"路径\", \"line\": 12, \"text\": \"问题与修改建议\"}}]}}
```
blocker 只用于必须改的问题：错误、数据丢失、安全、违反验收条款或约束。没有问题时 verdict 为 pass、findings 为空。\n"
    ));
    out
}

pub fn reply_request(card: &ReviewCard) -> String {
    let mut out = format!(
        "{REPLY_MARKER} 第 {} 轮 · 审查者 {}\n另一个 Agent 只读审查了你上一回合的改动，意见如下。逐条判断：\n- 采纳（adopt）：照着改；\n- 驳回（reject）：你有更完整的上下文、认为不该改时，写清理由；\n- 需要用户定（ask）：涉及取舍或超出用户要求时。\n不要做超出这些意见和用户原始要求的改动。\n\n",
        card.round, card.reviewer_label
    );
    for (i, f) in card.findings.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, line(f)));
    }
    out.push_str(
        "\n处理完后，在回答末尾给一个 JSON 代码块：
```json
{\"responses\": [{\"id\": \"r1\", \"decision\": \"adopt|reject|ask\", \"reason\": \"驳回或待定时写理由\"}]}
```\n",
    );
    out
}

fn line(f: &Finding) -> String {
    let at = match (&f.file, f.line) {
        (Some(file), Some(line)) => format!(" {file}:{line}"),
        (Some(file), None) => format!(" {file}"),
        _ => String::new(),
    };
    format!("[{} · {}]{at} — {}", f.id, severity(f.severity), f.text)
}

pub fn severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Blocker => "阻断",
        Severity::Major => "重要",
        Severity::Minor => "次要",
    }
}

fn answer(f: &Finding) -> String {
    let word = match f.decision {
        Some(Decision::Adopt) => "采纳",
        Some(Decision::Reject) => "驳回",
        Some(Decision::Ask) => "交给用户",
        None => "未答复",
    };
    match &f.reason {
        Some(reason) => format!("{word}（{reason}）"),
        None => word.to_string(),
    }
}

fn list(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {title}\n"));
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
}

fn shell_quote(path: &str) -> String {
    if path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    {
        path.to_string()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}
