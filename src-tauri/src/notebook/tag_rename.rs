//! 跨文件把 `#旧标签` 改成 `#新标签`,带 changed / skipped / failed 报告。
//!
//! ## 为什么和索引共用一个词法器
//!
//! Markio 的 `tag-ops.ts` 用一条**新的**正则找要改的位置
//! (`(?<![#\w一-鿿])#tag(?![\w一-鿿./-])`),而标签云用另一套字符扫。
//! 两者不等价,后果不是"少改了几处"这么中性:
//!
//! - 索引把 `##heading` 收成标签 `heading`,重命名的 lookbehind 把它排除 —— 用户看到
//!   "3 处",改完只动了 2 处,剩下那 1 处在哪没人说得清。
//! - 那条正则**不跳代码块**,于是 ```` ```sh #work ```` 里的字样会被改掉:一次重命名
//!   顺手改了脚本内容。
//!
//! 这里的做法是:重命名不认识"什么是标签",它只接受 `tags::tag_hits` 给出的字节区间,
//! 按区间原地替换。数得出来的一定改得动,改的一定是数出来的那几处 —— 同一份代码保证。
//!
//! ## 逐处失败都收,不在第一个失败处停
//!
//! Markio 遇到第一个保存失败就 `return`,剩下的文件既没改也没进报告。用户拿到的是
//! "改了 4 个,第 5 个失败" —— 后面还有几个、有没有别的冲突,要再点一次才知道,而
//! 再点一次时前 4 个已经改过了,报告的含义又变了。
//!
//! 这里每篇独立处理,失败的进 `failed` 继续往下走。一次操作后用户看到的是完整的一
//! 张账:改了哪些、跳过哪些、哪些没成功。
//!
//! ## 只改精确匹配的那一个标签,不动子标签
//!
//! 改 `#work` 不会碰 `#work/deep`。理由是面板里它们是**两行**、各有各的处数:改
//! `#work` 那一行时用户看到的数字是 3,那就该正好改 3 处。要连带改子标签的话得先在
//! 界面上说清会波及哪些行,那是另一个功能。
//!
//! ## 两趟:先定名单,再逐篇读-改-写
//!
//! 遍历时不写盘 —— 边遍历边改目录内容在某些平台上行为未定义。而且第二趟要**重新读**
//! 一次:第一趟的内容是遍历时的快照,直接拿它改写会把这中间别人的编辑覆盖掉。第二趟
//! 走 `read_note` + `save_note(expected)`,冲突检测和版本快照都是既有那一套。

use std::path::Path;

use super::fs_ops::{read_note, save_note, SaveOutcome};
use super::state::{FileSig, NotebookState};
use super::tags::{normalize_key, tag_hits, validate_tag};
use super::vault_walk::{walk_notes, WalkNext};

/// 一次重命名最多碰多少篇。超出的进 `skipped`,理由是 `TooManyFiles`。
///
/// 上限存在的理由不是性能,是**可回滚性**:每篇都会留一条版本快照,几千篇的话用户想
/// 撤回得一篇篇翻历史。真要改这么大范围,分几次改反而看得清。
const MAX_FILES_PER_RENAME: usize = 500;

/// 一篇被改过的笔记。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRenameChange {
    pub path: String,
    /// 这篇里改掉了几处。前端把它们加起来和面板上的处数对账。
    pub count: usize,
}

/// 一篇被跳过的笔记,连同**为什么**。
///
/// 跳过的理由必须报出来,不能只给个路径:"这篇里明明有 #work,怎么没改"是重命名之后
/// 最常见的疑问,而答案(在代码块里 / 在 frontmatter 里 / 是 `##work`)只有扫描器
/// 知道。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRenameSkip {
    pub path: String,
    pub reason: TagSkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TagSkipReason {
    /// 有 `#旧标签` 的字样,但没有一处算标签(代码块 / frontmatter / `##` 之后)。
    NotATag,
    /// 第二趟重读时那些标签已经不在了 —— 中间被别人改过或删掉了。
    Vanished,
    /// 超过一次重命名的文件数上限。
    TooManyFiles,
}

/// 一篇没改成的笔记。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRenameFailure {
    pub path: String,
    pub message: String,
}

/// 一次重命名的完整报告。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRenameReport {
    pub changed: Vec<TagRenameChange>,
    pub skipped: Vec<TagRenameSkip>,
    pub failed: Vec<TagRenameFailure>,
}

/// 把全库的 `#old` 改成 `#new`。
///
/// `old` 按归一化 key 匹配(大小写不敏感,与面板里那一行的聚合口径一致);`new` 是
/// 要写进文件的**字面文本**,大小写照原样落盘。
pub(crate) fn rename_vault_tag(
    state: &NotebookState,
    root: &Path,
    old: &str,
    new: &str,
) -> Result<TagRenameReport, String> {
    let old_key = normalize_key(old);
    if old_key.is_empty() {
        return Err("The tag to rename is empty".to_string());
    }
    // 新名字必须**过同一个词法器**才收:写进去一个自己都扫不出来的标签,下一次打开
    // 面板它就消失了,而文件已经改完。这是 `validate_tag` 存在的全部理由。
    let new_tag = validate_tag(new)?;
    if normalize_key(&new_tag) == old_key && new_tag == old_key {
        return Err("The new tag is the same as the old one".to_string());
    }

    // 第一趟:只定名单。遍历时不写盘 —— 边走边改目录内容在某些平台上行为未定义。
    let mut targets: Vec<String> = Vec::new();
    let mut report = TagRenameReport::default();
    let mut overflow: Vec<String> = Vec::new();
    walk_notes(root, &mut |path, content| {
        let text = path.to_string_lossy().to_string();
        match plan_rewrite(content, &old_key, &new_tag) {
            Some(_) => {
                if targets.len() >= MAX_FILES_PER_RENAME {
                    overflow.push(text);
                } else {
                    targets.push(text);
                }
            }
            None => {
                /* 没有一处要改。只有"看起来该改却没改"才值得进报告 —— 全库大部分
                笔记根本不含这个标签,把它们都列出来等于没列。 */
                if mentions_tag_text(content, &old_key) {
                    report.skipped.push(TagRenameSkip {
                        path: text,
                        reason: TagSkipReason::NotATag,
                    });
                }
            }
        }
        WalkNext::Continue
    })?;

    for path in overflow {
        report.skipped.push(TagRenameSkip {
            path,
            reason: TagSkipReason::TooManyFiles,
        });
    }

    // 第二趟:逐篇重读再改写。第一趟的内容是遍历时的快照,直接拿它写会把这中间
    // 别人的编辑覆盖掉。
    for path in targets {
        match rewrite_one(state, &path, &old_key, &new_tag) {
            Ok(Some(count)) => report.changed.push(TagRenameChange { path, count }),
            Ok(None) => report.skipped.push(TagRenameSkip {
                path,
                reason: TagSkipReason::Vanished,
            }),
            // 一篇失败不中断:用户要的是一张完整的账,不是"到这里为止"。
            Err(message) => report.failed.push(TagRenameFailure { path, message }),
        }
    }

    report.changed.sort_by(|a, b| a.path.cmp(&b.path));
    report.skipped.sort_by(|a, b| a.path.cmp(&b.path));
    report.failed.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(report)
}

/// 读一篇、改写、存盘。`Ok(None)` = 重读之后已经没有要改的了。
fn rewrite_one(
    state: &NotebookState,
    path: &str,
    old_key: &str,
    new_tag: &str,
) -> Result<Option<usize>, String> {
    let opened = read_note(state, path)?;
    let Some((next, count)) = plan_rewrite(&opened.content, old_key, new_tag) else {
        return Ok(None);
    };
    write_rewritten(state, path, &next, opened.sig)?;
    Ok(Some(count))
}

/// 把改写后的内容写回去,`baseline` 是**读那一刻**的指纹。
///
/// 单独成函数是为了能被直接测到。这是整个重命名里唯一会静默丢别人数据的一步:第一趟
/// 遍历和第二趟重读之间、重读和写回之间,都可能有别的编辑挤进来。守卫是那个
/// `Some(baseline)` 加 `force: false` —— 少了任一个,冲突就变成覆盖,而覆盖是没有
/// 报错的,用户要等到打开笔记才发现自己的段落不见了。
pub(super) fn write_rewritten(
    state: &NotebookState,
    path: &str,
    content: &str,
    baseline: FileSig,
) -> Result<(), String> {
    // 版本快照由 `save_note` 一并记下,所以每一篇都能单独撤回。
    match save_note(state, path, content, Some(baseline), false)? {
        SaveOutcome::Saved { .. } => Ok(()),
        // 冲突不写盘,报成这一篇的 failure。整批不中断 —— 别的笔记该改还是要改。
        SaveOutcome::Conflict { .. } => {
            Err("The note changed on disk while renaming; nothing was written".to_string())
        }
    }
}

/// 按 `tag_hits` 的区间改写全篇。`None` = 没有一处匹配。
///
/// 这个函数是本模块的核心,也是它和 Markio 唯一的实质差别:它**不认识**什么是标签,
/// 只按扫描器给的区间替换。想让它漏改或错改,得先让索引也一起漏或错。
fn plan_rewrite(content: &str, old_key: &str, new_tag: &str) -> Option<(String, usize)> {
    let hits: Vec<_> = tag_hits(content)
        .into_iter()
        .filter(|hit| normalize_key(&hit.raw) == old_key)
        .collect();
    if hits.is_empty() {
        return None;
    }
    // 大小写不敏感匹配意味着 `#Work` → `#work` 是一次真实改动,但 `#work` → `#work`
    // 不是。逐处比对字面文本,全都已经是新名字就当没匹配。
    if hits.iter().all(|hit| hit.raw == new_tag) {
        return None;
    }
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    let mut count = 0usize;
    for hit in &hits {
        if hit.raw == new_tag {
            continue;
        }
        out.push_str(&content[cursor..hit.start]);
        out.push('#');
        out.push_str(new_tag);
        cursor = hit.end;
        count += 1;
    }
    out.push_str(&content[cursor..]);
    Some((out, count))
}

/// 这篇里有没有 `#旧标签` 的**字样**(不问它算不算标签)。
///
/// 只用来决定"要不要把这篇报成 skipped"。大小写不敏感,和匹配口径一致。
fn mentions_tag_text(content: &str, old_key: &str) -> bool {
    let needle = format!("#{old_key}");
    content.to_lowercase().contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只验改写那一半(不碰磁盘)。返回 (新内容, 改了几处)。
    fn rewrite(content: &str, old: &str, new: &str) -> Option<(String, usize)> {
        plan_rewrite(content, &normalize_key(old), new)
    }

    #[test]
    fn rewrites_every_hit_the_index_would_count() {
        let content = "#work 一处\n中间 #work 又一处\n";
        let (next, count) = rewrite(content, "work", "job").unwrap();
        assert_eq!(count, 2);
        assert_eq!(next, "#job 一处\n中间 #job 又一处\n");
    }

    #[test]
    fn leaves_code_blocks_and_frontmatter_alone() {
        /* Markio 那条重命名正则不跳代码块,于是一次重命名会顺手改掉脚本内容。这里
        改写只认扫描器给的区间,而扫描器本来就跳这些地方。 */
        let content = "---\ntags: [work]\n# 注释 #work\n---\n\n```sh\n#work 在代码里\n```\n\n`#work` 行内\n\n#work 只有这一处\n";
        let (next, count) = rewrite(content, "work", "job").unwrap();
        assert_eq!(count, 1);
        assert!(next.contains("tags: [work]"));
        assert!(next.contains("# 注释 #work"));
        assert!(next.contains("#work 在代码里"));
        assert!(next.contains("`#work` 行内"));
        assert!(next.contains("#job 只有这一处"));
    }

    #[test]
    fn does_not_touch_double_hash_headings() {
        // 索引也不把它当标签 —— 两边同一个判断,所以不会出现"数得出来改不动"。
        assert!(rewrite("##work 是标题\n", "work", "job").is_none());
    }

    #[test]
    fn keeps_child_tags_intact() {
        // 改 `#work` 时面板上那一行写的是 `#work` 自己的处数,就该正好改那么多处。
        let content = "#work 父\n#work/deep 子\n";
        let (next, count) = rewrite(content, "work", "job").unwrap();
        assert_eq!(count, 1);
        assert_eq!(next, "#job 父\n#work/deep 子\n");
    }

    #[test]
    fn matches_case_insensitively_but_writes_the_given_case() {
        // 面板按归一化 key 聚合(`#Work` 和 `#work` 同一行),重命名必须跟着这个口径。
        let content = "#Work 大写\n#work 小写\n";
        let (next, count) = rewrite(content, "WORK", "Job").unwrap();
        assert_eq!(count, 2);
        assert_eq!(next, "#Job 大写\n#Job 小写\n");
    }

    #[test]
    fn skips_hits_that_are_already_the_new_text() {
        /* `#Work` → `#work` 只该改大写那一处。全都已经是新名字时当没匹配 —— 否则
        一次空操作也会留下版本快照,把 30 条的保留窗口冲掉。 */
        let (next, count) = rewrite("#Work 大写\n#work 已经是了\n", "work", "work").unwrap();
        assert_eq!(count, 1);
        assert_eq!(next, "#work 大写\n#work 已经是了\n");
        assert!(rewrite("#work 已经是了\n", "work", "work").is_none());
    }

    #[test]
    fn trailing_separators_stay_outside_the_replaced_span() {
        // `#work/` 的斜杠不在标签区间里,改完还是斜杠 —— 吃掉它会改变那一行的意思。
        let (next, _) = rewrite("#work/ 尾巴\n", "work", "job").unwrap();
        assert_eq!(next, "#job/ 尾巴\n");
    }

    #[test]
    fn survives_multibyte_and_crlf() {
        // 区间是**字节**偏移。按字符算会在 CJK 和 CRLF 上切错位置。
        let content = "开头 #周报 中间\r\n第二行 #周报\r\n";
        let (next, count) = rewrite(content, "周报", "月报").unwrap();
        assert_eq!(count, 2);
        assert_eq!(next, "开头 #月报 中间\r\n第二行 #月报\r\n");
    }

    #[test]
    fn mentions_check_is_case_insensitive() {
        // skipped 的判定口径要和匹配一致,否则 `#WORK` 那篇会连 skipped 都不进。
        assert!(mentions_tag_text("看 #WORK 这里", "work"));
        assert!(!mentions_tag_text("看 work 这里", "work"));
    }
}
