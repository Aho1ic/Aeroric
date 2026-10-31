//! B4-2 delegation (2026-10-31), the host's side: the `delegation.*` methods
//! the app calls, the `context.delegate*` methods the context server calls
//! for an agent, and starting a child as a task marked `delegateOf` that runs
//! in the parent's working directory. The state machine is core's
//! (`aeroric_core::sessions::live::delegation`).

use std::sync::{Arc, Weak};

use aeroric_core::sessions::live::delegation::{
    Child, DelegationError, DelegationHost, ParentInfo,
};
use aeroric_core::sessions::live::SessionDriver;
use aeroric_core::storage::Record;
use aeroric_core::tasks::launch_plan::working_directory;
use aeroric_core::tasks::session_owner::launch_family_of;
use aeroric_core::tasks::{NewTask, ProtocolFamily};
use aeroric_protocol::checkpoint::Acceptance;
use aeroric_protocol::delegation::{
    self as wire, AgentChoice, Budget, ContextMode, Contract, DelegationCard, Origin, Role,
    Workspace,
};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::session_live::TaskParams;
use serde_json::{json, Value};

use super::review::{REVIEWERS, REVIEW_OF_FIELD};
use super::{blocking, context_scope, failure, now_ms, ok, params, Live, BRANCH_OF_FIELD};
use crate::dispatcher::ServiceSet;

/// The record field naming a child's parent task.
pub(super) const DELEGATE_OF_FIELD: &str = "delegateOf";

pub const CONTEXT_DELEGATE_METHOD: &str = "context.delegate";
pub const CONTEXT_DELEGATIONS_METHOD: &str = "context.delegations";
pub const CONTEXT_ROLES_METHOD: &str = "context.roles";

pub(super) struct HostDelegations {
    live: Weak<Live>,
    probe_installed: bool,
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>, probe_installed: bool) {
    let delegations = live.executor.delegations();
    delegations.set_dir(live.context.paths.data_dir().join("delegations"));
    delegations.set_host(Arc::new(HostDelegations {
        live: Arc::downgrade(live),
        probe_installed,
    }));
    set.register(wire::START_METHOD, blocking(live, delegation_start));
    set.register(wire::LIST_METHOD, blocking(live, delegation_list));
    set.register(wire::STOP_METHOD, blocking(live, delegation_stop));
    set.register(wire::SEND_METHOD, blocking(live, delegation_send));
    set.register(wire::REDO_METHOD, blocking(live, delegation_redo));
    set.register(wire::ROLES_METHOD, blocking(live, delegation_roles));
    set.register(wire::SAVE_ROLE_METHOD, blocking(live, delegation_save_role));
    set.register(
        wire::DELETE_ROLE_METHOD,
        blocking(live, delegation_delete_role),
    );
    set.register(wire::AUDIT_METHOD, blocking(live, delegation_audit));
    set.register(wire::STATS_METHOD, blocking(live, delegation_stats));
    set.register(wire::ATTENTION_METHOD, blocking(live, delegation_attention));
    set.register(
        wire::ACK_PROBES_METHOD,
        blocking(live, delegation_ack_probes),
    );
    set.register(CONTEXT_DELEGATE_METHOD, blocking(live, context_delegate));
    set.register(
        CONTEXT_DELEGATIONS_METHOD,
        blocking(live, context_delegations),
    );
    set.register(CONTEXT_ROLES_METHOD, blocking(live, context_roles));
}

/// A child, reviewer or side branch: never a parent (depth 1).
pub(super) fn nested(record: &Record) -> bool {
    [DELEGATE_OF_FIELD, REVIEW_OF_FIELD, BRANCH_OF_FIELD]
        .iter()
        .any(|k| record.get(k).is_some())
}

fn title_of(record: &Record) -> String {
    let agent = record.get("agent").and_then(Value::as_str).unwrap_or("");
    let title = ["name", "title"]
        .iter()
        .find_map(|key| {
            record
                .get(key)
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty() && *n != agent)
                .map(str::to_string)
        })
        .or_else(|| {
            record
                .get("prompt")
                .and_then(Value::as_str)
                .and_then(|p| p.lines().next())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "会话".into());
    title.chars().take(40).collect()
}

fn field(record: &Record, key: &str) -> Option<String> {
    record
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

impl HostDelegations {
    /// (agent, family, label, model, agent config) for `choice`.
    #[allow(clippy::type_complexity)]
    fn pick(
        &self,
        live: &Live,
        project: &Record,
        record: &Record,
        choice: &AgentChoice,
    ) -> Result<
        (
            &'static str,
            ProtocolFamily,
            &'static str,
            Option<String>,
            Option<String>,
        ),
        String,
    > {
        let writer = launch_family_of(record, live.families.as_ref());
        let cwd = working_directory(record, project).ok_or("主会话没有工作目录")?;
        let candidates: Vec<_> = REVIEWERS
            .iter()
            .filter(|(agent, family, _)| live.executor.takes_task(*family, agent, None, &cwd))
            .filter(|(agent, _, _)| {
                !self.probe_installed
                    || aeroric_core::settings::installed_agent_version(agent).is_some()
            })
            .copied()
            .collect();
        let none = || "本机没有可以接委派的 Claude Code 或 Codex".to_string();
        match choice {
            AgentChoice::Same => {
                let pick = candidates
                    .iter()
                    .find(|c| c.1 == writer)
                    .or(candidates.first())
                    .ok_or_else(none)?;
                // The parent's own model and source when it is the same harness.
                let (model, config) = if pick.1 == writer {
                    (
                        field(record, "selectedModel"),
                        field(record, "agentConfigId"),
                    )
                } else {
                    (None, None)
                };
                Ok((pick.0, pick.1, pick.2, model, config))
            }
            AgentChoice::Other => {
                let pick = candidates
                    .iter()
                    .find(|c| c.1 != writer)
                    .or(candidates.first())
                    .ok_or_else(none)?;
                Ok((pick.0, pick.1, pick.2, None, None))
            }
            AgentChoice::Named {
                agent,
                agent_config_id,
                model,
            } => {
                let pick = candidates
                    .iter()
                    .find(|c| c.0 == agent)
                    .ok_or_else(|| format!("{agent} 在本机不可用"))?;
                Ok((
                    pick.0,
                    pick.1,
                    pick.2,
                    model.clone(),
                    agent_config_id.clone(),
                ))
            }
        }
    }

    fn mark(
        live: &Live,
        project_id: &str,
        child: &str,
        parent: &Record,
        permission: &str,
    ) -> Result<(), String> {
        let parent_id = field(parent, "id").unwrap_or_default();
        // Same working directory as the parent (its worktree included).
        let worktree: Vec<(String, Value)> = parent
            .0
            .iter()
            .filter(|(key, _)| key.starts_with("worktree"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let child = child.to_string();
        let permission = permission.to_string();
        live.service
            .mutate(project_id, |records, _now| {
                let next = records
                    .iter()
                    .map(|r| {
                        let mut r = r.clone();
                        if r.get("id").and_then(Value::as_str) == Some(child.as_str()) {
                            r.0.insert(DELEGATE_OF_FIELD.into(), Value::String(parent_id.clone()));
                            r.0.insert("permissionMode".into(), Value::String(permission.clone()));
                            for (key, value) in &worktree {
                                r.0.insert(key.clone(), value.clone());
                            }
                        }
                        r
                    })
                    .collect();
                (next, true)
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

impl DelegationHost for HostDelegations {
    fn parent(&self, task_id: &str) -> Option<ParentInfo> {
        let live = self.live.upgrade()?;
        let (_, record) = live.record(task_id)?;
        Some(ParentInfo {
            permission: field(&record, "permissionMode").unwrap_or_else(|| "ask".into()),
            nested: nested(&record),
        })
    }

    fn start_child(&self, card: &DelegationCard, prompt: String) -> Result<Child, String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        let parent = &card.parent_task_id;
        let (project, record) = live.record(parent).ok_or("主会话不存在")?;
        let project_id = field(&project, "id").ok_or("会话的项目没有 id")?;
        let (agent, family, label, model, config) =
            self.pick(&live, &project, &record, &card.contract.agent)?;
        let suffix = match &card.role_name {
            Some(role) => format!("委派·{role}"),
            None => "委派".into(),
        };
        let writer = launch_family_of(&record, live.families.as_ref());
        // Core picked the id, so it hears the child's first words.
        let id = card
            .child_task_id
            .clone()
            .unwrap_or_else(|| format!("task-{}", uuid::Uuid::new_v4().simple()));
        if card.contract.context_mode == ContextMode::Fork {
            if family != writer {
                return Err("续做（fork）只能交给与主会话同一种 Harness".into());
            }
            crate::workspace_meta_services::fork_task_as(
                &live.context,
                &live.service,
                parent,
                &suffix,
                &id,
            )
            .map_err(|e| match e {
                crate::workspace_meta_services::ForkError::NotFound(m) => m.to_string(),
                crate::workspace_meta_services::ForkError::Invalid(m) => m,
            })?;
            Self::mark(&live, &project_id, &id, &record, &card.contract.permission)?;
            let driver: Arc<dyn SessionDriver> = live.reopen_quiet(&id)?;
            driver.prompt(prompt).map_err(|e| e.to_string())?;
        } else {
            live.service
                .create(
                    &project,
                    NewTask {
                        id: id.clone(),
                        project_id: project_id.clone(),
                        prompt,
                        agent: agent.to_string(),
                        permission_mode: card.contract.permission.clone(),
                        immediate: false,
                        selected_model: model.clone(),
                        reasoning_effort: None,
                        speed: None,
                        dsh_agent_preset: None,
                        agent_config_id: config,
                        images: Vec::new(),
                        texts: Vec::new(),
                    },
                )
                .map_err(|e| e.to_string())?;
            Self::mark(&live, &project_id, &id, &record, &card.contract.permission)?;
            live.service
                .rename(
                    &project_id,
                    &id,
                    &format!("{}（{suffix}）", title_of(&record)),
                )
                .map_err(|e| e.to_string())?;
            let (project, _) = live.record(&id).ok_or("子会话没有建成")?;
            live.service
                .start(
                    &project,
                    &id,
                    &format!("delegation-start-{id}-{}", now_ms()),
                    true,
                    None,
                    false,
                    Default::default(),
                    Default::default(),
                )
                .map_err(|e| e.to_string())?;
        }
        let model = if card.contract.context_mode == ContextMode::Fork {
            field(&record, "selectedModel")
        } else {
            model
        };
        Ok(Child {
            task_id: id,
            label: match &model {
                Some(model) => format!("{label} · {model}"),
                None => label.to_string(),
            },
            agent: agent.to_string(),
            model,
        })
    }

    fn prompt(&self, task_id: &str, text: String) -> Result<(), String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        let driver: Arc<dyn SessionDriver> = match live.driver(task_id) {
            Some(driver) => driver,
            None => live.reopen_quiet(task_id)?,
        };
        driver.prompt(text).map_err(|e| e.to_string())
    }

    fn interrupt(&self, task_id: &str) {
        if let Some(driver) = self.live.upgrade().and_then(|l| l.driver(task_id)) {
            let _ = driver.interrupt();
        }
    }

    fn idle(&self, task_id: &str) -> bool {
        self.live
            .upgrade()
            .and_then(|l| l.driver(task_id))
            .is_none_or(|d| !d.snapshot().turn_active)
    }

    fn tokens(&self, task_id: &str) -> Option<u64> {
        let state = self.live.upgrade()?.driver(task_id)?.snapshot();
        match (state.input_tokens, state.output_tokens) {
            (None, None) => None,
            (i, o) => Some(i.unwrap_or(0) + o.unwrap_or(0)),
        }
    }
}

fn delegation_failure(request: &Request, error: DelegationError) -> Frame {
    let code = match error {
        DelegationError::NoParent | DelegationError::NotFound => ErrorCode::NotFound,
        _ => ErrorCode::InvalidArgument,
    };
    failure(request.id.clone(), code, &error.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartParams {
    task_id: String,
    contract: Contract,
}

/// `delegation.start {taskId, contract}`: the user delegates from the app.
fn delegation_start(live: &Live, request: &Request) -> Frame {
    let p: StartParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live
        .executor
        .delegations()
        .start(&p.task_id, p.contract, Origin::User, "user")
    {
        Ok(card) => ok(request, json!({ "delegation": card })),
        Err(error) => delegation_failure(request, error),
    }
}

/// `delegation.list {taskId}`: the task's delegations, and its parent when
/// it is a child itself.
fn delegation_list(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((_, record)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    ok(
        request,
        json!({
            "delegations": live.executor.delegations().list(&p.task_id),
            "delegateOf": record.get(DELEGATE_OF_FIELD),
        }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActParams {
    task_id: String,
    delegation_id: String,
    #[serde(default)]
    note: Option<String>,
}

fn delegation_stop(live: &Live, request: &Request) -> Frame {
    let p: ActParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live
        .executor
        .delegations()
        .stop(&p.task_id, &p.delegation_id, "user")
    {
        Ok(()) => ok(request, json!({ "accepted": true })),
        Err(error) => delegation_failure(request, error),
    }
}

fn delegation_send(live: &Live, request: &Request) -> Frame {
    let p: ActParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live
        .executor
        .delegations()
        .send(&p.task_id, &p.delegation_id, "user")
    {
        Ok(()) => ok(request, json!({ "accepted": true })),
        Err(error) => delegation_failure(request, error),
    }
}

fn delegation_redo(live: &Live, request: &Request) -> Frame {
    let p: ActParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.delegations().redo(
        &p.task_id,
        &p.delegation_id,
        p.note.as_deref().unwrap_or(""),
        "user",
    ) {
        Ok(()) => ok(request, json!({ "accepted": true })),
        Err(error) => delegation_failure(request, error),
    }
}

/// `delegation.ackProbes {taskId, delegationId}`: the user saw the probes.
fn delegation_ack_probes(live: &Live, request: &Request) -> Frame {
    let p: ActParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live
        .executor
        .delegations()
        .ack_probes(&p.task_id, &p.delegation_id)
    {
        Ok(()) => ok(request, json!({ "accepted": true })),
        Err(error) => delegation_failure(request, error),
    }
}

fn delegation_roles(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "roles": live.executor.delegations().roles() }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveRoleParams {
    role: Role,
}

fn delegation_save_role(live: &Live, request: &Request) -> Frame {
    let p: SaveRoleParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.delegations().save_role(p.role) {
        Ok(role) => ok(request, json!({ "role": role })),
        Err(message) => failure(request.id.clone(), ErrorCode::InvalidArgument, &message),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RoleIdParams {
    id: String,
}

fn delegation_delete_role(live: &Live, request: &Request) -> Frame {
    let p: RoleIdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.delegations().delete_role(&p.id) {
        Ok(()) => ok(request, json!({})),
        Err(message) => failure(request.id.clone(), ErrorCode::InvalidArgument, &message),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuditParams {
    #[serde(default)]
    task_id: Option<String>,
}

fn delegation_audit(live: &Live, request: &Request) -> Frame {
    let p: AuditParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    ok(
        request,
        json!({ "entries": live.executor.delegations().audit(p.task_id.as_deref()) }),
    )
}

fn delegation_stats(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "rows": live.executor.delegations().stats() }),
    )
}

fn delegation_attention(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "items": live.executor.delegations().attention() }),
    )
}

// MARK: context server

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DelegateArgs {
    token: String,
    instruction: String,
    #[serde(default)]
    role: Option<String>,
    /// "same", "other", or an agent id ("claude", "codex").
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    context_mode: Option<ContextMode>,
    #[serde(default)]
    context_refs: Vec<String>,
    #[serde(default)]
    permission: Option<String>,
    #[serde(default)]
    workspace: Option<Workspace>,
    #[serde(default)]
    acceptance: Vec<Acceptance>,
    #[serde(default)]
    budget_minutes: Option<u32>,
    #[serde(default)]
    budget_tokens: Option<u64>,
}

impl DelegateArgs {
    fn contract(self) -> Contract {
        let agent = match self.agent.as_deref().map(str::trim) {
            None | Some("") => None,
            Some("same") => Some(AgentChoice::Same),
            Some("other") => Some(AgentChoice::Other),
            Some(agent) => Some(AgentChoice::Named {
                agent: agent.to_string(),
                agent_config_id: None,
                model: self.model,
            }),
        };
        let budget =
            (self.budget_minutes.is_some() || self.budget_tokens.is_some()).then_some(Budget {
                minutes: self.budget_minutes,
                tokens: self.budget_tokens,
            });
        Contract {
            instruction: self.instruction,
            role: self.role,
            agent,
            context_mode: self.context_mode,
            context_refs: self.context_refs,
            permission: self.permission,
            workspace: self.workspace,
            acceptance: self.acceptance,
            budget,
        }
    }
}

/// `context.delegate {token, instruction, …}`: the token's session delegates.
fn context_delegate(live: &Live, request: &Request) -> Frame {
    let args: DelegateArgs = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((parent, _)) = live.executor.context_tokens().resolve(&args.token) else {
        return failure(
            request.id.clone(),
            ErrorCode::Unauthenticated,
            "上下文令牌无效或会话已结束",
        );
    };
    let contract = args.contract();
    match live
        .executor
        .delegations()
        .start(&parent, contract, Origin::Session, &parent)
    {
        Ok(card) => {
            let mut text = format!(
                "已委派（{}），子会话正在启动。它和你同时进行的其他委派都结束后，Aeroric 会把结果合并成一条消息发给你；在那之前可以继续做别的，或结束这一回合等待。\n权限：{}；工作区：{}；上下文：{}。",
                card.delegation_id,
                card.contract.permission,
                match card.contract.workspace {
                    Workspace::ReadOnly => "只读",
                    Workspace::Shared => "共享可写",
                },
                match card.contract.context_mode {
                    ContextMode::Fork => "续做（带你的对话）",
                    ContextMode::Brief => "交接清单",
                    ContextMode::Isolated => "隔离",
                },
            );
            if card.contract.acceptance.is_empty() {
                text.push_str("\n注意：这项委派没有验收条款，结果无法核对；建议继续拆分，或给出可执行的验收条款。");
            }
            ok(request, json!({ "text": text, "delegation": card }))
        }
        Err(error) => failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &error.to_string(),
        ),
    }
}

/// `context.delegations {token}`: the token's session's delegations.
fn context_delegations(live: &Live, request: &Request) -> Frame {
    let (_, task_id, _) = match context_scope(live, request) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    ok(
        request,
        json!({ "delegations": live.executor.delegations().list(&task_id) }),
    )
}

/// `context.roles {token}`: the role library.
fn context_roles(live: &Live, request: &Request) -> Frame {
    if let Err(frame) = context_scope(live, request) {
        return *frame;
    }
    let roles: Vec<Value> = live
        .executor
        .delegations()
        .roles()
        .into_iter()
        .map(|r| {
            json!({"id": r.id, "name": r.name, "description": r.description,
                "contextMode": r.context_mode, "permission": r.permission, "workspace": r.workspace})
        })
        .collect();
    ok(request, json!({ "roles": roles }))
}
