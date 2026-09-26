//! Generates `contracts/native/v1/` from the Rust DTOs plus the frozen command
//! inventory (P2-01 Action 3/6). Rust is the source: the Swift and TypeScript
//! decoders read what this prints, and `scripts/native/contracts.mjs --check`
//! fails on drift without ever writing into the repository.
//!
//! The naming rule is re-implemented here on purpose. `meta.rpc_resolution` is
//! derived by `inventory.mjs`; if the two derivations disagree, generation fails
//! rather than publishing a schema that quietly renames or drops methods.
//!
//! Unknown verbs classify as *mutating*: a wrong guess there refuses a call, while
//! the opposite mistake would let an unrecognised mutation be replayed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aeroric_protocol::terminal::MAX_TERMINAL_CHUNK_BYTES;
use aeroric_protocol::{
    built_in_agents, detail_allowed_keys, error_codes, handshake_allows_peer, permission_modes,
    protocol_families, task_intent_ops, task_intent_ops_of, task_statuses, MAX_FRAME_BYTES,
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};

/// Verbs that only observe state. The generator refuses a verb no method uses.
const READ_VERBS: &[&str] = &[
    "get", "list", "read", "describe", "search", "query", "inspect", "preview", "assess",
    "validate",
];

/// Verbs that change state. Only used to decide which verb *leads* a command name:
/// a legacy name may stack a subsystem prefix in front of the action
/// (`remote_git_list_branches`), and a plain first-token test would call that a
/// read. Anything neither list knows stays mutating — the safe default, because an
/// unrecognised mutation must be refused when unknown rather than replayed, while a
/// read misread as mutating only costs the caller an idempotency key. Entries are
/// today's inventory, not a guess: a verb that stops matching is a dead rule.
const WRITE_VERBS: &[&str] = &[
    "create",
    "add",
    "set",
    "update",
    "replace",
    "write",
    "save",
    "delete",
    "remove",
    "rename",
    "move",
    "copy",
    "insert",
    "clear",
    "reset",
    "migrate",
    "apply",
    "commit",
    "merge",
    "checkout",
    "switch",
    "stage",
    "push",
    "pull",
    "fetch",
    "sync",
    "upload",
    "install",
    "uninstall",
    "upgrade",
    "register",
    "unregister",
    "start",
    "stop",
    "restart",
    "kill",
    "run",
    "launch",
    "execute",
    "send",
    "resume",
    "cancel",
    "interrupt",
    "archive",
    "restore",
    "discard",
    "fork",
    "attach",
    "mark",
    "import",
    "open",
];

/// Names where a read verb is really a *noun*: Redis has list/stream key types, so
/// `dbx_redis_list_remove` mutates a value despite leading with `list`. The
/// generator insists each entry would otherwise classify as read, so this list
/// cannot accumulate entries that no longer do anything.
const READ_VERB_IS_A_NOUN: &[&str] = &[
    "dbx_redis_list_remove",
    "dbx_redis_list_push",
    "dbx_redis_list_set",
];

/// First token that is an action, deciding read vs mutating; `None` when the name
/// leads with something neither list knows.
fn leading_action(command: &str) -> Option<bool> {
    for token in command.split('_') {
        if READ_VERBS.contains(&token) {
            return Some(false);
        }
        if WRITE_VERBS.contains(&token) {
            return Some(true);
        }
    }
    None
}

fn is_mutating(command: &str) -> bool {
    READ_VERB_IS_A_NOUN.contains(&command) || leading_action(command).unwrap_or(true)
}

/// Where a v1 event topic's name comes from. ADR-007 moved the authority into
/// `docs/native/event-topics.json`: a `mapped` row must name a document line there,
/// so the fixture check below validates provenance through the table instead of a
/// second list that could drift out of agreement with it.
const TOPIC_SOURCE_PATTERN: &str = "§";

/// Names a fixture uses *because* no host knows them: the forward-compatibility
/// cases. The generator refuses one that turns out to be published, and refuses one
/// no fixture uses.
const NEGATIVE_NAMES: &[&str] = &["some.newer.topic", "terminal.frobnicate_task"];

/// Collect every `"method"` and `"event"` string in a document tree.
fn collect_names(value: &Value, found: &mut BTreeMap<String, Vec<String>>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "method" || key == "event" {
                    if let Some(name) = child.as_str() {
                        found.entry(key.clone()).or_default().push(name.to_string());
                    }
                }
                collect_names(child, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_names(item, found);
            }
        }
        _ => {}
    }
}

/// A fixture is executable input for three decoders, not prose: a name in it that
/// nothing publishes is a client being taught to send a call that gets refused.
fn fixtures_are_sourced(
    documents: &[(String, Value)],
    methods: &[Value],
    topics: &[Value],
) -> Result<(), String> {
    let published_methods: Vec<&str> = methods.iter().filter_map(|m| m["name"].as_str()).collect();
    let published_events: Vec<&str> = topics.iter().filter_map(|t| t["topic"].as_str()).collect();
    let mut used: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, document) in documents {
        if !name.contains("fixtures/") {
            continue;
        }
        collect_names(document, &mut used);
    }
    let mut seen = Vec::new();
    for (key, names) in &used {
        for name in names {
            seen.push(name.as_str());
            let allowed = if key == "method" {
                published_methods.contains(&name.as_str())
            } else {
                // A topic must be in the ADR-007 table, or be a channel the bridge
                // has not renamed yet — never a name invented inside a fixture.
                published_events.contains(&name.as_str())
            } || NEGATIVE_NAMES.contains(&name.as_str());
            if !allowed {
                return Err(format!(
                    "fixture {key} {name:?} is published by nothing and sourced from nowhere: add the real name, or name its provenance in the const above"
                ));
            }
            if NEGATIVE_NAMES.contains(&name.as_str()) && published_events.contains(&name.as_str())
            {
                return Err(format!("{name:?} is declared negative but now published"));
            }
        }
    }
    for name in NEGATIVE_NAMES {
        if !seen.contains(name) {
            return Err(format!("{name:?} is declared but no fixture uses it"));
        }
    }
    // Q12: the published allowlist must actually describe the fixtures. A detail key
    // nobody declared would otherwise ship in the artifact and reach a log.
    let allowed = detail_allowed_keys();
    let mut details = Vec::new();
    for (name, document) in documents {
        if name.contains("fixtures/") {
            collect_detail_keys(document, &mut details);
        }
    }
    for key in details {
        if !allowed.contains(&key) {
            return Err(format!(
                "a fixture puts {key:?} into an error detail, which is not in the allowlist: {allowed:?}"
            ));
        }
    }
    Ok(())
}

/// Every object key that appears under a `"detail"` member of a document tree.
fn collect_detail_keys(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "detail" {
                    if let Some(fields) = child.as_object() {
                        found.extend(fields.keys().cloned());
                    }
                }
                collect_detail_keys(child, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_detail_keys(item, found);
            }
        }
        _ => {}
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn constants() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "taskStatuses": task_statuses(),
        "protocolFamilies": protocol_families(),
        "permissionModes": permission_modes(),
        "builtInAgents": built_in_agents(),
        "errorCodes": error_codes(),
        "storageRejections": storage_rejections(),
        "errorDetailAllowedKeys": detail_allowed_keys(),
        "taskIntents": {
            "ops": task_intent_ops(),
            "metadata": task_intent_ops_of("metadata"),
            "state": task_intent_ops_of("state"),
            "rule": "the save queue carries named intents, never whole-project snapshots (ADR-022); on a rejected write metadata ops replay exactly once and state ops never replay (ADR-026); delete-empty deletes tasks.json (ADR-037)",
            "authority": "aeroric_protocol::task_intents(); the authoritative enum is core's tasks::service::TaskIntent and a core test pins the two lists together"
        },
        "limits": { "maxFrameBytes": MAX_FRAME_BYTES, "maxTerminalChunkRawBytes": MAX_TERMINAL_CHUNK_BYTES,
                    "replay": { "maxEvents": 4096, "maxBytes": 8388608 } }
    })
}

/// ADR-011: how the three storage refusals look on the wire. Built from the Rust enum and
/// *validated* against ADR-008's allowlist here, so an unmappable key stops the contract
/// from being written rather than reaching a client as a detail nobody declared.
fn storage_rejections() -> Value {
    let allowed = aeroric_protocol::ErrorDetail::allowed_keys();
    let rows: Vec<Value> = aeroric_protocol::StorageRejection::ALL
        .iter()
        .map(|rejection| {
            let keys = rejection.detail_keys();
            for key in &keys {
                assert!(
                    allowed.iter().any(|allowed| allowed == *key),
                    "storage rejection {:?} declares detail key {key:?}, which is not in the ADR-008 allowlist",
                    rejection.as_str()
                );
            }
            json!({
                "rejection": rejection.as_str(),
                "code": rejection.error_code().as_str(),
                "detailKeys": keys,
                "retryable": rejection.retryable(),
            })
        })
        .collect();
    Value::Array(rows)
}

/// Task P2-03's parity table, published verbatim as a fixture. The rows were produced by
/// the shipping client's own TypeScript (`src/test/project-core-parity.test.ts` asserts
/// them every run), so this generator carries them into the artifact without inventing a
/// second copy of the expectations — and `contracts.mjs --check` fails if the committed
/// artifact and this embedded table ever disagree.
fn project_parity_cases() -> Value {
    let text = include_str!("../../knowledge/project-parity-cases.json");
    serde_json::from_str(text).expect("knowledge/project-parity-cases.json must be valid JSON")
}

/// Task P2-04's status/handoff/retention parity table, same rule as above: the shipped
/// client's own functions produced it, so the committed fixture cannot be quietly rewritten.
fn task_parity_cases() -> Value {
    let text = include_str!("../../knowledge/task-parity-cases.json");
    serde_json::from_str(text).expect("knowledge/task-parity-cases.json must be valid JSON")
}

fn quantity() -> Value {
    json!({
        "type": "string",
        "pattern": "^[0-9]+$",
        "reason": "decimal string: JS numbers lose precision past 2^53-1, and a wrapped seq or revision is silent data loss"
    })
}

/// Client-visible parameters of a frozen row. `params` is written by `inventory.mjs`
/// as `name: Type`, with a trailing `!` marking a Rust-injected handle (application
/// or state extractor) that no client ever sends.
///
/// The handle types are named descriptively rather than by their identifiers on
/// purpose: `inv02-core-source-scan` greps this crate for those identifiers, and a
/// comment that mentions one is a false positive that would teach someone to weaken
/// the scan instead of rewording the sentence.
fn visible_params(command: &Value) -> Vec<(String, String)> {
    command["params"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|entry| !entry.ends_with('!'))
                .filter(|entry| entry.contains(": "))
                .map(|entry| {
                    let mut halves = entry.splitn(2, ": ");
                    (
                        halves.next().unwrap_or_default().to_string(),
                        halves.next().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The members of `ProjectTarget`. A twin's transport handle is *consumed by the
/// target*, so it is not a parameter the collapsed method must also take.
const TARGET_FIELDS: &[&str] = &[
    "connection",
    "connection_id",
    "remote_path",
    "distribution",
    "linux_path",
];

/// `remote_project_path` and `linux_project_path` name the same argument the local
/// command calls `project_path`: the prefix is the transport, not the meaning.
fn normalize(name: &str) -> &str {
    ["remote_", "linux_", "wsl_"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(name)
}

/// ADR-006: a prefix twin collapses into its parent plus `params.target`. The
/// collapse is only publishable as `proven` when every remaining argument lines up
/// by name *and* type with the parent's — a silent rename or a changed optionality
/// would let a client send a payload the old command never accepted. Anything else
/// stays `pending: P3-06` with the diff recorded, and the host must refuse it.
fn twin_alias(rust_path: &str, twin: &Value, parent: &Value) -> Result<Value, String> {
    let parent_wire = parent["rpc_method"].as_str().ok_or_else(|| {
        format!(
            "{rust_path}: twin's parent {:?} has no wire name",
            parent["command"]
        )
    })?;
    let legacy = twin["command"].as_str().unwrap_or_default();
    let target_kind = if legacy.starts_with("wsl_") {
        "wsl"
    } else {
        "ssh"
    };
    let mine = visible_params(twin);
    let parents = visible_params(parent);
    let rest: Vec<&(String, String)> = mine
        .iter()
        .filter(|(name, _)| !TARGET_FIELDS.contains(&name.as_str()))
        .collect();
    let mut mapping: BTreeMap<String, String> = BTreeMap::new();
    let mut problems: Vec<String> = Vec::new();
    for (name, ty) in &rest {
        match parents
            .iter()
            .find(|(pname, _)| normalize(pname) == normalize(name))
        {
            Some((pname, pty)) => {
                mapping.insert(name.clone(), pname.clone());
                if pty != ty {
                    problems.push(format!("{name}: {ty} vs {pname}: {pty}"));
                }
            }
            None => problems.push(format!("{name}: {ty} has no parent argument")),
        }
    }
    let covered: Vec<&String> = mapping.values().collect();
    for (pname, _) in &parents {
        if !covered.contains(&pname) {
            problems.push(format!(
                "parent argument {pname} is not supplied by the twin"
            ));
        }
    }
    if rest.len() != parents.len() && problems.is_empty() {
        problems.push(format!(
            "arity differs: twin supplies {} of {} parent arguments",
            rest.len(),
            parents.len()
        ));
    }
    Ok(json!({
        "legacyCommand": legacy,
        "method": parent_wire,
        "targetKind": target_kind,
        "targetParams": mine.iter().filter(|(name, _)| TARGET_FIELDS.contains(&name.as_str()))
            .map(|(name, ty)| format!("{name}: {ty}")).collect::<Vec<_>>(),
        "paramMapping": if problems.is_empty() { "proven" } else { "pending: P3-06" },
        "paramDiff": problems,
        "requiresCapability": if target_kind == "wsl" { Value::String("wsl".into()) } else { Value::Null },
        "comment": "not a protocol method: renaming or dropping an alias is not a protocol change (ADR-006)"
    }))
}

/// One record per wire method, cross-checked against the derivation table. Prefix
/// twins leave the method surface entirely and become entries in `aliases`.
fn methods_from_inventory(map: &Value) -> Result<(Vec<Value>, Vec<Value>), String> {
    let table = map["meta"]["rpc_resolution"]["module_to_service"]
        .as_object()
        .ok_or("meta.rpc_resolution.module_to_service is absent")?;
    let commands = map["commands"]
        .as_array()
        .ok_or("`commands` is not an array")?;
    let by_command: BTreeMap<&str, &Value> = commands
        .iter()
        .filter_map(|c| c["command"].as_str().map(|name| (name, c)))
        .collect();
    let mut out = Vec::with_capacity(commands.len());
    let mut aliases: Vec<Value> = Vec::new();
    let mut skipped_unnamed = 0usize;
    for command in commands {
        let rust_path = command["rust_path"]
            .as_str()
            .ok_or("command without rust_path")?;
        let wire = match command["rpc_method"].as_str() {
            Some(name) => name,
            // 19 host_local_os_ui commands have no wire name by design (ADR-003).
            None => {
                if command["category"].as_str() != Some("host_local_os_ui") {
                    return Err(format!(
                        "{rust_path}: no rpc_method and is not host_local_os_ui"
                    ));
                }
                skipped_unnamed += 1;
                continue;
            }
        };
        let module = rust_path.split("::").next().unwrap_or_default();
        let leaf = rust_path.rsplit("::").next().unwrap_or_default();
        let service = table.get(module).and_then(Value::as_str).ok_or_else(|| {
            format!("{rust_path}: module {module:?} has no service mapping in meta.rpc_resolution")
        })?;
        let derived = format!("{service}.{leaf}");
        if wire != derived {
            return Err(format!(
                "{rust_path}: rpc_method {wire:?} != derived {derived:?}"
            ));
        }
        let legacy = command["command"]
            .as_str()
            .ok_or("command without `command`")?;
        if legacy != leaf {
            return Err(format!("{rust_path}: wire leaf {legacy:?} != rust leaf {leaf:?} (ADR-002 keeps them equal)"));
        }
        let category = command["category"].as_str().unwrap_or("domain_rpc");
        let reachability = &command["reachability"];
        let derived_of = reachability["prefix_derived_of"].as_str();
        let parent_live = reachability["prefix_derived_parent_reachable"].as_bool() == Some(true);
        let live = command["reachable"].as_bool() == Some(true);
        let evidence = command["caller_evidence"].as_str().unwrap_or("");
        let twin = derived_of.is_some() && parent_live && evidence.is_empty();
        if twin {
            let parent = *by_command
                .get(derived_of.unwrap_or_default())
                .ok_or_else(|| {
                    format!(
                        "{rust_path}: twin of {:?}, which the inventory does not list",
                        derived_of
                    )
                })?;
            aliases.push(twin_alias(rust_path, command, parent)?);
            continue;
        }
        // G16 triage, carried into the contract instead of being resolved by it: a
        // name is in the method surface because a caller literal, or ADR-005's typed
        // surface with per-row evidence, put it there. `uncalled-no-evidence` has
        // nothing behind it, so it is flagged rather than blessed — dropping or
        // keeping one is a protocol decision, not a migration detail.
        let (promotion, requires_decision) = if !evidence.is_empty() {
            (
                "stated-evidence".to_string(),
                !evidence.starts_with("proven"),
            )
        } else if live {
            ("caller-literal".to_string(), false)
        } else {
            ("uncalled-no-evidence".to_string(), true)
        };
        // ADR-003: the platform plane is capability-gated, and the capability is
        // named after the platform the legacy command actually needs. Only 21 of the
        // 23 non-twin platform commands mention WSL; the two cross-platform ones
        // (`get_platform_runtime_info`, `build_runnable_file_command`) must stay
        // callable on every host, so a blanket "wsl" would refuse calls that work
        // everywhere. WSL twins carry their gating on the alias instead.
        let capability = if category == "platform_specific" && legacy.contains("wsl") {
            Value::String("wsl".into())
        } else {
            Value::Null
        };
        out.push(json!({
            "name": wire,
            "service": service,
            "legacyCommand": legacy,
            "category": category,
            "mutating": is_mutating(legacy),
            "requiresCapability": capability,
            "coreService": command["core_service"],
            "checkModule": command["check_module"],
            "featureIds": command["feature_ids"],
            "promotion": promotion,
            "requiresTriageDecision": requires_decision,
            "takesTarget": command["category"].as_str() != Some("platform_specific"),
            "callerEvidence": command["caller_evidence"].clone()
        }));
    }
    if skipped_unnamed != 19 {
        return Err(format!(
            "expected 19 unnamed host_local_os_ui commands, saw {skipped_unnamed}"
        ));
    }
    let counted = |want: &str| out.iter().filter(|m| m["promotion"] == json!(want)).count();
    for (want, promotion) in [
        (449usize, "caller-literal"),
        (83, "stated-evidence"),
        // Q10 closed this group: every remaining name either has a caller literal or
        // states who decides. A non-zero count here means a name rode in on nothing.
        (0, "uncalled-no-evidence"),
    ] {
        if counted(promotion) != want {
            return Err(format!(
                "promotion {promotion:?} holds {} method(s), pinned at {want}: re-triage G16 before publishing",
                counted(promotion)
            ));
        }
    }
    if out.len() != 532 {
        return Err(format!(
            "the method surface holds {} names (pinned 532 = 641 - 109 twins): re-review ADR-006",
            out.len()
        ));
    }
    if aliases.len() != 109 {
        return Err(format!(
            "expected 109 prefix twins to become aliases, saw {}",
            aliases.len()
        ));
    }
    let gated = out
        .iter()
        .filter(|m| m["requiresCapability"] != Value::Null)
        .count();
    if gated != 21 {
        return Err(format!(
            "expected 21 wsl-gated methods after the collapse (ADR-003/006), saw {gated}"
        ));
    }
    let proven = aliases
        .iter()
        .filter(|a| a["paramMapping"] == json!("proven"))
        .count();
    if proven != 77 || aliases.len() - proven != 32 {
        return Err(format!(
            "alias mappings moved: {proven} proven / {} pending. Every pending alias must be refused by \
             the host until P3-06 maps it; a change here means a source signature changed.",
            aliases.len() - proven
        ));
    }
    let reads = out.iter().filter(|m| m["mutating"] == json!(false)).count();
    if reads != 120 {
        return Err(format!(
            "read-classified methods moved to {reads} (pinned 120): re-review the classification, then re-pin it here"
        ));
    }
    // A verb no method uses is a claim about the vocabulary that nothing checks, so
    // an unreadable list would rot silently. Both lists must earn their place.
    let names: Vec<&str> = out
        .iter()
        .chain(aliases.iter())
        .filter_map(|m| m["legacyCommand"].as_str())
        .collect();
    for verb in READ_VERBS.iter().chain(WRITE_VERBS) {
        if !names.iter().any(|n| n.split('_').any(|t| t == *verb)) {
            return Err(format!(
                "classification verb {verb:?} matches no method: dead rule"
            ));
        }
    }
    for name in READ_VERB_IS_A_NOUN {
        if leading_action(name) != Some(false) {
            return Err(format!("override {name:?} no longer needs overriding"));
        }
    }
    let flagged = out
        .iter()
        .filter(|m| m["requiresTriageDecision"] == json!(true))
        .count();
    if flagged != 59 {
        return Err(format!(
            "{flagged} methods are owed a triage decision, pinned at 59"
        ));
    }
    Ok((out, aliases))
}

fn events_from_inventory(map: &Value) -> Vec<Value> {
    map["events"]
        .as_array()
        .map(|events| {
            events
                .iter()
                .map(|event| {
                    let count = |field: &str| event[field].as_array().map(Vec::len).unwrap_or(0);
                    json!({
                        "name": event["name"],
                        "disposition": event["disposition"],
                        "orphan": event["orphan"],
                        "emitters": count("emitters"),
                        "listeners": count("listeners")
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn envelope_schema() -> Value {
    let open =
        "Carried forward unchanged: an addition by a newer host must not fail to parse here.";
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "aeroric-local-protocol-v1-envelope",
        "constants": constants(),
        "transport": {
            "socket": "unix domain socket",
            "framing": "UTF-8 JSONL, one frame per line",
            "maxFrameBytes": MAX_FRAME_BYTES,
            "maxTerminalChunkRawBytes": MAX_TERMINAL_CHUNK_BYTES,
            "controlQueue": { "maxItems": 256, "maxBytes": 4194304 },
            "replay": { "maxEvents": 4096, "maxBytes": 8388608, "beyond": "SNAPSHOT_REQUIRED" }
        },
        "frameKinds": {
            "request": {
                "required": ["v", "id", "method", "params"],
                "properties": {
                    "v": { "const": PROTOCOL_VERSION },
                    "id": { "type": "string" },
                    "method": { "type": "string", "pattern": "^[a-z0-9_]+\\.[A-Za-z0-9_]+$" },
                    "params": { "type": "object" },
                    "idempotencyKey": { "type": "string", "requiredFor": "mutating", "purpose": "retry deduplication" }
                },
                "openContent": open
            },
            "success": {
                "required": ["v", "id", "ok", "result"],
                "properties": { "ok": { "const": true }, "result": { "type": "object" } },
                "openContent": open
            },
            "failure": {
                "required": ["v", "id", "ok", "error"],
                "properties": {
                    "ok": { "const": false },
                    "error": {
                        "required": ["code", "message", "retryable"],
                        "properties": {
                            "code": { "enum": error_codes(), "unknownFallback": "UNKNOWN" },
                            "retryable": { "type": "boolean" },
                            "detail": {
                                "type": "object",
                                "mustNotContain": ["token", "password", "passphrase", "api key", "connection string", "raw session content"]
                            }
                        }
                    }
                },
                "openContent": open
            },
            "event": {
                "required": ["v", "event", "epoch", "seq", "data"],
                "properties": {
                    "event": { "type": "string" },
                    "epoch": { "type": "string", "note": "changes on host restart; seq never carries over" },
                    "seq": quantity(),
                    "data": { "type": "object" }
                },
                "openContent": open
            }
        },
        "versionPolicy": {
            "local": "the `v` in every frame must equal constants.protocolVersion exactly; `handshake_allows_peer` is the one implementation of that rule",
            "mismatch": "refuse at the handshake with a stated reason; never degrade to a partial connection",
            "compatibilityInsideMajor": "carried by named capabilities in HostCapabilities, never inferred from a version number — the same rule P7-02 fixed for OS assumptions (ADR-003)",
            "newMajor": "introducing v2 requires its own ADR first: method, topic and enum names become frozen by whatever replaces this",
            "acceptsSameMajor": handshake_allows_peer(PROTOCOL_VERSION),
            "rejectsAnyOtherVersion": !handshake_allows_peer(PROTOCOL_VERSION + 1)
                && !handshake_allows_peer(PROTOCOL_VERSION - 1)
                && !handshake_allows_peer(0),
            "remote": "the relay/mobile protocol is a separate version space: `packages/remote-contracts` declares no version constant today, so any bridge must name the pairs it maps (P8-03) instead of assuming the two move together"
        },
        "errorDetail": {
            "keys": detail_allowed_keys(),
            "rule": "closed set (Q12): a host builds `detail` from ErrorDetail, so an unregistered key cannot reach the wire without a diff-visible change to that type",
            "whyClosed": "frames keep unknown content for forward compatibility, but `detail` is where diagnostics go, and diagnostics get pasted into review evidence and .artifacts logs — from P7-04 real connection strings live nearby",
            "mustNotContain": ["token", "password", "passphrase", "api key", "connection string", "raw session content"]
        },
        "quantities": {
            "seq": quantity(),
            "revision": quantity(),
            "generation": quantity(),
            "byteOffset": quantity(),
            "timestamps": { "type": "integer", "unit": "epoch milliseconds" }
        },
        "terminal": {
            "identity": ["taskId", "generation"],
            "bytesField": "bytesBase64",
            "note": "raw PTY bytes travel as base64 inside JSON; remote/mobile keep their own binary framing behind an explicit adapter"
        },
        "ownership": {
            "singleWriter": "one data directory has exactly one business writer (INV-03)",
            "alreadyInUse": "must report the lock holder and a safe exit; never kill a PID automatically"
        },
        "cancellation": {
            "rpcCancel": "abandons the local wait only",
            "taskCancel": "terminal.cancel_task is what stops an agent",
            "disconnect": "a socket close is not a task end"
        },
        "resync": {
            "startAfterCrash": "an uncertain terminal.run_task is reconciled by querying the task's generation, never replayed blindly",
            "bufferTruncation": "emit an explicit reset plus truncated flag; do not resume feeding bytes mid escape sequence"
        }
    })
}

fn methods_schema(methods: &[Value], aliases: &[Value]) -> Value {
    let mutating = methods
        .iter()
        .filter(|m| m["mutating"] == json!(true))
        .count();
    let gated = methods
        .iter()
        .filter(|m| m["requiresCapability"] != Value::Null)
        .count();
    let mut by_service: BTreeMap<String, usize> = BTreeMap::new();
    for method in methods {
        *by_service
            .entry(method["service"].as_str().unwrap_or_default().to_string())
            .or_default() += 1;
    }
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "aeroric-local-protocol-v1-methods",
        "protocolVersion": PROTOCOL_VERSION,
        "source": "docs/native/command-map.json + meta.rpc_resolution (ADR-002), re-derived independently by export-schema",
        "count": methods.len(),
        "mutatingCount": mutating,
        "readCount": methods.len() - mutating,
        "capabilityGatedCount": gated,
        "classificationRule": {
            "readVerbs": READ_VERBS,
            "writeVerbs": WRITE_VERBS,
            "readVerbIsANoun": READ_VERB_IS_A_NOUN,
            "how": "the first token that is an action decides; a subsystem prefix in front of it is skipped",
            "default": "mutating",
            "why": "an unrecognised verb is treated as a mutation so it is refused when unknown instead of being replayed; a read misclassified as mutating only costs an idempotency key",
            "advisory": "says which calls must carry an idempotencyKey, nothing more: a client must never decide that a retry is safe from this field"
        },
        "byService": by_service,
        "surfaceProvenance": {
            "how": "each row carries `promotion`: how its name was earned in the G16 triage",
            "caller-literal": methods.iter().filter(|m| m["promotion"] == json!("caller-literal")).count(),
            "prefixDerivedAliases": aliases.len(),
            "stated-evidence": methods.iter().filter(|m| m["promotion"] == json!("stated-evidence")).count(),
            "uncalled-no-evidence": methods.iter().filter(|m| m["promotion"] == json!("uncalled-no-evidence")).count(),
            "requiresTriageDecision": methods.iter().filter(|m| m["requiresTriageDecision"] == json!(true)).count(),
            "adr": "ADR-006: prefix twins left the method surface; see `twinAliases`"
        },
        "excluded": {
            "hostLocalOsUi": "19 client-adapter commands carry no wire name; CLI answers INTERACTIVE_UI_REQUIRED (ADR-003)",
            "prefixTwins": "109 remote_/wsl_ twins carry no method name of their own (ADR-006); they are aliases whose host must also check the alias's own capability gate"
        },
        "aliasSummary": {
            "total": aliases.len(),
            "proven": aliases.iter().filter(|a| a["paramMapping"] == json!("proven")).count(),
            "pending": aliases.iter().filter(|a| a["paramMapping"] != json!("proven")).count(),
            "rule": "proven only when every non-target argument matches a parent argument by name and type; a rename or an optionality change stays pending and the host refuses it",
            "pendingBlocksHost": true
        },
        "methods": methods,
        "twinAliases": aliases
    })
}

/// ADR-007: the v1 topic plane. Every legacy channel must be covered exactly once,
/// every `mapped` row must say where its name comes from, and every `pending` row
/// must name a task that exists in the plan — otherwise "we will decide later" is
/// indistinguishable from "we forgot", which is how the 38 channels would drift.
fn event_topics(table: &Value, map: &Value) -> Result<Vec<Value>, String> {
    let rows = table["topics"]
        .as_array()
        .ok_or("event-topics.json has no `topics` array")?;
    let plan = std::fs::read_to_string(repo_root().join("MIGRATION_PLAN.md"))
        .map_err(|e| format!("cannot read MIGRATION_PLAN.md: {e}"))?;
    let mut seen_channels: BTreeMap<&str, usize> = BTreeMap::new();
    let mut seen_topics: BTreeSet<&str> = BTreeSet::new();
    let dotted = |text: &str| {
        text.contains('.')
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_'))
    };
    for row in rows {
        let topic = row["topic"].as_str().ok_or("topic row without a topic")?;
        if !dotted(topic) {
            return Err(format!(
                "topic {topic:?} is not `<domain>.<name>` (ADR-007)"
            ));
        }
        if !seen_topics.insert(topic) {
            return Err(format!("topic {topic:?} is declared twice"));
        }
        let status = row["status"].as_str().unwrap_or_default();
        if status == "mapped" {
            let source = row["source"].as_str().unwrap_or_default();
            if !source.contains(TOPIC_SOURCE_PATTERN) {
                return Err(format!(
                    "mapped topic {topic:?} names no document line as its source (ADR-007)"
                ));
            }
        } else if let Some(task) = status.strip_prefix("pending: ") {
            if !plan.contains(&format!("### Task {task} ")) {
                return Err(format!(
                    "topic {topic:?} is pending on {task}, which is not a Task"
                ));
            }
        } else if !status.starts_with("not-forwarded: ") {
            return Err(format!(
                "topic {topic:?} status {status:?} must be mapped / pending: <Task> / not-forwarded: <reason>"
            ));
        }
        let legacy_direction = row["legacyDirection"].as_str().unwrap_or_default();
        if ![
            "core-to-client",
            "client-to-core",
            "both",
            "unproven",
            "none",
        ]
        .contains(&legacy_direction)
        {
            return Err(format!("topic {topic:?} has an unusable legacyDirection"));
        }
        // A `mapped` row is a promise the bridge can actually keep: it must state
        // which way v1 sends the frame and what happens to the payload. An unmapped
        // row is not allowed to look decided.
        let payload = row["payload"].as_str().unwrap_or_default();
        let planned = row["plannedDirection"].as_str().unwrap_or_default();
        if status == "mapped" {
            if !["core-to-client", "client-to-core", "both"].contains(&planned) {
                return Err(format!(
                    "mapped topic {topic:?} does not state a planned direction"
                ));
            }
            if !(payload.starts_with("verbatim") || payload.starts_with("reshaped:")) {
                return Err(format!(
                    "mapped topic {topic:?} must say whether the payload is verbatim or reshaped, and how"
                ));
            }
        } else if !(planned.starts_with("pending") && payload.starts_with("pending")) {
            return Err(format!(
                "topic {topic:?} is {status:?} yet already states a direction or payload shape — that decision belongs to the pending task"
            ));
        }
        // `&[]` is const-promoted, so the fallback slice is 'static and the borrow
        // of `channel` can outlive the statement.
        let channels: &[Value] = row["legacyChannels"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for channel in channels {
            let name = channel.as_str().unwrap_or_default();
            *seen_channels.entry(name).or_insert(0) += 1;
        }
    }
    let legacy: &[Value] = map["events"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let channels: Vec<&str> = legacy.iter().filter_map(|e| e["name"].as_str()).collect();
    for name in &channels {
        match seen_channels.get(name).copied().unwrap_or(0) {
            0 => return Err(format!("legacy channel {name:?} is covered by no v1 topic")),
            1 => {}
            n => return Err(format!("legacy channel {name:?} is claimed by {n} topics")),
        }
    }
    if let Some(unknown) = seen_channels.keys().find(|n| !channels.contains(n)) {
        return Err(format!(
            "topic table names an unknown legacy channel {unknown:?}"
        ));
    }
    Ok(rows.clone())
}

fn topics_schema(rows: &[Value], table: &Value) -> Value {
    let by_status = |want: &str| {
        rows.iter()
            .filter(|r| r["status"].as_str().unwrap_or_default() == want)
            .count()
    };
    let shaped = rows
        .iter()
        .filter(|r| {
            r["payload"]
                .as_str()
                .unwrap_or_default()
                .starts_with("reshaped:")
        })
        .count();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "aeroric-local-protocol-v1-event-topics",
        "protocolVersion": PROTOCOL_VERSION,
        "adr": "ADR-007: v1 owns its dotted topic names; the 38 legacy Tauri channels are bridged by this table, not republished verbatim",
        "derivation": table["derivation"].clone(),
        "topics": rows.len(),
        "legacyChannels": rows.iter().map(|r| r["legacyChannels"].as_array().map(Vec::len).unwrap_or(0)).sum::<usize>(),
        "mapped": by_status("mapped"),
        "pending": rows.iter().filter(|r| r["status"].as_str().unwrap_or_default().starts_with("pending")).count(),
        "notForwarded": by_status("not-forwarded"),
        "reshapedPayloads": shaped,
        "legacyDirectionUnproven": rows.iter().filter(|r| r["legacyDirection"] == json!("unproven")).count(),
        "contract": {
            "unknownTopic": "a client keeps the frame and must not fail the stream",
            "pending": "a pending topic may not be wired by a host until the named task resolves it",
            "renameIsProtocolChange": "topics are wire names: adding, renaming or dropping one is a protocol change; editing this table's mappings is not"
        },
        "rows": rows
    })
}

fn events_schema(events: &[Value]) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "aeroric-local-protocol-v1-events",
        "count": events.len(),
        "contract": {
            "seq": "assigned at one ordered publish point; a gap is reported, never absorbed",
            "unknownEvent": "clients keep the frame and must not fail the stream on an unrecognised topic",
            "controlQueue": "control events are never dropped to make room; a slow terminal subscriber loses its subscription first"
        },
        "events": events
    })
}

fn fixtures() -> Vec<(&'static str, Value)> {
    let epoch = "11111111-1111-4111-8111-111111111111";
    vec![
        // Task P2-03: behaviour, not a frame. Both decoders of this file (Rust core and the
        // shipping TypeScript) must produce the stored `expected` for the stored `input`.
        ("projects.json", project_parity_cases()),
        ("task-transitions.json", task_parity_cases()),
        (
            "success.json",
            json!({
                "comment": "mutating request then authoritative snapshot response: revision 7 -> 8, generation created",
                "request": { "v": 1, "id": "req-1", "method": "terminal.run_task", "idempotencyKey": "op-1",
                             "params": { "projectId": "p", "taskId": "t", "expectedRevision": "7" } },
                "response": { "v": 1, "id": "req-1", "ok": true,
                              "result": { "taskId": "t", "generation": "2", "revision": "8" } }
            }),
        ),
        (
            "error.json",
            json!({
                "comment": "revision conflict: the second writer is refused rather than overwriting; retryable is false",
                "request": { "v": 1, "id": "req-2", "method": "terminal.complete_task", "idempotencyKey": "op-2",
                             "params": { "taskId": "t", "expectedRevision": "8" } },
                "response": { "v": 1, "id": "req-2", "ok": false,
                              "error": { "code": "REVISION_CONFLICT", "message": "State changed", "retryable": false,
                                         "detail": { "expectedRevision": "8", "actualRevision": "9" } } }
            }),
        ),
        (
            "unknown.json",
            json!({
                "comment": "forward compatibility: unknown fields and an unknown wire code degrade; an unknown mutating method is refused, not executed",
                "frameWithUnknownFields": { "v": 1, "id": "req-3", "ok": true, "result": {}, "hostFeature": { "enabled": true } },
                "unknownCode": { "code": "QUOTA_EXHAUSTED", "message": "from a newer host", "retryable": true },
                "unknownEvent": { "v": 1, "event": "some.newer.topic", "epoch": epoch, "seq": "42", "data": { "kept": 1 } },
                "refusedUnknownMutating": { "method": "terminal.frobnicate_task",
                                            "error": { "code": "INVALID_ARGUMENT", "message": "unknown mutating method", "retryable": false } }
            }),
        ),
        (
            "terminal.json",
            json!({
                "comment": "raw bytes as base64 with per-stream offset/generation; truncation is explicit",
                "output": { "v": 1, "event": "terminal.output", "epoch": epoch, "seq": "19",
                            "data": { "taskId": "t", "generation": "2", "offset": "0", "nextOffset": "3", "bytesBase64": "YWJj" } },
                "gap": { "v": 1, "event": "terminal.gap", "epoch": epoch, "seq": "25",
                         "data": { "taskId": "t", "generation": "2", "reason": "SNAPSHOT_REQUIRED", "reset": true, "truncated": true } },
                "lease": { "taskId": "t", "generation": "2", "holder": "swiftui-window-3", "acquiredAt": 1_760_000_000_000_u64 },
                "base64": {
                    "comment": "one vector list, three decoders: a lenient base64 reader turns a mangled PTY chunk into bytes nobody can see, so accepting where another language refuses is a failure",
                    "valid": [
                        { "text": "", "bytes": [] },
                        { "text": "YQ==", "bytes": [97] },
                        { "text": "YWJj", "bytes": [97, 98, 99] },
                        { "text": "YWJjZGU=", "bytes": [97, 98, 99, 100, 101] },
                        { "text": "+P4=", "bytes": [248, 254] },
                        { "text": "/+A=", "bytes": [255, 224] }
                    ],
                    "invalid": ["!!!", "YWJ", "YWJj=", "a=bc", "YWJj==", "====", "YWJjZGU", "YWJjYWJ", "Y WJj", "YWJjYWJj="]
                },
                "frameVectors": {
                    "comment": "shared strictness table, next to the base64 table: both exist so a fourth reader can check that all three decoders reach the same verdict. Limits come from transport.maxFrameBytes.",
                    "rejected": {
                        "empty": "   ",
                        "noMethod": { "v": 1, "id": "x" },
                        "noVersion": { "id": "x", "ok": true },
                        "numericSeq": { "v": 1, "event": "terminal.output", "epoch": "x", "seq": 9007199254740993_i64, "data": {} },
                        "unknownStatus": { "v": 1, "id": "x", "ok": true, "result": { "status": "vibrating" } }
                    },
                    "accepted": {
                        "maxU64Seq": { "v": 1, "event": "task.updated", "epoch": "e", "seq": "18446744073709551615", "data": {} }
                    }
                }
            }),
        ),
        (
            "session.json",
            json!({
                "comment": "the owner of a session is the recorded agent, not the UI selection; unknown content stays opaque with its source id",
                "owner": { "family": "omp", "sessionId": "s1",
                           "path": "/home/dev/.aeroric/agent-homes/omp/sessions/-home-dev-app/s.jsonl",
                           "source": "native", "recordedBy": "my-codex-like-profile" },
                "handoff": { "family": "codex", "sessionId": "s2", "source": "handoff", "originSessionId": "s1" },
                "page": { "sessionId": "s1", "family": "omp", "hasMore": true, "nextCursor": "opaque-cursor-2",
                          "messages": [ { "id": "m1", "role": "assistant", "blocks": [
                              { "kind": "text", "text": "kept verbatim" },
                              { "kind": "mystery_tool", "sourceId": "b7", "payload": { "raw": 1 }, "originFamily": "claude" } ] } ] }
            }),
        ),
        (
            "legacy-task.json",
            json!({
                "comment": "a task written before sessionFamily existed: absent optionals are not null, and no transcript means no resume",
                "stored": { "id": "old-1", "projectId": "p", "status": "interrupted", "agent": { "builtIn": "codex" },
                           "createdAt": 1_700_000_000_000_u64, "sessionCodexLike": true },
                "expectations": { "canResume": false, "familyFromAgent": "codex", "unknownFieldSurvives": true },
                // ADR-028. One agent, two families: `launchFamily` decides which CLI's
                // argument shape runs it, `sessionFamily` decides which transcript format
                // and config home it reads. They are not interchangeable, and a single
                // field cannot carry both — `claude_gpt55` is the row that proves it.
                //
                // "content-derived" is a deliberate non-value: the shipping host infers
                // claude-vs-codex for that agent by reading its start script
                // (`app_settings/launch_spec.rs:400-406`), so no adapter may replace this
                // row with a constant. Consumers must treat it as "ask the host", never as
                // a family name.
                "familySplit": {
                    "comment": "Rust, Swift and TypeScript all read the same three field names from here; the claude_gpt55 row is the case a single field gets wrong (ADR-028, ADR-029)",
                    "fields": { "launch": "launchFamily", "session": "sessionFamily", "resume": "resumeFamily" },
                    "builtIn": {
                        "claude": { "launch": "claude", "session": "claude", "resume": "claude" },
                        "codex": { "launch": "codex", "session": "codex", "resume": "codex" },
                        "dsh": { "launch": "dsh", "session": "dsh", "resume": "dsh" },
                        "omp": { "launch": "omp", "session": "omp", "resume": "omp" },
                        "claude_gpt55": { "launch": "content-derived", "session": "claude", "resume": "codex" }
                    },
                    "resumeIsNotSession": "claude_gpt55 reads Claude transcripts but its CLI resumes codex ones, so a `claude` task switching to it is a handoff today and must stay one",
                    "missingFieldRule": "derive-from-host then backfill-on-write; never a batch rewrite of stored tasks"
                },
                "resumeRule": {
                    "comment": "mirrors canResumeTask in src/components/task-panel/TaskList.tsx; Rust and TypeScript both assert their predicate against this table",
                    "withTranscript": {
                        "todo": false, "pending": false, "running": false, "input_required": false,
                        "detached": false, "done": true, "failed": true, "cancelled": true, "interrupted": true
                    },
                    "withoutTranscript": { "done": false, "failed": false, "cancelled": false, "interrupted": false },
                    "discardedWorktreeWithTranscript": false
                }
            }),
        ),
    ]
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let Some(index) = args.iter().position(|a| a == "--out") else {
        eprintln!(
            "usage: export-schema --out ROOT  (writes ROOT/contracts/native/v1/**; a repository root, so the tree layout is not duplicated here)\n\
             refusing to write: use `node scripts/native/contracts.mjs --write` to update the committed contract"
        );
        std::process::exit(78);
    };
    let Some(dir) = args.get(index + 1) else {
        eprintln!("export-schema: --out needs a directory");
        std::process::exit(78);
    };
    let base = PathBuf::from(dir);

    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("docs/native/command-map.json"))
        .map_err(|e| format!("cannot read docs/native/command-map.json: {e}"))?;
    let map: Value =
        serde_json::from_str(&raw).map_err(|e| format!("command-map.json is unparseable: {e}"))?;
    let topics_raw = std::fs::read_to_string(root.join("docs/native/event-topics.json"))
        .map_err(|e| format!("cannot read docs/native/event-topics.json (ADR-007): {e}"))?;
    let topic_table: Value =
        serde_json::from_str(&topics_raw).map_err(|e| format!("event-topics.json: {e}"))?;
    let topics = event_topics(&topic_table, &map)?;
    if topic_table["count"].as_u64() != Some(topics.len() as u64) {
        return Err(format!(
            "event-topics.json says count {} but holds {} rows",
            topic_table["count"],
            topics.len()
        ));
    }

    let (methods, aliases) = methods_from_inventory(&map)?;
    let events = events_from_inventory(&map);
    let mut documents: Vec<(String, Value)> = vec![
        ("v1/envelope.schema.json".into(), envelope_schema()),
        (
            "v1/methods.schema.json".into(),
            methods_schema(&methods, &aliases),
        ),
        ("v1/events.schema.json".into(), events_schema(&events)),
        (
            "v1/event-topics.schema.json".into(),
            topics_schema(&topics, &topic_table),
        ),
    ];
    for (name, value) in fixtures() {
        documents.push((format!("v1/fixtures/{name}"), value));
    }
    documents.sort_by(|a, b| a.0.cmp(&b.0));
    fixtures_are_sourced(&documents, &methods, &topics)?;

    for (name, value) in &documents {
        let path = base.join("contracts/native").join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{parent:?}: {e}"))?;
        }
        let text = serde_json::to_string_pretty(value).map_err(|e| format!("{name}: {e}"))? + "\n";
        std::fs::write(&path, text).map_err(|e| format!("{path:?}: {e}"))?;
    }
    println!(
        "export-schema: wrote {} documents to {} — {} methods + {} aliases ({} mutating, {} capability-gated), {} legacy channels, {} v1 topics",
        documents.len(),
        base.display(),
        methods.len(),
        methods.iter().filter(|m| m["mutating"] == json!(true)).count(),
        methods.iter().filter(|m| m["requiresCapability"] != Value::Null).count(),
        aliases.len(),
        events.len(),
        topics.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subsystem_prefix_does_not_hide_the_action() {
        assert!(!is_mutating("remote_git_list_branches"));
        assert!(!is_mutating("wsl_read_file_content"));
        assert!(is_mutating("remote_git_stash_apply"));
        assert!(is_mutating("notebook_export_site_write"));
    }

    #[test]
    fn an_unknown_verb_defaults_to_mutating() {
        assert!(is_mutating("task_frobnicate"));
        assert!(leading_action("task_frobnicate").is_none());
    }

    /// The three Redis key-type names are the reason the override exists; if the
    /// rule ever stops calling them reads the override has to go, not linger.
    #[test]
    fn a_read_verb_that_is_a_noun_still_mutates() {
        for name in READ_VERB_IS_A_NOUN {
            assert_eq!(leading_action(name), Some(false), "{name}");
            assert!(is_mutating(name), "{name} must be gated by the override");
        }
    }

    #[test]
    fn executing_sql_is_never_a_read() {
        assert!(is_mutating("dbx_execute_query"));
        assert!(is_mutating("db_execute_sql"));
    }
}
