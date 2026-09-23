//! Which protocol family a task's session actually belongs to, and what may continue it.
//!
//! The record carries up to four families' session fields at once — a task switched from
//! Claude to Codex and back keeps both ids until something cleans them up — plus a legacy
//! `sessionCodexLike` boolean from before `sessionFamily` existed. Picking the wrong
//! family is not a display bug: it sends `--resume` to the wrong CLI against the wrong
//! transcript.
//!
//! The precedence below is `src/taskSession.ts` verbatim. It is *ordered*, and the order
//! is the rule: a stored `sessionFamily` is a decision somebody already made and must
//! win, while inferring from which fields happen to be populated is the last resort.

use aeroric_protocol::task::ProtocolFamily;
use serde_json::Value;

use crate::storage::compat::Record;

/// The table `src/agents.ts` builds from built-ins plus the user's custom profiles.
/// Task P3-02 owns that table; this module only consumes it, because guessing a family
/// from an agent's *name* is exactly the bug `AgentId::Custom` was written to avoid.
/// The table is shared across the host's threads, so it is `Send + Sync` by definition:
/// a service that could not hand its own family table to a worker could not start a task
/// from anywhere but the caller's thread.
pub trait AgentFamilies: Send + Sync {
    fn family(&self, agent: &str) -> ProtocolFamily;
    /// Built-in agents of the same family may continue each other's sessions ("claude"
    /// resuming a "claude_gpt55" task). Two custom profiles may not, even when they
    /// share a family: they read different homes and different config.
    fn is_builtin(&self, agent: &str) -> bool;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionOwner {
    /// Whose CLI will be run: `sessionAgent` when the record names one, `agent` otherwise.
    pub agent: String,
    pub family: ProtocolFamily,
    /// Kept because the wire and the older records both speak it. Derived, never stored
    /// independently of `family`.
    pub codex_like: bool,
}

fn text<'a>(record: &'a Record, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

fn has_session(record: &Record, prefix: &str) -> bool {
    let id = record.get(&format!("{prefix}SessionId"));
    let path = record.get(&format!("{prefix}SessionPath"));
    id.is_some_and(|value| !value.is_null() && value != &Value::String(String::new()))
        || path.is_some_and(|value| !value.is_null() && value != &Value::String(String::new()))
}

pub fn has_task_session(record: &Record) -> bool {
    ["claude", "codex", "dsh", "omp"]
        .iter()
        .any(|prefix| has_session(record, prefix))
}

/// `hasTaskSessionPath`: a path but no id is enough to *read* a transcript from, which is
/// what the session-id recovery step needs.
pub fn has_task_session_path(record: &Record) -> bool {
    ["claude", "codex", "dsh", "omp"]
        .iter()
        .any(|prefix| record.get(&format!("{prefix}SessionPath")).is_some())
}

fn owner(agent: String, family: ProtocolFamily) -> SessionOwner {
    SessionOwner {
        agent,
        family,
        codex_like: family == ProtocolFamily::Codex,
    }
}

/// `resolveTaskSessionOwner`.
pub fn resolve_session_owner(record: &Record, families: &dyn AgentFamilies) -> SessionOwner {
    let agent = text(record, "sessionAgent")
        .or_else(|| text(record, "agent"))
        .unwrap_or_default()
        .to_string();

    if let Some(family) = record
        .get("sessionFamily")
        .and_then(Value::as_str)
        .and_then(|name| serde_json::from_value(Value::String(name.to_string())).ok())
    {
        return owner(agent, family);
    }

    if let Some(codex_like) = record.get("sessionCodexLike").and_then(Value::as_bool) {
        // An older record: derive the family from the boolean, except that a dsh session
        // with no claude session present means the boolean predates dsh existing at all.
        if !codex_like && has_session(record, "dsh") && !has_session(record, "claude") {
            return owner(agent, ProtocolFamily::Dsh);
        }
        return owner(
            agent,
            if codex_like {
                ProtocolFamily::Codex
            } else {
                ProtocolFamily::Claude
            },
        );
    }

    let present: Vec<ProtocolFamily> = [
        (ProtocolFamily::Codex, "codex"),
        (ProtocolFamily::Claude, "claude"),
        (ProtocolFamily::Dsh, "dsh"),
        (ProtocolFamily::Omp, "omp"),
    ]
    .into_iter()
    .filter(|(_, prefix)| has_session(record, prefix))
    .map(|(family, _)| family)
    .collect();
    if present.len() == 1 {
        return owner(agent.clone(), present[0]);
    }
    // Zero families, or an ambiguous mix: the configured agent decides.
    let family = families.family(&agent);
    owner(agent, family)
}

/// One family's session coordinates, plus whichever other family still holds a value —
/// the legacy fallback `getTaskSessionFieldsByFamily` offers to callers that must not
/// lose an id just because the record's family attribution moved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionFields {
    pub session_id: Option<String>,
    pub session_path: Option<String>,
    pub legacy_session_id: Option<String>,
    pub legacy_session_path: Option<String>,
}

fn string_field(record: &Record, key: &str) -> Option<String> {
    record
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
}

pub fn session_fields(record: &Record, family: ProtocolFamily) -> SessionFields {
    let prefix = family_prefix(family);
    let current = (
        string_field(record, &format!("{prefix}SessionId")),
        string_field(record, &format!("{prefix}SessionPath")),
    );
    let legacy = ProtocolFamily::ALL
        .iter()
        .map(|other| family_prefix(*other))
        .filter(|other| *other != prefix)
        .map(|other| {
            (
                string_field(record, &format!("{other}SessionId")),
                string_field(record, &format!("{other}SessionPath")),
            )
        })
        // `fields.sessionId || fields.sessionPath`: an empty string is falsy to the
        // client, so a record carrying `""` offers no legacy fallback. Measured, not
        // assumed — the parity table's `fields:emptyStrings:*` cases are what caught it.
        .find(|(id, path)| {
            id.as_deref().is_some_and(|value| !value.is_empty())
                || path.as_deref().is_some_and(|value| !value.is_empty())
        });
    SessionFields {
        session_id: current.0,
        session_path: current.1,
        legacy_session_id: legacy.as_ref().and_then(|l| l.0.clone()),
        legacy_session_path: legacy.as_ref().and_then(|l| l.1.clone()),
    }
}

pub fn family_prefix(family: ProtocolFamily) -> &'static str {
    match family {
        ProtocolFamily::Claude => "claude",
        ProtocolFamily::Codex => "codex",
        ProtocolFamily::Dsh => "dsh",
        ProtocolFamily::Omp => "omp",
    }
}

/// `canNativeResumeWithAgent`: the CLI's own `--resume` may be used.
pub fn can_native_resume(
    record: &Record,
    target_agent: &str,
    families: &dyn AgentFamilies,
) -> bool {
    let source = resolve_session_owner(record, families);
    // dsh has no native CLI resume at all — it reconnects over its own Web RPC channel.
    if source.family == ProtocolFamily::Dsh {
        return false;
    }
    if source.family != families.family(target_agent) {
        return false;
    }
    if families.is_builtin(&source.agent) && families.is_builtin(target_agent) {
        return true;
    }
    source.agent == target_agent
}

/// `canAdoptSessionForAgent`: same family, different home, so the transcript has to be
/// copied into the target's home before `--resume` can see it.
pub fn can_adopt(record: &Record, target_agent: &str, families: &dyn AgentFamilies) -> bool {
    let source = resolve_session_owner(record, families);
    // Adoption is a claude/codex home migration. omp's `--resume` takes a file path and
    // needs none; dsh cannot resume natively either way.
    if source.family == ProtocolFamily::Dsh || source.family == ProtocolFamily::Omp {
        return false;
    }
    if source.family != families.family(target_agent) {
        return false;
    }
    source.agent != target_agent && !can_native_resume(record, target_agent, families)
}

/// How a task continues across a manual configuration switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigSwitchStrategy {
    /// The target CLI can resume this session where it stands.
    Resume,
    /// Same family, different home: move the transcript first, then resume.
    Adopt,
    /// Neither: the conversation goes over as text, and the new agent starts fresh.
    Handoff,
}

/// `resolveConfigSwitchSessionStrategy`. `native_resume_supported` is a *runtime* fact
/// read from the session file — a transcript written by another build may not replay —
/// so it is an argument here rather than something this function can derive.
pub fn resolve_config_switch_strategy(
    record: &Record,
    target_agent: &str,
    native_resume_supported: bool,
    families: &dyn AgentFamilies,
) -> ConfigSwitchStrategy {
    if !native_resume_supported {
        return ConfigSwitchStrategy::Handoff;
    }
    if can_native_resume(record, target_agent, families) {
        return ConfigSwitchStrategy::Resume;
    }
    if can_adopt(record, target_agent, families) {
        return ConfigSwitchStrategy::Adopt;
    }
    ConfigSwitchStrategy::Handoff
}

/// `hasTaskContinuationContext`: is there anything at all to continue from?
pub fn has_continuation_context(record: &Record) -> bool {
    let prompt = record
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default();
    !prompt.trim().is_empty() || has_task_session(record)
}

/// `applyResolvedTaskSession` — the "one task, one family" invariant in write form. Every
/// family's fields are cleared first, then only the resolved family is filled back in, so
/// a session that moved from claude to codex cannot leave a stale claude id behind for the
/// next recovery step to pick up.
pub fn apply_resolved_session(
    record: &Record,
    owner: &SessionOwner,
    session_id: Option<&str>,
    session_path: Option<&str>,
) -> Record {
    if session_id.is_none() && session_path.is_none() {
        return record.clone();
    }
    let mut next = record.clone();
    for prefix in ["claude", "codex", "dsh", "omp"] {
        next.0.remove(&format!("{prefix}SessionId"));
        next.0.remove(&format!("{prefix}SessionPath"));
    }
    next.0.insert(
        "sessionAgent".to_string(),
        Value::String(owner.agent.clone()),
    );
    next.0.insert(
        "sessionCodexLike".to_string(),
        Value::Bool(owner.family == ProtocolFamily::Codex),
    );
    next.0.insert(
        "sessionFamily".to_string(),
        Value::String(
            serde_json::to_value(owner.family)
                .expect("a family is a string")
                .as_str()
                .expect("a family is a string")
                .to_string(),
        ),
    );
    let prefix = family_prefix(owner.family);
    if let Some(id) = session_id {
        next.0
            .insert(format!("{prefix}SessionId"), Value::String(id.to_string()));
    }
    if let Some(path) = session_path {
        next.0.insert(
            format!("{prefix}SessionPath"),
            Value::String(path.to_string()),
        );
    }
    next
}

/// The fake family table the task tests share: four built-ins plus one custom profile
/// whose family only `Table` knows about.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{AgentFamilies, ProtocolFamily};

    pub(crate) struct Table {
        custom: Vec<(String, ProtocolFamily)>,
    }

    impl Table {
        pub(crate) fn new() -> Self {
            Table {
                custom: vec![("my-codex".to_string(), ProtocolFamily::Codex)],
            }
        }
    }

    impl AgentFamilies for Table {
        fn family(&self, agent: &str) -> ProtocolFamily {
            match agent {
                "claude" | "claude_gpt55" => ProtocolFamily::Claude,
                "codex" => ProtocolFamily::Codex,
                "dsh" => ProtocolFamily::Dsh,
                "omp" => ProtocolFamily::Omp,
                other => self
                    .custom
                    .iter()
                    .find(|(id, _)| id == other)
                    .map(|(_, family)| *family)
                    .unwrap_or(ProtocolFamily::Claude),
            }
        }
        fn is_builtin(&self, agent: &str) -> bool {
            matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh" | "omp")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::Table;
    use super::*;
    use serde_json::json;

    fn record(value: Value) -> Record {
        Record(value.as_object().unwrap().clone())
    }

    fn claude() -> Table {
        Table::new()
    }

    #[test]
    fn a_declared_family_beats_every_inference() {
        let record = record(json!({
            "agent": "claude", "sessionAgent": "codex", "sessionFamily": "codex",
            "sessionCodexLike": false, "claudeSessionId": "c"
        }));
        let owner = resolve_session_owner(&record, &claude());
        assert_eq!(owner.family, ProtocolFamily::Codex);
        assert!(owner.codex_like);
        assert_eq!(
            owner.agent, "codex",
            "the agent that owns the session, not the config"
        );
    }

    #[test]
    fn a_legacy_boolean_falls_back_unless_a_dsh_session_says_otherwise() {
        let plain = record(json!({"agent": "claude", "sessionCodexLike": true}));
        assert_eq!(
            resolve_session_owner(&plain, &claude()).family,
            ProtocolFamily::Codex
        );
        let dsh = record(json!({
            "agent": "dsh", "sessionCodexLike": false, "dshSessionId": "d"
        }));
        assert_eq!(
            resolve_session_owner(&dsh, &claude()).family,
            ProtocolFamily::Dsh
        );
        // A claude session present alongside is ambiguous, and the boolean wins.
        let both = record(json!({
            "agent": "dsh", "sessionCodexLike": false, "dshSessionId": "d", "claudeSessionId": "c"
        }));
        assert_eq!(
            resolve_session_owner(&both, &claude()).family,
            ProtocolFamily::Claude,
            "the guard is `&& !hasClaudeSession`, not a bare dsh check"
        );
    }

    #[test]
    fn one_populated_family_is_enough_but_two_are_not() {
        let single = record(json!({"agent": "whatever", "ompSessionPath": "/p.jsonl"}));
        assert_eq!(
            resolve_session_owner(&single, &claude()).family,
            ProtocolFamily::Omp
        );
        let ambiguous = record(json!({
            "agent": "codex", "ompSessionPath": "/p.jsonl", "claudeSessionId": "c"
        }));
        assert_eq!(
            resolve_session_owner(&ambiguous, &claude()).family,
            ProtocolFamily::Codex,
            "an ambiguous record defers to the configured agent"
        );
    }

    #[test]
    fn legacy_fields_are_offered_from_the_other_families_in_order() {
        let record =
            record(json!({"agent": "claude", "codexSessionId": "x", "ompSessionPath": "/o"}));
        let fields = session_fields(&record, ProtocolFamily::Claude);
        assert_eq!(fields.session_id, None);
        assert_eq!(fields.legacy_session_id.as_deref(), Some("x"));
        assert_eq!(
            fields.legacy_session_path, None,
            "the first family with a value wins"
        );
        let fields = session_fields(&record, ProtocolFamily::Codex);
        assert_eq!(fields.session_id.as_deref(), Some("x"));
        assert_eq!(fields.legacy_session_path.as_deref(), Some("/o"));
    }

    #[test]
    fn resume_ability_is_per_family_and_builtins_trust_each_other() {
        let claude_row =
            record(json!({"agent": "claude", "claudeSessionId": "c", "sessionFamily": "claude"}));
        let table = claude();
        assert!(can_native_resume(&claude_row, "claude_gpt55", &table));
        assert!(!can_native_resume(&claude_row, "my-codex", &table));
        let omp = record(json!({"agent": "omp", "ompSessionId": "o", "sessionFamily": "omp"}));
        assert!(can_native_resume(&omp, "omp", &table));
        let dsh = record(json!({"agent": "dsh", "dshSessionId": "d", "sessionFamily": "dsh"}));
        assert!(
            !can_native_resume(&dsh, "dsh", &table),
            "dsh never resumes natively, not even as itself"
        );
        let custom = record(json!({
            "agent": "my-codex", "sessionFamily": "codex", "codexSessionId": "x"
        }));
        assert!(can_native_resume(&custom, "my-codex", &table));
        assert!(
            !can_native_resume(&custom, "codex", &table),
            "a custom profile and the built-in share a family but not a home"
        );
    }

    #[test]
    fn adoption_is_only_for_the_two_families_with_a_home_to_migrate() {
        let table = claude();
        let claude_record = record(json!({
            "agent": "claude", "sessionFamily": "claude", "claudeSessionId": "c"
        }));
        assert!(
            !can_adopt(&claude_record, "claude_gpt55", &table),
            "two built-ins already resume natively; adoption would be a wasted copy"
        );
        let omp = record(json!({"agent": "omp", "sessionFamily": "omp", "ompSessionId": "o"}));
        assert!(!can_adopt(&omp, "omp", &table));
        let dsh = record(json!({"agent": "dsh", "sessionFamily": "dsh", "dshSessionId": "d"}));
        assert!(!can_adopt(&dsh, "dsh", &table));
        assert!(can_adopt(&claude_record, "my-custom-claude", &table));
    }

    #[test]
    fn an_unreadable_transcript_bypasses_both_resume_and_adoption() {
        let record =
            record(json!({"agent": "claude", "sessionFamily": "claude", "claudeSessionId": "c"}));
        let table = claude();
        assert_eq!(
            resolve_config_switch_strategy(&record, "claude_gpt55", true, &table),
            ConfigSwitchStrategy::Resume
        );
        assert_eq!(
            resolve_config_switch_strategy(&record, "my-custom-claude", true, &table),
            ConfigSwitchStrategy::Adopt
        );
        for strategy in [
            resolve_config_switch_strategy(&record, "claude_gpt55", false, &table),
            resolve_config_switch_strategy(&record, "my-custom-claude", false, &table),
        ] {
            assert_eq!(
                strategy,
                ConfigSwitchStrategy::Handoff,
                "nativeResumeSupported=false short-circuits both"
            );
        }
    }

    #[test]
    fn writing_a_resolved_session_leaves_exactly_one_family_behind() {
        let record = record(json!({
            "agent": "codex", "claudeSessionId": "c", "codexSessionPath": "/old",
            "dshSessionId": "d", "ompSessionId": "o"
        }));
        let owner = SessionOwner {
            agent: "codex".to_string(),
            family: ProtocolFamily::Codex,
            codex_like: true,
        };
        let next = apply_resolved_session(&record, &owner, Some("new"), Some("/p"));
        assert_eq!(next.get("codexSessionId"), Some(&json!("new")));
        assert_eq!(next.get("codexSessionPath"), Some(&json!("/p")));
        for key in [
            "claudeSessionId",
            "dshSessionId",
            "ompSessionId",
            "claudeSessionPath",
        ] {
            assert_eq!(next.get(key), None, "{key} must be gone");
        }
        assert_eq!(next.get("sessionFamily"), Some(&json!("codex")));
        assert_eq!(next.get("sessionCodexLike"), Some(&json!(true)));
        let untouched = apply_resolved_session(&record, &owner, None, None);
        assert_eq!(untouched, record, "no session to write means no rewrite");
    }

    #[test]
    fn continuation_context_needs_a_prompt_or_a_session() {
        assert!(!has_continuation_context(&record(json!({"prompt": "   "}))));
        assert!(has_continuation_context(&record(
            json!({"prompt": "do it"})
        )));
        assert!(has_continuation_context(&record(json!({
            "prompt": "", "dshSessionPath": "/p"
        }))));
        assert!(
            !has_task_session(&record(json!({"claudeSessionId": ""}))),
            "an empty id is not a session"
        );
    }
}
