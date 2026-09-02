//! 用户自定义 CSS 主题:导入 / 列出 / 读取 / 删除。
//!
//! 主题文件放 `~/.aeroric/themes/<id>.css`,`id` 由源文件名 stem 经清洗得到。
//! 单文件上限 256 KB。
//!
//! 来源:移植自 Markio(`src-tauri/src/custom_themes.rs`),按 Aeroric 的约定改了三处:
//!
//! 1. **目录换成 `~/.aeroric/themes/`**,不按 OS 各走一套 config dir。Aeroric 的所有
//!    用户态数据都在 `~/.aeroric/` 下(`storage::aeroric_dir`),主题跟着走才能被同一套
//!    备份/迁移覆盖到。
//! 2. **导入按内容读+校验+写,不用 `fs::copy`。** Markio 先 `metadata().len()` 判上限
//!    再 copy —— 那中间有个 TOCTOU 窗口(判完之后源文件被换成大文件),而且 copy 会把源
//!    文件的权限位一起带过来。这里一次读进内存(读之前先按 metadata 挡掉明显超限的),
//!    校验完再 `atomic_write`,落地的是 Aeroric 自己的权限。
//! 3. **要求内容是 UTF-8。** 存进去之后前端要把它当字符串塞进 `<style>`,不是 UTF-8 的
//!    文件早失败比晚失败好。
//!
//! 安全边界:这里**不解析也不消毒 CSS**。自定义 CSS 的能力边界就是 CSS 本身 —— 它能改
//! 外观、能把元素藏起来,但不能执行脚本(`<style>` 里没有 JS 入口,`expression()` 是 IE
//! 时代的东西,现代 WebView 不认)。真正的风险是用户导入一份写坏的 CSS 把界面弄得没法
//! 操作,那是可用性问题,由前端的「一键停用」快捷键兜(见 `useCustomTheme.ts`),不是靠
//! 在这里猜哪些属性危险 —— 那种黑名单永远漏,而且会挡掉正常的主题。

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::storage::{aeroric_dir, atomic_write};

/// 单个主题文件的上限。与 Markio 一致。
pub const MAX_THEME_BYTES: u64 = 256 * 1024;

/// 清洗后的 id 长度上限。
const MAX_ID_LEN: usize = 64;

/// 唯一一道体积闸门。
///
/// 导入路径上要判两次(读之前按 metadata 挡掉几百 MB 的文件,读之后按真实字节数复判 ——
/// metadata 与 read 之间文件可能被换掉),`read` 回读时还要判一次。三处走同一个函数,
/// 这样它有一个能被直接断言的定义;三处各写一遍 `if x > MAX` 的话,中间那次就是一段
/// 正常路径上永远不触发、谁都观察不到的分支。
fn check_theme_size(bytes: u64) -> Result<(), String> {
    if bytes > MAX_THEME_BYTES {
        return Err(format!(
            "The theme file is too large ({bytes} bytes, limit {MAX_THEME_BYTES})"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTheme {
    /// 清洗后的标识,同时是磁盘文件名(不含扩展名)。
    pub id: String,
    /// 展示名。取源文件名 stem 的原文,可以是中文。
    pub name: String,
    pub path: String,
    pub size: u64,
}

fn themes_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("themes"))
}

/// 把源文件名 stem 清洗成一个只含 `[A-Za-z0-9_-]` 的 id。
///
/// 只保留 ASCII 字母数字与 `-` / `_`,空格与 `.` 折成 `-`,其余整个丢掉。这同时也是路径
/// 遍历的防线:`..`、`/`、`\`、`:` 全都活不下来,所以调用方拼 `dir.join(format!("{id}.css"))`
/// 是安全的 —— **不要**改成放宽字符集然后另外加 `..` 检查,那是两道容易走岔的闸门。
///
/// 全中文名会被清洗成空串而拿不到 id,这是刻意的:id 要出现在文件名里,而跨平台的文件名
/// 编码差异(NFC/NFD、Windows 的非 ASCII 行为)会让「同一个主题」在不同机器上对不上。
/// 展示名走 `name` 字段保留原文,用户看到的仍是自己起的名字。
fn sanitize_id(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c == ' ' || c == '.' {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() || trimmed.len() > MAX_ID_LEN {
        return None;
    }
    Some(trimmed)
}

pub fn ensure_dir() -> Result<PathBuf, String> {
    let dir = themes_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create the themes folder: {e}"))?;
    Ok(dir)
}

/// 列出已导入的主题。目录不存在或读不动都返回空列表 —— 「没有自定义主题」不是错误。
pub fn list() -> Result<Vec<CustomTheme>, String> {
    list_in(&ensure_dir()?)
}

/// 从任意路径导入一份 `.css`。同 id 覆盖(重新导入同一个文件就是更新)。
pub fn import(source_path: &Path) -> Result<CustomTheme, String> {
    import_into(&ensure_dir()?, source_path)
}

/// 读回一份主题的 CSS 文本。
pub fn read(id: &str) -> Result<String, String> {
    read_in(&ensure_dir()?, id)
}

/// 删除一份主题。已经不在了视为成功。
pub fn delete(id: &str) -> Result<(), String> {
    delete_in(&ensure_dir()?, id)
}

/// 主题目录路径,给「在文件管理器里打开」用。
pub fn dir_path() -> Result<String, String> {
    Ok(ensure_dir()?.to_string_lossy().to_string())
}

// 下面四个收 `dir` 参数的才是实现。拆出来是为了让测试能指向一个临时目录 ——
// 不然它们只能对着真实的 `~/.aeroric/themes/` 跑,要么污染用户数据,要么得改进程
// 全局的 `$HOME`(那在并行测试下是竞态)。

fn list_in(dir: &Path) -> Result<Vec<CustomTheme>, String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("css") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(id) = sanitize_id(stem) else {
            continue;
        };
        // 文件名 stem 与清洗结果不一致的条目跳过。落地时用的就是清洗后的 id,所以不一致
        // 意味着这个文件不是通过 `import` 进来的(用户手工丢进目录的),而 `read` / `delete`
        // 都按 id 拼路径 —— 列出来会给出一条点了就报「主题不存在」的条目。
        if id != stem {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if check_theme_size(size).is_err() {
            continue;
        }
        out.push(CustomTheme {
            id: id.clone(),
            name: stem.to_string(),
            path: path.to_string_lossy().to_string(),
            size,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn import_into(dir: &Path, source_path: &Path) -> Result<CustomTheme, String> {
    if source_path.extension().and_then(|s| s.to_str()) != Some("css") {
        return Err("Only .css files can be imported as a theme".to_string());
    }
    let meta = std::fs::metadata(source_path)
        .map_err(|e| format!("Failed to read the theme file: {e}"))?;
    if !meta.is_file() {
        return Err("The theme source is not a file".to_string());
    }
    // 先按 metadata 挡掉明显超限的,避免把一个几百 MB 的文件读进内存再拒绝。
    check_theme_size(meta.len())?;
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "The theme file name is not valid UTF-8".to_string())?;
    let id = sanitize_id(stem).ok_or_else(|| {
        "The theme file name has no usable ASCII letters, digits, '-' or '_'".to_string()
    })?;

    let bytes = std::fs::read(source_path).map_err(|e| format!("Failed to read the theme: {e}"))?;
    // metadata 之后文件可能被换掉,所以真正的上限判断落在读到的字节上。
    check_theme_size(bytes.len() as u64)?;
    let css = String::from_utf8(bytes)
        .map_err(|_| "The theme file is not valid UTF-8 text".to_string())?;

    let dest = dir.join(format!("{id}.css"));
    atomic_write(&dest, &css)?;
    Ok(CustomTheme {
        id,
        name: stem.to_string(),
        path: dest.to_string_lossy().to_string(),
        size: css.len() as u64,
    })
}

fn read_in(dir: &Path, id: &str) -> Result<String, String> {
    let clean = sanitize_id(id).ok_or_else(|| "The theme id is not valid".to_string())?;
    let path = dir.join(format!("{clean}.css"));
    let meta = std::fs::metadata(&path).map_err(|e| format!("The theme does not exist: {e}"))?;
    check_theme_size(meta.len())?;
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read the theme: {e}"))
}

fn delete_in(dir: &Path, id: &str) -> Result<(), String> {
    let clean = sanitize_id(id).ok_or_else(|| "The theme id is not valid".to_string())?;
    let path = dir.join(format!("{clean}.css"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to delete the theme: {e}")),
    }
}

// ── 命令 ─────────────────────────────────────────────────────────────────────
//
// 前缀是 `theme_custom_` 而不是 `notebook_`:自定义 CSS 注入的是整个应用的根节点,
// 不是随手记面板。它虽然由 P10 带进来,但归属上是应用级设置。
//
// 全部丢阻塞线程池 —— 这里每条都是同步文件 IO,在 async 命令里直接跑会占住 Tauri 的
// 运行时线程。

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| format!("theme task failed: {e}"))?
}

#[tauri::command]
pub async fn theme_custom_list() -> Result<Vec<CustomTheme>, String> {
    blocking(list).await
}

/// 导入一份 `.css`。`sourcePath` 由前端的文件对话框给出。
#[tauri::command]
pub async fn theme_custom_import(source_path: String) -> Result<CustomTheme, String> {
    blocking(move || import(Path::new(&source_path))).await
}

/// 读回 CSS 文本。前端拿到之后塞进 `<style>`。
#[tauri::command]
pub async fn theme_custom_read(id: String) -> Result<String, String> {
    blocking(move || read(&id)).await
}

#[tauri::command]
pub async fn theme_custom_delete(id: String) -> Result<(), String> {
    blocking(move || delete(&id)).await
}

/// 主题目录路径,给「在文件管理器里打开」用。
#[tauri::command]
pub async fn theme_custom_dir() -> Result<String, String> {
    blocking(dir_path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_themes_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(format!("aeroric-themes-{label}-{unique}"));
        std::fs::create_dir_all(&dir).expect("create temp themes dir");
        dir
    }

    fn write_source(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write source");
        path
    }

    #[test]
    fn check_theme_size_is_the_single_gate() {
        assert!(check_theme_size(0).is_ok());
        assert!(check_theme_size(MAX_THEME_BYTES).is_ok());
        assert!(check_theme_size(MAX_THEME_BYTES + 1).is_err());
    }

    #[test]
    fn sanitize_id_cannot_escape_the_themes_dir() {
        // 路径分隔符与 `:` 整个被丢掉,所以清洗结果里不可能留下能走出目录的片段。
        for raw in ["../../etc/passwd", "a/b\\c", "C:\\evil", "..", "./."] {
            match sanitize_id(raw) {
                None => {}
                Some(id) => {
                    assert!(!id.contains(['/', '\\', ':']), "{raw} → {id}");
                    assert_ne!(id, "..", "{raw} → {id}");
                }
            }
        }
        assert_eq!(sanitize_id("My Theme!").unwrap(), "My-Theme");
        assert_eq!(sanitize_id("ok-name_1").unwrap(), "ok-name_1");
        assert!(sanitize_id("").is_none());
        assert!(sanitize_id("中文").is_none());
        assert!(sanitize_id(&"a".repeat(MAX_ID_LEN + 1)).is_none());
        assert!(sanitize_id(&"a".repeat(MAX_ID_LEN)).is_some());
    }

    #[test]
    fn import_then_read_then_delete_round_trips() {
        let dir = temp_themes_dir("round-trip");
        let src = temp_themes_dir("round-trip-src");
        let source = write_source(&src, "solar.css", ":root { --accent: #ff8800; }");

        let theme = import_into(&dir, &source).expect("import");
        assert_eq!(theme.id, "solar");
        assert_eq!(theme.name, "solar");
        assert_eq!(theme.size, 28);

        let css = read_in(&dir, "solar").expect("read back");
        assert!(css.contains("--accent: #ff8800"));

        assert_eq!(list_in(&dir).expect("list").len(), 1);
        delete_in(&dir, "solar").expect("delete");
        assert!(list_in(&dir).expect("list after delete").is_empty());
        // 已经不在了仍然是成功 —— 前端重复点删除不该报错。
        delete_in(&dir, "solar").expect("delete twice");
    }

    #[test]
    fn import_keeps_the_original_name_but_sanitizes_the_id() {
        let dir = temp_themes_dir("name-vs-id");
        let src = temp_themes_dir("name-vs-id-src");
        let source = write_source(&src, "我的 Theme 2.css", "body { color: red; }");

        let theme = import_into(&dir, &source).expect("import");
        // 展示名保留原文(含中文与空格),id 只剩 ASCII。
        assert_eq!(theme.name, "我的 Theme 2");
        assert_eq!(theme.id, "Theme-2");
        assert!(theme.path.ends_with("Theme-2.css"));
        assert!(read_in(&dir, "Theme-2").is_ok());
    }

    #[test]
    fn import_rejects_non_css_oversize_and_non_utf8() {
        let dir = temp_themes_dir("reject");
        let src = temp_themes_dir("reject-src");

        let txt = write_source(&src, "theme.txt", "body {}");
        assert!(import_into(&dir, &txt).is_err());

        let big = src.join("big.css");
        std::fs::write(&big, vec![b'a'; (MAX_THEME_BYTES + 1) as usize]).expect("write big");
        let err = import_into(&dir, &big).expect_err("oversize rejected");
        assert!(err.contains("too large"), "{err}");

        let binary = src.join("bin.css");
        std::fs::write(&binary, [0xff, 0xfe, 0x00]).expect("write binary");
        let err = import_into(&dir, &binary).expect_err("non-utf8 rejected");
        assert!(err.contains("UTF-8"), "{err}");

        // 全中文文件名清洗后为空,拿不到 id。
        let cjk = write_source(&src, "主题.css", "body {}");
        assert!(import_into(&dir, &cjk).is_err());

        assert!(list_in(&dir).expect("nothing landed").is_empty());
    }

    #[test]
    fn import_accepts_exactly_the_limit() {
        // 闸门写成 `>=` 会把正好压线的文件拒掉。
        let dir = temp_themes_dir("limit");
        let src = temp_themes_dir("limit-src");
        let path = src.join("edge.css");
        std::fs::write(&path, vec![b'a'; MAX_THEME_BYTES as usize]).expect("write edge");
        let theme = import_into(&dir, &path).expect("exactly the limit is accepted");
        assert_eq!(theme.size, MAX_THEME_BYTES);
    }

    #[test]
    fn reimporting_the_same_id_overwrites_instead_of_duplicating() {
        let dir = temp_themes_dir("overwrite");
        let src = temp_themes_dir("overwrite-src");

        let first = write_source(&src, "dup.css", "body { color: red; }");
        import_into(&dir, &first).expect("first import");
        let second = write_source(&src, "dup.css", "body { color: blue; }");
        import_into(&dir, &second).expect("second import");

        assert_eq!(list_in(&dir).expect("list").len(), 1);
        assert!(read_in(&dir, "dup").expect("read").contains("blue"));
    }

    #[test]
    fn list_skips_entries_read_and_delete_could_not_address() {
        let dir = temp_themes_dir("list-skip");
        // 非 .css。
        std::fs::write(dir.join("notes.md"), "x").expect("write md");
        // 文件名 stem 与清洗结果不一致 —— 手工丢进来的,`read` 按 id 拼路径找不到它。
        std::fs::write(dir.join("has space.css"), "body {}").expect("write spaced");
        // 超限。
        std::fs::write(
            dir.join("huge.css"),
            vec![b'a'; (MAX_THEME_BYTES + 1) as usize],
        )
        .expect("write huge");
        // 正常的一条。
        std::fs::write(dir.join("good.css"), "body {}").expect("write good");

        let listed = list_in(&dir).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "good");
        // 列出来的每一条都必须真的能读回来。
        for theme in &listed {
            assert!(read_in(&dir, &theme.id).is_ok(), "{} unreadable", theme.id);
        }
    }

    #[test]
    fn list_on_a_missing_dir_is_empty_not_an_error() {
        let missing = std::env::temp_dir().join("aeroric-themes-definitely-absent");
        assert!(list_in(&missing)
            .expect("missing dir is not an error")
            .is_empty());
    }

    #[test]
    fn read_and_delete_reject_ids_that_do_not_survive_sanitizing() {
        let dir = temp_themes_dir("bad-id");
        assert!(read_in(&dir, "../../etc/passwd").is_err());
        assert!(read_in(&dir, "中文").is_err());
        assert!(delete_in(&dir, "中文").is_err());
    }
}
