//! B6 (2026-11-13): 后台整理（做梦）, frontier-roadmap §6.5. One scope (a
//! project or global) at a time:
//!
//! 1. its active profile, fact and experience items are grouped by how many
//!    character pairs they share — no model; a scope with no such group
//!    costs nothing;
//! 2. the analysis model says of each group whether it is one thing said
//!    twice (duplicate), two things that cannot both hold (conflict), or
//!    only alike in wording;
//! 3. core checks the reply (ids, the group, the snapshot) and applies it:
//!    duplicates of model-made experiences are merged at once — the others
//!    are marked replaced by the one kept, and the run can take that back;
//!    anything with the user's words, a pinned item, the profile or a
//!    conflict becomes a proposal the user decides.
//!
//! A dream never writes text of its own on its own: it only keeps one
//! existing item and marks the rest replaced.
//!
//! Second batch (2026-11-14):
//! - 过时检测: in a project, a fact or experience whose every named file and
//!   code symbol is gone becomes a proposal to archive it (`stale.rs`, no
//!   model). The automatic pass runs it once a day even when no dream is due.
//! - 画像压缩: when the profile and facts handed to a new session overflow
//!   the budget, the model rewrites the scope's unpinned profile into fewer
//!   items — a proposal: the only text a dream writes, and only once the
//!   user accepts it.
//! - A decided proposal can be taken back (`revert_dream`).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use aeroric_protocol::memory::{
    Assertion, DreamProposal, DreamProposalState, DreamRelation, DreamRetired, DreamRewrite,
    MemoryCategory, MemoryDraft, MemoryItem, MemoryKind, MemoryRun, MemoryRunKind, MemoryRunState,
    MemoryState, COMPRESS_MIN_ITEMS, DREAM_AFTER_NEW, DREAM_GROUP_MAX, DREAM_MAX_GROUPS,
    MAX_TEXT_CHARS,
};
use serde::Deserialize;

use aeroric_protocol::memory::SEMANTIC_DREAM_ALIKE;

use super::run::day_start_ms;
use super::{normalize, profile, secrets, stale, Memory, MemoryError};
use crate::settings::model_detect::ChatOnce;

/// The kinds a dream looks at. Session clues are one per session, a record
/// of what happened; two alike are two sessions, not one thing said twice.
pub const KINDS: [MemoryKind; 3] = [
    MemoryKind::Profile,
    MemoryKind::Fact,
    MemoryKind::Experience,
];

/// Two items are alike when this share of the shorter one's character
/// pairs is in the other (and at least `MIN_SHARED` pairs).
const ALIKE: f64 = 0.4;
const MIN_SHARED: usize = 2;

pub const SYSTEM: &str = "你在整理一个用户的长期记忆。下面每一组记忆在字面上相近。逐组判断它们的关系：\n\
- duplicate：说的是同一件事，只留一条就够；\n\
- conflict：互相矛盾，不能同时成立（例如一条说用 tab 缩进，一条说用空格缩进）；\n\
- unrelated：只是用词相近，各说各的。\n\
多值偏好不算冲突：“喜欢 A”不否定“喜欢 B”。只在确定时判 duplicate 或 conflict，拿不准就判 unrelated。\n\
ids 只列这组里真正相关的那几条（至少两条）；keep 从 ids 里选一条：duplicate 选说得最完整、最新的，conflict 选看起来仍然成立的。\n\
只输出一个 JSON：{\"groups\": [{\"group\": 1, \"relation\": \"duplicate\", \"ids\": [\"mem-…\", \"mem-…\"], \"keep\": \"mem-…\", \"reason\": \"一句话理由\"}]}。unrelated 的组可以不写。";

pub const COMPRESS_SYSTEM: &str = "你在压缩一个用户的画像：关于这个用户的长期信息，每次开新会话都会带入，现在太长、带不全了。把下面这些条目改写成更少、更短的几条：\n\
- 只合并和精简措辞，不新增原文没有的信息，不丢掉任何一条限制或偏好；\n\
- 互相重复的合成一条，相关的可以并成一句；\n\
- 每条一句话，尽量不超过 60 字；\n\
- category 取 language（语言）、interaction（沟通习惯）、preference（偏好）、constraint（限制）之一，拿不准就省略。\n\
只输出一个 JSON：{\"items\": [{\"text\": \"…\", \"category\": \"preference\"}], \"reason\": \"一句话说明合并了什么\"}。";

fn category_word(category: Option<MemoryCategory>) -> &'static str {
    match category {
        Some(MemoryCategory::Language) => "[语言] ",
        Some(MemoryCategory::Interaction) => "[习惯] ",
        Some(MemoryCategory::Preference) => "[偏好] ",
        Some(MemoryCategory::Constraint) => "[限制] ",
        None => "",
    }
}

pub fn compress_prompt(items: &[MemoryItem]) -> String {
    items
        .iter()
        .map(|i| format!("- {}{}\n", category_word(i.category), i.text))
        .collect()
}

#[derive(Deserialize)]
struct RawCompress {
    #[serde(default)]
    items: Vec<RawRewrite>,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct RawRewrite {
    #[serde(default)]
    text: String,
    #[serde(default)]
    category: Option<String>,
}

fn json_object(reply: &str) -> Result<&str, String> {
    let start = reply.find('{').ok_or("模型没有返回 JSON")?;
    let end = reply.rfind('}').ok_or("模型没有返回 JSON")?;
    if end < start {
        return Err("模型没有返回 JSON".into());
    }
    Ok(&reply[start..=end])
}

/// The rewrite, checked: fewer items and fewer characters than `from`, none
/// empty, too long or looking like a secret.
pub fn parse_compress(
    reply: &str,
    from: &[MemoryItem],
) -> Result<(Vec<DreamRewrite>, String), String> {
    let raw: RawCompress = serde_json::from_str(json_object(reply)?)
        .map_err(|e| format!("模型返回的 JSON 读不懂：{e}"))?;
    let mut out = Vec::new();
    for item in raw.items {
        let text = item.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err("改写后有一条太长".into());
        }
        if let Some(what) = secrets::looks_secret(&text) {
            return Err(format!("改写后的文字像是{what}"));
        }
        let category = match item.category.as_deref() {
            Some("language") => Some(MemoryCategory::Language),
            Some("interaction") => Some(MemoryCategory::Interaction),
            Some("preference") => Some(MemoryCategory::Preference),
            Some("constraint") => Some(MemoryCategory::Constraint),
            _ => None,
        };
        out.push(DreamRewrite { text, category });
    }
    let before: usize = from.iter().map(|i| i.text.chars().count()).sum();
    let after: usize = out.iter().map(|r| r.text.chars().count()).sum();
    // 目检 2026-11-15: 26 items of 1153 characters for 20 of 2226 read
    // "没有变短"; say which of the two did not shrink.
    if out.is_empty() {
        return Err("改写后一条也没有".into());
    }
    if out.len() >= from.len() {
        return Err(format!(
            "改写后是 {} 条，原来 {} 条，条数没有变少",
            out.len(),
            from.len()
        ));
    }
    if after >= before {
        return Err(format!("改写后是 {after} 字，原来 {before} 字，没有变短"));
    }
    Ok((out, raw.reason.trim().chars().take(200).collect()))
}

/// The scope's unpinned profile items to rewrite, unless a rewrite of them
/// waits or the user already turned this very set down.
fn compressible(items: &[MemoryItem], proposals: &[DreamProposal]) -> Vec<MemoryItem> {
    let profile: Vec<MemoryItem> = items
        .iter()
        .filter(|i| i.kind == MemoryKind::Profile && !i.pinned)
        .cloned()
        .collect();
    if profile.len() < COMPRESS_MIN_ITEMS {
        return Vec::new();
    }
    let key = group_key(&profile.iter().map(|i| i.id.clone()).collect::<Vec<_>>());
    let blocked = proposals.iter().any(|p| {
        p.relation == DreamRelation::Compress
            && (p.state == DreamProposalState::Pending
                || (matches!(
                    p.state,
                    DreamProposalState::Rejected | DreamProposalState::Reverted
                ) && group_key(&p.ids) == key))
    });
    if blocked {
        Vec::new()
    } else {
        profile
    }
}

/// A project's facts and experiences gone stale, with what is missing —
/// unless proposed already, or kept by the user while the same names were
/// missing.
fn stale_items(
    root: &Path,
    items: &[MemoryItem],
    proposals: &[DreamProposal],
) -> Vec<(MemoryItem, Vec<String>)> {
    let candidates: Vec<MemoryItem> = items
        .iter()
        .filter(|i| matches!(i.kind, MemoryKind::Fact | MemoryKind::Experience))
        .filter(|i| {
            !proposals.iter().any(|p| {
                p.relation == DreamRelation::Stale
                    && p.state == DreamProposalState::Pending
                    && p.ids.contains(&i.id)
            })
        })
        .cloned()
        .collect();
    let Some(found) = stale::check(root, &candidates) else {
        return Vec::new();
    };
    found
        .into_iter()
        .filter(|(id, missing)| {
            !proposals.iter().any(|p| {
                p.relation == DreamRelation::Stale
                    && matches!(
                        p.state,
                        DreamProposalState::Rejected | DreamProposalState::Reverted
                    )
                    && p.ids.contains(id)
                    && p.missing == *missing
            })
        })
        .filter_map(|(id, missing)| {
            let item = candidates.iter().find(|i| i.id == id)?.clone();
            Some((item, missing))
        })
        .collect()
}

fn proposal(
    run: &MemoryRun,
    project_id: Option<&str>,
    kind: MemoryKind,
    relation: DreamRelation,
    ids: Vec<String>,
    keep: String,
    reason: String,
) -> DreamProposal {
    DreamProposal {
        id: new_id("dream"),
        run_id: run.id.clone(),
        project_id: project_id.map(str::to_string),
        kind,
        relation,
        ids,
        keep,
        reason,
        state: DreamProposalState::Pending,
        created_ms: run.at_ms,
        decided_ms: None,
        missing: Vec::new(),
        rewrite: Vec::new(),
        created: Vec::new(),
    }
}

/// The text compared: an experience's title with its problem.
fn compared(item: &MemoryItem) -> String {
    match &item.experience {
        Some(detail) => format!("{} {}", item.text, detail.problem),
        None => item.text.clone(),
    }
}

fn pairs(text: &str) -> HashSet<(char, char)> {
    let chars: Vec<char> = normalize(text).chars().collect();
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

fn alike(a: &HashSet<(char, char)>, b: &HashSet<(char, char)>) -> bool {
    let shared = a.intersection(b).count();
    let smaller = a.len().min(b.len());
    smaller > 0 && shared >= MIN_SHARED && shared as f64 / smaller as f64 >= ALIKE
}

fn group_key(ids: &[String]) -> String {
    let mut ids = ids.to_vec();
    ids.sort();
    ids.join(",")
}

/// Groups of alike items of the same kind, largest first, at most
/// `DREAM_MAX_GROUPS`. Groups the user already decided are left out.
pub fn groups(items: &[MemoryItem], decided: &HashSet<String>) -> Vec<Vec<MemoryItem>> {
    groups_with(items, decided, &HashMap::new())
}

/// [`groups`], where two items are also alike when their vectors are this
/// close (`SEMANTIC_DREAM_ALIKE`): the same thing in other words.
pub fn groups_with(
    items: &[MemoryItem],
    decided: &HashSet<String>,
    vectors: &HashMap<String, Vec<f32>>,
) -> Vec<Vec<MemoryItem>> {
    let near = |a: &MemoryItem, b: &MemoryItem| match (vectors.get(&a.id), vectors.get(&b.id)) {
        (Some(x), Some(y)) => super::semantic::cosine(x, y) >= SEMANTIC_DREAM_ALIKE,
        _ => false,
    };
    let mut out = Vec::new();
    for kind in KINDS {
        let mine: Vec<&MemoryItem> = items
            .iter()
            .filter(|i| i.kind == kind && i.state == MemoryState::Active)
            .collect();
        let shapes: Vec<_> = mine.iter().map(|i| pairs(&compared(i))).collect();
        // Union-find over the alike pairs.
        let mut parent: Vec<usize> = (0..mine.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        for a in 0..mine.len() {
            for b in a + 1..mine.len() {
                if alike(&shapes[a], &shapes[b]) || near(mine[a], mine[b]) {
                    let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                    parent[ra] = rb;
                }
            }
        }
        let mut sets: HashMap<usize, Vec<&MemoryItem>> = HashMap::new();
        for (i, item) in mine.iter().enumerate() {
            let r = root(&mut parent, i);
            sets.entry(r).or_default().push(item);
        }
        for mut set in sets.into_values().filter(|s| s.len() >= 2) {
            set.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms));
            set.truncate(DREAM_GROUP_MAX);
            let ids: Vec<String> = set.iter().map(|i| i.id.clone()).collect();
            if !decided.contains(&group_key(&ids)) {
                out.push(set.into_iter().cloned().collect::<Vec<_>>());
            }
        }
    }
    out.sort_by(|a: &Vec<MemoryItem>, b| {
        b.len().cmp(&a.len()).then_with(|| {
            let newest = |g: &Vec<MemoryItem>| g.iter().map(|i| i.updated_ms).max();
            newest(b).cmp(&newest(a))
        })
    });
    out.truncate(DREAM_MAX_GROUPS);
    out
}

fn kind_word(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Profile => "关于用户",
        MemoryKind::Fact => "项目约定",
        MemoryKind::Experience => "经验",
        MemoryKind::SessionFact => "会话线索",
    }
}

fn source_word(item: &MemoryItem) -> &'static str {
    match item.assertion {
        Assertion::UserExplicit => "用户亲口说的",
        Assertion::AgentConfirmed => "用户确认过",
        Assertion::ToolVerified => "工具核实",
        Assertion::ModelInferred => "模型整理",
    }
}

pub fn prompt(groups: &[Vec<MemoryItem>]) -> String {
    let mut out = String::new();
    for (n, group) in groups.iter().enumerate() {
        out.push_str(&format!(
            "## 第 {} 组（{}）\n",
            n + 1,
            kind_word(group[0].kind)
        ));
        for item in group {
            out.push_str(&format!("- {}（{}", item.id, source_word(item)));
            if item.pinned {
                out.push_str("，置顶");
            }
            out.push_str(&format!("）：{}", item.text));
            if let Some(detail) = &item.experience {
                out.push_str(&format!(
                    "｜问题：{}｜解法：{}",
                    detail.problem, detail.solution
                ));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

#[derive(Deserialize)]
struct RawReply {
    #[serde(default)]
    groups: Vec<RawGroup>,
}

#[derive(Deserialize)]
struct RawGroup {
    #[serde(default)]
    group: usize,
    #[serde(default)]
    relation: String,
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    keep: String,
    #[serde(default)]
    reason: String,
}

/// One checked judgement: `ids` are all in `group` (index into the groups
/// sent), at least two, and `keep` is one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    pub group: usize,
    pub relation: DreamRelation,
    pub ids: Vec<String>,
    pub keep: String,
    pub reason: String,
}

/// The model's reply, checked against the groups it was sent; what does not
/// fit is dropped and counted.
pub fn parse(reply: &str, groups: &[Vec<MemoryItem>]) -> Result<(Vec<Judgement>, u32), String> {
    let raw: RawReply = serde_json::from_str(json_object(reply)?)
        .map_err(|e| format!("模型返回的 JSON 读不懂：{e}"))?;
    let mut out: Vec<Judgement> = Vec::new();
    let mut dropped = 0;
    for g in raw.groups {
        let relation = match g.relation.as_str() {
            "duplicate" => DreamRelation::Duplicate,
            "conflict" => DreamRelation::Conflict,
            _ => continue,
        };
        let Some(sent) = g.group.checked_sub(1).and_then(|i| groups.get(i)) else {
            dropped += 1;
            continue;
        };
        let known: HashSet<&str> = sent.iter().map(|i| i.id.as_str()).collect();
        let mut ids: Vec<String> = Vec::new();
        for id in g.ids {
            if known.contains(id.as_str()) && !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.len() < 2 || !ids.contains(&g.keep) || out.iter().any(|j| j.group == g.group - 1) {
            dropped += 1;
            continue;
        }
        out.push(Judgement {
            group: g.group - 1,
            relation,
            ids,
            keep: g.keep,
            reason: g.reason.trim().chars().take(200).collect(),
        });
    }
    Ok((out, dropped))
}

/// What a dream may merge without asking: model-made experiences, none
/// pinned, said twice.
fn merges_itself(relation: DreamRelation, items: &[&MemoryItem]) -> bool {
    relation == DreamRelation::Duplicate
        && items.iter().all(|i| {
            i.kind == MemoryKind::Experience && !i.pinned && i.assertion != Assertion::UserExplicit
        })
}

/// The one a merge keeps: the most trusted, then the most useful, then the
/// newest — so a merge never loses the item people relied on.
fn best<'a>(items: &[&'a MemoryItem]) -> &'a MemoryItem {
    items
        .iter()
        .max_by_key(|i| {
            (
                i.confidence,
                i64::from(i.stats.helpful) - i64::from(i.stats.wrong),
                i.updated_ms,
            )
        })
        .copied()
        .expect("a group has items")
}

fn scope_key(project_id: Option<&str>) -> String {
    format!("dream:{}", project_id.unwrap_or(""))
}

fn stale_key(project_id: &str) -> String {
    format!("stale:{project_id}")
}

fn new_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

/// The dream of one scope, as the host starts it.
pub struct DreamScope<'a> {
    /// `None` is global.
    pub project_id: Option<&'a str>,
    /// For 整理记录: the project's name, or "全局".
    pub title: &'a str,
    /// The user asked (现在整理): a scope with nothing alike is recorded as
    /// skipped instead of passing silently.
    pub forced: bool,
    /// A local project's folder: its facts and experiences are checked for
    /// files and symbols gone (过时检测).
    pub root: Option<&'a Path>,
    /// Only what needs no model (过时检测): the daily check when no dream is
    /// due; a run is recorded only if it found something.
    pub free_only: bool,
}

impl Memory {
    /// The automatic pass may dream this scope: at most once a local day,
    /// and only once `DREAM_AFTER_NEW` items were added since the last time.
    pub fn dream_due(&self, project_id: Option<&str>, now_ms: u64) -> bool {
        let last = self
            .with(|store| store.kv(&scope_key(project_id)))
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        if last >= day_start_ms(now_ms) {
            return false;
        }
        self.with(|store| store.added_since(project_id, &KINDS, last))
            .is_ok_and(|n| n >= DREAM_AFTER_NEW)
    }

    /// 过时检测 is due for a project: at most once a local day.
    pub fn stale_due(&self, project_id: &str, now_ms: u64) -> bool {
        self.with(|store| store.kv(&stale_key(project_id)))
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok())
            .is_none_or(|last| last < day_start_ms(now_ms))
    }

    /// Dream one scope. `Err` means nothing was done and nothing recorded
    /// (no model, the day's limit, already dreaming); otherwise the row.
    pub fn dream(
        &self,
        scope: DreamScope<'_>,
        call: impl FnMut(&str, &str) -> Result<ChatOnce, String>,
        now_ms: u64,
    ) -> Result<Option<MemoryRun>, String> {
        let key = scope_key(scope.project_id);
        if !self.dreaming.lock().insert(key.clone()) {
            return Err("这里正在整理".into());
        }
        let result = self.dream_locked(&scope, call, now_ms);
        self.dreaming.lock().remove(&key);
        result
    }

    fn dream_locked(
        &self,
        scope: &DreamScope<'_>,
        mut call: impl FnMut(&str, &str) -> Result<ChatOnce, String>,
        now_ms: u64,
    ) -> Result<Option<MemoryRun>, String> {
        let settings = self.settings();
        let model = settings
            .model
            .clone()
            .filter(|_| settings.connection_id.is_some());
        if model.is_none() && !scope.free_only {
            return Err("还没有选分析模型：在记忆页的“整理设置”里选一个".into());
        }
        let (items, proposals, overflow) = self
            .with(|store| {
                let items = store.active(scope.project_id)?;
                let proposals = store.dream_proposals(scope.project_id)?;
                let overflow = match scope.project_id {
                    Some(_) => profile::overflows(&store.active(None)?, &items),
                    None => profile::overflows(&items, &[]),
                };
                Ok((items, proposals, overflow))
            })
            .map_err(|e| e.to_string())?;
        let decided: HashSet<String> = proposals
            .iter()
            .filter(|p| p.state != DreamProposalState::Withdrawn)
            .map(|p| group_key(&p.ids))
            .collect();
        let groups = if scope.free_only {
            Vec::new()
        } else if settings.semantic {
            groups_with(&items, &decided, &self.vectors(scope.project_id))
        } else {
            groups(&items, &decided)
        };
        let profile = if scope.free_only || !overflow {
            Vec::new()
        } else {
            compressible(&items, &proposals)
        };
        let stale = match (scope.project_id, scope.root) {
            (Some(project), Some(root)) => {
                let _ = self.with(|store| store.set_kv(&stale_key(project), &now_ms.to_string()));
                stale_items(root, &items, &proposals)
            }
            _ => Vec::new(),
        };
        let mark = |memory: &Memory| {
            memory.with(|store| store.set_kv(&scope_key(scope.project_id), &now_ms.to_string()))
        };
        let mut run = MemoryRun {
            id: new_id("run"),
            task_id: String::new(),
            project_id: scope.project_id.unwrap_or_default().to_string(),
            title: scope.title.to_string(),
            at_ms: now_ms,
            state: MemoryRunState::Skipped,
            reason: None,
            model: None,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            facts: 0,
            candidates: 0,
            experiences: 0,
            failed_writes: 0,
            kind: MemoryRunKind::Dream,
            retired: Vec::new(),
            proposed: 0,
            undone_ms: None,
        };
        let needs_model = !groups.is_empty() || !profile.is_empty();
        if !needs_model && stale.is_empty() {
            if scope.free_only {
                return Ok(None);
            }
            mark(self).map_err(|e| e.to_string())?;
            if !scope.forced {
                return Ok(None);
            }
            run.reason = Some("没有字面相近的记忆，画像也没超出，不用整理".into());
            self.add_run(&run).map_err(|e| e.to_string())?;
            return Ok(Some(run));
        }
        let mut notes = Vec::new();
        let mut failures = Vec::new();
        let mut judgements = Vec::new();
        let mut rewrite: Option<(Vec<DreamRewrite>, String)> = None;
        if needs_model {
            let used = self.counted_since(day_start_ms(now_ms));
            let model = model.clone().unwrap_or_default();
            if used >= settings.daily_limit {
                if stale.is_empty() {
                    return Err(format!("今天已经整理了 {used} 次，到上限了，明天再整理"));
                }
                notes.push(format!(
                    "今天已经整理了 {used} 次，到上限了，这次只查了过时"
                ));
            } else {
                mark(self).map_err(|e| e.to_string())?;
                run.model = Some(model.clone());
                let spent = |reply: &ChatOnce, run: &mut MemoryRun| {
                    run.input_tokens += reply.input_tokens;
                    run.output_tokens += reply.output_tokens;
                };
                if !groups.is_empty() {
                    match call(SYSTEM, &prompt(&groups)) {
                        Err(error) => failures.push(format!("调用分析模型失败：{error}")),
                        Ok(reply) => {
                            spent(&reply, &mut run);
                            match parse(&reply.text, &groups) {
                                Ok((parsed, dropped)) => {
                                    judgements = parsed;
                                    if dropped > 0 {
                                        notes.push(format!("丢掉 {dropped} 组对不上的判断"));
                                    }
                                }
                                Err(error) => failures.push(error),
                            }
                        }
                    }
                }
                if !profile.is_empty() {
                    match call(COMPRESS_SYSTEM, &compress_prompt(&profile)) {
                        Err(error) => failures.push(format!("改写画像时调用分析模型失败：{error}")),
                        Ok(reply) => {
                            spent(&reply, &mut run);
                            match parse_compress(&reply.text, &profile) {
                                Ok(parsed) => rewrite = Some(parsed),
                                Err(error) => failures.push(format!("画像改写没有用上：{error}")),
                            }
                        }
                    }
                }
                run.cost_usd = (run.input_tokens + run.output_tokens > 0).then(|| {
                    crate::metrics::analytics::estimate_cost_usd(
                        &model,
                        run.input_tokens,
                        run.output_tokens,
                    )
                    .0
                });
            }
        }
        let snapshot: HashMap<String, u64> = groups
            .iter()
            .flatten()
            .chain(&profile)
            .chain(stale.iter().map(|(item, _)| item))
            .map(|i| (i.id.clone(), i.updated_ms))
            .collect();
        let mut changed = 0;
        let applied = self.with(|store| {
            // An item changed since the snapshot drops out of this run.
            let unchanged = |store: &super::store::Store,
                             id: &str|
             -> Result<Option<MemoryItem>, MemoryError> {
                Ok(store.get(id)?.filter(|item| {
                    item.state == MemoryState::Active && snapshot.get(id) == Some(&item.updated_ms)
                }))
            };
            for judgement in &judgements {
                // Too few left, or the kept one gone, drops the judgement.
                let mut now_items = Vec::new();
                for id in &judgement.ids {
                    match unchanged(store, id)? {
                        Some(item) => now_items.push(item),
                        None => changed += 1,
                    }
                }
                let refs: Vec<&MemoryItem> = now_items.iter().collect();
                if refs.len() < 2 || !refs.iter().any(|i| i.id == judgement.keep) {
                    continue;
                }
                if merges_itself(judgement.relation, &refs) {
                    let keep = best(&refs).id.clone();
                    for item in refs.iter().filter(|i| i.id != keep) {
                        if store.retire(&item.id, &keep, now_ms)? {
                            run.retired.push(DreamRetired {
                                id: item.id.clone(),
                                keep: keep.clone(),
                            });
                        }
                    }
                } else {
                    store.put_dream_proposal(&proposal(
                        &run,
                        scope.project_id,
                        refs[0].kind,
                        judgement.relation,
                        refs.iter().map(|i| i.id.clone()).collect(),
                        judgement.keep.clone(),
                        judgement.reason.clone(),
                    ))?;
                    run.proposed += 1;
                }
            }
            if let Some((texts, reason)) = &rewrite {
                let mut whole = true;
                for item in &profile {
                    if unchanged(store, &item.id)?.is_none() {
                        whole = false;
                    }
                }
                if whole {
                    let mut p = proposal(
                        &run,
                        scope.project_id,
                        MemoryKind::Profile,
                        DreamRelation::Compress,
                        profile.iter().map(|i| i.id.clone()).collect(),
                        String::new(),
                        reason.clone(),
                    );
                    p.rewrite = texts.clone();
                    store.put_dream_proposal(&p)?;
                    run.proposed += 1;
                } else {
                    notes.push("画像在整理期间被改过，这次不改写".into());
                }
            }
            for (item, missing) in &stale {
                if unchanged(store, &item.id)?.is_none() {
                    changed += 1;
                    continue;
                }
                let mut p = proposal(
                    &run,
                    scope.project_id,
                    item.kind,
                    DreamRelation::Stale,
                    vec![item.id.clone()],
                    item.id.clone(),
                    format!("提到的 {} 在项目里都找不到了", missing.join("、"))
                        .chars()
                        .take(200)
                        .collect(),
                );
                p.missing = missing.clone();
                store.put_dream_proposal(&p)?;
                run.proposed += 1;
            }
            Ok(())
        });
        let produced = !run.retired.is_empty() || run.proposed > 0;
        if let Err(error) = applied {
            failures.push(format!("写入出错：{error}"));
        }
        if scope.free_only && !produced && failures.is_empty() {
            return Ok(None);
        }
        run.state = match (failures.is_empty(), produced) {
            (true, _) => MemoryRunState::Done,
            (false, true) => MemoryRunState::Partial,
            (false, false) => MemoryRunState::Failed,
        };
        if changed > 0 {
            notes.push(format!("{changed} 条在整理期间被改过，这次不动"));
        }
        failures.extend(notes);
        run.reason = (!failures.is_empty()).then(|| failures.join("；"));
        self.add_run(&run).map_err(|e| e.to_string())?;
        Ok(Some(run))
    }

    /// A scope's dream proposals, pending first, then newest.
    pub fn dream_proposals(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<DreamProposal>, MemoryError> {
        let mut out = self.with(|store| store.dream_proposals(project_id))?;
        out.sort_by_key(|p| {
            (
                p.state != DreamProposalState::Pending,
                std::cmp::Reverse(p.created_ms),
            )
        });
        Ok(out)
    }

    /// The user decides a proposal: accepting keeps `keep` (the model's
    /// choice unless the user picked another) and marks the others replaced
    /// by it; rejecting leaves them all, and the group is not proposed again.
    pub fn decide_dream(
        &self,
        id: &str,
        accept: bool,
        keep: Option<&str>,
        now_ms: u64,
    ) -> Result<DreamProposal, MemoryError> {
        self.with(|store| {
            let mut proposal = store.dream_proposal(id)?.ok_or(MemoryError::UnknownItem)?;
            if proposal.state != DreamProposalState::Pending {
                return Err(MemoryError::WrongState);
            }
            if accept {
                match proposal.relation {
                    DreamRelation::Stale => {
                        let item = store.get(&proposal.keep)?.ok_or(MemoryError::UnknownItem)?;
                        if item.state != MemoryState::Active {
                            return Err(MemoryError::WrongState);
                        }
                        store.set_state(&item.id, MemoryState::Archived, now_ms)?;
                    }
                    DreamRelation::Compress => accept_rewrite(store, &mut proposal, now_ms)?,
                    DreamRelation::Duplicate | DreamRelation::Conflict => {
                        let keep = keep.unwrap_or(&proposal.keep).to_string();
                        if !proposal.ids.contains(&keep) {
                            return Err(MemoryError::UnknownItem);
                        }
                        let kept = store.get(&keep)?.ok_or(MemoryError::UnknownItem)?;
                        if kept.state != MemoryState::Active {
                            return Err(MemoryError::WrongState);
                        }
                        for other in proposal.ids.iter().filter(|i| **i != keep) {
                            store.retire(other, &keep, now_ms)?;
                        }
                        proposal.keep = keep;
                    }
                }
                proposal.state = DreamProposalState::Accepted;
            } else {
                proposal.state = DreamProposalState::Rejected;
            }
            proposal.decided_ms = Some(now_ms);
            store.put_dream_proposal(&proposal)?;
            Ok(proposal)
        })
    }

    /// Take back one accepted proposal: what it replaced comes back (unless
    /// changed since), an archived stale item is active again, and a
    /// profile rewrite's new items go. The same set is not proposed again.
    pub fn revert_dream(&self, id: &str, now_ms: u64) -> Result<DreamProposal, MemoryError> {
        self.with(|store| {
            let mut proposal = store.dream_proposal(id)?.ok_or(MemoryError::UnknownItem)?;
            if proposal.state != DreamProposalState::Accepted {
                return Err(MemoryError::WrongState);
            }
            match proposal.relation {
                DreamRelation::Stale => {
                    if let Some(item) = store.get(&proposal.keep)? {
                        if item.state == MemoryState::Archived {
                            store.set_state(&item.id, MemoryState::Active, now_ms)?;
                        }
                    }
                }
                DreamRelation::Duplicate | DreamRelation::Conflict | DreamRelation::Compress => {
                    for old in proposal.ids.iter().filter(|i| **i != proposal.keep) {
                        store.unretire(old, &proposal.keep, now_ms)?;
                    }
                    for made in &proposal.created {
                        if let Some(item) = store.get(made)? {
                            store.delete(&item)?;
                        }
                    }
                }
            }
            proposal.state = DreamProposalState::Reverted;
            proposal.decided_ms = Some(now_ms);
            store.put_dream_proposal(&proposal)?;
            Ok(proposal)
        })
    }

    /// The proposals waiting in every scope, newest first (需要我处理).
    pub fn pending_dreams(&self) -> Result<Vec<DreamProposal>, MemoryError> {
        self.with(|store| store.pending_dream_proposals())
    }

    /// 撤销这次整理: what the dream merged itself comes back (unless changed
    /// since), and its proposals still waiting are withdrawn. What the user
    /// already decided stays decided.
    pub fn undo_dream(&self, run_id: &str, now_ms: u64) -> Result<MemoryRun, MemoryError> {
        self.with(|store| {
            let mut run = store.run(run_id)?.ok_or(MemoryError::UnknownItem)?;
            if run.kind != MemoryRunKind::Dream || run.undone_ms.is_some() {
                return Err(MemoryError::WrongState);
            }
            for retired in &run.retired {
                store.unretire(&retired.id, &retired.keep, now_ms)?;
            }
            for mut proposal in
                store.dream_proposals(Some(run.project_id.as_str()).filter(|p| !p.is_empty()))?
            {
                if proposal.run_id == run.id && proposal.state == DreamProposalState::Pending {
                    proposal.state = DreamProposalState::Withdrawn;
                    proposal.decided_ms = Some(now_ms);
                    store.put_dream_proposal(&proposal)?;
                }
            }
            run.undone_ms = Some(now_ms);
            store.add_run(&run)?;
            Ok(run)
        })
    }
}

/// Accept a profile rewrite: the new items are made (the user's yes makes
/// them agent-confirmed), and the old ones are marked replaced by the
/// first. All the old ones must still be as proposed.
fn accept_rewrite(
    store: &mut super::store::Store,
    proposal: &mut DreamProposal,
    now_ms: u64,
) -> Result<(), MemoryError> {
    let project = proposal.project_id.as_deref();
    for id in &proposal.ids {
        let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
        if item.state != MemoryState::Active {
            return Err(MemoryError::WrongState);
        }
    }
    let before: HashSet<String> = store.active(project)?.into_iter().map(|i| i.id).collect();
    let source = format!("整理时由 {} 条画像改写", proposal.ids.len());
    let mut made = Vec::new();
    for rewrite in &proposal.rewrite {
        let draft = MemoryDraft {
            project_id: proposal.project_id.clone(),
            kind: MemoryKind::Profile,
            category: rewrite.category,
            text: rewrite.text.clone(),
            supersedes: None,
        };
        let item = store.insert(
            &draft,
            &rewrite.text,
            Assertion::AgentConfirmed,
            MemoryState::Active,
            super::evidence(None, None, None, &source, now_ms),
            now_ms,
        )?;
        if !made.contains(&item.id) {
            made.push(item.id);
        }
    }
    let keep = made.first().cloned().ok_or(MemoryError::WrongState)?;
    for old in proposal.ids.iter().filter(|id| !made.contains(id)) {
        store.retire(old, &keep, now_ms)?;
    }
    proposal.created = made.into_iter().filter(|id| !before.contains(id)).collect();
    proposal.keep = keep;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use aeroric_protocol::memory::{
        Confidence, DreamProposalState, DreamRelation, ExperienceDetail, MemoryDraft, MemoryKind,
        MemoryRunKind, MemoryRunState, MemorySettings, MemoryState,
    };

    use super::{groups, parse, parse_compress, DreamScope, COMPRESS_SYSTEM};
    use crate::memory::{evidence, Memory};
    use crate::settings::model_detect::ChatOnce;

    fn rig() -> Memory {
        let dir = std::env::temp_dir().join(format!("aeroric-dream-{}", uuid::Uuid::new_v4()));
        let memory = Memory::default();
        memory.set_dir(dir).unwrap();
        memory.set_project_enabled("p1", true).unwrap();
        memory
            .set_settings(&MemorySettings {
                auto: true,
                connection_id: Some("c1".into()),
                model: Some("deepseek-v4.1-flash".into()),
                ..MemorySettings::default()
            })
            .unwrap();
        memory
    }

    fn fact(memory: &Memory, text: &str, at: u64) -> String {
        memory
            .remember(
                MemoryDraft {
                    project_id: Some("p1".into()),
                    kind: MemoryKind::Fact,
                    category: None,
                    text: text.into(),
                    supersedes: None,
                },
                evidence(None, None, None, text, at),
                at,
            )
            .unwrap()
            .id
    }

    fn experience(memory: &Memory, title: &str, confidence: Confidence, at: u64) -> String {
        memory
            .add_experience(
                "p1",
                title,
                &ExperienceDetail {
                    problem: "SSH 连接卡住".into(),
                    solution: "加 ConnectTimeout".into(),
                    ..ExperienceDetail::default()
                },
                confidence,
                evidence(Some("t1"), Some("analysis"), None, title, at),
                at,
            )
            .unwrap()
            .id
    }

    fn reply(text: String) -> impl FnMut(&str, &str) -> Result<ChatOnce, String> {
        move |_, _| {
            Ok(ChatOnce {
                text: text.clone(),
                input_tokens: 900,
                output_tokens: 80,
            })
        }
    }

    fn scope(forced: bool) -> DreamScope<'static> {
        DreamScope {
            project_id: Some("p1"),
            title: "trainer",
            forced,
            root: None,
            free_only: false,
        }
    }

    fn state(memory: &Memory, id: &str) -> MemoryState {
        memory.item(id).unwrap().unwrap().state
    }

    #[test]
    fn alike_items_of_one_kind_are_grouped_and_decided_groups_are_left_out() {
        let memory = rig();
        let a = fact(&memory, "发布前先跑 native:check sessions", 1);
        let b = fact(&memory, "发布之前要跑 native:check sessions", 2);
        fact(&memory, "数据集放在 /data/coco", 3);
        experience(&memory, "SSH 超时改用 ConnectTimeout", Confidence::Low, 4);
        let items = memory.list(Some("p1"), None).unwrap();
        let found = groups(&items, &HashSet::new());
        assert_eq!(found.len(), 1, "{found:?}");
        let ids: HashSet<_> = found[0].iter().map(|i| i.id.clone()).collect();
        assert_eq!(ids, HashSet::from([a.clone(), b.clone()]));
        let mut key = [a, b];
        key.sort();
        assert!(groups(&items, &HashSet::from([key.join(",")])).is_empty());
    }

    #[test]
    fn a_reply_is_checked_against_the_groups_it_was_sent() {
        let memory = rig();
        let a = fact(&memory, "发布前先跑 native:check sessions", 1);
        let b = fact(&memory, "发布之前要跑 native:check sessions", 2);
        let items = memory.list(Some("p1"), None).unwrap();
        let sent = groups(&items, &HashSet::new());
        let text = format!(
            r#"好的 {{"groups": [
              {{"group": 1, "relation": "duplicate", "ids": ["{a}", "{b}", "mem-made-up"], "keep": "{a}", "reason": "同一件事"}},
              {{"group": 2, "relation": "conflict", "ids": ["{a}", "{b}"], "keep": "{a}"}},
              {{"group": 1, "relation": "duplicate", "ids": ["{a}"], "keep": "{a}"}},
              {{"group": 1, "relation": "unrelated"}}]}}"#
        );
        let (judgements, dropped) = parse(&text, &sent).unwrap();
        assert_eq!(judgements.len(), 1);
        assert_eq!(judgements[0].relation, DreamRelation::Duplicate);
        assert_eq!(
            judgements[0].ids,
            vec![a.clone(), b.clone()],
            "the made-up id is gone"
        );
        assert_eq!(dropped, 2, "a group never sent, and one id is not a pair");
        assert!(parse("没有", &sent).is_err());
    }

    #[test]
    fn experiences_merge_themselves_the_rest_waits_and_undo_takes_it_back() {
        let memory = rig();
        // The facts are newer, so theirs is the first group sent.
        let e1 = experience(&memory, "SSH 超时改用 ConnectTimeout", Confidence::High, 1);
        let e2 = experience(&memory, "SSH 超时就用 ConnectTimeout", Confidence::Low, 2);
        let f1 = fact(&memory, "发布前先跑 native:check sessions", 3);
        let f2 = fact(&memory, "发布之前要跑 native:check sessions", 4);
        let mut sent = String::new();
        let text = format!(
            r#"{{"groups": [
              {{"group": 1, "relation": "duplicate", "ids": ["{f1}", "{f2}"], "keep": "{f2}", "reason": "同一条发布约定"}},
              {{"group": 2, "relation": "duplicate", "ids": ["{e1}", "{e2}"], "keep": "{e2}", "reason": "同一个修法"}}]}}"#
        );
        let run = memory
            .dream(
                scope(true),
                |system, user| {
                    sent = format!("{system}\n{user}");
                    reply(text.clone())(system, user)
                },
                10,
            )
            .unwrap()
            .unwrap();
        assert!(sent.contains("用户亲口说的") && sent.contains("模型整理"));
        assert_eq!(run.kind, MemoryRunKind::Dream);
        assert_eq!(run.state, MemoryRunState::Done);
        assert_eq!(run.retired.len(), 1);
        assert_eq!(run.proposed, 1);
        assert!(run.cost_usd.is_some());
        // The merge keeps the trusted experience, not the model's pick.
        assert_eq!(state(&memory, &e1), MemoryState::Active);
        assert_eq!(state(&memory, &e2), MemoryState::Superseded);
        // The user's own words wait for the user.
        assert_eq!(state(&memory, &f1), MemoryState::Active);
        assert_eq!(state(&memory, &f2), MemoryState::Active);
        let proposals = memory.dream_proposals(Some("p1")).unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].kind, MemoryKind::Fact);
        assert_eq!(proposals[0].keep, f2);

        let undone = memory.undo_dream(&run.id, 20).unwrap();
        assert_eq!(undone.undone_ms, Some(20));
        assert_eq!(state(&memory, &e2), MemoryState::Active);
        assert_eq!(
            memory.dream_proposals(Some("p1")).unwrap()[0].state,
            DreamProposalState::Withdrawn
        );
        assert!(memory.undo_dream(&run.id, 21).is_err(), "once");
        assert_eq!(memory.runs(10).unwrap()[0].undone_ms, Some(20));
    }

    #[test]
    fn the_user_decides_a_conflict_and_a_rejected_group_is_not_asked_again() {
        let memory = rig();
        let tab = fact(&memory, "Python 代码缩进用 tab", 1);
        let spaces = fact(&memory, "Python 代码缩进用 4 个空格", 2);
        let text = format!(
            r#"{{"groups": [{{"group": 1, "relation": "conflict", "ids": ["{tab}", "{spaces}"], "keep": "{spaces}", "reason": "缩进只能一种"}}]}}"#
        );
        memory
            .dream(scope(true), reply(text.clone()), 10)
            .unwrap()
            .unwrap();
        let proposal = memory.dream_proposals(Some("p1")).unwrap().remove(0);
        assert_eq!(proposal.relation, DreamRelation::Conflict);
        // The user keeps the other one.
        let decided = memory
            .decide_dream(&proposal.id, true, Some(&tab), 11)
            .unwrap();
        assert_eq!(decided.state, DreamProposalState::Accepted);
        assert_eq!(state(&memory, &tab), MemoryState::Active);
        assert_eq!(state(&memory, &spaces), MemoryState::Superseded);
        assert!(memory.decide_dream(&proposal.id, true, None, 12).is_err());

        let other = rig();
        let a = fact(&other, "Python 代码缩进用 tab", 1);
        let b = fact(&other, "Python 代码缩进用 4 个空格", 2);
        let text = format!(
            r#"{{"groups": [{{"group": 1, "relation": "conflict", "ids": ["{a}", "{b}"], "keep": "{b}"}}]}}"#
        );
        other.dream(scope(true), reply(text), 10).unwrap();
        let id = other.dream_proposals(Some("p1")).unwrap()[0].id.clone();
        other.decide_dream(&id, false, None, 11).unwrap();
        assert_eq!(state(&other, &a), MemoryState::Active);
        let again = other
            .dream(scope(true), |_, _| unreachable!("nothing left to ask"), 12)
            .unwrap()
            .unwrap();
        assert_eq!(again.state, MemoryRunState::Skipped);
    }

    #[test]
    fn an_item_changed_during_the_call_drops_out_of_the_run() {
        let memory = rig();
        let e1 = experience(&memory, "SSH 超时改用 ConnectTimeout", Confidence::Low, 1);
        let e2 = experience(&memory, "SSH 超时就用 ConnectTimeout", Confidence::Low, 2);
        let text = format!(
            r#"{{"groups": [{{"group": 1, "relation": "duplicate", "ids": ["{e1}", "{e2}"], "keep": "{e1}"}}]}}"#
        );
        let run = memory
            .dream(
                scope(false),
                |system, user| {
                    memory.pin(&e2, true, 5).unwrap();
                    reply(text.clone())(system, user)
                },
                3,
            )
            .unwrap()
            .unwrap();
        assert!(run.retired.is_empty());
        assert!(run.reason.unwrap().contains("被改过"));
        assert_eq!(state(&memory, &e1), MemoryState::Active);
        assert_eq!(state(&memory, &e2), MemoryState::Active);
    }

    #[test]
    fn nothing_alike_costs_nothing_and_the_automatic_pass_waits_for_twenty_new() {
        let memory = rig();
        fact(&memory, "数据集放在 /data/coco", 1);
        assert_eq!(
            memory
                .dream(scope(false), |_, _| unreachable!(), 2)
                .unwrap(),
            None,
            "the automatic pass records nothing"
        );
        let forced = memory
            .dream(scope(true), |_, _| unreachable!(), 3)
            .unwrap()
            .unwrap();
        assert_eq!(forced.state, MemoryRunState::Skipped);
        assert_eq!(memory.counted_since(0), 0, "a skip is not counted");

        let day = 1_800_000_000_000;
        let fresh = rig();
        for n in 0..19 {
            fact(
                &fresh,
                &format!("约定 {n} 号：{}", "甲乙丙丁".repeat(n % 3 + 1)),
                day + n as u64,
            );
        }
        assert!(!fresh.dream_due(Some("p1"), day + 100));
        fact(&fresh, "第二十条", day + 50);
        assert!(fresh.dream_due(Some("p1"), day + 100));
        fresh
            .dream(
                DreamScope {
                    project_id: Some("p1"),
                    title: "x",
                    forced: false,
                    root: None,
                    free_only: false,
                },
                |_, _| {
                    Ok(ChatOnce {
                        text: r#"{"groups": []}"#.into(),
                        input_tokens: 1,
                        output_tokens: 1,
                    })
                },
                day + 100,
            )
            .unwrap();
        assert!(!fresh.dream_due(Some("p1"), day + 200), "once a day");
    }

    fn repo() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aeroric-dream-repo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/live.rs"), "fn live_fn() {}\n").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        dir
    }

    #[test]
    fn a_stale_fact_is_only_proposed_and_kept_ones_stay_quiet_while_nothing_changes() {
        let memory = rig();
        let root = repo();
        let t = 1_800_000_000_000u64;
        let gone = fact(&memory, "超时在 `src/gone.rs` 的 `gone_fn` 里设", 1);
        let live = fact(&memory, "入口是 `live_fn`", 2);
        let free = DreamScope {
            project_id: Some("p1"),
            title: "trainer",
            forced: false,
            root: Some(&root),
            free_only: true,
        };
        assert!(memory.stale_due("p1", t + 10));
        let run = memory
            .dream(free, |_, _| unreachable!("no model for 过时检测"), t + 10)
            .unwrap()
            .unwrap();
        assert!(!memory.stale_due("p1", t + 11), "once a day");
        assert_eq!(run.state, MemoryRunState::Done);
        assert_eq!(run.model, None);
        assert_eq!(run.proposed, 1);
        assert_eq!(memory.counted_since(0), 0, "no model, no allowance used");
        let proposal = memory.dream_proposals(Some("p1")).unwrap().remove(0);
        assert_eq!(proposal.relation, DreamRelation::Stale);
        assert_eq!(proposal.ids, vec![gone.clone()]);
        assert_eq!(proposal.missing, vec!["src/gone.rs", "gone_fn"]);
        assert_eq!(
            state(&memory, &gone),
            MemoryState::Active,
            "nothing archived on its own"
        );
        assert_eq!(state(&memory, &live), MemoryState::Active);

        let again = |at| {
            memory
                .dream(
                    DreamScope {
                        project_id: Some("p1"),
                        title: "trainer",
                        forced: false,
                        root: Some(&root),
                        free_only: true,
                    },
                    |_, _| unreachable!(),
                    at,
                )
                .unwrap()
        };
        assert_eq!(again(t + 12), None, "a waiting proposal is not made twice");
        // 还有用: not asked again while the same names are missing.
        memory
            .decide_dream(&proposal.id, false, None, t + 13)
            .unwrap();
        assert_eq!(again(t + 14), None);

        // 归档, then taken back.
        let other = fact(&memory, "旧脚本在 `tools/old.py`", 15);
        again(t + 16).unwrap();
        let id = memory
            .dream_proposals(Some("p1"))
            .unwrap()
            .into_iter()
            .find(|p| p.ids == vec![other.clone()])
            .unwrap()
            .id;
        memory.decide_dream(&id, true, None, t + 17).unwrap();
        assert_eq!(state(&memory, &other), MemoryState::Archived);
        let reverted = memory.revert_dream(&id, t + 18).unwrap();
        assert_eq!(reverted.state, DreamProposalState::Reverted);
        assert_eq!(state(&memory, &other), MemoryState::Active);
        assert!(memory.revert_dream(&id, t + 19).is_err(), "once");
        assert!(memory.pending_dreams().unwrap().is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn about(memory: &Memory, text: &str, at: u64) -> String {
        memory
            .remember(
                MemoryDraft {
                    project_id: None,
                    kind: MemoryKind::Profile,
                    category: None,
                    text: text.into(),
                    supersedes: None,
                },
                evidence(None, None, None, text, at),
                at,
            )
            .unwrap()
            .id
    }

    #[test]
    fn an_overflowing_profile_is_rewritten_only_on_the_users_yes_and_can_be_taken_back() {
        let memory = rig();
        let old: Vec<String> = ["甲", "乙", "丙", "丁"]
            .iter()
            .enumerate()
            .map(|(n, c)| {
                about(
                    &memory,
                    &format!("偏好{n}：{}", c.repeat(380)),
                    n as u64 + 1,
                )
            })
            .collect();
        let pinned = about(&memory, "回复用简体中文", 9);
        memory.pin(&pinned, true, 9).unwrap();
        let global = || DreamScope {
            project_id: None,
            title: "全局",
            forced: true,
            root: None,
            free_only: false,
        };
        let text = r#"{"items": [{"text": "偏好甲乙", "category": "preference"}, {"text": "偏好丙丁"}], "reason": "两两合并"}"#;
        let mut systems = Vec::new();
        let run = memory
            .dream(
                global(),
                |system, user| {
                    systems.push(system.to_string());
                    assert!(
                        !user.contains("回复用简体中文"),
                        "pinned items are not rewritten"
                    );
                    reply(text.into())(system, user)
                },
                10,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            systems,
            vec![COMPRESS_SYSTEM.to_string()],
            "nothing alike: one call"
        );
        assert_eq!(run.proposed, 1);
        let proposal = memory.dream_proposals(None).unwrap().remove(0);
        assert_eq!(proposal.relation, DreamRelation::Compress);
        assert_eq!(proposal.rewrite.len(), 2);
        assert_eq!(
            proposal.ids.iter().collect::<HashSet<_>>(),
            old.iter().collect::<HashSet<_>>()
        );
        assert!(old
            .iter()
            .all(|id| state(&memory, id) == MemoryState::Active));
        assert_eq!(memory.pending_dreams().unwrap().len(), 1);

        let accepted = memory.decide_dream(&proposal.id, true, None, 11).unwrap();
        assert_eq!(accepted.created.len(), 2);
        assert!(old
            .iter()
            .all(|id| state(&memory, id) == MemoryState::Superseded));
        let shown = memory.profile(None).unwrap();
        assert!(shown.contains("偏好甲乙") && shown.contains("回复用简体中文"));
        assert!(!shown.contains("还有"), "it fits now: {shown}");

        memory.revert_dream(&proposal.id, 12).unwrap();
        assert!(old
            .iter()
            .all(|id| state(&memory, id) == MemoryState::Active));
        assert!(accepted
            .created
            .iter()
            .all(|id| memory.item(id).unwrap().is_none()));
        // The same set, turned down, is not rewritten again.
        let skipped = memory
            .dream(global(), |_, _| unreachable!(), 13)
            .unwrap()
            .unwrap();
        assert_eq!(skipped.state, MemoryRunState::Skipped);
    }

    #[test]
    fn a_rewrite_must_be_shorter_and_plain() {
        let memory = rig();
        let ids: Vec<String> = (0..3)
            .map(|n| about(&memory, &format!("偏好 {n}：简短"), n))
            .collect();
        let items: Vec<_> = ids
            .iter()
            .map(|id| memory.item(id).unwrap().unwrap())
            .collect();
        assert!(
            parse_compress(r#"{"items": [{"text": "偏好 0、1、2：都要简短"}]}"#, &items).is_ok()
        );
        let longer = format!(r#"{{"items": [{{"text": "{}"}}]}}"#, "长".repeat(40));
        assert!(parse_compress(&longer, &items)
            .unwrap_err()
            .contains("没有变短"));
        assert!(parse_compress(
            r#"{"items": [{"text": "a"}, {"text": "b"}, {"text": "c"}]}"#,
            &items
        )
        .unwrap_err()
        .contains("条数没有变少"));
        assert!(parse_compress("不是 JSON", &items).is_err());
    }
}
