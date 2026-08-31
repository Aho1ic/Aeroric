//! 用户自定义模板:扫 `<vault>/.notebook/templates/*.md`。
//!
//! 内置模板(日记 / 周报 / OKR / 会议…)在前端 `noteTemplates.ts` 里,文案走 i18n。
//! 这一层解决的是内置那几个覆盖不到的场景:每个团队的会议纪要、复盘、值班记录格式
//! 都不一样,而"把自己的格式放进去"不该要求改代码。
//!
//! 落在 `.notebook/` 下面而不是 vault 根:那个目录本来就是 vault 私有数据(历史快照、
//! 回收站、排序),既不入 Git 也不会被 `read_tree` 扫成笔记 —— 模板放到根下面的话
//! 它们自己会出现在笔记列表里,而模板不是笔记。
//!
//! frontmatter 只读三个标量:
//! - `title`:面板里显示的名字,缺省用文件名 stem
//! - `name`:新建时的默认文件名,可含占位符,缺省用 title
//! - `body`:frontmatter 之后的全部内容,原样回传
//!
//! 占位符(`{{date}}` / `{{time}}` / `{{title}}`)**不在这里**替换:日期要按用户本地
//! 时区算,而后端不知道 webview 的时区(容器里跑 UTC 的情况很常见)。替换在前端
//! `noteTemplates.ts` 做,那份有测试。
//!
//! 读不出来的条目一律跳过而不是报错:一个坏文件不该让整份模板列表打不开 —— 那会
//! 让「模板没了」看起来像功能坏了,而不是某个文件坏了。

use std::fs;
use std::path::Path;

use super::fs_ops::{is_note_file, private_dir};
use super::vault_index::{split_frontmatter, unquote_scalar};

/// 模板目录名(在 `.notebook/` 下)。
const TEMPLATES_DIR: &str = "templates";

/// 单个模板的读取上限。模板是给人抄格式的骨架,不是数据文件。
const MAX_BYTES: u64 = 256 * 1024;

/// 列表上限。目录里塞了几千个文件时不必全读 —— 面板也放不下。
const MAX_TEMPLATES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTemplate {
    /// 文件名 stem,作为稳定 id(前端命令 id 是 `template.user:<id>`)。
    pub id: String,
    /// 显示标题:frontmatter `title`,缺省用 stem。
    pub title: String,
    /// 默认文件名(可含占位符):frontmatter `name`,缺省用 title。
    pub name: String,
    /// 模板正文(frontmatter 之后的部分,原样)。
    pub body: String,
}

/// 从 frontmatter 里取一个标量。缺失或空串都当没有 —— `title:` 写了但留空时
/// 回落到 stem 比显示一条无名条目有用。
fn field(front: &str, key: &str) -> Option<String> {
    for line in front.lines() {
        // 没有冒号的行跳过就好(YAML 的续行、注释)。用 `?` 提前返回会让它**之后**
        // 的字段全部读不到。
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case(key) {
            continue;
        }
        let scalar = unquote_scalar(value.trim());
        return if scalar.is_empty() {
            None
        } else {
            Some(scalar)
        };
    }
    None
}

/// 把一个模板文件的内容解析成 `UserTemplate`。
pub(crate) fn parse_template(stem: &str, content: &str) -> UserTemplate {
    let (front, body) = split_frontmatter(content);
    let title = field(front, "title").unwrap_or_else(|| stem.to_string());
    let name = field(front, "name").unwrap_or_else(|| title.clone());
    UserTemplate {
        id: stem.to_string(),
        title,
        name,
        body: body.to_string(),
    }
}

/// 列出 vault 的自定义模板,按文件名排序。
///
/// 目录不存在时返回空表而不是报错:绝大多数 vault 没有这个目录,那是正常状态,
/// 不是错误。
pub(crate) fn list_user_templates(vault: &Path) -> Vec<UserTemplate> {
    let dir = private_dir(vault).join(TEMPLATES_DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        // 只认 `.md`,而且必须是文件:子目录不递归 —— 模板是平铺的一层,
        // 递归下去会把别人放在里面的素材目录也扫成模板。
        .filter(|path| is_note_file(path) && path.is_file())
        .collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        if out.len() >= MAX_TEMPLATES {
            break;
        }
        // 先看大小再读:一个被误放进来的大文件不该把它整个读进内存。
        match fs::metadata(&path) {
            Ok(meta) if meta.len() > MAX_BYTES => continue,
            Ok(_) => {}
            Err(_) => continue,
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        out.push(parse_template(&stem, &content));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault(tag: &str) -> std::path::PathBuf {
        let unique = format!(
            "aeroric-tpl-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        fs::create_dir_all(private_dir(&root).join(TEMPLATES_DIR)).unwrap();
        root
    }

    fn write_template(vault: &Path, name: &str, content: &str) {
        fs::write(private_dir(vault).join(TEMPLATES_DIR).join(name), content).unwrap();
    }

    #[test]
    fn reads_title_and_name_from_frontmatter() {
        let t = parse_template(
            "meeting",
            "---\ntitle: \"周会纪要\"\nname: \"{{date}} 周会\"\n---\n# {{title}}\n\n## 议题\n",
        );
        assert_eq!(t.id, "meeting");
        assert_eq!(t.title, "周会纪要");
        assert_eq!(t.name, "{{date}} 周会");
        assert_eq!(t.body, "# {{title}}\n\n## 议题\n");
    }

    #[test]
    fn falls_back_to_stem_then_title() {
        // 没有 frontmatter:标题和默认文件名都用 stem,正文是原文。
        let plain = parse_template("值班记录", "# 值班\n");
        assert_eq!(plain.title, "值班记录");
        assert_eq!(plain.name, "值班记录");
        assert_eq!(plain.body, "# 值班\n");

        // 有 title 没 name:name 跟 title,而不是跟 stem —— 用户改了显示名,
        // 默认文件名跟着改才符合预期。
        let titled = parse_template("draft", "---\ntitle: 复盘\n---\nbody\n");
        assert_eq!(titled.name, "复盘");
    }

    #[test]
    fn field_after_a_colonless_line_is_still_found() {
        // 第一行没有冒号(YAML 注释 / 列表续行)。它只该被跳过,不该让后面的 title 读不到。
        let t = parse_template("x", "---\n# 一条注释\n  - 续行\ntitle: 找得到\n---\nbody\n");
        assert_eq!(t.title, "找得到");
    }

    #[test]
    fn blank_field_is_treated_as_missing() {
        // `title:` 写了但留空。回落到 stem,不能是空标题(面板上会是一条点不动的空行)。
        let t = parse_template("retro", "---\ntitle:\nname:   \n---\nbody\n");
        assert_eq!(t.title, "retro");
        assert_eq!(t.name, "retro");
    }

    #[test]
    fn unclosed_frontmatter_is_body() {
        // 开了 `---` 没闭合的是正文里的分隔线,不是 frontmatter。与前端 splitNote 一致。
        let t = parse_template("sep", "---\ntitle: 不算\nbody without close\n");
        assert_eq!(t.title, "sep");
        assert_eq!(t.body, "---\ntitle: 不算\nbody without close\n");
    }

    #[test]
    fn lists_sorted_and_skips_non_markdown() {
        let vault = temp_vault("list");
        write_template(&vault, "b.md", "---\ntitle: Bee\n---\nb\n");
        write_template(&vault, "a.md", "a\n");
        write_template(&vault, "notes.txt", "not a template\n");
        fs::create_dir_all(private_dir(&vault).join(TEMPLATES_DIR).join("nested")).unwrap();

        let list = list_user_templates(&vault);
        assert_eq!(
            list.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(list[1].title, "Bee");

        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn missing_dir_is_empty_not_error() {
        let unique = std::env::temp_dir().join(format!("aeroric-tpl-none-{}", std::process::id()));
        assert!(list_user_templates(&unique).is_empty());
    }

    #[test]
    fn oversized_file_is_skipped() {
        let vault = temp_vault("big");
        write_template(&vault, "small.md", "ok\n");
        write_template(&vault, "huge.md", &"x".repeat(MAX_BYTES as usize + 1));

        let list = list_user_templates(&vault);
        assert_eq!(
            list.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["small"]
        );

        let _ = fs::remove_dir_all(&vault);
    }
}
