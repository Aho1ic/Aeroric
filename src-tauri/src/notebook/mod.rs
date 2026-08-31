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
//!     history/                  版本历史快照(每文件 30 条)
//!     trash/                    软删的笔记 + 清单
//! <project>/.aeroric/notes/     项目级 vault(默认不开启)
//! ```
//!
//! 命令一律 `notebook_` 前缀,并且必须逐个列进 `lib.rs` 的 `generate_handler!`
//! —— 那个宏用自己的语法解析参数列表,聚合宏在那个位置展不开(见文件末尾的注)。
//! 漏注册由 `command_registration_tests` 守卫。

pub mod attachments;
pub mod export;
pub mod fields;
pub mod fs_ops;
pub mod html2md;
pub mod links;
pub mod mentions;
pub mod migrate;
pub mod rag;
pub mod snapshots;
pub mod state;
pub mod tag_rename;
pub mod tags;
pub mod tasks;
pub mod trash;
pub mod user_templates;
pub mod vault_index;
mod vault_walk;

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

/// 读自定义图标(vault 相对路径 → 图标名)。缺失或损坏时返回空表 —— 图标丢了
/// 只是回落到默认图标,不该让面板打不开。
#[tauri::command]
pub async fn notebook_read_icons(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || Ok(fs_ops::read_icons(&resolved))).await
}

/// 写自定义图标。整张表一起写 —— 图标只在用户点选时改,一次一张表比维护
/// 增删两条命令简单,也不会出现"删到一半"的中间态。
#[tauri::command]
pub async fn notebook_write_icons(
    state: State<'_, NotebookState>,
    vault: String,
    icons: std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || fs_ops::write_icons(&resolved, &icons)).await
}

#[tauri::command]
pub async fn notebook_open_note(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<fs_ops::OpenedNote, String> {
    fs_ops::read_note(&state, &path)
}

/// 只读地取一篇笔记的内容。给嵌入(`![[note]]`)用。
///
/// 和 `notebook_open_note` 的区别只有一条:**不**登记指纹表。区别的理由见
/// `fs_ops::read_note_content` 的文档注释 —— 简单说,嵌入不是"打开",把它记成打开
/// 会让别人的保存拿一个没人持有的基线去比。
#[tauri::command]
pub async fn notebook_peek_note(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<fs_ops::OpenedNote, String> {
    fs_ops::read_note_content(&state, &path).map(|(_, opened)| opened)
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

/// 删除笔记 / 文件夹。软删到 `<vault>/.notebook/trash/`,不是 unlink,也不是
/// 直接进系统回收站。
///
/// 为什么不用系统回收站:它记不住"这条原来在 vault 的哪个子目录",恢复一个
/// `untitled.md` 时用户根本不知道该往哪放;跨到系统回收站还可能是跨设备复制,
/// 大 vault 上会卡住 IPC。系统回收站是**彻底删除**的落点,见
/// [`notebook_trash_purge`]。
#[tauri::command]
pub async fn notebook_delete_note(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<trash::TrashItem, String> {
    let resolved = state.resolve_in_vaults(&path, false)?;
    let vault = state.owning_vault(&resolved)?;
    let target = resolved.clone();
    let item = blocking(move || trash::trash(&vault, &target)).await?;
    // 指纹在软删**成功之后**才清:失败的话文件还在原位,提前清掉会让下一次
    // 保存拿不到基线而被当成冲突。目录要连带清子树里的每个文件。
    if item.is_dir {
        state.record_close_subtree(&resolved)?;
    } else {
        state.record_close(&resolved)?;
    }
    Ok(item)
}

// ── 回收站 ─────────────────────────────────────────────────────────────────

/// 列出 vault 回收站,新删的在前。
#[tauri::command]
pub async fn notebook_trash_list(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<trash::TrashItem>, String> {
    let root = resolve_vault_root(&state, &vault)?;
    blocking(move || trash::list(&root)).await
}

/// 把一条恢复回原路径。原路径已被占用时报 `ALREADY_EXISTS:<path>`。
#[tauri::command]
pub async fn notebook_trash_restore(
    state: State<'_, NotebookState>,
    vault: String,
    id: String,
) -> Result<trash::RestoredItem, String> {
    let root = resolve_vault_root(&state, &vault)?;
    blocking(move || trash::restore(&root, &id)).await
}

/// 彻底删除一条:载荷进系统回收站,清单和历史快照删掉。
#[tauri::command]
pub async fn notebook_trash_purge(
    state: State<'_, NotebookState>,
    vault: String,
    id: String,
) -> Result<(), String> {
    let root = resolve_vault_root(&state, &vault)?;
    blocking(move || trash::purge(&root, &id)).await
}

/// 清空回收站,返回清掉的条数。每条都走单条彻底删除的那套路径。
#[tauri::command]
pub async fn notebook_trash_purge_all(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<u32, String> {
    let root = resolve_vault_root(&state, &vault)?;
    blocking(move || trash::purge_all(&root)).await
}

/// 把前端传来的 vault 路径校验并归一到 vault 根。
///
/// 两步都要:`resolve_in_vaults` 过 allowlist 并 canonicalize(前端传的可能带
/// symlink 或 `..`),`owning_vault` 再把它收敛到**注册时的那个根** —— 否则传一个
/// vault 内的子目录进来,回收站就会开在那个子目录下面,而 `list` 又永远去根下面
/// 找,删掉的笔记从此消失。
fn resolve_vault_root(
    state: &State<'_, NotebookState>,
    vault: &str,
) -> Result<std::path::PathBuf, String> {
    let resolved = state.resolve_in_vaults(vault, false)?;
    state.owning_vault(&resolved)
}

#[tauri::command]
pub async fn notebook_rename_note(
    state: State<'_, NotebookState>,
    from: String,
    to: String,
) -> Result<(), String> {
    fs_ops::rename_note(&state, &from, &to)
}

// ── 附件 ───────────────────────────────────────────────────────────────────

/// 存一份剪贴板 / 网页拖来的附件。`dataBase64` 允许带 `data:...;base64,` 前缀。
#[tauri::command]
pub async fn notebook_attachment_save(
    state: State<'_, NotebookState>,
    note: String,
    mime: String,
    data_base64: String,
    file_name: Option<String>,
) -> Result<attachments::SavedAttachment, String> {
    use base64::Engine;
    let resolved_note = state.resolve_in_vaults(&note, false)?;
    let vault = state.owning_vault(&resolved_note)?;
    // data URL 前缀在前端剥不干净的情况太多(有的浏览器给 `;charset=`),
    // 这里统一按最后一个逗号切。
    let payload = data_base64
        .rsplit_once(',')
        .map(|(_, data)| data)
        .unwrap_or(&data_base64);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|e| format!("The attachment is not valid base64: {e}"))?;
    blocking(move || {
        attachments::save_bytes(&vault, &resolved_note, file_name.as_deref(), &mime, &bytes)
    })
    .await
}

/// 把磁盘上的文件复制进附件目录(从文件管理器拖入)。
///
/// `src` 不过 `resolve_in_vaults` —— 它是 vault **外**的路径,那道闸门会拒掉。
/// 这里的安全边界在写入侧:目标目录由 `owning_vault` 算出来,源只被读。
#[tauri::command]
pub async fn notebook_attachment_save_from_path(
    state: State<'_, NotebookState>,
    note: String,
    src: String,
) -> Result<attachments::SavedAttachment, String> {
    let resolved_note = state.resolve_in_vaults(&note, false)?;
    let vault = state.owning_vault(&resolved_note)?;
    let src = std::path::PathBuf::from(src);
    blocking(move || attachments::save_from_path(&vault, &resolved_note, &src)).await
}

/// 列出 vault 里的附件,新的在前。
#[tauri::command]
pub async fn notebook_attachment_list(
    state: State<'_, NotebookState>,
    vault: String,
    max: Option<usize>,
) -> Result<Vec<attachments::Attachment>, String> {
    let root = resolve_vault_root(&state, &vault)?;
    let max = max.unwrap_or(attachments::DEFAULT_LIST_LIMIT);
    blocking(move || attachments::list(&root, max)).await
}

/// 读一个附件的原始字节。前端拿它做 blob URL 显示图片。
///
/// 走 `resolve_in_vaults`:附件读取和笔记读写共用同一道 allowlist,不因为
/// "只是读一张图"就放宽到任意路径。
///
/// 返回 `ipc::Response` 而不是 `Vec<u8>`:后者会被序列化成 JSON 数字数组,一张
/// 5MB 的图变成十几 MB 的文本,而且前端还要再逐个元素转回字节。`Response` 走的
/// 是原始 body,前端直接拿到 ArrayBuffer。
#[tauri::command]
pub async fn notebook_attachment_read(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<tauri::ipc::Response, String> {
    let resolved = state.resolve_in_vaults(&path, false)?;
    let bytes = blocking(move || attachments::read(&resolved)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// 一条笔记在磁盘上的元数据。属性面板用。
///
/// 面板里那份 `NotebookNote` 不带这些:它的 `updatedAt` 是**打开时**的时间戳,而
/// 属性面板要回答的是"这个文件现在多大、什么时候改的"。缓冲区里还没保存的编辑
/// 不该改变这两个数,所以这里直接看盘。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteStat {
    pub size: u64,
    pub modified_ms: i64,
    /// 创建时间。取不到就是 None —— 有些 Linux 文件系统不记它,报一个假的
    /// 时间比留空更糟。
    pub created_ms: Option<i64>,
}

/// 看一个文件的元数据。命令体单独提出来是为了能不经 Tauri `State` 直接测。
pub(crate) fn stat_note(path: &std::path::Path) -> Result<NoteStat, String> {
    // `symlink_metadata` 而不是 `metadata`:笔记树不跟进软链,属性面板也不该跟 ——
    // 否则它报的是链接指向的那个文件的大小,而用户看的是这一条笔记。
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    if meta.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }
    Ok(NoteStat {
        size: meta.len(),
        modified_ms: system_time_ms(meta.modified().ok()).unwrap_or(0),
        created_ms: system_time_ms(meta.created().ok()),
    })
}

/// 读一条笔记的磁盘元数据。
#[tauri::command]
pub async fn notebook_note_stat(
    state: State<'_, NotebookState>,
    path: String,
) -> Result<NoteStat, String> {
    let resolved = state.resolve_in_vaults(&path, false)?;
    blocking(move || stat_note(&resolved)).await
}

/// 扫全库,给出每条笔记的**真实标题**(frontmatter 优先)。
///
/// 前端的 `listNotes` 只读目录项,给未读入的笔记填的是文件名 stem。可是
/// `[[链接]]` 写的是标题,而标题存在 frontmatter 里 —— 不扫一遍文件头,指向
/// "还没打开过的笔记"的链接就全是死链。这条命令补的正是那份缺口。
#[tauri::command]
pub async fn notebook_vault_index(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<vault_index::VaultIndexEntry>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || vault_index::scan_vault_titles(&resolved)).await
}

/// 扫全库的 `[[wikilink]]` 出现,给反链面板用。
///
/// 返回的是**未解析**的原始 body + 行号 + 行预览:解析规则(stem / 标题 / 路径的
/// 优先级、`#小节`、`|别名`、歧义)在前端 `noteLinks.ts` 里,那份有测试。在 Rust
/// 里抄一遍会得到两套会各自漂移的规则,而漂移的表现是"链接能点,反链面板里却
/// 没有它"—— 没人会想到怀疑是两边不一致。
#[tauri::command]
pub async fn notebook_vault_links(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<links::NoteLinkSource>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || links::scan_vault_links(&resolved)).await
}

/// 扫全库的行内 `#标签`,每篇一条(含标签文本、行号、行预览)。
///
/// 聚合(哪个标签有几处、分布在哪几篇)在前端做:那是纯计算,而且标签云的排序 /
/// 筛选都要在同一份数据上反复算,来回过 IPC 只会更慢。
///
/// frontmatter 里的 `tags:` 不在这里 —— 那是另一套机制(要动 YAML),见 `tags.rs`
/// 的模块注释。
#[tauri::command]
pub async fn notebook_vault_tags(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<tags::NoteTagSource>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || tags::scan_vault_tags(&resolved)).await
}

/// 扫全库的 frontmatter 字段,每篇一条(key + 该篇里的值)。
///
/// 和 `notebook_vault_tags` 同一个分工:聚合(全库有哪些 key、某个值命中哪几篇)在
/// 前端做。frontmatter 的边界与标题索引共用一份解析,见 `fields.rs` 的模块注释。
#[tauri::command]
pub async fn notebook_vault_fields(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<fields::NoteFieldSource>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || fields::scan_vault_fields(&resolved)).await
}

/// 扫全库的 `- [ ]` 任务,每篇一条(行号 + 完成态 + 任务原文)。
///
/// 和 `notebook_vault_tags` 同一个分工:结构化解析(#标签、@截止、!优先级)与分组在
/// 前端做。行号按整个 `.md` 文件数,和标签 / 反链同一个坐标系 —— 它**不是**勾选写回
/// 用的那个坐标系,见 `tasks.rs` 的模块注释。
#[tauri::command]
pub async fn notebook_vault_tasks(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<tasks::NoteTaskSource>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    blocking(move || tasks::scan_vault_tasks(&resolved)).await
}

/// 扫全库的**未链接提及**:写了 `names` 里任一名字、却没写成 `[[链接]]` 的地方。
///
/// `names` 由前端给 —— 一篇笔记的可链接名字有哪些(frontmatter 标题、文件名 stem、
/// 以后的别名)是 `noteLinks.ts` 的解析规则,那份有测试。在 Rust 里再判一次会得到两套
/// 会各自漂移的"名字",而漂移的表现是"提及列表里有它、点了却包出一条死链"。
///
/// `note` 自己整篇跳过。每一处带可信度:中日韩邻字判 `ambiguous`,批量链接不动它们,
/// 见 `mentions.rs` 的模块注释。
#[tauri::command]
pub async fn notebook_vault_mentions(
    state: State<'_, NotebookState>,
    vault: String,
    note: String,
    names: Vec<String>,
) -> Result<Vec<mentions::MentionSource>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    let note_path = state.resolve_in_vaults(&note, false)?;
    blocking(move || mentions::scan_vault_mentions(&resolved, &note_path, &names)).await
}

/// 把指定的那几处提及包成 `[[..]]`,返回 changed / skipped / failed 的完整报告。
///
/// `targets` 是**用户在列表里看见过的**那几处(路径 + 字节区间 + 当时的原文)。不传
/// needle 让后端自己再全库包一遍 —— 扫描和点击之间新写的段落会被静默包上链接,而列表
/// 就不再是这次操作的完整清单。每一处在重读后逐个校验,对不上的报成 `vanished`。
///
/// 和 `notebook_rename_tag` 一样在当前线程上跑:要用 `&state`,而 `State<'_, _>`
/// 不是 `'static`。
#[tauri::command]
pub async fn notebook_link_mentions(
    state: State<'_, NotebookState>,
    vault: String,
    targets: Vec<mentions::MentionTarget>,
) -> Result<mentions::MentionLinkReport, String> {
    let resolved = resolve_vault_root(&state, &vault)?;
    mentions::link_mentions(&state, &resolved, &targets)
}

/// 跨文件把 `#old` 改成 `#new`,返回 changed / skipped / failed 的完整报告。
///
/// `old` 按归一化 key 匹配(大小写不敏感,和面板里那一行的聚合口径一致),`new` 是要
/// 写进文件的字面文本。改写按扫描器给的字节区间做 —— 数得出来的一定改得动,见
/// `tag_rename.rs` 的模块注释。
///
/// 和 `notebook_save_note` 一样在当前线程上跑而不进阻塞池:要用 `&state`,而
/// `State<'_, _>` 不是 `'static`。批量写盘确实更重,但每篇都走既有的冲突检测和版本
/// 快照那一路,改成后台任务得先把那套东西也搬过去。
#[tauri::command]
pub async fn notebook_rename_tag(
    state: State<'_, NotebookState>,
    vault: String,
    old: String,
    new: String,
) -> Result<tag_rename::TagRenameReport, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    tag_rename::rename_vault_tag(&state, &resolved, &old, &new)
}

/// `SystemTime` → epoch 毫秒。1970 之前的时间戳(时钟错乱、坏归档)取不到就是 None。
fn system_time_ms(time: Option<std::time::SystemTime>) -> Option<i64> {
    time?
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
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

// ── 自定义模板 ─────────────────────────────────────────────────────────────

/// 列出 `<vault>/.notebook/templates/*.md`。目录不存在时是空表,不是错误。
///
/// 占位符不在后端替换 —— 日期要按 webview 的本地时区算,见 `user_templates` 的
/// 模块注释。
#[tauri::command]
pub async fn notebook_list_user_templates(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<Vec<user_templates::UserTemplate>, String> {
    let root = resolve_vault_root(&state, &vault)?;
    blocking(move || Ok(user_templates::list_user_templates(&root))).await
}

// ── 导出 ───────────────────────────────────────────────────────────────────

/// 写一个单文档导出(HTML / Markdown)到用户在保存对话框里选的路径。
///
/// 路径来自前端,所以**不能**当成用户意图的证明:白名单在 `export.rs` 那边把写入
/// 位置限死在笔记库和桌面 / 文档 / 下载目录内。
#[tauri::command]
pub async fn notebook_export_write_file(
    state: State<'_, NotebookState>,
    path: String,
    content: String,
) -> Result<(), String> {
    let roots = export::export_roots(state.registered_vaults()?);
    blocking(move || export::write_export(&path, &content, &roots)).await
}

/// 写整库静态站点里的一页。`relPath` 是仓库结构算出来的站内相对路径。
#[tauri::command]
pub async fn notebook_export_site_write(
    state: State<'_, NotebookState>,
    out_dir: String,
    rel_path: String,
    content: String,
) -> Result<(), String> {
    let roots = export::export_roots(state.registered_vaults()?);
    blocking(move || export::write_site_page(&out_dir, &rel_path, &content, &roots)).await
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
