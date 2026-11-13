//! 2026-11-12 discussion (round table), the host's side: the discussion's
//! own task record (`discussion: true`, never started), its members as
//! read-only sessions marked `discussionOf`, and the `discussion.*` methods
//! the app calls. The rounds are core's
//! (`aeroric_core::sessions::live::discussion`).

use std::sync::{Arc, Weak};

use aeroric_core::projects::ProjectService;
use aeroric_core::sessions::live::discussion::{
    self as core, Discussion, DiscussionError, DiscussionHost, Member, DEFAULT_ROUNDS,
};
use aeroric_core::sessions::live::Decision;
use aeroric_core::sessions::transcript_stats;
use aeroric_core::storage::Record;
use aeroric_core::tasks::NewTask;
use aeroric_protocol::delivery::{DeliveryKind, DeliveryMode};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::ProtocolFamily;
use serde::Deserialize;
use serde_json::{json, Value};

use super::delegation::field;
use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const START_METHOD: &str = "discussion.start";
pub const GET_METHOD: &str = "discussion.get";
pub const PAUSE_METHOD: &str = "discussion.pause";
pub const RESUME_METHOD: &str = "discussion.resume";
pub const SUMMARIZE_METHOD: &str = "discussion.summarize";
pub const STOP_METHOD: &str = "discussion.stop";

/// The record field marking a discussion's own task.
pub(super) const DISCUSSION_FIELD: &str = "discussion";
/// The record field naming the discussion a member speaks in.
pub(super) const DISCUSSION_OF_FIELD: &str = "discussionOf";

/// Members only read: every harness's plan or read-only mode.
const READ_ONLY: &str = "plan";

struct HostDiscussions {
    live: Weak<Live>,
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    let discussions = live.executor.discussions();
    discussions.set_host(Arc::new(HostDiscussions {
        live: Arc::downgrade(live),
    }));
    discussions.set_dir(live.context.paths.data_dir().join("discussions"));
    set.register(START_METHOD, blocking(live, start));
    set.register(GET_METHOD, blocking(live, get));
    set.register(PAUSE_METHOD, blocking(live, pause));
    set.register(RESUME_METHOD, blocking(live, resume));
    set.register(SUMMARIZE_METHOD, blocking(live, summarize));
    set.register(STOP_METHOD, blocking(live, stop));
}

/// Set fields on one record of `project_id`.
fn set_fields(live: &Live, project_id: &str, task_id: &str, fields: Vec<(&str, Value)>) {
    let task_id = task_id.to_string();
    let _ = live.service.mutate(project_id, |records, now| {
        let next = records
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if r.get("id").and_then(Value::as_str) == Some(task_id.as_str()) {
                    for (key, value) in &fields {
                        r.0.insert((*key).to_string(), value.clone());
                    }
                    r.0.insert("updatedAt".into(), json!(now));
                }
                r
            })
            .collect();
        (next, true)
    });
}

impl DiscussionHost for HostDiscussions {
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        let (project, record) = live.record(task_id).ok_or("成员会话不存在")?;
        // 目检 2026-11-12: a member whose session never started has nothing
        // for the queue to carry on; it is started again with the message.
        if aeroric_core::tasks::session_owner::current_session(&record)
            .0
            .is_none()
        {
            return live
                .service
                .start(
                    &project,
                    task_id,
                    &format!("discussion-retry-{task_id}-{}", super::now_ms()),
                    true,
                    Some(text),
                    false,
                    Default::default(),
                    Default::default(),
                )
                .map(|_| ())
                .map_err(|e| e.to_string());
        }
        live.executor
            .deliveries()
            .enqueue(
                task_id,
                DeliveryKind::Delegation,
                DeliveryMode::AfterTurn,
                &super::delivery::label("讨论", &text),
                &text,
                None,
            )
            .map(|_| ())
    }

    fn interrupt(&self, task_id: &str) {
        if let Some(driver) = self.live.upgrade().and_then(|l| l.driver(task_id)) {
            let _ = driver.interrupt();
        }
    }

    fn respond(&self, task_id: &str, request_id: &str, decision: Decision) {
        if let Some(driver) = self.live.upgrade().and_then(|l| l.driver(task_id)) {
            let _ = driver.respond(request_id, decision);
        }
    }

    fn changed(&self, discussion: &Discussion) {
        let Some(live) = self.live.upgrade() else {
            return;
        };
        let status = serde_json::to_value(discussion.task_status()).unwrap_or(Value::Null);
        set_fields(
            &live,
            &discussion.project_id,
            &discussion.id,
            vec![("status", status)],
        );
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemberParams {
    configuration_id: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    angle: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartParams {
    project_id: String,
    topic: String,
    members: Vec<MemberParams>,
    #[serde(default)]
    moderator: usize,
    rounds: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskParams {
    task_id: String,
}

fn discussion_failure(request: &Request, error: DiscussionError) -> Frame {
    let code = match error {
        DiscussionError::NotFound => ErrorCode::NotFound,
        DiscussionError::Invalid(_) => ErrorCode::InvalidArgument,
        DiscussionError::Io(_) => ErrorCode::IoError,
    };
    failure(request.id.clone(), code, &error.to_string())
}

/// Labels for the members; repeats get a number.
fn labels(names: &[String]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let before = names[..i].iter().filter(|n| *n == name).count();
            let total = names.iter().filter(|n| *n == name).count();
            if total > 1 {
                format!("{name} #{}", before + 1)
            } else {
                name.clone()
            }
        })
        .collect()
}

fn first_line(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("讨论");
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line.to_string()
    }
}

fn start(live: &Live, request: &Request) -> Frame {
    let p: StartParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let fail = |code, text: &str| failure(request.id.clone(), code, text);
    if !(core::MIN_MEMBERS..=core::MAX_MEMBERS).contains(&p.members.len()) {
        return fail(
            ErrorCode::InvalidArgument,
            &format!(
                "讨论要 {} 到 {} 个成员",
                core::MIN_MEMBERS,
                core::MAX_MEMBERS
            ),
        );
    }
    if p.topic.trim().is_empty() {
        return fail(ErrorCode::InvalidArgument, "题目不能为空");
    }
    let project = match ProjectService::new(Arc::clone(&live.context)).list() {
        Ok(projects) => projects
            .into_iter()
            .find(|r| r.get("id").and_then(Value::as_str) == Some(p.project_id.as_str())),
        Err(error) => return fail(ErrorCode::IoError, &error.to_string()),
    };
    let Some(project) = project else {
        return fail(ErrorCode::NotFound, "项目不存在");
    };
    let catalog =
        match aeroric_core::settings::session_catalog::for_project(&live.context, &p.project_id) {
            Ok((_, catalog)) => catalog,
            Err(reason) => return fail(ErrorCode::InvalidArgument, &reason),
        };
    // Every member is checked before anything is made.
    let mut picked = Vec::new();
    for member in &p.members {
        let Some(configuration) = catalog
            .configurations
            .iter()
            .find(|c| c.id == member.configuration_id)
        else {
            return fail(
                ErrorCode::InvalidArgument,
                &format!("{}：配置不存在", member.configuration_id),
            );
        };
        if let Some(model) = &member.model {
            if !configuration.models.is_empty() && !configuration.models.contains(model) {
                return fail(
                    ErrorCode::InvalidArgument,
                    &format!("{}：不支持模型 {model}", configuration.display_name),
                );
            }
        }
        let name = match &member.model {
            Some(model) => format!("{} · {model}", configuration.display_name),
            None => configuration.display_name.clone(),
        };
        picked.push((configuration.clone(), name));
    }
    let names: Vec<String> = picked.iter().map(|(_, n)| n.clone()).collect();
    let labels = labels(&names);
    let id = format!("task-{}", uuid::Uuid::new_v4().simple());
    let title = first_line(&p.topic, 40);
    let moderator_agent = picked
        .get(p.moderator)
        .map(|(c, _)| c.agent.clone())
        .unwrap_or_default();
    // The discussion's own record: listed in the sidebar, never started.
    if let Err(error) = live.service.create(
        &project,
        NewTask {
            id: id.clone(),
            project_id: p.project_id.clone(),
            prompt: p.topic.clone(),
            agent: moderator_agent,
            permission_mode: READ_ONLY.into(),
            immediate: false,
            selected_model: None,
            reasoning_effort: None,
            speed: None,
            dsh_agent_preset: None,
            agent_config_id: None,
            images: Vec::new(),
            texts: Vec::new(),
        },
    ) {
        return fail(ErrorCode::IoError, &error.to_string());
    }
    set_fields(
        live,
        &p.project_id,
        &id,
        vec![(DISCUSSION_FIELD, Value::Bool(true))],
    );
    let _ = live
        .service
        .rename(&p.project_id, &id, &format!("讨论 · {title}"));
    let members: Vec<Member> = picked
        .iter()
        .zip(&p.members)
        .zip(&labels)
        .map(|(((configuration, _), params), label)| Member {
            task_id: format!("task-{}", uuid::Uuid::new_v4().simple()),
            label: label.clone(),
            configuration_id: configuration.id.clone(),
            agent: configuration.agent.clone(),
            model: params.model.clone(),
            angle: params.angle.clone().filter(|a| !a.trim().is_empty()),
            seen: Vec::new(),
        })
        .collect();
    let discussion = Discussion {
        id: id.clone(),
        project_id: p.project_id.clone(),
        topic: p.topic.trim().to_string(),
        members,
        moderator: p.moderator,
        max_rounds: p.rounds.unwrap_or(DEFAULT_ROUNDS),
        round: 0,
        state: core::DiscussionState::Running,
        posts: Vec::new(),
        note: None,
        created_ms: super::now_ms(),
    };
    let discussions = live.executor.discussions();
    // Registered before the members start, so their first words are heard.
    let discussion = match discussions.begin(discussion) {
        Ok(d) => d,
        Err(error) => {
            let _ = live
                .service
                .delete(&p.project_id, std::slice::from_ref(&id));
            return discussion_failure(request, error);
        }
    };
    // Like a delegated child: made and started directly, because the
    // catalog lists no `plan` for a submission to ask for.
    for (i, params) in p.members.iter().enumerate() {
        if let Err(reason) = start_member(live, &project, &id, &title, &discussion, i, params) {
            discussions.member_failed(&id, i, &format!("没能开始：{reason}"));
        }
    }
    let discussion = discussions.get(&id).unwrap_or(discussion);
    ok(request, json!({ "discussion": view(live, &discussion) }))
}

/// Make member `i`'s session, read-only, marked as the discussion's, and
/// start it with its round-1 message.
fn start_member(
    live: &Live,
    project: &Record,
    discussion_id: &str,
    title: &str,
    discussion: &Discussion,
    i: usize,
    params: &MemberParams,
) -> Result<(), String> {
    let member = &discussion.members[i];
    let project_id = &discussion.project_id;
    live.service
        .create(
            project,
            NewTask {
                id: member.task_id.clone(),
                project_id: project_id.clone(),
                prompt: core::prompt::opening(discussion, i),
                agent: member.agent.clone(),
                permission_mode: READ_ONLY.into(),
                immediate: false,
                selected_model: member.model.clone(),
                reasoning_effort: params.reasoning_effort.clone(),
                speed: None,
                dsh_agent_preset: None,
                agent_config_id: aeroric_core::agent_connections::launch::config_id_from_catalog(
                    &member.configuration_id,
                )
                .map(str::to_string),
                images: Vec::new(),
                texts: Vec::new(),
            },
        )
        .map_err(|e| e.to_string())?;
    set_fields(
        live,
        project_id,
        &member.task_id,
        vec![(
            DISCUSSION_OF_FIELD,
            Value::String(discussion_id.to_string()),
        )],
    );
    let _ = live.service.rename(
        project_id,
        &member.task_id,
        &format!("{title}（讨论 · {}）", member.label),
    );
    let (project, _) = live.record(&member.task_id).ok_or("成员会话没有建成")?;
    live.service
        .start(
            &project,
            &member.task_id,
            &format!("discussion-start-{}-{}", member.task_id, super::now_ms()),
            true,
            None,
            false,
            Default::default(),
            Default::default(),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The discussion with each post's usage and each member's family.
fn view(live: &Live, discussion: &Discussion) -> Value {
    let turns: Vec<Vec<transcript_stats::TurnStat>> = discussion
        .members
        .iter()
        .map(|m| member_turns(live, &m.task_id))
        .collect();
    let usage: Vec<Value> = discussion
        .posts
        .iter()
        .map(|post| {
            turns
                .get(post.member)
                .and_then(|t| core::usage(post, t))
                .map(|u| json!(u))
                .unwrap_or(Value::Null)
        })
        .collect();
    let families: Vec<Value> = discussion
        .members
        .iter()
        .map(|m| json!(live.families.session_family(&m.agent)))
        .collect();
    json!({
        "discussion": discussion,
        "usage": usage,
        "families": families,
    })
}

/// A member's turns from its transcript.
fn member_turns(live: &Live, task_id: &str) -> Vec<transcript_stats::TurnStat> {
    let Some((_, record)) = live.record(task_id) else {
        return Vec::new();
    };
    let (_, path) = aeroric_core::tasks::session_owner::current_session(&record);
    let Some(raw) = path.and_then(|p| {
        aeroric_core::sessions::transcript_file::read_to_string(std::path::Path::new(&p)).ok()
    }) else {
        return Vec::new();
    };
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let agent = field(&record, "agent").unwrap_or_default();
    let stats = match live.families.session_family(&agent) {
        ProtocolFamily::Codex => transcript_stats::codex(&lines),
        ProtocolFamily::Claude => transcript_stats::claude(&lines),
        ProtocolFamily::Omp => transcript_stats::omp(&lines),
        ProtocolFamily::Dsh => transcript_stats::dsh(&lines),
    };
    stats.map(|s| s.turns).unwrap_or_default()
}

fn task_params(request: &Request) -> Result<TaskParams, Box<Frame>> {
    params(request)
}

fn get(live: &Live, request: &Request) -> Frame {
    let p = match task_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.discussions().get(&p.task_id) {
        Some(discussion) => ok(request, json!({ "discussion": view(live, &discussion) })),
        None => discussion_failure(request, DiscussionError::NotFound),
    }
}

fn act(
    live: &Live,
    request: &Request,
    f: impl FnOnce(&core::Discussions, &str) -> Result<Discussion, DiscussionError>,
) -> Frame {
    let p = match task_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match f(live.executor.discussions(), &p.task_id) {
        Ok(discussion) => ok(request, json!({ "discussion": view(live, &discussion) })),
        Err(error) => discussion_failure(request, error),
    }
}

fn pause(live: &Live, request: &Request) -> Frame {
    act(live, request, |d, id| d.pause(id))
}

fn resume(live: &Live, request: &Request) -> Frame {
    act(live, request, |d, id| d.resume(id))
}

fn summarize(live: &Live, request: &Request) -> Frame {
    act(live, request, |d, id| d.summarize_now(id))
}

fn stop(live: &Live, request: &Request) -> Frame {
    act(live, request, |d, id| d.stop(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_member_names_are_numbered() {
        let names = ["Codex · a", "Claude", "Codex · a"].map(String::from);
        assert_eq!(labels(&names), ["Codex · a #1", "Claude", "Codex · a #2"]);
        assert_eq!(first_line("\n  为什么慢\n细节", 40), "为什么慢");
        assert_eq!(first_line("一二三四五", 3), "一二三…");
    }
}
