//! Task identity and state. Mirrors `src/types.ts` `Task` for the fields that
//! cross the wire, and keeps the legacy spellings so an old data directory reads
//! without a migration.

use serde::{Deserialize, Serialize};

/// Wire status of a task. All nine values are load-bearing today: `detached` and
/// `interrupted` in particular are *not* synonyms for `running`/`pending`, and
/// collapsing them would let a cancelled task look resumable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Pending,
    Running,
    InputRequired,
    Detached,
    Interrupted,
    Done,
    Failed,
    Cancelled,
}

impl TaskStatus {
    /// The complete wire vocabulary, in the order `contracts/native/v1/` publishes
    /// it. Pinned by `the_published_vocabulary_matches_the_enum`.
    pub const ALL: &'static [TaskStatus] = &[
        TaskStatus::Todo,
        TaskStatus::Pending,
        TaskStatus::Running,
        TaskStatus::InputRequired,
        TaskStatus::Detached,
        TaskStatus::Interrupted,
        TaskStatus::Done,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ];

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Done | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    /// States that still hold an agent or expect input; cleanup must leave them alone.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            TaskStatus::Pending
                | TaskStatus::Running
                | TaskStatus::InputRequired
                | TaskStatus::Detached
        )
    }
}

/// Protocol family: decides launch arguments, session file format and config shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolFamily {
    Claude,
    Codex,
    Dsh,
    Omp,
}

impl ProtocolFamily {
    pub const ALL: &'static [ProtocolFamily] = &[
        ProtocolFamily::Claude,
        ProtocolFamily::Codex,
        ProtocolFamily::Dsh,
        ProtocolFamily::Omp,
    ];
}

/// Which agent a task runs. Built-ins are closed; a custom profile is an open
/// string, so the family must be carried separately — inferring it from the id
/// would let a custom agent named `codex-like` change the wire protocol.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum AgentId {
    BuiltIn {
        built_in: BuiltInAgent,
    },
    Custom {
        id: String,
        family: Option<ProtocolFamily>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInAgent {
    Claude,
    ClaudeGpt55,
    Codex,
    Dsh,
    Omp,
}

impl BuiltInAgent {
    pub const ALL: &'static [BuiltInAgent] = &[
        BuiltInAgent::Claude,
        BuiltInAgent::ClaudeGpt55,
        BuiltInAgent::Codex,
        BuiltInAgent::Dsh,
        BuiltInAgent::Omp,
    ];

    pub fn family(self) -> ProtocolFamily {
        match self {
            BuiltInAgent::Claude | BuiltInAgent::ClaudeGpt55 => ProtocolFamily::Claude,
            BuiltInAgent::Codex => ProtocolFamily::Codex,
            BuiltInAgent::Dsh => ProtocolFamily::Dsh,
            BuiltInAgent::Omp => ProtocolFamily::Omp,
        }
    }
}

impl AgentId {
    /// Falls back only for records written before `sessionFamily` existed; the
    /// caller must not use this to *choose* a protocol for a new launch.
    pub fn declared_family(&self) -> Option<ProtocolFamily> {
        match self {
            AgentId::BuiltIn { built_in } => Some(built_in.family()),
            AgentId::Custom { family, .. } => *family,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Ask,
    AutoEdit,
    FullAccess,
}

impl PermissionMode {
    pub const ALL: &'static [PermissionMode] = &[
        PermissionMode::Ask,
        PermissionMode::AutoEdit,
        PermissionMode::FullAccess,
    ];
}

/// Authoritative task snapshot. `revision` and `generation` are decimal strings so
/// a JS client cannot round them; `expected_revision` on writes is what makes a
/// second window fail loudly instead of overwriting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: String,
    pub project_id: String,
    pub status: TaskStatus,
    pub agent: AgentId,
    #[serde(default)]
    pub revision: crate::envelope::Quantity,
    #[serde(default)]
    pub generation: crate::envelope::Quantity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_owner: Option<crate::session::SessionOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    /// Set once a worktree was merged or discarded: the branch and path stay for
    /// audit, but resume/merge/discard must all be refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_discarded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    /// Anything a newer writer added. Flatten keeps it so a round trip cannot drop
    /// a field this build has never heard of.
    #[serde(
        flatten,
        default,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub unknown_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

impl TaskSnapshot {
    /// "Is there a transcript to go back to", mirroring `hasTaskSession` in
    /// `src/taskSession.ts`: an id **or** a path counts, because a record can have
    /// discovered one without the other.
    pub fn has_session(&self) -> bool {
        self.session_owner.as_ref().is_some_and(|owner| {
            !owner.session_id.is_empty()
                || owner.path.as_deref().is_some_and(|path| !path.is_empty())
        })
    }

    /// The resume rule the shipping client already applies, in
    /// `src/components/task-panel/TaskList.tsx` (`canResumeTask`): settled, not
    /// `todo`, worktree intact, and a recorded transcript. Refusing here is what
    /// stops a resume from silently becoming a brand-new session on a task whose
    /// history is gone.
    pub fn can_resume(&self) -> bool {
        self.status != TaskStatus::Todo
            && !self.status.is_live()
            && self.worktree_discarded != Some(true)
            && self.has_session()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_nine_statuses_round_trip_with_their_wire_spelling() {
        for (value, text) in [
            (TaskStatus::Todo, "\"todo\""),
            (TaskStatus::InputRequired, "\"input_required\""),
            (TaskStatus::Detached, "\"detached\""),
            (TaskStatus::Interrupted, "\"interrupted\""),
            (TaskStatus::Cancelled, "\"cancelled\""),
        ] {
            assert_eq!(serde_json::to_string(&value).unwrap(), text);
            let back: TaskStatus = serde_json::from_str(text).unwrap();
            assert_eq!(back, value);
        }
    }

    #[test]
    fn terminal_and_live_sets_do_not_overlap() {
        for status in [TaskStatus::Done, TaskStatus::Failed, TaskStatus::Cancelled] {
            assert!(status.is_terminal());
            assert!(!status.is_live(), "{status:?} may not be both");
        }
        assert!(TaskStatus::Detached.is_live());
        assert!(!TaskStatus::Detached.is_terminal());
    }

    #[test]
    fn a_custom_agent_id_carries_its_family_instead_of_guessing_one() {
        let custom: AgentId = serde_json::from_str(r#"{"id":"my-cli","family":"codex"}"#).unwrap();
        assert_eq!(custom.declared_family(), Some(ProtocolFamily::Codex));
        let naked: AgentId = serde_json::from_str(r#"{"id":"weird-name"}"#).unwrap();
        assert_eq!(naked.declared_family(), None);
    }

    #[test]
    fn a_discarded_worktree_stays_visible_but_unresumable() {
        let json = r#"{"id":"t","projectId":"p","status":"done","agent":{"builtIn":"codex"},
            "worktreePath":"/w","worktreeBranch":"b","worktreeDiscarded":true,"unknownField":{"kept":1},
            "sessionOwner":{"family":"codex","sessionId":"s1"}}"#;
        let snapshot: TaskSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snapshot.worktree_path.as_deref(), Some("/w"));
        assert!(snapshot.has_session(), "the transcript is still there");
        assert!(!snapshot.can_resume());
        assert!(snapshot.unknown_fields.contains_key("unknownField"));
    }

    /// Each clause of the resume rule gets one case that flips it on its own, so
    /// the predicate cannot pass by being always false.
    #[test]
    fn resume_needs_a_settled_task_with_a_transcript() {
        let session = r#"{"family":"codex","sessionId":"s1"}"#;
        let record = |status: &str, owner: Option<&str>| -> TaskSnapshot {
            let tail = match owner {
                Some(value) => format!(r#","sessionOwner":{value}"#),
                None => String::new(),
            };
            serde_json::from_str(&format!(
                r#"{{"id":"t","projectId":"p","status":"{status}","agent":{{"builtIn":"codex"}}{tail}}}"#
            ))
            .unwrap()
        };

        assert!(record("done", Some(session)).can_resume());
        for status in ["todo", "pending", "running", "input_required", "detached"] {
            assert!(!record(status, Some(session)).can_resume(), "{status}");
        }
        for status in ["done", "failed", "cancelled", "interrupted"] {
            assert!(
                !record(status, None).can_resume(),
                "{status} without any transcript is not resumable"
            );
            assert!(
                record(status, Some(session)).can_resume(),
                "{status} with a transcript must be resumable"
            );
        }
        // A path alone counts, exactly as the shipping client's `hasTaskSession`.
        let path_only: TaskSnapshot = serde_json::from_str(
            r#"{"id":"t","projectId":"p","status":"done","agent":{"builtIn":"omp"},
               "sessionOwner":{"family":"omp","sessionId":"","path":"/s.jsonl"}}"#,
        )
        .unwrap();
        assert!(path_only.has_session());
        assert!(path_only.can_resume());
        let empty: TaskSnapshot = serde_json::from_str(
            r#"{"id":"t","projectId":"p","status":"done","agent":{"builtIn":"omp"},
               "sessionOwner":{"family":"omp","sessionId":"","path":""}}"#,
        )
        .unwrap();
        assert!(
            !empty.has_session(),
            "an empty id and path is no transcript"
        );
    }

    /// These `ALL` lists are what `contracts/native/v1/` publishes, so a variant
    /// missing from one is a silently shrunken wire contract. Each match below is
    /// exhaustive: growing an enum fails to compile here, and the length pins then
    /// force `ALL` and the schema to move in the same change.
    #[test]
    fn every_vocabulary_lists_the_complete_enum() {
        fn status_index(v: TaskStatus) -> usize {
            match v {
                TaskStatus::Todo => 0,
                TaskStatus::Pending => 1,
                TaskStatus::Running => 2,
                TaskStatus::InputRequired => 3,
                TaskStatus::Detached => 4,
                TaskStatus::Interrupted => 5,
                TaskStatus::Done => 6,
                TaskStatus::Failed => 7,
                TaskStatus::Cancelled => 8,
            }
        }
        fn family_index(v: ProtocolFamily) -> usize {
            match v {
                ProtocolFamily::Claude => 0,
                ProtocolFamily::Codex => 1,
                ProtocolFamily::Dsh => 2,
                ProtocolFamily::Omp => 3,
            }
        }
        fn agent_index(v: BuiltInAgent) -> usize {
            match v {
                BuiltInAgent::Claude => 0,
                BuiltInAgent::ClaudeGpt55 => 1,
                BuiltInAgent::Codex => 2,
                BuiltInAgent::Dsh => 3,
                BuiltInAgent::Omp => 4,
            }
        }
        fn mode_index(v: PermissionMode) -> usize {
            match v {
                PermissionMode::Ask => 0,
                PermissionMode::AutoEdit => 1,
                PermissionMode::FullAccess => 2,
            }
        }
        fn dense(mut indices: Vec<usize>) -> usize {
            indices.sort_unstable();
            indices.dedup();
            assert_eq!(indices, (0..indices.len()).collect::<Vec<_>>());
            indices.len()
        }
        assert_eq!(
            dense(TaskStatus::ALL.iter().map(|v| status_index(*v)).collect()),
            9
        );
        assert_eq!(
            dense(
                ProtocolFamily::ALL
                    .iter()
                    .map(|v| family_index(*v))
                    .collect()
            ),
            4
        );
        assert_eq!(
            dense(BuiltInAgent::ALL.iter().map(|v| agent_index(*v)).collect()),
            5
        );
        assert_eq!(
            dense(PermissionMode::ALL.iter().map(|v| mode_index(*v)).collect()),
            3
        );
    }

    #[test]
    fn every_variant_serializes_to_its_own_listed_name() {
        for status in TaskStatus::ALL {
            let text = serde_json::to_string(status).unwrap();
            assert_eq!(
                serde_json::from_str::<TaskStatus>(&text).unwrap(),
                *status,
                "{text}"
            );
        }
        for family in ProtocolFamily::ALL {
            let text = serde_json::to_string(family).unwrap();
            assert!(text.starts_with('"') && text.len() > 2, "{text}");
        }
    }
}
