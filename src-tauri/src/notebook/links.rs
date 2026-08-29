//! 全库链接扫描:每篇笔记正文里出现过的 `[[...]]`,连同行号和那一行的预览。
//!
//! **这一层只做词法提取,不做解析。** 它给出的是"第 12 行有一条内容为 `周报#本周`
//! 的 wikilink",而不是"第 12 行指向 `/vault/cao-gao.md`"。
//!
//! 为什么这么切:解析规则(stem / 标题 / 路径尾段的优先级、`#小节`、`|别名`、
//! 归一化、歧义)全在前端 `noteLinks.ts` 里,那份有 29 条测试。把它在 Rust 里
//! 抄一遍就有了两份会各自漂移的实现 —— 而漂移的表现是"渲染出来的链接能点,反链
//! 面板里却看不到它",谁都不会想到去怀疑是两套规则不一致。
//!
//! 反过来,扫描必须在 Rust:反链要读**全文**(链接可能在最后一行),几百篇笔记
//! 意味着几百次 IPC 往返 + 把整个 vault 的正文搬进 JS 堆。
//!
//! 与 Markio 的差异:Markio 的 `find_backlinks` 直接在 Rust 里按文件名 stem grep
//! `[[stem`,一个文件只出一条。这里给出所有出现,因为随手记的链接可以按 frontmatter
//! 标题写(文件名是新建时的 slug,之后改标题不改文件名),按 stem grep 会整片漏掉。

use std::path::Path;

use super::vault_walk::{preview_line, walk_notes, WalkNext};

/// 单篇笔记记多少条链接。超出的丢掉:一篇里 1000 条链接已经不是人写出来的。
const MAX_LINKS_PER_FILE: usize = 1_000;

/// 全库链接上限。文件数 / 深度 / 单文件大小的上限在 `vault_walk` 那一层。
const MAX_TOTAL_LINKS: usize = 20_000;

/// 一条 wikilink 出现。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteLinkRef {
    /// 方括号里的原始内容,未解析、未归一化。前端拿它过 `parseWikiLinkBody`。
    pub raw: String,
    /// 1-based 行号。点一条反链要跳到这里。
    pub line: u32,
    /// 那一行的文本(两端 trim,超长截断)。
    pub preview: String,
    /// 是不是 `![[...]]` 嵌入。嵌入也算引用,但 UI 上要能区分。
    pub embed: bool,
}

/// 一篇笔记里的全部链接。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteLinkSource {
    /// 绝对路径,与笔记列表里的 `id` 同一个值。
    pub path: String,
    pub links: Vec<NoteLinkRef>,
}

/// 扫一遍 vault,返回每篇**含链接**的笔记及其链接。
///
/// 没有链接的笔记不进结果:反链面板只关心"谁指向了谁",空文件占位只是让 payload
/// 变大。
pub(crate) fn scan_vault_links(root: &Path) -> Result<Vec<NoteLinkSource>, String> {
    let mut out = Vec::new();
    let mut total = 0usize;
    walk_notes(root, &mut |path, content| {
        // 整篇没有 `[[` 就不用逐行了。绝大多数笔记走这条捷径。
        if !content.contains("[[") {
            return WalkNext::Continue;
        }
        let links = scan_links(content, MAX_TOTAL_LINKS - total);
        if links.is_empty() {
            return WalkNext::Continue;
        }
        total += links.len();
        out.push(NoteLinkSource {
            path: path.to_string_lossy().to_string(),
            links,
        });
        if total >= MAX_TOTAL_LINKS {
            return WalkNext::Stop;
        }
        WalkNext::Continue
    })?;
    // 按路径排序,和 `vault_index` 同一个理由:两次扫描的结果顺序要一致,否则
    // 反链列表的排列会随文件系统遍历顺序漂移。
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 扫一篇正文里的所有 wikilink。
///
/// 逐行扫是安全的:wikilink 不跨行(前端那条正则的 body 排除了 `\n`),所以
/// 按行切和整篇扫等价,而按行切天然拿到行号。
fn scan_links(content: &str, remaining: usize) -> Vec<NoteLinkRef> {
    let cap = remaining.min(MAX_LINKS_PER_FILE);
    let mut out: Vec<NoteLinkRef> = Vec::new();
    if cap == 0 {
        return out;
    }
    for (index, line) in content.lines().enumerate() {
        if out.len() >= cap {
            break;
        }
        if !line.contains("[[") {
            continue;
        }
        let preview = preview_line(line);
        for found in scan_line(line) {
            if out.len() >= cap {
                break;
            }
            out.push(NoteLinkRef {
                raw: found.raw,
                line: (index + 1) as u32,
                preview: preview.clone(),
                embed: found.embed,
            });
        }
    }
    out
}

struct LineHit {
    raw: String,
    embed: bool,
}

/// 前端那条正则的 body 上限(UTF-16 code unit)。
///
/// 必须和 `noteLinks.ts` 里的 `{1,200}` 对上,而 JS 的正则量词数的是 UTF-16 code
/// unit —— emoji 那类星平面字符在 JS 里算两个。用 `len_utf16` 而不是 `chars().count()`
/// 就是为了这个:两边判定不一致时,超长链接会"渲染出来是普通文本、反链里却算一条",
/// 而这种偏差只在含 emoji 的长链接上出现,几乎不可能被人工发现。
const MAX_BODY_UNITS: usize = 200;

/// 扫一行里的 wikilink。
///
/// 这是前端 `/\[\[([^\]\n]{1,200})\]\]/g` 的等价实现,包括它的回溯行为:body 里
/// 不许出现 `]`,所以遇到单个 `]` 就整个匹配失败、从下一个位置重试。
fn scan_line(line: &str) -> Vec<LineHit> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor + 4 <= bytes.len() {
        // 找下一个 `[[`。
        let Some(offset) = line[cursor..].find("[[") else {
            break;
        };
        let open = cursor + offset;
        let body_start = open + 2;
        // body 一路吃到第一个 `]`(前端正则的 `[^\]\n]` 不许 body 含 `]`)。
        let rest = &line[body_start..];
        let Some(bracket) = rest.find(']') else {
            break;
        };
        let body = &rest[..bracket];
        let closed = rest[bracket..].starts_with("]]");
        let units: usize = body.chars().map(char::len_utf16).sum();
        if !closed || body.is_empty() || units > MAX_BODY_UNITS {
            // 不是一条合法链接。从 `[[` 之后一格重试 —— 正则引擎也是这么退的,
            // 于是 `[[[[a]]` 里那条 `[[a]]` 仍然能被认出来。
            cursor = open + 1;
            continue;
        }
        out.push(LineHit {
            raw: body.to_string(),
            // `![[...]]` 是嵌入。看 `[[` 前一个字节即可 —— `!` 是 ASCII,不会
            // 落在多字节字符中间。
            embed: open > 0 && bytes[open - 1] == b'!',
        });
        cursor = body_start + bracket + 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raws(line: &str) -> Vec<String> {
        scan_line(line).into_iter().map(|hit| hit.raw).collect()
    }

    /// 与前端那条正则的**共享黄金用例**。
    ///
    /// 同一张表在 `src/test/notebook-note-links.test.ts` 的
    /// `与 Rust 侧词法提取的共享黄金用例` 里跑一遍 `scanWikiLinks`,期望值逐字相同。
    /// 两边各自的注释都写着"我和另一边等价" —— 声明不值钱,同一张表两边都过才值钱。
    /// 改这里的任何一行,记得同步改那边。
    fn golden() -> Vec<(&'static str, Vec<String>)> {
        let long_ok = "x".repeat(200);
        let long_over = "x".repeat(201);
        let emoji_ok = "🙂".repeat(100);
        let emoji_over = "🙂".repeat(101);
        vec![
            (
                "见 [[周报]] 和 [[notes/foo|别名]]",
                vec!["周报".into(), "notes/foo|别名".into()],
            ),
            // 目标为空不是链接。
            ("[[]] 空的", vec![]),
            // 没闭合的、只有一个 `]` 的,都不是。
            ("[[周报 没有闭合", vec![]),
            ("[[周报]", vec![]),
            // body 里不许有 `]`,于是这两行整个不匹配 —— 哪怕后面还有一对 `]]`。
            ("[[a]b]]", vec![]),
            ("[[a] b]]", vec![]),
            // 多余的 `[[` 落进 body 里(正则从失败位置之后一格重试的结果)。
            ("[[[[a]]", vec!["[[a".into()]),
            ("[[a]] [[b]]", vec!["a".into(), "b".into()]),
            ("![[图]] 与 [[图]]", vec!["图".into(), "图".into()]),
            // 200 是上限,201 超。
            (
                Box::leak(format!("[[{long_ok}]]").into_boxed_str()),
                vec![long_ok.clone()],
            ),
            (
                Box::leak(format!("[[{long_over}]]").into_boxed_str()),
                vec![],
            ),
            /* 这一条盯的是"失败后退**一格**"本身:从第一个 `[[` 起算 body 是
            `[` + 200 个 x,201 超限;退一格之后正好 200,于是仍然匹配得上。退两格
            就整条漏掉。正则引擎的 lastIndex 就是加一,这里的重试步长必须一致。 */
            (
                Box::leak(format!("[[[{long_ok}]]").into_boxed_str()),
                vec![long_ok.clone()],
            ),
            // JS 正则的量词数 UTF-16 code unit:100 个星平面字符正好 200。
            (
                Box::leak(format!("[[{emoji_ok}]]").into_boxed_str()),
                vec![emoji_ok.clone()],
            ),
            (
                Box::leak(format!("[[{emoji_over}]]").into_boxed_str()),
                vec![],
            ),
        ]
    }

    #[test]
    fn matches_the_frontend_regex_on_the_golden_cases() {
        for (line, expected) in golden() {
            assert_eq!(raws(line), expected, "line: {line:?}");
        }
    }

    #[test]
    fn detects_embed_syntax() {
        let hits = scan_line("![[图]] 与 [[图]]");
        assert_eq!(hits.len(), 2);
        assert!(hits[0].embed, "`![[..]]` 是嵌入");
        assert!(!hits[1].embed);
    }

    #[test]
    fn line_numbers_are_one_based_and_survive_crlf() {
        let content = "开头\r\n第二行 [[周报]]\r\n结尾\r\n";
        let links = scan_links(content, 100);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].line, 2);
        // `lines()` 会把 `\r` 去掉,预览里不该留下它。
        assert_eq!(links[0].preview, "第二行 [[周报]]");
    }

    #[test]
    fn per_file_cap_is_respected() {
        let content = "[[a]]\n".repeat(MAX_LINKS_PER_FILE + 50);
        assert_eq!(
            scan_links(&content, MAX_TOTAL_LINKS).len(),
            MAX_LINKS_PER_FILE
        );
        // 全库剩余额度更小时按它截。
        assert_eq!(scan_links(&content, 7).len(), 7);
    }
}
