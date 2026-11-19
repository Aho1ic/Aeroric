//! R5-2 (2026-11-11): run one turn again with other models, side by side.
//!
//! A rerun starts where the original turn did: each run is a session of its
//! own in an isolated copy made from the turn's `before` snapshot, sent the
//! turn's message (editable) and, unless the user asks for the message
//! alone, what the original session had settled before that turn. The
//! groups are kept per original session under `<data>/reruns/<task>.json`;
//! the comparison is read fresh from each run's transcript and ledger.
//!
//! B7 (2026-11-19): when a rerun starts, what each side started from is
//! frozen with the group — the tree, the prompt, the context, the
//! permission, the harness and model — the original turn's as well as each
//! run's as it really began. The comparison says from that record whether
//! the inputs were the same, never by inference.

use std::path::Path;

use aeroric_protocol::checkpoint::{Checkpoint, Entry};
use aeroric_protocol::turn_ledger::{TurnCheck, TurnRecord, TurnState};
use serde::{Deserialize, Serialize};

use super::transcript_stats::TurnStat;
use super::turn_outcome::{NotRun, ProductFailure};

/// Runs one rerun may start: the comparison stays readable side by side.
pub const MAX_RUNS: usize = 3;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RerunGroup {
    pub id: String,
    pub origin_task_id: String,
    pub turn_id: String,
    pub turn_started_ms: u64,
    /// The snapshot tree every run's copy starts from.
    pub base: String,
    /// The message as the user sent it again (before the brief is added).
    pub message: String,
    pub with_brief: bool,
    pub created_ms: u64,
    pub runs: Vec<RerunRun>,
    /// B7: the original turn's inputs; unset on groups from before B7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_inputs: Option<RunInputs>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RerunRun {
    pub task_id: String,
    pub configuration_id: String,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// e.g. `Codex · gpt-6.1-sol`.
    pub label: String,
    /// B7: what this run really started with; unset on runs from before B7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<RunInputs>,
}

/// B7: what one side of a comparison started from. A field left unset was
/// not recorded (it reads "未记录", never as the same or different).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunInputs {
    /// The work tree the agent found: for a run, its copy's own snapshot
    /// taken right after the copy was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// [`text_hash`] of the message (without the brief).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// `conversation` (the session so far), `brief:<hash>` (what had been
    /// settled, then the message) or `message` (the message alone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Skills and their versions; unset until they are recorded (C7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
}

/// The first 16 hex digits of the SHA-256 of `text`, trimmed: the same
/// message typed again hashes the same.
pub fn text_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.trim().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// B7: one input across the columns (original first, then each run).
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputItem {
    pub name: &'static str,
    /// Per column, as shown; `None`: not recorded.
    pub values: Vec<Option<String>>,
    /// `same` | `differs` | `unrecorded` | `compared` (what the rerun is
    /// for: harness and model).
    pub verdict: &'static str,
    /// The runs differ among themselves (not only from the original).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub runs_differ: bool,
}

/// B7: whether the sides started from the same inputs.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputComparison {
    /// One line for the card's head.
    pub summary: String,
    /// At least one difference: results may differ for that reason, not
    /// the model's.
    pub warn: bool,
    pub items: Vec<InputItem>,
}

pub const INPUTS_UNRECORDED: &str = "这组重跑早于输入记录，说不出两边的输入是否一致";

/// B7: the group's inputs side by side, from what was recorded at start.
pub fn compare_inputs(group: &RerunGroup) -> InputComparison {
    let Some(original) = &group.original_inputs else {
        return InputComparison {
            summary: INPUTS_UNRECORDED.into(),
            warn: true,
            items: Vec::new(),
        };
    };
    let empty = RunInputs::default();
    let mut sides = vec![original];
    sides.extend(
        group
            .runs
            .iter()
            .map(|r| r.inputs.as_ref().unwrap_or(&empty)),
    );
    let short = |v: &Option<String>, n: usize| v.as_ref().map(|t| t.chars().take(n).collect());
    let item = |name: &'static str, values: Vec<Option<String>>, compared: bool| {
        let known: Vec<&String> = values.iter().flatten().collect();
        let runs: Vec<&String> = values[1..].iter().flatten().collect();
        let runs_differ = runs.windows(2).any(|w| w[0] != w[1]);
        let verdict = if compared {
            "compared"
        } else if known.len() < values.len() {
            "unrecorded"
        } else if known.windows(2).all(|w| w[0] == w[1]) {
            "same"
        } else {
            "differs"
        };
        InputItem {
            name,
            values,
            verdict,
            runs_differ: runs_differ && !compared,
        }
    };
    let items = vec![
        item(
            "起点",
            sides.iter().map(|s| short(&s.base, 10)).collect(),
            false,
        ),
        item(
            "提示词",
            sides.iter().map(|s| short(&s.prompt, 8)).collect(),
            false,
        ),
        item(
            "上下文",
            sides
                .iter()
                .map(|s| s.context.as_deref().map(context_text))
                .collect(),
            false,
        ),
        item(
            "权限",
            sides.iter().map(|s| s.permission.clone()).collect(),
            false,
        ),
        item(
            "技能",
            sides
                .iter()
                .map(|s| s.skills.as_ref().map(|k| k.join("、")))
                .collect(),
            false,
        ),
        item(
            "Harness 与模型",
            sides
                .iter()
                .map(|s| {
                    s.harness.as_ref().map(|h| match &s.model {
                        Some(m) => format!("{h} · {m}"),
                        None => h.clone(),
                    })
                })
                .collect(),
            true,
        ),
    ];
    let names = |pick: &dyn Fn(&InputItem) -> bool| -> Vec<&'static str> {
        items.iter().filter(|i| pick(i)).map(|i| i.name).collect()
    };
    let between = names(&|i| i.runs_differ);
    let differs = names(&|i| i.verdict == "differs");
    // Skills are not recorded anywhere yet (C7): said, but they do not keep
    // the rest from being judged.
    let unknown = names(&|i| i.verdict == "unrecorded" && i.name != "技能");
    let skills_unknown = verdict_of(&items, "技能") == Some("unrecorded");
    let mut summary = if !between.is_empty() {
        format!(
            "重跑之间有 {} 处输入不同（{}），结果的差异不一定来自模型",
            between.len(),
            between.join("、")
        )
    } else if !differs.is_empty() {
        format!(
            "重跑之间输入一致；和原回合比，{}不同，结果的差异不一定来自模型",
            differs.join("、")
        )
    } else if !unknown.is_empty() {
        format!("说不出输入是否一致：{}未记录", unknown.join("、"))
    } else {
        "输入一致，只换了 Harness 和模型".to_string()
    };
    if skills_unknown {
        summary.push_str("。技能未记录");
    }
    InputComparison {
        summary,
        warn: !differs.is_empty() || !between.is_empty() || !unknown.is_empty(),
        items,
    }
}

fn verdict_of(items: &[InputItem], name: &str) -> Option<&'static str> {
    items.iter().find(|i| i.name == name).map(|i| i.verdict)
}

fn context_text(context: &str) -> String {
    match context {
        "conversation" => "完整对话".into(),
        "message" => "只有消息".into(),
        other => match other.strip_prefix("brief:") {
            Some(hash) => format!("已确认的决定（{}）+ 消息", &hash[..hash.len().min(6)]),
            None => other.into(),
        },
    }
}

fn path(dir: &Path, origin: &str) -> std::path::PathBuf {
    let safe: String = origin
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("{safe}.json"))
}

/// An unreadable file reads as no reruns: the runs are sessions of their own
/// and stay in the sidebar either way.
pub fn load(dir: &Path, origin: &str) -> Vec<RerunGroup> {
    std::fs::read(path(dir, origin))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn append(dir: &Path, group: RerunGroup) -> Result<(), String> {
    let mut groups = load(dir, &group.origin_task_id);
    groups.push(group.clone());
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let target = path(dir, &group.origin_task_id);
    let tmp = target.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&groups).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

/// What the original session had settled before its turn `index` (0-based):
/// the goal, decisions and constraints whose evidence came from an earlier
/// turn. Checkpoint turns count from 1, so earlier means `turn <= index`;
/// an entry with no turn on its evidence is left out, its time unknown.
pub fn brief(checkpoint: &Checkpoint, index: u64) -> Option<String> {
    let earlier = |entry: &Entry| {
        entry
            .evidence
            .iter()
            .filter_map(|e| e.turn)
            .min()
            .is_some_and(|turn| turn <= index)
    };
    let list = |entries: &[Entry]| -> Vec<String> {
        entries
            .iter()
            .filter(|e| earlier(e))
            .map(|e| format!("- {}", e.text))
            .collect()
    };
    let goal = checkpoint.goal.as_ref().filter(|g| index > 0 && earlier(g));
    let decisions = list(&checkpoint.decisions);
    let constraints = list(&checkpoint.constraints);
    if goal.is_none() && decisions.is_empty() && constraints.is_empty() {
        return None;
    }
    let mut out = vec!["这个任务之前已经确认过下面这些，供你参考：".to_string()];
    if let Some(goal) = goal {
        out.push(format!("目标：{}", goal.text));
    }
    if !decisions.is_empty() {
        out.push("已确认的决定：".into());
        out.extend(decisions);
    }
    if !constraints.is_empty() {
        out.push("约束：".into());
        out.extend(constraints);
    }
    Some(out.join("\n"))
}

/// The prompt a run is sent: the brief first when there is one.
pub fn prompt(message: &str, brief: Option<&str>) -> String {
    match brief {
        Some(brief) => format!("{brief}\n\n---\n\n{message}"),
        None => message.to_string(),
    }
}

/// A turn undone after a rerun of it started was replaced by a run, not
/// judged a failure: its product failure is lifted wherever turns are shown
/// (the transcript's capsule agrees with the comparison card).
pub fn lift_replaced(turns: &mut [TurnStat], records: &[TurnRecord], groups: &[RerunGroup]) {
    for turn in turns {
        let Some(record) = super::change_cost::nearest(turn.started_at, records) else {
            continue;
        };
        let replaced = record.state == TurnState::Undone
            && groups.iter().any(|g| {
                g.turn_id == record.turn_id && record.undone_ms.is_some_and(|at| at >= g.created_ms)
            });
        if replaced
            && turn
                .product_failure
                .as_ref()
                .is_some_and(|f| f.undone && f.check.is_none())
        {
            turn.product_failure = None;
        }
    }
}

/// The permission a run gets: `auto_edit` when its configuration offers it.
/// A run works in its own copy, so asking before every command only stalls
/// it and makes its time a measure of the user's replies (R5 目检
/// 2026-11-12: 8 min 40 s, mostly waiting for 批准一次).
pub fn permission(offered: &[String], original: Option<&str>) -> String {
    if offered.iter().any(|mode| mode == "auto_edit") {
        "auto_edit".into()
    } else {
        original.unwrap_or("ask").to_string()
    }
}

/// One column of the comparison.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    /// running / pending / completed / notRun / interrupted / productFailure
    /// / replaced (the original turn, undone after the rerun started: the
    /// user took a run instead, which says nothing against the turn itself).
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_run: Option<NotRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_failure: Option<ProductFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<u64>,
    pub files: usize,
    pub additions: u64,
    pub deletions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<TurnCheck>,
}

/// A turn's column from its graded stats and the ledger records of its
/// changes. `undone_after`: the group's start, so an original turn undone
/// since reads `replaced` rather than a product failure.
pub fn column(
    stat: Option<&TurnStat>,
    changes: &[&TurnRecord],
    running: bool,
    undone_after: Option<u64>,
) -> Column {
    let replaced = undone_after.is_some_and(|since| {
        changes
            .iter()
            .any(|r| r.state == TurnState::Undone && r.undone_ms.is_some_and(|at| at >= since))
    });
    let outcome = match stat {
        _ if running => "running",
        None => "pending",
        Some(s) if s.not_run.is_some() => "notRun",
        Some(s) if s.interrupted => "interrupted",
        Some(_) if replaced => "replaced",
        Some(s) if s.product_failure.is_some() => "productFailure",
        Some(s) if s.ended_at.is_none() => "running",
        Some(_) => "completed",
    };
    Column {
        outcome,
        not_run: stat.and_then(|s| s.not_run.clone()),
        product_failure: stat
            .and_then(|s| s.product_failure.clone())
            .filter(|_| !replaced),
        input_tokens: stat.and_then(|s| s.input_tokens),
        output_tokens: stat.and_then(|s| s.output_tokens),
        cost_usd: stat.and_then(|s| s.cost_usd),
        duration_ms: stat.and_then(|s| s.ended_at.map(|e| e.saturating_sub(s.started_at))),
        tools: stat.map(|s| s.tools.len() as u64 + u64::from(s.more_tools)),
        files: changes.iter().map(|r| r.files.len()).sum(),
        additions: changes.iter().map(|r| r.additions).sum(),
        deletions: changes.iter().map(|r| r.deletions).sum(),
        check: changes.iter().rev().find_map(|r| r.check.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_protocol::checkpoint::{Epistemic, EvidenceKind, EvidenceRef};

    fn entry(text: &str, turn: Option<u64>) -> Entry {
        Entry {
            text: text.into(),
            status: Epistemic::Verified,
            evidence: vec![EvidenceRef {
                kind: EvidenceKind::UserMessage,
                seq: None,
                turn,
                detail: String::new(),
            }],
        }
    }

    #[test]
    fn the_brief_keeps_only_what_was_settled_before_the_turn() {
        let checkpoint = Checkpoint {
            goal: Some(entry("把导出改成流式", Some(1))),
            decisions: vec![
                entry("用 csv 不用 xlsx", Some(1)),
                entry("分页 500 行", Some(2)),
                entry("时间不明", None),
            ],
            constraints: vec![entry("不改公开 API", Some(1))],
            ..Checkpoint::new("t")
        };
        assert_eq!(
            brief(&checkpoint, 0),
            None,
            "the first turn has nothing before it"
        );
        assert_eq!(
            brief(&checkpoint, 1).unwrap(),
            "这个任务之前已经确认过下面这些，供你参考：\n目标：把导出改成流式\n已确认的决定：\n- 用 csv 不用 xlsx\n约束：\n- 不改公开 API"
        );
        assert!(brief(&checkpoint, 2).unwrap().contains("- 分页 500 行"));
        assert!(!brief(&checkpoint, 5).unwrap().contains("时间不明"));
        assert_eq!(prompt("再做一次", None), "再做一次");
        assert_eq!(prompt("再做一次", Some("背景")), "背景\n\n---\n\n再做一次");
    }

    #[test]
    fn groups_are_kept_per_original_session() {
        let dir = std::env::temp_dir().join(format!("aeroric-reruns-{}", uuid::Uuid::new_v4()));
        let group = |id: &str| RerunGroup {
            id: id.into(),
            origin_task_id: "task/1".into(),
            turn_id: "turn-1".into(),
            turn_started_ms: 1,
            base: "abc".into(),
            message: "m".into(),
            with_brief: true,
            created_ms: 2,
            runs: vec![RerunRun {
                task_id: "rerun-1".into(),
                configuration_id: "codex".into(),
                agent: "codex".into(),
                model: Some("gpt-6.1-sol".into()),
                label: "Codex · gpt-6.1-sol".into(),
                inputs: None,
            }],
            original_inputs: None,
        };
        assert!(load(&dir, "task/1").is_empty());
        append(&dir, group("g1")).unwrap();
        append(&dir, group("g2")).unwrap();
        let ids: Vec<String> = load(&dir, "task/1").into_iter().map(|g| g.id).collect();
        assert_eq!(ids, ["g1", "g2"]);
        assert!(load(&dir, "task-2").is_empty());
    }

    fn record(state: TurnState, undone_ms: Option<u64>) -> TurnRecord {
        TurnRecord {
            turn_id: "t".into(),
            started_ms: 0,
            ended_ms: 1,
            top: String::new(),
            before: String::new(),
            after: String::new(),
            head_before: None,
            head_after: None,
            files: vec![],
            additions: 1,
            deletions: 1,
            state,
            undone_ms,
            undo_group: None,
            state_before_undo: None,
            check: None,
        }
    }

    /// R5 目检: undoing the original to bring a run back made its column read
    /// 「产品失败 · 已撤销」.
    #[test]
    fn an_original_undone_after_the_rerun_reads_replaced() {
        let undone = TurnStat {
            started_at: 0,
            ended_at: Some(10),
            product_failure: Some(ProductFailure {
                undone: true,
                check: None,
            }),
            ..TurnStat::default()
        };
        let after = record(TurnState::Undone, Some(500));
        let replaced = column(Some(&undone), &[&after], false, Some(100));
        assert_eq!(replaced.outcome, "replaced");
        assert_eq!(replaced.product_failure, None);
        let before = record(TurnState::Undone, Some(50));
        assert_eq!(
            column(Some(&undone), &[&before], false, Some(100)).outcome,
            "productFailure"
        );
        assert_eq!(
            column(Some(&undone), &[&after], false, None).outcome,
            "productFailure"
        );
        assert_eq!(column(None, &[], true, None).outcome, "running");
        assert_eq!(column(None, &[], false, None).outcome, "pending");
    }

    #[test]
    fn the_transcript_agrees_a_replaced_turn_is_no_failure() {
        let failed = || TurnStat {
            started_at: 0,
            ended_at: Some(10),
            product_failure: Some(ProductFailure {
                undone: true,
                check: None,
            }),
            ..TurnStat::default()
        };
        let group = |created_ms: u64| RerunGroup {
            id: "g".into(),
            origin_task_id: "o".into(),
            turn_id: "t".into(),
            turn_started_ms: 0,
            base: String::new(),
            message: String::new(),
            with_brief: true,
            created_ms,
            runs: vec![],
            original_inputs: None,
        };
        let records = [record(TurnState::Undone, Some(500))];
        let mut turns = vec![failed()];
        lift_replaced(&mut turns, &records, &[group(100)]);
        assert_eq!(turns[0].product_failure, None);
        let mut earlier = vec![failed()];
        lift_replaced(&mut earlier, &records, &[group(900)]);
        assert!(
            earlier[0].product_failure.is_some(),
            "undone before the rerun"
        );
    }

    #[test]
    fn runs_edit_on_their_own_when_the_configuration_allows() {
        let modes = |list: &[&str]| list.iter().map(|m| m.to_string()).collect::<Vec<_>>();
        assert_eq!(
            permission(&modes(&["ask", "auto_edit", "full_access"]), Some("ask")),
            "auto_edit"
        );
        assert_eq!(permission(&modes(&["ask"]), Some("ask")), "ask");
        assert_eq!(permission(&[], None), "ask");
    }

    fn inputs(
        base: &str,
        prompt: &str,
        context: &str,
        permission: &str,
        harness: &str,
    ) -> RunInputs {
        RunInputs {
            base: Some(base.into()),
            prompt: Some(text_hash(prompt)),
            context: Some(context.into()),
            permission: Some(permission.into()),
            harness: Some(harness.into()),
            model: None,
            skills: None,
        }
    }

    fn compared(original: Option<RunInputs>, runs: Vec<Option<RunInputs>>) -> InputComparison {
        compare_inputs(&RerunGroup {
            id: "g".into(),
            origin_task_id: "o".into(),
            turn_id: "t".into(),
            turn_started_ms: 0,
            base: "tree-a".into(),
            message: "m".into(),
            with_brief: false,
            created_ms: 0,
            runs: runs
                .into_iter()
                .enumerate()
                .map(|(i, inputs)| RerunRun {
                    task_id: format!("r{i}"),
                    configuration_id: "c".into(),
                    agent: "codex".into(),
                    model: None,
                    label: "Codex".into(),
                    inputs,
                })
                .collect(),
            original_inputs: original,
        })
    }

    fn verdict<'a>(c: &'a InputComparison, name: &str) -> &'a InputItem {
        c.items.iter().find(|i| i.name == name).unwrap()
    }

    /// B7: same tree, same message, only the model changed: consistent,
    /// and skills (not recorded yet) say so instead of passing as the same.
    #[test]
    fn inputs_that_match_say_only_the_model_changed() {
        let original = inputs("tree-a", "修复超时", "message", "auto_edit", "claude");
        let run = inputs("tree-a", "修复超时 ", "message", "auto_edit", "codex");
        let c = compared(Some(original), vec![Some(run.clone()), Some(run)]);
        assert_eq!(verdict(&c, "起点").verdict, "same");
        assert_eq!(
            verdict(&c, "提示词").verdict,
            "same",
            "trailing blank ignored"
        );
        assert_eq!(verdict(&c, "Harness 与模型").verdict, "compared");
        assert_eq!(verdict(&c, "技能").verdict, "unrecorded");
        assert!(!c.warn);
        assert_eq!(c.summary, "输入一致，只换了 Harness 和模型。技能未记录");
    }

    /// B7: an edited message and the original's whole conversation differ
    /// from the original; a run whose copy began elsewhere differs among
    /// the runs, and that is what the head says first.
    #[test]
    fn differing_inputs_are_named_and_runs_apart_come_first() {
        let original = inputs("tree-a", "修复超时", "conversation", "ask", "claude");
        let run = inputs(
            "tree-a",
            "修复超时，并补测试",
            "brief:0123456789",
            "auto_edit",
            "codex",
        );
        let c = compared(
            Some(original.clone()),
            vec![Some(run.clone()), Some(run.clone())],
        );
        assert!(c.warn);
        assert_eq!(
            c.summary,
            "重跑之间输入一致；和原回合比，提示词、上下文、权限不同，结果的差异不一定来自模型。技能未记录"
        );
        assert_eq!(
            verdict(&c, "上下文").values,
            [
                Some("完整对话".to_string()),
                Some("已确认的决定（012345）+ 消息".to_string()),
                Some("已确认的决定（012345）+ 消息".to_string())
            ]
        );

        let elsewhere = RunInputs {
            base: Some("tree-b".into()),
            ..run.clone()
        };
        let c = compared(Some(original), vec![Some(run), Some(elsewhere)]);
        assert!(verdict(&c, "起点").runs_differ);
        assert!(
            c.summary.starts_with("重跑之间有 1 处输入不同（起点）"),
            "{}",
            c.summary
        );
    }

    /// B7: a group from before the record cannot claim anything; a run
    /// without its own record reads 未记录, not the same.
    #[test]
    fn unrecorded_inputs_are_never_called_the_same() {
        let c = compared(None, vec![None]);
        assert_eq!(c.summary, INPUTS_UNRECORDED);
        assert!(c.warn && c.items.is_empty());

        let original = inputs("tree-a", "m", "message", "ask", "claude");
        let c = compared(Some(original), vec![None]);
        assert_eq!(verdict(&c, "起点").verdict, "unrecorded");
        assert_eq!(
            verdict(&c, "起点").values,
            [Some("tree-a".to_string()), None]
        );
        assert_eq!(
            c.summary,
            "说不出输入是否一致：起点、提示词、上下文、权限未记录。技能未记录"
        );
        assert!(c.warn);
    }
}
