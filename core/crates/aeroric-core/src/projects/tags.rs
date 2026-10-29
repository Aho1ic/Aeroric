//! 项目与会话标签（2026-10-29，T1–T4）。一套标签库：每个标签有名字、预设色
//! 和范围（所有会话，或仅某个项目）；`assignments` 把侧栏行的 id（项目 id、
//! `task.<id>`、`imported.<id>`）映射到它贴着的标签，`row_projects` 记下每行
//! 所在的项目，收窄范围时据此摘掉别处的标签。核心独占的 `tags.json`，读改写
//! 在一把进程锁里完成——宿主并发处理请求，两次贴标签不能互相覆盖。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::storage::{atomic, FileMode, ReadOutcome};

pub const TAG_NAME_MAX_CHARS: usize = 24;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    /// A `ProjectColor` id (`red`, `blue`, …).
    pub color: String,
    /// Set for a 仅当前项目 tag; absent for 所有会话.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TagLibrary {
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// Sidebar row id → tag ids, in library order.
    #[serde(default)]
    pub assignments: BTreeMap<String, Vec<String>>,
    /// Sidebar row id → the project it sat in when tagged (a project row is
    /// its own project). Narrowing a tag's scope takes it off rows elsewhere.
    #[serde(default)]
    pub row_projects: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TagError {
    #[error("{0}")]
    Invalid(String),
    #[error("没有这个标签")]
    NotFound,
    #[error("{0}")]
    Storage(String),
}

/// What a tag is being put on. Both take 所有会话 tags and their own
/// project's (a project is its own project; 2026-10-29 Q1-a).
pub enum TagTarget<'a> {
    Project { project_id: &'a str },
    Session { project_id: Option<&'a str> },
}

impl TagTarget<'_> {
    fn project_id(&self) -> Option<&str> {
        match self {
            Self::Project { project_id } => Some(project_id),
            Self::Session { project_id } => *project_id,
        }
    }
}

static LOCK: Mutex<()> = Mutex::new(());

pub struct TagStore {
    path: PathBuf,
}

impl TagStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<TagLibrary, TagError> {
        match atomic::read_json(&self.path).map_err(|e| TagError::Storage(e.to_string()))? {
            ReadOutcome::Absent => Ok(TagLibrary::default()),
            ReadOutcome::Found(value) => {
                serde_json::from_value(value).map_err(|e| TagError::Storage(e.to_string()))
            }
        }
    }

    fn save(&self, library: &TagLibrary) -> Result<(), TagError> {
        let text =
            serde_json::to_string_pretty(library).map_err(|e| TagError::Storage(e.to_string()))?;
        atomic::ensure_replaceable(&self.path).map_err(|e| TagError::Storage(e.to_string()))?;
        atomic::write_atomic(
            &self.path,
            format!("{text}\n").as_bytes(),
            FileMode::Private,
        )
        .map_err(|e| TagError::Storage(e.to_string()))
    }

    fn change<T>(
        &self,
        apply: impl FnOnce(&mut TagLibrary) -> Result<T, TagError>,
    ) -> Result<T, TagError> {
        let _guard = LOCK.lock();
        let mut library = self.load()?;
        let out = apply(&mut library)?;
        self.save(&library)?;
        Ok(out)
    }

    pub fn create(
        &self,
        name: &str,
        color: &str,
        project_id: Option<&str>,
    ) -> Result<Tag, TagError> {
        let name = clean_name(name)?;
        let color = clean_color(color)?;
        let project_id = clean_scope(project_id);
        self.change(|library| {
            if library
                .tags
                .iter()
                .any(|t| t.name == name && t.project_id == project_id)
            {
                return Err(TagError::Invalid(format!("已经有名为「{name}」的标签")));
            }
            let tag = Tag {
                id: format!("tag-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
                name,
                color,
                project_id,
            };
            library.tags.push(tag.clone());
            Ok(tag)
        })
    }

    /// `scope`: `Some(None)` = 所有会话, `Some(Some(p))` = 仅项目 p. Narrowing
    /// takes the tag off rows that sit in another project (or in none).
    pub fn update(
        &self,
        id: &str,
        name: Option<&str>,
        color: Option<&str>,
        scope: Option<Option<&str>>,
    ) -> Result<Tag, TagError> {
        let name = name.map(clean_name).transpose()?;
        let color = color.map(clean_color).transpose()?;
        let new_scope = scope.map(clean_scope);
        self.change(|library| {
            let current = library
                .tags
                .iter()
                .find(|t| t.id == id)
                .ok_or(TagError::NotFound)?;
            let scope_after = new_scope
                .clone()
                .unwrap_or_else(|| current.project_id.clone());
            let name_after = name.clone().unwrap_or_else(|| current.name.clone());
            if library
                .tags
                .iter()
                .any(|t| t.id != id && t.name == name_after && t.project_id == scope_after)
            {
                return Err(TagError::Invalid(format!(
                    "已经有名为「{name_after}」的标签"
                )));
            }
            if let Some(Some(project)) = &new_scope {
                let row_projects = library.row_projects.clone();
                for (row, tags) in library.assignments.iter_mut() {
                    if row_projects.get(row) != Some(project) {
                        tags.retain(|t| t != id);
                    }
                }
                prune(library);
            }
            let tag = library
                .tags
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or(TagError::NotFound)?;
            tag.name = name_after;
            if let Some(color) = color {
                tag.color = color;
            }
            tag.project_id = scope_after;
            Ok(tag.clone())
        })
    }

    /// Removes the tag and takes it off everything it was on.
    pub fn delete(&self, id: &str) -> Result<(), TagError> {
        self.change(|library| {
            let before = library.tags.len();
            library.tags.retain(|t| t.id != id);
            if library.tags.len() == before {
                return Err(TagError::NotFound);
            }
            for tags in library.assignments.values_mut() {
                tags.retain(|t| t != id);
            }
            prune(library);
            Ok(())
        })
    }

    /// Sets exactly which tags `target_id` carries.
    pub fn assign(
        &self,
        target_id: &str,
        target: TagTarget<'_>,
        tag_ids: &[String],
    ) -> Result<Vec<String>, TagError> {
        let target_id = target_id.trim();
        if target_id.is_empty() {
            return Err(TagError::Invalid("缺少要贴标签的项目或会话".into()));
        }
        self.change(|library| {
            for id in tag_ids {
                let tag = library
                    .tags
                    .iter()
                    .find(|t| &t.id == id)
                    .ok_or(TagError::NotFound)?;
                if let Some(scope) = tag.project_id.as_deref() {
                    if target.project_id() != Some(scope) {
                        return Err(TagError::Invalid(format!(
                            "「{}」只能用在它所属的项目里",
                            tag.name
                        )));
                    }
                }
            }
            // Library order, no duplicates.
            let chosen: Vec<String> = library
                .tags
                .iter()
                .filter(|t| tag_ids.contains(&t.id))
                .map(|t| t.id.clone())
                .collect();
            if chosen.is_empty() {
                library.assignments.remove(target_id);
            } else {
                library
                    .assignments
                    .insert(target_id.to_string(), chosen.clone());
            }
            match target.project_id() {
                Some(project) if !chosen.is_empty() => {
                    library
                        .row_projects
                        .insert(target_id.to_string(), project.to_string());
                }
                _ => {
                    library.row_projects.remove(target_id);
                }
            }
            Ok(chosen)
        })
    }
}

/// Drops rows left without tags, and their project records.
fn prune(library: &mut TagLibrary) {
    library.assignments.retain(|_, tags| !tags.is_empty());
    let rows = &library.assignments;
    library.row_projects.retain(|row, _| rows.contains_key(row));
}

fn clean_scope(project_id: Option<&str>) -> Option<String> {
    project_id
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
}

fn clean_name(name: &str) -> Result<String, TagError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TagError::Invalid("标签名不能为空".into()));
    }
    if name.chars().count() > TAG_NAME_MAX_CHARS {
        return Err(TagError::Invalid(format!(
            "标签名最多 {TAG_NAME_MAX_CHARS} 个字"
        )));
    }
    Ok(name.to_string())
}

fn clean_color(color: &str) -> Result<String, TagError> {
    const COLORS: [&str; 8] = [
        "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
    ];
    let color = color.trim();
    if COLORS.contains(&color) {
        Ok(color.to_string())
    } else {
        Err(TagError::Invalid(format!("未知的标签颜色 {color}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (TagStore, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("aeroric-tags-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        (TagStore::new(dir.join("tags.json")), dir)
    }

    fn session(project: &str) -> TagTarget<'_> {
        TagTarget::Session {
            project_id: Some(project),
        }
    }

    #[test]
    fn a_project_tag_only_goes_on_that_project_and_its_sessions() {
        let (store, dir) = store();
        let global = store.create("重要", "red", None).unwrap();
        let local = store.create("调研", "purple", Some("p1")).unwrap();
        let both = vec![global.id.clone(), local.id.clone()];
        assert_eq!(store.assign("task.a", session("p1"), &both).unwrap(), both);
        assert!(store.assign("task.b", session("p2"), &both).is_err());
        assert!(store
            .assign(
                "p2",
                TagTarget::Project { project_id: "p2" },
                std::slice::from_ref(&local.id)
            )
            .is_err());
        // A project carries its own 仅本项目 tags (Q1-a).
        store
            .assign(
                "p1",
                TagTarget::Project { project_id: "p1" },
                std::slice::from_ref(&local.id),
            )
            .unwrap();
        // Imported sessions are rows like any other.
        store
            .assign(
                "imported.imp_x",
                TagTarget::Session { project_id: None },
                std::slice::from_ref(&global.id),
            )
            .unwrap();
        let library = store.load().unwrap();
        assert_eq!(library.assignments.len(), 3);
        assert_eq!(
            library.row_projects.get("task.a").map(String::as_str),
            Some("p1")
        );
        assert!(!library.row_projects.contains_key("imported.imp_x"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn deleting_a_tag_takes_it_off_everything() {
        let (store, dir) = store();
        let a = store.create("甲", "blue", None).unwrap();
        let b = store.create("乙", "green", None).unwrap();
        store
            .assign(
                "task.a",
                TagTarget::Session { project_id: None },
                &[a.id.clone(), b.id.clone()],
            )
            .unwrap();
        store
            .assign(
                "p1",
                TagTarget::Project { project_id: "p1" },
                std::slice::from_ref(&a.id),
            )
            .unwrap();
        store.delete(&a.id).unwrap();
        let library = store.load().unwrap();
        assert_eq!(library.tags, vec![b.clone()]);
        assert_eq!(library.assignments.get("task.a"), Some(&vec![b.id.clone()]));
        assert!(
            !library.assignments.contains_key("p1"),
            "an emptied row drops out"
        );
        assert!(!library.row_projects.contains_key("p1"));
        assert_eq!(store.delete(&a.id), Err(TagError::NotFound));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn names_are_unique_per_scope_and_colors_are_presets() {
        let (store, dir) = store();
        store.create("重要", "red", None).unwrap();
        assert!(store.create(" 重要 ", "blue", None).is_err());
        assert!(
            store.create("重要", "blue", Some("p1")).is_ok(),
            "another scope may reuse it"
        );
        assert!(store.create("新", "magenta", None).is_err());
        assert!(store.create("   ", "red", None).is_err());
        let tag = store.create("旧名", "red", None).unwrap();
        let renamed = store
            .update(&tag.id, Some("新名"), Some("teal"), None)
            .unwrap();
        assert_eq!(
            (renamed.name.as_str(), renamed.color.as_str()),
            ("新名", "teal")
        );
        // Moving into a scope that already has the name is refused.
        assert!(store
            .update(&tag.id, Some("重要"), None, Some(Some("p1")))
            .is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn narrowing_a_scope_takes_the_tag_off_other_projects() {
        let (store, dir) = store();
        let tag = store.create("部署", "green", None).unwrap();
        let ids = std::slice::from_ref(&tag.id);
        store.assign("task.a", session("p1"), ids).unwrap();
        store.assign("task.b", session("p2"), ids).unwrap();
        store
            .assign("p1", TagTarget::Project { project_id: "p1" }, ids)
            .unwrap();
        store
            .assign("imported.x", TagTarget::Session { project_id: None }, ids)
            .unwrap();
        let narrowed = store.update(&tag.id, None, None, Some(Some("p1"))).unwrap();
        assert_eq!(narrowed.project_id.as_deref(), Some("p1"));
        let library = store.load().unwrap();
        let rows: Vec<&str> = library.assignments.keys().map(String::as_str).collect();
        assert_eq!(rows, vec!["p1", "task.a"]);
        // Widening again removes nothing.
        store.update(&tag.id, None, None, Some(None)).unwrap();
        assert_eq!(store.load().unwrap().assignments.len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }
}
