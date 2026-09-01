//! 全库签名扫描:走一遍 vault,给每个**要同步的文件**算一份 `(相对路径, mtime,
//! 大小, 内容 hash)`。
//!
//! 这是三方 diff 的「本地」那一路输入。结果是瞬态的 —— 每轮同步重算,不落盘。落盘
//! 的只有「上次同步成功时的样子」(见 [`super::store`])。
//!
//! ## 为什么不复用 `vault_walk::walk_notes`
//!
//! 那个遍历器服务反链和标签,三条限制每一条都会把附件挡在同步之外:只收
//! `is_note_file`(md/markdown/mdx)、用 `read_to_string` 只读 UTF-8、单文件上限
//! 2MB。而附件目录里全是二进制,单个可到 25MB(`attachments.rs` 的
//! `MAX_ATTACHMENT_BYTES`)。挡掉的表现不是报错,是「笔记同步过去了,图片全裂」。
//!
//! 所以这里自己走:不限扩展名、按字节读、上限放到附件之上。跳过的目录沿用
//! `is_scan_skip_dir`(它已经跳掉 `.notebook`,历史快照/回收站/索引都不是用户数据),
//! 但**不**跳 `attachments/` —— 那个只在笔记树里跳(见 `fs_ops` 的
//! `is_tree_skip_dir`),同步必须带上它。
//!
//! ## 为什么每轮都重新 hash 全库
//!
//! 「mtime 和 size 都没变就认为内容没变」是标准优化,但有一类静默漏检:等长改动
//! 落在 mtime 精度内(编辑器保存、脚本批改)。笔记是文本,几千篇合起来几十 MB,
//! FNV 是内存带宽级的;为这点时间换一类「改了但同步不认」的 bug 不值得。这条是
//! 刻意的选择。

use std::path::{Path, PathBuf};

use crate::notebook::fs_ops::is_scan_skip_dir;
use crate::notebook::state::hash64;

/// 单个文件的 hash 上限。超过就只记尺寸不记内容(见 [`OVERSIZE_PREFIX`])。
///
/// 放在附件上限(25MB)之上:正常附件必须能算出真 hash,否则每一张图都会退化成
/// 「在场但不同步」。再大的东西不是笔记数据,读进内存算 hash 的代价也不该由一次
/// 例行扫描承担。
pub(crate) const MAX_HASH_BYTES: u64 = 64 * 1024 * 1024;

/// 全库上限。与 `vault_walk` 同一量级,防止 home 目录被挂成 vault。
pub(crate) const MAX_FILES: usize = 20_000;
pub(crate) const MAX_DEPTH: usize = 12;

/// 超大文件的 hash 前缀。整串形如 `oversize:1234`(字节数)。
///
/// 关键不在「不传」,而在**它在场**。直接从扫描结果里省掉的话,diff 会看到「基线
/// 有、本地无」判成本地删除,把远端那份也删了 —— 用户只是放了个大文件进来,却丢了
/// 远端副本。所以它要出现在结果里,而 diff 对带这个前缀的条目不产生任何动作。
pub(crate) const OVERSIZE_PREFIX: &str = "oversize:";

/// 扫描出的一个文件。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSig {
    /// vault 根相对路径,`/` 分隔,无前导斜杠。
    ///
    /// 用 `/` 而不是平台分隔符:这个值要跟远端比,而远端(云盘 key、git 路径)一律
    /// 是 `/`。在 Windows 上存成 `\` 的后果是同一篇笔记在两个平台上算成两个路径,
    /// 于是每台机器都把对方的那份当新文件下载下来。
    pub path: String,
    pub mtime_ms: i64,
    pub size: u64,
    /// 内容 hash 的十进制串,或 `oversize:<字节数>`。
    ///
    /// 十进制串而非 u64:超过 JS 安全整数,序列化成数字会在前端被静默截断
    /// (与 `state::FileSig` 同一个理由)。
    pub hash: String,
}

impl FileSig {
    /// 这个条目是不是因为太大而没算内容 hash。
    pub fn is_oversize(&self) -> bool {
        self.hash.starts_with(OVERSIZE_PREFIX)
    }
}

/// 扫一遍 vault。结果按路径排序 —— 目录遍历顺序是文件系统给的,不排序会让同一个
/// vault 在两次扫描间给出不同顺序,而顺序会进到进度显示和测试断言里。
pub fn scan_vault(vault: &Path) -> Result<Vec<FileSig>, String> {
    if !vault.is_dir() {
        return Err(format!("{} is not a directory", vault.display()));
    }
    let mut out = Vec::new();
    let mut files = 0usize;
    walk(vault, vault, 0, &mut files, &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(vault: &Path, dir: &Path, depth: usize, files: &mut usize, out: &mut Vec<FileSig>) {
    if depth > MAX_DEPTH || *files >= MAX_FILES {
        return;
    }
    // 读不动某个子目录(权限)不该让整次扫描失败:其余文件仍然该被同步。
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *files >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // 不跟软链,与 `vault_walk` 一致:跟了会让同一份内容以两个相对路径出现,
        // 于是同一份内容被上传两次,而删掉本体后那个软链路径还留在远端。
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_scan_skip_dir(&name) {
                continue;
            }
            walk(vault, &path, depth + 1, files, out);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(sig) = signature_of(vault, &path) else {
            continue;
        };
        *files += 1;
        out.push(sig);
    }
}

/// 算一个文件的签名。读不动就返回 `None`(静默跳过,同 `vault_walk` 的取舍)。
fn signature_of(vault: &Path, path: &Path) -> Option<FileSig> {
    let rel = relative_path(vault, path)?;
    let meta = std::fs::metadata(path).ok()?;
    let size = meta.len();
    let mtime_ms = mtime_ms_of(&meta);
    let hash = if size > MAX_HASH_BYTES {
        format!("{OVERSIZE_PREFIX}{size}")
    } else {
        let bytes = std::fs::read(path).ok()?;
        hash64(&bytes).to_string()
    };
    Some(FileSig {
        path: rel,
        mtime_ms,
        size,
        hash,
    })
}

/// vault 根相对路径,统一用 `/`。
///
/// 不接受非 `Normal` 的路径组件:`..` 出现在这里意味着遍历跑到了 vault 外面,
/// 那种条目宁可丢掉也不能带进同步 —— 它会被当成远端的一个相对 key 用。
fn relative_path(vault: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(vault).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            _ => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn mtime_ms_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 把扫描结果做成 `路径 → 签名` 的表。
pub fn by_path(sigs: Vec<FileSig>) -> std::collections::BTreeMap<String, FileSig> {
    sigs.into_iter()
        .map(|sig| (sig.path.clone(), sig))
        .collect()
}

/// 单个文件的当前签名,给「破坏性动作前就地复验」用。
///
/// 文件已经不在了返回 `Ok(None)` —— 那本身就是一种「和快照对不上」,调用方要区分
/// 「没变」「变了」「没了」三种,不能把后两种混成一个错误。
pub fn signature_at(vault: &Path, rel_path: &str) -> Result<Option<FileSig>, String> {
    let path = resolve_rel(vault, rel_path)?;
    if !path.is_file() {
        return Ok(None);
    }
    Ok(signature_of(vault, &path))
}

/// 这条相对路径在同步范围之外吗?
///
/// 本地扫描靠 [`is_scan_skip_dir`] 在**走目录**时跳过;远端只有一串路径字符串,没有目录
/// 可跳。两侧必须是同一条口径,否则会出现下面这个:
///
/// ```text
/// 本地扫描  跳过 .notebook/  →  快照里没有它 → 没有基线
/// 远端列表  照收 .notebook/  →  diff 看到「远端有、本地没有、无基线」→ 判成新文件
/// 执行      下载 .notebook/sync.db  →  覆盖正在用的同步库
/// ```
///
/// 触发它**不需要恶意远端**:用户拿网盘客户端把整个 vault 传上去一次就够了,而那是很自然
/// 的操作。同一条路还能覆盖回收站清单(丢掉软删的退路)和 `.git/`(如果这个 vault 同时开
/// 着 git 同步,等于用云端的旧副本砸掉本地仓库)。
///
/// 所以范围判定收在这一个函数里,远端列举、下载写入两处都问它。
pub fn is_out_of_scope(rel_path: &str) -> bool {
    // 不额外判空段:`is_scan_skip_dir("")` 恒为 false(`SKIP_DIRS` 里没有空串),加一句
    // `!part.is_empty()` 改不了任何结果,只会让读者以为空段在这里有特殊含义。空段本身由
    // `resolve_rel` 拒掉。
    rel_path.split('/').any(is_scan_skip_dir)
}

/// 相对路径拼回绝对路径,并确认它没跑出 vault。
///
/// 这个 rel_path 的来源包括远端列表 —— 也就是**不可信输入**。远端给一个
/// `../../.ssh/authorized_keys` 就能让下载写到 vault 外面去,所以逐组件校验,
/// 只放 `Normal`。
pub fn resolve_rel(vault: &Path, rel_path: &str) -> Result<PathBuf, String> {
    if rel_path.is_empty() {
        return Err("Empty relative path".to_string());
    }
    let mut out = vault.to_path_buf();
    for part in rel_path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("Unsafe relative path: {rel_path}"));
        }
        // Windows 上 `\` 也是分隔符:只按 `/` 切的话 `a\..\b` 会被当成一个文件名
        // 放过去,而 std 在 Windows 上解析它时又会变成上跳一级。
        if part.contains('\\') {
            return Err(format!("Unsafe relative path: {rel_path}"));
        }
        out.push(part);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-sync-scan-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp vault");
        dir
    }

    fn write(vault: &Path, rel: &str, body: &[u8]) {
        let path = vault.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }

    #[test]
    fn scanning_collects_notes_and_attachments_alike() {
        // 附件是同步的一等公民。只收 .md 的话表现是「笔记过去了,图片全裂」。
        let vault = temp_vault();
        write(&vault, "一.md", b"# note");
        write(&vault, "attachments/img.png", &[0x89, 0x50, 0x4e, 0x47]);
        write(&vault, "sub/二.markdown", b"body");
        let sigs = scan_vault(&vault).expect("scan");
        let paths: Vec<&str> = sigs.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["attachments/img.png", "sub/二.markdown", "一.md"]
        );
    }

    #[test]
    fn binary_and_non_utf8_files_are_not_skipped() {
        // `vault_walk` 用 read_to_string,非 UTF-8 直接跳过。同步这条路必须按字节
        // 读:跳过意味着那个文件永远不同步,而且会被判成本地删除去删远端。
        let vault = temp_vault();
        write(&vault, "raw.bin", &[0xff, 0xfe, 0x00, 0x01]);
        let sigs = scan_vault(&vault).expect("scan");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].size, 4);
        assert!(!sigs[0].is_oversize());
        assert_eq!(sigs[0].hash, hash64(&[0xff, 0xfe, 0x00, 0x01]).to_string());
    }

    #[test]
    fn the_private_dir_is_not_synced() {
        // `.notebook/` 里是历史快照、回收站、索引库 —— 派生数据。同步过去的后果是
        // 两台机器的索引库互相覆盖,而那个库还带着 -wal。
        let vault = temp_vault();
        write(&vault, ".notebook/index.db", b"sqlite");
        write(&vault, ".notebook/history/一.md", b"old");
        write(&vault, "一.md", b"new");
        let sigs = scan_vault(&vault).expect("scan");
        let paths: Vec<&str> = sigs.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["一.md"]);
    }

    #[test]
    fn build_output_dirs_are_skipped() {
        let vault = temp_vault();
        write(&vault, "node_modules/pkg/index.js", b"x");
        write(&vault, ".git/HEAD", b"ref");
        write(&vault, "keep.md", b"y");
        let sigs = scan_vault(&vault).expect("scan");
        let paths: Vec<&str> = sigs.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["keep.md"]);
    }

    #[test]
    fn relative_paths_use_forward_slashes() {
        // 这个值要跟远端 key 比。存成平台分隔符的话,同一篇笔记在 Windows 与 macOS
        // 上算成两个路径,两台机器会各自把对方那份当新文件下载。
        let vault = temp_vault();
        write(&vault, "a/b/c.md", b"deep");
        let sigs = scan_vault(&vault).expect("scan");
        assert_eq!(sigs[0].path, "a/b/c.md");
        assert!(!sigs[0].path.contains('\\'));
    }

    #[test]
    fn the_hash_agrees_with_the_rest_of_the_notebook() {
        // 同步、RAG 索引、保存冲突检测三处必须对同一份内容给出同一个值。这里盯的
        // 就是「同步没有偷偷换一套 hash」。
        let vault = temp_vault();
        let body = b"# hello\n\xe4\xbd\xa0\xe5\xa5\xbd\n";
        write(&vault, "n.md", body);
        let sigs = scan_vault(&vault).expect("scan");
        assert_eq!(sigs[0].hash, hash64(body).to_string());
    }

    #[test]
    fn an_oversize_file_is_present_but_unhashed() {
        // 在场是关键:省掉它的话 diff 判成本地删除,会去删远端那份。
        let vault = temp_vault();
        let path = vault.join("big.bin");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_HASH_BYTES + 1).expect("set_len");
        drop(file);
        let sigs = scan_vault(&vault).expect("scan");
        assert_eq!(sigs.len(), 1, "超大文件必须出现在结果里");
        assert!(sigs[0].is_oversize());
        assert_eq!(
            sigs[0].hash,
            format!("{OVERSIZE_PREFIX}{}", MAX_HASH_BYTES + 1)
        );
        assert_eq!(sigs[0].size, MAX_HASH_BYTES + 1);
    }

    #[test]
    fn a_file_exactly_at_the_limit_is_still_hashed() {
        // 边界:`>` 而不是 `>=`。反过来的话正好卡在上限的文件会莫名变成 oversize。
        let vault = temp_vault();
        let path = vault.join("edge.bin");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_HASH_BYTES).expect("set_len");
        drop(file);
        let sigs = scan_vault(&vault).expect("scan");
        assert!(!sigs[0].is_oversize(), "正好等于上限的应该照常算 hash");
    }

    #[test]
    fn symlinks_are_not_followed() {
        // 跟了的话同一份内容以两个 relPath 出现:上传两次,而删掉本体后那个软链
        // 路径还赖在远端。
        let vault = temp_vault();
        write(&vault, "real.md", b"body");
        #[cfg(unix)]
        std::os::unix::fs::symlink(vault.join("real.md"), vault.join("link.md")).expect("symlink");
        #[cfg(not(unix))]
        return;
        let sigs = scan_vault(&vault).expect("scan");
        let paths: Vec<&str> = sigs.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["real.md"]);
    }

    #[test]
    fn scanning_a_missing_vault_is_an_error_not_an_empty_result() {
        // 空结果会被 diff 读成「本地全删了」,于是把远端整个清空。这个区别必须是
        // 错误而不是 Ok(vec![])。
        let vault = temp_vault().join("nope");
        assert!(scan_vault(&vault).is_err());
    }

    #[test]
    fn a_remote_supplied_path_cannot_escape_the_vault() {
        // rel_path 的来源包括远端列表 —— 不可信。放过 `..` 就能让下载写到 vault
        // 外面去。
        let vault = temp_vault();
        assert!(resolve_rel(&vault, "../evil").is_err());
        assert!(resolve_rel(&vault, "a/../../evil").is_err());
        assert!(resolve_rel(&vault, "./a").is_err());
        assert!(resolve_rel(&vault, "").is_err());
        assert!(resolve_rel(&vault, "a//b").is_err());
        // Windows 上 `\` 也是分隔符:只按 `/` 切会把这一段当成单个文件名放过去,
        // 而 std 在那边解析时又会真的上跳一级。
        assert!(resolve_rel(&vault, "a\\..\\evil").is_err());
        assert_eq!(
            resolve_rel(&vault, "a/b.md").expect("ok"),
            vault.join("a").join("b.md")
        );
    }

    #[test]
    fn re_signing_one_file_distinguishes_unchanged_changed_and_gone() {
        // 破坏性动作前的复验要能区分这三种。混成一个错误的话,「文件没了」会被当成
        // 「读失败」重试到超时。
        let vault = temp_vault();
        write(&vault, "n.md", b"one");
        let before = signature_at(&vault, "n.md").expect("ok").expect("present");
        assert_eq!(before.hash, hash64(b"one").to_string());

        write(&vault, "n.md", b"two");
        let after = signature_at(&vault, "n.md").expect("ok").expect("present");
        assert_ne!(after.hash, before.hash);

        std::fs::remove_file(vault.join("n.md")).expect("rm");
        assert!(signature_at(&vault, "n.md").expect("ok").is_none());
    }

    #[test]
    fn by_path_keys_on_the_relative_path() {
        let vault = temp_vault();
        write(&vault, "a.md", b"a");
        write(&vault, "b/c.md", b"c");
        let table = by_path(scan_vault(&vault).expect("scan"));
        assert_eq!(table.len(), 2);
        assert_eq!(table["b/c.md"].hash, hash64(b"c").to_string());
    }

    #[test]
    fn out_of_scope_matches_what_the_walk_skips() {
        // 这条谓词存在的意义就是「和 scan_vault 走目录时的口径一致」,所以拿真扫描当基准:
        // 凡是扫描收进来的,它必须说在范围内;凡是扫描跳掉的,它必须说在范围外。
        let vault = temp_vault();
        write(&vault, "a.md", b"a");
        write(&vault, "sub/b.md", b"b");
        write(&vault, "attachments/c.png", b"c");
        write(&vault, ".notebook/sync.db", b"db");
        write(&vault, ".git/HEAD", b"ref");
        write(&vault, "node_modules/pkg/i.js", b"x");
        write(&vault, "sub/.git/config", b"cfg");

        let scanned = by_path(scan_vault(&vault).expect("scan"));
        for rel in scanned.keys() {
            assert!(!is_out_of_scope(rel), "扫描收了 {rel},谓词却说范围外");
        }
        for rel in [
            ".notebook/sync.db",
            ".git/HEAD",
            "node_modules/pkg/i.js",
            "sub/.git/config",
        ] {
            assert!(!scanned.contains_key(rel), "前提错了:扫描不该收 {rel}");
            assert!(is_out_of_scope(rel), "扫描跳了 {rel},谓词却说范围内");
        }
        // 附件必须在范围内 —— 它只在**笔记树**里被跳掉,同步一定要带上。
        assert!(scanned.contains_key("attachments/c.png"));
        assert!(!is_out_of_scope("attachments/c.png"));
    }

    #[test]
    fn out_of_scope_is_not_a_prefix_test() {
        // 名字相似的目录是用户的正当数据。按字符串前缀判会把它们一起吞掉。
        assert!(!is_out_of_scope(".notebook-backup/a.md"));
        assert!(!is_out_of_scope("gitnotes/a.md"));
        assert!(!is_out_of_scope("my.git.notes/a.md"));
        assert!(!is_out_of_scope("a.md"));
    }
}
