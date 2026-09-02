//! 把 LSP 的 `WorkspaceEdit` 真正写到磁盘上(本地一套、远端一套)。
//!
//! 从 `lsp.rs` 整块搬出来,内容一行没改。
//!
//! 这一段和 `parse.rs` 相反 —— **全是副作用**,而且是**改用户源文件**的副作用,
//! 重命名符号会一次性写多个文件。所以两条不变量写在这里:
//!   1. 每个目标路径都要先钉死在项目根内(本地走 `canonicalize`,
//!      远端只能做字符串判断,拿不到远端 fs);
//!   2. 中途任何一个文件写失败,已经写掉的必须回滚
//!      (`rollback_local_workspace_writes` / `rollback_remote_workspace_writes`),
//!      否则用户的工作区会停在改了一半的状态。
//!
//! `lsp_position_to_offset` / `utf16_character_to_offset` 也在这里:LSP 的
//! character 是 **UTF-16 码元**偏移,不是字节也不是字符,换算错了就会切在字中间。

use super::*;

pub(super) fn apply_workspace_edit_for_root(
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

pub(super) fn run_remote_lsp_output(
    connection: &SshConnection,
    remote_command: String,
) -> Result<Vec<u8>, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, remote_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

pub(super) fn read_remote_lsp_text_file(
    connection: &SshConnection,
    remote_path: &str,
) -> Result<String, String> {
    let stdout =
        run_remote_lsp_output(connection, build_remote_lsp_read_text_command(remote_path))?;
    String::from_utf8(stdout).map_err(|err| err.to_string())
}

pub(super) fn write_remote_lsp_text_file(
    connection: &SshConnection,
    remote_path: &str,
    content: &str,
) -> Result<(), String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_lsp_write_text_command(remote_path),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open ssh stdin".to_string())?;
        stdin
            .write_all(content.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

pub(super) fn apply_remote_workspace_edit_for_root(
    connection: &SshConnection,
    remote_root: &str,
    edit: &LspWorkspaceEdit,
) -> Result<LspApplyWorkspaceEditSummary, String> {
    let remote_root = normalize_remote_lsp_path(remote_root, "Remote project path")?;
    let mut files_changed = 0;
    let mut edits_applied = 0;
    let mut edits_skipped = 0;
    let mut pending_writes: Vec<(String, String, String, usize)> = Vec::new();

    for file in &edit.files {
        let Ok(path) = validate_remote_lsp_edit_path(&remote_root, &file.path) else {
            edits_skipped += file.edits.len();
            continue;
        };

        let content = match read_remote_lsp_text_file(connection, &path) {
            Ok(content) => content,
            Err(_) => {
                edits_skipped += file.edits.len();
                continue;
            }
        };
        let edit_result = apply_text_edits_to_content(&content, &file.edits);
        edits_skipped += edit_result.edits_skipped;
        if edit_result.edits_applied > 0 {
            pending_writes.push((
                path,
                content,
                edit_result.content,
                edit_result.edits_applied,
            ));
        }
    }

    let mut written_originals: Vec<(String, String)> = Vec::new();
    for (path, original_content, next_content, applied_count) in pending_writes {
        if let Err(err) = write_remote_lsp_text_file(connection, &path, &next_content) {
            let rollback_error = rollback_remote_workspace_writes(connection, &written_originals);
            return Err(format!(
                "failed to write remote file: {err}; rolled back {} file(s){}",
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

pub(super) struct AppliedTextEdits {
    content: String,
    edits_applied: usize,
    edits_skipped: usize,
}

pub(super) fn apply_text_edits_to_content(
    content: &str,
    edits: &[LspTextEdit],
) -> AppliedTextEdits {
    let mut ranges = Vec::new();
    let mut edits_skipped = 0;
    for text_edit in edits {
        let Some(start) = lsp_position_to_offset(content, &text_edit.range.start) else {
            edits_skipped += 1;
            continue;
        };
        let Some(end) = lsp_position_to_offset(content, &text_edit.range.end) else {
            edits_skipped += 1;
            continue;
        };
        if start > end {
            edits_skipped += 1;
            continue;
        }
        ranges.push((start, end, text_edit.new_text.clone()));
    }

    ranges.sort_by_key(|right| std::cmp::Reverse(right.0));
    let mut next_content = content.to_string();
    let mut edits_applied = 0;
    for (start, end, new_text) in ranges {
        if end > next_content.len()
            || !next_content.is_char_boundary(start)
            || !next_content.is_char_boundary(end)
        {
            edits_skipped += 1;
            continue;
        }
        next_content.replace_range(start..end, &new_text);
        edits_applied += 1;
    }

    AppliedTextEdits {
        content: next_content,
        edits_applied,
        edits_skipped,
    }
}

pub(super) fn rollback_local_workspace_writes(writes: &[(PathBuf, String)]) -> Option<String> {
    let mut errors = Vec::new();
    for (path, content) in writes.iter().rev() {
        if let Err(err) = fs::write(path, content) {
            errors.push(format!("{}: {err}", path.display()));
        }
    }
    (!errors.is_empty()).then(|| errors.join("; "))
}

pub(super) fn rollback_remote_workspace_writes(
    connection: &SshConnection,
    writes: &[(String, String)],
) -> Option<String> {
    let mut errors = Vec::new();
    for (path, content) in writes.iter().rev() {
        if let Err(err) = write_remote_lsp_text_file(connection, path, content) {
            errors.push(format!("{path}: {err}"));
        }
    }
    (!errors.is_empty()).then(|| errors.join("; "))
}

pub(super) fn lsp_position_to_offset(content: &str, position: &LspPosition) -> Option<usize> {
    let mut current_line = 0_u32;
    let mut line_start = 0_usize;
    for segment in content.split_inclusive('\n') {
        let line_end = line_start + segment.len();
        if current_line == position.line {
            return utf16_character_to_offset(content, line_start, line_end, position.character);
        }
        line_start = line_end;
        current_line += 1;
    }
    if current_line == position.line {
        return utf16_character_to_offset(content, line_start, content.len(), position.character);
    }
    None
}

pub(super) fn utf16_character_to_offset(
    content: &str,
    line_start: usize,
    mut line_end: usize,
    character: u32,
) -> Option<usize> {
    if line_end > line_start && content.as_bytes().get(line_end - 1) == Some(&b'\n') {
        line_end -= 1;
    }
    if line_end > line_start && content.as_bytes().get(line_end - 1) == Some(&b'\r') {
        line_end -= 1;
    }

    let line = content.get(line_start..line_end)?;
    let mut units = 0_u32;
    for (relative, ch) in line.char_indices() {
        if units == character {
            return Some(line_start + relative);
        }
        units += ch.len_utf16() as u32;
        if units > character {
            return None;
        }
    }
    (units == character).then_some(line_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    // `temp_project` 提到了 `lsp.rs` 的模块层(那边还有 5 个用例在用),取回来共用一份定义。
    use super::super::temp_project;
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
