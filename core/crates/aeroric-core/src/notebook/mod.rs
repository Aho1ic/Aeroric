//! 随手记(quick notes)的后端（P7-05 自 `src-tauri/src/notebook/` 原样上收；
//! 命令宏与 Tauri `State` 查找留在宿主，业务主体在此）。
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
//! 路径安全模型：所有文件命令先过 [`state::NotebookState::resolve_in_vaults`]
//! 的 vault allowlist（注册过的 vault 根目录集合），解析含 symlink 落根内判定；
//! `rag/`、`sync/` 与 Tauri 命令层仍属 P7-06/宿主。

pub mod attachments;
pub mod export;
pub mod fields;
pub mod fs_ops;
pub mod html2md;
pub mod import;
pub mod links;
pub mod mentions;
pub mod migrate;
pub mod snapshots;
pub mod state;
pub mod tag_rename;
pub mod tags;
pub mod tasks;
pub mod trash;
pub mod user_templates;
pub mod vault_index;
pub mod vault_walk;

#[cfg(test)]
mod tests;

use state::FileSig;
use std::path::Path;

/// 按标题在 vault 里新建笔记的返回：创建出来的路径和指纹。
///
/// 前端不参与命名 —— slug 规则含 Windows 保留名和 UTF-8 边界截断这些平台细节,
/// 放两份实现迟早漂。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedNote {
    pub path: String,
    pub sig: FileSig,
}

/// 把前端传来的 vault 路径校验并归一到 vault 根。
///
/// 两步都要:`resolve_in_vaults` 过 allowlist 并 canonicalize(前端传的可能带
/// symlink 或 `..`),`owning_vault` 再把它收敛到**注册时的那个根** —— 否则传一个
/// vault 内的子目录进来,回收站就会开在那个子目录下面,而 `list` 又永远去根下面
/// 找,删掉的笔记从此消失。
pub fn resolve_vault_root(
    state: &state::NotebookState,
    vault: &str,
) -> Result<std::path::PathBuf, String> {
    let resolved = state.resolve_in_vaults(vault, false)?;
    state.owning_vault(&resolved)
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

/// 看一个文件的元数据。
pub fn stat_note(path: &Path) -> Result<NoteStat, String> {
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

/// `SystemTime` → epoch 毫秒。1970 之前的时间戳(时钟错乱、坏归档)取不到就是 None。
pub fn system_time_ms(time: Option<std::time::SystemTime>) -> Option<i64> {
    time?
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
}

/// 云盘那条路只接 `cloud` 目标。
///
/// 抽成函数是为了能测:命令体要宿主状态才能构造,而这条判断放在命令体内
/// 就等于没有测试守着。git 目标混进来的话会一路走到「找不到存储连接」才失败,
/// 用户看到的是失败,而真正的问题是他点错了同步方式。(git/云盘命令体属 P7-06。)
pub fn require_cloud_target(remote_id: &str, kind: &str) -> Result<(), String> {
    if kind == "cloud" {
        return Ok(());
    }
    Err(format!(
        "Remote {remote_id} is a {kind} target; use the matching sync command"
    ))
}
