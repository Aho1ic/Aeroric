//! Opt-in native composer contract. No credentials, executable paths or argv.
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const CAPABILITY: &str = "task.submissions";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigurationCatalog {
    pub schema_version: u32,
    pub project_id: String,
    pub revision: String,
    pub as_of: String,
    pub configurations: Vec<AgentConfiguration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfiguration {
    pub id: String,
    pub version: String,
    pub display_name: String,
    pub agent: String,
    pub harness: String,
    pub models: Vec<String>,
    /// Empty until the stored configuration has an authoritative effort catalog.
    pub efforts: Vec<String>,
    pub permission_modes: Vec<String>,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmissionRequest {
    pub schema_version: u32,
    pub submission_id: String,
    pub project_id: String,
    pub configuration_id: String,
    pub configuration_version: String,
    pub fingerprint: String,
    pub agent: String,
    pub prompt: String,
    pub permission_mode: String,
    /// An explicit choice for this Agent, not an inferred cross-Harness grant.
    pub permission_agent: String,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub attachments: Vec<String>,
    pub worktree: Option<String>,
    pub recipe_id: Option<String>,
    pub team: bool,
    pub review: bool,
    pub evolution: bool,
}

impl SubmissionRequest {
    /// Portable canonical bytes: header, then each listed field in order.
    /// Strings use an unsigned big-endian UTF-8 byte length; optional strings
    /// have a presence byte. Booleans are 0/1. No JSON sorting/escaping assumptions.
    /// The fingerprint and submission identity are intentionally excluded:
    /// fingerprint describes the immutable payload; identity names the attempt.
    pub fn fingerprint_material(&self) -> Vec<u8> {
        let mut out = b"aeroric.task-submission.v1\0".to_vec();
        fn text(out: &mut Vec<u8>, value: &str) {
            out.extend_from_slice(&(value.len() as u64).to_be_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        fn optional(out: &mut Vec<u8>, value: Option<&str>) {
            out.push(u8::from(value.is_some()));
            if let Some(value) = value {
                text(out, value);
            }
        }
        for value in [
            self.project_id.as_str(),
            &self.configuration_id,
            &self.configuration_version,
            &self.agent,
            &self.prompt,
            &self.permission_mode,
            &self.permission_agent,
        ] {
            text(&mut out, value);
        }
        optional(&mut out, self.selected_model.as_deref());
        optional(&mut out, self.reasoning_effort.as_deref());
        out.extend_from_slice(&(self.attachments.len() as u64).to_be_bytes());
        for path in &self.attachments {
            text(&mut out, path);
        }
        optional(&mut out, self.worktree.as_deref());
        optional(&mut out, self.recipe_id.as_deref());
        out.extend([
            u8::from(self.team),
            u8::from(self.review),
            u8::from(self.evolution),
        ]);
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionState {
    Pending,
    Accepted,
    RejectedBeforeSideEffect,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptionConsumption {
    pub option: String,
    pub applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmissionReceipt {
    pub schema_version: u32,
    pub submission_id: String,
    pub project_id: String,
    pub configuration_id: String,
    pub configuration_version: String,
    pub fingerprint: String,
    pub state: SubmissionState,
    pub task_id: Option<String>,
    pub reasons: Vec<String>,
    pub agent: String,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub permission_mode: String,
    pub permission_agent: String,
    pub options: Vec<OptionConsumption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmissionLookup {
    pub schema_version: u32,
    pub submission_id: String,
    pub fingerprint: String,
    /// None is UNKNOWN, never permission to edit or submit again.
    pub receipt: Option<SubmissionReceipt>,
}

/// Synthetic cross-language fixture. No installed Agent or local-machine paths.
pub fn fixture_request() -> SubmissionRequest {
    SubmissionRequest {
        schema_version: 1,
        submission_id: "fixture-submission".into(),
        project_id: "fixture-project".into(),
        configuration_id: "custom-fixture".into(),
        configuration_version: "fixture-v1".into(),
        fingerprint: "sha256:cb156eef8bed21a31bc0ad0a020b528c229423b5cd6f6eea12bcfa5efd256e26"
            .into(),
        agent: "custom-fixture".into(),
        prompt: "目标：保留\n原文 / \\\"".into(),
        permission_mode: "ask".into(),
        permission_agent: "custom-fixture".into(),
        selected_model: Some("fixture/model".into()),
        reasoning_effort: None,
        attachments: vec![],
        worktree: None,
        recipe_id: None,
        team: false,
        review: false,
        evolution: false,
    }
}

/// Supplementary native contract, separate from the frozen legacy inventory.
pub fn schema() -> serde_json::Value {
    use serde_json::json;
    let text = json!({"type":"string"});
    let optional = json!({"type":["string","null"]});
    let strings = json!({"type":"array","items":{"type":"string"}});
    let mut request = serde_json::Map::new();
    for key in [
        "submissionId",
        "projectId",
        "configurationId",
        "configurationVersion",
        "fingerprint",
        "agent",
        "prompt",
        "permissionMode",
        "permissionAgent",
    ] {
        request.insert(key.into(), text.clone());
    }
    for key in ["selectedModel", "reasoningEffort", "worktree", "recipeId"] {
        request.insert(key.into(), optional.clone());
    }
    for key in ["team", "review", "evolution"] {
        request.insert(key.into(), json!({"type":"boolean"}));
    }
    request.insert("schemaVersion".into(), json!({"const":1}));
    request.insert("attachments".into(), strings.clone());
    let mut receipt = request.clone();
    for key in [
        "prompt",
        "attachments",
        "worktree",
        "recipeId",
        "team",
        "review",
        "evolution",
    ] {
        receipt.remove(key);
    }
    receipt.insert(
        "state".into(),
        json!({"enum":["pending","accepted","rejected_before_side_effect","unknown"]}),
    );
    receipt.insert("taskId".into(), optional);
    receipt.insert("reasons".into(), strings.clone());
    receipt.insert("options".into(), json!({
        "type":"array","items":{"type":"object","additionalProperties":false,
        "required":["option","applied"],"properties":{"option":{"type":"string"},"applied":{"type":"boolean"}}}
    }));
    let required_request: Vec<_> = request.keys().cloned().collect();
    let required_receipt: Vec<_> = receipt.keys().cloned().collect();
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"aeroric-native-session-submission-v1", "capability":CAPABILITY,
        "source":"aeroric-protocol/src/submission.rs + docs/native/command-map.json nativeServices",
        "methods":{
            "agent.configurations":{"mutating":false,"params":{"type":"object","additionalProperties":false,
                "required":["projectId"],"properties":{"projectId":{"type":"string"}}},"result":{"$ref":"#/$defs/catalog"}},
            "task.run":{"mutating":true,"optIn":"schemaVersion/submissionId","params":{"$ref":"#/$defs/request"},
                "result":{"$ref":"#/$defs/receipt"}},
            "task.submission.get":{"mutating":false,"params":{"type":"object","additionalProperties":false,
                "required":["submissionId","fingerprint"],"properties":{"submissionId":{"type":"string"},"fingerprint":{"type":"string"}}},
                "result":{"$ref":"#/$defs/lookup"}}
        },
        "retryRule":"Never replay unknown/pending. Same identity plus same fingerprint returns the original receipt; a changed payload requires a new identity after explicit rejection.",
        "fingerprint":{
            "algorithm":"SHA-256", "prefix":"sha256:", "header":"aeroric.task-submission.v1\\0",
            "strings":"u64 big-endian UTF-8 byte length followed by bytes",
            "optional":"0 if null, otherwise 1 followed by the string",
            "order":["projectId","configurationId","configurationVersion","agent","prompt","permissionMode",
                "permissionAgent","selectedModel?","reasoningEffort?","attachments.length:u64","attachments[]",
                "worktree?","recipeId?","team:u8","review:u8","evolution:u8"]
        },
        "$defs":{
            "request":{"type":"object","additionalProperties":false,"properties":request,"required":required_request},
            "receipt":{"type":"object","additionalProperties":false,"properties":receipt,"required":required_receipt},
            "lookup":{"type":"object","additionalProperties":false,"required":["schemaVersion","submissionId","fingerprint","receipt"],
                "properties":{"schemaVersion":{"const":1},"submissionId":{"type":"string"},"fingerprint":{"type":"string"},
                    "receipt":{"anyOf":[{"$ref":"#/$defs/receipt"},{"type":"null"}]}}},
            "catalog":{"type":"object","additionalProperties":false,
                "required":["schemaVersion","projectId","revision","asOf","configurations"],
                "properties":{"schemaVersion":{"const":1},"projectId":{"type":"string"},"revision":{"type":"string"},
                    "asOf":{"type":"string","pattern":"^[0-9]+$"},"configurations":{"type":"array","items":{
                        "type":"object","additionalProperties":false,
                        "required":["id","version","displayName","agent","harness","models","efforts","permissionModes","blockedReasons"],
                        "properties":{"id":{"type":"string"},"version":{"type":"string"},"displayName":{"type":"string"},
                            "agent":{"type":"string"},"harness":{"type":"string"},"models":strings,"efforts":strings,
                            "permissionModes":strings,"blockedReasons":strings}}}}}
        }
    })
}
