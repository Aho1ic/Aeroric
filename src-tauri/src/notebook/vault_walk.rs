//! 全库正文遍历:递归走一遍 vault,把每篇笔记的**全文**交给 visitor。
//!
//! 反链(`links.rs`)和标签(`tags.rs`)都要读全文,遍历规则也必须逐条一致 ——
//! 不跟软链、跳过私有目录、跳过非 `.md`、跳过超大文件、深度与文件数有上限。抄两份
//! 的代价不是重复代码本身,而是它们会各自漂移:比如反链跳过了回收站、标签没跳,
//! 于是"标签云里有 3 篇,点进去只有 2 篇"。
//!
//! 两档共用侧栏那一列(互斥),所以刻意**不**合并成一次扫描返回两样东西 —— 永远
//! 只有一档在场,合并只会让每次扫描都多做一半没人看的提取。共享的是遍历(IO),
//! 不是提取(CPU)。
//!
//! 索引不落盘也不缓存,和 `vault_index.rs` 同一个理由:一次全库扫描在几百条笔记的
//! 量级上是毫秒级,而缓存要处理外部编辑和跨进程失效。

use std::path::Path;

use super::fs_ops::{is_note_file, is_scan_skip_dir};

/// 单个文件的读取上限。超了整篇跳过 —— 侧栏的索引不值得为一个几 MB 的文件卡住,
/// 而正常笔记远小于这个数。
pub(crate) const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 全库上限。和笔记树同一量级,防止 home 目录被挂成 vault 时扫到天荒地老。
pub(crate) const MAX_FILES: usize = 20_000;
pub(crate) const MAX_DEPTH: usize = 12;

/// visitor 的回执:还要不要继续走。
///
/// 各档自己的产出上限(单篇多少条、全库多少条)由 visitor 决定 —— 遍历这一层不
/// 知道"一条链接"或"一个标签"是什么,把额度算进来只会让它同时服务两套语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkNext {
    Continue,
    Stop,
}

/// 走一遍 `root` 下的全部笔记,对每篇调用 `visit(路径, 全文)`。
///
/// visitor 返回 `Stop` 时立刻收工。读不动的文件(权限、非 UTF-8、正在被写)静默
/// 跳过:这些索引都是只读视图,为一个文件报错会让整份结果消失。
pub(crate) fn walk_notes<F>(root: &Path, visit: &mut F) -> Result<(), String>
where
    F: FnMut(&Path, &str) -> WalkNext,
{
    let mut files = 0usize;
    walk(root, 0, &mut files, visit)?;
    Ok(())
}

fn walk<F>(dir: &Path, depth: usize, files: &mut usize, visit: &mut F) -> Result<WalkNext, String>
where
    F: FnMut(&Path, &str) -> WalkNext,
{
    if depth > MAX_DEPTH || *files >= MAX_FILES {
        return Ok(WalkNext::Stop);
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // 读不动某个子目录(权限)不该让整次扫描失败 —— 其余笔记仍然该出来。
        Err(_) => return Ok(WalkNext::Continue),
    };
    for entry in entries.flatten() {
        if *files >= MAX_FILES {
            return Ok(WalkNext::Stop);
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // 不跟软链,与笔记树 / 标题索引一致:跟了会让同一篇笔记以两个路径出现,
        // 反链里就是两条一模一样的条目,标签云里就是重复计数。
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
            if walk(&path, depth + 1, files, visit)? == WalkNext::Stop {
                return Ok(WalkNext::Stop);
            }
            continue;
        }
        if !is_note_file(&path) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
        }
        *files += 1;
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if visit(&path, &content) == WalkNext::Stop {
            return Ok(WalkNext::Stop);
        }
    }
    Ok(WalkNext::Continue)
}

/// frontmatter 占掉的行数(0 = 没有 frontmatter)。
///
/// 未闭合的 `---` **不算** frontmatter:那多半是一条分隔线。这与 `noteOutline.ts` 的
/// 既有判定一致 —— 两边不一致的话同一篇笔记的"哪里算正文"会随功能而变。
///
/// 标签和任务共用这一份:两处不一致的话,frontmatter 里写了 `- [ ]` 的那一篇会在
/// 任务收集箱里多出一条跳过去什么都没有的任务,而标签档同一篇是干净的。
pub(crate) fn frontmatter_lines(lines: &[(usize, &str)]) -> usize {
    if lines.first().map(|(_, line)| line.trim()) != Some("---") {
        return 0;
    }
    lines
        .iter()
        .skip(1)
        .position(|(_, line)| line.trim() == "---")
        // position 是相对 skip(1) 的下标:+2 才是"闭合行之后"的行数。
        .map(|at| at + 2)
        .unwrap_or(0)
}

/// 逐行切分,给出每行的起始字节偏移和内容(行尾的 `\r` 摘掉但**算进偏移**)。
pub(crate) fn line_spans(content: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut cursor = 0usize;
    content.split('\n').map(move |raw| {
        let base = cursor;
        cursor += raw.len() + 1; // +1 是那个 `\n`
        (base, raw.strip_suffix('\r').unwrap_or(raw))
    })
}

/// 这一行是不是围栏标记。返回 (字符, 连续个数)。
///
/// 和 `frontmatter_lines` 同一个理由要共用:围栏里的 `- [ ]` 不该出现在收集箱里
/// (marked 不会为它产复选框,阅读态那里也点不到),而围栏里的 `#include` 不该出现在
/// 标签云里 —— 两处对"围栏从哪到哪"的答案必须是同一个。
pub(crate) fn fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|c| *c == first).count();
    if count >= 3 {
        Some((first, count))
    } else {
        None
    }
}

/// 一行里的行内代码区间(字节),含两侧的反引号。
///
/// 闭合的反引号数量必须和开启的一样多(CommonMark 规则),这样 `` `a` `` 里的单个
/// 反引号不会提前收尾。没闭合的反引号是字面量,后面的内容照常算标签 / 提及。
///
/// 和 `fence_marker` 同一个理由要共用:标签档说"`` `#fff` `` 是颜色值不是标签",
/// 未链接提及那一档也必须说"`` `计划` `` 里的字样不是提及"。两处各写一份的表现是
/// 「同一行在标签云里干净、在提及列表里却冒出一条」,而这种不一致看起来像其中一档
/// 扫错了行。
pub(crate) fn inline_code_spans(line: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let ticks = i - start;
        let mut j = i;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] != b'`' {
                j += 1;
                continue;
            }
            let run_start = j;
            while j < bytes.len() && bytes[j] == b'`' {
                j += 1;
            }
            if j - run_start == ticks {
                closed = Some(j);
                break;
            }
        }
        match closed {
            Some(end) => {
                spans.push((start, end));
                i = end;
            }
            // 没闭合:这串反引号是字面量,不构成代码区间。
            None => break,
        }
    }
    spans
}

/// 预览截断长度(按字符,不是字节)。
const PREVIEW_CHARS: usize = 160;

/// 一行的预览:两端 trim,超长按字符截断并加省略号。
///
/// 反链和标签共用同一份截断规则 —— 两处不一致的话同一行在两档里长得不一样,而这
/// 种差异看起来像"其中一档把内容截错了"。
pub(crate) fn preview_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= PREVIEW_CHARS {
        return trimmed.to_string();
    }
    let mut preview: String = trimmed.chars().take(PREVIEW_CHARS).collect();
    preview.push('…');
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_trims_and_truncates() {
        assert_eq!(preview_line("   见 [[周报]]   "), "见 [[周报]]");
        let long = "字".repeat(300);
        let preview = preview_line(&long);
        assert_eq!(preview.chars().count(), PREVIEW_CHARS + 1);
        assert!(preview.ends_with('…'));
    }
}
