//! 落点:导入的东西写到 vault 的哪里、叫什么名字。
//!
//! Markio 这一层就是「`<workspace>/imports/<provider>/` + 逐字节 `fs::copy`」。随手记
//! 不能照抄,因为 vault 是**被索引的**,而索引对路径和文件名有意见:
//!
//! - `is_scan_skip_dir` 会整个跳过 `.notebook/` 与 `.git`、`node_modules` 那一串。落进
//!   去的笔记在应用里根本不存在 —— 导入「成功」了但用户找不到。
//! - **文件名是 wikilink 的目标**(标题在 frontmatter,文件名定了就不该再动)。所以名字
//!   在这一层定一次,之后不改 —— 导入完再改名会把刚重写好的链接全部打断。
//! - 笔记文件的判定是 `is_note_file`(`.md` / `.markdown` / `.mdx`)。落点的扩展名错了,
//!   内容在磁盘上但索引看不见。
//!
//! 另外一条不是索引带来的,是 Windows 带来的:保留设备名(`CON`、`PRN`、`AUX`、`NUL`、
//! `COM1`-`COM9`、`LPT1`-`LPT9`)即使带扩展名也创建不出文件。Markio 那段注释里的理由是
//! 对的 —— 一篇这样命名的笔记会让写入失败,而 `?` 会把**后续全部笔记**一起中断掉。

use std::path::{Path, PathBuf};

use super::super::fs_ops::is_scan_skip_dir;
#[cfg(test)]
use super::super::fs_ops::VAULT_PRIVATE_DIR;

/// 导入落点的顶层目录名。所有 provider 落在它下面,一个 provider 一个子目录。
///
/// 单独一层是为了让「哪些笔记是导进来的」在树上一眼可见,也让用户能整目录挪走或删掉。
pub const IMPORT_DIR: &str = "imports";

/// 文件名清洗。
///
/// 保留:字母数字(**含 CJK**)、`-`、`_`、`.`、空格。其余换成 `_`。
///
/// CJK 必须保留 —— 中文笔记占多数,洗掉的话导入结果是一目录的 `____.md`,而文件名
/// 同时还是 wikilink 的目标,等于把链接目标也洗成一样的。
///
/// 注意这里和 `attachments::sanitize_stem` 的口径**故意不同**:那边只留字母数字和
/// `-_`(附件名不是链接目标,洗狠一点无所谓),这边要保住可读性。
pub fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ' ' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    // 前后空白和首尾的点都去掉:`. hidden .md` 在 Unix 上是隐藏文件,在 Windows 上
    // 尾部的点会被系统静默吃掉(于是磁盘上的名字和记录在报告里的名字不一致)。
    let trimmed = out.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        return "imported".to_string();
    }
    if is_windows_reserved(trimmed) {
        return format!("_{trimmed}");
    }
    trimmed.to_string()
}

/// Windows 保留设备名。带扩展名也一样创建不出来(`CON.md` 同样失败)。
fn is_windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    // COM1-COM9 / LPT1-LPT9。`COM0` 不是保留名。
    let bytes = stem.as_bytes();
    (stem.starts_with("COM") || stem.starts_with("LPT"))
        && bytes.len() == 4
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

/// 洗一条**相对路径**的每一段,并挡掉会让落点跑出去或者被索引跳过的那些。
///
/// 三件事:
///
/// 1. `..` 与绝对路径前缀直接判非法。zip 里的 `../../.zshrc` 是真实存在的攻击面;
///    zip 那一路还会额外过 `enclosed_name()`,但目录型导入没有那道,所以这里必须自带。
/// 2. 每一段过 [`sanitize_name`]。
/// 3. 任何一段落在 [`is_scan_skip_dir`] 里就判非法 —— 源端有个 `node_modules/` 或者
///    `.git/`,照原样落进 vault 的话那一整棵子树索引根本不看,用户会以为导入漏了文件。
///    报成 `Skipped` 才是诚实的。
///
/// 返回 `None` = 这条路径不该落。
pub fn sanitize_relative(path: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    // 反斜杠也当分隔符:zip 里 Windows 打出来的条目用 `\`,不拆的话整条会被当成
    // **一个**文件名,于是 `a\..\b` 洗完变成一个叫 `a_.._b` 的文件而不是被拒。
    for raw in path.split(['/', '\\']) {
        match raw {
            "" | "." => continue,
            ".." => return None,
            _ => {}
        }
        // Windows 盘符前缀(`C:`)与 UNC 残留。洗过之后 `:` 会变 `_`,那时候就看不出
        // 它原本是个绝对路径了,所以在洗之前判。
        if raw.len() >= 2 && raw.as_bytes()[1] == b':' {
            return None;
        }
        if is_scan_skip_dir(raw) {
            return None;
        }
        let clean = sanitize_name(raw);
        // 洗完撞上被跳过的目录名也要挡。`.notebook` 洗完还是 `.notebook`,
        // 但 `node modules` 洗完是 `node modules`(不撞),真正会撞的是那些
        // 只差一个非法字符的名字。
        if is_scan_skip_dir(&clean) {
            return None;
        }
        parts.push(clean);
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// provider 的落点目录(vault 相对)。
pub fn provider_dir(provider: &str) -> String {
    format!("{IMPORT_DIR}/{}", sanitize_name(provider))
}

/// 落点的绝对路径,并确认它确实在 vault 里面。
///
/// 拼完再校验一次而不是信 [`sanitize_relative`]:那个函数管的是「这条相对路径干净吗」,
/// 这里管的是「拼出来的绝对路径还在 vault 下吗」。两件事分开,因为 vault 本身可能是个
/// 软链,`starts_with` 在没 canonicalize 的路径上不成立。
///
/// 用 `canonicalize` 的父目录版本:落点文件此刻还不存在,对它自己 canonicalize 会失败。
pub fn resolve_in_vault(vault: &Path, relative: &str) -> Result<PathBuf, String> {
    let clean = sanitize_relative(relative).ok_or_else(|| format!("非法落点:{relative}"))?;
    let target = vault.join(&clean);
    let parent = target
        .parent()
        .ok_or_else(|| format!("落点没有父目录:{relative}"))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("创建 {} 失败:{e}", parent.display()))?;
    let root = vault
        .canonicalize()
        .map_err(|e| format!("vault 不可用:{e}"))?;
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("落点父目录不可用:{e}"))?;
    if !parent.starts_with(&root) {
        return Err(format!("落点跑出 vault:{relative}"));
    }
    Ok(target)
}

/// 不覆盖的落点:撞名就加 `-2`、`-3`。
///
/// 为什么不覆盖:导入的对面是用户已有的笔记。一次 `imports/obsidian/Note.md` 的重名
/// 覆盖掉的可能是上一次导入后用户已经编辑过的内容,而导入这个动作本身不该有删改权限。
///
/// 后缀加在**扩展名之前**(`Note-2.md`,不是 `Note.md-2`)。顺序错了落点就不再是
/// `is_note_file` 认的笔记,内容在磁盘上而索引看不见。
pub fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let clean = sanitize_name(file_name);
    let candidate = dir.join(&clean);
    if !candidate.exists() {
        return candidate;
    }
    let as_path = Path::new(&clean);
    let stem = as_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("imported");
    let ext = as_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    for i in 2..10_000 {
        let next = if ext.is_empty() {
            format!("{stem}-{i}")
        } else {
            format!("{stem}-{i}.{ext}")
        };
        let path = dir.join(next);
        if !path.exists() {
            return path;
        }
    }
    // 一万个同名走到这里。带上纳秒基本不可能再撞,而且比返回 Err 好 ——
    // 已经导了一万篇的这一轮不该因为第一万零一篇失败而整体报错。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    dir.join(if ext.is_empty() {
        format!("{stem}-{nanos}")
    } else {
        format!("{stem}-{nanos}.{ext}")
    })
}

/// 报告笔记的文件名。带时间戳 —— 报告自己也是 vault 里的一篇笔记,固定名字的话
/// 第二次导入要么覆盖掉上一次的报告,要么被 [`unique_path`] 加成 `-2` 那种看不出
/// 时间的名字。
pub fn report_name(provider: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    format!("导入报告-{}-{stamp}.md", sanitize_name(provider))
}

/// vault 私有目录名。落点校验里要用它做否定断言,导出来避免测试重复写字面量。
///
/// 本文件的生产路径不直接用这个常量 —— 私有目录是被 `is_scan_skip_dir` 一并挡掉的,
/// 所以包装与它的导入都只在测试下编译。
#[cfg(test)]
pub fn private_dir_name() -> &'static str {
    VAULT_PRIVATE_DIR
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
            "aeroric-import-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("建临时 vault");
        dir
    }

    #[test]
    fn cjk_survives_sanitizing() {
        // 洗掉 CJK 的话中文笔记会全变成 `____.md`,而文件名同时是 wikilink 的目标。
        assert_eq!(sanitize_name("我的笔记.md"), "我的笔记.md");
    }

    #[test]
    fn path_separators_and_control_chars_become_underscores() {
        assert_eq!(sanitize_name("a/b:c*d?.md"), "a_b_c_d_.md");
        assert_eq!(sanitize_name("tab\there.md"), "tab_here.md");
    }

    #[test]
    fn empty_after_sanitizing_falls_back() {
        assert_eq!(sanitize_name(""), "imported");
        assert_eq!(sanitize_name("..."), "imported");
        assert_eq!(sanitize_name("   "), "imported");
    }

    #[test]
    fn windows_reserved_device_names_get_prefixed() {
        // 带扩展名也创建不出来。写失败会让 `?` 中断整轮导入,丢掉后面全部笔记。
        for name in [
            "CON",
            "con.md",
            "PRN.md",
            "AUX",
            "NUL.markdown",
            "com1.md",
            "LPT9.md",
        ] {
            assert!(
                sanitize_name(name).starts_with('_'),
                "{name} 是 Windows 保留名,应加前缀"
            );
        }
    }

    #[test]
    fn lookalikes_of_reserved_names_are_left_alone() {
        // `COM0` 不是保留名;`CONSOLE` / `COM10` 也不是。全加前缀会把正常笔记改名,
        // 而改名会打断指向它的 wikilink。
        for name in ["COM0.md", "CONSOLE.md", "COM10.md", "LPT.md", "CONTRACT.md"] {
            assert!(
                !sanitize_name(name).starts_with('_'),
                "{name} 不该被当成保留名"
            );
        }
    }

    #[test]
    fn trailing_dots_are_stripped() {
        // Windows 会静默吃掉尾部的点,于是磁盘上的名字和报告里记的名字不一致。
        assert_eq!(sanitize_name("note.md."), "note.md");
        assert_eq!(sanitize_name(".hidden"), "hidden");
    }

    #[test]
    fn parent_traversal_is_rejected() {
        assert_eq!(sanitize_relative("../../.zshrc"), None);
        assert_eq!(sanitize_relative("a/../../b"), None);
    }

    #[test]
    fn backslash_is_also_a_separator() {
        // zip 里 Windows 打出来的条目用 `\`。不拆的话 `a\..\b` 会洗成一个叫
        // `a_.._b` 的文件,而不是被判非法。
        assert_eq!(sanitize_relative(r"a\..\b"), None);
        assert_eq!(sanitize_relative(r"a\b\c.md"), Some("a/b/c.md".to_string()));
    }

    #[test]
    fn drive_prefixes_are_rejected_before_sanitizing() {
        // 洗过之后 `:` 变 `_`,那时候已经看不出它原本是绝对路径了。
        assert_eq!(sanitize_relative("C:/Windows/x.md"), None);
        assert_eq!(sanitize_relative(r"C:\Windows\x.md"), None);
    }

    #[test]
    fn skipped_dirs_are_rejected_not_silently_landed() {
        // 落进去的话那一整棵子树索引不看,用户会以为导入漏了文件。
        for path in [
            "node_modules/pkg/readme.md",
            ".git/config",
            &format!("{}/history/x.md", private_dir_name()),
            "a/target/b.md",
        ] {
            assert_eq!(sanitize_relative(path), None, "{path} 应被判非法");
        }
    }

    #[test]
    fn ordinary_nested_paths_pass_through() {
        assert_eq!(
            sanitize_relative("pages/项目 A/note.md"),
            Some("pages/项目 A/note.md".to_string())
        );
        assert_eq!(sanitize_relative("./a/./b.md"), Some("a/b.md".to_string()));
    }

    #[test]
    fn empty_relative_paths_are_rejected() {
        assert_eq!(sanitize_relative(""), None);
        assert_eq!(sanitize_relative("///"), None);
        assert_eq!(sanitize_relative("./."), None);
    }

    #[test]
    fn provider_dir_lives_under_the_import_root() {
        assert_eq!(provider_dir("notion"), "imports/notion");
        // provider 名同样要洗:它进的是路径。`/` 变 `_`,前导点被 trim 掉,
        // 于是 `../evil` 落成一个普通的单段名字,不再是「上一层」。
        assert_eq!(provider_dir("../evil"), "imports/_evil");
    }

    #[test]
    fn unique_path_appends_before_the_extension() {
        // 顺序错了(`Note.md-2`)落点就不再是 `is_note_file` 认的笔记,
        // 内容在磁盘上而索引看不见。
        let dir = temp_vault("unique");
        std::fs::write(dir.join("Note.md"), "x").expect("写第一份");
        let next = unique_path(&dir, "Note.md");
        assert_eq!(next.file_name().unwrap().to_str().unwrap(), "Note-2.md");
        std::fs::write(&next, "y").expect("写第二份");
        let third = unique_path(&dir, "Note.md");
        assert_eq!(third.file_name().unwrap().to_str().unwrap(), "Note-3.md");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unique_path_never_overwrites() {
        // 导入不该有删改权限:撞上的可能是用户上次导入后已经编辑过的内容。
        let dir = temp_vault("nooverwrite");
        std::fs::write(dir.join("a.md"), "原始内容").expect("写");
        let next = unique_path(&dir, "a.md");
        assert_ne!(next, dir.join("a.md"));
        assert_eq!(
            std::fs::read_to_string(dir.join("a.md")).expect("读"),
            "原始内容"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resolve_creates_parents_and_stays_inside_the_vault() {
        let vault = temp_vault("resolve");
        let target = resolve_in_vault(&vault, "imports/notion/sub/a.md").expect("落点合法");
        assert!(target.parent().expect("父目录").is_dir());
        assert!(target.starts_with(&vault));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn resolve_rejects_traversal_and_skipped_dirs() {
        let vault = temp_vault("resolve-bad");
        assert!(resolve_in_vault(&vault, "../outside.md").is_err());
        assert!(resolve_in_vault(&vault, "node_modules/a.md").is_err());
        // 私有目录被拦住,否则导入的笔记会落到历史快照和回收站旁边。
        assert!(resolve_in_vault(&vault, &format!("{}/a.md", private_dir_name())).is_err());
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn report_name_is_timestamped_and_is_a_note() {
        let name = report_name("notion");
        assert!(name.ends_with(".md"), "报告要能被索引当成笔记:{name}");
        assert!(name.starts_with("导入报告-notion-"));
        // 时间戳段:`-YYYYmmdd-HHMMSS.md`。固定名字会让第二次导入覆盖或变成 `-2`。
        let stamp = name
            .trim_start_matches("导入报告-notion-")
            .trim_end_matches(".md");
        let (date, time) = stamp.split_once('-').expect("日期和时间用 - 分隔");
        assert_eq!(date.len(), 8);
        assert_eq!(time.len(), 6);
        assert!(date.chars().all(|c| c.is_ascii_digit()));
        assert!(time.chars().all(|c| c.is_ascii_digit()));
    }
}
