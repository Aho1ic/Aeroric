//! E5 (2026-11-19): what a turn did outside the machine — pushed, published,
//! deployed, sent or posted something — read from the agent's own
//! transcript, never from what the model says it did. A turn a host restart
//! cut that already acted outward is not run again on its own: doing it
//! twice (a second push, a second message) is worse than stopping.
//!
//! The rule is OpenMausBot's runtime contract §5 (no rerun once the host
//! says the turn acted outward), with the host's knowledge here being the
//! transcript's tool calls.

use super::transcript_stats::{ToolSpan, TurnStat};

/// Commands that act outside the machine, by what their line contains; the
/// label is what the user is told.
const COMMANDS: &[(&str, &str)] = &[
    ("git push", "git push"),
    ("gh pr create", "gh pr create"),
    ("gh pr merge", "gh pr merge"),
    ("gh pr comment", "gh pr comment"),
    ("gh pr review", "gh pr review"),
    ("gh pr close", "gh pr close"),
    ("gh pr edit", "gh pr edit"),
    ("gh issue create", "gh issue create"),
    ("gh issue comment", "gh issue comment"),
    ("gh issue close", "gh issue close"),
    ("gh issue edit", "gh issue edit"),
    ("gh release create", "gh release create"),
    ("gh workflow run", "gh workflow run"),
    ("npm publish", "npm publish"),
    ("pnpm publish", "pnpm publish"),
    ("yarn publish", "yarn publish"),
    ("yarn npm publish", "yarn npm publish"),
    ("cargo publish", "cargo publish"),
    ("twine upload", "twine upload"),
    ("gem push", "gem push"),
    ("pod trunk push", "pod trunk push"),
    ("docker push", "docker push"),
    ("helm push", "helm push"),
    ("kubectl apply", "kubectl apply"),
    ("kubectl delete", "kubectl delete"),
    ("kubectl rollout", "kubectl rollout"),
    ("terraform apply", "terraform apply"),
    ("terraform destroy", "terraform destroy"),
    ("vercel deploy", "vercel deploy"),
    ("vercel --prod", "vercel --prod"),
    ("netlify deploy", "netlify deploy"),
    ("fly deploy", "fly deploy"),
    ("firebase deploy", "firebase deploy"),
    ("wrangler deploy", "wrangler deploy"),
    ("wrangler publish", "wrangler publish"),
    ("sendmail", "sendmail"),
];

/// A `curl` or `gh api` that writes: a method other than GET, or a body.
const WRITE_FLAGS: &[&str] = &[
    "-X POST",
    "-X PUT",
    "-X PATCH",
    "-X DELETE",
    "-XPOST",
    "-XPUT",
    "-XPATCH",
    "-XDELETE",
    "--request POST",
    "--request PUT",
    "--request PATCH",
    "--request DELETE",
    "--method POST",
    "--method PUT",
    "--method PATCH",
    "--method DELETE",
    " -d ",
    " --data",
    " -F ",
    " --form",
    " -f ",
    " --field",
    " --raw-field",
];

/// The verbs of an MCP tool that writes somewhere else.
const MCP_VERBS: &[&str] = &[
    "send",
    "post",
    "publish",
    "merge",
    "comment",
    "reply",
    "push",
    "submit",
    "invite",
    "create_issue",
    "create_pull_request",
    "create_release",
    "create_event",
    "create_message",
];

/// What one tool call did outward, if anything.
pub fn classify(name: &str, detail: Option<&str>) -> Option<String> {
    if let Some(line) = detail {
        let line = line.trim();
        if let Some((_, label)) = COMMANDS.iter().find(|(p, _)| has_command(line, p)) {
            return Some((*label).to_string());
        }
        for tool in ["curl", "gh api"] {
            if has_command(line, tool) && WRITE_FLAGS.iter().any(|f| line.contains(f)) {
                return Some(format!("{tool}（写请求）"));
            }
        }
    }
    mcp(name)
}

/// `pattern` as a command of `line`: at its start or after a separator, so
/// `echo "git push"` is not a push but `cd x && git push` is.
fn has_command(line: &str, pattern: &str) -> bool {
    line.match_indices(pattern).any(|(at, _)| {
        let before = line[..at].trim_end();
        let after = line[at + pattern.len()..].chars().next();
        let starts = before.is_empty()
            || before.ends_with("&&")
            || before.ends_with("||")
            || before.ends_with(';')
            || before.ends_with('|')
            || before.ends_with('(')
            || before.ends_with("sudo")
            || before.ends_with("npx")
            || before.ends_with("bunx");
        starts && after.is_none_or(|c| c.is_whitespace() || c == ';')
    })
}

/// `mcp__server__tool` (Claude) or `server__tool`: a tool of another
/// server whose name says it sends or posts. Aeroric's own tools are not.
fn mcp(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split("__").collect();
    if parts.len() < 2 {
        return None;
    }
    let tool = parts[parts.len() - 1].to_ascii_lowercase();
    let server = parts[parts.len() - 2].to_ascii_lowercase();
    if server.contains("aeroric") {
        return None;
    }
    let words: Vec<&str> = tool.split(['_', '-']).collect();
    MCP_VERBS
        .iter()
        .any(|verb| {
            if verb.contains('_') {
                tool.contains(verb)
            } else {
                words.contains(verb)
            }
        })
        .then(|| format!("{server} · {}", parts[parts.len() - 1]))
}

/// One outward action of a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outward {
    pub label: String,
    /// The transcript has its result: it ran. `false`: it was still running
    /// when the turn was cut, so it may have.
    pub done: bool,
}

impl Outward {
    /// "git push" or "git push（被打断时还在执行）".
    pub fn text(&self) -> String {
        if self.done {
            self.label.clone()
        } else {
            format!("{}（被打断时还在执行）", self.label)
        }
    }
}

/// What the turn running at `cut` (the last one started by then) did
/// outward, each label once. Failed calls did nothing.
pub fn at_cut(turns: &[TurnStat], cut: u64) -> Vec<Outward> {
    let Some(turn) = turns.iter().rev().find(|t| t.started_at <= cut) else {
        return Vec::new();
    };
    of_turn(&turn.tools)
}

pub fn of_turn(tools: &[ToolSpan]) -> Vec<Outward> {
    let mut out: Vec<Outward> = Vec::new();
    for tool in tools.iter().filter(|t| !t.failed) {
        let Some(label) = classify(&tool.name, tool.detail.as_deref()) else {
            continue;
        };
        let done = tool.ended_at.is_some();
        match out.iter_mut().find(|o| o.label == label) {
            Some(seen) => seen.done |= done,
            None => out.push(Outward { label, done }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(name: &str, detail: &str, ended: bool, failed: bool) -> ToolSpan {
        ToolSpan {
            name: name.into(),
            detail: (!detail.is_empty()).then(|| detail.into()),
            started_at: 10,
            ended_at: ended.then_some(20),
            failed,
        }
    }

    #[test]
    fn commands_and_tools_that_act_outside_are_told_apart() {
        let outward = |name: &str, line: &str| classify(name, Some(line).filter(|l| !l.is_empty()));
        assert_eq!(
            outward("Bash", "git push origin main").as_deref(),
            Some("git push")
        );
        assert_eq!(
            outward("shell", "cd repo && git push -u origin x").as_deref(),
            Some("git push")
        );
        assert_eq!(outward("Bash", "echo \"git push\"").as_deref(), None);
        assert_eq!(
            outward("Bash", "git commit -m x").as_deref(),
            None,
            "a commit stays local"
        );
        assert_eq!(outward("Bash", "git pushd").as_deref(), None);
        assert_eq!(
            outward("Bash", "npx vercel --prod").as_deref(),
            Some("vercel --prod")
        );
        assert_eq!(outward("Bash", "gh pr view 12").as_deref(), None);
        assert_eq!(
            outward("Bash", "gh pr create --fill").as_deref(),
            Some("gh pr create")
        );
        assert_eq!(
            outward("Bash", "curl -s https://x/api").as_deref(),
            None,
            "a read"
        );
        assert_eq!(
            outward("Bash", "curl -X POST https://hooks.example/x").as_deref(),
            Some("curl（写请求）")
        );
        assert_eq!(
            outward("Bash", "gh api repos/o/r/issues -f title=x").as_deref(),
            Some("gh api（写请求）")
        );
        assert_eq!(
            outward("mcp__slack__send_message", "").as_deref(),
            Some("slack · send_message")
        );
        assert_eq!(
            outward("mcp__github__create_pull_request", "").as_deref(),
            Some("github · create_pull_request")
        );
        assert_eq!(outward("mcp__github__get_issue", ""), None);
        assert_eq!(
            outward("mcp__aeroric__board_write", ""),
            None,
            "Aeroric's own"
        );
        assert_eq!(outward("mcp__fs__create_directory", ""), None);
        assert_eq!(outward("Read", ""), None);
    }

    #[test]
    fn the_cut_turn_says_what_ran_and_what_may_have() {
        let turns = vec![
            TurnStat {
                started_at: 100,
                ended_at: Some(150),
                tools: vec![span("Bash", "git push", true, false)],
                ..TurnStat::default()
            },
            TurnStat {
                started_at: 200,
                tools: vec![
                    span("Bash", "cargo test", true, false),
                    span("Bash", "git push origin x", true, true),
                    span("Bash", "npm publish", false, false),
                    span("mcp__slack__post_message", "", true, false),
                    span("mcp__slack__post_message", "", true, false),
                ],
                ..TurnStat::default()
            },
        ];
        let cut = at_cut(&turns, 500);
        let texts: Vec<String> = cut.iter().map(Outward::text).collect();
        assert_eq!(
            texts,
            ["npm publish（被打断时还在执行）", "slack · post_message"],
            "the failed push did nothing; each label once"
        );
        // The earlier turn's push is not the cut turn's.
        assert!(at_cut(&turns, 50).is_empty());
        assert_eq!(at_cut(&turns, 160)[0].label, "git push");
    }
}
