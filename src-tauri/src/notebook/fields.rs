//! 全库 frontmatter 字段扫描:每篇笔记有哪些 key、各自是什么值。
//!
//! 用途是字段浏览器(Markio 的 PropertyExplorer):全库有哪些 key、某个 key 都被写
//! 成过哪些值、某个 `key=value` 命中哪几篇。聚合放在前端 —— 和标签云同一个理由,
//! 那是纯计算,而筛选 / 排序会在同一份数据上反复算。
//!
//! **frontmatter 的边界与标题索引共用一份**(`vault_index::split_frontmatter`)。
//! 抄第二份的后果不是重复代码:两处对"开了 `---` 没闭合算不算 frontmatter"给出不
//! 同答案时,字段浏览器会显示 `title: X`,而笔记列表同一篇显示文件名 —— 用户看到
//! 的是两个视图互相矛盾,而不是某一处解析错了。引号标量的还原(`unquote_scalar`)
//! 同理:值在浏览器里必须和标题栏里长得一样。
//!
//! 只认**顶层** `key: value`。缩进行只在它是 `- item` 时被当作上一个 key 的块式
//! 列表项,其余缩进内容(嵌套映射、块标量的正文)整段跳过 —— 把 `meta.author` 的
//! 内层 key 摊平到顶层会和真的顶层 `author` 撞在一起,那种冲突在界面上无法解释。

use std::collections::BTreeMap;
use std::path::Path;

use super::vault_index::{split_frontmatter, unquote_scalar};
use super::vault_walk::{walk_notes, WalkNext};

/// 单篇笔记记多少个字段。超出的丢掉 —— 一篇 frontmatter 里 200 个 key 已经不是手写
/// 出来的。
const MAX_FIELDS_PER_FILE: usize = 200;

/// 单个 key 记多少个值。块式列表可以很长,但浏览器一次也看不了那么多。
const MAX_VALUES_PER_FIELD: usize = 200;

/// 全库字段总数上限(按"篇×key"计)。文件数 / 深度 / 单文件大小的上限在
/// `vault_walk` 那一层。
const MAX_TOTAL_FIELDS: usize = 20_000;

/// key 的字符上限。超长的多半是把正文一行误当成了 `key: value`。
const MAX_KEY_CHARS: usize = 128;

/// 值的字符上限。和标签一样只是防御性截断,正常的 frontmatter 值远短于此。
const MAX_VALUE_CHARS: usize = 512;

/// 一篇笔记里的一个 frontmatter 字段。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteField {
    /// key 的原始文本,保持大小写。归一化(大小写不敏感的聚合)在前端做。
    pub key: String,
    /// 这个 key 在这一篇里的值。标量是一个;列表是多个;
    /// `key:` 后面什么都没有(空值 / 只有嵌套内容)时是空数组 —— 那仍然算
    /// "这篇有这个 key",所以字段本身要出现。
    pub values: Vec<String>,
}

/// 一篇笔记的全部 frontmatter 字段。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFieldSource {
    /// 绝对路径,与笔记列表里的 `id` 同一个值。
    pub path: String,
    pub fields: Vec<NoteField>,
}

/// 扫一遍 vault,返回每篇**有 frontmatter 字段**的笔记及其字段。
pub(crate) fn scan_vault_fields(root: &Path) -> Result<Vec<NoteFieldSource>, String> {
    let mut out = Vec::new();
    let mut total = 0usize;
    walk_notes(root, &mut |path, content| {
        // frontmatter 必须从文件第一个字节开始,不以 `---` 开头的整篇不用解析。
        if !content.starts_with("---\n") {
            return WalkNext::Continue;
        }
        let fields = parse_fields(content, MAX_TOTAL_FIELDS - total);
        if fields.is_empty() {
            return WalkNext::Continue;
        }
        total += fields.len();
        out.push(NoteFieldSource {
            path: path.to_string_lossy().to_string(),
            fields,
        });
        if total >= MAX_TOTAL_FIELDS {
            return WalkNext::Stop;
        }
        WalkNext::Continue
    })?;
    // 按路径排序,和 `vault_index` / `tags` 同一个理由:两次扫描的顺序要一致,否则
    // 字段详情里那串笔记的排列会随文件系统遍历顺序漂移。
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 解析一篇笔记的 frontmatter 字段,最多 `remaining` 个。
///
/// 同一个 key 出现多次时值合并(YAML 里那是重复 key,但既然文件里写了,浏览器该把
/// 两个值都显示出来,而不是静默只留后一个)。
fn parse_fields(content: &str, remaining: usize) -> Vec<NoteField> {
    let cap = remaining.min(MAX_FIELDS_PER_FILE);
    if cap == 0 {
        return Vec::new();
    }
    let (front, _) = split_frontmatter(content);
    if front.is_empty() {
        return Vec::new();
    }
    // 顺序按首次出现,不按字母序:frontmatter 的书写顺序是作者的信息。
    let mut order: Vec<String> = Vec::new();
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // 当前顶层 key —— 块式列表(`- item`)挂在它上面。
    let mut current: Option<String> = None;
    for line in front.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let trimmed = line.trim();
        // 缩进的 `- item`:上一个 key 的块式列表项。
        if indented || trimmed.starts_with("- ") || trimmed == "-" {
            if let Some(item) = trimmed.strip_prefix('-').map(str::trim) {
                if item.is_empty() {
                    continue;
                }
                let Some(key) = current.as_ref() else {
                    continue;
                };
                if let Some(values) = map.get_mut(key) {
                    push_value(values, unquote_scalar(item));
                }
            }
            // 其余缩进内容(嵌套映射、块标量正文)不摊平,见模块注释。
            continue;
        }
        // 注释行。`#` 在 YAML 里只有行首或空白后才是注释;这里只处理行首那种,
        // 行内的 ` #` 一律留在值里 —— `title: Release # 5` 的 `# 5` 是标题的一部
        // 分,砍掉它会让浏览器显示的值和文件里的不一样。
        if trimmed.starts_with('#') {
            continue;
        }
        let Some((raw_key, rest)) = trimmed.split_once(':') else {
            // 顶层的非 `key: value` 行(裸标量、`---` 之外的分隔线)跳过。
            continue;
        };
        let key = raw_key.trim();
        if !is_field_key(key) {
            continue;
        }
        if !map.contains_key(key) && order.len() >= cap {
            // 额度用完:后面的新 key 不再收,但已收的 key 仍能继续攒值。
            current = None;
            continue;
        }
        let entry = map.entry(key.to_string()).or_insert_with(|| {
            order.push(key.to_string());
            Vec::new()
        });
        let value = rest.trim();
        if let Some(items) = parse_inline_list(value) {
            for item in items {
                push_value(entry, item);
            }
        } else if !value.is_empty() {
            push_value(entry, unquote_scalar(value));
        }
        current = Some(key.to_string());
    }
    order
        .into_iter()
        .map(|key| {
            let values = map.remove(&key).unwrap_or_default();
            NoteField { key, values }
        })
        .collect()
}

/// 收一个值:截断、去重、按上限封顶。
///
/// 去重是按篇按 key 去的 —— 同一篇里 `tags: [a, a]` 算一次。计数口径是"多少篇写了
/// 这个值",重复出现不带增量信息。
fn push_value(values: &mut Vec<String>, value: String) {
    if value.is_empty() || values.len() >= MAX_VALUES_PER_FIELD {
        return;
    }
    let value = truncate_chars(&value, MAX_VALUE_CHARS);
    if values.iter().any(|existing| *existing == value) {
        return;
    }
    values.push(value);
}

/// 行内列表 `[a, b, c]` → 各项。不是行内列表时返回 `None`。
///
/// 逗号分项、不处理嵌套 —— frontmatter 里的行内列表就是标签 / 别名那种一层结构。
/// `[]` 是合法的空列表,返回空 `Vec` 而不是 `None`:那和 `key:` 一样表示"有这个
/// key、没有值"。
fn parse_inline_list(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(unquote_scalar)
            .collect(),
    )
}

/// 这串文本能不能当 frontmatter 的 key。
///
/// 刻意**不**限制成 ASCII 标识符:`作者: 张三` 是中文笔记里再正常不过的写法,按
/// ASCII 过滤会让它整条消失,而界面上看不出为什么。拦掉的是明显不是 key 的形状 ——
/// 空串、markdown 列表 / 注释的行首、含空白(顶层 `key: value` 的 key 不带空格,
/// 带空格的多半是正文里一句带冒号的话)、超长。
fn is_field_key(key: &str) -> bool {
    if key.is_empty() || key.chars().count() > MAX_KEY_CHARS {
        return false;
    }
    if key.starts_with('-') || key.starts_with('#') {
        return false;
    }
    !key.chars().any(char::is_whitespace)
}

/// 按字符截断,超长加省略号。
fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut out: String = value.chars().take(limit).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields_of(content: &str) -> Vec<(String, Vec<String>)> {
        parse_fields(content, MAX_TOTAL_FIELDS)
            .into_iter()
            .map(|field| (field.key, field.values))
            .collect()
    }

    #[test]
    fn reads_top_level_scalars_in_written_order() {
        let fields = fields_of("---\ntitle: 周报\nstatus: done\n---\n正文\n");
        assert_eq!(
            fields,
            vec![
                ("title".to_string(), vec!["周报".to_string()]),
                ("status".to_string(), vec!["done".to_string()]),
            ]
        );
    }

    #[test]
    fn unquotes_scalars_the_same_way_the_title_index_does() {
        let fields = fields_of("---\ntitle: \"引号\\\"里\"\nalias: 'it''s'\n---\n");
        assert_eq!(fields[0].1, vec!["引号\"里".to_string()]);
        assert_eq!(fields[1].1, vec!["it's".to_string()]);
    }

    #[test]
    fn reads_inline_and_block_lists() {
        let fields = fields_of("---\ntags: [a, b]\nrefs:\n  - x\n  - y\n---\n");
        assert_eq!(fields[0].1, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(fields[1].1, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn keeps_a_key_that_has_no_value() {
        // `key:` 空值和 `[]` 空列表都算"这篇有这个 key"。字段浏览器要能看见它们,
        // 否则"哪些笔记缺 status"这种问题答不出来。
        let fields = fields_of("---\nstatus:\ntags: []\n---\n");
        assert_eq!(
            fields,
            vec![
                ("status".to_string(), Vec::<String>::new()),
                ("tags".to_string(), Vec::<String>::new()),
            ]
        );
    }

    #[test]
    fn keeps_inline_hash_inside_a_value() {
        // ` #` 之后的内容是值的一部分。Markio 在这里砍成了注释,于是
        // `title: Release # 5` 在浏览器里显示成 `Release`,和文件里不一样。
        let fields = fields_of("---\ntitle: Release # 5\n---\n");
        assert_eq!(fields[0].1, vec!["Release # 5".to_string()]);
    }

    #[test]
    fn skips_leading_comment_lines() {
        let fields = fields_of("---\n# 这是注释: 不是字段\ntitle: 真的\n---\n");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "title");
    }

    #[test]
    fn accepts_non_ascii_keys() {
        // 中文 key 必须能出现 —— 按 ASCII 标识符过滤会让它整条消失。
        let fields = fields_of("---\n作者: 张三\n---\n");
        assert_eq!(fields, vec![("作者".to_string(), vec!["张三".to_string()])]);
    }

    #[test]
    fn rejects_shapes_that_are_not_keys() {
        // 含空白的("正文里一句话: 带冒号")、行首 `-` 的,都不是顶层 key。
        assert!(!is_field_key(""));
        assert!(!is_field_key("a b"));
        assert!(!is_field_key("- item"));
        assert!(!is_field_key("#tag"));
        assert!(is_field_key("题目"));
        assert!(!is_field_key(&"k".repeat(MAX_KEY_CHARS + 1)));
    }

    #[test]
    fn does_not_flatten_nested_maps() {
        // `author` 只有一个:内层的 `name` 不能摊到顶层,否则和真的顶层 `name`
        // 撞在一起。
        let fields = fields_of("---\nmeta:\n  name: 内层\nname: 顶层\n---\n");
        assert_eq!(
            fields,
            vec![
                ("meta".to_string(), Vec::<String>::new()),
                ("name".to_string(), vec!["顶层".to_string()]),
            ]
        );
    }

    #[test]
    fn merges_duplicate_keys_instead_of_keeping_the_last() {
        let fields = fields_of("---\ntag: a\ntag: b\n---\n");
        assert_eq!(
            fields,
            vec![("tag".to_string(), vec!["a".to_string(), "b".to_string()])]
        );
    }

    #[test]
    fn dedupes_values_within_one_note() {
        let fields = fields_of("---\ntags: [a, a, b]\n---\n");
        assert_eq!(fields[0].1, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn ignores_a_frontmatter_that_never_closes() {
        // 和标题索引同一个口径:开了 `---` 没闭合的是正文里的分隔线。
        assert!(fields_of("---\ntitle: 假的\n没有闭合\n").is_empty());
    }

    #[test]
    fn ignores_frontmatter_that_does_not_start_at_byte_zero() {
        assert!(fields_of("前言\n---\ntitle: 假的\n---\n").is_empty());
    }

    #[test]
    fn caps_fields_per_note_but_keeps_filling_known_keys() {
        let mut src = String::from("---\nkept: a\n");
        for i in 0..MAX_FIELDS_PER_FILE + 10 {
            src.push_str(&format!("k{i}: v\n"));
        }
        src.push_str("kept: b\n---\n");
        let fields = parse_fields(&src, MAX_TOTAL_FIELDS);
        assert_eq!(fields.len(), MAX_FIELDS_PER_FILE);
        // 额度用完之后 `kept` 仍然收到了第二个值。
        assert_eq!(fields[0].key, "kept");
        assert_eq!(fields[0].values, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn honours_the_remaining_budget() {
        let fields = parse_fields("---\na: 1\nb: 2\nc: 3\n---\n", 2);
        assert_eq!(fields.len(), 2);
        assert_eq!(parse_fields("---\na: 1\n---\n", 0).len(), 0);
    }

    #[test]
    fn truncates_an_overlong_value() {
        let long = "值".repeat(MAX_VALUE_CHARS + 50);
        let fields = fields_of(&format!("---\nk: {long}\n---\n"));
        let value = &fields[0].1[0];
        assert_eq!(value.chars().count(), MAX_VALUE_CHARS + 1);
        assert!(value.ends_with('…'));
    }

    #[test]
    fn caps_values_per_field() {
        let mut src = String::from("---\nlist:\n");
        for i in 0..MAX_VALUES_PER_FIELD + 20 {
            src.push_str(&format!("  - v{i}\n"));
        }
        src.push_str("---\n");
        assert_eq!(fields_of(&src)[0].1.len(), MAX_VALUES_PER_FIELD);
    }
}
