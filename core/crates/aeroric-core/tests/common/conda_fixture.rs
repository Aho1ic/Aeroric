//! Owned fake executable used by core, authenticated UDS, CLI and desktop tests.
//! No installed Conda, ambient HOME, shell rc or external process is consulted.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use aeroric_core::infrastructure::conda_face::CondaProbe;
use serde_json::{json, Value};

pub struct FakeConda {
    pub root: PathBuf,
    pub program: PathBuf,
    pub marker: PathBuf,
    pub raw: String,
    pub wire: Value,
}

pub fn script(root: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(root).unwrap();
    let file = root.join(name);
    std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
    file
}

impl FakeConda {
    pub fn new(root: &Path) -> Self {
        let base = root.join("Conda 安装");
        let named = base.join("envs/环境 with spaces");
        for env in [&base, &named] {
            std::fs::create_dir_all(env.join("bin")).unwrap();
            // Existence, not executability, is the old Python filtering policy.
            std::fs::write(env.join("bin/python"), b"fixture, never execute").unwrap();
        }
        let raw = json!({"envs": [
            base, named, named, base.join("envs/missing-python"), false, null,
        ]})
        .to_string();
        let wire = json!([
            {"name":"base", "path":base, "pythonPath":base.join("bin/python")},
            {"name":"环境 with spaces", "path":named, "pythonPath":named.join("bin/python")},
        ]);
        let program = script(
            root,
            "fake conda",
            r#"
[ "$#" = 3 ] && [ "$1" = env ] && [ "$2" = list ] && [ "$3" = --json ] || exit 93
[ "$HOME" = "$FAKE_CONDA_HOME" ] || exit 94
printf '%s\n' "$1 $2 $3" >> "$FAKE_CONDA_MARKER"
printf '%s\n' "$FAKE_CONDA_JSON"
exit "$FAKE_CONDA_EXIT"
"#,
        );
        Self {
            root: root.to_path_buf(),
            program,
            marker: root.join("probe.log"),
            raw,
            wire,
        }
    }

    pub fn probe(&self) -> CondaProbe {
        self.probe_with_output(&self.raw, 0)
    }

    pub fn probe_with_output(&self, output: &str, status: i32) -> CondaProbe {
        CondaProbe {
            candidates: Some(vec![self.program.clone()]),
            clear_env: true,
            env: vec![
                ("HOME".into(), self.root.as_os_str().to_owned()),
                ("FAKE_CONDA_HOME".into(), self.root.as_os_str().to_owned()),
                (
                    "FAKE_CONDA_MARKER".into(),
                    self.marker.as_os_str().to_owned(),
                ),
                ("FAKE_CONDA_JSON".into(), output.into()),
                ("FAKE_CONDA_EXIT".into(), status.to_string().into()),
            ],
        }
    }
}
