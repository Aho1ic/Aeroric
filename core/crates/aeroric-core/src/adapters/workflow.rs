//! 工作流适配(AP7 / P9-03):外部 workflow 的执行 owner、journal、有界重试、
//! memory maintenance(Dream / Distill)。
//!
//! ## 为什么宿主不做执行 owner
//!
//! P9-03 的要求是「外部工作流脚本由它自己的运行时执行,宿主不 `eval`」。这里把它
//! 变成结构约束:本模块只产出 [`WorkflowStep`](要跑什么)与 [`RetryDecision`]
//! (能不能重跑),**从不**提供执行入口。执行由外部 adapter 负责,宿主只持 journal。
//!
//! ## Journal 绑定
//!
//! [`WorkflowJournal`] 记录 `script_hash` / `adapter_version` / 每步 `input_hash`。
//! 续跑前必须 [`WorkflowJournal::can_resume`]:脚本变了、适配器变了、步骤输入变了
//! 都拒绝接着跑旧 journal,**但允许明确新建一个**(拒绝不是死路)。
//!
//! ## 有界重试
//!
//! 副作用步骤(commit/push/delete/worktree)默认 [`RetryPolicy::Never`] 且结果不明时
//! 先 reconcile;达到上限即 [`RetryDecision::Blocked`]。`max_attempts` 必须 >= 1,
//! `0` 是配置错误而不是「无限重试」。
//!
//! ## Dream / Distill
//!
//! 默认关闭([`MemoryMaintenanceConfig::default`] 的 `enabled == false`),开启后产物
//! 先进 staging,用户 review 通过才生效。生成脚本由外部运行时执行,本模块只做状态。

use serde::{Deserialize, Serialize};

// ── journal ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    pub stage: String,
    /// 该步骤的输入摘要(命令/参数/目标路径的 hash)。
    pub input_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_hash: Option<String>,
    /// 有副作用的步骤(commit/push/delete/worktree 等)。
    pub side_effect: bool,
    pub attempts: u32,
    pub status: StepStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed,
    /// 结果不明(超时/连接断):**不得**直接重跑,先 reconcile。
    Unknown,
    Skipped,
    Blocked,
}

impl StepStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            StepStatus::Done | StepStatus::Skipped | StepStatus::Blocked
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowJournal {
    pub workflow_id: String,
    pub adapter_version: String,
    pub script_hash: String,
    pub steps: Vec<StepRecord>,
    /// 上次 reconcile 的结论(结果不明的步骤在这里被裁定)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciled: Option<String>,
}

/// 拒绝续跑的原因。每一种都能被用户看见,不做「静默重来」。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ResumeRefusal {
    ScriptChanged {
        journal: String,
        current: String,
    },
    AdapterVersionChanged {
        journal: String,
        current: String,
    },
    StepInputChanged {
        stage: String,
        journal: String,
        current: String,
    },
    /// 结果不明的步骤还没 reconcile 就要求续跑。
    UnreconciledUnknownStep {
        stage: String,
    },
    /// 达到重试上限的步骤挡在那里。
    BlockedStep {
        stage: String,
        attempts: u32,
    },
}

impl WorkflowJournal {
    pub fn new(
        workflow_id: impl Into<String>,
        script_hash: impl Into<String>,
        adapter_version: impl Into<String>,
    ) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            adapter_version: adapter_version.into(),
            script_hash: script_hash.into(),
            steps: Vec::new(),
            reconciled: None,
        }
    }

    /// 记录(或更新)一个步骤。同一舞台重复调用是幂等的,不会插两条。
    pub fn record_step(
        &mut self,
        stage: &str,
        input_hash: &str,
        side_effect: bool,
        status: StepStatus,
    ) {
        if let Some(existing) = self.steps.iter_mut().find(|step| step.stage == stage) {
            existing.input_hash = input_hash.to_string();
            existing.side_effect = side_effect;
            existing.status = status;
            return;
        }
        self.steps.push(StepRecord {
            stage: stage.to_string(),
            input_hash: input_hash.to_string(),
            output_hash: None,
            side_effect,
            attempts: 0,
            status,
        });
    }

    pub fn step(&self, stage: &str) -> Option<&StepRecord> {
        self.steps.iter().find(|step| step.stage == stage)
    }

    /// 续跑门槛:`script_hash` 与 `adapter_version` 都是**部分身份**,任一变化都拒绝,
    /// 因为「同名的步骤」在新脚本里可能干了别的事。
    pub fn can_resume(
        &self,
        script_hash: &str,
        adapter_version: &str,
        step_inputs: &[(&str, &str)],
        max_attempts: u32,
    ) -> Result<(), ResumeRefusal> {
        if self.script_hash != script_hash {
            return Err(ResumeRefusal::ScriptChanged {
                journal: self.script_hash.clone(),
                current: script_hash.to_string(),
            });
        }
        if self.adapter_version != adapter_version {
            return Err(ResumeRefusal::AdapterVersionChanged {
                journal: self.adapter_version.clone(),
                current: adapter_version.to_string(),
            });
        }
        // 先扫 journal **自己**的步骤:调用方就算只列了一个舞台,夹在中间的
        // 未 reconcile/Running/Blocked 步骤照样必须挡住续跑。安全逻辑不能依赖
        // 调用方把舞台列全。
        for step in &self.steps {
            match step.status {
                StepStatus::Running | StepStatus::Unknown => {
                    return Err(ResumeRefusal::UnreconciledUnknownStep {
                        stage: step.stage.clone(),
                    })
                }
                StepStatus::Blocked => {
                    return Err(ResumeRefusal::BlockedStep {
                        stage: step.stage.clone(),
                        attempts: step.attempts,
                    })
                }
                _ => {}
            }
            if step.status == StepStatus::Failed && step.attempts >= max_attempts.max(1) {
                return Err(ResumeRefusal::BlockedStep {
                    stage: step.stage.clone(),
                    attempts: step.attempts,
                });
            }
        }
        for (stage, input_hash) in step_inputs {
            let Some(step) = self.step(stage) else {
                continue;
            };
            if step.input_hash != *input_hash {
                return Err(ResumeRefusal::StepInputChanged {
                    stage: stage.to_string(),
                    journal: step.input_hash.clone(),
                    current: input_hash.to_string(),
                });
            }
        }
        Ok(())
    }

    /// 需要重新执行的步骤:没到终态、没被阻塞的。
    pub fn resumable_steps(&self) -> Vec<&StepRecord> {
        self.steps
            .iter()
            .filter(|step| !step.status.is_terminal() && step.status != StepStatus::Failed)
            .collect()
    }

    /// 已经完成过的**读**步骤可以重放;有副作用的步骤一律不重放。
    pub fn replayable_steps(&self) -> Vec<&StepRecord> {
        self.steps
            .iter()
            .filter(|step| step.status == StepStatus::Done && !step.side_effect)
            .collect()
    }
}

// ── 有界重试 ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryPolicy {
    /// 有副作用:不自动重试。
    Never,
    /// 幂等的只读步骤:可以重试,但仍有上限。
    Bounded { max_attempts: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum RetryDecision {
    /// 可以重试,`attempt` 是本次的序号(从 1 起)。
    Retry { attempt: u32 },
    /// 结果不明,先 reconcile 再决定。
    ReconcileFirst { stage: String },
    /// 到顶了。这是终态,不是「再试试」。
    Blocked { stage: String, attempts: u32 },
}

/// 重试判定。副作用步骤永远先 reconcile;`max_attempts == 0` 按 1 处理
/// (配置写 0 是错误,但不该被解释成无限重试)。
pub fn decide_retry(
    policy: RetryPolicy,
    side_effect: bool,
    attempts: u32,
    outcome_known: bool,
) -> RetryDecision {
    if side_effect {
        if !outcome_known {
            return RetryDecision::ReconcileFirst {
                stage: String::new(),
            };
        }
        return RetryDecision::Blocked {
            stage: String::new(),
            attempts,
        };
    }
    let max = match policy {
        RetryPolicy::Never => 0,
        RetryPolicy::Bounded { max_attempts } => max_attempts.max(1),
    };
    if max == 0 || attempts >= max {
        RetryDecision::Blocked {
            stage: String::new(),
            attempts,
        }
    } else {
        RetryDecision::Retry {
            attempt: attempts + 1,
        }
    }
}

/// 带舞台名的重试判定。宿主调用这个;`decide_retry` 保留给纯策略测试。
pub fn decide_stage_retry(
    stage: &str,
    policy: RetryPolicy,
    side_effect: bool,
    attempts: u32,
    outcome_known: bool,
) -> RetryDecision {
    match decide_retry(policy, side_effect, attempts, outcome_known) {
        RetryDecision::ReconcileFirst { .. } => RetryDecision::ReconcileFirst {
            stage: stage.to_string(),
        },
        RetryDecision::Blocked { attempts, .. } => RetryDecision::Blocked {
            stage: stage.to_string(),
            attempts,
        },
        other => other,
    }
}

// ── Dream / Distill ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceKind {
    /// 复盘/整理(读一段历史,产出记忆候选)。
    Dream,
    /// 蒸馏(把长会话压成记忆)。
    Distill,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMaintenanceConfig {
    pub kind: MaintenanceKind,
    /// 默认 false:不开启就不跑。
    pub enabled: bool,
    /// 会读的范围(展示给用户:读了哪些会话/项目)。
    pub read_scope: Vec<String>,
    /// 会写的范围(staging 目录;review 通过后才落到正式记忆)。
    pub write_scope: Vec<String>,
    /// 是否要求用户先 review 才启用产物。默认 true,且不可被 silently 关掉。
    pub requires_review: bool,
    /// 预估调用成本(展示给用户)。`None` = 未知,UI 必须显示未知而不是 0。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
}

impl Default for MemoryMaintenanceConfig {
    fn default() -> Self {
        Self {
            kind: MaintenanceKind::Dream,
            enabled: false,
            read_scope: Vec::new(),
            write_scope: Vec::new(),
            requires_review: true,
            estimated_cost_usd: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceArtifactState {
    /// 生成物躺在 staging,等用户看。
    Staged,
    /// 用户通过,已并入正式记忆。
    Accepted,
    /// 用户丢弃。
    Discarded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceArtifact {
    pub artifact_id: String,
    pub kind: MaintenanceKind,
    pub state: MaintenanceArtifactState,
    pub summary: String,
    pub source_sessions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum MaintenanceError {
    /// 没开启就调用:拒绝,不做「顺手跑一次」。
    Disabled { kind: MaintenanceKind },
    /// 未 review 通过就想生效。
    ReviewRequired { artifact_id: String },
    /// 未知产物。
    UnknownArtifact { artifact_id: String },
}

#[derive(Clone, Debug, Default)]
pub struct MaintenanceStore {
    artifacts: Vec<MaintenanceArtifact>,
}

impl MaintenanceStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 只能入 staging。产物**永远**先到 staging,没有直达正式记忆的入口。
    pub fn stage(
        &mut self,
        config: &MemoryMaintenanceConfig,
        artifact: MaintenanceArtifact,
    ) -> Result<&MaintenanceArtifact, MaintenanceError> {
        if !config.enabled {
            return Err(MaintenanceError::Disabled { kind: config.kind });
        }
        self.artifacts.push(MaintenanceArtifact {
            state: MaintenanceArtifactState::Staged,
            ..artifact
        });
        Ok(self.artifacts.last().expect("just pushed"))
    }

    /// 用户 review 通过 → 生效。
    pub fn accept(&mut self, artifact_id: &str) -> Result<&MaintenanceArtifact, MaintenanceError> {
        let Some(artifact) = self
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.artifact_id == artifact_id)
        else {
            return Err(MaintenanceError::UnknownArtifact {
                artifact_id: artifact_id.to_string(),
            });
        };
        if artifact.state != MaintenanceArtifactState::Staged {
            return Err(MaintenanceError::ReviewRequired {
                artifact_id: artifact_id.to_string(),
            });
        }
        artifact.state = MaintenanceArtifactState::Accepted;
        Ok(self
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_id == artifact_id)
            .expect("artifact exists"))
    }

    pub fn discard(&mut self, artifact_id: &str) -> Result<(), MaintenanceError> {
        let Some(artifact) = self
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.artifact_id == artifact_id)
        else {
            return Err(MaintenanceError::UnknownArtifact {
                artifact_id: artifact_id.to_string(),
            });
        };
        artifact.state = MaintenanceArtifactState::Discarded;
        Ok(())
    }

    pub fn staged(&self) -> Vec<&MaintenanceArtifact> {
        self.artifacts
            .iter()
            .filter(|artifact| artifact.state == MaintenanceArtifactState::Staged)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_script_refuses_to_resume_the_old_journal() {
        let mut journal = WorkflowJournal::new("wf", "hash-a", "1.2.0");
        journal.record_step("build", "in-1", false, StepStatus::Done);
        journal.record_step("push", "in-2", true, StepStatus::Done);

        assert!(journal.can_resume("hash-a", "1.2.0", &[], 3).is_ok());

        let refusal = journal.can_resume("hash-b", "1.2.0", &[], 3).unwrap_err();
        assert_eq!(
            refusal,
            ResumeRefusal::ScriptChanged {
                journal: "hash-a".to_string(),
                current: "hash-b".to_string(),
            }
        );
        // 拒绝不等于死路:可以明确新建 journal。
        let fresh = WorkflowJournal::new("wf", "hash-b", "1.2.0");
        assert!(fresh.can_resume("hash-b", "1.2.0", &[], 3).is_ok());
    }

    #[test]
    fn a_changed_adapter_version_refuses_to_resume() {
        let journal = WorkflowJournal::new("wf", "h", "1.2.0");
        assert_eq!(
            journal.can_resume("h", "1.3.0", &[], 3).unwrap_err(),
            ResumeRefusal::AdapterVersionChanged {
                journal: "1.2.0".to_string(),
                current: "1.3.0".to_string(),
            }
        );
    }

    #[test]
    fn changed_step_inputs_and_unreconciled_steps_refuse_resume() {
        let mut journal = WorkflowJournal::new("wf", "h", "v");
        journal.record_step("build", "in-1", false, StepStatus::Done);
        assert_eq!(
            journal
                .can_resume("h", "v", &[("build", "in-2")], 3)
                .unwrap_err(),
            ResumeRefusal::StepInputChanged {
                stage: "build".to_string(),
                journal: "in-1".to_string(),
                current: "in-2".to_string(),
            }
        );

        // 调用方**没有**列出 deploy,只列了 build:未 reconcile 的步骤照样挡住续跑。
        journal.record_step("deploy", "in-3", true, StepStatus::Unknown);
        assert_eq!(
            journal
                .can_resume("h", "v", &[("build", "in-1")], 3)
                .unwrap_err(),
            ResumeRefusal::UnreconciledUnknownStep {
                stage: "deploy".to_string(),
            }
        );
        // reconcile 之后 + 输入未变 → 可以续跑。
        journal.record_step("deploy", "in-3", true, StepStatus::Failed);
        journal.reconciled = Some("not pushed".to_string());
        assert!(journal
            .can_resume("h", "v", &[("build", "in-1")], 3)
            .is_ok());

        // 重试到顶的步骤同样是 terminal-ish:挡住。
        journal.record_step("deploy", "in-3", true, StepStatus::Failed);
        if let Some(step) = journal.steps.iter_mut().find(|step| step.stage == "deploy") {
            step.attempts = 3;
        }
        assert_eq!(
            journal.can_resume("h", "v", &[], 3).unwrap_err(),
            ResumeRefusal::BlockedStep {
                stage: "deploy".to_string(),
                attempts: 3,
            }
        );
    }

    #[test]
    fn read_steps_replay_but_side_effects_do_not() {
        let mut journal = WorkflowJournal::new("wf", "h", "v");
        journal.record_step("lint", "i1", false, StepStatus::Done);
        journal.record_step("commit", "i2", true, StepStatus::Done);
        journal.record_step("test", "i3", false, StepStatus::Pending);

        let replayable: Vec<_> = journal
            .replayable_steps()
            .into_iter()
            .map(|step| step.stage.as_str())
            .collect();
        assert_eq!(replayable, vec!["lint"], "commit must never be replayed");

        let resumable: Vec<_> = journal
            .resumable_steps()
            .into_iter()
            .map(|step| step.stage.as_str())
            .collect();
        assert_eq!(resumable, vec!["test"]);
    }

    #[test]
    fn recording_the_same_stage_twice_is_idempotent() {
        let mut journal = WorkflowJournal::new("wf", "h", "v");
        journal.record_step("build", "i1", false, StepStatus::Running);
        journal.record_step("build", "i1", false, StepStatus::Done);
        assert_eq!(journal.steps.len(), 1);
        assert_eq!(journal.step("build").unwrap().status, StepStatus::Done);
    }

    #[test]
    fn side_effect_steps_are_never_auto_retried() {
        // 结果不明 → 先 reconcile,绝不直接重跑。
        assert_eq!(
            decide_stage_retry("push", RetryPolicy::Never, true, 0, false),
            RetryDecision::ReconcileFirst {
                stage: "push".to_string()
            }
        );
        // 已知失败 → 直接 blocked,不重试。
        assert_eq!(
            decide_stage_retry("push", RetryPolicy::Never, true, 0, true),
            RetryDecision::Blocked {
                stage: "push".to_string(),
                attempts: 0
            }
        );
    }

    #[test]
    fn read_steps_retry_up_to_the_bound_and_then_block() {
        assert_eq!(
            decide_stage_retry(
                "test",
                RetryPolicy::Bounded { max_attempts: 3 },
                false,
                0,
                true
            ),
            RetryDecision::Retry { attempt: 1 }
        );
        assert_eq!(
            decide_stage_retry(
                "test",
                RetryPolicy::Bounded { max_attempts: 3 },
                false,
                2,
                true
            ),
            RetryDecision::Retry { attempt: 3 }
        );
        assert_eq!(
            decide_stage_retry(
                "test",
                RetryPolicy::Bounded { max_attempts: 3 },
                false,
                3,
                true
            ),
            RetryDecision::Blocked {
                stage: "test".to_string(),
                attempts: 3
            }
        );
        // max_attempts: 0 是配置错误,按 1 处理,不是无限重试。
        assert_eq!(
            decide_stage_retry(
                "test",
                RetryPolicy::Bounded { max_attempts: 0 },
                false,
                0,
                true
            ),
            RetryDecision::Retry { attempt: 1 }
        );
        assert_eq!(
            decide_stage_retry(
                "test",
                RetryPolicy::Bounded { max_attempts: 0 },
                false,
                1,
                true
            ),
            RetryDecision::Blocked {
                stage: "test".to_string(),
                attempts: 1
            }
        );
    }

    #[test]
    fn dream_is_off_by_default_and_artifacts_land_in_staging() {
        let config = MemoryMaintenanceConfig::default();
        assert!(!config.enabled);
        assert!(config.requires_review);
        assert_eq!(config.kind, MaintenanceKind::Dream);

        let mut store = MaintenanceStore::new();
        let artifact = MaintenanceArtifact {
            artifact_id: "a1".to_string(),
            kind: MaintenanceKind::Dream,
            state: MaintenanceArtifactState::Staged,
            summary: "prefers explicit retries".to_string(),
            source_sessions: vec!["s1".to_string()],
        };
        // 没开启:拒绝,不做「顺手跑一次」。
        assert_eq!(
            store.stage(&config, artifact.clone()).unwrap_err(),
            MaintenanceError::Disabled {
                kind: MaintenanceKind::Dream
            }
        );
        assert!(store.is_empty());

        let enabled = MemoryMaintenanceConfig {
            enabled: true,
            ..config
        };
        store.stage(&enabled, artifact).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.staged().len(), 1);

        // review 通过才生效;重复 accept 是错误而不是静默成功。
        assert_eq!(
            store.accept("a1").unwrap().state,
            MaintenanceArtifactState::Accepted
        );
        assert_eq!(
            store.accept("a1").unwrap_err(),
            MaintenanceError::ReviewRequired {
                artifact_id: "a1".to_string()
            }
        );
    }
}
