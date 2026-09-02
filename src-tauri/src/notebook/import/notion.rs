//! Notion:导出 zip。
//!
//! Notion 的导出有两个特征要处理。
//!
//! **一:文件名带 32 位 hash 后缀。** `我的页面 a1b2c3....md`。不剥掉的话 vault 里全是
//! 这种名字,而文件名同时是 wikilink 的目标 —— 用户没法手写链接指到它。
//!
//! **二:页面之间的链接是 URL 编码的相对路径。** `[标题](我的页面%20a1b2c3....md)`。
//! 剥掉 hash 之后这些链接全部指向不存在的文件,所以要一起重写成 `[[标题]]`。
//!
//! Markio 那份实现的两个问题这里都改掉了:
//!
//! - 它用 `raw_name.split('/').last()` 取文件名,把归档里的**目录结构整个丢掉** ——
//!   Notion 导出里子页面是嵌套目录,压平之后同名子页面互相撞名(靠 `-2` 区分),而资源
//!   引用的相对路径全断。这里保留结构(路径守卫走 `enclosed_name`,见 `zip_src`)。
//! - 它的 `regex_like_strip` 有一条 `head.to_string()` 的兜底分支:名字里只要有空格但
//!   尾段不是 hash,就会把**最后一个空格之后的内容整段砍掉**。`我的 笔记.md` 会变成
//!   `我的`,连扩展名一起丢。这里只在确实认出 hash 时才动。

use std::path::Path;

use super::landing;
use super::manifest::{self, Session};
use super::report::{ImportReport, SkipReason};
use super::run;
use super::zip_src::{self, Handled};

pub const PROVIDER: &str = "notion";

/// Notion 的 hash 后缀长度。导出用的是 32 位十六进制,但历史上也见过 24 位,
/// 所以判定按「≥24 且全是 hex」来。
const MIN_HASH_LEN: usize = 24;

pub fn import(vault: &Path, archive: &Path) -> Result<ImportReport, String> {
    let file = std::fs::File::open(archive).map_err(|error| format!("打开 zip 失败:{error}"))?;
    run::run(vault, PROVIDER, &mut |session, dest_dir, report| {
        zip_src::walk_archive(
            &file,
            session,
            dest_dir,
            report,
            &mut |session, dest_dir, relative, bytes, _report| {
                handle(session, dest_dir, relative, bytes)
            },
        )
    })
}

fn handle(session: &mut Session, dest_dir: &Path, relative: &str, bytes: Vec<u8>) -> Handled {
    let key = manifest::fingerprint(&format!("{PROVIDER}::{relative}"));
    if session.is_known(&key) {
        return Handled::Skipped(SkipReason::AlreadyImported);
    }

    // 落点:逐段剥 hash。目录名也带 hash(子页面的目录),不剥的话目录名一样难认。
    let cleaned: Vec<String> = relative.split('/').map(strip_hash).collect();
    let Some((name, parents)) = cleaned.split_last() else {
        return Handled::Ignored;
    };
    let mut dir = dest_dir.to_path_buf();
    for parent in parents {
        dir.push(landing::sanitize_name(parent));
    }
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return Handled::Failed(format!("建目录失败:{error}"));
    }
    let target = landing::unique_path(&dir, name);

    let is_markdown = name.to_ascii_lowercase().ends_with(".md");
    let write_result = if is_markdown {
        // 只有 markdown 过重写。资源是二进制,按 UTF-8 解一遍会损坏它。
        let text = String::from_utf8_lossy(&bytes);
        std::fs::write(&target, rewrite_links(&text))
    } else {
        std::fs::write(&target, &bytes)
    };
    if let Err(error) = write_result {
        return Handled::Failed(format!("写入失败:{error}"));
    }

    session.record(key);
    let dest = dest_relative(dest_dir, &target);
    Handled::Landed {
        dest,
        issues: Vec::new(),
    }
}

/// 剥掉 ` <hash>` 后缀,扩展名保留。
///
/// 只在尾段**确实**是 ≥24 位十六进制时才动。认不出就原样返回 —— 一个正常的
/// 「我的 笔记.md」不该被砍成「我的」。
fn strip_hash(name: &str) -> String {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem, Some(extension)),
        None => (name, None),
    };
    let Some((title, tail)) = stem.rsplit_once(' ') else {
        return name.to_string();
    };
    if tail.len() < MIN_HASH_LEN || !tail.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return name.to_string();
    }
    if title.is_empty() {
        // 整个名字就是一个 hash。剥完会剩空串,那时候保留原样更有用。
        return name.to_string();
    }
    match extension {
        Some(extension) => format!("{title}.{extension}"),
        None => title.to_string(),
    }
}

/// 把 Notion 的页面链接改写成 wikilink。
///
/// 判定条件:`](...)` 里的 URL 以 `.md` 结尾,而且看得出是 Notion 那种带 hash 的名字
/// (URL 编码的空格 `%20` 后跟一段 hex)。**不能只看 `.md` 结尾** —— 用户自己写的
/// `[说明](./readme.md)` 也满足那个条件,改成 `[[说明]]` 就把一条本来有效的相对链接
/// 换成了一个指向不存在笔记的 wikilink。
fn rewrite_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open..];
        // `[label](url)` 的形状。找不到就把这个 `[` 当普通字符。
        let Some(mid) = after.find("](") else {
            out.push('[');
            rest = &after[1..];
            continue;
        };
        let label = &after[1..mid];
        let url_start = mid + 2;
        let Some(close) = after[url_start..].find(')') else {
            out.push('[');
            rest = &after[1..];
            continue;
        };
        let url = &after[url_start..url_start + close];
        if is_notion_page_link(url) && !label.is_empty() {
            out.push_str("[[");
            out.push_str(label);
            out.push_str("]]");
        } else {
            out.push_str(&after[..url_start + close + 1]);
        }
        rest = &after[url_start + close + 1..];
    }
    out.push_str(rest);
    out
}

fn is_notion_page_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if !lower.ends_with(".md") {
        return false;
    }
    // 外部链接不动。
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return false;
    }
    // 取最后一段(可能带目录),看它是不是 `名字%20<hash>.md`。
    let last = url.rsplit('/').next().unwrap_or(url);
    // 去掉 `.md`。按字节切是安全的:上面已经确认它以 `.md` 结尾,而那三个都是 ASCII,
    // 所以 `len() - 3` 一定落在字符边界上。
    let stem = &last[..last.len() - 3];
    let Some((_, tail)) = stem.rsplit_once("%20") else {
        return false;
    };
    tail.len() >= MIN_HASH_LEN && tail.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// 落点的 vault 相对路径。`dest_dir` 是 `<vault>/imports/notion`,报告里要的是
/// `imports/notion/...`,所以拿 provider 目录名重新拼。
fn dest_relative(dest_dir: &Path, target: &Path) -> String {
    let prefix = landing::provider_dir(PROVIDER);
    match target.strip_prefix(dest_dir) {
        Ok(rest) => format!("{prefix}/{}", rest.to_string_lossy().replace('\\', "/")),
        Err(_) => prefix,
    }
}
