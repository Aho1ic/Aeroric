//! 2026-11-13 Smart Friend (frontier-roadmap §5.2 form B), the host's side.
//! A session's agent asks a stronger model through `ask_expert`: the
//! context server calls `context.askExpert`, which starts a read-only
//! delegation of the built-in expert role on the model set in 设置 →
//! Agents, then polls `context.expertAnswer` until the answer is in. The
//! app reads and sets the expert with `delegation.expert`.

use std::sync::Arc;

use aeroric_core::sessions::live::delegation::{prompt, ExpertAnswer};
use aeroric_core::settings::session_catalog;
use aeroric_protocol::delegation::{
    AgentChoice, Budget, ContextMode, Contract, ExpertChoice, Origin, Workspace,
    EXPERT_ASKS_PER_TURN, EXPERT_MINUTES, EXPERT_ROLE,
};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use super::review::REVIEWERS;
use super::{blocking, context_scope, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const EXPERT_METHOD: &str = "delegation.expert";
pub const CONTEXT_ASK_METHOD: &str = "context.askExpert";
pub const CONTEXT_ANSWER_METHOD: &str = "context.expertAnswer";
pub const CONTEXT_AVAILABLE_METHOD: &str = "context.expertAvailable";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(EXPERT_METHOD, blocking(live, expert));
    set.register(CONTEXT_ASK_METHOD, blocking(live, ask));
    set.register(CONTEXT_ANSWER_METHOD, blocking(live, answer));
    set.register(CONTEXT_AVAILABLE_METHOD, blocking(live, available));
}

/// The configurations an expert can run on: Claude Code or Codex ones that
/// can start now (delegation only starts those two).
fn choices(live: &Live) -> Result<Vec<Value>, String> {
    let fields = aeroric_core::storage::load_settings(&live.context)
        .map_err(|_| "Agent 设置读取失败".to_string())?
        .found()
        .unwrap_or_default();
    let sources = session_catalog::AgentSources::load(&live.context)?;
    // Not tied to a project: the expert is one for every session.
    let catalog =
        session_catalog::catalog_with(&fields, &sources, "", "", live.context.clock.now_ms())?;
    Ok(catalog
        .configurations
        .into_iter()
        .filter(|c| c.blocked_reasons.is_empty())
        .filter(|c| REVIEWERS.iter().any(|(agent, _, _)| *agent == c.agent))
        .map(|c| {
            json!({"configurationId": c.id, "title": c.display_name, "agent": c.agent,
                "models": c.models})
        })
        .collect())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetParams {
    configuration_id: String,
    #[serde(default)]
    model: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExpertParams {
    /// Set the expert to this configuration and model.
    #[serde(default)]
    set: Option<SetParams>,
    /// Clear it: `ask_expert` is no longer offered.
    #[serde(default)]
    clear: bool,
}

/// `delegation.expert {set?, clear?}` → `{expert, choices}`.
fn expert(live: &Live, request: &Request) -> Frame {
    let p: ExpertParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let delegations = live.executor.delegations();
    let choices = match choices(live) {
        Ok(choices) => choices,
        Err(reason) => return failure(request.id.clone(), ErrorCode::IoError, &reason),
    };
    if p.clear {
        if let Err(reason) = delegations.set_expert(None) {
            return failure(request.id.clone(), ErrorCode::IoError, &reason);
        }
    } else if let Some(set) = p.set {
        let Some(choice) = choices
            .iter()
            .find(|c| c["configurationId"] == set.configuration_id.as_str())
        else {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "这个配置现在不能用作专家（不存在、不可用，或不是 Claude Code / Codex）",
            );
        };
        let models: Vec<&str> = choice["models"]
            .as_array()
            .map(|m| m.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if let Some(model) = &set.model {
            if !models.is_empty() && !models.contains(&model.as_str()) {
                return failure(
                    request.id.clone(),
                    ErrorCode::InvalidArgument,
                    &format!("这个配置没有模型 {model}"),
                );
            }
        }
        let title = choice["title"].as_str().unwrap_or_default();
        let label = match &set.model {
            Some(model) => format!("{title} · {model}"),
            None => title.to_string(),
        };
        let saved = ExpertChoice {
            configuration_id: set.configuration_id,
            agent: choice["agent"].as_str().unwrap_or_default().to_string(),
            model: set.model,
            label,
        };
        if let Err(reason) = delegations.set_expert(Some(saved)) {
            return failure(request.id.clone(), ErrorCode::IoError, &reason);
        }
    }
    ok(
        request,
        json!({ "expert": delegations.expert(), "choices": choices }),
    )
}

/// `context.expertAvailable {token}`: whether `ask_expert` is offered.
fn available(live: &Live, request: &Request) -> Frame {
    if let Err(frame) = context_scope(live, request) {
        return *frame;
    }
    let available = live.executor.delegations().expert().is_some();
    ok(request, json!({ "available": available }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AskParams {
    token: String,
    question: String,
}

/// `context.askExpert {token, question}` → `{delegationId, label}`.
fn ask(live: &Live, request: &Request) -> Frame {
    let p: AskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let fail = |code, text: &str| failure(request.id.clone(), code, text);
    let Some((parent, _)) = live.executor.context_tokens().resolve(&p.token) else {
        return fail(ErrorCode::Unauthenticated, "上下文令牌无效或会话已结束");
    };
    if p.question.trim().is_empty() {
        return fail(ErrorCode::InvalidArgument, "问题不能为空");
    }
    let delegations = live.executor.delegations();
    let Some(expert) = delegations.expert() else {
        return fail(
            ErrorCode::InvalidArgument,
            "用户还没有设置专家模型（设置 → Agents → 求助专家）",
        );
    };
    if delegations.expert_asks_this_turn(&parent) >= EXPERT_ASKS_PER_TURN {
        return fail(
            ErrorCode::InvalidArgument,
            &format!("这一回合已经问过 {EXPERT_ASKS_PER_TURN} 次专家；先用已有的回答推进，或在下一回合再问"),
        );
    }
    let contract = Contract {
        instruction: prompt::expert_question(&p.question),
        role: Some(EXPERT_ROLE.into()),
        agent: Some(AgentChoice::Named {
            agent: expert.agent.clone(),
            agent_config_id: aeroric_core::agent_connections::launch::config_id_from_catalog(
                &expert.configuration_id,
            )
            .map(str::to_string),
            model: expert.model.clone(),
        }),
        context_mode: Some(ContextMode::Brief),
        context_refs: Vec::new(),
        permission: Some("plan".into()),
        workspace: Some(Workspace::ReadOnly),
        acceptance: Vec::new(),
        budget: Some(Budget {
            minutes: Some(EXPERT_MINUTES),
            tokens: None,
        }),
    };
    match delegations.start(&parent, contract, Origin::Session, &parent) {
        Ok(card) => ok(
            request,
            json!({ "delegationId": card.delegation_id, "label": expert.label }),
        ),
        Err(error) => fail(ErrorCode::InvalidArgument, &error.to_string()),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnswerParams {
    token: String,
    delegation_id: String,
}

/// `context.expertAnswer {token, delegationId}` → `{state, text?, label?}`;
/// state is pending, answered or failed.
fn answer(live: &Live, request: &Request) -> Frame {
    let p: AnswerParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((parent, _)) = live.executor.context_tokens().resolve(&p.token) else {
        return failure(
            request.id.clone(),
            ErrorCode::Unauthenticated,
            "上下文令牌无效或会话已结束",
        );
    };
    match live
        .executor
        .delegations()
        .take_expert_answer(&parent, &p.delegation_id)
    {
        Ok(ExpertAnswer::Pending) => ok(request, json!({ "state": "pending" })),
        Ok(ExpertAnswer::Answered { text, label }) => ok(
            request,
            json!({ "state": "answered", "text": text, "label": label }),
        ),
        Ok(ExpertAnswer::Failed(reason)) => {
            ok(request, json!({ "state": "failed", "text": reason }))
        }
        Err(error) => failure(request.id.clone(), ErrorCode::NotFound, &error.to_string()),
    }
}
