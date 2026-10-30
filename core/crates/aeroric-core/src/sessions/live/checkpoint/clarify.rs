//! 开工前澄清 (B4′ design §3): the first turn only lists the requirements
//! it infers and proposes checkable acceptance criteria, as one JSON block;
//! the host turns that into a clarification card, and nothing runs until the
//! user accepts it.

use aeroric_protocol::checkpoint::Acceptance;
use serde::Deserialize;

/// The first line of a wrapped prompt; also how a session's first message is
/// recognised as a clarification turn.
pub const MARKER: &str = "[Aeroric 开工前澄清]";
const REQUEST_HEADING: &str = "用户的请求：\n";
/// The permission a clarification turn runs under (read-only, plan).
pub const PERMISSION: &str = "plan";
/// The record field that remembers the permission to restore on accept.
pub const RESTORE_FIELD: &str = "clarifyRestorePermission";

/// The user's request, wrapped with the clarification instructions.
pub fn wrap(request: &str) -> String {
    format!(
        "{MARKER}
先不要修改任何文件、不要运行有副作用的命令。只读必要的代码，然后只输出一个 JSON 代码块：
```json
{{\"requirements\": [\"你推断出、但用户没有明说的需求\"],
 \"acceptance\": [{{\"text\": \"可检查的验收条款\", \"check\": \"能核对它的一条 shell 命令，没有就省略\"}}],
 \"questions\": [\"必须由用户回答的问题，没有就留空\"]}}
```
验收条款要具体、可核对，3 到 7 条。用户确认后才会开工。
---
{REQUEST_HEADING}{request}"
    )
}

/// The user's own request inside a wrapped prompt; `None` when `text` is not one.
pub fn unwrap(text: &str) -> Option<&str> {
    if !text.starts_with(MARKER) {
        return None;
    }
    text.split_once(REQUEST_HEADING).map(|(_, request)| request)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Proposal {
    pub requirements: Vec<String>,
    pub acceptance: Vec<Acceptance>,
    pub questions: Vec<String>,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    acceptance: Vec<RawAcceptance>,
    #[serde(default)]
    questions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawAcceptance {
    Text(String),
    Item {
        text: String,
        #[serde(default)]
        check: Option<String>,
    },
}

/// The proposal in the turn's text: a ```json block, else the outermost
/// `{…}`. `None` when neither reads as one (the card then shows the text).
pub fn parse(text: &str) -> Option<Proposal> {
    let candidate = fenced(text).or_else(|| {
        let start = text.find('{')?;
        let end = text.rfind('}')?;
        (end > start).then(|| &text[start..=end])
    })?;
    let raw: Raw = serde_json::from_str(candidate).ok()?;
    let clean = |list: Vec<String>| -> Vec<String> {
        list.into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let acceptance: Vec<Acceptance> = raw
        .acceptance
        .into_iter()
        .filter_map(|a| {
            let (text, check) = match a {
                RawAcceptance::Text(text) => (text, None),
                RawAcceptance::Item { text, check } => (text, check),
            };
            let text = text.trim().to_string();
            let check = check
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty());
            (!text.is_empty()).then_some(Acceptance { text, check })
        })
        .collect();
    let proposal = Proposal {
        requirements: clean(raw.requirements),
        acceptance,
        questions: clean(raw.questions),
    };
    (!proposal.acceptance.is_empty() || !proposal.requirements.is_empty()).then_some(proposal)
}

fn fenced(text: &str) -> Option<&str> {
    let start = text.find("```json")? + "```json".len();
    let end = text[start..].find("```")? + start;
    Some(text[start..end].trim())
}

/// The message that starts the work once the user accepted.
pub fn start_message(acceptance: &[Acceptance], constraints: &[String]) -> String {
    let mut out = String::from("需求已确认，现在开始实施。\n");
    if !acceptance.is_empty() {
        out.push_str("\n验收条款（完成前逐条核对，并说明怎么核对的）：\n");
        for (i, a) in acceptance.iter().enumerate() {
            match &a.check {
                Some(check) => out.push_str(&format!("{}. {}（核对：`{check}`）\n", i + 1, a.text)),
                None => out.push_str(&format!("{}. {}\n", i + 1, a.text)),
            }
        }
    }
    if !constraints.is_empty() {
        out.push_str("\n约束（必须遵守）：\n");
        for c in constraints {
            out.push_str(&format!("- {c}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrapped_prompt_unwraps_to_the_request() {
        let wrapped = wrap("修复超时\n第二行");
        assert!(wrapped.starts_with(MARKER));
        assert_eq!(unwrap(&wrapped), Some("修复超时\n第二行"));
        assert_eq!(unwrap("修复超时"), None);
    }

    #[test]
    fn a_fenced_block_is_read_and_cleaned() {
        let text = "看完了。\n```json\n{\"requirements\": [\"超时可配置\", \" \"],\n \"acceptance\": [{\"text\": \"测试通过\", \"check\": \"cargo test\"}, \"文档更新\", {\"text\": \"\"}],\n \"questions\": []}\n```\n就这些。";
        let p = parse(text).unwrap();
        assert_eq!(p.requirements, ["超时可配置"]);
        assert_eq!(
            p.acceptance,
            [
                Acceptance {
                    text: "测试通过".into(),
                    check: Some("cargo test".into())
                },
                Acceptance {
                    text: "文档更新".into(),
                    check: None
                },
            ]
        );
    }

    #[test]
    fn a_bare_object_is_read_and_prose_is_not() {
        assert!(parse("{\"acceptance\": [\"能跑\"]}").is_some());
        assert!(parse("我建议先确认一下需求。").is_none());
        assert!(parse("{\"note\": 1}").is_none());
    }

    #[test]
    fn the_start_message_lists_criteria_and_constraints() {
        let msg = start_message(
            &[Acceptance {
                text: "测试通过".into(),
                check: Some("cargo test".into()),
            }],
            &["不要推送".into()],
        );
        assert!(msg.contains("1. 测试通过（核对：`cargo test`）"));
        assert!(msg.contains("- 不要推送"));
    }
}
