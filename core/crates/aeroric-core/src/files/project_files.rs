//! Project filename enumeration and ranking, extracted from the desktop adapter.
//! The caller authorizes `root` and supplies Git and its subprocess environment.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectFileSearchResult {
    pub path: String,
    pub name: String,
    pub dir: String,
    pub extension: Option<String>,
}

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".next",
    ".nuxt",
    "dist",
    "build",
    "target",
    "__pycache__",
    ".cache",
    "coverage",
    ".turbo",
    ".expo",
    "out",
    ".output",
    ".venv",
    "venv",
    ".tox",
];

const MAX_FILE_SEARCH_RESULTS: usize = 200;

/// Enumerate project files when Git is unavailable (a common case for a
/// freshly installed Windows desktop client). This keeps @-references useful
/// for ordinary folders while avoiding the dependency directories that make a
/// recursive fallback prohibitively large.
fn list_project_files_from_directory(root: &Path) -> Result<Vec<String>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("Cannot read {}: {error}", directory.display()))?;

        for entry in entries {
            let entry = entry.map_err(|error| format!("Cannot read project entry: {error}"))?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Cannot inspect {}: {error}", entry.path().display()))?;

            if file_type.is_dir() {
                let name = entry.file_name();
                if IGNORED_DIRS
                    .iter()
                    .any(|ignored| name.to_string_lossy().eq_ignore_ascii_case(ignored))
                {
                    continue;
                }
                pending.push(entry.path());
                continue;
            }

            // Do not follow symlinked directories during a fallback walk. A
            // project may contain links into large external trees, and Git's
            // file list does not traverse those directories either.
            if !file_type.is_file() {
                continue;
            }

            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| format!("Cannot make project path relative: {error}"))?
                .to_string_lossy()
                .replace('\\', "/");
            if !relative.is_empty() && relative_git_path_is_safe(&relative) {
                files.push(relative);
            }
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

pub fn list_project_files(
    root: &Path,
    git_path: Option<&str>,
    env: &[(String, String)],
) -> Result<Vec<String>, String> {
    let Some(git_path) = git_path.filter(|path| !path.is_empty()) else {
        return list_project_files_from_directory(root);
    };

    let mut cmd = Command::new(git_path);
    crate::runtime::subprocess::configure_background_command(&mut cmd);
    let output = cmd
        .args([
            "-c",
            "core.quotePath=false",
            "ls-files",
            "-c",
            "-o",
            "--exclude-standard",
        ])
        .current_dir(root)
        .envs(env.iter().cloned())
        .output();

    let Ok(output) = output else {
        return list_project_files_from_directory(root);
    };

    if !output.status.success() {
        return list_project_files_from_directory(root);
    }

    let mut files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim_end_matches('\r').replace('\\', "/"))
        .filter(|l| !l.is_empty() && relative_git_path_is_safe(l))
        .collect();

    files.sort();
    files.dedup();
    Ok(files)
}

fn relative_git_path_is_safe(path: &str) -> bool {
    let path = Path::new(path);
    path.components().all(|component| {
        matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    })
}

fn split_relative_file_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => ("".to_string(), path.to_string()),
    }
}

fn file_extension_lower(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .and_then(|(_, ext)| (!ext.is_empty()).then(|| ext.to_ascii_lowercase()))
}

pub fn search_project_files(
    root: &Path,
    query: &str,
    extensions: &[String],
    limit: Option<usize>,
    git_path: &str,
    env: &[(String, String)],
) -> Result<Vec<ProjectFileSearchResult>, String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let query = query.trim().to_ascii_lowercase();
    let extension_filters: std::collections::HashSet<String> = extensions
        .iter()
        .map(|ext| ext.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
        .collect();
    let limit = limit.unwrap_or(80).clamp(1, MAX_FILE_SEARCH_RESULTS);

    let mut cmd = Command::new(git_path);
    crate::runtime::subprocess::configure_background_command(&mut cmd);
    let output = cmd
        .args(["-c", "core.quotePath=false", "ls-files", "-z"])
        .current_dir(root)
        .envs(env.iter().cloned())
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let mut matches: Vec<(u8, ProjectFileSearchResult)> = Vec::new();
    for rel in String::from_utf8_lossy(&output.stdout).split('\0') {
        if rel.is_empty() || !relative_git_path_is_safe(rel) {
            continue;
        }

        let (dir, name) = split_relative_file_path(rel);
        let name_lower = name.to_ascii_lowercase();
        if !query.is_empty() && !name_lower.contains(&query) {
            continue;
        }

        let extension = file_extension_lower(&name);
        if !extension_filters.is_empty()
            && !extension
                .as_ref()
                .is_some_and(|ext| extension_filters.contains(ext))
        {
            continue;
        }

        let score = if query.is_empty() {
            3
        } else if name_lower == query {
            0
        } else if name_lower.starts_with(&query) {
            1
        } else {
            2
        };

        let full_path = root.join(rel);
        if !full_path.is_file() {
            continue;
        }
        let Ok(canonical_path) = full_path.canonicalize() else {
            continue;
        };
        if !canonical_path.starts_with(&canonical_root) {
            continue;
        }

        matches.push((
            score,
            ProjectFileSearchResult {
                path: full_path.to_string_lossy().into_owned(),
                name,
                dir,
                extension,
            },
        ));
    }

    matches.sort_by(|(score_a, a), (score_b, b)| {
        score_a
            .cmp(score_b)
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.dir.cmp(&b.dir))
    });

    Ok(matches
        .into_iter()
        .take(limit)
        .map(|(_, result)| result)
        .collect())
}
