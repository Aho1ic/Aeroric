//! 远端 sidecar manifest:记每个远端文件的内容 hash 与逻辑戳。
//!
//! ## 为什么需要它
//!
//! `StorageEntry`(`storage_backend/mod.rs`)只有 `name/path/is_dir/size/modified_at_ms`,
//! **没有 etag**。而三方 diff 判「远端变了没」必须比内容,不能比挂钟(见 `diff` 的模块
//! 文档)。所以在远端根下自己放一份清单,把 hash 补上。
//!
//! ## 它**不是**存在性的依据 —— 这一条是数据安全的关键
//!
//! diff 看到「远端没有这个路径 + 有基线」会判成远端删除,进而软删本地那份
//! (`diff.rs` 的 `remote_deleted_local_unchanged`)。如果 manifest 充当存在性依据,那么
//! 一次没写成功的 manifest(崩在中间、网络断、别的设备并发覆盖)就会让**用户的笔记
//! 被删进回收站**。
//!
//! 所以分工是死的:
//!
//! ```text
//! 存在性  ← read_dir 递归列目录。远端真有什么就是什么,列不出来的东西不存在。
//! 内容身份 ← 当前远端内容。可 hash 文件每轮读取后计算,不能由清单代替。
//! 逻辑戳   ← manifest。只有当前内容 hash 与清单一致时,才复用 device / seq。
//! ```
//!
//! 于是 manifest 丢失的最坏后果是逻辑戳暂时未知,不是删数据。
//! 这也让「每轮结束写一次」成为可以接受的策略 —— 逐文件写的话,两万个文件要把整份
//! 清单来回写两万遍。
//!
//! ## 为什么不能用 size 验证 hash
//!
//! 旧格式保留 `size` 以维持兼容并辅助诊断,但它不是内容身份。远端文件可能被外部工具
//! 从 `alpha` 等长改成 `bravo`;size 仍为 5,旧 hash 却已经失效。`mtime` 同样不能充当
//! 校验字段:它是挂钟,而且不同 provider 的语义不同。没有可信 etag/version 时,只有
//! 重新计算当前内容 hash 才能验证一条 manifest 记录。

use std::collections::BTreeMap;

/// manifest 在远端根下的相对位置。
///
/// 放在一个点开头的目录里,和本地 `.notebook/` 对称。列目录时要**跳过整个目录** ——
/// 不然它自己会被当成一篇笔记同步下来,而它每轮都在变,于是每轮都有一个假冲突。
pub const MANIFEST_DIR: &str = crate::notebook::fs_ops::SYNC_PRIVATE_DIR;
pub const MANIFEST_NAME: &str = "manifest.json";

/// 写入时的临时名。先写它再 rename,避免读到写了一半的清单。
pub const MANIFEST_TMP_NAME: &str = "manifest.json.tmp";

/// 当前格式版本。读到更高的版本就整份当空 —— 猜一份不认识的清单不如重算。
pub const FORMAT_VERSION: u32 = 1;

/// manifest 里的一条。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// 内容 hash,口径与 `scan::FileSig::hash` / `state::hash64` 一致。
    pub hash: String,
    /// 写下这个版本的设备与它当时的逻辑序号。只用于显示与排障,**不参与顺序判定**。
    pub device: String,
    pub seq: i64,
    /// 写入时的内容大小。保留用于格式兼容与排障,不能代替内容 hash 验证。
    pub size: u64,
}

/// 整份清单。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    /// 路径 → 条目。路径是相对远端根的、`/` 分隔的相对路径,和本地 `FileSig::path` 同口径。
    pub entries: BTreeMap<String, ManifestEntry>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

impl Manifest {
    /// 解析。**任何解析失败都退化成空清单,不报错。**
    ///
    /// 清单是可重建的缓存:坏了就重算 hash,代价是几次下载。而报错会让整轮同步停住,
    /// 那才是真的坏 —— 一份被别的设备写坏的 json 不该把用户锁在同步之外。
    pub fn parse(bytes: &[u8]) -> Self {
        let parsed: Option<Manifest> = serde_json::from_slice(bytes).ok();
        match parsed {
            // 更高的版本不猜。字段语义可能变了,照旧解释比重算更危险。
            Some(m) if m.version <= FORMAT_VERSION => m,
            _ => Manifest::default(),
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(self)
            .map_err(|e| format!("Cannot serialize notebook sync manifest: {e}"))
    }

    /// 当前内容 hash 与记录一致时,返回它的逻辑戳来源。
    pub fn matching_hash(&self, path: &str, actual_hash: &str) -> Option<&ManifestEntry> {
        self.entries
            .get(path)
            .filter(|entry| entry.hash == actual_hash)
    }

    pub fn put(&mut self, path: &str, entry: ManifestEntry) {
        self.entries.insert(path.to_string(), entry);
    }

    pub fn remove(&mut self, path: &str) {
        self.entries.remove(path);
    }
}

/// 这个远端相对路径是不是 manifest 自己的地盘。
pub fn is_manifest_path(rel_path: &str) -> bool {
    rel_path == MANIFEST_DIR || rel_path.starts_with(&format!("{MANIFEST_DIR}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str, size: u64) -> ManifestEntry {
        ManifestEntry {
            hash: hash.to_string(),
            device: "dev-a".to_string(),
            seq: 3,
            size,
        }
    }

    #[test]
    fn a_round_trip_keeps_every_field() {
        let mut m = Manifest::default();
        m.put("a.md", entry("h1", 10));
        m.put("attachments/i.png", entry("h2", 2048));
        let bytes = m.to_bytes().expect("serialize");
        assert_eq!(Manifest::parse(&bytes), m);
    }

    #[test]
    fn garbage_parses_as_empty_instead_of_failing() {
        // 清单是可重建的缓存。报错会把用户锁在同步之外,而重算只是几次下载。
        for bad in [&b""[..], b"not json", b"{", b"[]", b"{\"version\":1}"] {
            let m = Manifest::parse(bad);
            assert!(m.entries.is_empty(), "{bad:?} 应该退化成空清单");
        }
    }

    #[test]
    fn a_newer_format_version_is_not_guessed_at() {
        // 字段语义可能变了。照旧解释比重算更危险。
        let bytes =
            br#"{"version":99,"entries":{"a.md":{"hash":"h","device":"d","seq":1,"size":5}}}"#;
        assert!(Manifest::parse(bytes).entries.is_empty());
    }

    #[test]
    fn the_current_version_is_accepted() {
        let bytes =
            br#"{"version":1,"entries":{"a.md":{"hash":"h","device":"d","seq":1,"size":5}}}"#;
        let m = Manifest::parse(bytes);
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.entries["a.md"].hash, "h");
    }

    #[test]
    fn only_a_matching_current_hash_reuses_the_recorded_stamp() {
        let mut m = Manifest::default();
        m.put("a.md", entry("current", 10));
        assert!(m.matching_hash("a.md", "current").is_some());
        assert!(
            m.matching_hash("a.md", "different").is_none(),
            "陈旧 hash 不能贡献旧版本的逻辑戳"
        );
    }

    #[test]
    fn an_unrecorded_path_has_no_matching_hash() {
        let m = Manifest::default();
        assert!(m.matching_hash("nope.md", "hash").is_none());
    }

    #[test]
    fn a_zero_byte_file_can_still_match_by_hash() {
        let mut m = Manifest::default();
        m.put("empty.md", entry("h-empty", 0));
        assert!(m.matching_hash("empty.md", "h-empty").is_some());
    }

    #[test]
    fn removing_an_entry_removes_its_logical_stamp() {
        let mut m = Manifest::default();
        m.put("a.md", entry("h", 1));
        m.remove("a.md");
        assert!(m.matching_hash("a.md", "h").is_none());
    }

    #[test]
    fn the_manifest_dir_is_recognised_so_it_is_never_synced_as_a_note() {
        // 漏掉这个判断的后果:清单自己被当成一篇笔记同步下来,而它每轮都在变,
        // 于是每轮都有一个假冲突。
        assert!(is_manifest_path(".notebook-sync"));
        assert!(is_manifest_path(".notebook-sync/manifest.json"));
        assert!(is_manifest_path(".notebook-sync/nested/thing"));
        assert!(!is_manifest_path("a.md"));
        assert!(!is_manifest_path("notebook-sync/a.md"));
        // 前缀相同但不是同一个目录 —— 不能用裸 starts_with。
        assert!(!is_manifest_path(".notebook-sync-backup/a.md"));
    }
}
