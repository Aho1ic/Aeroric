//! 未链接的提及:全库里**写了某篇笔记的名字、却没写成 `[[链接]]`** 的那些地方,
//! 以及把它们一键包成链接。
//!
//! 反链回答"谁链到了我",这一档回答"谁提到了我却没链"。后者是双链笔记里补全网络的
//! 主要入口 —— 人写字的时候不会记得加括号。
//!
//! ## 为什么在 Rust 侧扫
//!
//! 和标签 / 任务同一个理由:笔记正文是**按需读**的,前端手上只有已经打开过的那几篇。
//! 在内存里聚合出来的"未链接提及"会随用户点开过哪些笔记而变多,那不是一份能信的清单。
//!
//! ## 判定必须复用既有词法器
//!
//! 「这处字样算不算未链接」等价于「它是不是落在某个不该算的区间里」,而那些区间的定义
//! 已经各有归属:frontmatter 与围栏在 `vault_walk`,行内代码在 `vault_walk::inline_code_spans`,
//! 已有链接在 `links::scan_line`。这里一个都不重写 —— 重写的代价不是重复代码,是分叉:
//! 提及列表说"未链接",渲染出来却已经是链接,于是一键链接把 `[[计划]]` 包成
//! `[[[[计划]]]]`。
//!
//! ## Markio 那份的五个坑,这里逐条不搬
//!
//! Markio 的 `find_mentions` / `link_mention_in_file` 是 grep 式的:
//!
//! 1. **没有 CJK 边界处理**。它的词边界只对 ASCII needle 生效(`let blocked = if
//!    ascii_needle { … } else { false }`),于是笔记《计划》会把「原计划表」改成
//!    「原[[计划]]表」—— 一次不可见的正文损坏。这里把中日韩邻字判成
//!    [`MentionConfidence::Ambiguous`],要用户逐条确认,**不进批量**。
//! 2. **不跳 frontmatter / 围栏 / 行内代码**。于是 YAML 的 `title:` 那行、示例代码里的
//!    标识符都算提及,包成链接直接把 YAML 和代码改坏。
//! 3. **已链接判定太弱**。它只按"这个文件里 grep 到 `[[stem`"整篇排除,于是同一篇里
//!    链了一处、另有五处没链时,那五处一条都报不出来。这里按**区间**判,粒度是"这一处"。
//! 4. **`stem.len() >= 2` 数的是字节**。一个汉字 3 字节,于是单字笔记《书》也参与匹配,
//!    在中文正文里等于全文高亮。这里按**字符**数,并且同样是 2。
//! 5. **"全部链接"每个文件只包一处**,但报告里按文件数报成"已链接 N 处"。用户看到
//!    "已链接 12 处",实际改了 12 个文件里的 12 处、剩下的几十处还在。这里由前端把
//!    要改的每一处显式传下来,报告里的数就是**处数**。

use std::path::Path;

use super::fs_ops::read_note;
use super::links::scan_line;
use super::state::NotebookState;
use super::tag_rename::write_rewritten;
use super::vault_walk::{
    fence_marker, frontmatter_lines, inline_code_spans, line_spans, preview_line, walk_notes,
    WalkNext,
};

/// 候选名字的字符下限。1 个字的名字在正文里等于全文高亮。
///
/// 数**字符**不是字节:Markio 那份 `stem.len() >= 2` 数的是字节,一个汉字就 3 字节,
/// 于是单字笔记照样参与匹配。
const MIN_NEEDLE_CHARS: usize = 2;

/// 候选名字的字符上限。和标签一致 —— 超长的"名字"多半是把一整段当成了标题。
const MAX_NEEDLE_CHARS: usize = 64;

/// 一次最多接受几个候选名字。一篇笔记的名字来源只有 frontmatter 标题和文件名,
/// 上限给足冗余(别名那一步会加,见 `noteLinks.ts`)。
const MAX_NEEDLES: usize = 8;

/// 单篇笔记最多报多少处提及。
const MAX_MENTIONS_PER_FILE: usize = 200;

/// 全库最多报多少处。超出就停 —— 列表本来也不是用来滚一万条的。
const MAX_TOTAL_MENTIONS: usize = 5_000;

/// 一次"链接"操作最多碰多少篇。
///
/// 和 `tag_rename` 的上限同一个理由:**可回滚性**。每篇留一条版本快照,几千篇的话
/// 用户想撤回得一篇篇翻历史。
const MAX_FILES_PER_LINK: usize = 500;

/// 这一处提及有多可信。
///
/// 存在的理由就是 CJK:中文没有词边界,「计划」出现在「原计划表」里时,从字符层面
/// 看不出该不该包。判断不了的事情不该猜 —— 猜错的代价是用户正文里多出一条谁都没写过
/// 的链接,而且他不会立刻发现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MentionConfidence {
    /// 两侧都是非文字字符(空白、标点、行首行尾)。批量链接只动这一类。
    Confident,
    /// 至少一侧紧贴着别的文字(典型是中日韩邻字)。要用户逐条确认。
    Ambiguous,
}

/// 一处未链接的提及。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionHit {
    /// 命中的候选名字(前端传下来的那一个,原样回传)。给 UI 显示"匹配的是哪个名字"。
    pub needle: String,
    /// 命中处的**原文**。
    ///
    /// 和 `needle` 分成两个字段是必须的:匹配大小写不敏感,所以正文里的 `PLAN` 会命中
    /// 候选名 `Plan`,而"链接"那一步校验的是「这个区间里的原文还是不是当时那段」。
    /// 拿 `needle` 去校验的话,每一个大小写不同的命中都会被报成 `vanished` —— 用户
    /// 看到列表里有它、点了却说"已经不在了"。
    pub text: String,
    /// 1-based 行号,按**整个 `.md` 文件**数 —— 和标签 / 反链 / 任务同一个坐标系。
    pub line: u32,
    /// 这处字样在**整篇内容里**的字节区间。链接那一步按它原地包。
    pub start: usize,
    pub end: usize,
    /// 那一行的预览(两端 trim,超长截断)。
    pub preview: String,
    pub confidence: MentionConfidence,
}

/// 一篇笔记里的全部未链接提及。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionSource {
    /// 绝对路径,与笔记列表里的 `id` 同一个值。
    pub path: String,
    pub mentions: Vec<MentionHit>,
}

/// 扫一遍 vault,找出提到了 `needles` 里任一名字、却没写成链接的地方。
///
/// `self_path` 是这些名字所属的那篇笔记 —— 它自己整篇跳过。理由不只是"自己提自己没
/// 意义":正文里写自己的标题是很常见的(H1、摘要),把它们包成指向自己的链接是一条
/// 死循环链接,而且反链面板会立刻多出一条自引用。
pub(crate) fn scan_vault_mentions(
    root: &Path,
    self_path: &Path,
    needles: &[String],
) -> Result<Vec<MentionSource>, String> {
    let needles = normalize_needles(needles);
    if needles.is_empty() {
        return Ok(Vec::new());
    }
    // 归一化后的自身路径。比对失败(文件刚被删)时退回原值 —— 那样最多是把一篇不存在
    // 的笔记算进扫描范围,而它读不出来会被 `walk_notes` 静默跳过。
    let self_canon = std::fs::canonicalize(self_path).unwrap_or_else(|_| self_path.to_path_buf());
    let mut out = Vec::new();
    let mut total = 0usize;
    walk_notes(root, &mut |path, content| {
        if path == self_canon {
            return WalkNext::Continue;
        }
        let mentions = scan_mentions(content, &needles, MAX_TOTAL_MENTIONS - total);
        if mentions.is_empty() {
            return WalkNext::Continue;
        }
        total += mentions.len();
        out.push(MentionSource {
            path: path.to_string_lossy().to_string(),
            mentions,
        });
        if total >= MAX_TOTAL_MENTIONS {
            return WalkNext::Stop;
        }
        WalkNext::Continue
    })?;
    // 按路径排序,和 `vault_index` 同一个理由:两次扫描的顺序要一致,否则列表里那串
    // 条目的排列会随文件系统遍历顺序漂移。
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 收下前端传来的候选名字:去空白、去重(大小写不敏感)、过长短限制、砍到上限。
///
/// 大小写不敏感去重是因为标题和文件名 stem 经常只差大小写(`Plan.md` + `title: plan`),
/// 两个都留会让同一处提及报两条。
fn normalize_needles(input: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in input {
        let needle = raw.trim();
        let chars = needle.chars().count();
        if !(MIN_NEEDLE_CHARS..=MAX_NEEDLE_CHARS).contains(&chars) {
            continue;
        }
        // 含 `[` / `]` 的名字包不出合法链接(`[[a]b]]` 的 body 不许含 `]`),扫出来也
        // 没用 —— 与其报一条点了会写坏文件的条目,不如不报。
        if needle.contains('[') || needle.contains(']') || needle.contains('\n') {
            continue;
        }
        let lower = needle.to_lowercase();
        if out.iter().any(|kept| kept.to_lowercase() == lower) {
            continue;
        }
        out.push(needle.to_string());
        if out.len() >= MAX_NEEDLES {
            break;
        }
    }
    out
}

/// 一篇正文里的未链接提及。
///
/// 逐行走,跳掉不算正文的整行(frontmatter、围栏内、ATX 标题),再在剩下的行里排除
/// 不算提及的**区间**(行内代码、已有链接、markdown 链接、裸 URL)。
fn scan_mentions(content: &str, needles: &[String], remaining: usize) -> Vec<MentionHit> {
    let cap = remaining.min(MAX_MENTIONS_PER_FILE);
    let mut out = Vec::new();
    if cap == 0 {
        return out;
    }
    let lines: Vec<(usize, &str)> = line_spans(content).collect();
    let skip = frontmatter_lines(&lines);
    let mut fence: Option<(char, usize)> = None;
    for (index, (base, line)) in lines.iter().copied().enumerate().skip(skip) {
        if out.len() >= cap {
            break;
        }
        let marker = fence_marker(line);
        if let Some((open_char, open_len)) = fence {
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
        // ATX 标题整行跳过。包一个标题会同时改掉两样东西:大纲里那一条的文字,和
        // `[[笔记#小节]]` 这种小节链接的锚点 —— 后者是**静默失效**,别处指向这个小节
        // 的链接会突然点不动。为了自动补一条链接而弄坏已有的链接不划算。
        if is_atx_heading(line) {
            continue;
        }
        let skip_spans = protected_spans(line);
        let mut hits = line_mentions(line, needles, &skip_spans);
        // 同一行里按位置排,让列表的顺序和肉眼从左到右读的顺序一致。
        hits.sort_by_key(|hit| hit.0);
        for (offset, needle, end, confidence) in hits {
            if out.len() >= cap {
                break;
            }
            out.push(MentionHit {
                needle,
                text: line[offset..end].to_string(),
                line: (index + 1) as u32,
                start: base + offset,
                end: base + end,
                preview: preview_line(line),
                confidence,
            });
        }
    }
    out
}

/// 这一行是不是 ATX 标题(`#` 到 `######` 后跟空白或行尾)。
///
/// setext 标题(下一行是 `===`)不判:那要看下一行,而漏判它的代价只是"标题行里的提及
/// 也会被报出来",比错判一行正文小。
fn is_atx_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return false;
    }
    matches!(
        trimmed[hashes..].chars().next(),
        None | Some(' ') | Some('\t')
    )
}

/// 一行里**不该算提及**的字节区间。
///
/// 四类,每一类都有具体的坏结果:
///
/// - 行内代码:`` `计划` `` 是标识符,包成链接直接改坏代码。
/// - 已有 `[[链接]]`:重复包成 `[[[[计划]]]]`,渲染出来是字面括号。
/// - markdown 链接 `[文字](目标)`:包在 `[]` 里会得到 `[[[计划]]](url)`,链接语法就废了;
///   包在 `(url)` 里会把 URL 改成一个不存在的路径。
/// - 裸 URL:`https://example.com/计划/index` 里的字样不是提及,包了链接就打不开。
fn protected_spans(line: &str) -> Vec<(usize, usize)> {
    let mut spans = inline_code_spans(line);
    // 已有链接的区间来自**定义链接的那个词法器**,不是这里重写一遍找 `[[`/`]]`。
    for hit in scan_line(line) {
        spans.push((hit.start, hit.end));
    }
    spans.extend(markdown_link_spans(line));
    spans.extend(url_spans(line));
    spans
}

/// markdown 行内链接 / 图片的区间:`[文字](目标)` 与 `![替换文字](目标)` 整体。
///
/// 只认**紧挨着**的 `](`(CommonMark 不允许中间有空白)。`[[wiki]]` 由 `scan_line`
/// 负责,这里遇到 `[[` 直接往后挪一格,免得把 `[[a]](b)` 这种混写切错。
fn markdown_link_spans(line: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let text_start = i;
        // 文字部分一路吃到第一个 `]`。嵌套的 `[]` 不处理 —— 那种写法在 CommonMark 里
        // 本身就要转义。
        let Some(close) = line[i + 1..].find(']').map(|at| i + 1 + at) else {
            break;
        };
        if !line[close..].starts_with("](") {
            i += 1;
            continue;
        }
        // 目标部分吃到配对的 `)`。URL 里可以有括号(维基百科那种),按深度配。
        let mut depth = 0usize;
        let mut j = close + 1;
        let mut end = None;
        while j < bytes.len() {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j + 1);
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        match end {
            Some(end) => {
                // 图片的 `!` 也算进去,和 `scan_line` 对 `![[..]]` 的处理一致。
                let start = if text_start > 0 && bytes[text_start - 1] == b'!' {
                    text_start - 1
                } else {
                    text_start
                };
                spans.push((start, end));
                i = end;
            }
            // 没配对的 `(`:不是链接,从 `[` 之后一格重试。
            None => i = text_start + 1,
        }
    }
    spans
}

/// 一行里的裸 URL 区间:`http://` / `https://` 开头,一路吃到空白或 `)` `>`。
fn url_spans(line: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while cursor < line.len() {
        let rest = &line[cursor..];
        let Some(at) = rest.find("http") else { break };
        let start = cursor + at;
        let tail = &line[start..];
        if !tail.starts_with("http://") && !tail.starts_with("https://") {
            cursor = start + 4;
            continue;
        }
        let mut end = start;
        for ch in tail.chars() {
            if ch.is_whitespace() || ch == ')' || ch == '>' || ch == '"' {
                break;
            }
            end += ch.len_utf8();
        }
        spans.push((start, end));
        cursor = end.max(start + 1);
    }
    spans
}

/// 一条边界的判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    /// 邻位是空白 / 标点 / 行首行尾 —— 干净的词边界。
    Clean,
    /// 邻位是**别的文字**,但两边不同文字体系(`Plan表`)或都是表意文字(`原计划`)。
    /// 判断不了,交给用户。
    Ambiguous,
    /// 邻位把它并成了同一个词(`Planning` 里的 `Plan`)。这不是提及,直接丢。
    Blocked,
}

/// 判一条边界。`outer` 是紧贴 needle 外侧的那个字符,`inner` 是 needle 靠这一侧的字符。
///
/// 规则只有一条,但要分两种文字看:
///
/// - 两边都是**有词边界的文字**(拉丁字母、数字、下划线):连在一起就是一个词,
///   `Planning` 里的 `Plan` 不是对《Plan》的提及。判 [`Edge::Blocked`]。
/// - 只要有一边是**表意文字**(中日韩):没有词边界可言。`原计划` 里的 `计划` 可能是
///   提及也可能不是,`Plan表` 同理。判 [`Edge::Ambiguous`],不猜。
///
/// Markio 那份只做了第一种(而且只在整个 needle 都是 ASCII 时才做),第二种直接放行 ——
/// 于是《计划》把「原计划表」改成「原[[计划]]表」。
fn classify_edge(outer: Option<char>, inner: char) -> Edge {
    let Some(outer) = outer else {
        return Edge::Clean;
    };
    if !is_word_char(outer) {
        return Edge::Clean;
    }
    // 到这里两边都是文字。只要任一边没有词边界概念,就判不了。
    if is_ideographic(outer) || is_ideographic(inner) {
        return Edge::Ambiguous;
    }
    Edge::Blocked
}

/// 算不算"文字"(连起来可能构成一个词)。
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// 是不是**不分词**的表意文字:中日韩统一表意文字(含扩展 A)、兼容表意文字、
/// 日文假名、韩文音节。
///
/// 韩文实际上是用空格分词的,把它算进来会让「계획」两侧有韩文时报成 Ambiguous 而不是
/// Blocked —— 方向是偏保守(多问一次而不是擅自改),这个偏差可以接受;反过来把它当成
/// 有词边界的文字则会**丢掉**真实提及。
fn is_ideographic(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF      // 平假名 / 片假名
        | 0x3400..=0x4DBF    // 扩展 A
        | 0x4E00..=0x9FFF    // 基本区
        | 0xAC00..=0xD7AF    // 韩文音节
        | 0xF900..=0xFAFF    // 兼容表意文字
        | 0x20000..=0x2FA1F  // 扩展 B 及以后
    )
}

/// 一行里的提及。返回 (行内起点, 命中的候选名, 行内终点, 可信度)。
///
/// 匹配大小写不敏感 —— 链接解析本身就是(`noteLinks.ts` 的 `normalizeLinkTarget` 折
/// 小写),所以 `plan` 包成 `[[plan]]` 照样指向《Plan》。**不**把整行转小写再找:
/// 有些字符转小写后字节长度会变(`İ` 是 2 字节,小写是 3 字节),那样算出来的偏移会
/// 错位,而错位的偏移意味着包链接时切在字符中间。Markio 那份正是靠 `line_lower.len()
/// != line_str.len()` 检测这种情况,然后**整行放弃**。这里逐字符原地比,不需要放弃。
fn line_mentions(
    line: &str,
    needles: &[String],
    skip: &[(usize, usize)],
) -> Vec<(usize, String, usize, MentionConfidence)> {
    let mut out: Vec<(usize, String, usize, MentionConfidence)> = Vec::new();
    for needle in needles {
        let Some(first) = needle.chars().next() else {
            continue;
        };
        let Some(last) = needle.chars().next_back() else {
            continue;
        };
        let mut cursor = 0usize;
        while cursor < line.len() {
            let Some(offset) = find_ci(&line[cursor..], needle) else {
                break;
            };
            let start = cursor + offset;
            // 命中处的实际字节长度要按原文数:大小写不敏感比对是逐字符走的,而同一个
            // 字符的大小写形式字节数可能不同。
            let Some(end) = ci_match_end(line, start, needle) else {
                cursor = start + 1;
                continue;
            };
            // 下一轮从这次命中之后开始,避免重叠命中。
            cursor = end.max(start + 1);
            if skip.iter().any(|(from, to)| start < *to && end > *from) {
                continue;
            }
            let before = line[..start].chars().next_back();
            let after = line[end..].chars().next();
            let left = classify_edge(before, first);
            let right = classify_edge(after, last);
            if left == Edge::Blocked || right == Edge::Blocked {
                continue;
            }
            let confidence = if left == Edge::Clean && right == Edge::Clean {
                MentionConfidence::Confident
            } else {
                MentionConfidence::Ambiguous
            };
            // 同一处被两个候选名命中(标题和 stem 只差大小写之外的情形,比如
            // stem 是标题的前缀)时只留一条:用户看到的是"这一行有一处提及"。
            if out
                .iter()
                .any(|(kept_start, _, kept_end, _)| start < *kept_end && end > *kept_start)
            {
                continue;
            }
            out.push((start, needle.clone(), end, confidence));
        }
    }
    out
}

/// 在 `haystack` 里找 `needle` 的首次出现(大小写不敏感),返回字节偏移。
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .char_indices()
        .find(|(offset, _)| ci_match_end(haystack, *offset, needle).is_some())
        .map(|(offset, _)| offset)
}

/// `haystack[at..]` 是否以 `needle` 开头(大小写不敏感)。是则返回**原文里**的结束偏移。
///
/// 逐字符比小写形式,但**偏移始终按 `haystack` 的原始字节推进** —— 这就是"不把整行转
/// 小写"能保住偏移精度的地方。Unicode 里一个字符的小写可能展开成多个(`İ` → `i̇`),
/// 于是转换后的字符串长度和原文对不上;Markio 那份正是靠 `line_lower.len() !=
/// line_str.len()` 检测这种行,然后**整行放弃**(那一行里的提及一处都链不了)。
///
/// needle 在某个原始字符的展开**中间**用完时不算匹配:那意味着终点切在字符内部,
/// 包链接会写出坏 UTF-8。
fn ci_match_end(haystack: &str, at: usize, needle: &str) -> Option<usize> {
    if !haystack.is_char_boundary(at) {
        return None;
    }
    let mut want = needle.chars().flat_map(char::to_lowercase).peekable();
    let mut cursor = at;
    for ch in haystack[at..].chars() {
        if want.peek().is_none() {
            break;
        }
        for lower in ch.to_lowercase() {
            if want.next() != Some(lower) {
                return None;
            }
        }
        cursor += ch.len_utf8();
    }
    // needle 还有剩:haystack 先到头了。
    if want.peek().is_some() {
        return None;
    }
    Some(cursor)
}

// ── 一键链接 ───────────────────────────────────────────────────────────────

/// 前端要包的一处提及。
///
/// 为什么不是"把 needle 传下来、后端自己再扫一遍全包掉":那样用户点"全部链接"时,
/// 后端会连**列表里没显示过的**提及一起改 —— 扫描和点击之间有时间差,期间新写的段落
/// 会被静默包上链接。这里只改用户看见过的那几处,列表就是操作的完整清单。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionTarget {
    pub path: String,
    /// 扫描时给出的字节区间。
    pub start: usize,
    pub end: usize,
    /// 扫描时那一处的**原文**。重读之后必须还是它,否则算 `Vanished`。
    pub text: String,
}

/// 一篇里改了几处。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionLinkChange {
    pub path: String,
    pub count: usize,
}

/// 一处没链上的提及,连同**为什么**。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionLinkSkip {
    pub path: String,
    pub start: usize,
    pub reason: MentionSkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MentionSkipReason {
    /// 重读之后这个位置上已经不是那段文字了 —— 中间被别人改过。
    Vanished,
    /// 这个位置已经在一对 `[[]]` 里了(别的操作先包过,或者用户手工包了)。
    AlreadyLinked,
    /// 超过一次操作的文件数上限。
    TooManyFiles,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionLinkFailure {
    pub path: String,
    pub message: String,
}

/// 一次"链接提及"的完整报告。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionLinkReport {
    pub changed: Vec<MentionLinkChange>,
    pub skipped: Vec<MentionLinkSkip>,
    pub failed: Vec<MentionLinkFailure>,
    /// 一共链上了几**处**(不是几个文件)。
    ///
    /// 由后端算好发下去,而不是让前端自己加 —— Markio 那份把文件数报成了处数
    /// ("已链接 12 处"实际是 12 个文件各一处,剩下几十处还在),而那正是"前端自己
    /// 挑一个数来报"最容易犯的错。这里给它一个现成的、语义唯一的数。
    pub linked: usize,
}

/// 把 `targets` 里的每一处包成 `[[..]]`。
///
/// 按文件分组后逐篇处理:每篇**重读一次**,在新内容上逐处校验(位置上还是那段文字吗、
/// 是不是已经在链接里了),然后从后往前替换 —— 从后往前是因为前面的替换会让后面的偏移
/// 全部右移,而从后往前改时尚未处理的偏移都还没被动过。
pub(crate) fn link_mentions(
    state: &NotebookState,
    root: &Path,
    targets: &[MentionTarget],
) -> Result<MentionLinkReport, String> {
    let mut report = MentionLinkReport::default();
    if targets.is_empty() {
        return Ok(report);
    }

    // 按文件分组,顺带过一遍 allowlist:路径来自前端,必须逐个校验落在已注册 vault 内,
    // 而且必须落在**这一次操作的那个 vault** 里 —— 否则一次请求就能跨 vault 写。
    let mut groups: Vec<(String, Vec<&MentionTarget>)> = Vec::new();
    for target in targets {
        let resolved = state.resolve_in_vaults(&target.path, false)?;
        if !resolved.starts_with(root) {
            return Err(format!("{} is outside the vault being edited", target.path));
        }
        let path = resolved.to_string_lossy().to_string();
        match groups.iter_mut().find(|(kept, _)| *kept == path) {
            Some((_, list)) => list.push(target),
            None => groups.push((path, vec![target])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));

    let (batch, overflow) = groups.split_at(groups.len().min(MAX_FILES_PER_LINK));
    for (path, list) in overflow {
        for target in list {
            report.skipped.push(MentionLinkSkip {
                path: path.clone(),
                start: target.start,
                reason: MentionSkipReason::TooManyFiles,
            });
        }
    }

    for (path, list) in batch {
        match link_one(state, path, list) {
            Ok((count, skipped)) => {
                if count > 0 {
                    report.changed.push(MentionLinkChange {
                        path: path.clone(),
                        count,
                    });
                }
                report.skipped.extend(skipped);
            }
            // 一篇失败不中断:用户要的是一张完整的账,不是"到这里为止"。
            Err(message) => report.failed.push(MentionLinkFailure {
                path: path.clone(),
                message,
            }),
        }
    }

    report.changed.sort_by(|a, b| a.path.cmp(&b.path));
    report
        .skipped
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.start.cmp(&b.start)));
    report.failed.sort_by(|a, b| a.path.cmp(&b.path));
    report.linked = report.changed.iter().map(|change| change.count).sum();
    Ok(report)
}

/// 处理一篇:重读、逐处校验、从后往前包、写回。返回 (改了几处, 跳过的那些)。
fn link_one(
    state: &NotebookState,
    path: &str,
    targets: &[&MentionTarget],
) -> Result<(usize, Vec<MentionLinkSkip>), String> {
    let opened = read_note(state, path)?;
    let (next, count, skipped) = plan_wrap(&opened.content, path, targets);
    if count == 0 {
        return Ok((0, skipped));
    }
    write_rewritten(state, path, &next, opened.sig)?;
    Ok((count, skipped))
}

/// 在 `content` 上把 `targets` 逐处包成 `[[..]]`。返回 (新内容, 改了几处, 跳过的那些)。
///
/// 单独成函数是为了能不碰磁盘直接测 —— 这是整个流程里唯一会改用户正文的一步。
fn plan_wrap(
    content: &str,
    path: &str,
    targets: &[&MentionTarget],
) -> (String, usize, Vec<MentionLinkSkip>) {
    let mut skipped = Vec::new();
    // 先按位置**降序**排:从后往前替换,前面那些还没处理的偏移就不会被这次插入顶偏。
    let mut ordered: Vec<&MentionTarget> = targets.to_vec();
    ordered.sort_by(|a, b| b.start.cmp(&a.start));

    let mut out = content.to_string();
    let mut count = 0usize;
    // 同一处被传两遍(前端去重出错)不在这里单独设闸:包完之后那个位置上的文字已经变了,
    // 第二遍会被下面的 Vanished 挡住;正文恰好是 `[[[[` 这种连括号时文字没变,则会被
    // already_linked 挡住(包完成 `[[[[[[]]`,整条链接覆盖了原区间)。少一道闸的好处是
    // 重复位置会**如实进 skipped**,而不是悄悄消失 —— 用户拿到的账才是完整的。
    for target in ordered {
        let mut skip = |reason| {
            skipped.push(MentionLinkSkip {
                path: path.to_string(),
                start: target.start,
                reason,
            });
        };
        // 位置上还是那段文字吗。区间越界、切在字符中间、文字变了,全算 Vanished ——
        // 它们的成因是同一个:扫描之后这篇被改过。
        if target.end > out.len()
            || target.start >= target.end
            || !out.is_char_boundary(target.start)
            || !out.is_char_boundary(target.end)
            || out[target.start..target.end] != target.text
        {
            skip(MentionSkipReason::Vanished);
            continue;
        }
        // 已经在一对 `[[]]` 里了?按**当前**内容里那一行重扫一遍已有链接的区间 ——
        // 判定来自定义链接的那个词法器,不是这里数括号。
        if already_linked(&out, target.start, target.end) {
            skip(MentionSkipReason::AlreadyLinked);
            continue;
        }
        out.replace_range(target.start..target.end, &format!("[[{}]]", target.text));
        count += 1;
    }
    (out, count, skipped)
}

/// `content[start..end]` 是不是落在某条已有 wikilink 里。
fn already_linked(content: &str, start: usize, end: usize) -> bool {
    // 找出这一处所在的那一行,把区间换算成行内偏移。
    let line_start = content[..start].rfind('\n').map(|at| at + 1).unwrap_or(0);
    let line_end = content[end..]
        .find('\n')
        .map(|at| end + at)
        .unwrap_or(content.len());
    let line = content[line_start..line_end].trim_end_matches('\r');
    let from = start - line_start;
    let to = end - line_start;
    scan_line(line)
        .into_iter()
        .any(|hit| from < hit.end && to > hit.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 扫一篇,只取 (行号, 命中原文, 可信度) —— 大多数用例关心的就是这三样。
    fn hits(content: &str, needles: &[&str]) -> Vec<(u32, String, MentionConfidence)> {
        let needles: Vec<String> = needles.iter().map(|n| n.to_string()).collect();
        scan_mentions(content, &normalize_needles(&needles), 999)
            .into_iter()
            .map(|hit| {
                let text = content[hit.start..hit.end].to_string();
                (hit.line, text, hit.confidence)
            })
            .collect()
    }

    /// 只取命中原文。
    fn texts(content: &str, needles: &[&str]) -> Vec<String> {
        hits(content, needles)
            .into_iter()
            .map(|(_, text, _)| text)
            .collect()
    }

    #[test]
    fn finds_a_plain_mention_with_whole_file_line_numbers() {
        let content = "---\ntitle: \"甲\"\n---\n\n见 计划 一节\n";
        // frontmatter 占 3 行、空行 1 行,所以正文那行是第 5 行(整篇坐标系)。
        assert_eq!(
            hits(content, &["计划"]),
            vec![(5, "计划".to_string(), MentionConfidence::Confident)]
        );
    }

    #[test]
    fn skips_frontmatter_fences_and_inline_code() {
        let content = concat!(
            "---\n",
            "title: \"计划\"\n", // frontmatter:不算
            "---\n",
            "```rust\n",
            "let 计划 = 1;\n", // 围栏内:不算
            "```\n",
            "`计划` 是变量名\n",  // 行内代码:不算
            "真正提到 计划 了\n", // 算
        );
        assert_eq!(hits(content, &["计划"]).len(), 1);
        assert_eq!(hits(content, &["计划"])[0].0, 8);
    }

    #[test]
    fn skips_existing_wikilinks_per_occurrence_not_per_file() {
        // 同一篇里链了一处、另有一处没链:没链的那一处必须报出来。
        // Markio 按整篇 grep `[[stem` 排除,这一行它一条都报不出来。
        let content = "先看 [[计划]],再看 计划 的附录\n";
        assert_eq!(texts(content, &["计划"]), vec!["计划"]);
        let hit = &scan_mentions(content, &["计划".to_string()], 9)[0];
        // 报出来的必须是后面那一处(第 12 字节之后),不是链接里那一处。
        assert!(hit.start > content.find("[[").unwrap());
    }

    #[test]
    fn skips_embeds_too() {
        assert!(texts("![[计划]]\n", &["计划"]).is_empty());
    }

    #[test]
    fn skips_markdown_links_and_images() {
        assert!(texts("见 [计划](./plan.md)\n", &["计划"]).is_empty());
        assert!(texts("见 [说明](./计划.md)\n", &["计划"]).is_empty());
        assert!(texts("![计划](./a.png)\n", &["计划"]).is_empty());
        // URL 里带括号的那种,配对要按深度算。
        assert!(texts("见 [说明](./a(计划).md)\n", &["计划"]).is_empty());
    }

    #[test]
    fn skips_bare_urls() {
        assert!(texts("https://example.com/计划/index\n", &["计划"]).is_empty());
        // URL 之后的正文照常算。
        assert_eq!(
            texts("见 https://example.com/a 和 计划 一节\n", &["计划"]),
            vec!["计划"]
        );
    }

    #[test]
    fn skips_atx_headings() {
        // 包一个标题会同时改掉大纲文字和 `[[笔记#小节]]` 的锚点。
        assert!(texts("## 计划\n", &["计划"]).is_empty());
        assert!(texts("###### 计划\n", &["计划"]).is_empty());
        // 七个 `#` 不是标题(CommonMark 上限 6)。
        assert_eq!(texts("####### 计划\n", &["计划"]), vec!["计划"]);
        // `#计划` 是标签写法,不是标题 —— `#` 后面没空白。
        assert_eq!(texts("#计划\n", &["计划"]), vec!["计划"]);
    }

    #[test]
    fn ascii_word_boundaries_drop_substring_matches() {
        // `Planning` 里的 `Plan` 不是对《Plan》的提及。
        assert!(texts("Planning is hard\n", &["Plan"]).is_empty());
        assert!(texts("myPlan\n", &["Plan"]).is_empty());
        assert!(texts("plan2\n", &["plan"]).is_empty());
        assert!(texts("a_plan\n", &["plan"]).is_empty());
        // 标点和连字符是干净边界。
        assert_eq!(texts("Plan-B\n", &["Plan"]), vec!["Plan"]);
        assert_eq!(texts("(Plan)\n", &["Plan"]), vec!["Plan"]);
        assert_eq!(texts("Plan.\n", &["Plan"]), vec!["Plan"]);
    }

    #[test]
    fn cjk_neighbours_are_ambiguous_not_silently_wrapped() {
        // 这条用例就是 Markio 那份的缺陷:它会把「原计划表」改成「原[[计划]]表」。
        // 这里报成 ambiguous,批量不动,要用户逐条确认。
        assert_eq!(
            hits("原计划表在这里\n", &["计划"]),
            vec![(1, "计划".to_string(), MentionConfidence::Ambiguous)]
        );
        // 只有一侧贴着汉字也是 ambiguous。
        assert_eq!(
            hits("原计划 表\n", &["计划"])[0].2,
            MentionConfidence::Ambiguous
        );
        assert_eq!(
            hits("原 计划表\n", &["计划"])[0].2,
            MentionConfidence::Ambiguous
        );
        // 两侧都干净才 confident。
        assert_eq!(
            hits("原 计划 表\n", &["计划"])[0].2,
            MentionConfidence::Confident
        );
        // 中文标点也是干净边界。
        assert_eq!(
            hits("看:计划,好\n", &["计划"])[0].2,
            MentionConfidence::Confident
        );
    }

    #[test]
    fn mixed_script_neighbours_are_ambiguous() {
        // `Plan表`:两边不同文字体系,判不了。
        assert_eq!(
            hits("Plan表\n", &["Plan"])[0].2,
            MentionConfidence::Ambiguous
        );
        assert_eq!(
            hits("表Plan\n", &["Plan"])[0].2,
            MentionConfidence::Ambiguous
        );
        // 假名同理。
        assert_eq!(
            hits("計画です\n", &["計画"])[0].2,
            MentionConfidence::Ambiguous
        );
    }

    #[test]
    fn matches_case_insensitively_and_keeps_the_prose_casing() {
        // 链接解析本身大小写不敏感(`normalizeLinkTarget` 折小写),所以 `PLAN` 包成
        // `[[PLAN]]` 照样指向《Plan》—— 不该为了大小写漏掉真实提及。
        assert_eq!(texts("see PLAN here\n", &["Plan"]), vec!["PLAN"]);
        assert_eq!(texts("see plan here\n", &["Plan"]), vec!["plan"]);
        // 命中原文照原样回传,不折成候选名的大小写。
        assert_eq!(texts("see pLaN here\n", &["Plan"]), vec!["pLaN"]);
    }

    #[test]
    fn hit_carries_both_the_candidate_name_and_the_prose_text() {
        // 两个字段必须分开:`needle` 给 UI 显示"匹配的是哪个名字",`text` 是"链接"
        // 那一步的校验依据。合成一个的话,大小写不同的命中会被校验成 `vanished` ——
        // 列表里有它、点了却说"已经不在了"。
        let found = scan_mentions("see PLAN here\n", &["Plan".to_string()], 9);
        assert_eq!(found[0].needle, "Plan");
        assert_eq!(found[0].text, "PLAN");
        // 而按 `text` 构造的 target 校验得过、包出来的是正文那个大小写。
        let target = MentionTarget {
            path: "/vault/a.md".to_string(),
            start: found[0].start,
            end: found[0].end,
            text: found[0].text.clone(),
        };
        let (next, count, skipped) = plan_wrap("see PLAN here\n", "/vault/a.md", &[&target]);
        assert_eq!((count, next.as_str()), (1, "see [[PLAN]] here\n"));
        assert!(skipped.is_empty());
        // 反过来:拿 `needle` 当校验依据就是 `vanished`。这条断言钉住的正是"合成一个
        // 字段"会造成的那个缺陷。
        let wrong = MentionTarget {
            text: found[0].needle.clone(),
            ..target
        };
        let (_, count, skipped) = plan_wrap("see PLAN here\n", "/vault/a.md", &[&wrong]);
        assert_eq!(count, 0);
        assert_eq!(skipped[0].reason, MentionSkipReason::Vanished);
    }

    #[test]
    fn offsets_survive_case_folding_that_changes_byte_length() {
        // 开尔文符号 `K`(U+212A,UTF-8 占 3 字节)小写成 `k`(1 字节)。整行转小写
        // 之后这一段短了 2 字节,拿转换后的偏移回原文切就会**切在字符中间**。
        // Markio 那份正是检测到 `line_lower.len() != line_str.len()` 就**整行放弃**
        // (那一行里的提及一处都链不了)。这里逐字符原地比,偏移始终是原文的。
        let content = "见 \u{212A}elvin 一节\n";
        let found = scan_mentions(content, &["kelvin".to_string()], 9);
        assert_eq!(found.len(), 1);
        // 切出来的就是原文那一段(3 + 5 = 8 字节),不是小写后那 6 字节。
        assert_eq!(&content[found[0].start..found[0].end], "\u{212A}elvin");
        assert_eq!(found[0].end - found[0].start, 8);
        // 包出来的链接必须是原文那一段,不能是折过小写的。
        let (next, count, _) = wrap_all(content, &["kelvin"]);
        assert_eq!(count, 1);
        assert_eq!(next, "见 [[\u{212A}elvin]] 一节\n");
    }

    #[test]
    fn case_folding_follows_the_same_rule_as_the_frontend() {
        // `İ`(U+0130)小写是 `i` + U+0307,**不**等于 `istanbul` —— JS 的
        // `toLowerCase()` 也是这个结果,而链接解析用的正是它(`normalizeLinkTarget`)。
        // 两边一致比"看起来更宽松"重要:报一条解析不到的提及,点了就是一条死链。
        assert!(texts("见 İstanbul 一节\n", &["istanbul"]).is_empty());
        // 带组合符的那一form 能匹配。
        assert_eq!(
            texts("见 İstanbul 一节\n", &["i\u{307}stanbul"]),
            vec!["İstanbul"]
        );
    }

    #[test]
    fn needle_limits_count_characters_not_bytes() {
        // 一个汉字 3 字节。Markio 的 `stem.len() >= 2` 会放行单字笔记《书》,
        // 在中文正文里等于全文高亮。
        assert!(normalize_needles(&["书".to_string()]).is_empty());
        assert_eq!(normalize_needles(&["计划".to_string()]).len(), 1);
        // 上限同样按字符。
        let long = "字".repeat(MAX_NEEDLE_CHARS + 1);
        assert!(normalize_needles(&[long]).is_empty());
        let ok = "字".repeat(MAX_NEEDLE_CHARS);
        assert_eq!(normalize_needles(&[ok]).len(), 1);
    }

    #[test]
    fn needles_are_deduped_case_insensitively() {
        // `Plan.md` + `title: plan` 会给出两个只差大小写的候选。留两个的话同一处
        // 提及会报两条。
        let input = vec!["Plan".to_string(), "plan".to_string(), "计划".to_string()];
        assert_eq!(normalize_needles(&input), vec!["Plan", "计划"]);
    }

    #[test]
    fn needles_that_cannot_form_a_link_are_dropped() {
        // body 里不许有 `]`,所以含方括号的名字包不出合法链接 —— 报一条点了会写坏
        // 文件的条目比不报更糟。
        assert!(normalize_needles(&["a]b".to_string()]).is_empty());
        assert!(normalize_needles(&["a[b".to_string()]).is_empty());
    }

    #[test]
    fn overlapping_candidates_report_one_hit() {
        // 两个候选名都命中同一处(stem 是标题的前缀)时只留一条。
        let content = "见 计划表 一节\n";
        assert_eq!(hits(content, &["计划表", "计划"]).len(), 1);
    }

    #[test]
    fn hits_in_one_line_come_out_left_to_right() {
        let content = "计划 和 方案 都要\n";
        assert_eq!(texts(content, &["方案", "计划"]), vec!["计划", "方案"]);
    }

    #[test]
    fn respects_the_per_file_cap() {
        let content = "计划 ".repeat(10) + "\n";
        assert_eq!(scan_mentions(&content, &["计划".to_string()], 3).len(), 3);
    }

    #[test]
    fn survives_crlf() {
        let content = "---\r\ntitle: \"甲\"\r\n---\r\n\r\n见 计划 一节\r\n";
        let found = scan_mentions(content, &["计划".to_string()], 9);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 5);
        assert_eq!(&content[found[0].start..found[0].end], "计划");
    }

    // ── 改写那一半(不碰磁盘)────────────────────────────────────────────

    /// 按扫描结果构造 target,再跑一遍改写。这是真实调用链的形状:UI 显示的就是扫描
    /// 结果,点下去传回来的就是它。
    fn wrap_all(content: &str, needles: &[&str]) -> (String, usize, Vec<MentionSkipReason>) {
        let needles: Vec<String> = needles.iter().map(|n| n.to_string()).collect();
        let found = scan_mentions(content, &normalize_needles(&needles), 999);
        let targets: Vec<MentionTarget> = found
            .iter()
            .map(|hit| MentionTarget {
                path: "/vault/a.md".to_string(),
                start: hit.start,
                end: hit.end,
                text: content[hit.start..hit.end].to_string(),
            })
            .collect();
        let refs: Vec<&MentionTarget> = targets.iter().collect();
        let (next, count, skipped) = plan_wrap(content, "/vault/a.md", &refs);
        (
            next,
            count,
            skipped.into_iter().map(|skip| skip.reason).collect(),
        )
    }

    #[test]
    fn wraps_every_occurrence_the_scan_reported() {
        // Markio 的"全部链接"每个文件只包一处,报告却按文件数报成处数。
        let content = "计划 在前,计划 在后\n";
        let (next, count, skipped) = wrap_all(content, &["计划"]);
        assert_eq!(count, 2);
        assert!(skipped.is_empty());
        assert_eq!(next, "[[计划]] 在前,[[计划]] 在后\n");
    }

    #[test]
    fn wraps_from_the_back_so_offsets_stay_valid() {
        // 从前往后改的话,第一处插进去的 4 个字节会把后面每一处的偏移都顶偏,
        // 于是第二处会切在字符中间 —— 表现是正文里多出乱码。
        let content = "aa 计划 bb 计划 cc 计划 dd\n";
        let (next, count, _) = wrap_all(content, &["计划"]);
        assert_eq!(count, 3);
        assert_eq!(next, "aa [[计划]] bb [[计划]] cc [[计划]] dd\n");
    }

    #[test]
    fn keeps_the_prose_casing_when_wrapping() {
        let content = "see PLAN here\n";
        let (next, count, _) = wrap_all(content, &["Plan"]);
        assert_eq!(count, 1);
        // 写进去的是正文里那个大小写,不是候选名的 —— 链接解析不敏感,而改用户的
        // 用词是没必要的越界。
        assert_eq!(next, "see [[PLAN]] here\n");
    }

    #[test]
    fn stale_offsets_are_reported_as_vanished_not_written() {
        let target = MentionTarget {
            path: "/vault/a.md".to_string(),
            start: 3,
            end: 9,
            text: "计划".to_string(),
        };
        // 内容变了:那个位置上现在是别的字。
        let content = "aa 方案 bb\n";
        let (next, count, skipped) = plan_wrap(content, "/vault/a.md", &[&target]);
        assert_eq!(count, 0);
        assert_eq!(next, content);
        assert_eq!(
            skipped.iter().map(|s| s.reason).collect::<Vec<_>>(),
            vec![MentionSkipReason::Vanished]
        );
    }

    #[test]
    fn out_of_range_and_mid_character_offsets_are_vanished() {
        let content = "aa 计划\n";
        for (start, end) in [(3usize, 999usize), (4, 9), (3, 8), (5, 5)] {
            let target = MentionTarget {
                path: "/vault/a.md".to_string(),
                start,
                end,
                text: "计划".to_string(),
            };
            let (next, count, skipped) = plan_wrap(content, "/vault/a.md", &[&target]);
            assert_eq!(count, 0, "({start}, {end}) 不该写盘");
            assert_eq!(next, content);
            assert_eq!(skipped[0].reason, MentionSkipReason::Vanished);
        }
    }

    #[test]
    fn a_position_already_inside_a_link_is_reported_as_already_linked() {
        // 扫描之后用户自己手工包了,或者上一次操作已经包过。再包一次会得到
        // `[[[[计划]]]]`,渲染出来是字面括号。
        let content = "见 [[计划]] 一节\n";
        let start = content.find("计划").unwrap();
        let target = MentionTarget {
            path: "/vault/a.md".to_string(),
            start,
            end: start + "计划".len(),
            text: "计划".to_string(),
        };
        let (next, count, skipped) = plan_wrap(content, "/vault/a.md", &[&target]);
        assert_eq!(count, 0);
        assert_eq!(next, content);
        assert_eq!(skipped[0].reason, MentionSkipReason::AlreadyLinked);
    }

    #[test]
    fn the_same_position_passed_twice_is_wrapped_once() {
        let content = "见 计划 一节\n";
        let start = content.find("计划").unwrap();
        let target = MentionTarget {
            path: "/vault/a.md".to_string(),
            start,
            end: start + "计划".len(),
            text: "计划".to_string(),
        };
        let (next, count, skipped) = plan_wrap(content, "/vault/a.md", &[&target, &target]);
        assert_eq!(count, 1);
        assert_eq!(next, "见 [[计划]] 一节\n");
        // 第二遍必须**如实记账**,不能悄悄消失:包完之后那个位置上已经不是 `计划` 了。
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].start, start);
        assert_eq!(skipped[0].reason, MentionSkipReason::Vanished);
    }

    #[test]
    fn a_repeated_position_whose_text_is_all_brackets_is_caught_as_already_linked() {
        // 上一条走的是 Vanished 那道闸。这里补另一道:正文恰好是连续 `[[` 时,包完
        // (`[[[[[[]]`)那个位置上的文字**没变**,于是只剩 already_linked 能挡住第二遍。
        // 这两条一起替掉了原先那个"同一位置只处理一次"的显式去重 —— 一处只该有一道闸。
        let content = "x [[[[ y\n";
        let start = content.find("[[[[").unwrap();
        let target = MentionTarget {
            path: "/vault/a.md".to_string(),
            start,
            end: start + "[[[[".len(),
            text: "[[[[".to_string(),
        };
        let (next, count, skipped) = plan_wrap(content, "/vault/a.md", &[&target, &target]);
        assert_eq!(count, 1);
        assert_eq!(next, "x [[[[[[]] y\n");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].reason, MentionSkipReason::AlreadyLinked);
    }

    #[test]
    fn wrapping_an_ambiguous_hit_still_works_when_asked_explicitly() {
        // 批量不动 ambiguous,但用户逐条确认时要能包 —— 分级是给批量用的,不是禁令。
        let content = "原计划表\n";
        let (next, count, _) = wrap_all(content, &["计划"]);
        assert_eq!(count, 1);
        assert_eq!(next, "原[[计划]]表\n");
    }

    #[test]
    fn wrapped_text_scans_back_as_a_link_not_as_a_mention() {
        // 闭环:包完再扫一遍,同一处必须变成"已链接"而不是又报一条未链接提及。
        // 这是"数得出来的就一定改得动、改完就不再数出来"的那条不变量。
        let content = "见 计划 一节\n";
        let (next, count, _) = wrap_all(content, &["计划"]);
        assert_eq!(count, 1);
        assert!(texts(&next, &["计划"]).is_empty());
        assert_eq!(scan_line(next.trim_end()).len(), 1);
    }

    /// 落盘跑一遍 `link_mentions`。快照写在 vault 内的 `.notebook/`,所以整个用例
    /// 自包含,不碰用户目录。
    fn temp_vault(files: &[(&str, &str)]) -> (NotebookState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("aeroric-mentions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for (name, content) in files {
            std::fs::write(root.join(name), content).unwrap();
        }
        let state = NotebookState::default();
        // register 会 canonicalize(macOS 上 /var -> /private/var),后面一律用它的返回值。
        let canon = state.register_vault(&root).unwrap();
        (state, canon)
    }

    /// 用扫描器算出真实偏移,再交给写入侧 —— 用例本身就是一次"数得出来的改得动"。
    fn targets_for(root: &Path, name: &str, needle: &str) -> Vec<MentionTarget> {
        let path = root.join(name);
        let content = std::fs::read_to_string(&path).unwrap();
        let needles = normalize_needles(&[needle.to_string()]);
        scan_mentions(&content, &needles, 999)
            .into_iter()
            .map(|hit| MentionTarget {
                path: path.to_string_lossy().to_string(),
                start: hit.start,
                end: hit.end,
                text: hit.text,
            })
            .collect()
    }

    #[test]
    fn the_report_counts_occurrences_not_files() {
        // Markio 把文件数报成了处数("已链接 12 处"其实是 12 个文件各一处)。`linked`
        // 这个字段存在的唯一理由就是挡住这个错,所以它必须在 Rust 侧有断言:前端那边
        // 的数是测试替身自己算的,替身算对了不代表这里算对了。
        let (state, root) = temp_vault(&[
            ("a.md", "见 计划 一节,另见 计划 附录\n"),
            ("b.md", "只提一次 计划\n"),
        ]);
        let mut targets = targets_for(&root, "a.md", "计划");
        targets.extend(targets_for(&root, "b.md", "计划"));
        assert_eq!(targets.len(), 3, "两篇一共该扫出三处");

        let report = link_mentions(&state, &root, &targets).unwrap();
        assert!(report.failed.is_empty(), "failed: {:?}", report.failed);
        assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
        assert_eq!(report.changed.len(), 2, "改了两个文件");
        // 关键断言:3(处)而不是 2(文件)。
        assert_eq!(report.linked, 3);

        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "见 [[计划]] 一节,另见 [[计划]] 附录\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("b.md")).unwrap(),
            "只提一次 [[计划]]\n"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_path_outside_the_vault_being_edited_is_refused() {
        // 路径来自前端。跨 vault 写一次就够毁一次 —— 这条闸必须在落盘路径上有断言。
        let (state, root) = temp_vault(&[("a.md", "见 计划 一节\n")]);
        let (_other_state, other) = temp_vault(&[("x.md", "见 计划 一节\n")]);
        // 同一个 state 注册两个 vault:resolve 能过,但 root 归属这一关必须拦住。
        state.register_vault(&other).unwrap();
        let targets = targets_for(&other, "x.md", "计划");
        assert_eq!(targets.len(), 1);

        let error = link_mentions(&state, &root, &targets).unwrap_err();
        assert!(error.contains("outside the vault"), "{error}");
        // 没写盘。
        assert_eq!(
            std::fs::read_to_string(other.join("x.md")).unwrap(),
            "见 计划 一节\n"
        );
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&other).ok();
    }
}
