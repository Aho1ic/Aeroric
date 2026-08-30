//! 全库索引:扫一遍 vault,把每篇笔记的**显示标题**取出来。
//!
//! 为什么必须有这一层:面板的笔记列表只拿元数据(路径 + mtime),正文按需读 ——
//! 几百条笔记时这是打开面板即时的唯一办法。代价是列表里未读入的笔记 `title`
//! 只能先用文件名顶着。
//!
//! 而 `[[wikilink]]` 要按标题解析。随手记的标题存在 frontmatter 里、文件名是新建
//! 时的 slug,两者可以差得很远(标题改成「周报」,文件名还是 `cao-gao.md`)。
//! 没有这一层的话 `[[周报]]` 在目标笔记**被打开过之前**解析不到 —— 而"先写链接、
//! 之后才点开那篇笔记"恰恰是双链最常见的用法。
//!
//! 这里只读每个文件的头部若干字节,不把正文交给前端:标题在 frontmatter 或第一个
//! 标题行里,读全文只是为了拿前几行。
//!
//! 索引不落盘也不缓存。一次全库扫描是"每个文件读几 KB",在几百条笔记的量级上
//! 是毫秒级;而缓存要处理外部编辑、跨进程失效,复杂度远高于它省下的时间。

use std::path::Path;

use super::fs_ops::{is_note_file, is_scan_skip_dir};

/// 单个文件读取上限。标题在头部,读多了纯属浪费。
///
/// 8KB 而不是"只读第一行":frontmatter 里可能有很多字段(第三方工具写的),
/// `title` 不一定在第一行;而 `# 标题` 那一档还要跳过整个 frontmatter 块。
const HEAD_BYTES: usize = 8 * 1024;

/// 扫描上限。和笔记树保持同一量级 —— 用户把 home 目录挂成 vault 时不能扫到天荒地老。
const MAX_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 12;

/// 一篇笔记在索引里的样子。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultIndexEntry {
    /// 绝对路径。前端拿它当身份,与笔记列表里的 `id` 是同一个值。
    pub path: String,
    /// 显示标题。优先级与前端 `deriveTitle` 一致:frontmatter title → 第一个
    /// `# 标题` → 文件名 stem。
    pub title: String,
}

/// 扫一遍 vault,返回每篇笔记的标题。
pub(crate) fn scan_vault_titles(root: &Path) -> Result<Vec<VaultIndexEntry>, String> {
    let mut out = Vec::new();
    walk(root, 0, &mut out)?;
    // 按路径排序:同一个 vault 两次扫描的结果要一致,否则前端的"歧义时取第一篇"
    // 会随文件系统的遍历顺序漂移(同名笔记时点进去的是哪一篇会变)。
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<VaultIndexEntry>) -> Result<(), String> {
    if depth > MAX_DEPTH || out.len() >= MAX_ENTRIES {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // 读不动某个子目录(权限)不该让整个索引失败 —— 其余笔记的链接仍然该能解析。
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_ENTRIES {
            return Ok(());
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // `file_type` 不跟软链;笔记树也不跟,两处保持一致,否则索引里会出现
        // 树里看不到的条目。
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if is_scan_skip_dir(&name) {
                continue;
            }
            walk(&path, depth + 1, out)?;
            continue;
        }
        if !is_note_file(&path) {
            continue;
        }
        let head = read_head(&path);
        out.push(VaultIndexEntry {
            title: derive_title(head.as_deref().unwrap_or(""), &path),
            path: path.to_string_lossy().to_string(),
        });
    }
    Ok(())
}

/// 读文件头部。读不动(权限 / 正在被写)时返回 None,标题回落到文件名。
fn read_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEAD_BYTES];
    let read = file.read(&mut buf).ok()?;
    buf.truncate(read);
    // 截断处可能切裂一个多字节字符 —— `from_utf8_lossy` 会把它变成替换字符,
    // 而那个位置在头部 8KB 的末尾,不会影响开头的标题。
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// 显示标题。与前端 `deriveTitle` 同一套优先级 —— 两边算出不同的标题会让
/// 「列表里叫 A、链接解析成 B」这种谁都想不通的现象出现。
fn derive_title(source: &str, path: &Path) -> String {
    let normalized = source.replace("\r\n", "\n");
    let (front, body) = split_frontmatter(&normalized);
    if let Some(title) = read_title_field(front) {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(heading) = first_heading(body) {
        return heading;
    }
    stem_of(path)
}

/// 拆出 (frontmatter 内容, 正文)。没有 frontmatter 时前者为空。
///
/// 与前端 `splitNote` 对齐:开了 `---` 却没闭合的不算 frontmatter(那是正文里
/// 的一条分隔线)。
///
/// 字段浏览器(`fields.rs`)复用这一份而不是自己再拆一次:两处对边界给出不同答案
/// 时,同一篇笔记会在字段浏览器里有 `title`、在笔记列表里显示文件名 —— 用户看到的
/// 是两个视图互相矛盾。
pub(crate) fn split_frontmatter(source: &str) -> (&str, &str) {
    let Some(rest) = source.strip_prefix("---\n") else {
        return ("", source);
    };
    let Some(end) = rest.find("\n---") else {
        return ("", source);
    };
    let front = &rest[..end];
    // `\n---` 之后还有那一行的换行要跳过。找不到换行说明文件正好在 `---` 结束,
    // 正文为空。
    let after = &rest[end + 4..];
    let body = after.find('\n').map(|i| &after[i + 1..]).unwrap_or("");
    (front, body)
}

/// 从 frontmatter 里读 `title`。只认单层 `key: value`,与前端一致。
fn read_title_field(front: &str) -> Option<String> {
    for line in front.lines() {
        let Some(value) = line.strip_prefix("title:") else {
            continue;
        };
        return Some(unquote_scalar(value.trim()));
    }
    None
}

/// 还原 frontmatter 标量的引号与转义。不带引号的原样返回。
///
/// 和 `split_frontmatter` 一样给 `fields.rs` 共用:字段浏览器里的值必须和标题栏里
/// 那个标题长得一样,否则同一个 `title` 在两处显示成两个东西。
pub(crate) fn unquote_scalar(value: &str) -> String {
    // 双引号标量:还原 `formatScalar` 的转义。
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        return inner.replace("\\\"", "\"").replace("\\\\", "\\");
    }
    // 单引号标量:YAML 里 '' 表示一个单引号。
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    value.to_string()
}

/// 正文里第一个 ATX 标题的文本。
fn first_heading(body: &str) -> Option<String> {
    for line in body.lines() {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        // 1..=6 个 `#` 且后面跟空白才是标题。`#hashtag` 不是。
        if hashes == 0 || hashes > 6 {
            continue;
        }
        let rest = &line[hashes..];
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let text = rest.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}
