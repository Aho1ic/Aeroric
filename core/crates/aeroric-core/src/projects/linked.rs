//! 关联文件夹（2026-10-29，L1–L2）。项目记录里的 `linkedFolders` 是一组绝对
//! 路径：启动会话时作为额外工作目录交给 Agent，文件树里另列一区。不往用户
//! 仓库写任何东西（不建符号链接）；只支持本地项目。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::storage::Record;

pub const KEY: &str = "linkedFolders";

/// The folders a project record lists, as stored.
pub fn stored(project: &Record) -> Vec<String> {
    project
        .0
        .get(KEY)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The stored folders that still exist; a missing one is skipped at launch,
/// never dropped from the record (Q4).
pub fn usable(project: &Record) -> Vec<String> {
    stored(project)
        .into_iter()
        .filter(|p| Path::new(p).is_dir())
        .collect()
}

fn canonical(path: &str) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Checks a folder before it is linked and returns its canonical path.
/// Refused: a remote project, a missing folder, the project itself, a folder
/// that contains the project, one already inside it, and one linked already.
pub fn validate(
    project_path: &str,
    existing: &[String],
    candidate: &str,
) -> Result<String, String> {
    if project_path.contains("://") {
        return Err("关联文件夹目前只支持本地项目".into());
    }
    let candidate_path = canonical(candidate.trim()).ok_or("找不到这个文件夹")?;
    if !candidate_path.is_dir() {
        return Err("只能关联文件夹".into());
    }
    let root = canonical(project_path).unwrap_or_else(|| PathBuf::from(project_path));
    if candidate_path == root {
        return Err("这就是项目本身，不用关联".into());
    }
    if root.starts_with(&candidate_path) {
        return Err("这个文件夹包含了整个项目，不能关联".into());
    }
    if candidate_path.starts_with(&root) {
        return Err("这个文件夹已经在项目里面，不用再关联".into());
    }
    let dup = existing
        .iter()
        .any(|p| canonical(p).unwrap_or_else(|| PathBuf::from(p)) == candidate_path);
    if dup {
        return Err("这个文件夹已经关联过了".into());
    }
    Ok(candidate_path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (PathBuf, String, String, String) {
        let base =
            std::env::temp_dir().join(format!("aeroric-linked-{}", uuid::Uuid::new_v4().simple()));
        let project = base.join("app");
        let inner = project.join("src");
        let other = base.join("shared");
        for dir in [&inner, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let s = |p: &PathBuf| p.to_string_lossy().into_owned();
        (base.clone(), s(&project), s(&inner), s(&other))
    }

    #[test]
    fn the_refusal_rules() {
        let (base, project, inner, other) = tree();
        assert!(validate(&project, &[], &other).is_ok());
        assert_eq!(
            validate(&project, &[], &project).unwrap_err(),
            "这就是项目本身，不用关联"
        );
        assert!(validate(&project, &[], &inner)
            .unwrap_err()
            .contains("已经在项目里面"));
        assert!(validate(&project, &[], &base.to_string_lossy())
            .unwrap_err()
            .contains("包含了整个项目"));
        assert!(validate(&project, std::slice::from_ref(&other), &other)
            .unwrap_err()
            .contains("已经关联过"));
        assert!(validate(&project, &[], "/no/such/folder").is_err());
        assert!(validate("ssh://host/app", &[], &other)
            .unwrap_err()
            .contains("本地项目"));
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn missing_folders_stay_stored_but_are_not_used() {
        let (base, _, _, other) = tree();
        let mut record = Record(serde_json::Map::new());
        record.0.insert(
            KEY.into(),
            serde_json::json!([other.clone(), "/no/such/folder"]),
        );
        assert_eq!(stored(&record).len(), 2);
        assert_eq!(usable(&record), vec![other]);
        std::fs::remove_dir_all(base).ok();
    }
}
