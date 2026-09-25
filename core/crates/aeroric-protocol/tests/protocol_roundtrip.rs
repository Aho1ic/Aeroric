//! The Rust half of the three-language round trip (P2-01): every case here reads a
//! committed `contracts/native/v1/` document, so Rust, Swift and TypeScript decode
//! *the same bytes* rather than three hand-written near-misses.
//!
//! A missing artifact must fail, never skip: a skipped golden test is how a frozen
//! contract ends up untested.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aeroric_protocol::{
    built_in_agents, decode_line, detail_allowed_keys, error_codes, handshake_allows_peer,
    permission_modes, protocol_families, task_intent_ops, task_intent_ops_of, task_intents,
    task_statuses, AgentId, BytesBase64, ContentBlock, ErrorCode, Frame, FrameError, GapReason,
    PermissionMode, ProtocolFamily, Quantity, Request, RpcError, SessionOwner, SessionPage,
    SessionSource, Success, TaskSnapshot, TaskStatus, TerminalGap, TerminalLease, TerminalOutput,
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};

fn contract(relative: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("contracts/native/v1")
        .join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} — regenerate with `pnpm native:contracts --write` before running this test",
            path.display()
        )
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

fn fixture(name: &str) -> Value {
    contract(&format!("fixtures/{name}"))
}

fn decode(value: &Value) -> Frame {
    let text = serde_json::to_vec(value).unwrap();
    decode_line(&text)
        .unwrap_or_else(|e| panic!("{}: {e}", serde_json::to_string_pretty(value).unwrap()))
}

fn list(value: &Value, field: &str) -> Vec<Value> {
    value[field]
        .as_array()
        .unwrap_or_else(|| panic!("`{field}` is not an array"))
        .clone()
}

fn bytes_of(vector: &Value) -> Vec<u8> {
    vector["bytes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b.as_u64().unwrap() as u8)
        .collect()
}

#[test]
fn success_and_failure_frames_decode_into_the_typed_envelope() {
    let case = fixture("success.json");
    let request: Request = serde_json::from_value(case["request"].clone()).unwrap();
    assert_eq!(
        request.method, "terminal.run_task",
        "a fixture must name a method the published surface actually has"
    );
    assert_eq!(
        request.idempotency_key.as_deref(),
        Some("op-1"),
        "`idempotencyKey` must bind to the typed field, not to `extra`"
    );
    assert_eq!(
        request.params["expectedRevision"],
        json!("7"),
        "a revision must arrive as a decimal string, never a JSON number"
    );

    let frame = decode(&case["response"]);
    let Frame::Success(Success { result, .. }) = frame else {
        panic!("expected a success frame, got {frame:?}");
    };
    assert_eq!(result["revision"], json!("8"));
    assert_eq!(result["generation"], json!("2"));

    let case = fixture("error.json");
    let frame = decode(&case["response"]);
    let Frame::Failure(failure) = frame else {
        panic!("expected a failure frame, got {frame:?}");
    };
    assert_eq!(failure.error.code, ErrorCode::RevisionConflict);
    assert!(
        !failure.error.retryable,
        "a conflict must not be retried blindly"
    );
    assert_eq!(
        failure
            .error
            .detail
            .expect("a conflict names both revisions")["actualRevision"],
        json!("9")
    );
}

#[test]
fn unknown_fields_codes_events_and_topics_survive() {
    let case = fixture("unknown.json");

    let frame = decode(&case["frameWithUnknownFields"]);
    let Frame::Success(success) = frame else {
        panic!("expected success, got {frame:?}");
    };
    assert_eq!(
        success.extra["hostFeature"],
        json!({ "enabled": true }),
        "a newer host's addition must be carried, not dropped"
    );

    let unknown: RpcError = serde_json::from_value(case["unknownCode"].clone()).unwrap();
    assert_eq!(
        unknown.code,
        ErrorCode::Other("QUOTA_EXHAUSTED".to_string())
    );
    assert_eq!(unknown.code.as_str(), "QUOTA_EXHAUSTED");
    assert_eq!(unknown.message, "from a newer host");

    let frame = decode(&case["unknownEvent"]);
    let Frame::Event(event) = frame else {
        panic!("expected an event frame, got {frame:?}");
    };
    assert_eq!(event.event, "some.newer.topic");
    assert_eq!(event.seq.as_str(), "42");

    let refused: RpcError =
        serde_json::from_value(case["refusedUnknownMutating"]["error"].clone()).unwrap();
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        case["refusedUnknownMutating"]["method"],
        json!("terminal.frobnicate_task"),
        "the fixture must keep naming a method no host implements"
    );
}

#[test]
fn the_terminal_plane_decodes_bytes_gaps_and_leases() {
    let case = fixture("terminal.json");

    let Frame::Event(output) = decode(&case["output"]) else {
        panic!("terminal.output must be an event frame");
    };
    let stream: TerminalOutput = serde_json::from_value(output.data).unwrap();
    assert_eq!(stream.identity.task_id, "t");
    assert_eq!(stream.identity.generation.as_u64(), 2);
    assert_eq!(stream.bytes_base64.unwrap().decode().unwrap(), b"abc");

    let Frame::Event(gap) = decode(&case["gap"]) else {
        panic!("terminal.gap must be an event frame");
    };
    let gap: TerminalGap = serde_json::from_value(gap.data).unwrap();
    assert_eq!(gap.reason, GapReason::SnapshotRequired);
    assert!(gap.reset, "a compaction must tell the renderer to reset");
    assert_eq!(gap.truncated, Some(true));

    let lease: TerminalLease = serde_json::from_value(case["lease"].clone()).unwrap();
    assert_eq!(lease.holder, "swiftui-window-3");

    for vector in list(&case["base64"], "valid") {
        let text = vector["text"].as_str().unwrap();
        let decoded = serde_json::from_value::<BytesBase64>(json!(text))
            .unwrap_or_else(|e| panic!("{text:?} must decode: {e}"))
            .decode()
            .unwrap();
        assert_eq!(decoded, bytes_of(&vector), "{text:?} decoded wrongly");
    }
    for text in list(&case["base64"], "invalid") {
        let text = text.as_str().unwrap();
        assert!(
            serde_json::from_value::<BytesBase64>(json!(text)).is_err(),
            "{text:?} must not decode: a mangled PTY chunk is worse than a dropped frame"
        );
    }
    let mangled = json!({
        "taskId": "t", "generation": "2", "offset": "0", "nextOffset": "4",
        "bytesBase64": case["base64"]["invalid"][0]
    });
    assert!(
        serde_json::from_value::<TerminalOutput>(mangled).is_err(),
        "the same rule must hold inside a frame"
    );
}

#[test]
fn session_ownership_and_opaque_content_survive_the_wire() {
    let case = fixture("session.json");
    let owner: SessionOwner = serde_json::from_value(case["owner"].clone()).unwrap();
    assert_eq!(owner.family, ProtocolFamily::Omp);
    assert_eq!(owner.source, SessionSource::Native);
    assert_eq!(owner.recorded_by.as_deref(), Some("my-codex-like-profile"));

    let handoff: SessionOwner = serde_json::from_value(case["handoff"].clone()).unwrap();
    assert_eq!(handoff.source, SessionSource::Handoff);
    assert_eq!(handoff.origin_session_id.as_deref(), Some("s1"));
    assert_eq!(handoff.path, None, "an absent path stays absent, not null");

    let page: SessionPage = serde_json::from_value(case["page"].clone()).unwrap();
    assert_eq!(page.next_cursor.as_deref(), Some("opaque-cursor-2"));
    assert!(page.has_more);
    let blocks = &page.messages[0].blocks;
    assert_eq!(blocks.len(), 2);
    assert!(
        matches!(&blocks[0], ContentBlock::Text { text, .. } if text == "kept verbatim"),
        "{:?}",
        blocks[0]
    );
    match &blocks[1] {
        ContentBlock::Unknown(opaque) => {
            assert_eq!(opaque.kind, "mystery_tool");
            assert_eq!(opaque.source_id.as_deref(), Some("b7"));
            assert_eq!(opaque.origin_family, Some(ProtocolFamily::Claude));
        }
        other => panic!("an unrecognised block must stay opaque, got {other:?}"),
    }
}

#[test]
fn a_legacy_task_reads_without_a_migration() {
    let case = fixture("legacy-task.json");
    let stored = case["stored"].clone();
    let task: TaskSnapshot = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(task.status, TaskStatus::Interrupted);
    assert_eq!(task.agent.declared_family(), Some(ProtocolFamily::Codex));
    assert!(!task.can_resume(), "no transcript means nothing to resume");
    assert!(task.session_owner.is_none());
    assert!(
        task.unknown_fields.contains_key("sessionCodexLike"),
        "{:?}",
        task.unknown_fields
    );
    assert_eq!(
        case["expectations"]["canResume"],
        json!(task.can_resume()),
        "the published expectation must be the verdict this build reaches"
    );

    let rewritten = serde_json::to_value(&task).unwrap();
    assert!(
        rewritten.get("sessionOwner").is_none(),
        "an absent optional must not be written back as null: {rewritten}"
    );
    assert_eq!(rewritten["createdAt"], json!(1_700_000_000_000_u64));
    assert_eq!(rewritten["sessionCodexLike"], json!(true));
    assert_eq!(rewritten["agent"], json!({ "builtIn": "codex" }));
}

/// ADR-028: an agent has two families, so there are two fields, and the launch half of one
/// built-in is deliberately *not* a family name. If anyone teaches `ProtocolFamily` to
/// parse `"content-derived"`, the split silently collapses back into the single field that
/// created the problem — which is why this is asserted rather than commented.
#[test]
fn the_family_split_names_three_fields_and_refuses_a_fake_family() {
    let split = fixture("legacy-task.json")["familySplit"].clone();
    assert_eq!(split["fields"]["launch"], json!("launchFamily"));
    assert_eq!(split["fields"]["session"], json!("sessionFamily"));
    assert_eq!(split["fields"]["resume"], json!("resumeFamily"));
    let names: Vec<&str> = split["fields"]
        .as_object()
        .expect("fields is a table")
        .values()
        .filter_map(|value| value.as_str())
        .collect();
    let distinct: std::collections::HashSet<&str> = names.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        names.len(),
        "one field carrying two of these answers is exactly what ADR-028/029 prevent"
    );

    let built_in = split["builtIn"]
        .as_object()
        .expect("builtIn must be a table of agents");
    assert_eq!(
        built_in.len(),
        5,
        "the four ordinary built-ins plus claude_gpt55"
    );
    for agent in ["claude", "codex", "dsh", "omp"] {
        assert_eq!(built_in[agent]["launch"], json!(agent), "{agent} launch");
        assert_eq!(built_in[agent]["session"], json!(agent), "{agent} session");
    }

    let gpt55 = &built_in["claude_gpt55"];
    assert_eq!(
        gpt55["session"],
        json!("claude"),
        "its transcript is Claude-format and lives under ~/.claude"
    );
    assert_eq!(
        gpt55["resume"],
        json!("codex"),
        "…but its --resume reads codex transcripts, which is why the switch is a handoff"
    );
    assert_ne!(
        gpt55["session"], gpt55["resume"],
        "ADR-029: session and resume disagree for this one agent"
    );
    assert_eq!(
        gpt55["launch"],
        json!("content-derived"),
        "how it is started is read from the user's script; a constant here would override them"
    );
    serde_json::from_value::<ProtocolFamily>(gpt55["launch"].clone())
        .expect_err("the placeholder must never parse as a protocol family");
    assert_eq!(
        serde_json::from_value::<ProtocolFamily>(gpt55["session"].clone()).ok(),
        Some(ProtocolFamily::Claude)
    );

    assert_eq!(
        split["missingFieldRule"],
        json!("derive-from-host then backfill-on-write; never a batch rewrite of stored tasks"),
        "the migration rule is part of the contract, not a private choice in one language"
    );
}

/// The resume table is the shared authority for a rule that exists twice — once
/// here, once in TypeScript — on top of a list the shipping client already builds.
#[test]
fn the_resume_rule_matches_the_published_table() {
    let rule = fixture("legacy-task.json")["resumeRule"].clone();
    let record = |status: &str, transcript: bool, discarded: bool| -> TaskSnapshot {
        let mut value = json!({
            "id": "t", "projectId": "p", "status": status, "agent": { "builtIn": "codex" }
        });
        if transcript {
            value["sessionOwner"] = json!({ "family": "codex", "sessionId": "s1" });
        }
        if discarded {
            value["worktreeDiscarded"] = json!(true);
        }
        serde_json::from_value(value).unwrap()
    };

    let mut checked = 0usize;
    for (transcript, key) in [(true, "withTranscript"), (false, "withoutTranscript")] {
        let table = rule[key].as_object().expect("a status table");
        for (status, expected) in table {
            assert_eq!(
                record(status, transcript, false).can_resume(),
                expected.as_bool().unwrap(),
                "{status} with transcript={transcript}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 13, "only {checked} verdicts were checked");
    assert_eq!(
        record("done", true, true).can_resume(),
        rule["discardedWorktreeWithTranscript"]
            .as_bool()
            .expect("a verdict")
    );
    assert_eq!(
        task_statuses().len(),
        rule["withTranscript"].as_object().expect("table").len(),
        "the table must cover every published status, not the ones remembered"
    );
}

#[test]
fn malformed_frames_are_refused_with_the_right_error() {
    let vectors = fixture("terminal.json")["frameVectors"].clone();
    let rejected = vectors["rejected"].as_object().unwrap();

    let empty = rejected["empty"].as_str().unwrap();
    assert_eq!(
        decode_line(empty.as_bytes()).unwrap_err(),
        FrameError::Empty,
        "whitespace is not a frame"
    );
    for key in ["noMethod", "noVersion", "numericSeq"] {
        let line = serde_json::to_vec(&rejected[key]).unwrap();
        let error = decode_line(&line).unwrap_err();
        assert!(matches!(error, FrameError::Malformed(_)), "{key}: {error}");
    }
    // A frame that parses but carries a status this vocabulary has never heard of:
    // the envelope must not launder that into a default either.
    let Frame::Success(success) = decode(&rejected["unknownStatus"]) else {
        panic!("expected success")
    };
    assert_eq!(success.result["status"], json!("vibrating"));
    let nested = json!({
        "id": "t", "projectId": "p", "status": success.result["status"],
        "agent": { "builtIn": "codex" }
    });
    assert!(
        serde_json::from_value::<TaskSnapshot>(nested).is_err(),
        "an unrecognised status must be refused, never defaulted"
    );

    // One byte past the limit, read from the artifact rather than the constant, so
    // a schema that disagrees with the decoder is caught here.
    let limit = contract("envelope.schema.json")["transport"]["maxFrameBytes"]
        .as_u64()
        .expect("a published frame limit") as usize;
    assert_eq!(limit, aeroric_protocol::MAX_FRAME_BYTES);
    let padding = "a".repeat(limit);
    let error = decode_line(format!("{{\"x\":\"{padding}\"}}").as_bytes()).unwrap_err();
    assert!(matches!(error, FrameError::TooLarge { .. }), "{error}");

    let error = decode_line(&[0xff_u8, 0xfe, 0x00, 0x01]).unwrap_err();
    assert_eq!(error, FrameError::NotUtf8);
}

#[test]
fn a_64bit_cursor_survives_the_round_trip_exactly() {
    let accepted = fixture("terminal.json")["frameVectors"]["accepted"]["maxU64Seq"].clone();
    let expected = accepted["seq"].as_str().unwrap().to_string();
    assert_eq!(expected, u64::MAX.to_string());
    let Frame::Event(event) = decode(&accepted) else {
        panic!("expected an event frame")
    };
    assert_eq!(event.seq.as_str(), expected);
    assert_eq!(event.seq, Quantity::from_u64(u64::MAX));
    let encoded = aeroric_protocol::encode_line(&Frame::Event(event)).unwrap();
    assert!(
        encoded.contains(&format!("\"seq\":\"{expected}\"")),
        "a cursor that loses precision is silent data loss: {encoded}"
    );
}

#[test]
fn the_committed_schema_agrees_with_the_rust_vocabularies() {
    let constants = contract("envelope.schema.json")["constants"].clone();
    assert_eq!(constants["protocolVersion"], json!(1));
    assert_eq!(constants["taskStatuses"], json!(task_statuses()));
    assert_eq!(constants["protocolFamilies"], json!(protocol_families()));
    assert_eq!(constants["permissionModes"], json!(permission_modes()));
    assert_eq!(constants["builtInAgents"], json!(built_in_agents()));
    assert_eq!(constants["errorCodes"], json!(error_codes()));
    assert_eq!(
        constants["errorDetailAllowedKeys"],
        json!(detail_allowed_keys())
    );
    // ADR-037 词汇表进契约（含 Q72 `EditTodo`）：ops/10/4 三份名单与 protocol 的函数逐条同拼法。
    assert_eq!(constants["taskIntents"]["ops"], json!(task_intent_ops()));
    assert_eq!(
        constants["taskIntents"]["metadata"],
        json!(task_intent_ops_of("metadata"))
    );
    assert_eq!(
        constants["taskIntents"]["state"],
        json!(task_intent_ops_of("state"))
    );
    assert_eq!(
        task_intents()
            .iter()
            .filter(|(_, kind)| *kind == "metadata")
            .count(),
        10
    );
    assert_eq!(
        task_intents()
            .iter()
            .filter(|(_, kind)| *kind == "state")
            .count(),
        4
    );
    assert!(
        !constants["errorDetailAllowedKeys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| ["token", "password", "connectionString", "apiKey"]
                .contains(&k.as_str().unwrap())),
        "a secret-shaped key must never be on the allowlist"
    );
    let policy = contract("envelope.schema.json")["versionPolicy"].clone();
    assert_eq!(policy["acceptsSameMajor"], json!(handshake_allows_peer(1)));
    assert_eq!(policy["rejectsAnyOtherVersion"], json!(true));
    assert!(handshake_allows_peer(PROTOCOL_VERSION));
    for other in [0u8, 2, 255] {
        assert!(!handshake_allows_peer(other), "v{other} must be refused");
    }
    assert_eq!(
        constants["limits"]["replay"]["maxEvents"],
        json!(4096),
        "the replay window is a protocol promise, so it belongs in the artifact"
    );

    let methods = contract("methods.schema.json");
    assert_eq!(
        methods["count"],
        json!(533),
        "ADR-006: the surface is 642 minus 109 twins"
    );
    assert_eq!(
        methods["count"].as_u64().unwrap(),
        methods["methods"].as_array().unwrap().len() as u64,
        "`count` must equal the array it advertises"
    );
    assert_eq!(
        methods["readCount"].as_u64().unwrap() + methods["mutatingCount"].as_u64().unwrap(),
        methods["count"].as_u64().unwrap(),
        "every method must be classified exactly once"
    );
    let names: Vec<&str> = methods["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "two methods share a wire name");
    assert!(
        !names.iter().any(|n| n.contains('/')),
        "ADR-002 forbids a slash in a wire name"
    );
    assert_eq!(methods["capabilityGatedCount"], json!(21));

    // G16 provenance: the surface must state how each name was earned, and the
    // numbers must add up to the surface they describe.
    let provenance = &methods["surfaceProvenance"];
    let groups = ["caller-literal", "stated-evidence", "uncalled-no-evidence"];
    let total: usize = groups
        .iter()
        .map(|group| provenance[*group].as_u64().unwrap() as usize)
        .sum();
    assert_eq!(
        total,
        methods["count"].as_u64().unwrap() as usize,
        "the provenance groups must partition the published surface"
    );
    assert_eq!(provenance["stated-evidence"], json!(83));
    assert_eq!(
        provenance["uncalled-no-evidence"],
        json!(0),
        "Q10 closed this group: a name with neither a caller nor a stated reason may not ride in"
    );
    assert_eq!(provenance["requiresTriageDecision"], json!(59));
    assert_eq!(provenance["prefixDerivedAliases"], json!(109));
    let flagged: Vec<&Value> = methods["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["requiresTriageDecision"] == json!(true))
        .collect();
    assert_eq!(flagged.len(), 59);
    assert!(
        flagged
            .iter()
            .all(|m| m["promotion"] != json!("caller-literal")),
        "a name with a literal caller is not owed a decision"
    );
    // ADR-006: the twins left the method surface. An alias must point at a published
    // method, and its mapping may only read `proven` when nothing about the
    // arguments had to be guessed — otherwise the host has to refuse it, because a
    // silently renamed or newly-optional argument is a payload the parent command
    // never accepted.
    let aliases = methods["twinAliases"].as_array().unwrap();
    assert_eq!(aliases.len(), 109, "provenance says 109 aliases");
    for alias in aliases {
        let legacy = alias["legacyCommand"].as_str().unwrap();
        let target = alias["method"].as_str().unwrap();
        assert!(
            names.contains(&target),
            "{legacy} aliases {target}, which is not published"
        );
        let proven = alias["paramMapping"] == json!("proven");
        let empty_diff = alias["paramDiff"].as_array().is_some_and(Vec::is_empty);
        assert_eq!(
            proven, empty_diff,
            "{legacy}: mapping status and diff disagree"
        );
        assert!(
            ["ssh", "wsl"].contains(&alias["targetKind"].as_str().unwrap_or("")),
            "{legacy} has no target kind"
        );
    }
    let summary = &methods["aliasSummary"];
    assert_eq!(summary["total"], json!(109));
    assert_eq!(summary["proven"], json!(77));
    assert_eq!(summary["pending"], json!(32));
    assert_eq!(
        summary["pendingBlocksHost"],
        json!(true),
        "a pending alias must not be accepted by a host"
    );

    // ADR-007: the v1 topic plane must cover the legacy channels exactly once, and a
    // row may not look "decided" before the task that owns it has decided.
    let topics = contract("event-topics.schema.json");
    assert_eq!(
        topics["topics"],
        json!(39),
        "38 channels plus the synthesised terminal.gap"
    );
    assert_eq!(topics["legacyChannels"], json!(38));
    assert_eq!(topics["mapped"], json!(3));
    assert_eq!(topics["pending"], json!(36));
    assert_eq!(topics["reshapedPayloads"], json!(3));
    assert_eq!(topics["legacyDirectionUnproven"], json!(11));
    let rows = topics["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 39);
    let mut claimed: BTreeMap<&str, usize> = BTreeMap::new();
    for row in rows {
        let topic = row["topic"].as_str().unwrap();
        assert!(topic.contains('.'), "{topic} is not dotted");
        for channel in row["legacyChannels"].as_array().unwrap() {
            *claimed.entry(channel.as_str().unwrap()).or_insert(0) += 1;
        }
        if row["status"] == json!("mapped") {
            assert!(
                row["source"].as_str().is_some_and(|s| s.contains('§')),
                "{topic} is mapped without naming a document line"
            );
            assert!(
                row["payload"]
                    .as_str()
                    .is_some_and(|p| p.starts_with("reshaped:") || p.starts_with("verbatim")),
                "{topic} is mapped without a payload statement"
            );
        } else {
            assert!(
                row["plannedDirection"]
                    .as_str()
                    .unwrap()
                    .starts_with("pending"),
                "{topic} is not mapped yet states a direction"
            );
        }
    }
    let legacy = contract("events.schema.json");
    for event in legacy["events"].as_array().unwrap() {
        let name = event["name"].as_str().unwrap();
        assert_eq!(
            claimed.get(name).copied().unwrap_or(0),
            1,
            "legacy channel {name:?} must be claimed by exactly one v1 topic"
        );
    }
    assert_eq!(
        claimed.len(),
        38,
        "the table claims a channel that does not exist"
    );

    let events = contract("events.schema.json");
    assert_eq!(events["count"], json!(38));
    assert_eq!(
        events["count"].as_u64().unwrap(),
        events["events"].as_array().unwrap().len() as u64
    );
}

/// Drift in the other direction: a name the schema publishes must still be one the
/// DTOs accept, or a client is told to call something this build cannot parse.
#[test]
fn published_names_are_the_ones_the_dto_accepts() {
    for status in task_statuses() {
        let value: TaskStatus = serde_json::from_value(json!(status))
            .unwrap_or_else(|e| panic!("schema publishes {status:?} but the DTO rejects it: {e}"));
        assert_eq!(serde_json::to_value(value).unwrap(), json!(status));
    }
    for mode in permission_modes() {
        let value: PermissionMode = serde_json::from_value(json!(mode)).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), json!(mode));
    }
    for agent in built_in_agents() {
        let value: AgentId = serde_json::from_value(json!({ "builtIn": agent })).unwrap();
        assert!(value.declared_family().is_some(), "{agent} has no family");
    }
    for family in protocol_families() {
        let _: ProtocolFamily = serde_json::from_value(json!(family)).unwrap();
    }
    for code in error_codes() {
        let parsed: RpcError = serde_json::from_value(json!({
            "code": code, "message": "x", "retryable": false
        }))
        .unwrap();
        assert!(
            !matches!(parsed.code, ErrorCode::Other(_)),
            "{code} is published but only decodes as the fallback"
        );
        assert_eq!(
            parsed.code.as_str(),
            code,
            "the text must survive a round trip"
        );
    }
}
