//! 随手记的文件读写。核心是三件事:原子写、冲突检测、树扫描。
//!
//! 保存路径上的不变式:
//! - 写入永不留半个文件(tmp → fsync → rename)
//! - 写入前必须确认"磁盘上的内容还是我以为的那个",否则报冲突让用户决定
//! - 新建用 `create_new`,不覆盖已有文件

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::state::{signature_for, signature_for_bytes, FileSig, NotebookState};

/// vault 私有目录。用 `.notebook` 而非 `.markio`,避免和用户已装的 Markio
/// 抢同一个目录。
pub const VAULT_PRIVATE_DIR: &str = ".notebook";

/// 单个笔记文件的读取上限。超过这个尺寸的 markdown 不是笔记,是数据文件,
/// 读进 WebView 只会卡死渲染。
const MAX_NOTE_BYTES: u64 = 8 * 1024 * 1024;
/// 树扫描上限。防止用户把 home 目录挂成 vault 后扫到天荒地老。
const MAX_TREE_ENTRIES: usize = 20_000;
const MAX_TREE_DEPTH: usize = 12;

/// 扫描时永不进入的目录名(小写比对)。
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
    ".turbo",
    "coverage",
];

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_ms: i64,
    pub children: Option<Vec<NoteEntry>>,
    /// 因为触到深度/数量上限而没继续往下扫。UI 要能把这件事告诉用户,
    /// 而不是假装这个目录是空的。
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedNote {
    pub content: String,
    pub sig: FileSig,
}

/// 保存的结果。冲突不用 `Err` 表达 —— 它不是错误,是需要用户决策的正常分支,
/// 用 Err 会让前端把它和"磁盘满了"混在一个 catch 里处理。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SaveOutcome {
    Saved { sig: FileSig },
    Conflict { disk: FileSig },
}

fn is_skip_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SKIP_DIRS.iter().any(|skip| *skip == lower)
        // vault 私有目录不进树:历史快照和索引不是用户的笔记。
        || lower == VAULT_PRIVATE_DIR
}

/// 是否是随手记认的笔记文件。
pub fn is_note_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("md") | Some("markdown") | Some("mdx")
    )
}

/// 编译期确认 `dir` 落在 vault 私有目录**里面**。
///
/// 私有子目录的相对路径只能写字面量(`concat!` 吃不进 const item),所以它们和
/// [`VAULT_PRIVATE_DIR`] 之间没有语法上的联系。分家的后果不是编译报错,是历史
/// 快照和回收站跑到树扫描看得见的地方去 —— 用户会在笔记列表里看到自己删掉的
/// 文件和历史。
pub const fn assert_inside_private_dir(dir: &str) {
    let dir = dir.as_bytes();
    let private = VAULT_PRIVATE_DIR.as_bytes();
    assert!(dir.len() > private.len());
    let mut index = 0;
    while index < private.len() {
        assert!(dir[index] == private[index]);
        index += 1;
    }
    assert!(dir[private.len()] == b'/');
}

/// Windows 上 rename/remove 会被杀软和搜索索引器短暂占用,重试几次就过去了。
/// 非 Windows 只试一次。
pub(super) fn with_fs_retry<T>(
    mut operation: impl FnMut() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let attempts = if cfg!(windows) { 8 } else { 1 };
    let mut last_error = None;
    for attempt in 0..attempts {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let retryable = matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Other
                );
                if !cfg!(windows) || !retryable || attempt + 1 == attempts {
                    return Err(error);
                }
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(75 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("filesystem retry failed")))
}

/// 原子写:临时文件 → 写入 → fsync → rename。
///
/// fsync 不能省:没有它,rename 可能先落盘而数据还在 page cache 里,断电后
/// 得到一个大小正确但内容是零的文件。
pub fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;

    let unique = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("note");
    let tmp = path.with_file_name(format!(".{file_name}.{unique}.tmp"));

    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot write {}: {error}", path.display()));
    }

    if let Err(error) = with_fs_retry(|| std::fs::rename(&tmp, path)) {
        // rename 失败要收走临时文件,否则 vault 里会攒一堆 .tmp 垃圾。
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot finalize {}: {error}", path.display()));
    }
    Ok(())
}

pub fn read_note(state: &NotebookState, path: &str) -> Result<OpenedNote, String> {
    let resolved = state.resolve_in_vaults(path, false)?;
    let meta = std::fs::metadata(&resolved)
        .map_err(|e| format!("Cannot read {}: {e}", resolved.display()))?;
    if meta.is_dir() {
        return Err("Cannot open a directory as a note".to_string());
    }
    if meta.len() > MAX_NOTE_BYTES {
        return Err(format!(
            "Note is too large to open ({} bytes, limit {MAX_NOTE_BYTES})",
            meta.len()
        ));
    }
    let content = std::fs::read_to_string(&resolved)
        .map_err(|e| format!("Cannot read {}: {e}", resolved.display()))?;
    let sig = signature_for(&resolved).map_err(|e| e.to_string())?;
    state.record_open(&resolved, sig.clone())?;
    Ok(OpenedNote { content, sig })
}

/// 保存。`expected` 是前端持有的基线指纹。
///
/// 为什么优先信前端传的基线而不是进程内 `opened` 表:同一个文件可能被两个
/// tab 打开,或者被快速捕获流程写过一次。`opened` 表里存的是"最后一次读写"
/// 的指纹,可能比当前 tab 手里的基线更新 —— 拿它比对会让旧 tab 的保存被
/// 当成"没冲突"而静默覆盖掉新内容。
pub fn save_note(
    state: &NotebookState,
    path: &str,
    content: &str,
    expected: Option<FileSig>,
    force: bool,
) -> Result<SaveOutcome, String> {
    let resolved = state.resolve_in_vaults(path, true)?;

    if !force && resolved.exists() {
        let disk = signature_for(&resolved).map_err(|e| e.to_string())?;
        let baseline = expected.or_else(|| state.last_sig(&resolved));
        let stale = match baseline {
            // 两个维度都要比:mtime 挪动可能只是被 touch 过(内容没变,不算
            // 冲突);内容变了但 mtime 精度不够时只有 hash 能看出来。
            Some(base) => disk.hash != base.hash && disk.mtime_ms != base.mtime_ms,
            // 没有任何基线 —— 前端在打开文件前就要求保存。这种情况下我们无法
            // 判断磁盘内容的来历,只能报冲突,让用户确认要不要覆盖。
            None => true,
        };
        if stale {
            return Ok(SaveOutcome::Conflict { disk });
        }
    }

    // 快照记在冲突检查**之后**:报冲突的那次保存并没有写盘,给它留快照会用
    // 一堆内容相同的条目把 30 条的保留窗口冲掉。
    if resolved.exists() {
        if let Ok(vault) = state.owning_vault(&resolved) {
            super::snapshots::record_before_save(&vault, &resolved, content);
        }
    }

    atomic_write(&resolved, content)?;
    let sig = signature_for_bytes(&resolved, content.as_bytes()).map_err(|e| e.to_string())?;
    state.record_open(&resolved, sig.clone())?;
    Ok(SaveOutcome::Saved { sig })
}

/// 新建笔记。`create_new` 保证不覆盖 —— 存在时返回 Err,而不是先 exists()
/// 再写(那中间有 TOCTOU 窗口)。
pub fn create_note(state: &NotebookState, path: &str, content: &str) -> Result<FileSig, String> {
    let resolved = state.resolve_in_vaults(path, true)?;
    let parent = resolved
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;

    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&resolved)
            .map_err(|error| match error.kind() {
                // 前端靠这个前缀区分"重名"和真正的失败。
                std::io::ErrorKind::AlreadyExists => {
                    format!("ALREADY_EXISTS:{}", resolved.display())
                }
                _ => format!("Cannot create {}: {error}", resolved.display()),
            })?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("Cannot write {}: {e}", resolved.display()))?;
        file.sync_all()
            .map_err(|e| format!("Cannot sync {}: {e}", resolved.display()))?;
    }

    let sig = signature_for_bytes(&resolved, content.as_bytes()).map_err(|e| e.to_string())?;
    state.record_open(&resolved, sig.clone())?;
    Ok(sig)
}

pub fn create_folder(state: &NotebookState, path: &str) -> Result<(), String> {
    let resolved = state.resolve_in_vaults(path, true)?;
    std::fs::create_dir_all(&resolved)
        .map_err(|e| format!("Cannot create {}: {e}", resolved.display()))
}

pub fn rename_note(state: &NotebookState, from: &str, to: &str) -> Result<(), String> {
    let source = state.resolve_in_vaults(from, false)?;
    let target = state.resolve_in_vaults(to, true)?;
    if target.exists() {
        return Err(format!("ALREADY_EXISTS:{}", target.display()));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    }
    with_fs_retry(|| std::fs::rename(&source, &target))
        .map_err(|e| format!("Cannot rename {}: {e}", source.display()))?;
    state.record_rename(&source, &target)?;
    Ok(())
}

/// 扫描 vault 得到笔记树。只收目录和笔记文件 —— 附件在附件面板里单独列
/// (P3),混进树里会让树变成一个杂乱的文件浏览器。
pub fn read_tree(state: &NotebookState, root: &str) -> Result<Vec<NoteEntry>, String> {
    let resolved = state.resolve_in_vaults(root, false)?;
    if !resolved.is_dir() {
        return Err("Notebook vault root is not a directory".to_string());
    }
    let mut budget = MAX_TREE_ENTRIES;
    let (entries, _) = scan_dir(&resolved, 0, &mut budget)?;
    Ok(entries)
}

fn scan_dir(
    dir: &Path,
    depth: usize,
    budget: &mut usize,
) -> Result<(Vec<NoteEntry>, bool), String> {
    if depth >= MAX_TREE_DEPTH {
        return Ok((Vec::new(), true));
    }

    let read = std::fs::read_dir(dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;
    let mut dirs: Vec<NoteEntry> = Vec::new();
    let mut files: Vec<NoteEntry> = Vec::new();
    let mut truncated = false;

    for entry in read {
        let entry = entry.map_err(|e| format!("Cannot read entry in {}: {e}", dir.display()))?;
        if *budget == 0 {
            truncated = true;
            break;
        }

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // 用 symlink_metadata:不跟随符号链接。跟随会让指向父目录的链接把
        // 扫描拖进无限循环。
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            // 扫描过程中文件被删掉是正常的,跳过而不是让整棵树失败。
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }

        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|delta| delta.as_millis() as i64)
            .unwrap_or(0);

        if meta.is_dir() {
            if is_skip_dir(&name) {
                continue;
            }
            *budget -= 1;
            let (children, child_truncated) = scan_dir(&path, depth + 1, budget)?;
            dirs.push(NoteEntry {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: true,
                size: 0,
                modified_ms,
                children: Some(children),
                truncated: child_truncated,
            });
            continue;
        }

        if !is_note_file(&path) {
            continue;
        }
        *budget -= 1;
        files.push(NoteEntry {
            name,
            path: path.to_string_lossy().to_string(),
            is_dir: false,
            size: meta.len(),
            modified_ms,
            children: None,
            truncated: false,
        });
    }

    // 目录在前、文件在后,各自按名字排序。用 lowercase 比对,免得大小写混排
    // 在三个平台上给出三种顺序。
    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    dirs.append(&mut files);
    Ok((dirs, truncated))
}

/// vault 私有目录的绝对路径。
pub fn private_dir(vault: &Path) -> PathBuf {
    vault.join(VAULT_PRIVATE_DIR)
}

/// 手工排序的落盘位置。
fn order_file(vault: &Path) -> PathBuf {
    private_dir(vault).join("order.json")
}

/// 读手工排序(文件名列表,不含目录)。
///
/// 为什么单独存一个文件而不是往每条笔记的 frontmatter 里写 `order` 字段:
/// 拖一次要重排一批笔记,后者意味着一次拖动重写十几个文件 —— 既慢,又会把
/// 每个文件的 mtime 全部推新,连带打乱「最近修改」的语义。
pub fn read_order(vault: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(order_file(vault)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// 写手工排序。
pub fn write_order(vault: &Path, names: &[String]) -> Result<(), String> {
    let private = private_dir(vault);
    std::fs::create_dir_all(&private)
        .map_err(|e| format!("Cannot create {}: {e}", private.display()))?;
    let text = serde_json::to_string(names).map_err(|e| e.to_string())?;
    atomic_write(&order_file(vault), &text)
}
