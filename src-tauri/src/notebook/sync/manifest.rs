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
//! 内容身份 ← manifest。没记到的条目退化成「不知道 hash」,那时候现算(下载后 hash),
//!            而不是当成「不存在」。
//! ```
//!
//! 于是 manifest 丢失的最坏后果是**下一轮多花几次下载重新算 hash**,不是删数据。
//! 这也让「每轮结束写一次」成为可以接受的策略 —— 逐文件写的话,两万个文件要把整份
//! 清单来回写两万遍。
//!
//! ## 条目为什么带 size
//!
//! `size` 是 manifest 的**校验字段**,不是元数据。远端文件被别的工具改过(直接在网盘
//! 网页版编辑、另一个客户端覆盖),manifest 里的 hash 就成了陈旧的谎。比 size 能抓住
//! 绝大多数这种情况,代价是零 —— `read_dir` 本来就带 size。对不上就当没记过,现算。
//!
//! 注意 `mtime` **不做**校验字段:那是挂钟,而且网盘的 mtime 语义各家不同(有的记
//! 上传时间,有的透传原始时间)。用它会把「时钟不可信」这个已经关掉的缺口从后门放回来。

// 这个模块还没有非测试调用方 —— 云盘同步的命令层未落地。下面这行说的是「还没有人
// 调」,不是「没测试覆盖」:每个导出项都被单测走过。命令层接上之后删掉它。
#![allow(dead_code)]

use std::collections::BTreeMap;

use super::diff::RemoteEntry;

/// manifest 在远端根下的相对位置。
///
/// 放在一个点开头的目录里,和本地 `.notebook/` 对称。列目录时要**跳过整个目录** ——
/// 不然它自己会被当成一篇笔记同步下来,而它每轮都在变,于是每轮都有一个假冲突。
pub const MANIFEST_DIR: &str = ".notebook-sync";
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
    /// 校验字段:和 `read_dir` 报的 size 对不上就说明这条已经过期。
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

    /// 取一条,并用 `read_dir` 报的实际 size 校验。
    ///
    /// 返回 `None` 有两种含义,调用方处理方式相同(现算 hash):没记过,或者记的已经过期。
    pub fn verified(&self, path: &str, actual_size: u64) -> Option<&ManifestEntry> {
        self.entries
            .get(path)
            .filter(|entry| entry.size == actual_size)
    }

    pub fn put(&mut self, path: &str, entry: ManifestEntry) {
        self.entries.insert(path.to_string(), entry);
    }

    pub fn remove(&mut self, path: &str) {
        self.entries.remove(path);
    }

    /// 组装成 diff 要的一条。
    pub fn to_remote_entry(path: &str, entry: &ManifestEntry) -> RemoteEntry {
        RemoteEntry {
            path: path.to_string(),
            hash: entry.hash.clone(),
            device: entry.device.clone(),
            seq: entry.seq,
        }
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
    fn a_size_mismatch_invalidates_the_recorded_hash() {
        // 远端被别的工具改过(网页版编辑、另一个客户端覆盖),manifest 里的 hash 就是
        // 陈旧的谎。比 size 零成本地抓住绝大多数这种情况。
        let mut m = Manifest::default();
        m.put("a.md", entry("stale", 10));
        assert!(m.verified("a.md", 10).is_some());
        assert!(
            m.verified("a.md", 11).is_none(),
            "size 对不上就不能拿这个 hash 当真"
        );
    }

    #[test]
    fn an_unrecorded_path_is_not_verified() {
        let m = Manifest::default();
        assert!(m.verified("nope.md", 0).is_none());
    }

    #[test]
    fn a_zero_byte_file_can_still_be_verified() {
        // size 0 不能被当成「没读到」的哨兵 —— 空笔记是合法的。
        let mut m = Manifest::default();
        m.put("empty.md", entry("h-empty", 0));
        assert!(m.verified("empty.md", 0).is_some());
    }

    #[test]
    fn removing_an_entry_makes_it_unverifiable() {
        let mut m = Manifest::default();
        m.put("a.md", entry("h", 1));
        m.remove("a.md");
        assert!(m.verified("a.md", 1).is_none());
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

    #[test]
    fn to_remote_entry_carries_the_logical_stamp() {
        let e = entry("h", 4);
        let got = Manifest::to_remote_entry("a.md", &e);
        assert_eq!(got.path, "a.md");
        assert_eq!(got.hash, "h");
        assert_eq!(got.device, "dev-a");
        assert_eq!(got.seq, 3);
    }
}
