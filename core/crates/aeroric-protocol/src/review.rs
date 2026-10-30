//! B4-1 review loop (2026-10-30): another agent reviews a turn's changes in
//! isolation and read-only; its findings go back to the session that wrote
//! them, which answers each one (frontier-roadmap §5.2 form A, Q-B1–Q-B7).
//!
//! One loop is a [`ReviewCard`]; every step of it appends a `review` session
//! update with the same `reviewId`, and a client shows only the latest.

use serde::{Deserialize, Serialize};

pub const GET_METHOD: &str = "review.get";
pub const SET_METHOD: &str = "review.set";
pub const REQUEST_METHOD: &str = "review.request";
pub const SEND_METHOD: &str = "review.send";
pub const RETRY_METHOD: &str = "review.retry";
pub const REVIEWERS_METHOD: &str = "review.reviewers";
pub const ATTENTION_METHOD: &str = "review.attention";

/// Rounds per loop; past this the loop stops and asks the user (P17).
pub const MAX_ROUNDS: u32 = 3;
/// A finding's text beyond this is cut.
pub const MAX_TEXT_CHARS: usize = 600;
/// The reviewer's raw answer kept when it held no readable verdict.
pub const MAX_RAW_CHARS: usize = 4000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewState {
    /// The reviewer is working on this round.
    #[default]
    Reviewing,
    /// Findings are in, held back until the user sends them (paused).
    WaitingSend,
    /// Findings were sent; waiting for the writer's answers.
    WaitingCoder,
    /// No blocking finding.
    Passed,
    /// Over without a pass: every blocker rejected, or nothing adopted.
    Settled,
    /// Needs the user: an "ask" answer, or past [`MAX_ROUNDS`].
    Escalated,
    /// The host restarted mid-round.
    Interrupted,
    /// The reviewer could not be started or gave no readable verdict.
    Failed,
}

impl ReviewState {
    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Reviewing | Self::WaitingSend | Self::WaitingCoder
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Blocker,
    Major,
    #[default]
    Minor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Adopt,
    Reject,
    Ask,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// The writer's answer, once it gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCard {
    pub review_id: String,
    pub round: u32,
    pub state: ReviewState,
    /// The turn this round reviews.
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_task_id: Option<String>,
    /// "Claude Code · sonnet".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reviewer_label: String,
    /// No other family was available, so the reviewer is the writer's own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub same_family: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The reviewer's answer when it held no readable verdict, or why the
    /// round failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl ReviewCard {
    pub fn blockers(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Blocker)
    }
}

/// Who reviews: `"auto"` (another family when there is one) or a named agent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ReviewerChoice {
    #[default]
    Auto,
    Named {
        agent: String,
        agent_config_id: Option<String>,
        model: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NamedReviewer {
    agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_config_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

impl Serialize for ReviewerChoice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::Named {
                agent,
                agent_config_id,
                model,
            } => NamedReviewer {
                agent: agent.clone(),
                agent_config_id: agent_config_id.clone(),
                model: model.clone(),
            }
            .serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ReviewerChoice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::String(tag) if tag == "auto" => Ok(Self::Auto),
            value @ serde_json::Value::Object(_) => {
                let named: NamedReviewer =
                    serde_json::from_value(value).map_err(D::Error::custom)?;
                Ok(Self::Named {
                    agent: named.agent,
                    agent_config_id: named.agent_config_id,
                    model: named.model,
                })
            }
            _ => Err(D::Error::custom(
                "reviewer is \"auto\" or {agent, agentConfigId?, model?}",
            )),
        }
    }
}

/// A task's review settings (its record's `reviewLoop` field).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub reviewer: ReviewerChoice,
    /// Hold findings until the user sends them.
    #[serde(default)]
    pub paused: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_read_auto_and_named_reviewers() {
        let auto: ReviewSettings = serde_json::from_value(json!({"enabled": true})).unwrap();
        assert_eq!(auto.reviewer, ReviewerChoice::Auto);
        assert_eq!(
            serde_json::to_value(&auto).unwrap(),
            json!({"enabled": true, "reviewer": "auto", "paused": false})
        );
        let named: ReviewSettings =
            serde_json::from_value(json!({"reviewer": {"agent": "claude", "model": "sonnet"}}))
                .unwrap();
        assert_eq!(
            named.reviewer,
            ReviewerChoice::Named {
                agent: "claude".into(),
                agent_config_id: None,
                model: Some("sonnet".into())
            }
        );
        assert!(serde_json::from_value::<ReviewSettings>(json!({"reviewer": "manual"})).is_err());
    }

    #[test]
    fn a_card_round_trips_with_only_what_it_has() {
        let card = ReviewCard {
            review_id: "rv-1".into(),
            round: 1,
            state: ReviewState::WaitingCoder,
            turn_id: "u1".into(),
            findings: vec![Finding {
                id: "r1".into(),
                severity: Severity::Blocker,
                text: "重试没有上限".into(),
                file: Some("src/client.rs".into()),
                line: Some(88),
                decision: Some(Decision::Adopt),
                reason: None,
            }],
            ..ReviewCard::default()
        };
        let value = serde_json::to_value(&card).unwrap();
        assert_eq!(value["state"], "waitingCoder");
        assert_eq!(value["findings"][0]["decision"], "adopt");
        assert!(value.get("raw").is_none());
        assert_eq!(serde_json::from_value::<ReviewCard>(value).unwrap(), card);
        assert_eq!(card.blockers().count(), 1);
    }
}
