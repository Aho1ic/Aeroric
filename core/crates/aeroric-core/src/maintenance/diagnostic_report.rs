//! A13: the redacted diagnostics report a user can export and share.
//!
//! What goes in is chosen by the host (build, capabilities, counts, the host
//! log tail); this module owns the two rules every byte passes through before
//! it leaves the machine: secrets are masked, and the user's home path is
//! shown as `~`. Prompts, messages and credentials are never collected.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

pub const MASK: &str = "****";

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // Aeroric gateway task tokens.
            r"aeroric-task-[0-9a-fA-F]{16,}",
            // OpenAI/Anthropic-style keys.
            r"\bsk-[A-Za-z0-9_\-]{8,}",
            // Authorization header values.
            r"(?i)\bbearer\s+[A-Za-z0-9._\-~+/=]{8,}",
            // `key=value` / `"key": "value"` for secret-looking keys.
            r#"(?i)("?(?:api[_-]?key|token|password|secret|authorization)"?\s*[:=]\s*"?)[^"\s,}]{4,}"#,
        ]
        .iter()
        .map(|p| Regex::new(p).expect("diagnostic redaction patterns are valid"))
        .collect()
    })
}

/// Mask known secrets and secret-shaped values, and show `home` as `~`.
pub fn redact(text: &str, home: Option<&Path>, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets.iter().filter(|s| s.len() >= 4) {
        out = out.replace(secret.as_str(), MASK);
    }
    for (index, pattern) in patterns().iter().enumerate() {
        out = if index == 3 {
            pattern
                .replace_all(&out, format!("${{1}}{MASK}"))
                .into_owned()
        } else {
            pattern.replace_all(&out, MASK).into_owned()
        };
    }
    if let Some(home) = home.map(|h| h.to_string_lossy().into_owned()) {
        if home.len() > 1 {
            out = out.replace(&home, "~");
        }
    }
    out
}

/// The last `lines` lines of `text`.
pub fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// One titled section of the report.
pub struct Section {
    pub title: String,
    pub body: String,
}

/// Markdown report: a heading per section, bodies in fenced blocks, every
/// byte redacted.
pub fn render(
    title: &str,
    sections: &[Section],
    home: Option<&Path>,
    secrets: &[String],
) -> String {
    let mut out = format!("# {title}\n\n");
    out.push_str(
        "> 已脱敏：凭据与令牌替换为 `****`，家目录显示为 `~`；不含提示词、对话内容与凭据。\n",
    );
    for section in sections {
        out.push_str(&format!(
            "\n## {}\n\n```\n{}\n```\n",
            section.title,
            section.body.trim_end()
        ));
    }
    redact(&out, home, secrets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_tokens_and_home_are_masked() {
        let text = concat!(
            "token aeroric-task-0123456789abcdef0123456789abcdef in /Users/me/Library/x\n",
            "Authorization: Bearer abcdefgh12345678\n",
            "key sk-ant-REALKEY123456 and {\"api_key\": \"plainsecret\"}\n",
            "registered host-token-value-xyz\n"
        );
        let out = redact(
            text,
            Some(Path::new("/Users/me")),
            &["host-token-value-xyz".to_string()],
        );
        for leaked in [
            "0123456789abcdef",
            "abcdefgh12345678",
            "REALKEY123456",
            "plainsecret",
            "host-token-value-xyz",
            "/Users/me",
        ] {
            assert!(!out.contains(leaked), "{leaked} leaked: {out}");
        }
        assert!(out.contains("~/Library/x"), "{out}");
        assert!(out.contains("\"api_key\": \"****"), "{out}");
    }

    #[test]
    fn short_or_empty_secrets_do_not_wipe_the_text() {
        let out = redact("abc def", None, &["".into(), "a".into()]);
        assert_eq!(out, "abc def");
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        assert_eq!(tail("1\n2\n3\n4", 2), "3\n4");
        assert_eq!(tail("1", 5), "1");
    }

    #[test]
    fn render_redacts_every_section() {
        let report = render(
            "诊断",
            &[Section {
                title: "日志".into(),
                body: "Bearer zzzzzzzzzzzz at /Users/me/x".into(),
            }],
            Some(Path::new("/Users/me")),
            &[],
        );
        assert!(report.contains("## 日志"));
        assert!(!report.contains("zzzzzzzzzzzz") && !report.contains("/Users/me"));
    }
}
