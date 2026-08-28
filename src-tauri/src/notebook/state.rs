//! 随手记仓库(vault)的注册表与文件指纹表。
//!
//! 两件事:
//! 1. **allowlist** —— 前端注册过的 vault 根目录。所有 `notebook_*` 文件命令
//!    必须落在其中,否则拒绝。这是随手记从 localStorage 走向磁盘后唯一的
//!    路径闸门。
//! 2. **指纹表** —— 已打开文件的 mtime + hash。保存时用来发现"这个文件在我
//!    编辑期间被外部改过",避免静默覆盖。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 文件指纹:mtime + 内容哈希。两者都比对,因为 mtime 在某些文件系统上
/// 精度只到秒,同秒内的改动只有 hash 能看出来。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSig {
    pub mtime_ms: i64,
    /// 序列化成字符串:u64 超出 JS 安全整数范围(2^53),走 number 会丢精度,
    /// 而丢精度的 hash 会让冲突检测随机误判。
    pub hash: String,
}

#[derive(Default)]
pub struct Inner {
    /// 已注册的 vault 根目录(均已 canonicalize)。
    pub vaults: HashSet<PathBuf>,
    /// 已打开文件的最新指纹。
    pub opened: HashMap<PathBuf, FileSig>,
}

#[derive(Default)]
pub struct NotebookState {
    pub inner: Mutex<Inner>,
}

impl NotebookState {
    pub fn register_vault(&self, path: &Path) -> Result<PathBuf, String> {
        let canon = path
            .canonicalize()
            .map_err(|e| format!("Cannot register notebook vault: {e}"))?;
        if !canon.is_dir() {
            return Err("Notebook vault path is not a directory".to_string());
        }
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        inner.vaults.insert(canon.clone());
        Ok(canon)
    }

    /// 注销 vault。目录可能已被用户删掉,所以 canonicalize 失败时退回原路径
    /// 匹配(前端通常回传上次 register 拿到的 canon path)。
    pub fn unregister_vault(&self, path: &Path) -> Result<(), String> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        inner.vaults.remove(&canon);
        // 连带丢掉该 vault 下所有文件的指纹,否则重新注册后会拿旧基线比对。
        inner.opened.retain(|file, _| !file.starts_with(&canon));
        Ok(())
    }

    pub fn registered_vaults(&self) -> Result<Vec<PathBuf>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut list: Vec<PathBuf> = inner.vaults.iter().cloned().collect();
        list.sort();
        Ok(list)
    }

    /// 校验 `target` 落在某个已注册 vault 内,返回校验后的绝对路径。
    ///
    /// `allow_missing` 为 true 时目标可以尚不存在(新建 / 保存新文件场景),
    /// 此时只 canonicalize 父目录 —— 父目录里的 `..` 和中间层 symlink 仍会被
    /// 解析,所以目录穿越依然拦得住。
    pub fn resolve_in_vaults(&self, target: &str, allow_missing: bool) -> Result<PathBuf, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        resolve_within(&inner.vaults, target, allow_missing)
    }

    /// 调用方需保证 `path` 已过 `resolve_in_vaults`。这里不再二次 canonicalize:
    /// 刚校验完文件被外部移走的话,二次 canonicalize 会失败或退化成别的 key。
    pub fn record_open(&self, path: &Path, sig: FileSig) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        inner.opened.insert(path.to_path_buf(), sig);
        Ok(())
    }

    pub fn record_close(&self, path: &Path) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        inner.opened.remove(path);
        Ok(())
    }

    pub fn last_sig(&self, path: &Path) -> Option<FileSig> {
        let inner = self.inner.lock().ok()?;
        inner.opened.get(path).cloned()
    }

    /// 文件被改名后把指纹迁到新路径,省得下次保存拿不到基线而退回宽松模式。
    pub fn record_rename(&self, from: &Path, to: &Path) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        if let Some(sig) = inner.opened.remove(from) {
            inner.opened.insert(to.to_path_buf(), sig);
        }
        Ok(())
    }
}

/// `resolve_in_vaults` 的纯函数版本,便于单测。
pub fn resolve_within(
    vaults: &HashSet<PathBuf>,
    target: &str,
    allow_missing: bool,
) -> Result<PathBuf, String> {
    let target_path = Path::new(target);
    if !target_path.is_absolute() {
        return Err("Notebook path must be absolute".to_string());
    }
    if vaults.is_empty() {
        return Err("No notebook vault is registered".to_string());
    }

    // 已存在的路径:整条解析掉(含末段 symlink)。指向 vault 外的 symlink 会
    // 在这里被拒 —— 写入路径上不接受 symlink 逃逸。
    if let Ok(canon) = target_path.canonicalize() {
        return accept_if_inside(vaults, canon, target_path);
    }

    if !allow_missing {
        return Err(format!(
            "Notebook path is outside every registered vault ({})",
            target_path.display()
        ));
    }

    let file_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid notebook file name".to_string())?;
    let parent = target_path
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    let parent_canon = parent
        .canonicalize()
        .map_err(|e| format!("Cannot resolve parent directory: {e}"))?;
    accept_if_inside(vaults, parent_canon.join(file_name), target_path)
}

fn accept_if_inside(
    vaults: &HashSet<PathBuf>,
    canon: PathBuf,
    original: &Path,
) -> Result<PathBuf, String> {
    // starts_with 是按路径组件比对的,不是字符串前缀,所以 `/vault-evil` 不会
    // 被 `/vault` 放行。
    if vaults.iter().any(|vault| canon.starts_with(vault)) {
        return Ok(canon);
    }
    Err(format!(
        "Notebook path is outside every registered vault ({})",
        original.display()
    ))
}

/// FNV-1a 64-bit。用途只是"内容变了没",不需要抗碰撞强度。
pub fn hash64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn mtime_ms_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|delta| delta.as_millis() as i64)
        .unwrap_or(0)
}

pub fn signature_for(path: &Path) -> std::io::Result<FileSig> {
    let meta = std::fs::metadata(path)?;
    let bytes = std::fs::read(path)?;
    Ok(FileSig {
        mtime_ms: mtime_ms_of(&meta),
        hash: hash64(&bytes).to_string(),
    })
}

/// 同上,但用调用方已有的字节算哈希,省一次读盘。仅在"磁盘内容确定等于这些
/// 字节"时可用(如刚写完)。
pub fn signature_for_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<FileSig> {
    let meta = std::fs::metadata(path)?;
    Ok(FileSig {
        mtime_ms: mtime_ms_of(&meta),
        hash: hash64(bytes).to_string(),
    })
}
