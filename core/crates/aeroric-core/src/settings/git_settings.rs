//! 2026-11-24 设置 › Git 与工作副本: what the user adds to Aeroric's Git work,
//! kept in `<data dir>/git-settings.json` beside `budget.json`.
//!
//! - **提交说明的附加指引:** added after the project's own commit prompt
//!   (`.aeroric` project config `git.commit_prompt`) when Aeroric writes a
//!   commit message (Git panel → 生成).
//! - **独立副本保留天数:** an isolated copy with nothing left to bring back is
//!   removed this many days after it emptied (`isolated_copy::prune_after`,
//!   when the host starts). 0 keeps them. A copy with changes still in it is
//!   never removed by this, whatever the number.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "git-settings.json";
pub const DEFAULT_KEEP_DAYS: u32 = 30;
pub const MAX_KEEP_DAYS: u32 = 365;
pub const MAX_GUIDANCE_CHARS: usize = 2000;
const DAY_MS: u64 = 24 * 3600 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSettings {
    #[serde(default)]
    pub commit_guidance: String,
    #[serde(default = "default_keep_days")]
    pub cleared_copy_keep_days: u32,
}

fn default_keep_days() -> u32 {
    DEFAULT_KEEP_DAYS
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            commit_guidance: String::new(),
            cleared_copy_keep_days: DEFAULT_KEEP_DAYS,
        }
    }
}

impl GitSettings {
    /// How long an emptied copy is kept; `None` keeps it.
    pub fn keep_cleared_ms(&self) -> Option<u64> {
        (self.cleared_copy_keep_days > 0).then(|| u64::from(self.cleared_copy_keep_days) * DAY_MS)
    }

    /// The project's commit prompt with the user's guidance after it.
    pub fn commit_prompt(&self, project_prompt: &str) -> String {
        let guidance = self.commit_guidance.trim();
        if guidance.is_empty() {
            return project_prompt.to_string();
        }
        format!("{project_prompt}\n\nAlso follow these requirements from the user:\n{guidance}")
    }
}

/// What is saved; missing or unreadable is the default.
pub fn load(data_dir: &Path) -> GitSettings {
    std::fs::read(data_dir.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Check and save; returns what was saved.
pub fn save(data_dir: &Path, settings: &GitSettings) -> Result<GitSettings, String> {
    let guidance = settings.commit_guidance.trim().to_string();
    if guidance.chars().count() > MAX_GUIDANCE_CHARS {
        return Err(format!("提交说明的附加指引最多 {MAX_GUIDANCE_CHARS} 个字"));
    }
    if guidance.contains('\0') {
        return Err("提交说明的附加指引里有不能用的字符".to_string());
    }
    if settings.cleared_copy_keep_days > MAX_KEEP_DAYS {
        return Err(format!("保留天数最多 {MAX_KEEP_DAYS} 天"));
    }
    let saved = GitSettings {
        commit_guidance: guidance,
        cleared_copy_keep_days: settings.cleared_copy_keep_days,
    };
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let path = data_dir.join(FILE);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&saved).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aeroric-git-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn nothing_saved_is_the_default() {
        let dir = scratch();
        assert_eq!(load(&dir), GitSettings::default());
        assert_eq!(load(&dir).keep_cleared_ms(), Some(30 * DAY_MS));
        std::fs::write(dir.join(FILE), b"not json").unwrap();
        assert_eq!(
            load(&dir),
            GitSettings::default(),
            "unreadable: the default"
        );
        std::fs::write(dir.join(FILE), br#"{"commitGuidance":"x"}"#).unwrap();
        assert_eq!(
            load(&dir).cleared_copy_keep_days,
            30,
            "a missing field: its default"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_checks_and_trims() {
        let dir = scratch();
        let saved = save(
            &dir,
            &GitSettings {
                commit_guidance: "  用中文写  \n".into(),
                cleared_copy_keep_days: 0,
            },
        )
        .unwrap();
        assert_eq!(saved.commit_guidance, "用中文写");
        assert_eq!(load(&dir), saved);
        assert_eq!(saved.keep_cleared_ms(), None, "0 keeps them");

        let long = GitSettings {
            commit_guidance: "长".repeat(MAX_GUIDANCE_CHARS + 1),
            ..GitSettings::default()
        };
        assert!(save(&dir, &long).is_err());
        let years = GitSettings {
            cleared_copy_keep_days: MAX_KEEP_DAYS + 1,
            ..GitSettings::default()
        };
        assert!(save(&dir, &years).is_err());
        assert_eq!(load(&dir), saved, "a refused save leaves the file");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn guidance_goes_after_the_projects_prompt() {
        let none = GitSettings::default();
        assert_eq!(none.commit_prompt("P"), "P");
        let some = GitSettings {
            commit_guidance: "用中文写".into(),
            ..GitSettings::default()
        };
        let prompt = some.commit_prompt("P");
        assert!(prompt.starts_with("P\n\n"));
        assert!(prompt.ends_with("用中文写"));
    }
}
