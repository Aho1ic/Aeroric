//! 随手记的版本历史。
//!
//! 不自己实现快照仓库 —— 复用 [`crate::local_history`],只换一套布局参数。
//! 那边已经解决了两个难做对的地方:同毫秒并发用 `create_new` 抢名字(冲突就
//! 加序号后缀,不会互相覆盖),以及按时间戳裁剪保留窗口。
//!
//! 三处和代码文件不同:
//! - 目录是 `<vault>/.notebook/history/`。`.notebook` 已经被树扫描排除,
//!   快照不会作为笔记冒出来;放 `.aeroric/local-history` 反而会在 vault 里
//!   多出一个树扫描不认识的目录。
//! - 每个文件保留 30 条(计划书定的)。
//! - **有最小间隔**。随手记每 800ms 自动保存一次,不限流的话 30 条快照只覆盖
//!   二十几秒,历史面板等于没有。

use std::path::{Path, PathBuf};

use crate::local_history::{self, HistoryLayout, LocalHistoryEntry, LocalHistorySnapshot};

use super::fs_ops;
use super::state::{signature_for_bytes, FileSig, NotebookState};

/// 每个笔记保留的快照数。
const MAX_NOTE_SNAPSHOTS: usize = 30;

/// 两条快照之间至少隔三分钟。
///
/// 配 30 条上限就是 90 分钟的可回溯窗口 —— 一次写作会话的量级。调小会让窗口
/// 变短(30 条 × 间隔),调大会让"刚才那版"找不回来。
const MIN_SNAPSHOT_INTERVAL_MS: u64 = 3 * 60 * 1000;

/// 相对 vault 根的快照目录。
const NOTE_HISTORY_DIR: &str = ".notebook/history";

/// 快照仓库的布局。`pub(crate)` 是给测试用的 —— 保留上限那条只能靠"造满再数"
/// 来验,而按真实的三分钟间隔造满要跑一个半小时。测试拿这个常量换掉间隔、
/// 保留其余字段,验的就还是这里写的 30。
pub(crate) const NOTE_LAYOUT: HistoryLayout = HistoryLayout {
    dir: NOTE_HISTORY_DIR,
    max_entries: MAX_NOTE_SNAPSHOTS,
    min_interval_ms: MIN_SNAPSHOT_INTERVAL_MS,
};

/// 编译期确认快照目录还在 vault 私有目录**里面**。回收站那边也有一条同样的。
const _: () = fs_ops::assert_inside_private_dir(NOTE_HISTORY_DIR);

/// 保存前记一条快照。
///
/// 失败不阻断保存:历史是附加能力,让它把用户的编辑挡在外面是本末倒置。
/// 但也不静默 —— 走 stderr,免得"历史一直是空的"查不出原因。
pub fn record_before_save(vault: &Path, file: &Path, next_content: &str) {
    match local_history::record_snapshot_in(NOTE_LAYOUT, vault, file, Some(next_content)) {
        Ok(_) => {}
        Err(error) => {
            eprintln!(
                "notebook: cannot snapshot {} before save: {error}",
                file.display()
            );
        }
    }
}

/// 丢掉某条笔记的全部快照。
///
/// 只在"彻底删除"路径上用,所以 `file` 允许已经不存在 —— 参数是它**曾经**在的
/// 位置(快照按相对路径归档,算目录名不需要读盘)。不清的话历史会留在
/// `.notebook/history/` 里,而同路径的新笔记一出生就继承上一条的历史。
pub fn discard(vault: &Path, file: &Path) -> Result<(), String> {
    local_history::discard_history_in(NOTE_LAYOUT, vault, file)
}

pub fn list(state: &NotebookState, path: &str) -> Result<Vec<LocalHistoryEntry>, String> {
    let (vault, file) = resolve_note(state, path)?;
    local_history::list_entries_in(NOTE_LAYOUT, &vault, &file)
}

pub fn read(
    state: &NotebookState,
    path: &str,
    entry_id: &str,
) -> Result<LocalHistorySnapshot, String> {
    let (vault, file) = resolve_note(state, path)?;
    local_history::read_entry_in(NOTE_LAYOUT, &vault, &file, entry_id)
}

/// 回滚的结果。带上新指纹 —— 前端手里的基线是回滚前的,不换掉的话下一次保存
/// 会被冲突检测拦下来,而那个"冲突"是我们自己造的。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredNote {
    pub content: String,
    pub sig: FileSig,
    pub entry: LocalHistoryEntry,
}

/// 回滚到某条快照。
///
/// 顺序是"先给当前内容留一条兜底,再写" —— 反过来的话回滚本身就不可撤销了。
/// 兜底那条走 `force_snapshot_in` 无视最小间隔:限流是为了压自动保存的噪音,
/// 不该把用户主动的破坏性操作也一起压掉。
pub fn restore(state: &NotebookState, path: &str, entry_id: &str) -> Result<RestoredNote, String> {
    let (vault, file) = resolve_note(state, path)?;
    let snapshot = local_history::read_entry_in(NOTE_LAYOUT, &vault, &file, entry_id)?;
    if let Err(error) = local_history::force_snapshot_in(NOTE_LAYOUT, &vault, &file) {
        // 兜底失败就不回滚:这一步的全部意义就是让回滚可撤销。
        return Err(format!(
            "Cannot snapshot the current note before rollback: {error}"
        ));
    }
    fs_ops::atomic_write(&file, &snapshot.content)?;
    let sig = signature_for_bytes(&file, snapshot.content.as_bytes()).map_err(|e| e.to_string())?;
    state.record_open(&file, sig.clone())?;
    Ok(RestoredNote {
        content: snapshot.content,
        sig,
        entry: snapshot.entry,
    })
}

/// 解析笔记路径并找出它属于哪个 vault。
///
/// 快照目录必须落在**该笔记所在的** vault 里,不能拿"第一个注册的 vault"凑数:
/// 全局 vault 和项目 vault 可以同时注册,弄错的话项目笔记的历史会写进用户 home。
fn resolve_note(state: &NotebookState, path: &str) -> Result<(PathBuf, PathBuf), String> {
    let file = state.resolve_in_vaults(path, false)?;
    let vault = state.owning_vault(&file)?;
    Ok((vault, file))
}
