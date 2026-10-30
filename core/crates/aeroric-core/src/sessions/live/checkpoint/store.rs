//! `<data>/checkpoints/<task_id>.json`: written to a temporary file, synced,
//! then renamed over the old one, so a crash leaves the old or the new.

use std::io::Write;
use std::path::{Path, PathBuf};

use aeroric_protocol::checkpoint::Checkpoint;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not a task id: {0}")]
    BadId(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn path(dir: &Path, task_id: &str) -> Result<PathBuf, StoreError> {
    let ok = !task_id.is_empty()
        && task_id.len() <= 128
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !task_id.starts_with('.');
    if !ok {
        return Err(StoreError::BadId(task_id.to_string()));
    }
    Ok(dir.join(format!("{task_id}.json")))
}

pub fn load(dir: &Path, task_id: &str) -> Result<Option<Checkpoint>, StoreError> {
    let path = path(dir, task_id)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn save(dir: &Path, checkpoint: &Checkpoint) -> Result<(), StoreError> {
    let path = path(dir, &checkpoint.task_id)?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", checkpoint.task_id));
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(checkpoint)?)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn remove(dir: &Path, task_id: &str) -> Result<(), StoreError> {
    match std::fs::remove_file(path(dir, task_id)?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A directory under the system temp dir, removed on drop.
    pub(crate) struct Scratch(PathBuf);
    impl Scratch {
        pub(crate) fn new() -> Self {
            Self(std::env::temp_dir().join(format!("aeroric-checkpoint-{}", uuid::Uuid::new_v4())))
        }
        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_task_id_cannot_leave_the_directory() {
        let dir = Scratch::new();
        for bad in ["../x", "a/b", "", ".hidden"] {
            assert!(
                matches!(load(dir.path(), bad), Err(StoreError::BadId(_))),
                "{bad}"
            );
        }
        assert!(load(dir.path(), "task-1").unwrap().is_none());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = Scratch::new();
        let mut cp = Checkpoint::new("task-1");
        cp.turn = 4;
        save(dir.path(), &cp).unwrap();
        assert_eq!(load(dir.path(), "task-1").unwrap(), Some(cp));
        remove(dir.path(), "task-1").unwrap();
        remove(dir.path(), "task-1").unwrap();
        assert!(load(dir.path(), "task-1").unwrap().is_none());
    }
}
