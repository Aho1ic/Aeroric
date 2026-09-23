//! Where Aeroric keeps its managed state.
//!
//! The layout is inherited from the shipping client (Task P2-02 Action 1) and is a
//! compatibility surface, not an implementation detail: the same directory has to stay
//! readable by the old Tauri client until P2-05 hands over the writer role. Every path
//! is resolved from an injected home directory, so a test never touches a real user's
//! `~/.aeroric` and the core never reads environment variables behind its caller's back.

use std::path::{Path, PathBuf};

/// Errors for a path that must not be used at all. Ids reach here from the UI and from
/// session files, so a rejected id is expected input, not a platform failure.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    #[error("{label} id is empty")]
    EmptyId { label: &'static str },
    #[error("{label} id {value:?} uses characters outside [A-Za-z0-9_-]")]
    IllegalIdCharacter { label: &'static str, value: String },
}

/// Managed data plus the immutable external home.
///
/// Two roots on purpose: `--data-dir` moves what Aeroric owns (`settings.json`,
/// `projects/`, `terminal-history/`, `agent-homes/`) and must never redirect the
/// *external* transcripts it only reads (`~/.claude`, `~/.codex`, `~/.omp`). Folding
/// both into one knob is how a relocated data directory would silently change which
/// agent's history a session list shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    user_home: PathBuf,
    data_dir: PathBuf,
}

impl AppPaths {
    /// The default location, matching the shipping client: `$HOME/.aeroric`.
    pub fn from_user_home(home: impl Into<PathBuf>) -> Self {
        let user_home = home.into();
        let data_dir = user_home.join(".aeroric");
        Self {
            user_home,
            data_dir,
        }
    }

    /// `override_dir` is what `--data-dir` will supply (P3-07). Only the managed
    /// directory moves; the external home stays where it is.
    pub fn with_data_dir(home: impl Into<PathBuf>, override_dir: Option<&Path>) -> Self {
        let user_home = home.into();
        let data_dir = match override_dir {
            Some(dir) if dir.as_os_str().is_empty() => user_home.join(".aeroric"),
            Some(dir) => dir.to_path_buf(),
            None => user_home.join(".aeroric"),
        };
        Self {
            user_home,
            data_dir,
        }
    }

    /// The user's real home. Read-only usage: external transcripts, never writes.
    pub fn user_home(&self) -> &Path {
        &self.user_home
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn projects_file(&self) -> PathBuf {
        self.data_dir.join("projects.json")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }

    pub fn project_dir(&self, project_id: &str) -> Result<PathBuf, PathError> {
        Self::validate_id(project_id, "project")?;
        Ok(self.projects_dir().join(project_id))
    }

    pub fn tasks_file(&self, project_id: &str) -> Result<PathBuf, PathError> {
        Ok(self.project_dir(project_id)?.join("tasks.json"))
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn terminal_history_dir(&self) -> PathBuf {
        self.data_dir.join("terminal-history")
    }

    pub fn terminal_history_file(&self, task_id: &str) -> Result<PathBuf, PathError> {
        Self::validate_id(task_id, "task")?;
        Ok(self.terminal_history_dir().join(format!("{task_id}.log")))
    }

    pub fn agent_homes_dir(&self) -> PathBuf {
        self.data_dir.join("agent-homes")
    }

    pub fn agent_credentials_dir(&self) -> PathBuf {
        self.data_dir.join("agent-credentials")
    }

    pub fn events_dir(&self, task_id: &str) -> Result<PathBuf, PathError> {
        Self::validate_id(task_id, "task")?;
        Ok(self.data_dir.join("events").join(task_id))
    }

    /// The project group list (ADR-013): core-exclusive, like `runtime.json`. It cannot live
    /// in `settings.json`, because the shipping client's `AppSettings` is a typed struct and
    /// its next save would drop any key it does not declare.
    pub fn project_groups_file(&self) -> PathBuf {
        self.data_dir.join("project-groups.json")
    }

    /// `task-operations.json` — which start attempts were already claimed, and how far
    /// each task's generation counter got. Core-exclusive like `project-groups.json`: the
    /// legacy client's full-list save drops fields it does not model, and a lost claim is a
    /// lost spawn-once guarantee.
    pub fn task_operations_file(&self) -> PathBuf {
        self.data_dir.join("task-operations.json")
    }

    /// The single-writer lease file (see `storage_lock`). One per data directory, so
    /// the lock travels with the data rather than with a process name.
    pub fn lease_file(&self) -> PathBuf {
        self.data_dir.join(".write.lock")
    }

    /// `schema/epoch/revision` metadata, kept in its own file so a crash cannot leave
    /// a half-written record inside the user's data.
    pub fn runtime_metadata_file(&self) -> PathBuf {
        self.data_dir.join("runtime.json")
    }

    /// Ids become path components, so this is the injection check, not cosmetics: the
    /// character class is exactly the one the shipping client enforces
    /// (`validate_storage_id` in `src-tauri/src/storage.rs`), and because it excludes
    /// `.`, `/` and `\` there is no spelling left that could walk out of `projects/` —
    /// which is why no separate "would escape" error exists here.
    pub fn validate_id(id: &str, label: &'static str) -> Result<(), PathError> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(PathError::EmptyId { label });
        }
        if !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(PathError::IllegalIdCharacter {
                label,
                value: id.to_string(),
            });
        }
        Ok(())
    }

    /// Directories that hold user data are private (0700) — the same rule as today,
    /// applied before anything is written into them.
    pub fn ensure_private(&self) -> std::io::Result<()> {
        for dir in [
            self.data_dir.clone(),
            self.projects_dir(),
            self.terminal_history_dir(),
        ] {
            std::fs::create_dir_all(&dir)?;
            private_dir(&dir)?;
        }
        Ok(())
    }
}

/// Apply "only this user" to a directory. On Windows the inherited-ACL approach the
/// shipping client uses lives in the platform adapter (plan P2-02 Action 5), so this
/// reports success without weakening anything: the directory was created with the
/// process's default ACL, and tightening is the adapter's job to assert.
#[cfg(not(windows))]
fn private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn private_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> AppPaths {
        AppPaths::from_user_home("/home/dev")
    }

    #[test]
    fn defaults_match_the_shipping_client() {
        let p = paths();
        assert_eq!(p.data_dir(), Path::new("/home/dev/.aeroric"));
        assert_eq!(
            p.tasks_file("p1").unwrap(),
            PathBuf::from("/home/dev/.aeroric/projects/p1/tasks.json")
        );
        assert_eq!(
            p.terminal_history_file("t_9").unwrap(),
            PathBuf::from("/home/dev/.aeroric/terminal-history/t_9.log")
        );
        assert_eq!(p.user_home(), Path::new("/home/dev"));
    }

    /// Relocating managed data must not relocate what we only read.
    #[test]
    fn data_dir_override_moves_managed_state_only() {
        let moved = AppPaths::with_data_dir("/home/dev", Some(Path::new("/Volumes/fast/aeroric")));
        assert_eq!(moved.data_dir(), Path::new("/Volumes/fast/aeroric"));
        assert_eq!(moved.user_home(), Path::new("/home/dev"));
        assert!(moved.settings_file().starts_with("/Volumes/fast/aeroric"));
        assert!(!moved.agent_homes_dir().starts_with("/home/dev"));
        let external = moved.user_home().join(".claude").join("projects");
        assert!(
            external.starts_with("/home/dev"),
            "external transcript roots must stay in the real home: {external:?}"
        );
    }

    #[test]
    fn an_empty_override_falls_back_to_the_default_instead_of_the_filesystem_root() {
        let p = AppPaths::with_data_dir("/home/dev", Some(Path::new("")));
        assert_eq!(p.data_dir(), Path::new("/home/dev/.aeroric"));
    }

    #[test]
    fn ids_that_could_escape_are_refused() {
        for (id, label) in [
            ("", "project"),
            ("   ", "project"),
            ("../../etc/passwd", "project"),
            ("a/b", "project"),
            ("a\\b", "task"),
            ("a;b", "task"),
            ("$(id)", "task"),
            ("a b", "task"),
            (".", "project"),
            ("..", "project"),
        ] {
            let error = match AppPaths::validate_id(id, label) {
                Ok(()) => panic!("{id:?} must be refused"),
                Err(error) => error,
            };
            assert!(
                matches!(
                    error,
                    PathError::EmptyId { .. } | PathError::IllegalIdCharacter { .. }
                ),
                "{id:?} -> {error:?}"
            );
        }
        for id in ["abc", "A-9_b", "0", "1234567890"] {
            AppPaths::validate_id(id, "task").expect("legitimate ids pass");
        }
    }

    #[test]
    fn a_refused_id_never_reaches_the_filesystem() {
        let p = paths();
        assert!(p.tasks_file("../evil").is_err());
        assert!(p.terminal_history_file("/etc/passwd").is_err());
        assert!(p.events_dir("").is_err());
    }

    #[test]
    fn the_lease_and_metadata_live_with_the_data() {
        let p = AppPaths::with_data_dir("/home/dev", Some(Path::new("/srv/aeroric")));
        assert_eq!(p.lease_file(), PathBuf::from("/srv/aeroric/.write.lock"));
        assert_eq!(
            p.runtime_metadata_file(),
            PathBuf::from("/srv/aeroric/runtime.json")
        );
    }
}
