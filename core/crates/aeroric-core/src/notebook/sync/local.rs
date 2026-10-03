//! 生产版 [`LocalFs`]:同步引擎对本地 vault 的三个动作(软删 / 写 / 读)。
//!
//! 引擎里那个假实现只记内存,这一层真动磁盘,所以三件事在这里落地:
//!
//! 1. **软删走回收站**,不是 `unlink`。远端的一次误删(别的设备出 bug、用户在网盘网页版
//!    删错)不该让本地内容无法找回。
//! 2. **写入前拦住范围外的路径**。理由见 [`scan::is_out_of_scope`]。
//! 3. **写入前拦住 symlink 逃逸**。`scan::resolve_rel` 逐组件校验只能保证「没有 `..`、
//!    没有绝对路径」,拦不住 `notes/link` 是一条指向 vault 外的软链 —— 那时候
//!    `notes/link/x.md` 会写到 vault 外面去。
//!
//! ## 为什么不复用 `state::resolve_within`
//!
//! 它做的是同一件事,但 `allow_missing` 那条路只 canonicalize **父目录**,要求父目录已经
//! 存在。而同步下载经常要一次建好几级新目录(远端新建了 `年报/2026/一季度/x.md`),父目录
//! 那时还不存在,它会直接报错。所以这里换成「最深已存在的祖先」:存在的那一段整条解析掉,
//! 不存在的尾巴由 `resolve_rel` 保证只可能是普通名字。

use std::path::{Path, PathBuf};

use super::engine::LocalFs;
use super::scan;
use crate::notebook::{fs_ops, trash};

/// 真动磁盘的 [`LocalFs`]。没有状态 —— vault 每次由调用方传进来。
pub struct VaultLocalFs;

/// 沿着 `path` 往上找第一个存在的祖先。
///
/// 用 `symlink_metadata` 判存在:断掉的软链对它是「存在」,而 `exists()` 会说不存在。差别
/// 要紧 —— 把断链当不存在的话,它会被当成「尾巴的一部分」跳过,而它恰好是逃逸的入口。
fn deepest_existing(path: &Path) -> Option<&Path> {
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        if std::fs::symlink_metadata(current).is_ok() {
            return Some(current);
        }
        cursor = current.parent();
    }
    None
}

/// 相对路径 → 校验过的绝对路径。范围、穿越、symlink 逃逸三道一起过。
fn resolve_checked(vault: &Path, rel_path: &str) -> Result<PathBuf, String> {
    if scan::is_out_of_scope(rel_path) {
        return Err(format!(
            "Path is outside the notebook sync scope: {rel_path}"
        ));
    }
    let path = scan::resolve_rel(vault, rel_path)?;

    let vault_canon = vault
        .canonicalize()
        .map_err(|e| format!("Cannot resolve vault {}: {e}", vault.display()))?;
    let anchor =
        deepest_existing(&path).ok_or_else(|| format!("Cannot resolve {}", path.display()))?;
    let anchor_canon = anchor
        .canonicalize()
        .map_err(|e| format!("Cannot resolve {}: {e}", anchor.display()))?;
    if !anchor_canon.starts_with(&vault_canon) {
        return Err(format!("Path escapes the vault via a symlink: {rel_path}"));
    }
    Ok(path)
}

impl LocalFs for VaultLocalFs {
    fn soft_delete(&mut self, vault: &Path, rel_path: &str) -> Result<(), String> {
        let path = resolve_checked(vault, rel_path)?;
        trash::trash(vault, &path).map(|_| ())
    }

    fn write(&mut self, vault: &Path, rel_path: &str, bytes: &[u8]) -> Result<(), String> {
        let path = resolve_checked(vault, rel_path)?;
        fs_ops::atomic_write_bytes(&path, bytes)
    }

    fn read(&self, vault: &Path, rel_path: &str) -> Result<Vec<u8>, String> {
        let path = resolve_checked(vault, rel_path)?;
        std::fs::read(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Vault(PathBuf);

    impl Vault {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "aeroric-sync-local-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create vault");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Vault {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_download_creates_the_missing_directories() {
        let vault = Vault::new("mkdir");
        let mut fs = VaultLocalFs;
        fs.write(vault.path(), "年报/2026/一季度/x.md", b"hi")
            .expect("write");
        assert_eq!(
            std::fs::read(vault.path().join("年报/2026/一季度/x.md")).expect("read"),
            b"hi"
        );
    }

    #[test]
    fn bytes_survive_a_round_trip_without_being_read_as_utf8() {
        let vault = Vault::new("bytes");
        let mut fs = VaultLocalFs;
        // 一段不是合法 UTF-8 的字节。附件是二进制,这一层不该把它解成文本。
        let raw = [0xffu8, 0xfe, 0x00, 0x42];
        fs.write(vault.path(), "attachments/a.png", &raw)
            .expect("write");
        assert_eq!(
            fs.read(vault.path(), "attachments/a.png").expect("read"),
            raw
        );
    }

    #[test]
    fn a_remote_entry_cannot_overwrite_the_sync_database() {
        let vault = Vault::new("scope");
        let mut fs = VaultLocalFs;
        let error = fs
            .write(vault.path(), ".notebook/sync.db", b"wiped")
            .expect_err("must refuse");
        assert!(error.contains("outside the notebook sync scope"), "{error}");
        assert!(!vault.path().join(".notebook/sync.db").exists());
    }

    #[test]
    fn a_remote_entry_cannot_overwrite_the_git_directory() {
        let vault = Vault::new("scope-git");
        let mut fs = VaultLocalFs;
        assert!(fs.write(vault.path(), ".git/HEAD", b"ref: x").is_err());
        // 中间层也算范围外,不只是第一段。
        assert!(fs.write(vault.path(), "notes/.git/config", b"x").is_err());
    }

    #[test]
    fn a_remote_entry_cannot_soft_delete_inside_the_private_directory() {
        let vault = Vault::new("scope-del");
        let target = vault.path().join(".notebook/trash/entries.json");
        std::fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        std::fs::write(&target, b"manifest").expect("seed");

        let mut fs = VaultLocalFs;
        assert!(fs
            .soft_delete(vault.path(), ".notebook/trash/entries.json")
            .is_err());
        assert!(target.exists(), "回收站清单必须还在");
    }

    #[test]
    fn traversal_is_refused() {
        let vault = Vault::new("traverse");
        let mut fs = VaultLocalFs;
        assert!(fs.write(vault.path(), "../evil.md", b"x").is_err());
        assert!(fs.write(vault.path(), "a/../../evil.md", b"x").is_err());
        assert!(fs.read(vault.path(), "../evil.md").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_pointing_out_of_the_vault_cannot_be_written_through() {
        let vault = Vault::new("symlink");
        let outside = Vault::new("symlink-outside");
        std::os::unix::fs::symlink(outside.path(), vault.path().join("link")).expect("symlink");

        let mut fs = VaultLocalFs;
        let error = fs
            .write(vault.path(), "link/escaped.md", b"leaked")
            .expect_err("must refuse");
        assert!(error.contains("escapes the vault"), "{error}");
        assert!(!outside.path().join("escaped.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_broken_symlink_is_rejected_by_the_guard_not_by_luck() {
        let vault = Vault::new("broken-link");
        // 指向不存在的位置。`exists()` 说 false,`symlink_metadata` 说 true —— 这一句就是
        // `deepest_existing` 为什么必须用后者。
        std::os::unix::fs::symlink("/tmp/aeroric-does-not-exist", vault.path().join("gone"))
            .expect("symlink");

        let mut fs = VaultLocalFs;
        let error = fs
            .write(vault.path(), "gone/x.md", b"x")
            .expect_err("must refuse");

        // 断言**是谁拒的**,不能只断言 `is_err()`。按 `exists()` 判的话这条链会被当成
        // 「还不存在的尾巴」跳过校验,守卫放行,然后靠 `create_dir_all` 撞上 EEXIST 偶然
        // 失败 —— 两个版本都报错,只看 is_err() 分不出来。
        //
        // 而这个区别有后果:守卫放行之后、真正写入之前,如果那条断链的目标被建出来
        // (另一个进程、或同一轮同步里更早的一个文件),`create_dir_all` 就会成功,临时
        // 文件和 rename 都落到 vault 外面去。守卫必须靠自己拒掉,不能指望下游的巧合。
        assert!(
            error.contains("Cannot resolve") || error.contains("escapes the vault"),
            "要由路径守卫拒掉,而不是由写入那一步偶然失败: {error}"
        );
        assert!(
            !error.contains("Cannot create"),
            "这是写入层的错,说明守卫放行了: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_the_vault_still_works() {
        let vault = Vault::new("symlink-inner");
        std::fs::create_dir_all(vault.path().join("real")).expect("mkdir");
        std::os::unix::fs::symlink(vault.path().join("real"), vault.path().join("alias"))
            .expect("symlink");

        let mut fs = VaultLocalFs;
        // 拦的是逃逸,不是 symlink 本身。库内的软链是用户的正当用法。
        fs.write(vault.path(), "alias/x.md", b"ok").expect("write");
        assert_eq!(
            std::fs::read(vault.path().join("real/x.md")).expect("read"),
            b"ok"
        );
    }

    #[test]
    fn a_soft_delete_goes_to_the_trash_rather_than_unlink() {
        let vault = Vault::new("trash");
        std::fs::write(vault.path().join("note.md"), b"precious").expect("seed");

        let mut fs = VaultLocalFs;
        fs.soft_delete(vault.path(), "note.md")
            .expect("soft delete");

        assert!(!vault.path().join("note.md").exists());
        let items = trash::list(vault.path()).expect("list trash");
        assert_eq!(items.len(), 1, "远端的一次误删必须能找回");
        assert_eq!(items[0].name, "note.md");
    }

    #[test]
    fn deleting_something_already_gone_is_an_error_not_a_silent_success() {
        let vault = Vault::new("gone");
        let mut fs = VaultLocalFs;
        // 报错让这一轮不推进 seq、下一轮重来。当成成功会让引擎写下「已同步」的基线,
        // 而那是一句谎 —— 它会传给别的设备。
        assert!(fs.soft_delete(vault.path(), "never.md").is_err());
    }
}
