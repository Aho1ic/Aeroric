//! R3-3a (2026-11-10): what a turn whose changes were kept costs, per agent
//! configuration and model.
//!
//! A turn's spend comes from its transcript (`TurnStat`); whether its changes
//! were kept comes from the turn ledger (R2-1), which records only the turns
//! that changed files. The two are matched by start time: both are stamped
//! when the person's message goes out, within a second of each other.

use std::collections::HashMap;

use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};
use serde::Serialize;

use super::transcript_stats::TurnStat;

/// A ledger turn further than this from a transcript turn's start is not it.
const MATCH_MS: u64 = 60_000;

/// What became of a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Undone,
    /// Changed files the person has not accepted or undone yet.
    Pending,
    /// Changed no files: a question, an investigation, a failed attempt.
    NoChange,
}

/// The ledger turn started nearest `started_at`, if one is close enough.
pub fn outcome(started_at: u64, records: &[TurnRecord]) -> Outcome {
    let nearest = records
        .iter()
        .filter(|r| r.started_ms.abs_diff(started_at) <= MATCH_MS)
        .min_by_key(|r| r.started_ms.abs_diff(started_at));
    match nearest.map(|r| r.state) {
        Some(TurnState::Accepted) => Outcome::Accepted,
        Some(TurnState::Undone) => Outcome::Undone,
        Some(TurnState::Pending) => Outcome::Pending,
        None => Outcome::NoChange,
    }
}

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub turns: u32,
    /// Prompt (cache reads included) plus output, as the bill counts them.
    pub tokens: u64,
    /// `None` once any of its turns is unpriced: a partial sum would read as
    /// the whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl Bucket {
    fn add(&mut self, stat: &TurnStat) {
        let priced = self.turns == 0 || self.cost_usd.is_some();
        self.turns += 1;
        self.tokens += tokens(stat);
        self.cost_usd = match (priced, stat.cost_usd) {
            (true, Some(cost)) => Some(self.cost_usd.unwrap_or(0.0) + cost),
            _ => None,
        };
    }
}

/// Prompt (cache reads included) plus output.
fn tokens(stat: &TurnStat) -> u64 {
    match (stat.input_tokens, stat.output_tokens) {
        (None, None) => stat.tokens,
        (input, output) => input.unwrap_or(0) + output.unwrap_or(0),
    }
}

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeCostRow {
    /// The session's agent family: `codex`, `claude`, `omp` or `dsh`.
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_id: Option<String>,
    /// Filled by the caller from the agent configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Empty when the transcript records no model.
    pub model: String,
    pub accepted: Bucket,
    pub undone: Bucket,
    pub pending: Bucket,
    pub no_change: Bucket,
}

impl ChangeCostRow {
    fn bucket(&mut self, outcome: Outcome) -> &mut Bucket {
        match outcome {
            Outcome::Accepted => &mut self.accepted,
            Outcome::Undone => &mut self.undone,
            Outcome::Pending => &mut self.pending,
            Outcome::NoChange => &mut self.no_change,
        }
    }

    fn tokens(&self) -> u64 {
        self.accepted.tokens + self.undone.tokens + self.pending.tokens + self.no_change.tokens
    }
}

/// Sums turns into one row per agent family, configuration and model.
#[derive(Default)]
pub struct ChangeCost {
    rows: HashMap<(String, Option<String>, String), ChangeCostRow>,
}

impl ChangeCost {
    /// One session's turns that started at or after `since_ms`.
    pub fn add(
        &mut self,
        agent: &str,
        config_id: Option<&str>,
        turns: &[TurnStat],
        records: &[TurnRecord],
        since_ms: u64,
    ) {
        // 目检 2026-11-10: a turn the transcript records no usage for (it
        // failed at once, or an old Claude transcript) says nothing about
        // cost and only added "1 轮 · 0 tok" cards.
        for stat in turns
            .iter()
            .filter(|t| t.started_at >= since_ms && tokens(t) > 0)
        {
            let model = stat.model.clone().unwrap_or_default();
            let key = (
                agent.to_string(),
                config_id.map(str::to_string),
                model.clone(),
            );
            let row = self.rows.entry(key).or_insert_with(|| ChangeCostRow {
                agent: agent.to_string(),
                config_id: config_id.map(str::to_string),
                model,
                ..Default::default()
            });
            row.bucket(outcome(stat.started_at, records)).add(stat);
        }
    }

    /// The rows, most tokens first.
    pub fn rows(self) -> Vec<ChangeCostRow> {
        let mut rows: Vec<ChangeCostRow> = self.rows.into_values().collect();
        rows.sort_by(|a, b| {
            b.tokens()
                .cmp(&a.tokens())
                .then_with(|| a.model.cmp(&b.model))
        });
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(started_at: u64, input: u64, output: u64, cost: Option<f64>) -> TurnStat {
        TurnStat {
            started_at,
            input_tokens: Some(input),
            output_tokens: Some(output),
            model: Some("m".into()),
            cost_usd: cost,
            ..Default::default()
        }
    }

    fn record(started_ms: u64, state: TurnState) -> TurnRecord {
        TurnRecord {
            turn_id: format!("t{started_ms}"),
            started_ms,
            ended_ms: started_ms + 10,
            top: String::new(),
            before: String::new(),
            after: String::new(),
            head_before: None,
            head_after: None,
            files: Vec::new(),
            additions: 0,
            deletions: 0,
            state,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
        }
    }

    #[test]
    fn a_turn_takes_the_state_of_the_ledger_turn_started_with_it() {
        let records = [
            record(100_000, TurnState::Accepted),
            record(200_000, TurnState::Undone),
        ];
        assert_eq!(outcome(100_400, &records), Outcome::Accepted);
        assert_eq!(outcome(199_900, &records), Outcome::Undone);
        assert_eq!(outcome(400_000, &records), Outcome::NoChange);
        assert_eq!(outcome(1, &[]), Outcome::NoChange);
    }

    #[test]
    fn turns_are_summed_per_model_by_what_became_of_them() {
        let mut cost = ChangeCost::default();
        let records = [
            record(1_000_000, TurnState::Accepted),
            record(2_000_000, TurnState::Accepted),
            record(3_000_000, TurnState::Undone),
            record(4_000_000, TurnState::Pending),
        ];
        let turns = [
            turn(10, 999, 1, Some(9.0)),
            turn(1_000_100, 900, 100, Some(0.5)),
            turn(2_000_100, 1_900, 100, Some(1.0)),
            turn(3_000_100, 400, 100, Some(0.25)),
            turn(4_000_100, 300, 0, Some(0.1)),
            turn(5_000_000, 50, 50, Some(0.01)),
        ];
        cost.add("codex", Some("cfg"), &turns, &records, 100);
        let rows = cost.rows();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            (row.agent.as_str(), row.config_id.as_deref()),
            ("codex", Some("cfg"))
        );
        assert_eq!((row.accepted.turns, row.accepted.tokens), (2, 3_000));
        assert_eq!(row.accepted.cost_usd, Some(1.5));
        assert_eq!((row.undone.turns, row.undone.tokens), (1, 500));
        assert_eq!((row.pending.turns, row.pending.tokens), (1, 300));
        assert_eq!((row.no_change.turns, row.no_change.tokens), (1, 100));
    }

    #[test]
    fn one_unpriced_turn_leaves_its_bucket_without_a_cost() {
        let mut cost = ChangeCost::default();
        let turns = [
            turn(10, 1, 1, Some(1.0)),
            turn(20, 1, 1, None),
            turn(30, 1, 1, Some(1.0)),
        ];
        cost.add("claude", None, &turns, &[], 0);
        let row = &cost.rows()[0];
        assert_eq!((row.no_change.turns, row.no_change.cost_usd), (3, None));
    }

    #[test]
    fn a_turn_with_no_recorded_usage_is_left_out() {
        let mut cost = ChangeCost::default();
        let empty = TurnStat {
            started_at: 10,
            ..Default::default()
        };
        cost.add("claude", Some("cc"), &[empty], &[], 0);
        assert!(cost.rows().is_empty());
    }

    #[test]
    fn rows_split_by_model_and_list_the_costliest_first() {
        let mut cost = ChangeCost::default();
        let mut big = turn(10, 5_000, 0, None);
        big.model = Some("big".into());
        cost.add("codex", None, &[turn(10, 10, 0, None), big], &[], 0);
        let models: Vec<String> = cost.rows().into_iter().map(|r| r.model).collect();
        assert_eq!(models, ["big", "m"]);
    }
}
