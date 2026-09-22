//! Where a project lives. Mirrors `ProjectLocation` in
//! `packages/remote-contracts/src/shared.ts`; the wire spelling must not drift.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum ProjectTarget {
    Local {
        path: String,
    },
    Ssh {
        connection_id: String,
        remote_path: String,
    },
    Wsl {
        distribution: String,
        linux_path: String,
    },
}

impl ProjectTarget {
    /// A target is comparable across processes only through this key, never by
    /// stringifying a path: two spellings of the same project must not create two
    /// worktrees, and an `ssh://` string must never be treated as a local path.
    pub fn identity_key(&self) -> String {
        match self {
            ProjectTarget::Local { path } => format!("local:{path}"),
            ProjectTarget::Ssh {
                connection_id,
                remote_path,
            } => format!("ssh:{connection_id}:{remote_path}"),
            ProjectTarget::Wsl {
                distribution,
                linux_path,
            } => format!("wsl:{distribution}:{linux_path}"),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, ProjectTarget::Local { .. })
    }

    /// Only a local target can be resolved against the host filesystem.
    pub fn local_path(&self) -> Option<&str> {
        match self {
            ProjectTarget::Local { path } => Some(path),
            _ => None,
        }
    }
}

/// The stored project record, limited to what crosses the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub target: ProjectTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_targets_never_expose_a_local_path() {
        let ssh: ProjectTarget =
            serde_json::from_str(r#"{"kind":"ssh","connectionId":"c1","remotePath":"/srv/app"}"#)
                .unwrap();
        assert!(!ssh.is_local());
        assert_eq!(ssh.local_path(), None);
        assert_eq!(ssh.identity_key(), "ssh:c1:/srv/app");
    }

    #[test]
    fn identity_key_distinguishes_transport_even_with_the_same_path() {
        let local = ProjectTarget::Local {
            path: "/home/dev/app".into(),
        };
        let wsl = ProjectTarget::Wsl {
            distribution: "Ubuntu".into(),
            linux_path: "/home/dev/app".into(),
        };
        assert_ne!(local.identity_key(), wsl.identity_key());
    }
}
