//! LSP `WorkspaceEdit` 的**远端**半边:多文件写 + 失败回滚(经调用方注入的
//! 远端 IO 端口执行——SSH 命令构建留在客户端适配层,core 不依赖 ssh 命令面)。
//!
//! 与 `edit_local.rs` 是同一功能的两套实现:远端拿不到远端 fs,路径只能做
//! **字符串**判定(`validate_remote_edit_path`,由调用方注入);文本区间算法与
//! 本地共用 `edit_shared.rs`。回滚不变量同本地:任何一次写失败,已写的逆序写回。

use std::path::Path;

use crate::ide::lsp_session::edit_shared::apply_text_edits_to_content;
use crate::ide::lsp_session::LspApplyWorkspaceEditSummary;

/// 远端文件的读/写端口(客户端适配层用 ssh 命令实现;core 只依赖这两个动作,
/// 不依赖连接类型,也不依赖任何 ssh 客户端)。
pub trait RemoteEditIo {
    fn read_file(&self, remote_path: &str) -> Result<String, String>;
    fn write_file(&self, remote_path: &str, content: &str) -> Result<(), String>;
}

/// 远端工作区编辑:逐文件读→算→写,任一次写失败即逆序回滚已写文件。
pub fn apply_remote_workspace_edit_for_root(
    io: &dyn RemoteEditIo,
    remote_root: &str,
    validate_remote_edit_path: impl Fn(&str, &str) -> Result<String, String>,
    edit: &crate::ide::lsp_session::LspWorkspaceEdit,
) -> Result<LspApplyWorkspaceEditSummary, String> {
    let mut files_changed = 0;
    let mut edits_applied = 0;
    let mut edits_skipped = 0;
    let mut pending_writes: Vec<(String, String, String, usize)> = Vec::new();

    for file in &edit.files {
        let Ok(path) = validate_remote_edit_path(remote_root, &file.path) else {
            edits_skipped += file.edits.len();
            continue;
        };

        let content = match io.read_file(&path) {
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
        if let Err(err) = io.write_file(&path, &next_content) {
            let rollback_error = rollback_remote_workspace_writes(io, &written_originals);
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

/// 逆序写回(最新的写最先撤)。
pub fn rollback_remote_workspace_writes(
    io: &dyn RemoteEditIo,
    writes: &[(String, String)],
) -> Option<String> {
    let mut errors = Vec::new();
    for (path, content) in writes.iter().rev() {
        if let Err(err) = io.write_file(path, content) {
            errors.push(format!("{path}: {err}"));
        }
    }
    (!errors.is_empty()).then(|| errors.join("; "))
}

/// 远端 `file_uri`(与本地同形,只为调用点少写一层 `Path::new`)。
pub fn remote_file_uri(path: &str) -> String {
    super::protocol::file_uri(Path::new(path))
}
