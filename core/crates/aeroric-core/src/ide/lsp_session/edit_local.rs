//! LSP `WorkspaceEdit` 的**本地**半边:多文件写 + 失败回滚。
//!
//! 这一片全是副作用,而且是**改用户源文件**的副作用——重命名符号会一次性写多个
//! 文件。两条不变量(搬运时原样保留):
//!   1. 每个目标路径都要先 `canonicalize` 钉死在项目根内;
//!   2. 中途任何一个文件写失败,已经写掉的必须回滚(逆序),否则用户的工作区会
//!      停在改了一半的状态。

use std::fs;
use std::path::{Path, PathBuf};

use crate::ide::lsp_session::edit_shared::apply_text_edits_to_content;
use crate::ide::lsp_session::{LspApplyWorkspaceEditSummary, LspWorkspaceEdit};

pub fn apply_workspace_edit_for_root(
    root: &Path,
    edit: &LspWorkspaceEdit,
) -> Result<LspApplyWorkspaceEditSummary, String> {
    let root = root
        .canonicalize()
        .map_err(|err| format!("failed to resolve project root: {err}"))?;
    let mut files_changed = 0;
    let mut edits_applied = 0;
    let mut edits_skipped = 0;
    let mut pending_writes: Vec<(PathBuf, String, String, usize)> = Vec::new();

    for file in &edit.files {
        let path = Path::new(&file.path);
        let Ok(canonical) = path.canonicalize() else {
            edits_skipped += file.edits.len();
            continue;
        };
        if !canonical.starts_with(&root) || !canonical.is_file() {
            edits_skipped += file.edits.len();
            continue;
        }

        let content =
            fs::read_to_string(&canonical).map_err(|err| format!("failed to read file: {err}"))?;
        let edit_result = apply_text_edits_to_content(&content, &file.edits);
        edits_skipped += edit_result.edits_skipped;
        if edit_result.edits_applied > 0 {
            pending_writes.push((
                canonical,
                content,
                edit_result.content,
                edit_result.edits_applied,
            ));
        }
    }

    let mut written_originals: Vec<(PathBuf, String)> = Vec::new();
    for (path, original_content, next_content, applied_count) in pending_writes {
        if let Err(err) = fs::write(&path, next_content) {
            let rollback_error = rollback_local_workspace_writes(&written_originals);
            return Err(format!(
                "failed to write file: {err}; rolled back {} file(s){}",
                written_originals.len(),
                rollback_error
                    .as_ref()
                    .map(|error| format!("; rollback failed: {error}"))
                    .unwrap_or_default()
            ));
        }
        written_originals.push((path, original_content));
        files_changed += 1;
        edits_applied += applied_count;
    }

    Ok(LspApplyWorkspaceEditSummary {
        files_changed,
        edits_applied,
        edits_skipped,
    })
}

pub fn rollback_local_workspace_writes(writes: &[(PathBuf, String)]) -> Option<String> {
    let mut errors = Vec::new();
    for (path, content) in writes.iter().rev() {
        if let Err(err) = fs::write(path, content) {
            errors.push(format!("{}: {err}", path.display()));
        }
    }
    (!errors.is_empty()).then(|| errors.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ide::lsp_session::temp_project;
    use crate::ide::lsp_session::*;
    use std::fs;
    #[test]
    fn apply_workspace_edit_rejects_paths_outside_project_root() {
        let root = temp_project("rename-apply-root");
        let outside_root = temp_project("rename-apply-outside");
        let outside = outside_root.join("outside.ts");
        fs::write(&outside, "const oldName = 1;\n").unwrap();
        let edit = LspWorkspaceEdit {
            files: vec![LspFileEdit {
                uri: file_uri(&outside),
                path: outside.to_string_lossy().into_owned(),
                edits: vec![LspTextEdit {
                    range: LspRange {
                        start: LspPosition {
                            line: 0,
                            character: 6,
                        },
                        end: LspPosition {
                            line: 0,
                            character: 13,
                        },
                    },
                    new_text: "newName".to_string(),
                }],
            }],
        };

        let summary = apply_workspace_edit_for_root(&root, &edit).unwrap();

        assert_eq!(summary.files_changed, 0);
        assert_eq!(summary.edits_applied, 0);
        assert_eq!(summary.edits_skipped, 1);
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "const oldName = 1;\n"
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside_root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn apply_workspace_edit_rolls_back_written_files_when_a_later_write_fails() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_project("rename-apply-rollback");
        let first = root.join("first.ts");
        let second = root.join("second.ts");
        fs::write(&first, "const oldName = 1;\n").unwrap();
        fs::write(&second, "const oldName = 2;\n").unwrap();

        let mut readonly = fs::metadata(&second).unwrap().permissions();
        readonly.set_mode(0o444);
        fs::set_permissions(&second, readonly).unwrap();

        let edit = LspWorkspaceEdit {
            files: vec![
                LspFileEdit {
                    uri: file_uri(&first),
                    path: first.to_string_lossy().into_owned(),
                    edits: vec![LspTextEdit {
                        range: LspRange {
                            start: LspPosition {
                                line: 0,
                                character: 6,
                            },
                            end: LspPosition {
                                line: 0,
                                character: 13,
                            },
                        },
                        new_text: "newName".to_string(),
                    }],
                },
                LspFileEdit {
                    uri: file_uri(&second),
                    path: second.to_string_lossy().into_owned(),
                    edits: vec![LspTextEdit {
                        range: LspRange {
                            start: LspPosition {
                                line: 0,
                                character: 6,
                            },
                            end: LspPosition {
                                line: 0,
                                character: 13,
                            },
                        },
                        new_text: "newName".to_string(),
                    }],
                },
            ],
        };

        let result = apply_workspace_edit_for_root(&root, &edit);

        let mut writable = fs::metadata(&second).unwrap().permissions();
        writable.set_mode(0o644);
        fs::set_permissions(&second, writable).unwrap();

        let err = result.expect_err("second write should fail");
        assert!(err.contains("rolled back 1 file(s)"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "const oldName = 1;\n");
        assert_eq!(fs::read_to_string(&second).unwrap(), "const oldName = 2;\n");
        fs::remove_dir_all(root).unwrap();
    }
}
