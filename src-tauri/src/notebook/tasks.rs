//! 全库任务扫描:每篇笔记正文里的 `- [ ]` / `- [x]` 行,连同行号和任务文本。
//!
//! 用途是任务收集箱(Markio 的 `TaskInbox`):把散在几十篇笔记里的待办折成一张按
//! 时间 / 优先级 / 笔记分组的清单。**结构化解析(#标签、@截止、!优先级)在前端做** ——
//! 和标签云、字段浏览器同一个分工:那是纯计算,而筛选与分组会在同一份数据上反复算。
//!
//! ## 为什么这里可以用逐行正则,而阅读态的复选框不行
//!
//! `noteTasks.ts` 特意**不**用正则,它走 marked 的 token 树 —— 因为那一路的产出会被
//! 写回文件(勾选),而"哪些行是任务"一旦和渲染器有分歧,表现就是勾错别人那一行。
//!
//! 收集箱是**只读**的:它只把用户送到那一行,不改任何字节。所以这里认多了一条
//! (比如某种 marked 不认的缩进)最坏是清单里多出一条点过去看起来不像任务的条目,
//! 而不是静默的数据损坏。这个差别是刻意的,不是两边没对齐。
//!
//! 反过来说:**收集箱不能变成能勾选的**。真要加写回,必须先把那一行交给
//! `noteTasks.ts` 复核,而不是直接按这里给的行号落笔。
//!
//! ## 不算任务的地方
//!
//! - frontmatter 块(`tags:` 底下的 `- [ ]` 是 YAML 列表项,不是待办)
//! - 围栏代码块(教人怎么写 markdown 的笔记里满地都是 `- [ ] 示例`)
//! - 空壳 `- [ ]`(后面没有任何内容)—— marked 也不把它当任务,阅读态那里同样不出
//!   复选框。两边保持一致,否则收集箱里会多出一堆点进去是空行的条目。
//!
//! 围栏与 frontmatter 的判定共用 `vault_walk` 那一份,见那里的注释。

use std::path::Path;

use super::vault_walk::{fence_marker, frontmatter_lines, line_spans, walk_notes, WalkNext};

/// 单篇笔记记多少条任务。超出的丢掉 —— 一篇里 500 条待办已经不是手写出来的。
const MAX_TASKS_PER_FILE: usize = 500;

/// 全库任务上限。文件数 / 深度 / 单文件大小的上限在 `vault_walk` 那一层。
const MAX_TOTAL_TASKS: usize = 20_000;

/// 任务文本的字符上限。防御性截断,正常待办远短于此。
const MAX_TASK_CHARS: usize = 512;

/// 一篇笔记里的一条任务。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteTaskRef {
    /// 1-based 行号,按**整个 `.md` 文件**数(frontmatter 那几行也算)。
    ///
    /// 和标签 / 反链同一个坐标系,所以跳转能共用面板里那一条路。注意它和
    /// `noteTasks.ts` 的 `line`(按**正文**数,不含 frontmatter)不是一回事 ——
    /// 那一份是给勾选写回用的,两个坐标系不能混。
    pub line: u32,
    /// 已完成(`[x]` / `[X]`)。
    pub checked: bool,
    /// 任务文本:摘掉 `- [ ]` 前缀之后的原文,两端 trim。
    ///
    /// 标记(`#标签`、`@2026-09-01`、`!high`)**保留**在里面 —— 摘除交给前端那一层,
    /// 它同时要产出结构化字段和显示文本,在这里先摘一半会让两边的口径分家。
    pub text: String,
}

/// 一篇笔记的全部任务。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteTaskSource {
    /// 绝对路径,与笔记列表里的 `id` 同一个值。
    pub path: String,
    pub tasks: Vec<NoteTaskRef>,
}

/// 扫一遍 vault,返回每篇**有任务**的笔记及其任务。
pub(crate) fn scan_vault_tasks(root: &Path) -> Result<Vec<NoteTaskSource>, String> {
    let mut out = Vec::new();
    let mut total = 0usize;
    walk_notes(root, &mut |path, content| {
        let tasks = parse_tasks(content, MAX_TOTAL_TASKS - total);
        if tasks.is_empty() {
            return WalkNext::Continue;
        }
        total += tasks.len();
        out.push(NoteTaskSource {
            path: path.to_string_lossy().to_string(),
            tasks,
        });
        if total >= MAX_TOTAL_TASKS {
            return WalkNext::Stop;
        }
        WalkNext::Continue
    })?;
    // 按路径排序,和 `tags` / `fields` 同一个理由:两次扫描的顺序要一致,否则清单里
    // 那些同组任务的排列会随文件系统遍历顺序漂移。
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 解析一篇笔记里的任务,最多 `remaining` 条。
fn parse_tasks(content: &str, remaining: usize) -> Vec<NoteTaskRef> {
    let cap = remaining.min(MAX_TASKS_PER_FILE);
    if cap == 0 {
        return Vec::new();
    }
    let lines: Vec<(usize, &str)> = line_spans(content).collect();
    let skip = frontmatter_lines(&lines);
    let mut out: Vec<NoteTaskRef> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (index, (_, line)) in lines.iter().copied().enumerate().skip(skip) {
        if out.len() >= cap {
            break;
        }
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
        let Some((checked, text)) = task_line(line) else {
            continue;
        };
        out.push(NoteTaskRef {
            line: (index + 1) as u32,
            checked,
            text: truncate_chars(text, MAX_TASK_CHARS),
        });
    }
    out
}

/// 这一行是不是任务项。返回 (已完成, 任务文本)。
///
/// 认的前缀:`-` / `*` / `+` / `1.` / `1)`,可以带 `>` blockquote 前缀 —— 和
/// `noteTasks.ts` 里 `TASK_MARK_RE` 认的那一套一致(那些 marked 都会产复选框)。
fn task_line(line: &str) -> Option<(bool, &str)> {
    let mut rest = line;
    // blockquote 前缀:`> `、`>> `、`  >  > ` 都算。
    loop {
        let trimmed = rest.trim_start();
        match trimmed.strip_prefix('>') {
            Some(next) => rest = next,
            None => {
                rest = trimmed;
                break;
            }
        }
    }
    // 列表标记。有序列表的数字长度不限,但必须紧跟 `.` 或 `)`。
    let after_marker = match rest.chars().next()? {
        '-' | '*' | '+' => &rest[1..],
        c if c.is_ascii_digit() => {
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            let tail = &rest[digits..];
            let mut chars = tail.chars();
            match chars.next() {
                Some('.') | Some(')') => &tail[1..],
                _ => return None,
            }
        }
        _ => return None,
    };
    // 标记和 `[` 之间必须**有**空白:`-[ ] x` 不是列表项(marked 也不认)。
    let after_space = after_marker.strip_prefix([' ', '\t'])?;
    let body = after_space.trim_start();
    let mark = body.as_bytes().first()?;
    if *mark != b'[' {
        return None;
    }
    let checked = match body.as_bytes().get(1)? {
        b' ' => false,
        b'x' | b'X' => true,
        // `[-]` / `[/]` 这些是别家扩展的"进行中"记法,marked 不认,阅读态也不出
        // 复选框 —— 收集箱跟着不认。
        _ => return None,
    };
    if *body.as_bytes().get(2)? != b']' {
        return None;
    }
    let text = body[3..].trim();
    // 空壳 `- [ ]` 不算任务:marked 的 `token.task` 在那里是 false,阅读态不出复选框。
    if text.is_empty() {
        return None;
    }
    Some((checked, text))
}

/// 按**字符**截断并加省略号(不按字节 —— 中文会切在半个字上)。
fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tasks_of(content: &str) -> Vec<(u32, bool, String)> {
        parse_tasks(content, MAX_TOTAL_TASKS)
            .into_iter()
            .map(|task| (task.line, task.checked, task.text))
            .collect()
    }

    #[test]
    fn collects_unchecked_and_checked() {
        let tasks = tasks_of("- [ ] 写周报\n- [x] 交报销\n- [X] 大写也算\n");
        assert_eq!(
            tasks,
            vec![
                (1, false, "写周报".to_string()),
                (2, true, "交报销".to_string()),
                (3, true, "大写也算".to_string()),
            ]
        );
    }

    #[test]
    fn line_numbers_count_the_whole_file() {
        // frontmatter 那几行也算进行号 —— 跳转和标签 / 反链共用同一个坐标系。
        let tasks = tasks_of("---\ntitle: 计划\n---\n\n- [ ] 第一条\n");
        assert_eq!(tasks, vec![(5, false, "第一条".to_string())]);
    }

    #[test]
    fn skips_frontmatter_list_items() {
        // frontmatter 里的 `- [ ]` 是 YAML 列表项,不是待办。
        let tasks = tasks_of("---\nchecklist:\n  - [ ] 不是待办\n---\n- [ ] 是待办\n");
        assert_eq!(tasks, vec![(5, false, "是待办".to_string())]);
    }

    #[test]
    fn unclosed_frontmatter_is_a_divider() {
        // 未闭合的 `---` 不算 frontmatter,那多半是一条分隔线 —— 后面的任务照常算。
        let tasks = tasks_of("---\n- [ ] 照常算\n");
        assert_eq!(tasks, vec![(2, false, "照常算".to_string())]);
    }

    #[test]
    fn skips_fenced_blocks() {
        let src = "- [ ] 真的\n```md\n- [ ] 教程里的例子\n```\n- [x] 也是真的\n";
        assert_eq!(
            tasks_of(src),
            vec![
                (1, false, "真的".to_string()),
                (5, true, "也是真的".to_string())
            ]
        );
    }

    #[test]
    fn nested_fence_needs_a_long_enough_close() {
        // ```` 里的 ``` 不该提前结束围栏,否则后面那条会被当成正文。
        let src = "````\n```\n- [ ] 围栏里\n```\n````\n- [ ] 围栏外\n";
        assert_eq!(tasks_of(src), vec![(6, false, "围栏外".to_string())]);
    }

    #[test]
    fn tilde_fence_is_not_closed_by_backticks() {
        let src = "~~~\n- [ ] 波浪围栏里\n```\n- [ ] 还在里面\n~~~\n- [ ] 出来了\n";
        assert_eq!(tasks_of(src), vec![(6, false, "出来了".to_string())]);
    }

    #[test]
    fn accepts_ordered_and_blockquote_markers() {
        let src = "1. [ ] 有序点\n2) [x] 圆括号\n> - [ ] 引用里\n>> - [x] 双层引用\n";
        assert_eq!(
            tasks_of(src),
            vec![
                (1, false, "有序点".to_string()),
                (2, true, "圆括号".to_string()),
                (3, false, "引用里".to_string()),
                (4, true, "双层引用".to_string()),
            ]
        );
    }

    #[test]
    fn accepts_indented_and_multi_digit_markers() {
        let src = "- [ ] 外层\n  - [ ] 缩进的\n12. [x] 两位数\n";
        assert_eq!(
            tasks_of(src),
            vec![
                (1, false, "外层".to_string()),
                (2, false, "缩进的".to_string()),
                (3, true, "两位数".to_string()),
            ]
        );
    }

    #[test]
    fn rejects_non_task_lines() {
        // 普通列表项、没有空格的 `-[ ]`、别家的 `[-]` / `[/]`、空壳 `- [ ]`。
        let src = "- 普通项\n-[ ] 没空格\n- [-] 进行中\n- [/] 也是别家的\n- [ ]\n- [ ]   \n";
        assert_eq!(tasks_of(src), Vec::new());
    }

    #[test]
    fn keeps_markers_in_the_text() {
        // 结构化解析在前端做:这里保留 `#标签` / `@日期` / `!优先级` 的原文。
        let tasks = tasks_of("- [ ] 交稿 #写作 @2026-09-01 !high\n");
        assert_eq!(
            tasks,
            vec![(1, false, "交稿 #写作 @2026-09-01 !high".to_string())]
        );
    }

    #[test]
    fn handles_crlf() {
        let tasks = tasks_of("- [ ] 第一条\r\n- [x] 第二条\r\n");
        assert_eq!(
            tasks,
            vec![
                (1, false, "第一条".to_string()),
                (2, true, "第二条".to_string())
            ]
        );
    }

    #[test]
    fn truncates_long_text_on_char_boundaries() {
        let long = "字".repeat(MAX_TASK_CHARS + 50);
        let tasks = tasks_of(&format!("- [ ] {long}\n"));
        let text = &tasks[0].2;
        assert_eq!(text.chars().count(), MAX_TASK_CHARS + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn caps_tasks_per_file() {
        let mut src = String::new();
        for index in 0..MAX_TASKS_PER_FILE + 20 {
            src.push_str(&format!("- [ ] 第 {index} 条\n"));
        }
        assert_eq!(parse_tasks(&src, MAX_TOTAL_TASKS).len(), MAX_TASKS_PER_FILE);
    }

    #[test]
    fn respects_the_remaining_budget() {
        let src = "- [ ] 一\n- [ ] 二\n- [ ] 三\n";
        assert_eq!(parse_tasks(src, 2).len(), 2);
        assert_eq!(parse_tasks(src, 0).len(), 0);
    }
}
