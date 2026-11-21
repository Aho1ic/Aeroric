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
//!
//! H8 first step (2026-11-21): a run is kept as a [`Record`] per harness —
//! per tool the hits, false triggers, calls to tools the case did not want
//! (无关调用) and how long until the wanted tool was first called — with a
//! fingerprint of each tool's description. Two records of one harness give a
//! before/after table ([`compare`]); records of several harnesses give one
//! table of trigger rates side by side ([`by_harness`]). A description is
//! tried before it is written into the code with [`with_descriptions`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};
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

/// What decides, for one request, which tools to call, in order (empty:
/// none). The first is the pick; any after it are counted too.
pub trait Harness {
    fn calls(&mut self, prompt: &str, tools: &[Value]) -> Vec<String>;
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
    fn calls(&mut self, prompt: &str, tools: &[Value]) -> Vec<String> {
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
            .into_iter()
            .collect()
    }
}

/// The tools as offered, with some descriptions replaced: a description is
/// tried here before it is written into the code.
pub fn with_descriptions(tools: &[Value], descriptions: &BTreeMap<String, String>) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let mut tool = tool.clone();
            let name = tool["name"].as_str().unwrap_or_default().to_string();
            if let Some(text) = descriptions.get(&name) {
                tool["description"] = json!(text);
            }
            tool
        })
        .collect()
}

/// A description's fingerprint (FNV-1a, 16 hex): whether it changed between
/// two records, not what it said.
pub fn fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// One request's outcome.
pub struct Outcome {
    pub case: Case,
    /// The first call, if any.
    pub chosen: Option<String>,
    /// The server's `structuredContent.status` for the first call.
    pub status: Option<String>,
    /// Every call, in order.
    pub calls: Vec<String>,
    /// From the request to the first call of the wanted tool (should-cases).
    pub first_hit_ms: Option<u64>,
}

impl Outcome {
    /// Calls of tools other than the one the case wanted.
    pub fn stray(&self) -> usize {
        self.calls
            .iter()
            .filter(|c| Some(c.as_str()) != self.case.expected.as_deref())
            .count()
    }
}

/// One tool's counts in a run.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub hits: usize,
    pub should: usize,
    pub false_triggers: usize,
    pub should_not: usize,
    /// Calls, in this tool's cases, of tools the case did not want.
    pub stray: usize,
    /// Mean time to the first call of the wanted tool, over the hits.
    pub first_hit_ms: Option<u64>,
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
    pub stray: usize,
    pub per_tool: BTreeMap<String, Row>,
}

impl Report {
    pub fn of(outcomes: &[Outcome]) -> Self {
        let mut report = Report::default();
        let mut waits: BTreeMap<String, Vec<u64>> = BTreeMap::new();
        for o in outcomes {
            let called = o.chosen.as_deref() == Some(o.case.tool.as_str());
            let row = report.per_tool.entry(o.case.tool.clone()).or_default();
            row.stray += o.stray();
            report.stray += o.stray();
            if o.case.should {
                report.should += 1;
                row.should += 1;
                if called {
                    report.hits += 1;
                    row.hits += 1;
                }
                if let Some(ms) = o.first_hit_ms {
                    waits.entry(o.case.tool.clone()).or_default().push(ms);
                }
            } else {
                report.should_not += 1;
                row.should_not += 1;
                if called {
                    report.false_triggers += 1;
                    row.false_triggers += 1;
                }
            }
            report.total += 1;
            if o.chosen == o.case.expected {
                report.exact += 1;
            }
        }
        for (tool, list) in waits {
            if let Some(row) = report.per_tool.get_mut(&tool) {
                row.first_hit_ms = Some(list.iter().sum::<u64>() / list.len() as u64);
            }
        }
        report
    }

    pub fn text(&self) -> String {
        let mut out = format!(
            "应触发命中 {}/{}，误触发 {}/{}，完全一致 {}/{}\n",
            self.hits, self.should, self.false_triggers, self.should_not, self.exact, self.total
        );
        for (tool, row) in &self.per_tool {
            out.push_str(&format!(
                "  {tool}: 命中 {}/{}，误触发 {}/{}\n",
                row.hits, row.should, row.false_triggers, row.should_not
            ));
        }
        out
    }
}

/// A run kept: which harness, under what label (`改描述前`), and per tool
/// its counts and its description's fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub version: u32,
    pub harness: String,
    pub label: String,
    pub at_ms: u64,
    pub rows: BTreeMap<String, Row>,
    pub described: BTreeMap<String, String>,
}

impl Record {
    pub fn of(harness: &str, label: &str, at_ms: u64, tools: &[Value], report: &Report) -> Self {
        Record {
            version: 1,
            harness: harness.into(),
            label: label.into(),
            at_ms,
            rows: report.per_tool.clone(),
            described: tools
                .iter()
                .filter_map(|t| {
                    Some((
                        t["name"].as_str()?.to_string(),
                        fingerprint(t["description"].as_str()?),
                    ))
                })
                .collect(),
        }
    }

    fn total(&self) -> Row {
        let mut out = Row::default();
        for row in self.rows.values() {
            out.hits += row.hits;
            out.should += row.should;
            out.false_triggers += row.false_triggers;
            out.should_not += row.should_not;
            out.stray += row.stray;
        }
        out
    }
}

fn ratio(a: usize, b: usize) -> String {
    format!("{a}/{b}")
}

/// The before/after table of one harness: a row per tool whose description
/// or counts changed, then the totals. Records of two harnesses are not
/// compared (a change is only seen on the harness it was measured on).
pub fn compare(before: &Record, after: &Record) -> Result<String, String> {
    if before.harness != after.harness {
        return Err(format!(
            "两份记录来自不同的 Harness（{} 和 {}），不能对比",
            before.harness, after.harness
        ));
    }
    let mut out = format!(
        "{}：{} → {}\n\n| 工具 | 描述 | 应触发命中 | 误触发 | 无关调用 |\n|---|---|---|---|---|\n",
        after.harness, before.label, after.label
    );
    let tools: BTreeSet<&String> = before.rows.keys().chain(after.rows.keys()).collect();
    let line = |name: &str, described: &str, a: &Row, b: &Row| {
        format!(
            "| {name} | {described} | {} → {} | {} → {} | {} → {} |\n",
            ratio(a.hits, a.should),
            ratio(b.hits, b.should),
            ratio(a.false_triggers, a.should_not),
            ratio(b.false_triggers, b.should_not),
            a.stray,
            b.stray
        )
    };
    for tool in tools {
        let empty = Row::default();
        let a = before.rows.get(tool).unwrap_or(&empty);
        let b = after.rows.get(tool).unwrap_or(&empty);
        let rewritten = before.described.get(tool) != after.described.get(tool);
        let moved = (a.hits, a.should, a.false_triggers, a.should_not, a.stray)
            != (b.hits, b.should, b.false_triggers, b.should_not, b.stray);
        if rewritten || moved {
            out.push_str(&line(tool, if rewritten { "改了" } else { "" }, a, b));
        }
    }
    out.push_str(&line("合计", "", &before.total(), &after.total()));
    Ok(out)
}

/// The trigger rates of several harnesses side by side: per tool, each
/// harness's hits and false triggers (`—` where it was not run).
pub fn by_harness(records: &[Record]) -> String {
    let mut out = String::from("| 工具 |");
    for r in records {
        out.push_str(&format!(" {} |", r.harness));
    }
    out.push_str("\n|---|");
    out.push_str(&"---|".repeat(records.len()));
    out.push('\n');
    let tools: BTreeSet<&String> = records.iter().flat_map(|r| r.rows.keys()).collect();
    for tool in tools {
        out.push_str(&format!("| {tool} |"));
        for r in records {
            match r.rows.get(tool) {
                Some(row) => out.push_str(&format!(
                    " 命中 {} · 误触发 {} |",
                    ratio(row.hits, row.should),
                    ratio(row.false_triggers, row.should_not)
                )),
                None => out.push_str(" — |"),
            }
        }
        out.push('\n');
    }
    out
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
        let started = Instant::now();
        let calls = harness.calls(&case.prompt, tools);
        let mut status = None;
        let mut first_hit_ms = None;
        for (i, name) in calls.iter().enumerate() {
            let reply = send(ask("tools/call", json!({"name": name, "arguments": {}}))).await;
            if i == 0 {
                status = reply["result"]["structuredContent"]["status"]
                    .as_str()
                    .map(str::to_string);
            }
            if case.should && first_hit_ms.is_none() && *name == case.tool {
                first_hit_ms = Some(started.elapsed().as_millis() as u64);
            }
        }
        outcomes.push(Outcome {
            chosen: calls.first().cloned(),
            case,
            status,
            calls,
            first_hit_ms,
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
        let outcome = |case: Case, calls: &[&str], first_hit_ms: Option<u64>| Outcome {
            case,
            chosen: calls.first().map(|c| c.to_string()),
            status: None,
            calls: calls.iter().map(|c| c.to_string()).collect(),
            first_hit_ms,
        };
        let outcomes = vec![
            outcome(case("a", true, Some("a")), &["b", "a"], Some(40)),
            outcome(case("a", true, Some("a")), &[], None),
            outcome(case("a", false, Some("b")), &["a"], None),
            outcome(case("a", false, None), &[], None),
            outcome(case("a", true, Some("a")), &["a"], Some(20)),
        ];
        let report = Report::of(&outcomes);
        assert_eq!(
            (
                report.hits,
                report.should,
                report.false_triggers,
                report.should_not
            ),
            (1, 3, 1, 2)
        );
        assert_eq!(report.exact, 2);
        let row = &report.per_tool["a"];
        assert_eq!(
            (row.hits, row.should, row.false_triggers, row.should_not),
            (1, 3, 1, 2)
        );
        // H8: the first case called `b` before `a`; the third wanted `b`.
        assert_eq!((row.stray, report.stray), (2, 2));
        assert_eq!(row.first_hit_ms, Some(30));
        assert!(report
            .text()
            .starts_with("应触发命中 1/3，误触发 1/2，完全一致 2/5"));
    }

    /// H8: a description tried before it is written into the code; the two
    /// records of the keyword harness give the before/after table, which
    /// names the tool rewritten and leaves out the tools that did not move.
    #[tokio::test]
    async fn a_description_change_gives_a_before_after_table() {
        let set = load();
        let tools = crate::mcp_context::all_tools();
        let run_with = async |tools: &[Value]| {
            let outcomes = run(&mut KeywordHarness { floor: 2 }, tools, cases(&set)).await;
            Report::of(&outcomes)
        };
        let before = Record::of("keyword", "改描述前", 1, &tools, &run_with(&tools).await);
        // The rewrite: board_write's description in its users' words.
        let rewrite = set.tools["board_write"].should.join(" ");
        let changed = with_descriptions(
            &tools,
            &BTreeMap::from([("board_write".to_string(), rewrite)]),
        );
        let after = Record::of(
            "keyword",
            "改描述后",
            2,
            &changed,
            &run_with(&changed).await,
        );
        assert!(
            after.rows["board_write"].hits > before.rows["board_write"].hits,
            "{:?} → {:?}",
            before.rows["board_write"],
            after.rows["board_write"]
        );
        let kept: Record = serde_json::from_str(&serde_json::to_string(&after).unwrap()).unwrap();
        assert_eq!(kept, after);

        let table = compare(&before, &after).unwrap();
        println!("H8 before/after (keyword harness):\n{table}");
        assert!(table.starts_with("keyword：改描述前 → 改描述后"));
        assert!(table.contains("\n| board_write | 改了 | "), "{table}");
        assert!(table
            .trim_end()
            .lines()
            .last()
            .unwrap()
            .starts_with("| 合计 |  | "));
        let still = before
            .rows
            .keys()
            .find(|t| {
                before.rows[*t] == after.rows[*t] && before.described[*t] == after.described[*t]
            })
            .expect("some tool did not move");
        assert!(!table.contains(&format!("| {still} |")), "{table}");
    }

    struct Scripted(Vec<&'static str>);

    impl Harness for Scripted {
        fn calls(&mut self, _: &str, _: &[Value]) -> Vec<String> {
            self.0.iter().map(|c| c.to_string()).collect()
        }
    }

    /// H8: every call is made; the wanted one is timed, the others counted
    /// as stray; the status is the first call's.
    #[tokio::test]
    async fn the_run_times_the_wanted_call_and_counts_the_others() {
        let tools = crate::mcp_context::all_tools();
        let case = Case {
            tool: "search_wiki".into(),
            prompt: "Wiki 里有没有讲网关？".into(),
            expected: Some("search_wiki".into()),
            should: true,
        };
        let mut harness = Scripted(vec!["memory_recall", "search_wiki"]);
        let outcomes = run(&mut harness, &tools, vec![case]).await;
        let o = &outcomes[0];
        assert_eq!(o.calls, vec!["memory_recall", "search_wiki"]);
        assert_eq!(o.chosen.as_deref(), Some("memory_recall"));
        assert!(o.status.is_some());
        assert_eq!(o.stray(), 1);
        assert!(o.first_hit_ms.is_some());
        let report = Report::of(&outcomes);
        assert_eq!(report.hits, 0, "the pick was another tool");
        assert_eq!(report.per_tool["search_wiki"].stray, 1);
    }

    /// H8: a change is compared on the harness it was measured on; several
    /// harnesses are shown side by side.
    #[test]
    fn harnesses_are_shown_side_by_side_not_compared() {
        let row = |hits, false_triggers| Row {
            hits,
            should: 3,
            false_triggers,
            should_not: 3,
            ..Row::default()
        };
        let record = |harness: &str, rows: Vec<(&str, Row)>| Record {
            version: 1,
            harness: harness.into(),
            label: "基线".into(),
            at_ms: 1,
            rows: rows.into_iter().map(|(t, r)| (t.to_string(), r)).collect(),
            described: BTreeMap::new(),
        };
        let keyword = record("keyword", vec![("a", row(1, 0)), ("b", row(2, 1))]);
        let codex = record("codex", vec![("a", row(3, 0))]);
        assert!(compare(&keyword, &codex)
            .unwrap_err()
            .contains("不同的 Harness"));
        let table = by_harness(&[keyword, codex]);
        assert!(
            table.starts_with("| 工具 | keyword | codex |\n|---|---|---|\n"),
            "{table}"
        );
        assert!(table.contains("| a | 命中 1/3 · 误触发 0/3 | 命中 3/3 · 误触发 0/3 |"));
        assert!(table.contains("| b | 命中 2/3 · 误触发 1/3 | — |"));
    }
}
