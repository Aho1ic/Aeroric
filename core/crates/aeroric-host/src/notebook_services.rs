//! Notebook（随手记）服务（P7-05）：把 `aeroric_core::notebook` 的业务面暴露给
//! 原生客户端，命令体与 Tauri 侧 `notebook_*` 逐一镜像（resolve → owning_vault →
//! 业务调用）。路径闸门 server 端权威：`NotebookState::resolve_in_vaults` 的
//! vault allowlist（注册过的根 + symlink 落根内判定）。
//!
//! sync/rag 面属 P7-06，不在本文件。`attachment_read` 与 Tauri 的 `ipc::Response`
//! 不同：JSON 传输下字节走 base64。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::notebook::state::NotebookState;
use aeroric_core::notebook::{
    attachments, export, fields, html2md, import, links, mentions, migrate, snapshots, state,
    tag_rename, tasks, trash, user_templates, vault_index,
};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

/// 进程内共享的 vault 注册表 + 打开文件指纹表。注册时创建，所有 notebook
/// handler 共享同一份（与 Tauri `.manage(NotebookState::default())` 同语义）。
#[derive(Clone, Default)]
pub struct NotebookHandle {
    state: Arc<NotebookState>,
}

impl std::ops::Deref for NotebookHandle {
    type Target = NotebookState;
    fn deref(&self) -> &NotebookState {
        &self.state
    }
}

macro_rules! try_f {
    ($expr:expr) => {
        match $expr {
            Ok(value) => value,
            // helpers 统一回 `Box<Frame>`（result_large_err，与 file_params 同法）。
            Err(boxed) => return *boxed,
        }
    };
}

/// `Result<_, String>` 版：业务错误 → IoError Failure。
macro_rules! try_s {
    ($request:expr, $expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(e) => return failure($request.id.clone(), ErrorCode::IoError, &e),
        }
    };
}

pub fn register_notebook_services(set: &mut ServiceSet, _context: Arc<CoreContext>) {
    let handle = NotebookHandle::default();
    macro_rules! service {
        ($method:expr, $fn_name:ident) => {{
            let handle = handle.clone();
            set.register(
                $method,
                Arc::new(move |request, _call| {
                    let handle = handle.clone();
                    Box::pin(async move { $fn_name(request, &handle) })
                }),
            );
        }};
    }

    // ── vault 注册 ──
    service!("notebook.register_vault", nb_register_vault);
    service!("notebook.unregister_vault", nb_unregister_vault);
    service!("notebook.list_vaults", nb_list_vaults);
    service!("notebook.ensure_default_vault", nb_ensure_default_vault);
    service!("notebook.ensure_project_vault", nb_ensure_project_vault);

    // ── 读写 ──
    service!("notebook.read_tree", nb_read_tree);
    service!("notebook.read_order", nb_read_order);
    service!("notebook.write_order", nb_write_order);
    service!("notebook.read_icons", nb_read_icons);
    service!("notebook.write_icons", nb_write_icons);
    service!("notebook.open_note", nb_open_note);
    service!("notebook.peek_note", nb_peek_note);
    service!("notebook.close_note", nb_close_note);
    service!("notebook.save_note", nb_save_note);
    service!("notebook.create_note", nb_create_note);
    service!("notebook.create_note_in_vault", nb_create_note_in_vault);
    service!("notebook.rename_to_title", nb_rename_to_title);
    service!("notebook.create_folder", nb_create_folder);
    service!("notebook.rename_note", nb_rename_note);
    service!("notebook.delete_note", nb_delete_note);

    // ── 回收站 ──
    service!("notebook.trash_list", nb_trash_list);
    service!("notebook.trash_restore", nb_trash_restore);
    service!("notebook.trash_purge", nb_trash_purge);
    service!("notebook.trash_purge_all", nb_trash_purge_all);

    // ── 附件 ──
    service!("notebook.attachment_save", nb_attachment_save);
    service!(
        "notebook.attachment_save_from_path",
        nb_attachment_save_from_path
    );
    service!("notebook.attachment_list", nb_attachment_list);
    service!("notebook.attachment_read", nb_attachment_read);

    // ── 元数据与扫描面 ──
    service!("notebook.note_stat", nb_note_stat);
    service!("notebook.vault_index", nb_vault_index);
    service!("notebook.vault_links", nb_vault_links);
    service!("notebook.vault_tags", nb_vault_tags);
    service!("notebook.vault_fields", nb_vault_fields);
    service!("notebook.vault_tasks", nb_vault_tasks);
    service!("notebook.vault_mentions", nb_vault_mentions);
    service!("notebook.link_mentions", nb_link_mentions);
    service!("notebook.rename_tag", nb_rename_tag);

    // ── 版本历史 ──
    service!("notebook.list_snapshots", nb_list_snapshots);
    service!("notebook.read_snapshot", nb_read_snapshot);
    service!("notebook.restore_snapshot", nb_restore_snapshot);

    // ── 迁移与模板 ──
    service!("notebook.migrate_legacy", nb_migrate_legacy);
    service!("notebook.convert_richtext", nb_convert_richtext);
    service!("notebook.list_user_templates", nb_list_user_templates);

    // ── 导入（7 家格式原样沿用）──
    service!("notebook.import_obsidian", nb_import_obsidian);
    service!("notebook.import_logseq", nb_import_logseq);
    service!("notebook.import_notion", nb_import_notion);
    service!("notebook.import_bear", nb_import_bear);
    service!("notebook.import_roam", nb_import_roam);
    service!("notebook.import_evernote", nb_import_evernote);
    service!("notebook.import_apple_notes", nb_import_apple_notes);

    // ── 导出 ──
    service!("notebook.export_write_file", nb_export_write_file);
    service!("notebook.export_site_write", nb_export_site_write);
    service!("notebook.html_to_markdown", nb_html_to_markdown);
}

// ── vault 注册 ──────────────────────────────────────────────────────────────

fn nb_register_vault(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    match handle.register_vault(Path::new(&path)) {
        Ok(canon) => ok(&request, json!({ "path": canon.to_string_lossy() })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_unregister_vault(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    match handle.unregister_vault(Path::new(&path)) {
        Ok(()) => ok(&request, json!({ "unregistered": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_list_vaults(request: Request, handle: &NotebookHandle) -> Frame {
    match handle.registered_vaults() {
        Ok(vaults) => ok(
            &request,
            json!({
                "vaults": vaults
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
            }),
        ),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_ensure_default_vault(request: Request, handle: &NotebookHandle) -> Frame {
    let result = migrate::ensure_default_vault().and_then(|vault| {
        handle
            .register_vault(&vault)
            .map(|canon| canon.to_string_lossy().to_string())
    });
    match result {
        Ok(path) => ok(&request, json!({ "path": path })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_ensure_project_vault(request: Request, handle: &NotebookHandle) -> Frame {
    let project_path = try_f!(need_str(&request, "projectPath"));
    let result = migrate::ensure_project_vault(&project_path).and_then(|vault| {
        handle
            .register_vault(&vault)
            .map(|canon| canon.to_string_lossy().to_string())
    });
    match result {
        Ok(path) => ok(&request, json!({ "path": path })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

// ── 读写 ─────────────────────────────────────────────────────────────────────

fn nb_read_tree(request: Request, handle: &NotebookHandle) -> Frame {
    let root = try_f!(need_str(&request, "root"));
    match aeroric_core::notebook::fs_ops::read_tree(handle, &root) {
        Ok(entries) => ok(&request, json!({ "entries": entries })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_read_order(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    ok(
        &request,
        json!({ "order": aeroric_core::notebook::fs_ops::read_order(&resolved) }),
    )
}

fn nb_write_order(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let names = try_f!(need::<Vec<String>>(&request, "names"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match aeroric_core::notebook::fs_ops::write_order(&resolved, &names) {
        Ok(()) => ok(&request, json!({ "written": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_read_icons(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    ok(
        &request,
        json!({ "icons": aeroric_core::notebook::fs_ops::read_icons(&resolved) }),
    )
}

fn nb_write_icons(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let icons = try_f!(need::<std::collections::BTreeMap<String, String>>(
        &request, "icons"
    ));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match aeroric_core::notebook::fs_ops::write_icons(&resolved, &icons) {
        Ok(()) => ok(&request, json!({ "written": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_open_note(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    match aeroric_core::notebook::fs_ops::read_note(handle, &path) {
        Ok(opened) => ok(
            &request,
            json!({ "content": opened.content, "sig": opened.sig }),
        ),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_peek_note(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    match aeroric_core::notebook::fs_ops::read_note_content(handle, &path) {
        Ok((_, opened)) => ok(
            &request,
            json!({ "content": opened.content, "sig": opened.sig }),
        ),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_close_note(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    // 关闭时路径可能已经不存在(文件刚被删)。解析失败就按原路径清一次。
    let result = match handle.resolve_in_vaults(&path, false) {
        Ok(resolved) => handle.record_close(&resolved),
        Err(_) => handle.record_close(Path::new(&path)),
    };
    match result {
        Ok(()) => ok(&request, json!({ "closed": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_save_note(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    let content = try_f!(need_str(&request, "content"));
    let expected = try_f!(need_opt::<state::FileSig>(&request, "expected"));
    let force = try_f!(need_opt::<bool>(&request, "force")).unwrap_or(false);
    match aeroric_core::notebook::fs_ops::save_note(handle, &path, &content, expected, force) {
        Ok(outcome) => ok(&request, json!({ "outcome": outcome })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_create_note(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    let content = try_f!(need_str(&request, "content"));
    match aeroric_core::notebook::fs_ops::create_note(handle, &path, &content) {
        Ok(sig) => ok(&request, json!({ "sig": sig })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_create_note_in_vault(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let title = try_f!(need_str(&request, "title"));
    let content = try_f!(need_str(&request, "content"));
    let created = (|| -> Result<aeroric_core::notebook::CreatedNote, String> {
        let vault_path = handle.resolve_in_vaults(&vault, false)?;
        let target = migrate::allocate_note_path(&vault_path, &title)?;
        let path = target.to_string_lossy().to_string();
        let sig = aeroric_core::notebook::fs_ops::create_note(handle, &path, &content)?;
        Ok(aeroric_core::notebook::CreatedNote { path, sig })
    })();
    match created {
        Ok(created) => ok(
            &request,
            json!({ "path": created.path, "sig": created.sig }),
        ),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_rename_to_title(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let path = try_f!(need_str(&request, "path"));
    let title = try_f!(need_str(&request, "title"));
    let renamed = (|| -> Result<String, String> {
        let vault_path = handle.resolve_in_vaults(&vault, false)?;
        let current = handle.resolve_in_vaults(&path, false)?;
        let target = migrate::allocate_note_path(&vault_path, &title)?;
        if target == current {
            return Ok(current.to_string_lossy().to_string());
        }
        let target_text = target.to_string_lossy().to_string();
        aeroric_core::notebook::fs_ops::rename_note(handle, &path, &target_text)?;
        Ok(target_text)
    })();
    match renamed {
        Ok(new_path) => ok(&request, json!({ "path": new_path })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_create_folder(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    match aeroric_core::notebook::fs_ops::create_folder(handle, &path) {
        Ok(()) => ok(&request, json!({ "created": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_rename_note(request: Request, handle: &NotebookHandle) -> Frame {
    let from = try_f!(need_str(&request, "from"));
    let to = try_f!(need_str(&request, "to"));
    match aeroric_core::notebook::fs_ops::rename_note(handle, &from, &to) {
        Ok(()) => ok(&request, json!({ "renamed": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_delete_note(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    // 与 Tauri 命令体一致：软删成功之后才清指纹（目录清子树）。
    let deleted = (|| -> Result<trash::TrashItem, String> {
        let resolved = handle.resolve_in_vaults(&path, false)?;
        let vault = handle.owning_vault(&resolved)?;
        let item = trash::trash(&vault, &resolved)?;
        if item.is_dir {
            handle.record_close_subtree(&resolved)?;
        } else {
            handle.record_close(&resolved)?;
        }
        Ok(item)
    })();
    match deleted {
        Ok(item) => ok(&request, json!({ "item": item })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

// ── 回收站 ───────────────────────────────────────────────────────────────────

fn nb_trash_list(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let root = try_s!(request, vault_root(handle, &vault));
    match trash::list(&root) {
        Ok(items) => ok(&request, json!({ "items": items })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_trash_restore(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let id = try_f!(need_str(&request, "id"));
    let root = try_s!(request, vault_root(handle, &vault));
    match trash::restore(&root, &id) {
        Ok(restored) => ok(
            &request,
            json!({ "path": restored.path, "isDir": restored.is_dir }),
        ),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_trash_purge(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let id = try_f!(need_str(&request, "id"));
    let root = try_s!(request, vault_root(handle, &vault));
    match trash::purge(&root, &id) {
        Ok(()) => ok(&request, json!({ "purged": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_trash_purge_all(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let root = try_s!(request, vault_root(handle, &vault));
    match trash::purge_all(&root) {
        Ok(count) => ok(&request, json!({ "purged": count })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

// ── 附件 ─────────────────────────────────────────────────────────────────────

fn nb_attachment_save(request: Request, handle: &NotebookHandle) -> Frame {
    let note = try_f!(need_str(&request, "note"));
    let mime = try_f!(need_str(&request, "mime"));
    let data_base64 = try_f!(need_str(&request, "dataBase64"));
    let file_name = try_f!(need_opt::<String>(&request, "fileName"));
    let saved = (|| -> Result<attachments::SavedAttachment, String> {
        let resolved_note = handle.resolve_in_vaults(&note, false)?;
        let vault = handle.owning_vault(&resolved_note)?;
        // data URL 前缀在前端剥不干净的情况太多，统一按最后一个逗号切。
        let payload = data_base64
            .rsplit_once(',')
            .map(|(_, data)| data)
            .unwrap_or(&data_base64);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .map_err(|e| format!("The attachment is not valid base64: {e}"))?;
        attachments::save_bytes(&vault, &resolved_note, file_name.as_deref(), &mime, &bytes)
    })();
    match saved {
        Ok(saved) => ok(&request, json_value(saved)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_attachment_save_from_path(request: Request, handle: &NotebookHandle) -> Frame {
    let note = try_f!(need_str(&request, "note"));
    let src = try_f!(need_str(&request, "src"));
    let saved = (|| -> Result<attachments::SavedAttachment, String> {
        // `src` 不过 resolve_in_vaults —— 它是 vault 外的路径（如下载目录）。
        // 安全边界在写入侧：目标目录由 owning_vault 算出来，源只被读。
        let resolved_note = handle.resolve_in_vaults(&note, false)?;
        let vault = handle.owning_vault(&resolved_note)?;
        attachments::save_from_path(&vault, &resolved_note, Path::new(&src))
    })();
    match saved {
        Ok(saved) => ok(&request, json_value(saved)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_attachment_list(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let max = try_f!(need_opt::<usize>(&request, "max"));
    let root = try_s!(request, vault_root(handle, &vault));
    match attachments::list(&root, max.unwrap_or(attachments::DEFAULT_LIST_LIMIT)) {
        Ok(items) => ok(&request, json!({ "items": items })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_attachment_read(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    let read = handle
        .resolve_in_vaults(&path, false)
        .and_then(|resolved| attachments::read(&resolved));
    match read {
        Ok(bytes) => ok(
            &request,
            json!({ "dataBase64": base64::engine::general_purpose::STANDARD.encode(bytes) }),
        ),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

// ── 元数据与扫描面 ───────────────────────────────────────────────────────────

fn nb_note_stat(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    let stat = handle
        .resolve_in_vaults(&path, false)
        .and_then(|resolved| aeroric_core::notebook::stat_note(&resolved));
    match stat {
        Ok(stat) => ok(&request, json_value(stat)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_vault_index(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match vault_index::scan_vault_titles(&resolved) {
        Ok(entries) => ok(&request, json!({ "entries": entries })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_vault_links(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match links::scan_vault_links(&resolved) {
        Ok(items) => ok(&request, json!({ "links": items })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_vault_tags(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match aeroric_core::notebook::tags::scan_vault_tags(&resolved) {
        Ok(items) => ok(&request, json!({ "tags": items })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_vault_fields(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match fields::scan_vault_fields(&resolved) {
        Ok(items) => ok(&request, json!({ "fields": items })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_vault_tasks(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match tasks::scan_vault_tasks(&resolved) {
        Ok(items) => ok(&request, json!({ "tasks": items })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_vault_mentions(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let note = try_f!(need_str(&request, "note"));
    let names = try_f!(need::<Vec<String>>(&request, "names"));
    let scanned = (|| -> Result<Vec<mentions::MentionSource>, String> {
        let resolved = handle.resolve_in_vaults(&vault, false)?;
        let note_path = handle.resolve_in_vaults(&note, false)?;
        mentions::scan_vault_mentions(&resolved, &note_path, &names)
    })();
    match scanned {
        Ok(items) => ok(&request, json!({ "mentions": items })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_link_mentions(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let targets = try_f!(need::<Vec<mentions::MentionTarget>>(&request, "targets"));
    let root = try_s!(request, vault_root(handle, &vault));
    match mentions::link_mentions(handle, &root, &targets) {
        Ok(report) => ok(&request, json_value(report)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_rename_tag(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let old = try_f!(need_str(&request, "old"));
    let new = try_f!(need_str(&request, "new"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match tag_rename::rename_vault_tag(handle, &resolved, &old, &new) {
        Ok(report) => ok(&request, json_value(report)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

// ── 版本历史 ─────────────────────────────────────────────────────────────────

fn nb_list_snapshots(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    match snapshots::list(handle, &path) {
        Ok(entries) => ok(&request, json!({ "entries": entries })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_read_snapshot(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    let entry_id = try_f!(need_str(&request, "entryId"));
    match snapshots::read(handle, &path, &entry_id) {
        Ok(snapshot) => ok(&request, json_value(snapshot)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_restore_snapshot(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    let entry_id = try_f!(need_str(&request, "entryId"));
    match snapshots::restore(handle, &path, &entry_id) {
        Ok(restored) => ok(&request, json_value(restored)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

// ── 迁移与模板 ───────────────────────────────────────────────────────────────

fn nb_migrate_legacy(request: Request, handle: &NotebookHandle) -> Frame {
    let raw_json = try_f!(need_str(&request, "rawJson"));
    let report = migrate::ensure_default_vault().and_then(|vault| {
        handle.register_vault(&vault)?;
        migrate::migrate_legacy_notes(&vault, &raw_json)
    });
    match report {
        Ok(report) => ok(&request, json_value(report)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_convert_richtext(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let resolved = try_s!(request, handle.resolve_in_vaults(&vault, false));
    match migrate::convert_richtext_notes(&resolved) {
        Ok(report) => ok(&request, json_value(report)),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_list_user_templates(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let root = try_s!(request, vault_root(handle, &vault));
    ok(
        &request,
        json!({ "templates": user_templates::list_user_templates(&root) }),
    )
}

// ── 导入 ─────────────────────────────────────────────────────────────────────

fn nb_import_obsidian(request: Request, handle: &NotebookHandle) -> Frame {
    let (root, source) = try_f!(import_params(&request, "sourcePath", handle));
    run_import(&request, || import::obsidian::import(&root, &source))
}

fn nb_import_logseq(request: Request, handle: &NotebookHandle) -> Frame {
    let (root, source) = try_f!(import_params(&request, "sourcePath", handle));
    run_import(&request, || import::logseq::import(&root, &source))
}

fn nb_import_notion(request: Request, handle: &NotebookHandle) -> Frame {
    let (root, source) = try_f!(import_params(&request, "zipPath", handle));
    run_import(&request, || import::notion::import(&root, &source))
}

fn nb_import_bear(request: Request, handle: &NotebookHandle) -> Frame {
    let (root, source) = try_f!(import_params(&request, "zipPath", handle));
    run_import(&request, || import::bear::import(&root, &source))
}

fn nb_import_roam(request: Request, handle: &NotebookHandle) -> Frame {
    let (root, source) = try_f!(import_params(&request, "zipPath", handle));
    run_import(&request, || import::roam::import(&root, &source))
}

fn nb_import_evernote(request: Request, handle: &NotebookHandle) -> Frame {
    let (root, source) = try_f!(import_params(&request, "enexPath", handle));
    run_import(&request, || import::evernote::import(&root, &source))
}

fn nb_import_apple_notes(request: Request, handle: &NotebookHandle) -> Frame {
    let vault = try_f!(need_str(&request, "vault"));
    let root = try_s!(request, vault_root(handle, &vault));
    run_import(&request, || import::apple_notes::import(&root))
}

/// 目录/归档型导入的共同参数：vault 归一 + 源路径。
fn import_params(
    request: &Request,
    source_key: &str,
    handle: &NotebookHandle,
) -> Result<(PathBuf, PathBuf), Box<Frame>> {
    let vault = need_str(request, "vault")?;
    let source = need_str(request, source_key)?;
    let root = match vault_root(handle, &vault) {
        Ok(root) => root,
        Err(e) => {
            return Err(Box::new(failure(
                request.id.clone(),
                ErrorCode::IoError,
                &e,
            )))
        }
    };
    Ok((root, PathBuf::from(source)))
}

fn run_import(
    request: &Request,
    body: impl FnOnce() -> Result<import::ImportReport, String>,
) -> Frame {
    match body() {
        Ok(report) => ok(request, json_value(report)),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e),
    }
}

// ── 导出 ─────────────────────────────────────────────────────────────────────

fn nb_export_write_file(request: Request, handle: &NotebookHandle) -> Frame {
    let path = try_f!(need_str(&request, "path"));
    let content = try_f!(need_str(&request, "content"));
    let written = handle
        .registered_vaults()
        .and_then(|vaults| export::write_export(&path, &content, &export::export_roots(vaults)));
    match written {
        Ok(()) => ok(&request, json!({ "written": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_export_site_write(request: Request, handle: &NotebookHandle) -> Frame {
    let out_dir = try_f!(need_str(&request, "outDir"));
    let rel_path = try_f!(need_str(&request, "relPath"));
    let content = try_f!(need_str(&request, "content"));
    let written = handle.registered_vaults().and_then(|vaults| {
        export::write_site_page(&out_dir, &rel_path, &content, &export::export_roots(vaults))
    });
    match written {
        Ok(()) => ok(&request, json!({ "written": true })),
        Err(e) => failure(request.id, ErrorCode::IoError, &e),
    }
}

fn nb_html_to_markdown(request: Request, _handle: &NotebookHandle) -> Frame {
    let html = try_f!(need_str(&request, "html"));
    ok(
        &request,
        json!({ "markdown": html2md::html_to_markdown(&html, false) }),
    )
}

// ── 共用 helpers ─────────────────────────────────────────────────────────────

fn json_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// 把前端传来的 vault 路径校验并归一到 vault 根（与 Tauri 命令体同款两步）。
fn vault_root(handle: &NotebookHandle, vault: &str) -> Result<PathBuf, String> {
    let resolved = handle.resolve_in_vaults(vault, false)?;
    handle.owning_vault(&resolved)
}

fn need_str(request: &Request, key: &str) -> Result<String, Box<Frame>> {
    match request.params.get(key).and_then(Value::as_str) {
        Some(value) => Ok(value.to_string()),
        None => Err(Box::new(missing(request, key))),
    }
}

fn need<T: DeserializeOwned>(request: &Request, key: &str) -> Result<T, Box<Frame>> {
    let value = request
        .params
        .get(key)
        .ok_or_else(|| Box::new(missing(request, key)))?;
    serde_json::from_value::<T>(value.clone()).map_err(|e| {
        Box::new(failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &format!("params.{key}: {e}"),
        ))
    })
}

fn need_opt<T: DeserializeOwned>(request: &Request, key: &str) -> Result<Option<T>, Box<Frame>> {
    match request.params.get(key) {
        Some(value) => serde_json::from_value::<T>(value.clone())
            .map(Some)
            .map_err(|e| {
                Box::new(failure(
                    request.id.clone(),
                    ErrorCode::InvalidArgument,
                    &format!("params.{key}: {e}"),
                ))
            }),
        None => Ok(None),
    }
}

fn missing(request: &Request, key: &str) -> Frame {
    failure(
        request.id.clone(),
        ErrorCode::InvalidArgument,
        &format!("requires params.{key}"),
    )
}
