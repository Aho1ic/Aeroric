//! Nothing that looks like a key or token is written to memory (A17). The
//! patterns are the diagnostics report's, plus GitHub tokens.

use crate::maintenance::diagnostic_report;

/// What the text looked like, when it looks like a secret.
pub(super) fn looks_secret(text: &str) -> Option<&'static str> {
    if text.contains("ghp_") || text.contains("github_pat_") {
        return Some("GitHub 令牌");
    }
    if diagnostic_report::redact(text, None, &[]) != text {
        return Some(if text.contains("sk-") {
            "sk-…"
        } else {
            "令牌或口令"
        });
    }
    None
}
