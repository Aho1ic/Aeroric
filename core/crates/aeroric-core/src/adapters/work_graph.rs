//! 外部 Agent 工作图(AP7 / P9-02):memory、checkpoint、task tree、goal、subagents。
//!
//! ## 不变量(P9-02 原文,逐条落到类型上)
//!
//! 1. **外部身份显式**:节点带 `external_session_id` / `external_item_id` /
//!    `source` / `revision` / `persistence_kind`。缺外部 id 的映射直接拒绝
//!    ([`WorkGraphError::MissingExternalId`]),不允许「按显示名猜」。
//! 2. **跨会话不串**:节点键是 `(adapter, session, node_id)` 三元组。两棵会话树各
//!    有一个 `T1` 是两个不同节点;把 B 会话的更新按 node_id 打进 A 会话必须失败
//!    ([`WorkGraphError::SessionMismatch`])。
//! 3. **终态不复活**:`done`/`failed`/`abandoned` 之后,任何迟到的 `started`/`running`
//!    更新都不改状态([`WorkGraphError::TerminalStateNotRevivable`]),只记证据。
//! 4. **不重复 spawn**:上游已经建过的子代理,`register_external` 幂等归一到同一记录;
//!    同 cwd 并发子代理给出警告([`WorkGraphWarning::SharedWorkingDirectory`])。
//! 5. **goal 不自动续跑**:从持久化恢复的 goal 一律标记未确认,`resume` 需要显式
//!    确认([`WorkGraphError::GoalNotConfirmed`]);进程重启不会替用户接着跑。
//! 6. **checkpoint 失败不吞**:写入失败要能被调用方看见([`CheckpointOutcome::Failed`]),
//!    工作继续,但「存过」这个事实不得伪造。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// 外部身份:`source` 之外的每个字段都来自上游结构化响应,不从终端文本推断。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRef {
    pub source: String,
    pub external_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

/// 持久化性质:`InMemory` 的东西在重启后**不存在**,不能假装恢复成功。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceKind {
    /// 只活在进程内(重启即丢)。
    InMemory,
    /// 宿主 checkpoint 文件。
    Checkpointed,
    /// 上游自己持久化(宿主只是视图)。
    External,
}

/// 节点键:`adapter + session + node`。跨会话隔离就靠它。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeKey {
    pub adapter: String,
    pub session_id: String,
    pub node_id: String,
}

impl NodeKey {
    pub fn new(
        adapter: impl Into<String>,
        session_id: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Self {
        Self {
            adapter: adapter.into(),
            session_id: session_id.into(),
            node_id: node_id.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Running,
    Done,
    Failed,
    /// 上游放弃/被取代——同样是终态。
    Abandoned,
}

impl NodeStatus {
    /// 终态一旦到达就不再有出边。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            NodeStatus::Done | NodeStatus::Failed | NodeStatus::Abandoned
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNode {
    pub key: NodeKey,
    pub status: NodeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<NodeKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub persistence: PersistenceKind,
    #[serde(default)]
    pub external: Option<ExternalRef>,
    /// 迟到的更新会被记在这里(状态不变),给 UI 与日志留证据。
    #[serde(default)]
    pub stale_updates: Vec<String>,
}

/// 警告:不是错误,但用户必须看得见。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "warning", rename_all = "snake_case")]
pub enum WorkGraphWarning {
    /// 同一个工作目录上有并发子代理,可能互相踩文件。
    SharedWorkingDirectory { cwd: String, sessions: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum WorkGraphError {
    /// 外部身份缺失:无法把外部对象映射成节点。
    MissingExternalId { node_id: String },
    /// 更新打到了别的会话的节点上。
    SessionMismatch {
        expected_session: String,
        got_session: String,
        node_id: String,
    },
    /// 终态不被复活。
    TerminalStateNotRevivable {
        node_id: String,
        current: NodeStatus,
        attempted: NodeStatus,
    },
    /// 恢复的 goal 未获用户确认,不得继续执行。
    GoalNotConfirmed { goal_id: String },
    /// 未知 goal。
    UnknownGoal { goal_id: String },
    /// revision 冲突(乐观并发)。
    RevisionConflict {
        goal_id: String,
        expected: u64,
        got: u64,
    },
}

#[derive(Clone, Debug, Default)]
pub struct TaskTree {
    nodes: BTreeMap<NodeKey, TaskNode>,
}

impl TaskTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记外部节点。`external_item_id` 缺失即拒绝——不允许按显示名猜身份。
    pub fn register_external(
        &mut self,
        session_id: &str,
        node_id: &str,
        source: &str,
        external_item_id: Option<&str>,
        persistence: PersistenceKind,
    ) -> Result<NodeKey, WorkGraphError> {
        if external_item_id
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Err(WorkGraphError::MissingExternalId {
                node_id: node_id.to_string(),
            });
        }
        let key = NodeKey::new(source, session_id, node_id);
        self.nodes.entry(key.clone()).or_insert_with(|| TaskNode {
            key: key.clone(),
            status: NodeStatus::Pending,
            parent: None,
            label: None,
            persistence,
            external: Some(ExternalRef {
                source: source.to_string(),
                external_session_id: session_id.to_string(),
                external_item_id: external_item_id.map(str::to_string),
                revision: None,
            }),
            stale_updates: Vec::new(),
        });
        Ok(key)
    }

    /// 应用一次状态更新。终态不可复活;跨会话更新被拒绝。
    pub fn apply_status(
        &mut self,
        key: &NodeKey,
        session_id: &str,
        status: NodeStatus,
    ) -> Result<&TaskNode, WorkGraphError> {
        if key.session_id != session_id {
            return Err(WorkGraphError::SessionMismatch {
                expected_session: key.session_id.clone(),
                got_session: session_id.to_string(),
                node_id: key.node_id.clone(),
            });
        }
        let node = self
            .nodes
            .get_mut(key)
            .ok_or_else(|| WorkGraphError::MissingExternalId {
                node_id: key.node_id.clone(),
            })?;
        if node.status.is_terminal() && !status.is_terminal() {
            node.stale_updates
                .push(format!("late {:?} after {:?}", status, node.status));
            return Err(WorkGraphError::TerminalStateNotRevivable {
                node_id: key.node_id.clone(),
                current: node.status,
                attempted: status,
            });
        }
        node.status = status;
        Ok(self.nodes.get(key).expect("node was just inserted"))
    }

    pub fn get(&self, key: &NodeKey) -> Option<&TaskNode> {
        self.nodes.get(key)
    }

    /// 按会话过滤出的节点(UI 每棵会话树一份)。
    pub fn session_nodes(&self, session_id: &str) -> Vec<&TaskNode> {
        self.nodes
            .values()
            .filter(|node| node.key.session_id == session_id)
            .collect()
    }

    /// 重启恢复:只有 `Checkpointed`/`External` 的节点算恢复,`InMemory` 不复活。
    pub fn restore(&mut self) {
        self.nodes
            .retain(|_, node| node.persistence != PersistenceKind::InMemory);
    }
}

// ── 子代理 ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRecord {
    pub session_id: String,
    pub parent_session_id: String,
    pub mode: Option<String>,
    pub cwd: Option<String>,
    pub running: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SubagentRegistry {
    records: BTreeMap<String, SubagentRecord>,
    /// 本进程**自己**启动过的子代理。上游已创建的不会进这里——这正是
    /// 「Aeroric 不重复 spawn 上游子代理」在数据结构上的落点。
    owned: BTreeSet<String>,
}

impl SubagentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记上游已存在的子代理。**幂等**:同一个 session 再来一次不会新建记录,
    /// 也不会触发第二次 spawn(宿主据此不重复启动)。
    pub fn register_external(&mut self, record: SubagentRecord) -> (bool, Vec<WorkGraphWarning>) {
        let created = match self.records.get(&record.session_id) {
            Some(existing) => existing != &record,
            None => true,
        };
        if !created && !self.records.contains_key(&record.session_id) {
            return (false, Vec::new());
        }
        let already = self.records.contains_key(&record.session_id);
        self.records
            .insert(record.session_id.clone(), record.clone());
        (already, self.warnings())
    }

    /// 同 cwd 上有多个**运行中**子代理 → 警告(它们会互相踩文件)。
    pub fn warnings(&self) -> Vec<WorkGraphWarning> {
        let mut by_cwd: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for record in self.records.values().filter(|record| record.running) {
            if let Some(cwd) = record.cwd.as_deref() {
                by_cwd.entry(cwd).or_default().insert(&record.session_id);
            }
        }
        by_cwd
            .into_iter()
            .filter(|(_, sessions)| sessions.len() > 1)
            .map(|(cwd, sessions)| WorkGraphWarning::SharedWorkingDirectory {
                cwd: cwd.to_string(),
                sessions: sessions.into_iter().map(str::to_string).collect(),
            })
            .collect()
    }

    pub fn get(&self, session_id: &str) -> Option<&SubagentRecord> {
        self.records.get(session_id)
    }

    /// 该子代理是否还需要宿主启动。
    ///
    /// 一旦登记过(无论是上游发现的还是本进程建的)就返回 `false`——重复调用
    /// `register_external` 不会产生第二次 spawn。
    pub fn needs_spawn(&self, session_id: &str) -> bool {
        !self.records.contains_key(session_id)
    }

    /// 宿主确实启动了一个子代理后调用;再次调用是 no-op。
    /// 返回 `true` 表示本次是这个 session 的**首次**启动。
    pub fn record_spawn(&mut self, session_id: &str) -> bool {
        self.owned.insert(session_id.to_string())
    }

    /// 本进程启动过的子代理。用于退出时清理自己建的,不碰上游的。
    pub fn owned_spawns(&self) -> Vec<&str> {
        self.owned.iter().map(String::as_str).collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ── goal ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Complete,
    /// 重启后从持久化读回,但**未获用户确认**:不继续跑。
    RestoredUnconfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRecord {
    pub goal_id: String,
    pub session_id: String,
    pub objective: String,
    pub revision: u64,
    pub status: GoalStatus,
    pub persistence: PersistenceKind,
}

#[derive(Clone, Debug, Default)]
pub struct GoalStore {
    goals: BTreeMap<String, GoalRecord>,
}

impl GoalStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, goal: GoalRecord) {
        self.goals.insert(goal.goal_id.clone(), goal);
    }

    /// 重启恢复:持久化的 active goal **不自动续跑**,改为待确认。
    pub fn restore_after_restart(&mut self) -> Vec<GoalRecord> {
        let mut pending = Vec::new();
        self.goals
            .retain(|_, goal| goal.persistence != PersistenceKind::InMemory);
        for goal in self.goals.values_mut() {
            if goal.status == GoalStatus::Active {
                goal.status = GoalStatus::RestoredUnconfirmed;
                pending.push(goal.clone());
            }
        }
        pending
    }

    /// 用户确认后才继续;未确认的 goal 拒绝 resume。
    pub fn resume(
        &mut self,
        goal_id: &str,
        confirmed: bool,
    ) -> Result<&GoalRecord, WorkGraphError> {
        let goal = self
            .goals
            .get_mut(goal_id)
            .ok_or_else(|| WorkGraphError::UnknownGoal {
                goal_id: goal_id.to_string(),
            })?;
        if goal.status == GoalStatus::RestoredUnconfirmed && !confirmed {
            return Err(WorkGraphError::GoalNotConfirmed {
                goal_id: goal_id.to_string(),
            });
        }
        goal.status = GoalStatus::Active;
        Ok(self.goals.get(goal_id).expect("goal exists"))
    }

    /// 乐观并发:revision 不匹配拒绝写入(迟到响应不覆盖新状态)。
    pub fn edit(
        &mut self,
        goal_id: &str,
        expected_revision: u64,
        objective: Option<&str>,
    ) -> Result<&GoalRecord, WorkGraphError> {
        let goal = self
            .goals
            .get_mut(goal_id)
            .ok_or_else(|| WorkGraphError::UnknownGoal {
                goal_id: goal_id.to_string(),
            })?;
        if goal.revision != expected_revision {
            return Err(WorkGraphError::RevisionConflict {
                goal_id: goal_id.to_string(),
                expected: expected_revision,
                got: goal.revision,
            });
        }
        if let Some(objective) = objective {
            goal.objective = objective.to_string();
        }
        goal.revision += 1;
        Ok(self.goals.get(goal_id).expect("goal exists"))
    }

    pub fn get(&self, goal_id: &str) -> Option<&GoalRecord> {
        self.goals.get(goal_id)
    }
}

// ── checkpoint ───────────────────────────────────────────────────────────────

/// checkpoint 写入端口。宿主实现文件 I/O;core 不碰磁盘。
///
/// **不吞错**:`save` 返回 `Err` 时调用方要拿到 [`CheckpointOutcome::Failed`]。
pub trait CheckpointWriter: Send + Sync {
    fn save(&self, key: &str, payload: &serde_json::Value) -> Result<(), String>;
    fn load(&self, key: &str) -> Result<Option<serde_json::Value>, String>;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CheckpointOutcome {
    Saved {
        key: String,
    },
    /// 写入失败:工作继续,但不得对外声称「已存」。
    Failed {
        key: String,
        error: String,
    },
}

/// 写 checkpoint。失败不 panic、不静默。
pub fn write_checkpoint(
    writer: &dyn CheckpointWriter,
    key: &str,
    payload: &serde_json::Value,
) -> CheckpointOutcome {
    match writer.save(key, payload) {
        Ok(()) => CheckpointOutcome::Saved {
            key: key.to_string(),
        },
        Err(error) => CheckpointOutcome::Failed {
            key: key.to_string(),
            error,
        },
    }
}

// ── memory ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    /// 用户明确要求记住。
    UserExplicit,
    /// 模型推断(可被审阅/删除)。
    ModelInferred,
    /// 从外部导入。
    Imported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    pub memory_id: String,
    /// 归属:跨会话记忆必须绑定来源会话,避免 A 会话的事实污染 B 会话。
    pub session_id: String,
    pub source: MemorySource,
    pub revision: u64,
    pub persistence: PersistenceKind,
    pub text: String,
}

impl MemoryItem {
    /// 只有用户明确要求或用户审阅通过的推断项才允许**跨会话**复用。
    pub fn is_cross_session_safe(&self) -> bool {
        matches!(self.source, MemorySource::UserExplicit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn two_sessions_may_each_have_a_t1_without_crossing() {
        let mut tree = TaskTree::new();
        let a = tree
            .register_external(
                "session-a",
                "T1",
                "dsh",
                Some("item-a"),
                PersistenceKind::Checkpointed,
            )
            .unwrap();
        let b = tree
            .register_external(
                "session-b",
                "T1",
                "dsh",
                Some("item-b"),
                PersistenceKind::Checkpointed,
            )
            .unwrap();
        assert_ne!(a, b);
        tree.apply_status(&a, "session-a", NodeStatus::Running)
            .unwrap();
        assert_eq!(tree.get(&a).unwrap().status, NodeStatus::Running);
        assert_eq!(tree.get(&b).unwrap().status, NodeStatus::Pending);
        assert_eq!(tree.session_nodes("session-a").len(), 1);
    }

    #[test]
    fn an_update_for_another_session_is_refused() {
        let mut tree = TaskTree::new();
        let a = tree
            .register_external(
                "session-a",
                "T1",
                "dsh",
                Some("item-a"),
                PersistenceKind::Checkpointed,
            )
            .unwrap();
        let error = tree
            .apply_status(&a, "session-b", NodeStatus::Running)
            .unwrap_err();
        assert_eq!(
            error,
            WorkGraphError::SessionMismatch {
                expected_session: "session-a".to_string(),
                got_session: "session-b".to_string(),
                node_id: "T1".to_string(),
            }
        );
    }

    #[test]
    fn terminal_states_are_not_revived_by_late_updates() {
        let mut tree = TaskTree::new();
        let key = tree
            .register_external("s", "T1", "dsh", Some("i"), PersistenceKind::Checkpointed)
            .unwrap();
        tree.apply_status(&key, "s", NodeStatus::Done).unwrap();
        let error = tree
            .apply_status(&key, "s", NodeStatus::Running)
            .unwrap_err();
        assert_eq!(
            error,
            WorkGraphError::TerminalStateNotRevivable {
                node_id: "T1".to_string(),
                current: NodeStatus::Done,
                attempted: NodeStatus::Running,
            }
        );
        let node = tree.get(&key).unwrap();
        assert_eq!(node.status, NodeStatus::Done);
        // 迟到证据留痕,便于诊断而不是无声丢弃。
        assert_eq!(node.stale_updates.len(), 1);
        // 终态 → 终态(如 done → abandoned)是允许的更正。
        tree.apply_status(&key, "s", NodeStatus::Abandoned).unwrap();
    }

    #[test]
    fn a_node_without_an_external_id_is_refused() {
        let mut tree = TaskTree::new();
        let error = tree
            .register_external("s", "T1", "dsh", None, PersistenceKind::InMemory)
            .unwrap_err();
        assert_eq!(
            error,
            WorkGraphError::MissingExternalId {
                node_id: "T1".to_string()
            }
        );
        assert!(tree
            .register_external("s", "T1", "dsh", Some("  "), PersistenceKind::InMemory)
            .is_err());
    }

    #[test]
    fn in_memory_nodes_do_not_survive_a_restart() {
        let mut tree = TaskTree::new();
        tree.register_external("s", "T1", "dsh", Some("i1"), PersistenceKind::InMemory)
            .unwrap();
        tree.register_external("s", "T2", "dsh", Some("i2"), PersistenceKind::Checkpointed)
            .unwrap();
        tree.restore();
        assert_eq!(tree.session_nodes("s").len(), 1);
        assert_eq!(tree.session_nodes("s")[0].key.node_id, "T2");
    }

    #[test]
    fn external_subagents_are_registered_once_and_shared_cwd_warns() {
        let mut registry = SubagentRegistry::new();
        let record = SubagentRecord {
            session_id: "child-1".to_string(),
            parent_session_id: "parent".to_string(),
            mode: Some("continuable".to_string()),
            cwd: Some("/repo".to_string()),
            running: true,
        };
        let (already, warnings) = registry.register_external(record.clone());
        assert!(!already, "first registration is new");
        assert!(warnings.is_empty());

        let (already, _) = registry.register_external(record.clone());
        assert!(already, "the same upstream subagent is not spawned twice");
        assert_eq!(registry.len(), 1);

        // 上游已有的子代理,本进程绝不代它启动。
        assert!(!registry.needs_spawn("child-1"));
        assert!(registry.needs_spawn("child-2"));
        assert!(registry.record_spawn("child-2"), "first spawn of child-2");
        assert!(
            !registry.record_spawn("child-2"),
            "a second spawn of the same session is a no-op"
        );
        assert_eq!(registry.owned_spawns(), vec!["child-2"]);
        assert!(
            !registry.owned_spawns().contains(&"child-1"),
            "upstream-created subagents are never ours to clean up"
        );

        let (_, warnings) = registry.register_external(SubagentRecord {
            session_id: "child-2".to_string(),
            cwd: Some("/repo".to_string()),
            ..record
        });
        assert_eq!(registry.len(), 2);
        assert_eq!(
            warnings,
            vec![WorkGraphWarning::SharedWorkingDirectory {
                cwd: "/repo".to_string(),
                sessions: vec!["child-1".to_string(), "child-2".to_string()],
            }]
        );
    }

    #[test]
    fn a_restored_goal_never_resumes_without_confirmation() {
        let mut store = GoalStore::new();
        store.insert(GoalRecord {
            goal_id: "g1".to_string(),
            session_id: "s".to_string(),
            objective: "ship AP7".to_string(),
            revision: 1,
            status: GoalStatus::Active,
            persistence: PersistenceKind::Checkpointed,
        });
        store.insert(GoalRecord {
            goal_id: "g2".to_string(),
            session_id: "s".to_string(),
            objective: "scratch".to_string(),
            revision: 1,
            status: GoalStatus::Active,
            persistence: PersistenceKind::InMemory,
        });
        let pending = store.restore_after_restart();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].goal_id, "g1");
        assert_eq!(
            store.get("g1").unwrap().status,
            GoalStatus::RestoredUnconfirmed
        );
        assert!(
            store.get("g2").is_none(),
            "in-memory goals do not come back"
        );

        let error = store.resume("g1", false).unwrap_err();
        assert_eq!(
            error,
            WorkGraphError::GoalNotConfirmed {
                goal_id: "g1".to_string()
            }
        );
        assert_eq!(store.resume("g1", true).unwrap().status, GoalStatus::Active);
    }

    #[test]
    fn stale_goal_edits_do_not_overwrite_newer_state() {
        let mut store = GoalStore::new();
        store.insert(GoalRecord {
            goal_id: "g1".to_string(),
            session_id: "s".to_string(),
            objective: "v1".to_string(),
            revision: 7,
            status: GoalStatus::Active,
            persistence: PersistenceKind::External,
        });
        let error = store.edit("g1", 6, Some("stale")).unwrap_err();
        assert_eq!(
            error,
            WorkGraphError::RevisionConflict {
                goal_id: "g1".to_string(),
                expected: 6,
                got: 7,
            }
        );
        assert_eq!(store.get("g1").unwrap().objective, "v1");
        let updated = store.edit("g1", 7, Some("v2")).unwrap();
        assert_eq!(updated.objective, "v2");
        assert_eq!(updated.revision, 8);
    }

    struct FailingWriter;
    impl CheckpointWriter for FailingWriter {
        fn save(&self, _key: &str, _payload: &serde_json::Value) -> Result<(), String> {
            Err("disk full".to_string())
        }
        fn load(&self, _key: &str) -> Result<Option<serde_json::Value>, String> {
            Ok(None)
        }
    }

    #[test]
    fn a_failed_checkpoint_is_reported_not_swallowed() {
        let outcome = write_checkpoint(&FailingWriter, "run-1", &json!({"stage": "a"}));
        match outcome {
            CheckpointOutcome::Failed { key, error } => {
                assert_eq!(key, "run-1");
                assert!(error.contains("disk full"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn only_user_explicit_memory_is_cross_session_safe() {
        let base = MemoryItem {
            memory_id: "m1".to_string(),
            session_id: "s".to_string(),
            source: MemorySource::ModelInferred,
            revision: 1,
            persistence: PersistenceKind::Checkpointed,
            text: "the user prefers tabs".to_string(),
        };
        assert!(!base.is_cross_session_safe());
        assert!(MemoryItem {
            source: MemorySource::UserExplicit,
            ..base
        }
        .is_cross_session_safe());
    }
}
