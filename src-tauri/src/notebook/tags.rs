//! 全库标签扫描:每篇笔记正文里出现过的 `#标签`,连同行号和那一行的预览。
//!
//! **只认正文里的行内 `#tag`。** frontmatter 里的 `tags:` 数组是另一套机制(要动
//! YAML,和标签云的"改一个词"不是一回事),留给 frontmatter 面板那一步。
//!
//! ## 索引和重命名必须用同一个词法器
//!
//! Markio 这里有个内在不一致:索引用字符扫(`##heading` 会被当成标签 `heading`
//! 索引),重命名用另一条正则(`(?<![#\w])` 把它排除)。于是有些标签数得出来却改
//! 不动 —— 用户看到"3 处引用",改完只动了 2 处,而剩下那 1 处在哪没人说得清。
//!
//! 这里只有 `tag_hits` 一个词法器,它同时给出标签文本和**字节偏移**;索引用前者,
//! 重命名用后者原地改写。数得出来的就一定改得动,这是同一份代码保证的,不是两处
//! 各自小心维护出来的。
//!
//! ## 不算标签的地方
//!
//! - frontmatter 块(YAML 注释就是 `#`)
//! - 围栏代码块(`#include`、`#!/bin/sh`、`#define` 满地都是)
//! - 行内代码(`` `#fff` `` 是颜色值,不是标签)
//! - `##heading`:`#` 前面是 `#`,不是空白
//!
//! ## `#` 前面只允许行首或空白
//!
//! 于是 `[文字](#锚点)` 里的 `#锚点`、`a#b`、`&#39;` 都不算。代价是 `(#标签)` 这种
//! 写法也不算 —— 反过来放宽到"允许前面是标点"就会把每一个 markdown 锚点链接都收
//! 进标签云,那是每篇长笔记都会有的噪声,比漏掉括号里的标签糟得多。

use std::path::Path;

use super::vault_walk::{
    fence_marker, frontmatter_lines, line_spans, preview_line, walk_notes, WalkNext,
};

/// 单篇笔记记多少个标签。超出的丢掉:一篇里 1000 个标签已经不是人写出来的。
const MAX_TAGS_PER_FILE: usize = 1_000;

/// 全库标签上限。文件数 / 深度 / 单文件大小的上限在 `vault_walk` 那一层。
const MAX_TOTAL_TAGS: usize = 20_000;

/// 标签正文的字符上限。和 Markio 一致。
const MAX_TAG_CHARS: usize = 64;

/// 一个标签出现。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteTagRef {
    /// 标签文本,**不含** `#`,保持原始大小写。
    pub raw: String,
    /// 1-based 行号。点一条引用要跳到这里。
    pub line: u32,
    /// 那一行的文本(两端 trim,超长截断)。
    pub preview: String,
}

/// 一篇笔记里的全部标签出现。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteTagSource {
    /// 绝对路径,与笔记列表里的 `id` 同一个值。
    pub path: String,
    pub tags: Vec<NoteTagRef>,
}

/// 扫一遍 vault,返回每篇**含标签**的笔记及其标签出现。
pub(crate) fn scan_vault_tags(root: &Path) -> Result<Vec<NoteTagSource>, String> {
    let mut out = Vec::new();
    let mut total = 0usize;
    walk_notes(root, &mut |path, content| {
        // 整篇没有 `#` 就不用逐行了。
        if !content.contains('#') {
            return WalkNext::Continue;
        }
        let tags = scan_tags(content, MAX_TOTAL_TAGS - total);
        if tags.is_empty() {
            return WalkNext::Continue;
        }
        total += tags.len();
        out.push(NoteTagSource {
            path: path.to_string_lossy().to_string(),
            tags,
        });
        if total >= MAX_TOTAL_TAGS {
            return WalkNext::Stop;
        }
        WalkNext::Continue
    })?;
    // 按路径排序,和 `vault_index` 同一个理由:两次扫描的结果顺序要一致,否则标签
    // 详情里那串引用的排列会随文件系统遍历顺序漂移。
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 一篇正文里的标签,带行号和预览。
fn scan_tags(content: &str, remaining: usize) -> Vec<NoteTagRef> {
    let cap = remaining.min(MAX_TAGS_PER_FILE);
    let mut out = Vec::new();
    if cap == 0 {
        return out;
    }
    for hit in tag_hits(content) {
        if out.len() >= cap {
            break;
        }
        out.push(NoteTagRef {
            raw: hit.raw,
            line: hit.line,
            preview: preview_line(hit.line_text),
        });
    }
    out
}

/// 一个标签出现的全部信息。`start`/`end` 是**整篇正文里**的字节区间,含 `#`。
///
/// 偏移这两个字段目前只被测试读到 —— 索引那一路只要 `raw`/`line`/`line_text`。它们
/// 在这里是因为**重命名必须和索引共用同一个词法器**:Markio 的索引用字符扫、重命名
/// 用另一条正则,结果是有些标签数得出来却改不动。所以偏移跟着 hit 一起出,而不是等
/// 重命名那一步再写第二个扫描器。测试已经钉住了它们(含 CRLF 与摘掉末尾 `/` 的情形)。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct TagHit<'a> {
    pub raw: String,
    pub line: u32,
    pub line_text: &'a str,
    /// `#` 那个字节的位置。
    pub start: usize,
    /// 标签末尾之后一个字节的位置。`content[start..end]` 就是 `#tag` 整体。
    pub end: usize,
}

/// 扫全篇的标签出现。索引和跨文件重命名共用这一个入口。
pub(crate) fn tag_hits(content: &str) -> Vec<TagHit<'_>> {
    let lines: Vec<(usize, &str)> = line_spans(content).collect();
    let skip = frontmatter_lines(&lines);
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (index, (base, line)) in lines.iter().copied().enumerate().skip(skip) {
        let line_no = (index + 1) as u32;
        let marker = fence_marker(line);
        if let Some((open_char, open_len)) = fence {
            // 闭合围栏的标记必须不短于开启的那个,否则 ```` 里的 ``` 会提前结束。
            if let Some((ch, len)) = marker {
                if ch == open_char && len >= open_len {
                    fence = None;
                }
            }
            continue;
        }
        if let Some((ch, len)) = marker {
            fence = Some((ch, len));
            continue;
        }
        let spans = code_spans(line);
        for (offset, raw, end) in line_tags(line) {
            if spans
                .iter()
                .any(|(from, to)| offset >= *from && offset < *to)
            {
                continue;
            }
            out.push(TagHit {
                raw,
                line: line_no,
                line_text: line,
                start: base + offset,
                end: base + end,
            });
        }
    }
    out
}

/// 一行里的行内代码区间(字节),含两侧的反引号。
///
/// 闭合的反引号数量必须和开启的一样多(CommonMark 规则),这样 `` `a` `` 里的单个
/// 反引号不会提前收尾。没闭合的反引号是字面量,后面的内容照常算标签。
fn code_spans(line: &str) -> Vec<(usize, usize)> {
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

/// 一行里的标签。返回 (`#` 的行内偏移, 标签文本, 末尾之后的行内偏移)。
fn line_tags(line: &str) -> Vec<(usize, String, usize)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }
        // `#` 前面只允许行首或空白。`#` 是 ASCII,所以看前一个字节不会切进多字节
        // 字符中间;但要判空白得看**字符**,于是取前面那一段的最后一个字符。
        let ok_prefix = i == 0
            || line[..i]
                .chars()
                .next_back()
                .map(char::is_whitespace)
                .unwrap_or(true);
        if !ok_prefix {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        let mut chars = 0usize;
        for ch in line[end..].chars() {
            if !is_tag_char(ch) {
                break;
            }
            end += ch.len_utf8();
            chars += 1;
            if chars >= MAX_TAG_CHARS {
                break;
            }
        }
        let body = &line[i + 1..end];
        match normalize_tag(body) {
            // 归一化会摘掉末尾的 `/` 和 `-`,`end` 要跟着回退,否则重命名会把那几个
            // 字符一起吃掉(`#work/` 变成 `#新名` 而不是 `#新名/`)。
            Some(kept) => {
                let kept_end = i + 1 + kept.len();
                out.push((i, kept.to_string(), kept_end));
                i = end.max(i + 1);
            }
            None => {
                i = end.max(i + 1);
            }
        }
    }
    out
}

/// 标签正文允许的字符:字母 / 数字(含 CJK,它们是 alphanumeric)、`_`、`-`、`/`。
fn is_tag_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '/'
}

/// 归一化一个标签正文,`None` 表示这不是标签。
///
/// - 末尾的 `/` 和 `-` 摘掉:`#work/` 和 `#work` 是同一个标签,而 `#work-` 多半是
///   `#work - 说明` 里少了个空格。
/// - 纯数字不算:`#1`、`#42` 在 markdown 里几乎总是 issue / 条目编号。Obsidian 也
///   是这么定的。
fn normalize_tag(body: &str) -> Option<&str> {
    let kept = body.trim_end_matches(['/', '-']);
    if kept.is_empty() {
        return None;
    }
    if kept.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(kept)
}

/// 聚合与匹配用的 key:去掉前导 `#`、两端空白、末尾的 `/`-`,折小写。
///
/// 必须和前端 `noteTags.ts` 的 `normalizeTag` 逐条一致 —— 面板按前端那份聚合出"这一行
/// 有 3 处",重命名按这份匹配。两边漂移的表现就是那个数字对不上,而这正是本模块要
/// 根除的那一类缺陷。两边各有一份是因为聚合在前端、改写在后端,共用一份得来回过 IPC。
pub(crate) fn normalize_key(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('#')
        .trim_end_matches(['/', '-'])
        .to_lowercase()
}

/// 校验一个用户输入的新标签名,返回**要写进文件的字面文本**(不含 `#`)。
///
/// 校验的口径就是 `tag_hits` 自己:把 `#候选` 喂进扫描器,扫出来的那一个必须原样等于
/// 候选。这样"能不能写"和"写完扫不扫得出来"是同一个问题的两面 —— 换成手写一串
/// `is_tag_char` 判断的话,任何一处和扫描器不一致都会写出一个自己都找不到的标签,而
/// 那时文件已经改完了。
pub(crate) fn validate_tag(input: &str) -> Result<String, String> {
    let candidate = input.trim().trim_start_matches('#').trim();
    if candidate.is_empty() {
        return Err("The new tag is empty".to_string());
    }
    if candidate.chars().count() > MAX_TAG_CHARS {
        return Err(format!(
            "The new tag is too long (limit {MAX_TAG_CHARS} characters)"
        ));
    }
    let probe = format!("#{candidate}");
    match tag_hits(&probe).as_slice() {
        [hit] if hit.raw == candidate && hit.start == 0 && hit.end == probe.len() => {
            Ok(candidate.to_string())
        }
        // 扫不出来或者扫出来的和输入不一样:含空格、含 `#`、纯数字、末尾是 `/`-`……
        // 逐种情形分别报错只会让文案和扫描器各自漂移,这里报同一句。
        _ => Err(format!(
            "\"{candidate}\" is not a valid tag: use letters, digits, `_`, `-` or `/`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raws(content: &str) -> Vec<String> {
        tag_hits(content).into_iter().map(|hit| hit.raw).collect()
    }

    #[test]
    fn picks_up_inline_tags_with_cjk_and_nesting() {
        assert_eq!(
            raws("#周报 和 #work/urgent 还有 #a_b-c"),
            vec!["周报", "work/urgent", "a_b-c"]
        );
    }

    #[test]
    fn hash_must_follow_start_of_line_or_whitespace() {
        // `##heading` 前面是 `#`;markdown 锚点前面是 `(`;`a#b` 前面是词字符。
        assert!(raws("## 二级标题").is_empty());
        assert!(raws("##紧挨着的标题").is_empty());
        assert!(raws("[文字](#锚点)").is_empty());
        assert!(raws("a#b").is_empty());
        assert!(raws("&#39;").is_empty());
        // 行首和空白之后都算。
        assert_eq!(raws("#行首"), vec!["行首"]);
        assert_eq!(raws("文字 #空白之后"), vec!["空白之后"]);
    }

    #[test]
    fn skips_frontmatter_but_not_a_bare_divider() {
        // frontmatter 里的 `tags:` 是另一套机制,YAML 注释也是 `#`。
        assert!(raws("---\ntitle: A\n# 注释\ntags: [x]\n---\n").is_empty());
        // 未闭合的 `---` 是分隔线,后面照常算正文。
        assert_eq!(raws("---\n#标签\n"), vec!["标签"]);
        // 闭合之后的正文照常算。
        assert_eq!(raws("---\ntitle: A\n---\n\n#标签\n"), vec!["标签"]);
    }

    #[test]
    fn skips_code_blocks_and_inline_code() {
        // 代码里的 `#` 满地都是,收进标签云就全是噪声。
        assert!(raws("```sh\n#!/bin/sh\n#include <x>\n```\n").is_empty());
        assert!(raws("~~~\n#define X\n~~~\n").is_empty());
        assert!(raws("颜色 `#fff` 不是标签").is_empty());
        // 更长的围栏里的短围栏不提前收尾。
        assert!(raws("````\n```\n#里面\n```\n````\n").is_empty());
        // 围栏关掉之后照常算。
        assert_eq!(raws("```\n#里面\n```\n#外面\n"), vec!["外面"]);
        // 没闭合的反引号是字面量,后面照常算。
        assert_eq!(raws("反引号 ` 之后 #标签"), vec!["标签"]);
    }

    #[test]
    fn rejects_pure_numbers_and_trailing_separators() {
        // `#1` 几乎总是条目编号,不是标签。
        assert!(raws("见 #1 和 #42").is_empty());
        assert!(raws("#/ #- #--").is_empty());
        // 末尾的 `/` 和 `-` 摘掉,而且**不**吃进重命名区间。
        let hits = tag_hits("#work/ 尾巴");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw, "work");
        assert_eq!(&"#work/ 尾巴"[hits[0].start..hits[0].end], "#work");
    }

    #[test]
    fn offsets_point_at_the_hash_and_survive_crlf() {
        let content = "开头\r\n第二行 #标签 尾巴\r\n";
        let hits = tag_hits(content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        // `\r` 不算内容但算偏移,否则重命名会切在错位置。
        assert_eq!(&content[hits[0].start..hits[0].end], "#标签");
        // `lines()` 会把 `\r` 去掉,预览里不该留下它。
        assert_eq!(preview_line(hits[0].line_text), "第二行 #标签 尾巴");
    }

    #[test]
    fn caps_tag_length_at_sixty_four_chars() {
        let long = "x".repeat(70);
        let content = format!("#{long}");
        let hits = tag_hits(&content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw.chars().count(), MAX_TAG_CHARS);
    }

    #[test]
    fn per_file_cap_is_respected() {
        let content = "#a\n".repeat(MAX_TAGS_PER_FILE + 50);
        assert_eq!(scan_tags(&content, MAX_TOTAL_TAGS).len(), MAX_TAGS_PER_FILE);
        // 全库剩余额度更小时按它截。
        assert_eq!(scan_tags(&content, 7).len(), 7);
    }

    #[test]
    fn normalize_key_matches_the_frontend_rules() {
        // 与 `noteTags.ts` 的 `normalizeTag` 逐条对着写:前端聚合出的处数和这里匹配
        // 到的处数必须是同一个数。
        assert_eq!(normalize_key("#Work"), "work");
        assert_eq!(normalize_key("  #work  "), "work");
        assert_eq!(normalize_key("work"), "work");
        assert_eq!(normalize_key("#project/"), "project");
        assert_eq!(normalize_key("#project-"), "project");
        assert_eq!(normalize_key("#a/b/"), "a/b");
        assert_eq!(normalize_key("#Project/Sub"), "project/sub");
        assert_eq!(normalize_key("##work"), "work");
        assert_eq!(normalize_key("#"), "");
        assert_eq!(normalize_key("   "), "");
    }

    #[test]
    fn validate_tag_accepts_what_the_scanner_finds() {
        assert_eq!(validate_tag("work").unwrap(), "work");
        // 前导 `#` 和两端空白是用户在输入框里最容易多打的,收下并摘掉。
        assert_eq!(validate_tag("  #work/deep  ").unwrap(), "work/deep");
        assert_eq!(validate_tag("周报").unwrap(), "周报");
        assert_eq!(validate_tag("a_b-c").unwrap(), "a_b-c");
    }

    #[test]
    fn validate_tag_rejects_what_the_scanner_would_lose() {
        /* 每一条都是"写进去之后自己扫不出来"的情形:文件已经改完,而面板里那个标签
        不存在。校验口径就是扫描器本身,所以这份清单不会和它漂移。 */
        assert!(validate_tag("").is_err());
        assert!(validate_tag("#").is_err());
        assert!(validate_tag("   ").is_err());
        // 空格会让扫描器只认前半截。
        assert!(validate_tag("a b").is_err());
        // 纯数字被当成条目编号。
        assert!(validate_tag("42").is_err());
        // 末尾的 `/` 会被摘掉 —— 写进去的和输入的不是一个东西。
        assert!(validate_tag("work/").is_err());
        // 中间的 `#` 会截断。
        assert!(validate_tag("a#b").is_err());
        // 标点不是标签字符。
        assert!(validate_tag("a,b").is_err());
        assert!(validate_tag(&"x".repeat(MAX_TAG_CHARS + 1)).is_err());
        // 正好到上限收下。
        assert!(validate_tag(&"x".repeat(MAX_TAG_CHARS)).is_ok());
    }
}
