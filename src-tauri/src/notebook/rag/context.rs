//! AI 对话的上下文装配。
//!
//! 一次提问要塞给模型的东西有三样:用户正开着的那篇笔记(问「这段是什么意思」
//! 时指的就是它)、检索回来的片段、以及一个不能超的 token 预算。这个模块只负责
//! 按预算把它们拼成一段文本,并给出与文本里 `[N]` 标记一一对应的引用清单 ——
//! 对话本身走 Aeroric 现有的 agent 基建(`agent_ops` / `local_router`),不在这里。
//!
//! 装配单独成模块且不碰 IO,因为「预算怎么分」是唯一会被反复调整的部分,而它
//! 出错的表现是静默的:上下文被悄悄截掉一半,模型答得头头是道却漏了关键段落,
//! 没有任何报错。
//!
//! 结构性文案(`## Current note` 这些)是英文的,与 Rust 侧其余字符串一致 ——
//! 这一层没有 i18n,而这段文本的读者是模型。用户可见的部分在前端。

use super::chunk::estimate_tokens;
use super::search::SearchHit;

/// 默认预算。
///
/// 3000 token 是「本地小模型也吃得下」与「装得进几段正文」之间的折中:8k 上下文
/// 的模型留出一半给对话历史和回答,而 3000 token 大约是两三千个中文字。
pub const DEFAULT_MAX_TOKENS: usize = 3000;

/// 当前笔记默认取开头多少个字符。
pub const DEFAULT_CURRENT_CHARS: usize = 1200;

/// 当前笔记最多占预算的几分之一。
///
/// 不设上限的话一篇长笔记会把预算吃干,一个片段都进不来 —— 而「只看当前笔记」
/// 恰恰是问答里最没用的一种上下文:用户自己就在看它。
const CURRENT_BUDGET_DIVISOR: usize = 2;

const FRAGMENTS_HEADER: &str = "## Retrieved fragments";

/// 用户正开着的笔记。
#[derive(Debug, Clone)]
pub struct CurrentNote {
    /// 绝对路径,与 [`SearchHit::path`] 同一个值 —— 去重要按它比对。
    pub path: String,
    pub title: String,
    /// **已剥掉 frontmatter 的**正文。必须与建索引时的口径一致,否则
    /// [`SearchHit::char_start`] 与这里的字符偏移不在同一个坐标系里,去重会错位。
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ContextOptions {
    /// 总预算。超了就丢片段(当前笔记只截不丢)。
    pub max_tokens: usize,
    /// 当前笔记最多取开头多少个**字符**。
    pub current_chars: usize,
}

impl Default for ContextOptions {
    fn default() -> Self {
        Self {
            max_tokens: DEFAULT_MAX_TOKENS,
            current_chars: DEFAULT_CURRENT_CHARS,
        }
    }
}

/// 进了上下文的一个片段。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    /// 从 1 起,与正文里的 `[N]` 标记对应。模型引用 `[2]` 时前端据此找到这一条。
    pub index: usize,
    /// 命中本身。跳回原文、预览高亮都用它,不必再查一遍库。
    #[serde(flatten)]
    pub hit: SearchHit,
}

/// 装配结果。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assembled {
    /// 拼好的上下文。空串表示既没有当前笔记也没有片段。
    pub text: String,
    pub citations: Vec<Citation>,
    /// [`text`](Assembled::text) 的估算 token 数,恒不超过 `max_tokens`。
    pub tokens: usize,
    /// 有内容因为预算被丢掉或截断。UI 该提示「上下文已截断」—— 否则用户只会
    /// 觉得模型漏看了东西。
    pub truncated: bool,
}

/// 按预算拼上下文。
///
/// `hits` 需已按相关度排好(检索返回的顺序)。装不下的片段被**整块**丢掉而不是
/// 截半:半块正文配一条声称覆盖整块的引用,会让预览里显示的内容和模型实际看到
/// 的不一致,而那种不一致没人查得出来。
pub fn assemble(
    current: Option<&CurrentNote>,
    hits: &[SearchHit],
    options: &ContextOptions,
) -> Assembled {
    let mut text = String::new();
    let mut truncated = false;
    // 当前笔记摘录覆盖到的字符位置。片段去重按它判。
    let mut excerpt_chars = 0usize;
    let current_path = current.map(|note| note.path.as_str()).unwrap_or_default();

    if let Some(note) = current {
        let body = note.body.trim_start();
        let excerpt = prefix_within(
            body,
            options.current_chars,
            options.max_tokens / CURRENT_BUDGET_DIVISOR.max(1),
        );
        // 偏移要按 `note.body` 算 —— `trim_start` 掉的那几个空白字符也占位置。
        let dropped = note.body.chars().count() - body.chars().count();
        let block = format!("## Current note: {}\n\n{}", note.title, excerpt.trim_end());
        let appended =
            !excerpt.trim().is_empty() && try_append(&mut text, &block, options.max_tokens);
        // 覆盖范围只在摘录**真的进了文本**之后才登记。去重的判据必须由文本里实际
        // 有什么决定,而不是由一个可能没拼进去的意图决定 —— 反过来会让这篇笔记在
        // 上下文里彻底消失:摘录没进去,它的片段又被当成重复的跳掉。
        if appended {
            excerpt_chars = dropped + excerpt.chars().count();
        }
        // 正文没全进去就要说。空白正文不算截断 —— 那不是预算的问题,而报「上下文
        // 已截断」会让用户以为漏了内容。
        if excerpt.chars().count() < body.chars().count() || (!appended && !body.trim().is_empty())
        {
            truncated = true;
        }
    }

    let mut citations: Vec<Citation> = Vec::new();
    for hit in hits {
        // 当前笔记开头那一段已经原文进了上下文,再作为片段进一次是纯浪费预算。
        // 判据是「块的起点落在摘录里」:同一篇更靠后的块是真正的新内容,要保留。
        if hit.path == current_path && hit.char_start < excerpt_chars {
            continue;
        }
        let index = citations.len() + 1;
        let fragment = fragment_of(index, hit);
        // 小节标题只在第一个片段进得去的时候才出现 —— 一个片段都没装下时留一个
        // 空标题在那里,会让模型以为检索过但什么都没找到。
        let block = if citations.is_empty() {
            format!("{FRAGMENTS_HEADER}\n\n{fragment}")
        } else {
            fragment
        };
        if !try_append(&mut text, &block, options.max_tokens) {
            // 不跳出循环:后面可能有更短的片段装得下。名次是降序的,拿一个短的
            // 低名次片段换一个装不下的高名次片段,总比空着好。
            truncated = true;
            continue;
        }
        citations.push(Citation {
            index,
            hit: hit.clone(),
        });
    }

    let tokens = estimate_tokens(&text);
    Assembled {
        text,
        citations,
        tokens,
        truncated,
    }
}

/// 试着把一段接到末尾。超预算就原样退回并返回 false。
///
/// 预算按**拼完的整段文本**判,而不是各块估算值相加:小节标题与段间空行也占
/// token,分开累加迟早会漏掉一处,而漏掉的表现是「明明限了 3000 却发出去 3100」。
fn try_append(text: &mut String, block: &str, max_tokens: usize) -> bool {
    let before = text.len();
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(block);
    if estimate_tokens(text) > max_tokens {
        // `before` 是上一次拼完时的长度,必然落在字符边界上。
        text.truncate(before);
        return false;
    }
    true
}

/// 一个片段的文本。
///
/// 只写标题与小节,不写路径:路径对模型没有用(它不会去读文件),而绝对路径既
/// 占 token 又把用户的目录结构塞进了会发给远端的内容里。前端要跳回原文,用的是
/// [`Citation::hit`] 里的 `path`。
fn fragment_of(index: usize, hit: &SearchHit) -> String {
    let mut label = if hit.title.trim().is_empty() {
        format!("[{index}]")
    } else {
        format!("[{index}] {}", hit.title.trim())
    };
    if !hit.heading.trim().is_empty() {
        label.push_str(" › ");
        label.push_str(hit.heading.trim());
    }
    format!("### {label}\n\n{}", hit.body.trim_end())
}

/// 取 `text` 开头最长的、同时满足「不超过 `max_chars` 个字符」与「不超过
/// `max_tokens` 个 token」的前缀。
///
/// 两道上限都要:1200 个中文字是 1200 token,而 1200 个英文字符只有 300 —— 只按
/// 字符截会让一篇中文笔记吃掉整个预算。
///
/// token 那一道用二分找,而不是自己再按 CJK 比例算一遍:[`estimate_tokens`] 的
/// 口径只该有一处定义,抄第二遍就会漂移,而漂移的表现是预算悄悄超或者上下文
/// 莫名变短。前缀越长 token 数单调不减,所以二分成立。
fn prefix_within(text: &str, max_chars: usize, max_tokens: usize) -> String {
    let chars: Vec<char> = text.chars().take(max_chars).collect();
    let whole: String = chars.iter().collect();
    if estimate_tokens(&whole) <= max_tokens {
        return whole;
    }
    // low 恒合法(空串是 0 token),high 恒不合法。
    let (mut low, mut high) = (0usize, chars.len());
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        let candidate: String = chars[..mid].iter().collect();
        if estimate_tokens(&candidate) <= max_tokens {
            low = mid;
        } else {
            high = mid;
        }
    }
    chars[..low].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notebook::rag::search::Span;

    fn hit(path: &str, title: &str, heading: &str, body: &str, char_start: usize) -> SearchHit {
        SearchHit {
            path: path.to_string(),
            title: title.to_string(),
            heading: heading.to_string(),
            body: body.to_string(),
            score: 1.0,
            source: "vector+fts".to_string(),
            char_start,
            char_end: char_start + body.chars().count(),
            body_spans: Vec::new(),
            source_spans: Vec::new(),
        }
    }

    fn note(path: &str, title: &str, body: &str) -> CurrentNote {
        CurrentNote {
            path: path.to_string(),
            title: title.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn nothing_in_nothing_out() {
        let out = assemble(None, &[], &ContextOptions::default());
        assert!(out.text.is_empty());
        assert!(out.citations.is_empty());
        assert_eq!(out.tokens, 0);
        assert!(!out.truncated);
    }

    #[test]
    fn the_current_note_comes_first() {
        let out = assemble(
            Some(&note("/v/a.md", "甲", "正文内容")),
            &[hit("/v/b.md", "乙", "小节", "片段内容", 0)],
            &ContextOptions::default(),
        );
        let note_at = out.text.find("## Current note: 甲").expect("当前笔记");
        let frag_at = out.text.find(FRAGMENTS_HEADER).expect("片段小节");
        assert!(note_at < frag_at, "当前笔记要排在片段前面:{}", out.text);
        assert!(out.text.contains("正文内容"));
    }

    #[test]
    fn citations_are_numbered_to_match_the_markers() {
        let out = assemble(
            None,
            &[
                hit("/v/a.md", "甲", "", "第一段", 0),
                hit("/v/b.md", "乙", "", "第二段", 0),
            ],
            &ContextOptions::default(),
        );
        assert_eq!(
            out.citations.iter().map(|c| c.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(out.text.contains("### [1] 甲"));
        assert!(out.text.contains("### [2] 乙"));
        assert_eq!(out.citations[1].hit.path, "/v/b.md");
    }

    #[test]
    fn a_fragment_shows_its_heading_path() {
        let out = assemble(
            None,
            &[hit("/v/a.md", "甲", "设计 > 存储", "正文", 0)],
            &ContextOptions::default(),
        );
        assert!(
            out.text.contains("### [1] 甲 › 设计 > 存储"),
            "小节路径要跟在标题后面:{}",
            out.text
        );
    }

    #[test]
    fn an_untitled_fragment_still_gets_a_marker() {
        // 标题空(笔记还没写标题)时不能拼出 `### [1] ` 这种带尾空格的标签。
        let out = assemble(
            None,
            &[hit("/v/a.md", "", "", "正文", 0)],
            &ContextOptions::default(),
        );
        assert!(out.text.contains("### [1]\n"), "{}", out.text);
    }

    #[test]
    fn the_budget_is_never_exceeded() {
        let hits: Vec<SearchHit> = (0..20)
            .map(|i| hit("/v/x.md", "笔记", "", &"中文内容".repeat(20), i * 100))
            .collect();
        let out = assemble(
            None,
            &hits,
            &ContextOptions {
                max_tokens: 200,
                current_chars: DEFAULT_CURRENT_CHARS,
            },
        );
        assert!(out.tokens <= 200, "算出来 {} token", out.tokens);
        assert_eq!(
            out.tokens,
            estimate_tokens(&out.text),
            "上报的 token 数必须就是文本自己的估算值"
        );
        assert!(out.truncated, "丢了片段就要说");
        assert!(
            !out.citations.is_empty(),
            "预算够装几段的时候不该一段都不装"
        );
    }

    #[test]
    fn every_citation_is_present_in_the_text() {
        // 引用清单与文本必须一致:清单里有而文本里没有,前端会显示一条模型
        // 根本没看见的引用。
        let hits: Vec<SearchHit> = (0..12)
            .map(|i| hit("/v/x.md", "笔记", "", &format!("片段{i}的内容"), i * 100))
            .collect();
        let out = assemble(
            None,
            &hits,
            &ContextOptions {
                max_tokens: 120,
                current_chars: DEFAULT_CURRENT_CHARS,
            },
        );
        for citation in &out.citations {
            assert!(
                out.text.contains(&format!("### [{}]", citation.index)),
                "引用 {} 不在文本里",
                citation.index
            );
            assert!(out.text.contains(citation.hit.body.trim_end()));
        }
        assert_eq!(
            out.text.matches("### [").count(),
            out.citations.len(),
            "文本里的标记数必须与清单等长"
        );
    }

    #[test]
    fn a_short_fragment_can_take_the_slot_a_long_one_could_not() {
        // 名次是降序的。高名次那段装不下时不该直接收工 —— 后面短的仍然有价值。
        let out = assemble(
            None,
            &[
                hit("/v/a.md", "甲", "", &"很长的内容".repeat(60), 0),
                hit("/v/b.md", "乙", "", "短", 0),
            ],
            &ContextOptions {
                max_tokens: 60,
                current_chars: DEFAULT_CURRENT_CHARS,
            },
        );
        assert_eq!(out.citations.len(), 1);
        assert_eq!(out.citations[0].hit.path, "/v/b.md");
        assert_eq!(out.citations[0].index, 1, "编号按实际装进去的顺序");
        assert!(out.truncated);
    }

    #[test]
    fn a_fragment_is_dropped_whole_never_cut_in_half() {
        let long = "内容".repeat(80);
        let out = assemble(
            None,
            &[hit("/v/a.md", "甲", "", &long, 0)],
            &ContextOptions {
                max_tokens: 40,
                current_chars: DEFAULT_CURRENT_CHARS,
            },
        );
        assert!(out.citations.is_empty());
        assert!(
            out.text.is_empty(),
            "装不下就整块不要,也不该留一个空的片段小节:{:?}",
            out.text
        );
        assert!(out.truncated);
    }

    #[test]
    fn the_current_note_cannot_eat_the_whole_budget() {
        // 一篇长笔记按 current_chars 截仍然可能是几千 token。不留额度给片段的话
        // 检索白做了。
        let body = "中文".repeat(2000);
        let out = assemble(
            Some(&note("/v/a.md", "甲", &body)),
            &[hit("/v/b.md", "乙", "", "片段内容", 0)],
            &ContextOptions {
                max_tokens: 400,
                current_chars: 4000,
            },
        );
        assert!(out.tokens <= 400);
        assert_eq!(out.citations.len(), 1, "片段必须还有位置进来");
        assert!(out.truncated);
    }

    #[test]
    fn a_long_cjk_note_is_cut_by_tokens_not_by_chars() {
        // 1200 个中文字是 1200 token。只按字符截会让预算被悄悄突破。
        let body = "中".repeat(1200);
        let out = assemble(
            Some(&note("/v/a.md", "甲", &body)),
            &[],
            &ContextOptions {
                max_tokens: 300,
                current_chars: 1200,
            },
        );
        assert!(out.tokens <= 300, "算出来 {} token", out.tokens);
        // 一半的预算 = 150 token = 150 个中文字。
        assert!(
            out.text.matches('中').count() <= 150,
            "截了 {} 个字",
            out.text.matches('中').count()
        );
        assert!(out.text.matches('中').count() >= 100, "别截得过头");
    }

    #[test]
    fn an_ascii_note_is_cut_by_chars_not_by_tokens() {
        // 反过来:1200 个 ASCII 字符只有 300 token,不该因为「token 还有余额」就
        // 越过 current_chars 去多拿正文。
        let body = "a".repeat(5000);
        let out = assemble(
            Some(&note("/v/a.md", "甲", &body)),
            &[],
            &ContextOptions {
                max_tokens: 3000,
                current_chars: 1200,
            },
        );
        assert_eq!(out.text.matches('a').count(), 1200);
        assert!(out.truncated, "正文被截了就要说");
    }

    #[test]
    fn a_note_that_fits_entirely_is_not_reported_as_truncated() {
        let out = assemble(
            Some(&note("/v/a.md", "甲", "很短的正文")),
            &[],
            &ContextOptions::default(),
        );
        assert!(!out.truncated);
        assert!(out.text.contains("很短的正文"));
    }

    #[test]
    fn prefix_never_splits_a_char() {
        // 按字节截会在多字节字符中间切开。这里断言的是「截出来的仍是合法字符串」,
        // 而 char 边界由 `chars()` 保证 —— 但这一条要钉住,免得有人改成按字节。
        let text = "中文abc中文";
        for limit in 0..=text.chars().count() {
            let out = prefix_within(text, limit, usize::MAX);
            assert!(text.starts_with(&out));
            assert_eq!(out.chars().count(), limit);
        }
    }

    #[test]
    fn a_hit_already_inside_the_excerpt_is_dropped() {
        // 当前笔记开头那一段已经原文进了上下文,再作为片段进一次是纯浪费预算 ——
        // 而浪费的表现是「明明检索到了别的笔记,模型却只看到当前这篇」。
        let body = "开头这一段".repeat(10);
        let out = assemble(
            Some(&note("/v/a.md", "甲", &body)),
            &[
                hit("/v/a.md", "甲", "", "开头这一段", 0),
                hit("/v/b.md", "乙", "", "别的笔记", 0),
            ],
            &ContextOptions::default(),
        );
        assert_eq!(
            out.citations
                .iter()
                .map(|c| c.hit.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/v/b.md"]
        );
    }

    #[test]
    fn a_later_hit_in_the_current_note_is_kept() {
        // 同一篇但落在摘录之外的块是真正的新内容。一律按路径去重会把它一起丢掉,
        // 那正是「问一篇长笔记的结尾,模型只看得到开头」的原因。
        let body = "开头".repeat(10);
        let out = assemble(
            Some(&note("/v/a.md", "甲", &body)),
            &[hit("/v/a.md", "甲", "结尾", "很后面的内容", 5000)],
            &ContextOptions::default(),
        );
        assert_eq!(out.citations.len(), 1);
        assert_eq!(out.citations[0].hit.heading, "结尾");
    }

    #[test]
    fn an_empty_note_does_not_suppress_its_own_hits() {
        // 正文全是空白时摘录是空的,覆盖范围必须留在 0 —— 否则这篇笔记的片段会被
        // 当成「已经在上下文里了」而跳掉,于是它一个字都没进去。
        let out = assemble(
            Some(&note("/v/a.md", "甲", "   \n\n  ")),
            &[hit("/v/a.md", "甲", "", "片段", 0)],
            &ContextOptions::default(),
        );
        assert!(!out.text.contains("## Current note"));
        assert_eq!(out.citations.len(), 1);
        assert!(!out.truncated, "空笔记不是「被截断」,别让 UI 报错觉");
    }

    #[test]
    fn a_budget_too_small_for_anything_reports_truncation() {
        // `max_tokens` 被调到荒谬的小值。什么都装不下时必须如实上报,而不是回一段
        // 空上下文让模型硬答。
        let out = assemble(
            Some(&note("/v/a.md", "甲", "有内容的正文")),
            &[hit("/v/b.md", "乙", "", "片段", 0)],
            &ContextOptions {
                max_tokens: 1,
                current_chars: 1200,
            },
        );
        assert!(out.text.is_empty());
        assert!(out.citations.is_empty());
        assert!(out.truncated);
    }

    #[test]
    fn leading_blank_lines_do_not_shift_the_dedup_boundary() {
        // frontmatter 剥掉之后正文常常以空行开头。摘录做了 trim_start,但去重判据
        // 是**源文**里的字符偏移 —— 不把 trim 掉的长度加回去,边界就会左移,而
        // 左移的后果是本该去掉的重复片段又进来了。
        //
        // 所以要卡在边界上验:两个换行 + 摘录 10 个字 ⇒ 边界是源文偏移 12。少加
        // 那 2 的话边界是 10,于是偏移 10 与 11 这两块会被放进来。
        let body = format!("\n\n{}", "正文".repeat(30));
        let options = ContextOptions {
            max_tokens: 3000,
            current_chars: 10,
        };
        let inside = assemble(
            Some(&note("/v/a.md", "甲", &body)),
            &[hit("/v/a.md", "甲", "", "正文", 11)],
            &options,
        );
        assert!(
            inside.citations.is_empty(),
            "偏移 11 是摘录的最后一个字,重复了"
        );

        let past = assemble(
            Some(&note("/v/a.md", "甲", &body)),
            &[hit("/v/a.md", "甲", "", "正文", 12)],
            &options,
        );
        assert_eq!(
            past.citations.len(),
            1,
            "偏移 12 是摘录之后的第一个字,是新内容"
        );
    }

    #[test]
    fn citations_serialize_flat_with_the_hit_fields() {
        // 前端拿到的是一个对象,`index` 与命中字段平铺在一起。嵌套一层的话
        // 引用预览要多一层解构 —— 而这个形状一旦发出去就不好改了。
        let mut source = hit("/v/a.md", "甲", "小节", "正文", 12);
        source.body_spans = vec![Span { start: 0, end: 2 }];
        let out = assemble(None, &[source], &ContextOptions::default());
        let json = serde_json::to_value(&out.citations[0]).expect("serialize");
        assert_eq!(json["index"], 1);
        assert_eq!(json["path"], "/v/a.md");
        assert_eq!(json["charStart"], 12);
        assert_eq!(json["bodySpans"][0]["end"], 2);
    }
}
