//! 随手记(quick notes)的后端。
//!
//! 随手记原本是 localStorage 里的一个 JSON 数组。这个模块把它变成磁盘上真正
//! 的 `.md` 文件 —— 这是后续所有能力(双链、全文检索、历史、同步、导入导出)
//! 的前提:它们都以"一条笔记 = 一个文件"为基础。
//!
//! 布局:
//! ```text
//! ~/.aeroric/notebook/          全局默认 vault(localStorage 迁移落点)
//!   *.md
//!   .notebook/                  vault 私有数据,不入 Git
//!     legacy-backup-*.json      迁移前的原始 localStorage 快照
//! <project>/.aeroric/notes/     项目级 vault(默认不开启)
//! ```
//!
//! 命令一律 `notebook_` 前缀。为了不让 `lib.rs` 的 `generate_handler!` 继续
//! 膨胀,这里用 [`notebook_commands!`] 宏聚合,`lib.rs` 只出现一行。

pub mod fs_ops;
pub mod html2md;
pub mod migrate;
pub mod snapshots;
pub mod state;

#[cfg(test)]
mod tests;

use state::{FileSig, NotebookState};
use tauri::State;

/// 把命令跑到阻塞线程池上。所有文件 IO 都走这里,免得堵住 webview 的 IPC。
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

// ── vault 注册 ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn notebook_register_vault(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<String, String> {
    let canon = state.register_vault(std::path::Path::new(&path))?;
    Ok(canon.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn notebook_unregister_vault(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<(), String> {
    state.unregister_vault(std::path::Path::new(&path))
}

#[tauri::command]
pub async fn notebook_list_vaults(state: State<'_, NotebookState>) -> Result<Vec<String>, String> {
    Ok(state
        .registered_vaults()?
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect())
}

/// 确保全局默认 vault 存在并注册它。前端启动时调一次 —— 不调的话所有文件
/// 命令都会因为 allowlist 为空而被拒。
#[tauri::command]
pub async fn notebook_ensure_default_vault(
    state: State<'_, NotebookState>,
) -> Result<String, String> {
    let vault = blocking(migrate::ensure_default_vault).await?;
    let canon = state.register_vault(&vault)?;
    Ok(canon.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn notebook_ensure_project_vault(
    state: State<'_, NotebookState>,
    project_path: String,
) -> Result<String, String> {
    let vault = blocking(move || migrate::ensure_project_vault(&project_path)).await?;
    let canon = state.register_vault(&vault)?;
    Ok(canon.to_string_lossy().to_string())
}

// ── 读写 ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn notebook_read_tree(
    state: State<'_, NotebookState>,
    root: String,
) -> Result<Vec<fs_ops::NoteEntry>, String> {
    fs_ops::read_tree(&state, &root)
}

/// 读手工排序(文件名列表)。缺失或损坏时返回空列表 —— 排序丢了只是回落到
/// 按修改时间排,不该让面板打不开。
#[tauri::command]
pub async fn notebook_read_order(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<String>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || Ok(fs_ops::read_order(&resolved))).await
}

#[tauri::command]
pub async fn notebook_write_order(
    state: State<'_, NotebookState>,
    vault: String,
    names: Vec<String>,
) -> Result<(), String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || fs_ops::write_order(&resolved, &names)).await
}

#[tauri::command]
pub async fn notebook_open_note(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<fs_ops::OpenedNote, String> {
    fs_ops::read_note(&state, &path)
}

#[tauri::command]
pub async fn notebook_close_note(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<(), String> {
    // 关闭时路径可能已经不存在(文件刚被删)。解析失败就按原路径清一次,
    // 免得指纹表里留下永远清不掉的条目。
    match state.resolve_in_vaults(&path, false) {
        Ok(resolved) => state.record_close(&resolved),
        Err(_) => state.record_close(std::path::Path::new(&path)),
    }
}

#[tauri::command]
pub async fn notebook_save_note(
    state: State<'_, NotebookState>,
    path: String,
    content: String,
    expected: Option<FileSig>,
    force: Option<bool>,
) -> Result<fs_ops::SaveOutcome, String> {
    fs_ops::save_note(&state, &path, &content, expected, force.unwrap_or(false))
}

#[tauri::command]
pub async fn notebook_create_note(
    state: State<'_, NotebookState>,
    path: String,
    content: String,
) -> Result<FileSig, String> {
    fs_ops::create_note(&state, &path, &content)
}

/// 按新标题给笔记重新分配文件名并改名。
///
/// 用途:新建笔记时标题还是空的,文件只能先叫 `untitled.md`。用户敲完标题后
/// 调这个命令把文件名对上 —— 否则 vault 在 Finder 里就是一堆 `untitled-N.md`,
/// 而「笔记是你能直接打开的文件」这个前提也就没了意义。
///
/// 返回新路径。如果算出来的名字和现在一样就原样返回,不做无谓的 rename。
#[tauri::command]
pub async fn notebook_rename_to_title(
    state: State<'_, NotebookState>,
    vault: String,
    path: String,
    title: String,
) -> Result<String, String> {
    let vault_path = state.resolve_in_vaults(&vault, false)?;
    let current = state.resolve_in_vaults(&path, false)?;
    let target = {
        let vault_path = vault_path.clone();
        blocking(move || migrate::allocate_note_path(&vault_path, &title)).await?
    };
    if target == current {
        return Ok(current.to_string_lossy().to_string());
    }
    let target_text = target.to_string_lossy().to_string();
    fs_ops::rename_note(&state, &path, &target_text)?;
    Ok(target_text)
}

/// 按标题在 vault 里新建笔记,文件名由后端分配。
///
/// 返回创建出来的路径和指纹。前端不参与命名 —— slug 规则含 Windows 保留名和
/// UTF-8 边界截断这些平台细节,放两份实现迟早漂。
#[tauri::command]
pub async fn notebook_create_note_in_vault(
    state: State<'_, NotebookState>,
    vault: String,
    title: String,
    content: String,
) -> Result<CreatedNote, String> {
    // 先过 allowlist:vault 必须是已注册的,否则这就是个任意写入口。
    let vault_path = state.resolve_in_vaults(&vault, false)?;
    let target = blocking(move || migrate::allocate_note_path(&vault_path, &title)).await?;
    let path = target.to_string_lossy().to_string();
    let sig = fs_ops::create_note(&state, &path, &content)?;
    Ok(CreatedNote { path, sig })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedNote {
    pub path: String,
    pub sig: FileSig,
}

#[tauri::command]
pub async fn notebook_create_folder(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<(), String> {
    fs_ops::create_folder(&state, &path)
}

/// 删除笔记。走系统回收站(Aeroric 已有 `trash` crate),不是 unlink ——
/// 随手记的删除必须可恢复。P3 会在此之上再加 vault 内的软删层。
#[tauri::command]
pub async fn notebook_delete_note(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<(), String> {
    let resolved = state.resolve_in_vaults(&path, false)?;
    state.record_close(&resolved)?;
    blocking(move || trash::delete(&resolved).map_err(|e| format!("Cannot move to trash: {e}")))
        .await
}

#[tauri::command]
pub async fn notebook_rename_note(
    state: State<'_, NotebookState>,
    from: String,
    to: String,
) -> Result<(), String> {
    fs_ops::rename_note(&state, &from, &to)
}

// ── 版本历史 ───────────────────────────────────────────────────────────────

/// 列出一条笔记的快照,新的在前。
///
/// 不复用 `list_local_history`:那个命令按项目根 + `.aeroric/local-history`
/// 定位,而随手记的仓库根是 vault、目录是 `.notebook/history`,而且校验走的是
/// vault allowlist 而不是项目根。
#[tauri::command]
pub async fn notebook_list_snapshots(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<Vec<crate::local_history::LocalHistoryEntry>, String> {
    snapshots::list(&state, &path)
}

#[tauri::command]
pub async fn notebook_read_snapshot(
    state: State<'_, NotebookState>,
    path: String,
    entry_id: String,
) -> Result<crate::local_history::LocalHistorySnapshot, String> {
    snapshots::read(&state, &path, &entry_id)
}

/// 回滚到某条快照。回滚前会把当前内容也存成一条快照,所以这一步可撤销。
#[tauri::command]
pub async fn notebook_restore_snapshot(
    state: State<'_, NotebookState>,
    path: String,
    entry_id: String,
) -> Result<snapshots::RestoredNote, String> {
    snapshots::restore(&state, &path, &entry_id)
}

// ── 迁移 ───────────────────────────────────────────────────────────────────

/// 把 localStorage 里的随手记迁到磁盘。
///
/// 前端传原始 JSON 字符串(不是解析后的对象)—— 备份要存的是真正的原文,
/// 包括后端可能不认识的字段。
///
/// 成功返回后前端才可以把 localStorage 的键改名。**不要删** —— 留一个版本
/// 周期的回退余地。
#[tauri::command]
pub async fn notebook_migrate_legacy(
    state: State<'_, NotebookState>,
    raw_json: String,
) -> Result<migrate::MigrationReport, String> {
    let vault = blocking(migrate::ensure_default_vault).await?;
    state.register_vault(&vault)?;
    blocking(move || migrate::migrate_legacy_notes(&vault, &raw_json)).await
}

/// 把 vault 里所有 `editor: richtext` 的笔记转成 Markdown(P1 收尾迁移)。
///
/// P0 为了无损把富文本的 HTML 原样落进了 `.md`。WYSIWYG 到位后这些笔记该变成
/// 真正的 Markdown —— 否则参与不了双链、RAG 分块、导出。
///
/// 转换前会把每个待转文件备份到 `.notebook/richtext-backup-<ts>/`。幂等:
/// 转完的笔记没有 `editor: richtext` 标记,重跑会跳过。
#[tauri::command]
pub async fn notebook_convert_richtext(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<migrate::RichtextConversionReport, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || migrate::convert_richtext_notes(&resolved)).await
}

/// 把 HTML 转成 Markdown。迁移之外,预览/粘贴富文本也会用到。
#[tauri::command]
pub async fn notebook_html_to_markdown(html: String) -> Result<String, String> {
    blocking(move || Ok(html2md::html_to_markdown(&html, false))).await
}

// 注:命令必须逐个列在 `lib.rs` 的 `generate_handler!` 里,不能用宏聚合。
// `generate_handler!` 用自己的语法解析参数列表,宏调用无法在那个位置展开
// (试过 `notebook_commands!()`,报 `expected \`,\``)。守卫测试
// `command_registration_tests` 会盯住漏注册。
