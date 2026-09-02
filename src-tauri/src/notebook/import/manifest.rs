//! 增量清单:记住「这一条已经导过了」。
//!
//! 没有它,第二次导入同一个 Notion 导出会把每篇笔记再落一遍(靠 `unique_path` 变成
//! `Note-2.md`、`Note-3.md`)—— 而这些副本是真正的笔记,会进索引、进搜索、被 wikilink
//! 指到,清理起来只能手工。
//!
//! 落在 `<vault>/.notebook/imports.json`。放私有目录是**刻意的**:它不是用户的笔记,
//! 不该出现在树里、搜索里,也不该被 P8 的同步当成内容传走。
//!
//! 指纹的语义由各 provider 自己定(zip 内路径 / 文件相对路径 / 标题+正文哈希),要求
//! 只有一条:**同一份源端内容下次还能算出同一个值**。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::super::fs_ops::{atomic_write, private_dir};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    providers: BTreeMap<String, ProviderKeys>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProviderKeys {
    #[serde(default)]
    keys: BTreeSet<String>,
}

fn manifest_path(vault: &Path) -> PathBuf {
    private_dir(vault).join("imports.json")
}

/// 指纹。sha256 截前 8 字节(16 个十六进制字符)。
///
/// 用 sha256 而不是 vault 里通用的 `state::hash64`(FNV-1a):导入的源端是**外部文件**,
/// 可能是网上下载的归档。FNV 是非加密 hash,构造碰撞很容易,而一次碰撞的后果是那条
/// 笔记被判成「已导入过」跳掉。虽然跳过会记进报告(不是完全静默),但让外部输入能决定
/// 哪些笔记进不来仍然不对。sha2 本来就是依赖,这里不多花任何东西。
pub fn fingerprint(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    let mut out = String::with_capacity(16);
    for byte in &digest[..8] {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 一次导入会话对清单的读写。开头 [`open`](Session::open),结束
/// [`save`](Session::save)。
pub struct Session {
    vault: PathBuf,
    provider: String,
    known: BTreeSet<String>,
    added: BTreeSet<String>,
}

impl Session {
    pub fn open(vault: &Path, provider: &str) -> Self {
        let manifest = load(vault);
        let known = manifest
            .providers
            .get(provider)
            .map(|entry| entry.keys.clone())
            .unwrap_or_default();
        Self {
            vault: vault.to_path_buf(),
            provider: provider.to_string(),
            known,
            added: BTreeSet::new(),
        }
    }

    /// 这一条以前导过吗。
    ///
    /// **本次会话内新记的也算。** 同一轮里源端出现两条同指纹的条目(zip 里同一个文件
    /// 被打包两次),第二条应该跳过而不是再落一份。
    pub fn is_known(&self, key: &str) -> bool {
        self.known.contains(key) || self.added.contains(key)
    }

    /// 记一条。只在**确实写进 vault 之后**调 —— 写失败还记的话,重试那一轮会把它
    /// 当成「导过了」跳掉,于是那篇笔记永远进不来。
    pub fn record(&mut self, key: String) {
        self.added.insert(key);
    }

    /// 落盘。
    ///
    /// 保存前**重读一次**主清单再合并:别的 provider 可能在这期间写过(两个导入
    /// 同时跑),直接拿会话开始时那份写回去会把它们的记录抹掉。
    pub fn save(self) -> Result<(), String> {
        let mut manifest = load(&self.vault);
        let entry = manifest.providers.entry(self.provider).or_default();
        entry.keys.extend(self.known);
        entry.keys.extend(self.added);
        let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        let path = manifest_path(&self.vault);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 {} 失败:{e}", parent.display()))?;
        }
        // 原子写:清单被写坏(断电、进程被杀)的后果是整份 JSON 解析失败,
        // 于是 `load` 落回默认值 —— 全部记录丢掉,下次导入把一切重导一遍。
        atomic_write(&path, &json)
    }
}

/// 读清单。解析失败落回空 —— 一份坏掉的清单让导入整体失败是不必要的,代价只是
/// 重导一次(重导会撞 `unique_path`,不会覆盖用户内容)。
fn load(vault: &Path) -> Manifest {
    std::fs::read_to_string(manifest_path(vault))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "aeroric-import-mf-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("建临时 vault");
        dir
    }

    #[test]
    fn fingerprint_is_stable_and_distinguishing() {
        assert_eq!(fingerprint("notion::a.md"), fingerprint("notion::a.md"));
        assert_ne!(fingerprint("notion::a.md"), fingerprint("notion::b.md"));
        assert_eq!(fingerprint("x").len(), 16);
    }

    #[test]
    fn provider_prefix_keeps_namespaces_apart() {
        // 同名文件从两个 provider 导入是两件事。
        assert_ne!(fingerprint("notion::a.md"), fingerprint("bear::a.md"));
    }

    #[test]
    fn recorded_keys_survive_a_round_trip() {
        let vault = temp_vault("roundtrip");
        let mut session = Session::open(&vault, "notion");
        assert!(!session.is_known("k1"));
        session.record("k1".to_string());
        session.save().expect("落盘");

        let next = Session::open(&vault, "notion");
        assert!(next.is_known("k1"));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn keys_recorded_this_session_are_already_known() {
        // 同一轮里源端出现两条同指纹的条目,第二条该跳过而不是再落一份。
        let vault = temp_vault("same-session");
        let mut session = Session::open(&vault, "notion");
        session.record("dup".to_string());
        assert!(session.is_known("dup"));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn providers_do_not_clobber_each_other() {
        // 保存前重读主清单的理由:两个导入同时跑,后保存的那个不该抹掉前一个。
        let vault = temp_vault("two-providers");
        let mut a = Session::open(&vault, "notion");
        let mut b = Session::open(&vault, "bear");
        a.record("ka".to_string());
        b.record("kb".to_string());
        a.save().expect("A 落盘");
        b.save().expect("B 落盘");

        assert!(Session::open(&vault, "notion").is_known("ka"));
        assert!(Session::open(&vault, "bear").is_known("kb"));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn a_later_session_does_not_drop_earlier_keys() {
        let vault = temp_vault("accumulate");
        let mut first = Session::open(&vault, "notion");
        first.record("k1".to_string());
        first.save().expect("第一轮");

        let mut second = Session::open(&vault, "notion");
        second.record("k2".to_string());
        second.save().expect("第二轮");

        let third = Session::open(&vault, "notion");
        assert!(third.is_known("k1") && third.is_known("k2"));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn the_manifest_lives_in_the_private_dir() {
        // 落在私有目录才不会进树、进搜索、被同步当成内容传走。
        let vault = temp_vault("location");
        Session::open(&vault, "notion").save().expect("落盘");
        let path = manifest_path(&vault);
        assert!(path.is_file());
        assert!(path.starts_with(private_dir(&vault)));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn a_corrupt_manifest_falls_back_to_empty_instead_of_failing() {
        let vault = temp_vault("corrupt");
        let path = manifest_path(&vault);
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建私有目录");
        std::fs::write(&path, "{ not json").expect("写坏文件");

        let session = Session::open(&vault, "notion");
        assert!(!session.is_known("k1"));
        // 而且还能正常写回去 —— 坏掉的清单不该让后续导入永久失败。
        let mut session = session;
        session.record("k1".to_string());
        session.save().expect("覆盖坏文件");
        assert!(Session::open(&vault, "notion").is_known("k1"));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn an_unknown_provider_starts_empty() {
        let vault = temp_vault("unknown");
        let mut notion = Session::open(&vault, "notion");
        notion.record("k".to_string());
        notion.save().expect("落盘");
        assert!(!Session::open(&vault, "logseq").is_known("k"));
        let _ = std::fs::remove_dir_all(vault);
    }
}
