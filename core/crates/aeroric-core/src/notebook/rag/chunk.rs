//! 把 markdown 切成适合 embedding 的块。
//!
//! 三条规则:
//! 1. 先按 ATX 标题分段,标题路径(`H1 > H2 > H3`)挂到每个块上 —— 检索命中时
//!    这是最省事的定位信息,也给 embedding 补了段落所属的语境。
//! 2. 段内超过 [`MAX_CHARS`] 时按空行(段落)聚合,相邻块之间保留
//!    [`OVERLAP_CHARS`] 的重叠,免得一句话被切在中间导致两块都读不通。
//! 3. 代码围栏内部不切:围栏里的空行不是段落边界,标题也不是标题。
//!
//! ## 偏移的含义
//!
//! `char_start` / `char_end` 是**字符**索引(不是字节),只覆盖这一块**新增**的
//! 正文 —— 前置的 overlap 尾部不计入。所以相邻块的偏移区间在源文里不重叠,
//! 检索命中可以据此精确跳回原文而不会指到上一块去。

use super::cjk;

pub const MAX_CHARS: usize = 1500;
pub const OVERLAP_CHARS: usize = 180;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// 标题路径,形如 `设计 > 存储`。文档没有标题时为空串。
    pub heading: String,
    /// 原文。检索结果回显与高亮用这一份。
    pub body: String,
    /// CJK 逐字切分版,进 FTS5 索引(见 [`cjk`])。
    pub body_seg: String,
    /// 标题路径的切分版,同样进索引 —— 用户搜标题里的词也该命中。
    pub heading_seg: String,
    pub char_start: usize,
    pub char_end: usize,
    /// 估算的 token 数,给上下文预算用。
    pub token_count: usize,
}

/// 估算 token 数:CJK 按一字一 token,其余按四字符一 token。
///
/// 真实分词器的结果会有出入,但这个函数的用途是「别把上下文撑爆」,低估才危险,
/// 而 CJK 取 1:1 是偏保守的一侧。
pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if cjk::is_cjk(ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    // 非 CJK 部分不足四个字符时也至少算一个 —— 返回 0 会让调用方把「一段有内容
    // 的文本」当成不占预算。
    let other_tokens = if other == 0 { 0 } else { (other / 4).max(1) };
    cjk + other_tokens
}

#[derive(Debug, Clone)]
struct Section {
    heading: String,
    start: usize,
    end: usize,
}

/// 段内聚合出的一块。`start`/`end` 相对 section body,只覆盖新增段落。
struct BodyPart {
    text: String,
    start: usize,
    end: usize,
}

/// 代码围栏的开合跟踪。
///
/// 单独抽出来是因为分段和切段两处都要用它,而两处对「在不在围栏内」给出不同
/// 答案时的后果是静默的:标题会被从代码块里切出来当真标题,而代码块被腰斩。
/// (Markio 的 `chunk.rs` 正是这样 —— `sections()` 用裸 `in_fence = !in_fence`
/// 无视围栏种类,`paragraph_spans()` 却记了 marker。)
#[derive(Default)]
struct FenceState {
    marker: Option<&'static str>,
}

impl FenceState {
    fn inside(&self) -> bool {
        self.marker.is_some()
    }

    /// 喂一行,更新状态。返回这一行处理**之后**是否在围栏内。
    fn feed(&mut self, line: &str) -> bool {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        match (self.marker, marker) {
            // 围栏外遇到任一种栏 → 进入,记下是哪种。
            (None, Some(found)) => self.marker = Some(found),
            // 围栏内只有**同种**栏能收 —— ``` 块里的 ~~~ 是代码内容。
            (Some(open), Some(found)) if open == found => self.marker = None,
            _ => {}
        }
        self.inside()
    }
}

/// 切块入口。`source` 是笔记正文(不含 frontmatter)。
pub fn split(source: &str) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    for section in sections(source) {
        let body = char_slice(source, section.start, section.end);
        if body.trim().is_empty() {
            continue;
        }
        let heading_seg = cjk::segment(&section.heading);
        for part in split_section_body(&body) {
            if part.text.trim().is_empty() {
                continue;
            }
            out.push(Chunk {
                heading: section.heading.clone(),
                body_seg: cjk::segment(&part.text),
                heading_seg: heading_seg.clone(),
                token_count: estimate_tokens(&part.text),
                body: part.text,
                char_start: section.start + part.start,
                char_end: section.start + part.end,
            });
        }
    }
    out
}

fn char_slice(source: &str, start: usize, end: usize) -> String {
    source
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// 按标题分段。无标题文档返回一个 heading 为空的整体段。
fn sections(source: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    // (层级, 标题) 栈,用来拼 `H1 > H2` 路径。
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut current_start = 0usize;
    let mut current_heading = String::new();
    let mut char_pos = 0usize;
    let mut fence = FenceState::default();

    for line in source.split_inclusive('\n') {
        let line_chars = line.chars().count();
        // 先判围栏:围栏行自己不可能是标题,而 feed 之后的状态才是这一行之后的。
        let inside = fence.inside();
        fence.feed(line);
        if !inside {
            if let Some((level, title)) = parse_heading(line.trim_start()) {
                if char_pos > current_start {
                    out.push(Section {
                        heading: std::mem::take(&mut current_heading),
                        start: current_start,
                        end: char_pos,
                    });
                }
                // 弹掉同级与更深的标题,再压入自己。
                while let Some((lvl, _)) = stack.last() {
                    if *lvl >= level {
                        stack.pop();
                    } else {
                        break;
                    }
                }
                stack.push((level, title));
                current_heading = stack
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" > ");
                // 标题行本身不进正文 —— 它已经在 heading 里了。
                current_start = char_pos + line_chars;
            }
        }
        char_pos += line_chars;
    }
    if char_pos > current_start {
        out.push(Section {
            heading: current_heading,
            start: current_start,
            end: char_pos,
        });
    }
    if out.is_empty() {
        out.push(Section {
            heading: String::new(),
            start: 0,
            end: char_pos,
        });
    }
    out
}

/// 认 ATX 标题,返回 (层级, 标题文本)。
fn parse_heading(line: &str) -> Option<(usize, String)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    // `#hashtag` 不是标题 —— 井号后必须跟空白。行尾恰好结束的 `#` 也不算。
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    // 闭合式 ATX(`## 标题 ##`)的尾部井号要去掉。
    let title = rest.trim().trim_end_matches('#').trim();
    if title.is_empty() {
        return None;
    }
    Some((hashes, title.to_string()))
}

/// 按空行切段,保留相对 body 的字符偏移。围栏内的空行不切。
fn paragraph_spans(body: &str) -> Vec<(String, usize, usize)> {
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    let mut seg = String::new();
    let mut seg_start = 0usize;
    let mut cur = 0usize;
    let mut fence = FenceState::default();

    for line in body.split_inclusive('\n') {
        let line_chars = line.chars().count();
        fence.feed(line);
        if line.trim().is_empty() && !fence.inside() {
            flush_span(&mut out, &mut seg, seg_start, cur);
            cur += line_chars;
            seg_start = cur;
            continue;
        }
        if seg.is_empty() {
            seg_start = cur;
        }
        seg.push_str(line);
        cur += line_chars;
    }
    flush_span(&mut out, &mut seg, seg_start, cur);
    out
}

fn flush_span(out: &mut Vec<(String, usize, usize)>, seg: &mut String, start: usize, end: usize) {
    let trimmed = seg.trim_end_matches('\n');
    if !trimmed.trim().is_empty() {
        out.push((trimmed.to_string(), start, end));
    }
    seg.clear();
}

/// 段内聚合到 [`MAX_CHARS`],相邻块保留 [`OVERLAP_CHARS`] 重叠。
fn split_section_body(body: &str) -> Vec<BodyPart> {
    let total = body.chars().count();
    if total <= MAX_CHARS {
        return vec![BodyPart {
            text: body.to_string(),
            start: 0,
            end: total,
        }];
    }
    let mut parts: Vec<BodyPart> = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;
    let mut start: Option<usize> = None;
    let mut end = 0usize;

    for (paragraph, p_start, p_end) in paragraph_spans(body) {
        let p_len = paragraph.chars().count();
        // 单个段落本身就超上限(一大坨代码、或整篇没有空行的长文)。聚合逻辑在
        // 这里无处下刀 —— 它只能在段落之间断开 —— 所以先把待聚合的部分收尾,
        // 再把这个段落按字符硬切。
        //
        // 必须真的切开:只把它整段推进去(Markio 那份实现就是如此)等于没切,
        // 一个几万字的段落会原样进 embedding 请求。
        if p_len > MAX_CHARS {
            if !current.trim().is_empty() {
                parts.push(BodyPart {
                    text: std::mem::take(&mut current),
                    start: start.take().unwrap_or(0),
                    end,
                });
            }
            current.clear();
            current_chars = 0;
            start = None;
            end = p_end;
            parts.extend(hard_split(&paragraph, p_start));
            continue;
        }
        // `+ 2` 是即将插入的段落分隔符。
        if current_chars + p_len + 2 > MAX_CHARS && !current.is_empty() {
            let tail = tail_chars(&current, OVERLAP_CHARS);
            parts.push(BodyPart {
                text: std::mem::take(&mut current),
                start: start.take().unwrap_or(0),
                end,
            });
            // overlap 只进文本不进偏移,所以下一块的 start 由下一个段落定。
            current.push_str(&tail);
            current_chars = tail.chars().count();
        }
        if start.is_none() {
            start = Some(p_start);
        }
        if !current.is_empty() {
            current.push_str("\n\n");
            current_chars += 2;
        }
        current.push_str(&paragraph);
        current_chars += p_len;
        end = p_end;
    }
    if !current.trim().is_empty() {
        parts.push(BodyPart {
            text: current,
            start: start.unwrap_or(0),
            end,
        });
    }
    parts
}

/// 把一个超长段落按字符切成若干块。`base` 是这个段落在 section body 里的起点。
///
/// 与聚合路径同一套 overlap 约定:重叠部分进文本、不进偏移,所以相邻块的
/// `start..end` 在原文里不重叠。这里的重叠尤其有用 —— 硬切的下刀位置是纯按
/// 字数定的,几乎一定切在句子中间。
fn hard_split(paragraph: &str, base: usize) -> Vec<BodyPart> {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    while cursor < chars.len() {
        let end = (cursor + MAX_CHARS).min(chars.len());
        // 前置上一块的尾部。第一块没有可前置的,`saturating_sub` 给出 0。
        let text_start = cursor.saturating_sub(OVERLAP_CHARS);
        parts.push(BodyPart {
            text: chars[text_start..end].iter().collect(),
            start: base + cursor,
            end: base + end,
        });
        cursor = end;
    }
    parts
}

/// 取末尾 `n` 个字符。按 char 取,不会切裂多字节字符。
fn tail_chars(text: &str, n: usize) -> String {
    let total = text.chars().count();
    text.chars().skip(total.saturating_sub(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_by_heading_and_keeps_the_path() {
        let chunks = split("# 设计\n\n开头。\n\n## 存储\n\n用 sqlite。\n");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading, "设计");
        assert_eq!(chunks[1].heading, "设计 > 存储");
        assert!(chunks[1].body.contains("用 sqlite"));
    }

    #[test]
    fn heading_line_itself_is_not_part_of_the_body() {
        // 标题已经在 heading 字段里,正文里再来一遍会让 embedding 重复计权。
        let chunks = split("# 设计\n\n开头。\n");
        assert!(!chunks[0].body.contains('#'));
    }

    #[test]
    fn pops_sibling_headings_from_the_path() {
        let chunks = split("# A\n\nx\n\n## B\n\ny\n\n## C\n\nz\n");
        let headings: Vec<&str> = chunks.iter().map(|c| c.heading.as_str()).collect();
        assert_eq!(headings, vec!["A", "A > B", "A > C"]);
    }

    #[test]
    fn document_without_headings_is_one_section() {
        let chunks = split("就是一段话,没有标题。\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, "");
    }

    #[test]
    fn empty_source_yields_no_chunks() {
        assert!(split("").is_empty());
        assert!(split("\n\n   \n").is_empty());
    }

    #[test]
    fn hashtag_is_not_a_heading() {
        let chunks = split("#标签 不是标题\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, "");
    }

    #[test]
    fn seven_hashes_is_not_a_heading() {
        let chunks = split("####### 太深了\n");
        assert_eq!(chunks[0].heading, "");
    }

    #[test]
    fn closing_atx_hashes_are_trimmed() {
        let chunks = split("## 标题 ##\n\n正文\n");
        assert_eq!(chunks[0].heading, "标题");
    }

    #[test]
    fn heading_inside_a_code_fence_is_not_a_heading() {
        let source = "# 真标题\n\n```md\n# 这是代码里的示例\n```\n\n尾巴\n";
        let chunks = split(source);
        // 全部内容都该留在「真标题」段里 —— 代码块里的 # 不能切段。
        assert!(chunks.iter().all(|c| c.heading == "真标题"));
    }

    #[test]
    fn tilde_fence_inside_backtick_fence_does_not_close_it() {
        // 这是 Markio 那份实现的 bug:裸翻转会在这里把围栏当成已闭合,
        // 于是 `# 冒充标题` 被切成真标题。
        let source = "# 真标题\n\n```md\n~~~\n# 冒充标题\n~~~\n```\n\n尾巴\n";
        let chunks = split(source);
        assert!(
            chunks.iter().all(|c| c.heading == "真标题"),
            "围栏种类没区分,代码块里的标题被切了出来: {:?}",
            chunks.iter().map(|c| &c.heading).collect::<Vec<_>>()
        );
    }

    #[test]
    fn blank_lines_inside_a_fence_do_not_split_paragraphs() {
        let body = "intro\n\n```py\na = 1\n\nb = 2\n```\n\nouter";
        let spans = paragraph_spans(body);
        assert_eq!(spans.len(), 3, "围栏被腰斩了: {spans:?}");
        assert!(spans
            .iter()
            .any(|(text, _, _)| text.contains("a = 1") && text.contains("b = 2")));
    }

    #[test]
    fn offsets_point_at_the_original_text() {
        let source = "# 标题\n\n第一段。\n";
        let chunks = split(source);
        let chars: Vec<char> = source.chars().collect();
        let slice: String = chars[chunks[0].char_start..chunks[0].char_end]
            .iter()
            .collect();
        assert!(slice.contains("第一段"), "偏移没对上原文: {slice:?}");
    }

    #[test]
    fn long_section_splits_with_overlap() {
        // 每段 100 字,凑到超过 MAX_CHARS。
        let paragraph = "啊".repeat(100);
        let source: String = (0..25)
            .map(|i| format!("{paragraph}{i}\n\n"))
            .collect::<Vec<_>>()
            .join("");
        let chunks = split(&source);
        assert!(chunks.len() > 1, "超长段落没被切开");
        // 后一块的开头应当能在前一块的尾部找到 —— 那就是 overlap。
        let head: String = chunks[1].body.chars().take(20).collect();
        assert!(
            chunks[0].body.contains(&head),
            "相邻块之间没有重叠,一句话会被切成两半都读不通"
        );
    }

    #[test]
    fn adjacent_offsets_do_not_overlap() {
        // overlap 进文本但不进偏移 —— 否则跳回原文会指到上一块去。
        let paragraph = "啊".repeat(100);
        let source: String = (0..25)
            .map(|i| format!("{paragraph}{i}\n\n"))
            .collect::<Vec<_>>()
            .join("");
        let chunks = split(&source);
        for pair in chunks.windows(2) {
            assert!(
                pair[0].char_end <= pair[1].char_start,
                "偏移区间重叠: {:?} vs {:?}",
                (pair[0].char_start, pair[0].char_end),
                (pair[1].char_start, pair[1].char_end),
            );
        }
    }

    #[test]
    fn every_chunk_has_a_sane_offset_range() {
        let source = "# A\n\n".to_string() + &"文字。\n\n".repeat(400);
        let total = source.chars().count();
        for chunk in split(&source) {
            assert!(chunk.char_start <= chunk.char_end, "区间反了");
            assert!(chunk.char_end <= total, "越过了文档末尾");
        }
    }

    #[test]
    fn oversized_single_paragraph_is_hard_split() {
        // 没有空行的一大坨,聚合逻辑无处下刀,只能硬切。
        let source = "啊".repeat(MAX_CHARS * 3);
        let chunks = split(&source);
        assert!(chunks.len() > 1, "超长单段没切开,会把 embedding 请求撑爆");
    }

    #[test]
    fn body_seg_is_the_segmented_form_of_body() {
        // 两者对不上会让 FTS 索引到的东西与回显的正文不是一回事。
        for chunk in split("# 标题\n\n随手记的导出功能\n") {
            assert_eq!(chunk.body_seg, cjk::segment(&chunk.body));
            assert_eq!(chunk.heading_seg, cjk::segment(&chunk.heading));
        }
    }

    #[test]
    fn estimates_cjk_as_one_token_each() {
        assert_eq!(estimate_tokens("随手记"), 3);
    }

    #[test]
    fn estimates_ascii_as_quarter_of_chars() {
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn short_ascii_still_costs_a_token() {
        // 返回 0 会让调用方以为这段不占预算。
        assert_eq!(estimate_tokens("hi"), 1);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn mixed_text_adds_both_sides() {
        // 三个汉字 + 四个 ascii。
        assert_eq!(estimate_tokens("随手记abcd"), 4);
    }

    #[test]
    fn fence_state_tracks_marker_kind() {
        let mut fence = FenceState::default();
        assert!(fence.feed("```py"));
        assert!(fence.feed("~~~"), "异种围栏不该闭合当前围栏");
        assert!(fence.feed("code"));
        assert!(!fence.feed("```"), "同种围栏该闭合");
    }

    #[test]
    fn fence_state_ignores_indented_prose() {
        let mut fence = FenceState::default();
        assert!(!fence.feed("普通一行"));
        assert!(fence.feed("   ```"), "缩进的围栏仍是围栏");
    }
}
