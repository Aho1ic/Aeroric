//! E6 (2026-11-21): caps on what unattended work may spend — scheduled
//! tasks, delegations no person started, reviews — and a switch that pauses
//! every agent. Work a person starts is never held by a cap.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapPeriod {
    /// The local day.
    #[default]
    Day,
    /// The local calendar month.
    Month,
}

/// One cap: tokens, money or both, over a day or a month.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCap {
    pub period: CapPeriod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// Estimated USD (the price list's; models it does not list add none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSettings {
    #[serde(default)]
    pub caps: Vec<UsageCap>,
    /// 暂停所有 Agent: running turns are stopped, new messages refused,
    /// scheduled runs skipped quietly.
    #[serde(default)]
    pub paused: bool,
}

/// What unattended work spent in a period.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub tokens: u64,
    pub cost_usd: f64,
}

/// Whether unattended work may start now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Gate {
    Go,
    /// It may, but a cap is at 80 % or more (the usage page changes colour).
    Near {
        reason: String,
    },
    /// A cap is reached: unattended work does not start.
    Stop {
        reason: String,
    },
    /// Every agent is paused.
    Paused,
}

/// The budget as the settings page shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatus {
    pub settings: BudgetSettings,
    pub today: Spend,
    pub month: Spend,
    pub gate: Gate,
}
