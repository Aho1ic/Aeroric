//! 导出落盘。
//!
//! 前端负责把笔记渲染成 HTML(marked 那条管线),这里只负责**把文本写到用户选的
//! 位置**。分成两个入口:单文档(用户在保存对话框里挑了一个具体文件)和静态站点
//! (用户挑了一个目录,里面写一棵 `.html`)。
//!
//! 为什么不能直接用 `fs::write`:那等于把"任意路径写任意内容"这条原语交给 WebView。
//! 保存对话框返回的路径**不能**当作用户意图的证明 —— 命令的参数是前端给的,一个
//! 被注入的脚本可以不开对话框就直接调过来,把 `~/.zshrc` 或者启动项覆盖掉。所以
//! 写入位置要过一层白名单:当前注册的 vault,加上 Desktop / Documents / Downloads。
//! 这三个目录是导出的实际去处,同时都不含"下次登录会被执行"的东西。

use std::path::{Component, Path, PathBuf};

/// 单次导出的内容上限。整库站点导出是逐文件调用,这个上限管的是**单页**。
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// 允许写入的根目录:注册在册的 vault + 用户的常用导出目录。
pub fn export_roots(vaults: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut roots = vaults;
    // Windows 上 `HOME` 通常不存在,`USERPROFILE` 才是家目录。
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        roots.push(home.join("Desktop"));
        roots.push(home.join("Documents"));
        roots.push(home.join("Downloads"));
    }
    roots
}

/// 落在白名单里的哪个根下面。给不出根就是越界。
fn inside_roots(canon: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| canon.starts_with(&root))
}

fn check_size(content: &str) -> Result<(), String> {
    if content.len() > MAX_BYTES {
        return Err("导出内容超过 64MB 上限".to_string());
    }
    Ok(())
}

/// 路径本身是否可以当成写入目标。
///
/// NUL 单独挡:它在 Rust 的 `String` 里是合法字符,但传到系统调用会被 C 字符串
/// 在那里截断 —— `a.html\0/../../etc/x` 检查的是一条路径,写的是另一条。
fn check_path_text(text: &str, label: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err(format!("{label}为空"));
    }
    if text.contains('\0') {
        return Err(format!("{label}含非法字符"));
    }
    Ok(())
}

/// 校验单文档导出的目标路径,返回可以写的绝对路径。
///
/// 父目录不存在时会创建:保存对话框允许用户在里面新建文件夹,但某些平台上返回的
/// 是"还没落地的"路径。
pub fn validate_export_dest(path: &str, roots: &[PathBuf]) -> Result<PathBuf, String> {
    check_path_text(path, "导出路径")?;
    let dest = Path::new(path);
    if !dest.is_absolute() {
        return Err("导出路径必须是绝对路径".to_string());
    }
    // 先挡目录:后面 `fs::write` 打开一个目录报的是 IO 错误,读起来像"写入失败"
    // 而不是"你选的是个文件夹"。
    if dest.is_dir() {
        return Err("导出目标不能是文件夹".to_string());
    }
    let parent = dest
        .parent()
        .ok_or_else(|| "导出路径缺少父目录".to_string())?;
    if !parent.as_os_str().is_empty() && !parent.exists() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败:{e}"))?;
    }
    // canonicalize 父目录而不是目标本身:目标通常还不存在,canonicalize 会直接报错。
    let parent_canon = parent
        .canonicalize()
        .map_err(|e| format!("导出目录无效:{e}"))?;
    if !inside_roots(&parent_canon, roots) {
        return Err("导出位置需在笔记库或桌面 / 文档 / 下载目录内".to_string());
    }
    let name = dest
        .file_name()
        .ok_or_else(|| "导出路径缺少文件名".to_string())?;
    let resolved = parent_canon.join(name);
    // 目标已经是符号链接的话,`fs::write` 会跟着它写到链接指向的地方 —— 那个地方
    // 不受上面这道闸门管。父目录已经规范化过了,所以链接只可能是**目标自己**。
    if let Ok(meta) = resolved.symlink_metadata() {
        if meta.file_type().is_symlink() {
            return Err("导出目标是符号链接,拒绝写入".to_string());
        }
    }
    Ok(resolved)
}

/// 校验站点导出的目标,返回 `out_dir/rel_path` 的绝对路径。
///
/// `rel_path` 是前端按仓库结构算出来的(`sub/dir/note.html`)。它必须是**普通**相对
/// 路径:`..` 能从 out_dir 里爬出去,绝对路径和 Windows 盘符则直接无视 out_dir。
pub fn validate_site_dest(
    out_dir: &str,
    rel_path: &str,
    roots: &[PathBuf],
) -> Result<PathBuf, String> {
    check_path_text(out_dir, "导出目录")?;
    check_path_text(rel_path, "导出相对路径")?;
    let base = Path::new(out_dir);
    if !base.is_absolute() {
        return Err("导出目录必须是绝对路径".to_string());
    }
    let rel = Path::new(rel_path);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err("非法的导出相对路径".to_string());
    }
    // 目录先建再 canonicalize:用户可以在保存对话框里指一个还不存在的新目录。
    std::fs::create_dir_all(base).map_err(|e| format!("创建导出目录失败:{e}"))?;
    let base_canon = base
        .canonicalize()
        .map_err(|e| format!("导出目录无效:{e}"))?;
    if !inside_roots(&base_canon, roots) {
        return Err("导出位置需在笔记库或桌面 / 文档 / 下载目录内".to_string());
    }
    Ok(base_canon.join(rel))
}

/// 写一个单文档导出。
pub fn write_export(path: &str, content: &str, roots: &[PathBuf]) -> Result<(), String> {
    check_size(content)?;
    let dest = validate_export_dest(path, roots)?;
    std::fs::write(&dest, content).map_err(|e| format!("写入失败:{e}"))
}

/// 写站点里的一页。
pub fn write_site_page(
    out_dir: &str,
    rel_path: &str,
    content: &str,
    roots: &[PathBuf],
) -> Result<(), String> {
    check_size(content)?;
    let dest = validate_site_dest(out_dir, rel_path, roots)?;
    // 子目录跟着 rel_path 建。`validate_site_dest` 已经排掉了 `..`,所以这里创建的
    // 目录一定在 out_dir 底下。
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败:{e}"))?;
    }
    std::fs::write(&dest, content).map_err(|e| format!("写入失败:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let unique = format!(
            "aeroric-export-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&root).unwrap();
        // macOS 的 temp_dir 是 /var/... 而 /var 是 /private/var 的符号链接。白名单比对
        // 用的是 canonicalize 后的路径,根这边不跟着规范化的话每条断言都会假阴性。
        root.canonicalize().unwrap()
    }

    #[test]
    fn writes_inside_an_allowed_root() {
        let root = temp_root("ok");
        let dest = root.join("note.html");
        write_export(
            dest.to_str().unwrap(),
            "<h1>hi</h1>",
            std::slice::from_ref(&root),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "<h1>hi</h1>");
    }

    #[test]
    fn creates_missing_parent_directories() {
        let root = temp_root("mkdir");
        let dest = root.join("out").join("deep").join("note.html");
        write_export(dest.to_str().unwrap(), "x", std::slice::from_ref(&root)).unwrap();
        assert!(dest.exists());
    }

    #[test]
    fn rejects_a_destination_outside_every_root() {
        let root = temp_root("outside");
        let other = temp_root("elsewhere");
        let err = write_export(other.join("x.html").to_str().unwrap(), "x", &[root]).unwrap_err();
        assert!(err.contains("笔记库"), "{err}");
    }

    /// 前缀相同不等于在里面:`/tmp/a-root` 不在 `/tmp/a` 底下。`starts_with` 是按
    /// **路径分量**比的(不是字符串前缀),这条测试钉住这件事。
    #[test]
    fn rejects_a_sibling_directory_sharing_the_root_prefix() {
        let root = temp_root("sib");
        let sibling = PathBuf::from(format!("{}-extra", root.to_str().unwrap()));
        std::fs::create_dir_all(&sibling).unwrap();
        let err = write_export(sibling.join("x.html").to_str().unwrap(), "x", &[root]).unwrap_err();
        assert!(err.contains("笔记库"), "{err}");
    }

    #[test]
    fn rejects_a_relative_path() {
        let root = temp_root("rel");
        let err = write_export("note.html", "x", &[root]).unwrap_err();
        assert!(err.contains("绝对路径"), "{err}");
    }

    #[test]
    fn rejects_a_directory_as_the_target() {
        let root = temp_root("isdir");
        let err =
            write_export(root.to_str().unwrap(), "x", std::slice::from_ref(&root)).unwrap_err();
        assert!(err.contains("文件夹"), "{err}");
    }

    #[test]
    fn rejects_a_path_with_an_embedded_nul() {
        let root = temp_root("nul");
        let err = write_export(
            &format!("{}/a\0b.html", root.to_str().unwrap()),
            "x",
            &[root],
        )
        .unwrap_err();
        assert!(err.contains("非法字符"), "{err}");
    }

    #[test]
    fn rejects_content_over_the_size_cap() {
        let root = temp_root("big");
        let huge = "a".repeat(MAX_BYTES + 1);
        let err = write_export(root.join("x.html").to_str().unwrap(), &huge, &[root]).unwrap_err();
        assert!(err.contains("64MB"), "{err}");
    }

    /// 目标是符号链接时不能跟着写过去 —— 链接指向的地方不受白名单管。
    #[cfg(unix)]
    #[test]
    fn refuses_to_follow_a_symlinked_target() {
        let root = temp_root("symlink");
        let outside = temp_root("symlink-target");
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, "original").unwrap();
        let link = root.join("note.html");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        let err = write_export(link.to_str().unwrap(), "injected", &[root]).unwrap_err();
        assert!(err.contains("符号链接"), "{err}");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "original");
    }

    #[test]
    fn site_export_writes_nested_pages() {
        let root = temp_root("site");
        let out = root.join("site");
        write_site_page(
            out.to_str().unwrap(),
            "sub/dir/note.html",
            "<p>page</p>",
            std::slice::from_ref(&root),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("sub").join("dir").join("note.html")).unwrap(),
            "<p>page</p>"
        );
    }

    #[test]
    fn site_export_rejects_parent_traversal() {
        let root = temp_root("traversal");
        let out = root.join("site");
        let err = write_site_page(
            out.to_str().unwrap(),
            "../escaped.html",
            "x",
            std::slice::from_ref(&root),
        )
        .unwrap_err();
        assert!(err.contains("非法的导出相对路径"), "{err}");
        assert!(!root.join("escaped.html").exists());
    }

    /// `..` 不只在开头要挡。`a/../../b` 折叠之后也在 out_dir 外面。
    #[test]
    fn site_export_rejects_traversal_in_the_middle() {
        let root = temp_root("mid");
        let out = root.join("site");
        let err =
            write_site_page(out.to_str().unwrap(), "a/../../b.html", "x", &[root]).unwrap_err();
        assert!(err.contains("非法的导出相对路径"), "{err}");
    }

    #[test]
    fn site_export_rejects_an_absolute_relative_path() {
        let root = temp_root("abs");
        let out = root.join("site");
        let err = write_site_page(out.to_str().unwrap(), "/etc/passwd", "x", &[root]).unwrap_err();
        assert!(err.contains("非法的导出相对路径"), "{err}");
    }

    #[test]
    fn site_export_rejects_an_out_dir_outside_every_root() {
        let root = temp_root("site-outside");
        let other = temp_root("site-elsewhere");
        let err = write_site_page(
            other.join("site").to_str().unwrap(),
            "note.html",
            "x",
            &[root],
        )
        .unwrap_err();
        assert!(err.contains("笔记库"), "{err}");
    }

    /// `./note.html` 是合法的普通相对路径(`CurDir` 分量),不该被traversal 那条挡掉。
    #[test]
    fn site_export_accepts_a_leading_current_dir() {
        let root = temp_root("curdir");
        let out = root.join("site");
        write_site_page(out.to_str().unwrap(), "./note.html", "x", &[root]).unwrap();
        assert!(out.join("note.html").exists());
    }

    #[test]
    fn export_roots_include_the_usual_user_directories() {
        let root = temp_root("roots");
        let roots = export_roots(vec![root.clone()]);
        assert!(roots.contains(&root));
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);
        if let Some(home) = home {
            assert!(roots.contains(&home.join("Desktop")));
            assert!(roots.contains(&home.join("Downloads")));
        }
    }
}
