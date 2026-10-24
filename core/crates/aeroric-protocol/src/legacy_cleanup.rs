//! Legacy custom Agent cleanup over the local UDS (B1, Q28/Q37).
//!
//! `plan` is read-only; `apply` acts only on the source ids the user confirmed.
//! Host-local: the remote plane never forwards these. No credentials and no
//! absolute paths cross the wire — the quarantine batch is a directory name
//! relative to `<data dir>/legacy-agent-quarantine/`.
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const CAPABILITY: &str = "agentConnections.legacyCleanup";
pub const PLAN_METHOD: &str = "agentConnections.legacyCleanup.plan";
pub const APPLY_METHOD: &str = "agentConnections.legacyCleanup.apply";
/// Upper bound on one apply; a real data root has a handful of custom Agents.
pub const MAX_SOURCE_IDS: usize = 256;
pub const SOURCE_PREFIX: &str = "custom:";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyRequest {
    pub schema_version: u32,
    pub source_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupReport {
    pub schema_version: u32,
    pub candidates: Vec<Candidate>,
    /// Batch directory this apply created; always `None` for plan and for an apply
    /// that only resumed or refused entries.
    pub quarantine_batch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Candidate {
    pub source: String,
    pub state: CandidateState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum CandidateState {
    /// Empty struct variants, not unit ones: serde ignores extra fields on unit
    /// variants of an internally tagged enum even with `deny_unknown_fields`.
    Eligible {},
    Blocked {
        reason: String,
    },
    Partial {
        remaining: Vec<CleanupStep>,
        reason: Option<String>,
    },
    Done {},
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupStep {
    RemoveProfile,
    QuarantineSidecar,
    QuarantineWrapper,
    DeleteOldSecrets,
}

impl ApplyRequest {
    /// Shape rules beyond what serde enforces. Returns the ids in request order
    /// without duplicates.
    pub fn validated_ids(&self) -> Result<Vec<String>, String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported schemaVersion {}; this host speaks {SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if self.source_ids.is_empty() {
            return Err("sourceIds must name at least one confirmed source".into());
        }
        let mut ids: Vec<String> = Vec::new();
        for id in &self.source_ids {
            let name = id.strip_prefix(SOURCE_PREFIX).unwrap_or_default();
            if name.trim().is_empty() {
                return Err(format!(
                    "`{id}` is not a custom Agent source; only `{SOURCE_PREFIX}<id>` can be cleaned up"
                ));
            }
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        if ids.len() > MAX_SOURCE_IDS {
            return Err(format!("at most {MAX_SOURCE_IDS} sources per apply"));
        }
        Ok(ids)
    }
}

pub fn fixture_plan() -> CleanupReport {
    CleanupReport {
        schema_version: SCHEMA_VERSION,
        candidates: vec![
            Candidate {
                source: "custom:fixture-ready".into(),
                state: CandidateState::Eligible {},
            },
            Candidate {
                source: "custom:fixture-rotated".into(),
                state: CandidateState::Blocked {
                    reason: "new credential `connection:legacy-connection-custom-66:credential` does not match the legacy credential".into(),
                },
            },
            Candidate {
                source: "custom:fixture-partial".into(),
                state: CandidateState::Partial {
                    remaining: vec![CleanupStep::QuarantineWrapper, CleanupStep::DeleteOldSecrets],
                    reason: Some("QuarantineWrapper: legacy launcher is no longer an Aeroric-generated wrapper; left in place".into()),
                },
            },
            Candidate {
                source: "custom:fixture-done".into(),
                state: CandidateState::Done {},
            },
        ],
        quarantine_batch: None,
    }
}

pub fn fixture_apply_request() -> ApplyRequest {
    ApplyRequest {
        schema_version: SCHEMA_VERSION,
        source_ids: vec!["custom:fixture-ready".into()],
    }
}

pub fn fixture_apply() -> CleanupReport {
    let mut report = fixture_plan();
    report.candidates[0].state = CandidateState::Done {};
    report.quarantine_batch = Some("20261024T120000.000Z-0".into());
    report
}

pub fn schema() -> serde_json::Value {
    use serde_json::json;
    let steps = json!({"enum":["removeProfile","quarantineSidecar","quarantineWrapper","deleteOldSecrets"]});
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"aeroric-native-legacy-agent-cleanup-v1", "capability":CAPABILITY,
        "source":"aeroric-protocol/src/legacy_cleanup.rs + docs/native/command-map.json nativeServices",
        "hostLocal":"Not forwarded by the remote plane.",
        "methods":{
            PLAN_METHOD:{"mutating":false,"params":{"type":"object","additionalProperties":false,"properties":{}},
                "result":{"$ref":"#/$defs/report"}},
            APPLY_METHOD:{"mutating":true,"params":{"$ref":"#/$defs/applyRequest"},
                "result":{"$ref":"#/$defs/report"}}
        },
        "order":"removeProfile → quarantineSidecar → quarantineWrapper → deleteOldSecrets; a failed step stops that entry as partial without rollback; apply again resumes it.",
        "$defs":{
            "applyRequest":{"type":"object","additionalProperties":false,"required":["schemaVersion","sourceIds"],
                "properties":{"schemaVersion":{"const":SCHEMA_VERSION},
                    "sourceIds":{"type":"array","minItems":1,"maxItems":MAX_SOURCE_IDS,
                        "items":{"type":"string","pattern":"^custom:.*\\S"}}}},
            "report":{"type":"object","additionalProperties":false,"required":["schemaVersion","candidates","quarantineBatch"],
                "properties":{"schemaVersion":{"const":SCHEMA_VERSION},
                    "candidates":{"type":"array","items":{"$ref":"#/$defs/candidate"}},
                    "quarantineBatch":{"type":["string","null"]}}},
            "candidate":{"type":"object","additionalProperties":false,"required":["source","state"],
                "properties":{"source":{"type":"string"},"state":{"oneOf":[
                    {"type":"object","additionalProperties":false,"required":["state"],"properties":{"state":{"const":"eligible"}}},
                    {"type":"object","additionalProperties":false,"required":["state","reason"],
                        "properties":{"state":{"const":"blocked"},"reason":{"type":"string"}}},
                    {"type":"object","additionalProperties":false,"required":["state","remaining","reason"],
                        "properties":{"state":{"const":"partial"},"remaining":{"type":"array","items":steps},
                            "reason":{"type":["string","null"]}}},
                    {"type":"object","additionalProperties":false,"required":["state"],"properties":{"state":{"const":"done"}}}
                ]}}}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fixtures_round_trip_strictly() {
        for report in [fixture_plan(), fixture_apply()] {
            let wire = serde_json::to_value(&report).unwrap();
            let back: CleanupReport = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(back, report);
            assert_eq!(serde_json::to_value(&back).unwrap(), wire);
        }
        let wire = serde_json::to_value(fixture_apply_request()).unwrap();
        assert_eq!(
            wire,
            json!({"schemaVersion":1,"sourceIds":["custom:fixture-ready"]})
        );
        let states: Vec<_> = fixture_plan()
            .candidates
            .iter()
            .map(|c| serde_json::to_value(&c.state).unwrap()["state"].clone())
            .collect();
        assert_eq!(
            states,
            [
                json!("eligible"),
                json!("blocked"),
                json!("partial"),
                json!("done")
            ]
        );
        assert_eq!(
            serde_json::to_value(fixture_plan()).unwrap()["quarantineBatch"],
            json!(null)
        );
    }

    #[test]
    fn unknown_fields_and_bad_shapes_are_rejected() {
        assert!(serde_json::from_value::<ApplyRequest>(
            json!({"schemaVersion":1,"sourceIds":["custom:a"],"force":true})
        )
        .is_err());
        assert!(serde_json::from_value::<CleanupReport>(
            json!({"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"eligible","extra":1}}],"quarantineBatch":null})
        )
        .is_err());
        assert!(serde_json::from_value::<CleanupReport>(
            json!({"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"exploded"}}],"quarantineBatch":null})
        )
        .is_err());
    }

    #[test]
    fn apply_ids_must_be_custom_sources() {
        let request = |ids: &[&str], version| ApplyRequest {
            schema_version: version,
            source_ids: ids.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(
            request(&["custom:a", "custom:b", "custom:a"], 1)
                .validated_ids()
                .unwrap(),
            ["custom:a", "custom:b"]
        );
        for bad in [
            &[][..],
            &["builtin:claude"],
            &["custom:"],
            &["custom:  "],
            &["a"],
        ] {
            assert!(request(bad, 1).validated_ids().is_err(), "{bad:?}");
        }
        assert!(request(&["custom:a"], 2).validated_ids().is_err());
        let many: Vec<String> = (0..=MAX_SOURCE_IDS)
            .map(|i| format!("custom:{i}"))
            .collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(request(&many, 1).validated_ids().is_err());
    }

    #[test]
    fn schema_names_both_methods_and_every_state() {
        let schema = schema();
        // Key order depends on serde_json's `preserve_order`, which another crate in
        // the workspace may switch on.
        let mut methods: Vec<_> = schema["methods"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        methods.sort();
        assert_eq!(methods, [APPLY_METHOD, PLAN_METHOD]);
        assert_eq!(
            schema["$defs"]["candidate"]["properties"]["state"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
}
