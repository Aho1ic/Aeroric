//! 把 `StorageBackend`(18 种协议)接成引擎要的 [`RemoteFs`]。
//!
//! ## 存在性来自 read_dir,身份来自 manifest
//!
//! 这是这一层最重要的一条,理由在 [`super::manifest`] 的模块文档里:manifest 若充当
//! 存在性依据,一次没写成功就会让 diff 判成远端删除,进而软删用户的笔记。所以:
//!
//! ```text
//! list() 的路径集合 ← 递归 read_dir。远端真有什么就是什么。
//! list() 的 hash    ← manifest 命中且 size 校验通过就用;否则下载现算。
//! ```
//!
//! manifest 丢了的代价是下一轮多几次下载,不是删数据。
//!
//! ## 为什么不用 `join_storage_path`
//!
//! `storage_backend::join_storage_path` 会对结果做 `normalize_storage_path`,而那个函数
//! **解析** `..`(`parts.pop()`)。解析就是接受:`join_storage_path("/root", "../evil")`
//! 得到 `/evil` —— 逃出了同步根。这一层自己拒绝 `..` 再纯拼接,安全性质留在本地、看得见。
//!
//! 远端路径同样要校验。名字是**服务器给的**,一个坏掉或恶意的后端可以在 `read_dir` 里
//! 回 `../../etc/passwd`,而我们会拿它去 `get`/`put`。本地侧 `scan::resolve_rel` 已经这么
//! 做了,远端侧不能少。

use std::collections::BTreeMap;

use super::diff::RemoteEntry;
use super::engine::RemoteFs;
use super::manifest::{
    self, Manifest, ManifestEntry, MANIFEST_DIR, MANIFEST_NAME, MANIFEST_TMP_NAME,
};
use super::scan::{self, MAX_DEPTH, MAX_FILES, MAX_HASH_BYTES, OVERSIZE_PREFIX};
use crate::notebook::state::hash64;
use crate::storage_backend::StorageBackend;

/// 一个 vault 作用域的远端。`root` 之外的东西碰不到。
pub struct StorageRemote<'a> {
    backend: &'a dyn StorageBackend,
    /// 已归一的远端根,形如 `/notes` 或 `/`。
    root: String,
    /// 本机 device_id 与当前逻辑序号,写进 manifest 条目用。
    device: String,
    seq: i64,
    manifest: Manifest,
    /// 有没有改动待写。没有就不写 —— 白写一遍会平白更新远端文件的 mtime。
    dirty: bool,
}

/// 归一远端根:去掉尾斜杠,保证前导斜杠。
///
/// 只做这两件事,**不解析 `..`** —— 根是用户在设置里填的,原样保留比悄悄改掉更好判断。
fn normalize_root(root: &str) -> String {
    let trimmed = root.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// 纯拼接。调用方保证 `rel_path` 已经过 [`check_rel`]。
fn join_under(root: &str, rel_path: &str) -> String {
    if root == "/" {
        format!("/{rel_path}")
    } else {
        format!("{root}/{rel_path}")
    }
}

/// 校验一个相对路径能不能安全地拼到根下面。
///
/// 和 `scan::resolve_rel` 同一套判据。`\` 也要拦:Windows 把它当分隔符,`a\..\evil`
/// 在这里看着是一个普通段名,到了那边就逃出去了。
fn check_rel(rel_path: &str) -> Result<(), String> {
    if rel_path.is_empty() {
        return Err("Empty remote path".to_string());
    }
    for segment in rel_path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return Err(format!("Unsafe remote path: {rel_path}"));
        }
    }
    Ok(())
}

/// 校验并拼出远端绝对路径。
fn resolve_remote(root: &str, rel_path: &str) -> Result<String, String> {
    check_rel(rel_path)?;
    Ok(join_under(root, rel_path))
}

/// 把远端绝对路径还原成相对根的路径。返回 `None` 表示不在根下面或不安全。
///
/// 只折叠重复斜杠,不做 `..` 解析 —— 解析会把 `<root>/a/../../etc` 变成看起来合法的
/// 东西。带 `..` 的一律丢掉。
fn relative_to(root: &str, path: &str) -> Option<String> {
    let collapsed: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let root_parts: Vec<&str> = root.split('/').filter(|s| !s.is_empty()).collect();
    if collapsed.len() <= root_parts.len() || collapsed[..root_parts.len()] != root_parts[..] {
        return None;
    }
    let rest = &collapsed[root_parts.len()..];
    if rest
        .iter()
        .any(|s| *s == "." || *s == ".." || s.contains('\\'))
    {
        return None;
    }
    Some(rest.join("/"))
}

/// 读远端清单。**读不到就当空的**,不报错。
///
/// 首次同步、清单被删、被别的设备写坏,都走这条路。报错会把用户锁在同步之外,而空清单
/// 只是让这一轮把该算的 hash 都算一遍。
fn load_manifest(backend: &dyn StorageBackend, root: &str) -> Manifest {
    let path = join_under(root, &format!("{MANIFEST_DIR}/{MANIFEST_NAME}"));
    match backend.read(&path) {
        Ok(bytes) => Manifest::parse(&bytes),
        Err(_) => Manifest::default(),
    }
}

impl<'a> StorageRemote<'a> {
    pub fn open(backend: &'a dyn StorageBackend, root: &str, device: &str, seq: i64) -> Self {
        let root = normalize_root(root);
        let manifest = load_manifest(backend, &root);
        Self {
            backend,
            root,
            device: device.to_string(),
            seq,
            manifest,
            dirty: false,
        }
    }

    /// 递归列出根下所有文件,返回 `相对路径 → size`。
    ///
    /// 显式栈而不是递归:远端目录深度是**服务器说了算**的,一个恶意或坏掉的后端能靠深
    /// 目录把栈打爆。上限沿用本地扫描那两个,两侧口径一致。
    ///
    /// size 保留 `Option`,**不 `unwrap_or(0)`**:「后端没报大小」和「这是个空文件」是
    /// 两件不同的事。压成 0 的话,前者会拿 0 去和清单里的真实 size 比,于是每条命中都
    /// 被判成过期,每轮把整个远端重新下载一遍。
    fn list_files(&self) -> Result<BTreeMap<String, Option<u64>>, String> {
        let mut out: BTreeMap<String, Option<u64>> = BTreeMap::new();
        let mut stack: Vec<(String, usize)> = vec![(self.root.clone(), 0)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > MAX_DEPTH || out.len() >= MAX_FILES {
                continue;
            }
            // 这里**不吞错**。列不动一个子目录就整轮失败 —— 静默跳过会让那些文件看起来
            // 「远端没有」,而那正好是 diff 判删除的条件,一次权限抖动就能删掉一批笔记。
            let entries = self.backend.read_dir(&dir)?;
            for entry in entries {
                let Some(rel) = relative_to(&self.root, &entry.path) else {
                    continue;
                };
                if manifest::is_manifest_path(&rel) {
                    continue;
                }
                // 本地扫描跳掉的目录,远端也不能收 —— 否则会被判成「本地没有的新文件」下载
                // 回来,而其中包括 `.notebook/sync.db` 这种正在用的库。见 `scan::is_out_of_scope`。
                // 放在 `is_dir` 分支之前:目录本身就不下潜,远端一个 `.git/` 可能是上万个
                // object,白跑一趟还会挤掉真笔记的 `MAX_FILES` 配额。
                if scan::is_out_of_scope(&rel) {
                    continue;
                }
                if entry.is_dir {
                    stack.push((entry.path.clone(), depth + 1));
                } else {
                    out.insert(rel, entry.size);
                }
            }
        }
        Ok(out)
    }
}

impl StorageRemote<'_> {
    /// 补一次 size。`read_dir` 没报大小时才走这里。
    ///
    /// 现存 18 个协议的 `capability_for` 都声明了 `rich_metadata`,所以这条路正常不会走
    /// 到。留着是因为「不会走到」和「走到了会错」是两回事:少了它,没报 size 的条目只能
    /// 拿一个校验不了的 hash 蒙,而蒙错的方向是**把远端的编辑当成没变**。
    fn resolve_size(&self, rel_path: &str, listed: Option<u64>) -> Option<u64> {
        if listed.is_some() {
            return listed;
        }
        if !self.backend.capability().stat {
            return None;
        }
        let abs = resolve_remote(&self.root, rel_path).ok()?;
        self.backend.stat(&abs).ok()?.size
    }
}

impl RemoteFs for StorageRemote<'_> {
    fn list(&self) -> Result<Vec<RemoteEntry>, String> {
        let files = self.list_files()?;
        let mut out = Vec::with_capacity(files.len());
        for (rel, listed_size) in files {
            let size = self.resolve_size(&rel, listed_size);
            // 命中清单且 size 对得上 —— 直接用记着的 hash,零下载。这是正常路径。
            //
            // size 未知时**不查清单**:没有校验字段的命中等于无条件相信一份可能陈旧的
            // hash,而 hash 陈旧的后果是远端的编辑被判成「没变」,然后被本地覆盖掉。
            if let Some(entry) = size.and_then(|s| self.manifest.verified(&rel, s)) {
                out.push(Manifest::to_remote_entry(&rel, entry));
                continue;
            }
            // 没命中(首次同步、被别的工具改过、上一轮 commit 丢了、size 未知)。
            if let Some(big) = size.filter(|s| *s > MAX_HASH_BYTES) {
                // 超大文件不下载来算 hash。标记成 oversize,和本地扫描同一个口径 ——
                // 本地也有同名文件时 diff 那边跳过它(两侧都算不出 hash),而它**在场**
                // 这件事让它不会被判成远端删除。
                out.push(RemoteEntry {
                    path: rel,
                    hash: format!("{OVERSIZE_PREFIX}{big}"),
                    device: String::new(),
                    seq: 0,
                });
                continue;
            }
            let abs = resolve_remote(&self.root, &rel)?;
            let bytes = self.backend.read(&abs)?;
            out.push(RemoteEntry {
                path: rel,
                hash: hash64(&bytes).to_string(),
                // 现算出来的 hash 没有对应的逻辑戳:我们不知道是谁写的。空值比编一个
                // 好 —— 这两个字段只用于显示,不参与任何判定。
                device: String::new(),
                seq: 0,
            });
        }
        Ok(out)
    }

    fn get(&self, path: &str) -> Result<Vec<u8>, String> {
        let abs = resolve_remote(&self.root, path)?;
        self.backend.read(&abs)
    }

    fn put(&mut self, path: &str, bytes: &[u8], hash: &str) -> Result<(), String> {
        let abs = resolve_remote(&self.root, path)?;
        // 先把父目录建出来。对象存储的「目录」是前缀模拟,建了也不花什么;挂载类后端
        // 缺了父目录会直接写失败。
        if let Some((parent, _)) = abs.rsplit_once('/') {
            if !parent.is_empty() && parent != self.root {
                let _ = self.backend.create_dir(parent);
            }
        }
        self.backend.write(&abs, bytes)?;
        self.manifest.put(
            path,
            ManifestEntry {
                hash: hash.to_string(),
                device: self.device.clone(),
                seq: self.seq,
                size: bytes.len() as u64,
            },
        );
        self.dirty = true;
        Ok(())
    }

    fn delete(&mut self, path: &str) -> Result<(), String> {
        let abs = resolve_remote(&self.root, path)?;
        self.backend.delete(&abs)?;
        self.manifest.remove(path);
        self.dirty = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let bytes = self.manifest.to_bytes()?;
        let dir = join_under(&self.root, MANIFEST_DIR);
        let _ = self.backend.create_dir(&dir);
        let final_path = join_under(&self.root, &format!("{MANIFEST_DIR}/{MANIFEST_NAME}"));

        // 先写临时名再 rename:别的设备可能正在读。读到写了一半的 json 不会造成数据
        // 损失(解析失败退化成空清单),但会让它把整个远端重新 hash 一遍,白花很多下载。
        if self.backend.capability().rename {
            let tmp = join_under(&self.root, &format!("{MANIFEST_DIR}/{MANIFEST_TMP_NAME}"));
            self.backend.write(&tmp, &bytes)?;
            self.backend.rename(&tmp, &final_path)?;
        } else {
            self.backend.write(&final_path, &bytes)?;
        }
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    use super::*;
    use crate::storage_backend::{Capability, StorageEntry, StorageStat};

    /// 内存后端。`StorageBackend` 的方法都取 `&self`,所以状态放 `Mutex` 里。
    #[derive(Default)]
    struct FakeBackend {
        inner: Mutex<FakeState>,
    }

    #[derive(Default)]
    struct FakeState {
        /// 绝对路径 → 内容。
        files: BTreeMap<String, Vec<u8>>,
        dirs: BTreeSet<String>,
        /// 谎报的 size:用来在不真造 64MB 的前提下走 oversize 分支。
        size_override: BTreeMap<String, u64>,
        /// 这些路径的 `read_dir` 会失败。
        fail_read_dir: BTreeSet<String>,
        /// 报不出 size(模拟 rich_metadata 不成立的后端)。
        hide_sizes: bool,
        can_rename: bool,
        can_stat: bool,
        /// 按顺序记下每一次写/改/删,用来断言 tmp-then-rename 的次序。
        calls: Vec<String>,
    }

    impl FakeBackend {
        fn new() -> Self {
            let backend = Self::default();
            {
                let mut state = backend.inner.lock().expect("lock");
                state.can_rename = true;
                state.can_stat = true;
                state.dirs.insert("/notes".to_string());
            }
            backend
        }

        fn put_file(&self, path: &str, bytes: &[u8]) {
            let mut state = self.inner.lock().expect("lock");
            state.files.insert(path.to_string(), bytes.to_vec());
            let mut cursor = path;
            while let Some((parent, _)) = cursor.rsplit_once('/') {
                if parent.is_empty() {
                    break;
                }
                state.dirs.insert(parent.to_string());
                cursor = parent;
            }
        }

        fn read_file(&self, path: &str) -> Option<Vec<u8>> {
            self.inner.lock().expect("lock").files.get(path).cloned()
        }

        fn calls(&self) -> Vec<String> {
            self.inner.lock().expect("lock").calls.clone()
        }

        fn paths(&self) -> Vec<String> {
            self.inner
                .lock()
                .expect("lock")
                .files
                .keys()
                .cloned()
                .collect()
        }
    }

    impl StorageBackend for FakeBackend {
        fn capability(&self) -> Capability {
            let state = self.inner.lock().expect("lock");
            Capability {
                rename: state.can_rename,
                stat: state.can_stat,
                rich_metadata: !state.hide_sizes,
                ..Capability::FULL
            }
        }

        fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String> {
            let state = self.inner.lock().expect("lock");
            if state.fail_read_dir.contains(path) {
                return Err(format!("read_dir refused: {path}"));
            }
            let prefix = if path == "/" {
                "/".to_string()
            } else {
                format!("{path}/")
            };
            let mut out: Vec<StorageEntry> = Vec::new();
            let mut seen_dirs: BTreeSet<String> = BTreeSet::new();
            for key in state.files.keys().chain(state.dirs.iter()) {
                let Some(rest) = key.strip_prefix(&prefix) else {
                    continue;
                };
                if rest.is_empty() {
                    continue;
                }
                match rest.split_once('/') {
                    Some((head, _)) => {
                        let dir_path = format!("{prefix}{head}");
                        if seen_dirs.insert(dir_path.clone()) {
                            out.push(StorageEntry {
                                name: head.to_string(),
                                path: dir_path,
                                is_dir: true,
                                size: None,
                                modified_at_ms: None,
                            });
                        }
                    }
                    None => {
                        let is_dir = !state.files.contains_key(key);
                        if is_dir && !seen_dirs.insert(key.clone()) {
                            continue;
                        }
                        let size = if is_dir || state.hide_sizes {
                            None
                        } else {
                            Some(
                                state
                                    .size_override
                                    .get(key)
                                    .copied()
                                    .unwrap_or_else(|| state.files[key].len() as u64),
                            )
                        };
                        out.push(StorageEntry {
                            name: rest.to_string(),
                            path: key.clone(),
                            is_dir,
                            size,
                            modified_at_ms: None,
                        });
                    }
                }
            }
            Ok(out)
        }

        fn read(&self, path: &str) -> Result<Vec<u8>, String> {
            let state = self.inner.lock().expect("lock");
            state
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("not found: {path}"))
        }

        fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
            let mut state = self.inner.lock().expect("lock");
            state.calls.push(format!("write {path}"));
            state.files.insert(path.to_string(), bytes.to_vec());
            Ok(())
        }

        fn create_dir(&self, path: &str) -> Result<(), String> {
            let mut state = self.inner.lock().expect("lock");
            state.calls.push(format!("create_dir {path}"));
            state.dirs.insert(path.to_string());
            Ok(())
        }

        fn delete(&self, path: &str) -> Result<(), String> {
            let mut state = self.inner.lock().expect("lock");
            state.calls.push(format!("delete {path}"));
            state.files.remove(path);
            state.dirs.remove(path);
            Ok(())
        }

        fn rename(&self, from: &str, to: &str) -> Result<(), String> {
            let mut state = self.inner.lock().expect("lock");
            state.calls.push(format!("rename {from} -> {to}"));
            let bytes = state
                .files
                .remove(from)
                .ok_or_else(|| format!("not found: {from}"))?;
            state.files.insert(to.to_string(), bytes);
            Ok(())
        }

        fn copy(&self, from: &str, to: &str) -> Result<(), String> {
            let bytes = self.read(from)?;
            self.write(to, &bytes)
        }

        fn stat(&self, path: &str) -> Result<StorageStat, String> {
            let state = self.inner.lock().expect("lock");
            if let Some(bytes) = state.files.get(path) {
                let size = state
                    .size_override
                    .get(path)
                    .copied()
                    .unwrap_or(bytes.len() as u64);
                return Ok(StorageStat {
                    is_dir: false,
                    size: Some(size),
                    modified_at_ms: None,
                });
            }
            if state.dirs.contains(path) {
                return Ok(StorageStat {
                    is_dir: true,
                    size: None,
                    modified_at_ms: None,
                });
            }
            Err(format!("not found: {path}"))
        }
    }

    fn hash_of(bytes: &[u8]) -> String {
        hash64(bytes).to_string()
    }

    // ---- 路径守卫 ----

    #[test]
    fn a_parent_traversal_is_refused_rather_than_resolved() {
        // 这是不用 `join_storage_path` 的全部理由:它会 normalize,也就是把 `..` 解析掉,
        // 于是 `/notes` + `../evil` 变成 `/evil` —— 一个合法路径,同步根之外。
        assert!(resolve_remote("/notes", "../evil").is_err());
        assert!(resolve_remote("/notes", "a/../../evil").is_err());
        assert!(resolve_remote("/notes", "..").is_err());
    }

    #[test]
    fn a_backslash_segment_is_refused() {
        // Windows 把 `\` 当分隔符。`a\..\evil` 在这里看着是一个普通段名。
        assert!(resolve_remote("/notes", "a\\..\\evil").is_err());
    }

    #[test]
    fn an_empty_or_dot_segment_is_refused() {
        assert!(resolve_remote("/notes", "").is_err());
        assert!(resolve_remote("/notes", "a//b").is_err());
        assert!(resolve_remote("/notes", "./a").is_err());
    }

    #[test]
    fn an_ordinary_relative_path_joins_under_the_root() {
        assert_eq!(
            resolve_remote("/notes", "a/b.md").expect("ok"),
            "/notes/a/b.md"
        );
        assert_eq!(resolve_remote("/", "a.md").expect("ok"), "/a.md");
    }

    #[test]
    fn the_root_is_normalized_without_resolving_dotdot() {
        assert_eq!(normalize_root("notes/"), "/notes");
        assert_eq!(normalize_root("/notes///"), "/notes");
        assert_eq!(normalize_root("  "), "/");
        assert_eq!(normalize_root("/"), "/");
        // 根是用户填的,原样留着比悄悄改掉好判断 —— 解析成 `/` 会让「同步整个网盘」
        // 这种最危险的配置从一个笔误里冒出来。
        assert_eq!(normalize_root("/notes/.."), "/notes/..");
    }

    #[test]
    fn a_remote_path_outside_the_root_is_dropped_not_clamped() {
        // 名字是服务器给的。一个坏掉的后端在 read_dir 里回根外的路径,要丢掉,
        // 不能截断成根内的某个东西。
        assert_eq!(
            relative_to("/notes", "/notes/a.md"),
            Some("a.md".to_string())
        );
        assert_eq!(
            relative_to("/notes", "/notes//a//b.md"),
            Some("a/b.md".to_string())
        );
        assert_eq!(relative_to("/notes", "/other/a.md"), None);
        assert_eq!(relative_to("/notes", "/notes"), None);
        assert_eq!(relative_to("/notes", "/notes/../evil"), None);
        assert_eq!(relative_to("/notes", "/notesX/a.md"), None);
        assert_eq!(relative_to("/", "/a/b.md"), Some("a/b.md".to_string()));
    }

    // ---- 存在性与身份的分工 ----

    #[test]
    fn a_first_sync_with_no_manifest_hashes_by_downloading() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        backend.put_file("/notes/sub/b.md", b"beta");

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        assert_eq!(
            listed.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["a.md", "sub/b.md"]
        );
        assert_eq!(listed[0].hash, hash_of(b"alpha"));
        assert_eq!(listed[1].hash, hash_of(b"beta"));
        // 现算出来的 hash 没有对应的逻辑戳。编一个会让 diff 的显示层撒谎。
        assert_eq!(listed[0].device, "");
        assert_eq!(listed[0].seq, 0);
    }

    #[test]
    fn a_manifest_hit_avoids_the_download() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        let mut manifest = Manifest::default();
        manifest.put(
            "a.md",
            ManifestEntry {
                hash: "recorded".to_string(),
                device: "dev-9".to_string(),
                seq: 42,
                size: 5,
            },
        );
        backend.put_file(
            "/notes/.notebook-sync/manifest.json",
            &manifest.to_bytes().expect("bytes"),
        );

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        assert_eq!(listed.len(), 1);
        // 用记着的值,没去读内容 —— 否则这里会是 alpha 的真 hash。
        assert_eq!(listed[0].hash, "recorded");
        assert_eq!(listed[0].device, "dev-9");
        assert_eq!(listed[0].seq, 42);
    }

    #[test]
    fn a_stale_manifest_entry_is_caught_by_the_size_check() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"edited elsewhere");
        let mut manifest = Manifest::default();
        manifest.put(
            "a.md",
            ManifestEntry {
                hash: "recorded".to_string(),
                device: "dev-9".to_string(),
                seq: 42,
                // 别的工具改过之后长度变了。
                size: 5,
            },
        );
        backend.put_file(
            "/notes/.notebook-sync/manifest.json",
            &manifest.to_bytes().expect("bytes"),
        );

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        assert_eq!(listed[0].hash, hash_of(b"edited elsewhere"));
    }

    #[test]
    fn the_manifest_directory_never_shows_up_as_a_note() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        backend.put_file("/notes/.notebook-sync/manifest.json", b"{}");
        backend.put_file("/notes/.notebook-sync/manifest.json.tmp", b"{}");

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        // 清单每轮都在变。跟着同步的话每轮都有一个假冲突。
        assert_eq!(
            listed.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["a.md"]
        );
    }

    #[test]
    fn the_vault_private_directory_is_never_offered_for_download() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        // 用户拿网盘客户端把整个 vault 传上去一次,远端就会有这些。本地扫描永远看不见它们
        // (`is_scan_skip_dir` 跳掉了),于是它们没有基线 —— 而「远端有 + 本地没有 + 无基线」
        // 正好是 diff 判「新文件,下载」的条件。收下它们等于用云端的旧副本覆盖正在用的同步
        // 库和回收站清单。
        backend.put_file("/notes/.notebook/sync.db", b"stale db");
        backend.put_file("/notes/.notebook/trash/entries.json", b"[]");
        backend.put_file("/notes/.git/HEAD", b"ref: refs/heads/main");
        backend.put_file("/notes/node_modules/pkg/index.js", b"x");
        backend.put_file("/notes/sub/.git/config", b"[core]");

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        assert_eq!(
            listed.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["a.md"]
        );
    }

    #[test]
    fn a_skipped_remote_directory_is_not_descended_into() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        backend.put_file("/notes/.git/objects/ab/cdef", b"blob");
        // 远端的 `.git/` 可能是上万个 object。列它一遍是白付的网络往返,而且会挤掉真笔记的
        // MAX_FILES 配额 —— 所以过滤要在下潜之前。
        backend
            .inner
            .lock()
            .expect("lock")
            .fail_read_dir
            .insert("/notes/.git".to_string());

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        // 那个目录若被下潜就会撞上 fail_read_dir 而整轮失败(`list_files` 刻意不吞错),
        // 所以这里的 `expect` 本身就是「没有下潜」的断言。
        let listed = remote.list().expect("must not descend");
        assert_eq!(
            listed.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["a.md"]
        );
    }

    #[test]
    fn a_similarly_named_directory_is_still_synced() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/.notebook-sync-backup/a.md", b"alpha");

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        // 裸 starts_with 会把它一起吞掉。
        assert_eq!(listed.len(), 1, "只有 .notebook-sync 本身该被跳过");
    }

    #[test]
    fn a_broken_manifest_degrades_to_rehashing_not_to_an_error() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        backend.put_file("/notes/.notebook-sync/manifest.json", b"} not json {");

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("坏清单不该让整轮同步停住");

        assert_eq!(listed[0].hash, hash_of(b"alpha"));
    }

    #[test]
    fn a_failed_subdirectory_listing_fails_the_round() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        backend.put_file("/notes/sub/b.md", b"beta");
        backend
            .inner
            .lock()
            .expect("lock")
            .fail_read_dir
            .insert("/notes/sub".to_string());

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let got = remote.list();

        // 吞掉这个错会让 sub/ 下的文件看起来「远端没有」,而那正好是 diff 判远端删除的
        // 条件 —— 一次权限抖动就能把一批笔记软删掉。
        assert!(
            got.is_err(),
            "列不动子目录必须整轮失败,不能当成那些文件不存在"
        );
    }

    #[test]
    fn an_oversize_remote_file_is_present_but_unhashed() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/big.bin", b"stub");
        backend
            .inner
            .lock()
            .expect("lock")
            .size_override
            .insert("/notes/big.bin".to_string(), MAX_HASH_BYTES + 1);

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].hash,
            format!("{OVERSIZE_PREFIX}{}", MAX_HASH_BYTES + 1),
            "超过上限就只记尺寸,不下载来算 hash"
        );
    }

    #[test]
    fn a_file_exactly_at_the_limit_is_still_hashed() {
        // 边界是 `>` 而不是 `>=`,和本地扫描一致。反过来的话正好卡在上限的文件在两侧
        // 会拿到不同口径的 hash,于是永远对不上、永远重传。
        let backend = FakeBackend::new();
        backend.put_file("/notes/edge.bin", b"stub");
        backend
            .inner
            .lock()
            .expect("lock")
            .size_override
            .insert("/notes/edge.bin".to_string(), MAX_HASH_BYTES);

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        assert_eq!(listed[0].hash, hash_of(b"stub"));
    }

    #[test]
    fn a_missing_size_falls_back_to_stat_before_giving_up() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        let mut manifest = Manifest::default();
        manifest.put(
            "a.md",
            ManifestEntry {
                hash: "recorded".to_string(),
                device: "dev-9".to_string(),
                seq: 42,
                size: 5,
            },
        );
        backend.put_file(
            "/notes/.notebook-sync/manifest.json",
            &manifest.to_bytes().expect("bytes"),
        );
        backend.inner.lock().expect("lock").hide_sizes = true;

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        // read_dir 没报 size,但 stat 能补上,于是清单命中照样成立。
        assert_eq!(listed[0].hash, "recorded");
    }

    #[test]
    fn an_unverifiable_manifest_entry_is_not_trusted() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"edited elsewhere");
        let mut manifest = Manifest::default();
        manifest.put(
            "a.md",
            ManifestEntry {
                hash: "recorded".to_string(),
                device: "dev-9".to_string(),
                seq: 42,
                size: 16,
            },
        );
        backend.put_file(
            "/notes/.notebook-sync/manifest.json",
            &manifest.to_bytes().expect("bytes"),
        );
        {
            let mut state = backend.inner.lock().expect("lock");
            state.hide_sizes = true;
            state.can_stat = false;
        }

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        // size 校验不了(read_dir 不报、stat 也没有)→ 现算,不能相信一个可能陈旧的
        // hash。相信的后果是远端的编辑被判成「没变」,然后被本地那份覆盖掉。
        assert_eq!(
            listed[0].hash,
            hash_of(b"edited elsewhere"),
            "无法校验的清单条目必须现算,不能直接用"
        );
    }

    #[test]
    fn an_unknown_size_is_not_silently_read_as_zero() {
        // 「后端没报大小」不能塌成 `Some(0)`。塌了的话,一条记着 size 0 的旧条目
        // (那篇笔记当时是空的)会在这里假命中,于是别的设备后来写进去的内容被判成
        // 「远端没变」,下一轮被本地那份空文件覆盖掉。
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"written by another device");
        let mut manifest = Manifest::default();
        manifest.put(
            "a.md",
            ManifestEntry {
                hash: "hash-of-the-empty-version".to_string(),
                device: "dev-9".to_string(),
                seq: 42,
                size: 0,
            },
        );
        backend.put_file(
            "/notes/.notebook-sync/manifest.json",
            &manifest.to_bytes().expect("bytes"),
        );
        {
            let mut state = backend.inner.lock().expect("lock");
            state.hide_sizes = true;
            state.can_stat = false;
        }

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        let listed = remote.list().expect("list");

        assert_eq!(
            listed[0].hash,
            hash_of(b"written by another device"),
            "size 未知时必须现算,不能拿 0 去和清单里的 size 比"
        );
    }

    // ---- 写入侧 ----

    #[test]
    fn get_reads_under_the_root_and_refuses_to_escape() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        backend.put_file("/secret.md", b"secret");

        let remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);

        assert_eq!(remote.get("a.md").expect("get"), b"alpha".to_vec());
        assert!(remote.get("../secret.md").is_err());
    }

    #[test]
    fn put_writes_the_bytes_and_records_the_stamp() {
        let backend = FakeBackend::new();
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);

        remote
            .put("sub/a.md", b"alpha", &hash_of(b"alpha"))
            .expect("put");
        remote.commit().expect("commit");

        assert_eq!(
            backend.read_file("/notes/sub/a.md"),
            Some(b"alpha".to_vec())
        );
        let written = Manifest::parse(
            &backend
                .read_file("/notes/.notebook-sync/manifest.json")
                .expect("manifest written"),
        );
        let entry = written.entries.get("sub/a.md").expect("entry");
        assert_eq!(entry.hash, hash_of(b"alpha"));
        assert_eq!(entry.device, "dev-1");
        assert_eq!(entry.seq, 7);
        // size 是校验字段,必须是实际写下去的长度,不能是别的什么。
        assert_eq!(entry.size, 5);
    }

    #[test]
    fn put_refuses_to_escape_the_root() {
        let backend = FakeBackend::new();
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);

        assert!(remote.put("../evil.md", b"x", "h").is_err());
        assert_eq!(backend.read_file("/evil.md"), None);
    }

    #[test]
    fn delete_removes_both_the_file_and_its_manifest_entry() {
        let backend = FakeBackend::new();
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        remote
            .put("a.md", b"alpha", &hash_of(b"alpha"))
            .expect("put");
        remote.commit().expect("commit");

        remote.delete("a.md").expect("delete");
        remote.commit().expect("commit");

        assert_eq!(backend.read_file("/notes/a.md"), None);
        let written = Manifest::parse(
            &backend
                .read_file("/notes/.notebook-sync/manifest.json")
                .expect("manifest"),
        );
        // 留着条目会让下一轮的 size 校验拿一个不存在的文件去比,虽然不致命,但清单会
        // 无限膨胀 —— 删了的路径永远不会再被清掉。
        assert!(written.entries.is_empty(), "删除要同时清掉清单条目");
    }

    #[test]
    fn delete_refuses_to_escape_the_root() {
        let backend = FakeBackend::new();
        backend.put_file("/secret.md", b"secret");
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);

        assert!(remote.delete("../secret.md").is_err());
        assert_eq!(backend.read_file("/secret.md"), Some(b"secret".to_vec()));
    }

    #[test]
    fn commit_writes_a_temp_file_first_then_renames() {
        let backend = FakeBackend::new();
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        remote
            .put("a.md", b"alpha", &hash_of(b"alpha"))
            .expect("put");

        remote.commit().expect("commit");

        let calls = backend.calls();
        let tmp_write = calls
            .iter()
            .position(|c| c == "write /notes/.notebook-sync/manifest.json.tmp")
            .expect("先写临时名");
        let rename = calls
            .iter()
            .position(|c| {
                c == "rename /notes/.notebook-sync/manifest.json.tmp -> /notes/.notebook-sync/manifest.json"
            })
            .expect("再 rename 到正式名");
        assert!(tmp_write < rename);
        // 别的设备可能正在读。读到半份 json 不丢数据(解析失败退化成空清单),但会让它
        // 把整个远端重新 hash 一遍。
        assert!(
            !calls.contains(&"write /notes/.notebook-sync/manifest.json".to_string()),
            "有 rename 能力时不该直接覆盖正式名"
        );
        assert_eq!(
            backend.read_file("/notes/.notebook-sync/manifest.json.tmp"),
            None
        );
    }

    #[test]
    fn commit_writes_directly_when_rename_is_unavailable() {
        let backend = FakeBackend::new();
        backend.inner.lock().expect("lock").can_rename = false;
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        remote
            .put("a.md", b"alpha", &hash_of(b"alpha"))
            .expect("put");

        remote.commit().expect("commit");

        assert!(backend
            .calls()
            .contains(&"write /notes/.notebook-sync/manifest.json".to_string()));
        let written = Manifest::parse(
            &backend
                .read_file("/notes/.notebook-sync/manifest.json")
                .expect("manifest"),
        );
        assert!(written.entries.contains_key("a.md"));
    }

    #[test]
    fn commit_without_changes_touches_nothing() {
        let backend = FakeBackend::new();
        backend.put_file("/notes/a.md", b"alpha");
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);

        remote.list().expect("list");
        remote.commit().expect("commit");

        // 只读一轮不该写清单:白写一遍会平白更新远端 mtime,而用户是拿 mtime 判断
        // 「这个文件夹最近有没有动」的。
        assert!(backend.calls().is_empty(), "没有改动就不该有任何写操作");
        assert_eq!(backend.paths(), vec!["/notes/a.md".to_string()]);
    }

    #[test]
    fn a_second_commit_after_more_writes_keeps_the_earlier_entries() {
        let backend = FakeBackend::new();
        let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
        remote
            .put("a.md", b"alpha", &hash_of(b"alpha"))
            .expect("put");
        remote.commit().expect("commit");

        remote.put("b.md", b"beta", &hash_of(b"beta")).expect("put");
        remote.commit().expect("commit");

        let written = Manifest::parse(
            &backend
                .read_file("/notes/.notebook-sync/manifest.json")
                .expect("manifest"),
        );
        assert_eq!(
            written.entries.len(),
            2,
            "第二次 commit 不能把第一次的记账冲掉"
        );
    }

    #[test]
    fn a_round_trip_through_put_and_list_needs_no_download() {
        let backend = FakeBackend::new();
        {
            let mut remote = StorageRemote::open(&backend, "/notes", "dev-1", 7);
            remote
                .put("a.md", b"alpha", &hash_of(b"alpha"))
                .expect("put");
            remote.commit().expect("commit");
        }

        // 换一个「设备」重新打开:它读到的 hash 应该来自清单,连内容都不用碰。
        let remote = StorageRemote::open(&backend, "/notes", "dev-2", 1);
        let listed = remote.list().expect("list");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].hash, hash_of(b"alpha"));
        assert_eq!(
            listed[0].device, "dev-1",
            "记账里的设备是写下这份内容的那台"
        );
        assert_eq!(listed[0].seq, 7);
    }
}
