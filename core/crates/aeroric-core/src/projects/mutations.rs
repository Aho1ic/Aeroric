//! Project operations that decide nothing about the filesystem.
//!
//! Every function here mirrors a named function in the shipping client
//! (`src/projectOrder.ts`, `src/projectGroups.ts`, `src/state/app/projectMutations.ts`,
//! `src/appProjectState.ts`) and is pinned by `contracts/native/v1/fixtures/projects.json`,
//! which the *real* TypeScript functions are also run against in `pnpm test`. Same input,
//! same output, or one of the two suites goes red — that is the whole point: the migration
//! may not "improve" an ordering rule while it moves it.
//!
//! Records stay untyped (see `crate::storage::compat`). A mutation reads the fields it
//! knows and rewrites only those, so `branch`, `color`, `avatar`, and any key this core has
//! never heard of survive a rename as they arrived.

use serde_json::{Map, Value};

use aeroric_protocol::ProjectTarget;

use crate::storage::Record;

/// `Number.MAX_SAFE_INTEGER` — the sentinel `projectOrder.ts` gives a project with no
/// usable `orderIndex`. A huge finite number rather than an "is last" flag, because the
/// comparator subtracts, and the port must subtract the same way.
const MAX_SAFE_INDEX: f64 = 9_007_199_254_740_991.0;

/// The `Project` fields the client itself touches. `id` and `path` are strings today;
/// anything else is read defensively because the file may predate this code.
pub fn text<'a>(record: &'a Record, key: &str) -> Option<&'a str> {
    record.str_field(key)
}

fn number_field(record: &Record, key: &str) -> Option<f64> {
    record.get(key).and_then(Value::as_f64)
}

fn bool_field(record: &Record, key: &str) -> bool {
    record.get(key).and_then(Value::as_bool) == Some(true)
}

/// `finiteOrderIndex`: a JSON number, or nothing. A string `"3"` is *not* an order index —
/// the client treats it as missing and reassigns, and so do we.
fn finite_order_index(record: &Record) -> Option<f64> {
    number_field(record, "orderIndex")
}

/// JS `String.prototype.trim()` and Rust `str::trim()` disagree on exactly one code point
/// this data can plausibly contain: U+FEFF. JS trims it, Rust does not. The group name a
/// user typed into a dialog goes through the JS rule, so the port adds it — otherwise a
/// pasted `\u{FEFF}Work` names a group on one client and a different group on the other.
fn trim_js_equivalent(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// `normalizeProjectGroupName`: trim, and empty (or non-string) is no group at all.
pub fn normalize_group_name(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    let trimmed = trim_js_equivalent(text);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// `normalizeProjectGroupNames`: order-preserving de-duplication of a configured list.
pub fn normalize_group_names(value: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let Some(items) = value.as_array() else {
        return names;
    };
    for item in items {
        if let Some(name) = normalize_group_name(Some(item)) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// `mergeProjectGroupNames`: configured names first, then any group a project mentions.
///
/// The configured list is normalised *first*, exactly as TypeScript does: `["Work",
/// "Work", "  "]` is one group, not three sections. A parity case caught this — the port
/// had cloned the raw list and produced duplicate and blank sections.
pub fn merge_group_names(projects: &[Record], configured: &[String]) -> Vec<String> {
    let mut names = normalize_group_names(&Value::Array(
        configured
            .iter()
            .map(|name| Value::from(name.clone()))
            .collect(),
    ));
    for project in projects {
        if let Some(group) = normalize_group_name(project.get("group")) {
            if !names.contains(&group) {
                names.push(group);
            }
        }
    }
    names
}

/// `UNGROUPED_PROJECT_GROUP`.
pub const UNGROUPED: &str = "__ungrouped__";

/// One rail section: a group name and the projects under it, in the order they arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupView {
    pub name: String,
    pub is_ungrouped: bool,
    /// Ids, not records: the view is a *shape*, and holding ids keeps a caller from
    /// mutating a copy and calling it saved.
    pub project_ids: Vec<String>,
}

/// `groupProjectsForRail`. Sections appear in `mergeProjectGroupNames` order, and the
/// ungrouped bucket is appended only when it has something in it.
pub fn group_projects_for_rail(projects: &[Record], configured: &[String]) -> Vec<GroupView> {
    let mut names = merge_group_names(projects, configured);
    let mut views: Vec<GroupView> = names
        .iter()
        .map(|name| GroupView {
            name: name.clone(),
            is_ungrouped: false,
            project_ids: Vec::new(),
        })
        .collect();
    let mut ungrouped = GroupView {
        name: UNGROUPED.to_string(),
        is_ungrouped: true,
        project_ids: Vec::new(),
    };
    for project in projects {
        let id = text(project, "id").unwrap_or_default().to_string();
        match normalize_group_name(project.get("group")) {
            Some(group) => {
                if names.contains(&group) {
                    if let Some(view) = views.iter_mut().find(|view| view.name == group) {
                        view.project_ids.push(id);
                    }
                } else {
                    // Unreachable through `merge_group_names`, but the TypeScript version
                    // keeps a `groups.set(group, [project])` fallback for the same case.
                    // Porting the fallback keeps the two honest about the same odd input
                    // instead of letting one panic or drop the project.
                    names.push(group.clone());
                    views.push(GroupView {
                        name: group,
                        is_ungrouped: false,
                        project_ids: vec![id],
                    });
                }
            }
            None => ungrouped.project_ids.push(id),
        }
    }
    if !ungrouped.project_ids.is_empty() {
        views.push(ungrouped);
    }
    views
}

/// `projectGroupForProject`.
pub fn group_of(project: &Record) -> String {
    normalize_group_name(project.get("group")).unwrap_or_else(|| UNGROUPED.to_string())
}

/// `normalizeProjectOrder`: give every un-ordered project the index of its position.
/// Returns `changed == false` (and the untouched input) when nothing needed assigning,
/// because the client uses that identity to avoid a redundant save.
pub fn normalize_order(projects: &[Record]) -> (Vec<Record>, bool) {
    let mut changed = false;
    let normalized = projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            if finite_order_index(project).is_some() {
                return project.clone();
            }
            changed = true;
            let mut next = project.clone();
            set_order_index(&mut next, index as f64);
            next
        })
        .collect();
    (normalized, changed)
}

fn set_order_index(record: &mut Record, value: f64) {
    let as_int = value.trunc();
    let number = if as_int == value && value.abs() <= (i64::MAX as f64) {
        Value::from(as_int as i64)
    } else {
        // A fractional index can only have come from the client. Carrying it as a number
        // rather than rounding keeps `changed` logic and comparator results identical.
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    };
    record.0.insert("orderIndex".to_string(), number);
}

/// `sortProjectsForRail`: pinned first, then by `orderIndex`, ties broken by position in
/// the input. The sort must be stable and the tie-break explicit — a drag-and-drop list
/// that reorders equal elements on every render is a visible bug.
pub fn sort_for_rail(projects: &[Record]) -> Vec<Record> {
    let mut entries: Vec<(usize, bool, Option<f64>, Record)> = projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            (
                index,
                bool_field(project, "pinned"),
                finite_order_index(project),
                project.clone(),
            )
        })
        .collect();
    entries.sort_by(|a, b| {
        if a.1 != b.1 {
            // pinned before unpinned
            return if a.1 {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        let a_order = a.2.unwrap_or(MAX_SAFE_INDEX);
        let b_order = b.2.unwrap_or(MAX_SAFE_INDEX);
        if a.2.is_some() || b.2.is_some() {
            // The client subtracts and relies on the sign; `f64::sub` then compare is the
            // same arithmetic, and NaN cannot appear here (JSON has no NaN).
            return a_order
                .partial_cmp(&b_order)
                .unwrap_or(std::cmp::Ordering::Equal);
        }
        a.0.cmp(&b.0)
    });
    entries
        .into_iter()
        .map(|(_, _, _, project)| project)
        .collect()
}

/// `applyProjectOrder`: the requested ids first in the given order (unknown ids are
/// dropped, exactly as the client's `filter` does), then everything else in rail order,
/// then the whole list is re-indexed from zero.
pub fn apply_order(projects: &[Record], ordered_ids: &[String]) -> Vec<Record> {
    let mut requested: Vec<&str> = Vec::with_capacity(ordered_ids.len());
    for id in ordered_ids.iter().map(String::as_str) {
        // A duplicated id used to duplicate the record (pinned by a parity case); the fix is
        // first-occurrence dedup, applied identically in the shipping client.
        if !requested.contains(&id) {
            requested.push(id);
        }
    }
    let mut out: Vec<Record> = Vec::with_capacity(projects.len());
    for id in &requested {
        if let Some(found) = projects
            .iter()
            .find(|project| text(project, "id") == Some(id))
        {
            out.push(found.clone());
        }
    }
    let seen = ordered_ids;
    for project in sort_for_rail(projects) {
        let id = text(&project, "id").unwrap_or_default();
        if !seen.iter().any(|wanted| wanted == id) {
            out.push(project);
        }
    }
    for (index, project) in out.iter_mut().enumerate() {
        set_order_index(project, index as f64);
    }
    out
}

/// The location a saved record points at, as the *contract* type — not a second
/// definition of the same idea. `ProjectTarget::identity_key` is the one place a location
/// is keyed, so `upsertSsh` and a future `target`-parameterised RPC cannot drift apart
/// about what "the same project" means.
///
/// A `location` that is absent, or a shape this build cannot name, resolves to local on the
/// legacy `path` field — the same fallback `resolveProjectLocation` uses. The one place the
/// two differ is a *malformed* `location`: TypeScript would hand the odd object straight
/// through, while the core has no type to represent it and so treats the record as local.
/// Nothing may silently invent a remote identity from a value it cannot read.
pub fn resolve_location(project: &Record) -> ProjectTarget {
    if let Some(location) = project.get("location") {
        if let Ok(target) = serde_json::from_value::<ProjectTarget>(location.clone()) {
            return target;
        }
    }
    ProjectTarget::Local {
        path: text(project, "path").unwrap_or_default().to_string(),
    }
}

/// Lossless local-path normalisation (ADR-013): drop redundant trailing separators and let
/// `\` stand for `/`. Deliberately **not** case folding and **not** `realpath` — those change
/// meaning (two directories that differ only by case are two projects on a case-sensitive
/// volume, and resolving symlinks would merge distinct working copies).
pub fn normalise_local_path(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    if slashed == "/" {
        return slashed;
    }
    slashed.trim_end_matches('/').to_string()
}

/// The identity two projects are compared by. `ssh://…` in `path` is a *display* string;
/// feeding it to a filesystem API is the mistake this helper exists to prevent.
pub fn identity_of(project: &Record) -> String {
    match resolve_location(project) {
        ProjectTarget::Local { path } => format!("local:{}", normalise_local_path(&path)),
        target => target.identity_key(),
    }
}

/// Whether a record's location can be *acted on*. An unrecognised `location.kind` is
/// addressable=false (ADR-013): listing and removing it stay possible, everything that needs
/// to know where the project lives refuses instead of guessing local (the shipping client
/// guesses remote there, and then dispatches a bare local command).
pub fn is_addressable(project: &Record) -> bool {
    match project.get("location") {
        None => true,
        Some(location) => serde_json::from_value::<ProjectTarget>(location.clone()).is_ok(),
    }
}

/// `sshProjectPath` — the *display* path. It starts with `ssh://`, so nothing may feed it
/// to a path API; the identity is `Location`, not this string.
pub fn ssh_path(connection_id: &str, remote_path: &str) -> String {
    let normalized = if remote_path.starts_with('/') {
        remote_path.to_string()
    } else {
        format!("/{remote_path}")
    };
    format!("ssh://{connection_id}{normalized}")
}

/// `wslProjectPath`. The distribution is percent-encoded by the client, which is the only
/// place a `/` inside a distribution name can survive.
pub fn wsl_path(distribution: &str, linux_path: &str) -> String {
    let normalized = if linux_path.starts_with('/') {
        linux_path.to_string()
    } else {
        format!("/{linux_path}")
    };
    format!("wsl://{}{normalized}", encode_uri_component(distribution))
}

/// `normalizeRemotePath`: trimmed, then absolute.
pub fn normalize_remote_path(path: &str) -> String {
    let trimmed = trim_js_equivalent(path);
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// `normalizeWslProjectPath`: keep the root, otherwise drop trailing slashes.
pub fn normalize_wsl_path(path: &str) -> String {
    let absolute = normalize_remote_path(path);
    if absolute == "/" {
        return "/".to_string();
    }
    absolute.trim_end_matches('/').to_string()
}

/// Minimal `encodeURIComponent` for the characters that can actually appear in a WSL
/// distribution name (letters, digits, `.`, `-`, `_` pass through; everything else is
/// UTF-8 percent-encoded, matching JS for the non-ASCII names users do have).
fn encode_uri_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(*byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// `deriveProjectName`: last path segment, either separator, after dropping trailing ones.
pub fn derive_project_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return path.to_string();
    }
    let last = trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_string();
    if last.is_empty() {
        path.to_string()
    } else {
        last
    }
}

/// What an upsert decided, so a caller can report "reopened" vs "added" and, in the
/// service, skip writing when nothing moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upserted {
    pub project: Record,
    pub projects: Vec<Record>,
    /// false when a new project was created, true when an existing one was touched.
    pub reused: bool,
}

fn record_map(fields: Vec<(&str, Value)>) -> Map<String, Value> {
    let mut map = Map::new();
    for (key, value) in fields {
        map.insert(key.to_string(), value);
    }
    map
}

/// `upsertLocalProject`: matched on the **literal** `path` string, as today. No
/// canonicalisation is introduced here — that would silently merge two entries the
/// shipping client keeps apart, and it is a product decision, not a migration detail.
pub fn upsert_local(projects: &[Record], path: &str, new_id: &str, now_ms: i64) -> Upserted {
    let wanted = format!("local:{}", normalise_local_path(path));
    if let Some(existing) = projects.iter().find(|p| identity_of(p) == wanted) {
        let mut project = existing.clone();
        project
            .0
            .insert("lastOpenedAt".to_string(), Value::from(now_ms));
        let next = projects
            .iter()
            .map(|p| {
                if identity_of(p) == wanted {
                    project.clone()
                } else {
                    p.clone()
                }
            })
            .collect();
        return Upserted {
            project,
            projects: next,
            reused: true,
        };
    }
    let project = Record(record_map(vec![
        ("id", Value::from(new_id)),
        ("name", Value::from(derive_project_name(path))),
        ("path", Value::from(path)),
        ("lastOpenedAt", Value::from(now_ms)),
        ("orderIndex", Value::from(0)),
    ]));
    prepend_and_reindex(projects, project)
}

/// The identity an SSH project is matched by (`upsertSshProject`): connection + remote
/// path, never the `ssh://` string.
pub fn upsert_ssh(
    projects: &[Record],
    name: &str,
    connection_id: &str,
    remote_path: &str,
    new_id: &str,
    now_ms: i64,
) -> Upserted {
    let remote_path = normalize_remote_path(remote_path);
    let path = ssh_path(connection_id, &remote_path);
    let wanted = ProjectTarget::Ssh {
        connection_id: connection_id.to_string(),
        remote_path: remote_path.clone(),
    }
    .identity_key();
    let matches = |project: &Record| resolve_location(project).identity_key() == wanted;
    if let Some(existing) = projects.iter().find(|p| matches(p)) {
        let mut project = existing.clone();
        project
            .0
            .insert("path".to_string(), Value::from(path.clone()));
        project
            .0
            .insert("lastOpenedAt".to_string(), Value::from(now_ms));
        let id = text(existing, "id").unwrap_or_default().to_string();
        let next = projects
            .iter()
            .map(|p| {
                if text(p, "id") == Some(id.as_str()) {
                    project.clone()
                } else {
                    p.clone()
                }
            })
            .collect();
        return Upserted {
            project,
            projects: next,
            reused: true,
        };
    }
    let project = Record(record_map(vec![
        ("id", Value::from(new_id)),
        ("name", Value::from(name)),
        ("path", Value::from(path)),
        (
            "location",
            Value::Object(record_map(vec![
                ("kind", Value::from("ssh")),
                ("connectionId", Value::from(connection_id)),
                ("remotePath", Value::from(remote_path)),
            ])),
        ),
        ("lastOpenedAt", Value::from(now_ms)),
        ("orderIndex", Value::from(0)),
    ]));
    prepend_and_reindex(projects, project)
}

/// `upsertWslProject` — same behaviour as SSH, on distribution + Linux path.
pub fn upsert_wsl(
    projects: &[Record],
    name: &str,
    distribution: &str,
    linux_path: &str,
    new_id: &str,
    now_ms: i64,
) -> Upserted {
    let linux_path = normalize_wsl_path(linux_path);
    let path = wsl_path(distribution, &linux_path);
    let wanted = ProjectTarget::Wsl {
        distribution: distribution.to_string(),
        linux_path: linux_path.clone(),
    }
    .identity_key();
    let matches = |project: &Record| resolve_location(project).identity_key() == wanted;
    if let Some(existing) = projects.iter().find(|p| matches(p)) {
        let mut project = existing.clone();
        project.0.insert("path".to_string(), Value::from(path));
        project
            .0
            .insert("lastOpenedAt".to_string(), Value::from(now_ms));
        let id = text(existing, "id").unwrap_or_default().to_string();
        let next = projects
            .iter()
            .map(|p| {
                if text(p, "id") == Some(id.as_str()) {
                    project.clone()
                } else {
                    p.clone()
                }
            })
            .collect();
        return Upserted {
            project,
            projects: next,
            reused: true,
        };
    }
    let project = Record(record_map(vec![
        ("id", Value::from(new_id)),
        ("name", Value::from(name)),
        ("path", Value::from(path)),
        (
            "location",
            Value::Object(record_map(vec![
                ("kind", Value::from("wsl")),
                ("distribution", Value::from(distribution)),
                ("linuxPath", Value::from(linux_path)),
            ])),
        ),
        ("lastOpenedAt", Value::from(now_ms)),
        ("orderIndex", Value::from(0)),
    ]));
    prepend_and_reindex(projects, project)
}

fn prepend_and_reindex(projects: &[Record], project: Record) -> Upserted {
    let mut prefixed = vec![project.clone()];
    prefixed.extend(projects.iter().cloned());
    let (normalized, _) = normalize_order(&prefixed);
    let next = normalized
        .into_iter()
        .enumerate()
        .map(|(index, mut record)| {
            set_order_index(&mut record, index as f64);
            record
        })
        .collect();
    Upserted {
        project,
        projects: next,
        reused: false,
    }
}

/// `touchProjectInList`: update `lastOpenedAt` in place, order untouched. A missing id
/// changes nothing — the client's `undefined` return is not a state the core may invent.
pub fn touch(projects: &[Record], project_id: &str, now_ms: i64) -> Upserted {
    let mut next = projects.to_vec();
    let mut updated: Option<Record> = None;
    if let Some(found) = next
        .iter_mut()
        .find(|record| text(record, "id") == Some(project_id))
    {
        found
            .0
            .insert("lastOpenedAt".to_string(), Value::from(now_ms));
        updated = Some(found.clone());
    }
    Upserted {
        project: updated.clone().unwrap_or_default(),
        projects: next,
        reused: updated.is_some(),
    }
}

// The single-field writers below all share one rule: rewrite only the named key, and do
// not reorder, reindex, or reformat anything else.

/// Copy the list, then apply `change` to the one record whose id matches. A copy is the
/// honest cost here: the client's `projects.map(...)` produces a new array too, and sharing
/// one mutable list between the two would let a failed later step leave a half-edit.
fn map_one(projects: &[Record], project_id: &str, change: impl FnOnce(&mut Record)) -> Vec<Record> {
    let mut next = projects.to_vec();
    if let Some(found) = next
        .iter_mut()
        .find(|record| text(record, "id") == Some(project_id))
    {
        change(found);
    }
    next
}

pub fn assign_group(projects: &[Record], project_id: &str, group: Option<&Value>) -> Vec<Record> {
    let normalized = normalize_group_name(group);
    map_one(projects, project_id, |record| match normalized {
        // JS writes `group: undefined`, which disappears at serialisation; the equivalent
        // here is removing the key, not storing null.
        Some(name) => {
            record.0.insert("group".to_string(), Value::from(name));
        }
        None => {
            record.0.remove("group");
        }
    })
}

pub fn rename_group(projects: &[Record], old_name: &str, next_name: &str) -> (Vec<Record>, bool) {
    let Some(normalized) = normalize_group_name(Some(&Value::from(next_name))) else {
        return (projects.to_vec(), false);
    };
    if normalized == old_name {
        return (projects.to_vec(), false);
    }
    let next = projects
        .iter()
        .map(|project| {
            if text(project, "group") != Some(old_name) {
                return project.clone();
            }
            let mut renamed = project.clone();
            renamed
                .0
                .insert("group".to_string(), Value::from(normalized.clone()));
            renamed
        })
        .collect();
    (next, true)
}

pub fn clear_group(projects: &[Record], group_name: &str) -> Vec<Record> {
    projects
        .iter()
        .map(|project| {
            if text(project, "group") != Some(group_name) {
                return project.clone();
            }
            let mut cleared = project.clone();
            cleared.0.remove("group");
            cleared
        })
        .collect()
}

pub fn rename(projects: &[Record], project_id: &str, name: &str) -> Vec<Record> {
    map_one(projects, project_id, |record| {
        record.0.insert("name".to_string(), Value::from(name));
    })
}

pub fn set_avatar(projects: &[Record], project_id: &str, avatar: Option<Value>) -> Vec<Record> {
    map_one(projects, project_id, |record| match avatar {
        Some(value) => {
            record.0.insert("avatar".to_string(), value);
        }
        None => {
            record.0.remove("avatar");
        }
    })
}

/// Display fields the sidebar's 更多 menu sets (2026-10-28): the folder
/// icon's tint and the Agent a new session starts with. `None` clears it.
pub const META_KEYS: [&str; 2] = ["color", "defaultAgent"];

pub fn set_meta(
    projects: &[Record],
    project_id: &str,
    key: &str,
    value: Option<Value>,
) -> Vec<Record> {
    map_one(projects, project_id, |record| match value {
        Some(value) => {
            record.0.insert(key.to_string(), value);
        }
        None => {
            record.0.remove(key);
        }
    })
}

pub fn toggle_hidden(projects: &[Record], project_id: &str) -> Vec<Record> {
    map_one(projects, project_id, |record| {
        let next = !bool_field(record, "hiddenFromRail");
        record
            .0
            .insert("hiddenFromRail".to_string(), Value::Bool(next));
    })
}

/// `toggleProjectPinnedInList` + `applyProjectPinnedChange`: an unknown id returns the
/// input unchanged, and a no-op toggle (same value already set) returns it unchanged too —
/// which the service uses to avoid a pointless write.
pub fn toggle_pinned(projects: &[Record], project_id: &str) -> (Vec<Record>, bool) {
    let Some(current) = projects.iter().find(|p| text(p, "id") == Some(project_id)) else {
        return (projects.to_vec(), false);
    };
    let next_pinned = !bool_field(current, "pinned");
    set_pinned(projects, project_id, next_pinned)
}

pub fn set_pinned(projects: &[Record], project_id: &str, pinned: bool) -> (Vec<Record>, bool) {
    let Some(current) = projects.iter().find(|p| text(p, "id") == Some(project_id)) else {
        return (projects.to_vec(), false);
    };
    if bool_field(current, "pinned") == pinned {
        return (projects.to_vec(), false);
    }
    (
        map_one(projects, project_id, |record| {
            record.0.insert("pinned".to_string(), Value::Bool(pinned));
        }),
        true,
    )
}

/// Remove a project record. Deleting anything on disk is a separate, explicitly-confirmed
/// operation (plan P2-03 Action 3), which is why this returns the record it dropped rather
/// than touching a path.
pub fn remove(projects: &[Record], project_id: &str) -> (Vec<Record>, Option<Record>) {
    let mut removed = None;
    let next: Vec<Record> = projects
        .iter()
        .filter(|project| {
            let drop = text(project, "id") == Some(project_id);
            if drop {
                removed = Some((*project).clone());
            }
            !drop
        })
        .cloned()
        .collect();
    (next, removed)
}

/// `sort_for_rail` + `group_projects_for_rail` together: what the rail renders.
pub fn visible_in_groups(
    projects: &[Record],
    configured: &[String],
) -> Vec<(GroupView, Vec<Record>)> {
    let sorted = sort_for_rail(projects);
    group_projects_for_rail(&sorted, configured)
        .into_iter()
        .map(|mut view| {
            let records: Vec<Record> = view
                .project_ids
                .iter()
                .filter_map(|id| {
                    sorted
                        .iter()
                        .find(|project| text(project, "id") == Some(id.as_str()))
                        .cloned()
                })
                .collect();
            view.project_ids = records
                .iter()
                .filter_map(|record| text(record, "id").map(str::to_string))
                .collect();
            (view, records)
        })
        .collect()
}

/// Field access the service needs to enforce the removal policy without re-declaring the
/// record shape.
pub fn id_of(project: &Record) -> Option<&str> {
    text(project, "id")
}

pub fn is_hidden(project: &Record) -> bool {
    bool_field(project, "hiddenFromRail")
}
