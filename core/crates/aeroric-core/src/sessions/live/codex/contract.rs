//! Test support (E9): the `codex app-server` schema snapshots in
//! `contracts/codex/`, and a check that what the driver sends fits every one
//! of them. The fake peer runs it on each message it receives, so every
//! driver test is also a contract test against the oldest supported and the
//! newest checked Codex. A new `app-server-<version>/` directory is picked up
//! without editing any test.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{json, Value};

/// One `codex app-server generate-json-schema` snapshot.
pub struct Snapshot {
    pub name: String,
    pub client_requests: Value,
    pub server_requests: Value,
    pub notifications: Value,
}

fn version_of(name: &str) -> Option<Vec<u64>> {
    name.strip_prefix("app-server-")?
        .split('.')
        .map(|part| part.parse().ok())
        .collect()
}

/// Every snapshot, oldest first.
pub fn snapshots() -> &'static [Snapshot] {
    static ALL: OnceLock<Vec<Snapshot>> = OnceLock::new();
    ALL.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../contracts/codex");
        let mut found: Vec<(Vec<u64>, String)> = std::fs::read_dir(&root)
            .expect("contracts/codex")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                Some((version_of(&name)?, name))
            })
            .collect();
        found.sort();
        let read = |name: &str, file: &str| -> Value {
            let path = root.join(name).join(file);
            serde_json::from_slice(
                &std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            )
            .unwrap()
        };
        found
            .into_iter()
            .map(|(_, name)| Snapshot {
                client_requests: read(&name, "ClientRequest.json"),
                server_requests: read(&name, "ServerRequest.json"),
                notifications: read(&name, "ServerNotification.json"),
                name,
            })
            .collect()
    })
}

/// The `oneOf` entry for `method` in one of a snapshot's message files.
fn entry<'a>(file: &'a Value, method: &str) -> Option<&'a Value> {
    file["oneOf"].as_array()?.iter().find(|entry| {
        entry
            .pointer("/properties/method/enum/0")
            .and_then(Value::as_str)
            == Some(method)
    })
}

/// The methods a snapshot's message file declares.
pub fn methods(file: &Value) -> Vec<String> {
    file["oneOf"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| e.pointer("/properties/method/enum/0")?.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn resolve<'a>(root: &'a Value, reference: &str) -> &'a Value {
    let name = reference
        .strip_prefix("#/definitions/")
        .unwrap_or_else(|| panic!("unsupported $ref {reference}"));
    root.get("definitions")
        .and_then(|d| d.get(name))
        .unwrap_or_else(|| panic!("missing definition {name}"))
}

fn type_fits(name: &str, value: &Value) -> bool {
    match name {
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => true,
    }
}

/// Checks `value` against the subset of JSON Schema these snapshots use
/// (`$ref`, `anyOf` / `oneOf` / `allOf`, `type`, `enum`, `const`,
/// `properties`, `required`, `additionalProperties`, `items`).
///
/// Stricter than JSON Schema in one way, on purpose: a field the schema's
/// `properties` do not name is an error unless `additionalProperties` allows
/// it. Codex ignores such a field, so a renamed parameter would otherwise be
/// dropped without a word.
pub fn check(root: &Value, schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    check_as(root, schema, value, path, Fill::Whole, errors)
}

/// Whether the value must carry every required field. A test's stand-in for
/// what Codex sends is `Partial`: it names only what the case needs, but
/// every field it names must be one Codex sends, with a value Codex sends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    Whole,
    Partial,
}

fn check_as(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    fill: Fill,
    errors: &mut Vec<String>,
) {
    if schema == &Value::Bool(true) {
        return;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return check_as(root, resolve(root, reference), value, path, fill, errors);
    }
    // Fields the schema names beside its branches are checked here, not by
    // each branch (`{"type": "audio"}` + one of `{url}` / `{fileId}`).
    let own = schema.get("properties").and_then(Value::as_object);
    let narrowed = match (own, value.as_object()) {
        (Some(own), Some(object)) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !own.contains_key(*key))
                .map(|(key, field)| (key.clone(), field.clone()))
                .collect(),
        ),
        _ => value.clone(),
    };
    let mut branch_fields: Vec<&str> = Vec::new();
    for key in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(key).and_then(Value::as_array) {
            for branch in branches {
                let branch = match branch.get("$ref").and_then(Value::as_str) {
                    Some(reference) => resolve(root, reference),
                    None => branch,
                };
                if let Some(names) = branch.get("properties").and_then(Value::as_object) {
                    branch_fields.extend(names.keys().map(String::as_str));
                }
            }
            let mut closest: Option<Vec<String>> = None;
            for branch in branches {
                let mut found = Vec::new();
                check_as(root, branch, &narrowed, path, fill, &mut found);
                if found.is_empty() {
                    closest = None;
                    break;
                }
                if closest.as_ref().is_none_or(|c| found.len() < c.len()) {
                    closest = Some(found);
                }
            }
            if let Some(closest) = closest {
                errors.extend(closest);
                return;
            }
        }
    }
    if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
        for part in parts {
            check_as(root, part, value, path, fill, errors);
        }
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(one)) => vec![one.as_str()],
        Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !types.is_empty() && !types.iter().any(|t| type_fits(t, value)) {
        errors.push(format!("{path}: {value} is not {}", types.join(" or ")));
        return;
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            errors.push(format!(
                "{path}: {value} is not one of {}",
                Value::from(allowed.clone())
            ));
        }
    }
    if let Some(constant) = schema.get("const") {
        if constant != value {
            errors.push(format!("{path}: {value} is not {constant}"));
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        for name in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if fill == Fill::Whole && !object.contains_key(name) {
                errors.push(format!("{path}.{name}: required, not sent"));
            }
        }
        for (key, field) in object {
            let at = format!("{path}.{key}");
            match (
                properties.and_then(|p| p.get(key)),
                schema.get("additionalProperties"),
            ) {
                (Some(property), _) => check_as(root, property, field, &at, fill, errors),
                (None, Some(Value::Bool(true))) => {}
                (None, Some(extra)) if extra.is_object() => {
                    check_as(root, extra, field, &at, fill, errors)
                }
                (None, Some(_)) => errors.push(format!("{at}: not a field this Codex knows")),
                (None, None) if branch_fields.contains(&key.as_str()) => {}
                (None, None) if properties.is_some() => {
                    errors.push(format!("{at}: not a field this Codex knows"))
                }
                (None, None) => {}
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            check_as(root, items, item, &format!("{path}[{index}]"), fill, errors);
        }
    }
}

/// What one snapshot finds wrong with a message the driver sent. `asked`
/// maps the ids of the requests the peer sent to their methods, so an answer
/// is checked against the request it answers.
pub fn problems(
    snapshot: &Snapshot,
    message: &Value,
    asked: &HashMap<String, String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    let method = message.get("method").and_then(Value::as_str);
    match (method, message.get("id")) {
        (Some(method), Some(_)) => {
            let root = &snapshot.client_requests;
            let Some(entry) = entry(root, method) else {
                return vec![format!("{method}: not a request this Codex takes")];
            };
            let params = message.get("params").unwrap_or(&Value::Null);
            match entry.pointer("/properties/params") {
                Some(schema) => check(root, schema, params, method, &mut errors),
                None if params.is_null() => {}
                None => errors.push(format!("{method}: this Codex takes no params")),
            }
        }
        // Client notifications (`initialized`) have no schema in the snapshot.
        (Some(_), None) => {}
        (None, Some(id)) if message.get("result").is_some() => {
            let key = id.to_string();
            if asked.get(&key).map(String::as_str) == Some("item/commandExecution/requestApproval")
            {
                let root = &snapshot.server_requests;
                let schema = json!({
                    "type": "object",
                    "properties": {"decision": {"$ref": "#/definitions/CommandExecutionApprovalDecision"}},
                    "required": ["decision"]
                });
                check(
                    root,
                    &schema,
                    &message["result"],
                    "commandExecution answer",
                    &mut errors,
                );
            }
        }
        _ => {}
    }
    errors
}

/// What one snapshot finds wrong with a message a test plays as Codex: a
/// server request or a notification. Checked as [`Fill::Partial`].
pub fn problems_from_codex(snapshot: &Snapshot, message: &Value) -> Vec<String> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Vec::new();
    };
    let (root, kind) = if message.get("id").is_some() {
        (&snapshot.server_requests, "request")
    } else {
        (&snapshot.notifications, "notification")
    };
    let Some(entry) = entry(root, method) else {
        return vec![format!("{method}: not a {kind} this Codex sends")];
    };
    let mut errors = Vec::new();
    if let Some(schema) = entry.pointer("/properties/params") {
        let params = message.get("params").unwrap_or(&Value::Null);
        check_as(root, schema, params, method, Fill::Partial, &mut errors);
    }
    errors
}

/// Panics, naming the snapshot, when a test plays Codex sending what no
/// Codex of that version sends.
pub fn assert_codex_sends(message: &Value) {
    for snapshot in snapshots() {
        let errors = problems_from_codex(snapshot, message);
        assert!(
            errors.is_empty(),
            "the test plays Codex sending what {} does not send:\n  {}\nmessage: {message}",
            snapshot.name,
            errors.join("\n  ")
        );
    }
}

/// Panics, naming the snapshot, when a message the driver sent does not fit.
pub fn assert_fits(message: &Value, asked: &HashMap<String, String>) {
    for snapshot in snapshots() {
        let errors = problems(snapshot, message, asked);
        assert!(
            errors.is_empty(),
            "the driver sent what {} does not take:\n  {}\nmessage: {message}",
            snapshot.name,
            errors.join("\n  ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors(message: Value) -> Vec<String> {
        let asked = HashMap::from([(
            "7".to_string(),
            "item/commandExecution/requestApproval".to_string(),
        )]);
        snapshots()
            .iter()
            .flat_map(|s| problems(s, &message, &asked))
            .collect()
    }

    #[test]
    fn every_snapshot_is_found_oldest_first() {
        let names: Vec<&str> = snapshots().iter().map(|s| s.name.as_str()).collect();
        assert!(names.len() >= 2, "{names:?}");
        assert_eq!(
            names[0], "app-server-0.162.1",
            "the oldest supported comes first"
        );
        assert!(names.contains(&"app-server-0.167.0"));
        assert!(names.contains(&"app-server-0.169.1"));
        assert_eq!(version_of("app-server-0.170.10"), Some(vec![0, 170, 10]));
        assert_eq!(version_of("README.md"), None);
    }

    #[test]
    fn a_fitting_request_has_no_problems() {
        let start = json!({"jsonrpc": "2.0", "id": 1, "method": "thread/start",
            "params": {"cwd": "/w", "approvalPolicy": "on-request", "sandbox": "workspace-write", "model": null}});
        assert_eq!(errors(start), Vec::<String>::new());
        let answer = json!({"jsonrpc": "2.0", "id": 7, "result": {"decision": "acceptForSession"}});
        assert_eq!(errors(answer), Vec::<String>::new());
        assert_eq!(
            errors(json!({"jsonrpc": "2.0", "method": "initialized"})),
            Vec::<String>::new()
        );
    }

    /// What an upgrade breaks shows up by name: a renamed field, a missing
    /// required one, a value Codex does not take, a method it dropped, an
    /// answer it would not understand.
    #[test]
    fn each_kind_of_misfit_is_named() {
        let renamed = errors(
            json!({"id": 1, "method": "thread/start", "params": {"sandboxMode": "read-only"}}),
        );
        assert!(
            renamed
                .iter()
                .any(|e| e == "thread/start.sandboxMode: not a field this Codex knows"),
            "{renamed:?}"
        );

        let missing =
            errors(json!({"id": 1, "method": "turn/start", "params": {"threadId": "th1"}}));
        assert!(
            missing
                .iter()
                .any(|e| e == "turn/start.input: required, not sent"),
            "{missing:?}"
        );

        let value =
            errors(json!({"id": 1, "method": "thread/start", "params": {"sandbox": "everything"}}));
        assert!(
            value.iter().any(|e| e.starts_with("thread/start.sandbox:")),
            "{value:?}"
        );

        let input = errors(json!({"id": 1, "method": "turn/start",
            "params": {"threadId": "th1", "input": [{"type": "text", "txt": "hi"}]}}));
        assert!(
            input.iter().any(|e| e.starts_with("turn/start.input[0]")),
            "{input:?}"
        );

        let gone = errors(json!({"id": 1, "method": "thread/forkInto", "params": {}}));
        assert!(
            gone.iter()
                .any(|e| e == "thread/forkInto: not a request this Codex takes"),
            "{gone:?}"
        );

        let answer = errors(json!({"id": 7, "result": {"decision": "approve"}}));
        assert!(
            answer
                .iter()
                .any(|e| e.starts_with("commandExecution answer.decision")),
            "{answer:?}"
        );
        assert_eq!(
            errors(json!({"id": 8, "result": {"anything": 1}})),
            Vec::<String>::new(),
            "an answer with no schema is not judged"
        );
    }

    /// 0.167.0 names an audio input's `type` beside its `url` / `fileId`
    /// branches; neither side calls the other's field unknown.
    #[test]
    fn fields_beside_branches_are_not_unknown_to_either() {
        let audio = |input: Value| {
            let turn = json!({"id": 1, "method": "turn/start", "params": {"threadId": "th1", "input": [input]}});
            problems(snapshots().last().unwrap(), &turn, &HashMap::new())
        };
        assert_eq!(
            audio(json!({"type": "audio", "url": "u"})),
            Vec::<String>::new()
        );
        assert_eq!(
            audio(json!({"type": "audio", "fileId": "f"})),
            Vec::<String>::new()
        );
        assert!(
            !audio(json!({"type": "audio"})).is_empty(),
            "neither url nor fileId"
        );
    }

    /// What a test plays as Codex may leave fields out, but each field it
    /// names, and each value, must be one Codex sends.
    #[test]
    fn a_stand_in_for_codex_is_checked_but_may_be_partial() {
        let from_codex = |message: Value| -> Vec<String> {
            snapshots()
                .iter()
                .flat_map(|s| problems_from_codex(s, &message))
                .collect()
        };
        let partial = json!({"method": "turn/completed", "params": {"turn": {"id": "u1", "status": "completed"}}});
        assert_eq!(
            from_codex(partial),
            Vec::<String>::new(),
            "threadId left out"
        );
        let approval = json!({"id": 3, "method": "item/commandExecution/requestApproval",
            "params": {"threadId": "th1", "command": "ls"}});
        assert_eq!(from_codex(approval), Vec::<String>::new());

        let state = from_codex(
            json!({"method": "turn/completed", "params": {"turn": {"status": "paused"}}}),
        );
        assert!(
            state
                .iter()
                .any(|e| e.starts_with("turn/completed.turn.status:")),
            "{state:?}"
        );
        let field =
            from_codex(json!({"method": "turn/started", "params": {"turn": {"turnId": "u1"}}}));
        assert!(
            field
                .iter()
                .any(|e| e == "turn/started.turn.turnId: not a field this Codex knows"),
            "{field:?}"
        );
        let method = from_codex(json!({"method": "turn/paused", "params": {}}));
        assert!(
            method
                .iter()
                .any(|e| e == "turn/paused: not a notification this Codex sends"),
            "{method:?}"
        );
        let request =
            from_codex(json!({"id": 1, "method": "item/shell/requestApproval", "params": {}}));
        assert!(
            request
                .iter()
                .any(|e| e == "item/shell/requestApproval: not a request this Codex sends"),
            "{request:?}"
        );
    }

    #[test]
    fn free_form_config_takes_any_key() {
        let start = json!({"id": 1, "method": "thread/start",
            "params": {"config": {"mcp_servers": {"aeroric_context": {"command": "x", "required": true}}}}});
        assert_eq!(errors(start), Vec::<String>::new());
    }
}
