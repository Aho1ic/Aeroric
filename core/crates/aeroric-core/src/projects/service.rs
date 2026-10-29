//! The project write path: what the rail shows, and what it costs to change it.
//!
//! Every mutating method here goes through the data-directory lease and then bumps
//! `runtime.json`'s revision. That wiring is the point of this file: Task P2-02 delivered
//! the mechanism and explicitly did not claim the semantics, and ADR-012 made "a write path
//! really calls it" P2-03's own acceptance item. A no-op mutation must therefore **not**
//! consume a revision — a client that toggles a pin to the value it already has changed
//! nothing, and pretending otherwise would make the counter meaningless as a staleness
//! signal.

use std::sync::Arc;

use aeroric_protocol::TaskStatus;

use crate::context::CoreContext;
use crate::projects::mutations::{self, Upserted};
use crate::storage::runtime::{self, Bumped};
use crate::storage::{Record, StorageError};
use serde_json::Value;

/// Keep the stored group list tidy after a project's group changes: the name a project now
/// carries must exist in the list, and nothing is dropped just because no project points at
/// it right now (an empty group is a thing users create on purpose).
fn normalize_owned(
    stored: &[String],
    group: Option<&serde_json::Value>,
    _after: &[Record],
) -> Vec<String> {
    let mut names: Vec<String> = stored.to_vec();
    if let Some(name) = mutations::normalize_group_name(group) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The configured group list, as the client keeps it (`localStorage` today; the host's
/// settings will carry it once P3-06 owns persistence). An empty list is a valid answer:
/// groups discovered from the projects themselves still render.
#[derive(Clone, Debug, Default)]
pub struct GroupConfig {
    pub names: Vec<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum ProjectError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("project {project_id:?} is not in the saved list")]
    NotFound { project_id: String },
    /// ADR-013: a record whose `location.kind` this build cannot name may be listed and
    /// removed, but anything that needs to know where the project lives refuses. Guessing
    /// "local" would run an agent in the wrong directory; the shipping client's guess
    /// ("remote, but with no remote command channel") is no better.
    #[error("project {project_id:?} has an unrecognised location kind {kind:?}; it can be listed or removed, nothing else")]
    Unaddressable { project_id: String, kind: String },
    #[error("two project records share id {project_id:?}; refusing to write a list with a duplicate identity")]
    DuplicateId { project_id: String },
    #[error(
        "project {project_id:?} still has {live_count} live task(s) ({statuses}); remove them first"
    )]
    TasksStillRunning {
        project_id: String,
        live_count: usize,
        statuses: String,
    },
}

/// What a mutation cost: whether anything was written, and the revision it produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub projects: Vec<Record>,
    pub revision: Option<String>,
    /// false means the operation changed nothing, so nothing was written and the revision
    /// did not move. Callers report this honestly instead of claiming a save.
    pub changed: bool,
    /// The group list after the operation, in display order. The shipping client returns
    /// `groupsNext: []` and re-reads `localStorage`; here it is the real list, because the
    /// list is now core-owned (`ADR-013` decision 4) and a caller must not have to guess.
    pub groups_next: Vec<String>,
}

/// The rail's shape for one saved state: groups in display order, each with its projects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rail {
    pub groups: Vec<RailGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RailGroup {
    pub name: String,
    pub is_ungrouped: bool,
    pub projects: Vec<Record>,
}

pub struct ProjectService {
    context: Arc<CoreContext>,
}

impl ProjectService {
    pub fn new(context: Arc<CoreContext>) -> Self {
        Self { context }
    }

    pub fn context(&self) -> &CoreContext {
        &self.context
    }

    /// The core-owned group list (`project-groups.json`), verbatim: trimmed and
    /// de-duplicated on write, never rewritten on read, so a caller sees what is on disk.
    /// An absent file is "no configured groups" — *not* an error, because groups discovered
    /// from the project records still render.
    pub fn stored_groups(&self) -> Result<Vec<String>, ProjectError> {
        let path = self.context.paths.project_groups_file();
        Ok(match crate::storage::atomic::read_json(&path)? {
            crate::storage::ReadOutcome::Absent => Vec::new(),
            crate::storage::ReadOutcome::Found(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect(),
            crate::storage::ReadOutcome::Found(other) => {
                return Err(ProjectError::from(StorageError::Corrupt {
                    path,
                    reason: format!("expected an array of group names, found {other}"),
                }));
            }
        })
    }

    fn write_groups(&self, names: &[String]) -> Result<(), ProjectError> {
        let path = self.context.paths.project_groups_file();
        let body = Value::Array(names.iter().cloned().map(Value::from).collect());
        let bytes = {
            let text = serde_json::to_string_pretty(&body).expect("a string array serialises");
            format!("{text}\n").into_bytes()
        };
        crate::storage::atomic::ensure_replaceable(&path)?;
        crate::storage::atomic::write_atomic(&path, &bytes, crate::storage::FileMode::Private)?;
        Ok(())
    }

    /// Stored list plus any group a project mentions, in display order.
    pub fn configured_groups(&self) -> Result<Vec<String>, ProjectError> {
        Ok(mutations::merge_group_names(
            &self.list()?,
            &self.stored_groups()?,
        ))
    }

    /// The saved projects, in file order. No window, no emitter, no UI callback: this is
    /// the call `aeroric-cli projects list` lands on.
    pub fn list(&self) -> Result<Vec<Record>, ProjectError> {
        Ok(crate::storage::load_projects(&self.context)?)
    }

    /// The rail exactly as the client computes it: pinned-first order, then grouped, with
    /// hidden projects left in (the client filters them at render, and dropping them here
    /// would make "hide" a data change rather than a view change).
    pub fn rail(&self, configured: &GroupConfig) -> Result<Rail, ProjectError> {
        let projects = self.list()?;
        let visible = mutations::visible_in_groups(&projects, &configured.names);
        Ok(Rail {
            groups: visible
                .into_iter()
                .map(|(view, records)| RailGroup {
                    name: view.name,
                    is_ungrouped: view.is_ungrouped,
                    projects: records,
                })
                .collect(),
        })
    }

    /// Groups that exist by configuration plus discovery, in display order.
    pub fn group_names(&self, configured: &GroupConfig) -> Result<Vec<String>, ProjectError> {
        Ok(mutations::merge_group_names(
            &self.list()?,
            &configured.names,
        ))
    }

    pub fn group_view_of(&self, project: &Record) -> String {
        mutations::group_of(project)
    }

    /// Open (or re-open) a local project. Re-opening the *same* path must not create a
    /// second entry. Matching is on the **normalised** local path (ADR-013), so `/repo/one/`
    /// and `/repo/one` touch the same record; case differences and symlinked spellings stay
    /// separate, and two records that already exist are never merged.
    pub fn open_local(
        &self,
        path: &str,
        new_id: &str,
        now_ms: i64,
    ) -> Result<(Upserted, Applied), ProjectError> {
        let projects = self.list()?;
        let upserted = mutations::upsert_local(&projects, path, new_id, now_ms);
        if !upserted.reused {
            self.require_addressable(&upserted.project)?;
        }
        let applied = self.persist(upserted.projects.clone(), true)?;
        Ok((upserted, applied))
    }

    pub fn open_ssh(
        &self,
        name: &str,
        connection_id: &str,
        remote_path: &str,
        new_id: &str,
        now_ms: i64,
    ) -> Result<(Upserted, Applied), ProjectError> {
        let projects = self.list()?;
        let upserted =
            mutations::upsert_ssh(&projects, name, connection_id, remote_path, new_id, now_ms);
        self.require_addressable(&upserted.project)?;
        let applied = self.persist(upserted.projects.clone(), true)?;
        Ok((upserted, applied))
    }

    pub fn open_wsl(
        &self,
        name: &str,
        distribution: &str,
        linux_path: &str,
        new_id: &str,
        now_ms: i64,
    ) -> Result<(Upserted, Applied), ProjectError> {
        let projects = self.list()?;
        let upserted =
            mutations::upsert_wsl(&projects, name, distribution, linux_path, new_id, now_ms);
        self.require_addressable(&upserted.project)?;
        let applied = self.persist(upserted.projects.clone(), true)?;
        Ok((upserted, applied))
    }

    pub fn touch(&self, project_id: &str, now_ms: i64) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        self.require_addressable_by_id(&projects, project_id)?;
        if projects
            .iter()
            .all(|project| mutations::id_of(project) != Some(project_id))
        {
            return Err(ProjectError::NotFound {
                project_id: project_id.to_string(),
            });
        }
        let upserted = mutations::touch(&projects, project_id, now_ms);
        self.persist(upserted.projects, true)
    }

    pub fn rename(&self, project_id: &str, name: &str) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        self.require_addressable_by_id(&projects, project_id)?;
        let next = mutations::rename(&self.list()?, project_id, name);
        self.persist(next, true)
    }

    pub fn set_avatar(
        &self,
        project_id: &str,
        avatar: Option<serde_json::Value>,
    ) -> Result<Applied, ProjectError> {
        self.require_project(project_id)?;
        let next = mutations::set_avatar(&self.list()?, project_id, avatar);
        self.persist(next, true)
    }

    /// One of [`mutations::META_KEYS`]; anything else is not the menu's to set.
    pub fn set_meta(
        &self,
        project_id: &str,
        key: &str,
        value: Option<serde_json::Value>,
    ) -> Result<Applied, ProjectError> {
        if !mutations::META_KEYS.contains(&key) {
            return Err(ProjectError::NotFound {
                project_id: format!("{project_id} (field {key})"),
            });
        }
        self.require_project(project_id)?;
        let next = mutations::set_meta(&self.list()?, project_id, key, value);
        self.persist(next, true)
    }

    /// Links a folder (2026-10-29): validated against the project's own path
    /// and its other links; stored as the canonical path. Returns the list.
    pub fn link_folder(&self, project_id: &str, path: &str) -> Result<Vec<String>, String> {
        let projects = self.list().map_err(|e| e.to_string())?;
        let project = projects
            .iter()
            .find(|p| mutations::id_of(p) == Some(project_id))
            .ok_or("项目不存在")?;
        let mut folders = super::linked::stored(project);
        let root = mutations::text(project, "path")
            .unwrap_or_default()
            .to_string();
        folders.push(super::linked::validate(&root, &folders, path)?);
        self.write_linked(&projects, project_id, &folders)?;
        Ok(folders)
    }

    /// Unlinks a folder (matching the stored path exactly).
    pub fn unlink_folder(&self, project_id: &str, path: &str) -> Result<Vec<String>, String> {
        let projects = self.list().map_err(|e| e.to_string())?;
        let project = projects
            .iter()
            .find(|p| mutations::id_of(p) == Some(project_id))
            .ok_or("项目不存在")?;
        let folders: Vec<String> = super::linked::stored(project)
            .into_iter()
            .filter(|p| p != path)
            .collect();
        self.write_linked(&projects, project_id, &folders)?;
        Ok(folders)
    }

    fn write_linked(
        &self,
        projects: &[Record],
        project_id: &str,
        folders: &[String],
    ) -> Result<(), String> {
        let value = (!folders.is_empty()).then(|| serde_json::json!(folders));
        let next = mutations::set_meta(projects, project_id, super::linked::KEY, value);
        self.persist(next, true)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn toggle_hidden(&self, project_id: &str) -> Result<Applied, ProjectError> {
        self.require_project(project_id)?;
        let next = mutations::toggle_hidden(&self.list()?, project_id);
        self.persist(next, true)
    }

    pub fn toggle_pinned(&self, project_id: &str) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        let (next, changed) = mutations::toggle_pinned(&projects, project_id);
        if !changed {
            return Err(ProjectError::NotFound {
                project_id: project_id.to_string(),
            });
        }
        self.persist(next, changed)
    }

    pub fn set_pinned(&self, project_id: &str, pinned: bool) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        let (next, changed) = mutations::set_pinned(&projects, project_id, pinned);
        self.persist(next, changed)
    }

    pub fn assign_group(
        &self,
        project_id: &str,
        group: Option<serde_json::Value>,
    ) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        self.require_addressable_by_id(&projects, project_id)?;
        let next = mutations::assign_group(&projects, project_id, group.as_ref());
        let stored = self.stored_groups()?;
        let discovered = normalize_owned(&stored, group.as_ref(), &next);
        self.persist_with_groups(next, true, stored, discovered)
    }

    pub fn rename_group(&self, old_name: &str, next_name: &str) -> Result<Applied, ProjectError> {
        let (next, changed) = mutations::rename_group(&self.list()?, old_name, next_name);
        let stored = self.stored_groups()?;
        let renamed: Vec<String> = stored
            .iter()
            .map(|name| {
                if name == old_name {
                    mutations::normalize_group_name(Some(&serde_json::Value::from(next_name)))
                        .unwrap_or_else(|| name.clone())
                } else {
                    name.clone()
                }
            })
            .collect();
        self.persist_with_groups(next, changed, stored, renamed)
    }

    /// Create an empty group. The client can do this today (an empty section survives in
    /// `localStorage`), so the core list must be writable without a project pointing at it.
    pub fn create_group(&self, name: &str) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        let stored = self.stored_groups()?;
        let Some(normalised) =
            mutations::normalize_group_name(Some(&serde_json::Value::from(name)))
        else {
            return self.unchanged(projects, stored);
        };
        let mut next = stored.clone();
        if next.contains(&normalised) {
            return self.unchanged(projects, stored);
        }
        next.push(normalised);
        self.persist_with_groups(projects, false, stored, next)
    }

    pub fn clear_group(&self, group_name: &str) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        let stored = self.stored_groups()?;
        if projects
            .iter()
            .all(|project| mutations::text(project, "group") != Some(group_name))
        {
            return self.unchanged(projects, stored);
        }
        let next = mutations::clear_group(&projects, group_name);
        let kept: Vec<String> = stored
            .iter()
            .filter(|name| name.as_str() != group_name)
            .cloned()
            .collect();
        self.persist_with_groups(next, true, stored, kept)
    }

    pub fn reorder(&self, ordered_ids: &[String]) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        for id in ordered_ids {
            self.require_addressable_by_id(&projects, id)?;
        }
        let next = mutations::apply_order(&projects, ordered_ids);
        self.persist(next, true)
    }

    /// Remove the *record*. Nothing on disk is deleted here: that is a separate, explicitly
    /// confirmed operation (plan P2-03 Action 3), and a project list entry is not a
    /// checkout.
    ///
    /// A project with live tasks is refused. "Live" is read through the frozen
    /// `TaskStatus` vocabulary rather than a string list invented here, so the core and the
    /// client cannot disagree about what still counts as running.
    pub fn remove(&self, project_id: &str) -> Result<Applied, ProjectError> {
        let projects = self.list()?;
        let live = self.live_task_statuses(project_id)?;
        if !live.is_empty() {
            let statuses = live
                .iter()
                .filter_map(|status| {
                    serde_json::to_value(status)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_string))
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ProjectError::TasksStillRunning {
                project_id: project_id.to_string(),
                live_count: live.len(),
                statuses,
            });
        }
        let (next, removed) = mutations::remove(&projects, project_id);
        let Some(_removed) = removed else {
            return Err(ProjectError::NotFound {
                project_id: project_id.to_string(),
            });
        };
        self.persist(next, true)
    }

    /// Which of a project's tasks still count as live, read through the frozen
    /// `TaskStatus` vocabulary. An unrecognised status is *not* treated as live: guessing
    /// "safer to block" would let a typo in an old file make a project undeletable, and
    /// `remove` is reversible while a refused deletion is a dead end for the user.
    fn live_task_statuses(&self, project_id: &str) -> Result<Vec<TaskStatus>, ProjectError> {
        let tasks = crate::storage::load_tasks(&self.context, project_id)?;
        Ok(tasks
            .iter()
            .filter_map(|task| {
                let status = task.str_field("status")?;
                serde_json::from_value::<TaskStatus>(serde_json::Value::String(status.to_string()))
                    .ok()
            })
            .filter(|status| status.is_live())
            .collect())
    }

    fn require_addressable(&self, project: &Record) -> Result<(), ProjectError> {
        if mutations::is_addressable(project) {
            return Ok(());
        }
        Err(ProjectError::Unaddressable {
            project_id: mutations::id_of(project).unwrap_or_default().to_string(),
            kind: project
                .get("location")
                .and_then(|location| location.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("(missing)")
                .to_string(),
        })
    }

    fn require_addressable_by_id(
        &self,
        projects: &[Record],
        project_id: &str,
    ) -> Result<(), ProjectError> {
        match projects
            .iter()
            .find(|project| mutations::id_of(project) == Some(project_id))
        {
            Some(project) => self.require_addressable(project),
            None => Err(ProjectError::NotFound {
                project_id: project_id.to_string(),
            }),
        }
    }

    /// The last thing between "a list we computed" and "a list on disk": two records with one
    /// id would make every later lookup hit the first, so a duplicate is a refusal, not a
    /// write. `id = String(Date.now())` made this reachable in the shipping client (ADR-013).
    fn assert_unique_ids(projects: &[Record]) -> Result<(), ProjectError> {
        let mut seen: Vec<&str> = Vec::with_capacity(projects.len());
        for project in projects {
            let Some(id) = mutations::id_of(project) else {
                return Err(ProjectError::DuplicateId {
                    project_id: "(missing id)".to_string(),
                });
            };
            if seen.contains(&id) {
                return Err(ProjectError::DuplicateId {
                    project_id: id.to_string(),
                });
            }
            seen.push(id);
        }
        Ok(())
    }

    fn require_project(&self, project_id: &str) -> Result<(), ProjectError> {
        if self
            .list()?
            .iter()
            .any(|project| mutations::id_of(project) == Some(project_id))
        {
            Ok(())
        } else {
            Err(ProjectError::NotFound {
                project_id: project_id.to_string(),
            })
        }
    }

    /// The one write path. `changed == false` skips both the file write and the revision
    /// bump, so an ineffective operation cannot be mistaken for new state.
    fn persist(&self, projects: Vec<Record>, changed: bool) -> Result<Applied, ProjectError> {
        let stored = self.stored_groups()?;
        self.persist_with_groups(projects, changed, stored.clone(), stored)
    }

    /// One write path for both files, so a group-list change and the project edits it belongs
    /// to cannot be observed apart. Order matters: the project list is the user's data and the
    /// group list is derived display state, so a failed project save leaves no half-state.
    fn persist_with_groups(
        &self,
        projects: Vec<Record>,
        changed: bool,
        before: Vec<String>,
        after: Vec<String>,
    ) -> Result<Applied, ProjectError> {
        Self::assert_unique_ids(&projects)?;
        let groups_changed = after != before;
        if !changed && !groups_changed {
            return self.unchanged(projects, after);
        }
        if changed {
            crate::storage::save_projects(&self.context, &projects)?;
        }
        if groups_changed {
            self.write_groups(&after)?;
        }
        let Bumped { metadata, .. } = runtime::bump_revision(&self.context)?;
        Ok(Applied {
            projects,
            revision: Some(metadata.revision),
            changed: true,
            groups_next: after,
        })
    }

    fn unchanged(
        &self,
        projects: Vec<Record>,
        groups: Vec<String>,
    ) -> Result<Applied, ProjectError> {
        let revision = match runtime::load(&self.context)? {
            crate::storage::ReadOutcome::Found(metadata) => Some(metadata.revision),
            crate::storage::ReadOutcome::Absent => None,
        };
        Ok(Applied {
            projects,
            revision,
            changed: false,
            groups_next: groups,
        })
    }
}

pub use mutations::UNGROUPED;
