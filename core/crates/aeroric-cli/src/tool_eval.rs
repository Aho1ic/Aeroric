//! C8 third step (2026-11-20): the trigger evaluation of the context tools.
//!
//! `evals/tool-triggers.json` holds, per tool, requests that should make an
//! agent call it and requests that should not (with what to use instead).
//! A run hands each request to a harness, which sees the tools' names and
//! descriptions and picks one or none; the pick is then called through the
//! real server over a pipe, as an agent would. Here the harness is a fake —
//! a keyword matcher — which proves the flow and gives a baseline; a run
//! with a real harness (a model choosing) is NOT RUN, see the note
//! `2026-11-20-c8-tool-trigger-eval`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Deserialize)]
pub struct EvalSet {
    pub version: u32,
    pub tools: BTreeMap<String, ToolCases>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCases {
    pub should: Vec<String>,
    pub should_not: Vec<Avoid>,
}

#[derive(Deserialize)]
pub struct Avoid {
    pub prompt: String,
    /// The tool to use instead, or `none`.
    pub instead: String,
}

pub fn load() -> EvalSet {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals/tool-triggers.json");
    serde_json::from_slice(&std::fs::read(path).expect("eval set")).expect("eval set parses")
}

/// One request and what should be called for it (`None`: no Aeroric tool).
pub struct Case {
    pub tool: String,
    pub prompt: String,
    pub expected: Option<String>,
    pub should: bool,
}

pub fn cases(set: &EvalSet) -> Vec<Case> {
    let mut out = Vec::new();
    for (tool, cases) in &set.tools {
        for prompt in &cases.should {
            out.push(Case {
                tool: tool.clone(),
                prompt: prompt.clone(),
                expected: Some(tool.clone()),
                should: true,
            });
        }
        for avoid in &cases.should_not {
            out.push(Case {
                tool: tool.clone(),
                prompt: avoid.prompt.clone(),
                expected: (avoid.instead != "none").then(|| avoid.instead.clone()),
                should: false,
            });
        }
    }
    out
}

/// What decides, for one request, which tool (if any) to call.
pub trait Harness {
    fn choose(&mut self, prompt: &str, tools: &[Value]) -> Option<String>;
}

/// The fake: scores each tool by how many of the request's words (Latin
/// words, Chinese character pairs) its name and description share, and
/// calls the best one above a floor. A baseline, not a model.
pub struct KeywordHarness {
    pub floor: usize,
}

fn words(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut latin = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_alphanumeric() || *c == '_' {
            latin.push(c.to_ascii_lowercase());
            continue;
        }
        if latin.len() > 2 {
            out.insert(std::mem::take(&mut latin));
        }
        latin.clear();
        if ('\u{4e00}'..='\u{9fff}').contains(c) {
            if let Some(next) = chars
                .get(i + 1)
                .filter(|n| ('\u{4e00}'..='\u{9fff}').contains(*n))
            {
                out.insert(format!("{c}{next}"));
            }
        }
    }
    if latin.len() > 2 {
        out.insert(latin);
    }
    out
}

impl Harness for KeywordHarness {
    fn choose(&mut self, prompt: &str, tools: &[Value]) -> Option<String> {
        let asked = words(prompt);
        tools
            .iter()
            .filter_map(|tool| {
                let name = tool["name"].as_str()?;
                if prompt.contains(name) {
                    return Some((usize::MAX, name.to_string()));
                }
                let known = words(&format!(
                    "{} {}",
                    name.replace('_', " "),
                    tool["description"].as_str()?
                ));
                Some((asked.intersection(&known).count(), name.to_string()))
            })
            .filter(|(score, _)| *score >= self.floor)
            .max_by_key(|(score, _)| *score)
            .map(|(_, name)| name)
    }
}

/// One request's outcome.
pub struct Outcome {
    pub case: Case,
    pub chosen: Option<String>,
    /// The server's `structuredContent.status` for the call made, if any.
    pub status: Option<String>,
}

/// How a run went, per tool and overall.
#[derive(Default)]
pub struct Report {
    /// Should-cases where the tool was called.
    pub hits: usize,
    pub should: usize,
    /// Should-not cases where the tool was called anyway.
    pub false_triggers: usize,
    pub should_not: usize,
    /// Cases whose pick was exactly what was expected (the tool, the
    /// `instead` tool, or none).
    pub exact: usize,
    pub total: usize,
    pub per_tool: BTreeMap<String, (usize, usize, usize, usize)>,
}

impl Report {
    pub fn of(outcomes: &[Outcome]) -> Self {
        let mut report = Report::default();
        for o in outcomes {
            let called = o.chosen.as_deref() == Some(o.case.tool.as_str());
            let row = report.per_tool.entry(o.case.tool.clone()).or_default();
            if o.case.should {
                report.should += 1;
                row.1 += 1;
                if called {
                    report.hits += 1;
                    row.0 += 1;
                }
            } else {
                report.should_not += 1;
                row.3 += 1;
                if called {
                    report.false_triggers += 1;
                    row.2 += 1;
                }
            }
            report.total += 1;
            if o.chosen == o.case.expected {
                report.exact += 1;
            }
        }
        report
    }

    pub fn text(&self) -> String {
        let mut out = format!(
            "应触发命中 {}/{}，误触发 {}/{}，完全一致 {}/{}\n",
            self.hits, self.should, self.false_triggers, self.should_not, self.exact, self.total
        );
        for (tool, (hit, should, wrong, not)) in &self.per_tool {
            out.push_str(&format!(
                "  {tool}: 命中 {hit}/{should}，误触发 {wrong}/{not}\n"
            ));
        }
        out
    }
}

/// Runs every case through `harness` and the real server: the server is
/// started on a pipe, initialized, listed, and each pick is called.
pub async fn run(harness: &mut dyn Harness, tools: &[Value], cases: Vec<Case>) -> Vec<Outcome> {
    let (agent, server) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server);
    let serving = tokio::spawn(async move {
        crate::mcp_context::serve_with(
            Path::new("/nonexistent"),
            "eval-token",
            BufReader::new(server_read),
            server_write,
        )
        .await;
    });
    let (agent_read, mut agent_write) = tokio::io::split(agent);
    let mut replies = BufReader::new(agent_read).lines();
    let mut next = 0u64;
    let mut ask = |method: &str, params: Value| {
        next += 1;
        json!({"jsonrpc": "2.0", "id": next, "method": method, "params": params})
    };
    let mut send = async |message: Value| -> Value {
        agent_write
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        let line = replies.next_line().await.unwrap().expect("a reply");
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], message["id"], "replies come in order");
        reply
    };
    let init = send(ask("initialize", json!({"protocolVersion": "2025-06-18"}))).await;
    assert!(init["result"]["serverInfo"]["name"].is_string());
    let listed = send(ask("tools/list", json!({}))).await;
    assert!(listed["result"]["tools"].is_array());
    let mut outcomes = Vec::new();
    for case in cases {
        let chosen = harness.choose(&case.prompt, tools);
        let status = match &chosen {
            Some(name) => {
                let reply = send(ask("tools/call", json!({"name": name, "arguments": {}}))).await;
                reply["result"]["structuredContent"]["status"]
                    .as_str()
                    .map(str::to_string)
            }
            None => None,
        };
        outcomes.push(Outcome {
            case,
            chosen,
            status,
        });
    }
    // Both halves of the agent's end gone: the server reads its end.
    drop(agent_write);
    drop(replies);
    serving.await.unwrap();
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::tools::AERORIC_TOOLS;

    /// The set covers exactly the shared tool table, 3–5 cases each way,
    /// every `instead` names a real tool (not the same one) or none, and
    /// no request appears twice.
    #[test]
    fn the_eval_set_covers_every_tool() {
        let set = load();
        assert_eq!(set.version, 1);
        let table: BTreeSet<&str> = AERORIC_TOOLS.iter().map(|(n, _, _)| *n).collect();
        let listed: BTreeSet<&str> = set.tools.keys().map(String::as_str).collect();
        assert_eq!(listed, table);
        let mut seen = BTreeSet::new();
        for (tool, cases) in &set.tools {
            assert!((3..=5).contains(&cases.should.len()), "{tool} should");
            assert!(
                (3..=5).contains(&cases.should_not.len()),
                "{tool} shouldNot"
            );
            for avoid in &cases.should_not {
                assert!(
                    avoid.instead == "none"
                        || (table.contains(avoid.instead.as_str()) && avoid.instead != *tool),
                    "{tool}: instead {}",
                    avoid.instead
                );
            }
            for prompt in cases
                .should
                .iter()
                .chain(cases.should_not.iter().map(|a| &a.prompt))
            {
                assert!(seen.insert(prompt.clone()), "twice: {prompt}");
            }
        }
    }

    /// The flow with the fake harness: every case is decided, every pick is
    /// a listed tool and is called through the server, and every call is
    /// answered with a status (no host here: nothing is sent, so `retry`).
    #[tokio::test]
    async fn the_fake_harness_runs_the_whole_set() {
        let set = load();
        let tools = crate::mcp_context::all_tools();
        let names: BTreeSet<String> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        let total = cases(&set).len();
        let outcomes = run(&mut KeywordHarness { floor: 2 }, &tools, cases(&set)).await;
        assert_eq!(outcomes.len(), total);
        for outcome in &outcomes {
            if let Some(chosen) = &outcome.chosen {
                assert!(names.contains(chosen));
                assert!(
                    matches!(
                        outcome.status.as_deref(),
                        Some("retry") | Some("rejected") | Some("unclear") | Some("ok")
                    ),
                    "{chosen}: {:?}",
                    outcome.status
                );
            } else {
                assert!(outcome.status.is_none());
            }
        }
        let report = Report::of(&outcomes);
        assert_eq!(report.total, total);
        assert_eq!(report.should + report.should_not, total);
        // The baseline, for the note; not a bar a fake has to clear.
        println!("C8 fake-harness baseline:\n{}", report.text());
    }

    #[test]
    fn the_report_counts_hits_false_triggers_and_exact_picks() {
        let case = |tool: &str, should: bool, expected: Option<&str>| Case {
            tool: tool.into(),
            prompt: String::new(),
            expected: expected.map(str::to_string),
            should,
        };
        let outcomes = vec![
            Outcome {
                case: case("a", true, Some("a")),
                chosen: Some("a".into()),
                status: None,
            },
            Outcome {
                case: case("a", true, Some("a")),
                chosen: None,
                status: None,
            },
            Outcome {
                case: case("a", false, Some("b")),
                chosen: Some("a".into()),
                status: None,
            },
            Outcome {
                case: case("a", false, None),
                chosen: None,
                status: None,
            },
        ];
        let report = Report::of(&outcomes);
        assert_eq!(
            (
                report.hits,
                report.should,
                report.false_triggers,
                report.should_not
            ),
            (1, 2, 1, 2)
        );
        assert_eq!(report.exact, 2);
        assert_eq!(report.per_tool["a"], (1, 2, 1, 2));
        assert!(report
            .text()
            .starts_with("应触发命中 1/2，误触发 1/2，完全一致 2/4"));
    }
}
