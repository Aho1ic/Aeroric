//! Roam Research:Markdown zip,或者 JSON 导出。
//!
//! Roam 能导两种格式,而用户常常不知道自己手上是哪一种,所以两种都收:
//!
//! - `.md`:原样落盘。`[[link]]` 与 `#tag` 和随手记兼容。
//! - `.json`:一个文件含**全部**页面,要转成一篇篇 markdown。块树 → 嵌套 bullet,
//!   `heading` 块 → 标题,`{{[[TODO]]}}` → 任务勾选。
//!
//! JSON 那一路和别的 provider 有一处结构性不同:**一条源端条目会变出很多条落点**。所以它
//! 不能走 `zip_common::land_verbatim`,报告里也不是一条记录 —— 一个 JSON 里有三百个页面,
//! 报成一条「已导入 roam.json」会让用户完全看不出哪个页面出了问题。

use std::path::Path;

use serde::Deserialize;

use super::landing;
use super::manifest::{self, Session};
use super::report::{ImportItem, ImportReport, SkipReason};
use super::run;
use super::zip_common;
use super::zip_src::{self, Handled};

pub const PROVIDER: &str = "roam";

#[derive(Deserialize)]
struct Page {
    title: Option<String>,
    #[serde(default)]
    children: Vec<Block>,
}

#[derive(Deserialize)]
struct Block {
    string: Option<String>,
    #[serde(default)]
    children: Vec<Block>,
    heading: Option<u8>,
}

pub fn import(vault: &Path, archive: &Path) -> Result<ImportReport, String> {
    let file =
        std::fs::File::open(archive).map_err(|error| format!("打开 roam zip 失败:{error}"))?;
    run::run(vault, PROVIDER, &mut |session, dest_dir, report| {
        zip_src::walk_archive(
            &file,
            session,
            dest_dir,
            report,
            &mut |session, dest_dir, relative, bytes, report| {
                handle(session, dest_dir, relative, bytes, report)
            },
        )
    })
}

fn handle(
    session: &mut Session,
    dest_dir: &Path,
    relative: &str,
    bytes: Vec<u8>,
    report: &mut ImportReport,
) -> Handled {
    let lower = relative.to_ascii_lowercase();
    if lower.ends_with(".json") {
        return handle_json(session, dest_dir, relative, &bytes, report);
    }
    if lower.ends_with(".md") || lower.ends_with(".markdown") {
        return zip_common::land_verbatim(PROVIDER, session, dest_dir, relative, bytes);
    }
    // Roam 的导出里除了这两种就只有 `.edn`(数据库快照,不是内容)。
    let extension = Path::new(relative)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    Handled::Skipped(SkipReason::Unsupported { extension })
}

/// JSON 那一路:一条条目变出 N 篇笔记,所以自己往报告里记账,并对外报 `Ignored`
/// (外层再记一条会变成重复计数)。
fn handle_json(
    session: &mut Session,
    dest_dir: &Path,
    relative: &str,
    bytes: &[u8],
    report: &mut ImportReport,
) -> Handled {
    let text = String::from_utf8_lossy(bytes);
    let pages: Vec<Page> = match serde_json::from_str(&text) {
        Ok(pages) => pages,
        // 解析不了就报在这条条目上。**不能**转成 `Skipped` —— 用户选了 JSON 导出,
        // 解析失败意味着他的笔记一篇都没进来,那是失败不是跳过。
        Err(error) => return Handled::Failed(format!("解析 Roam JSON 失败:{error}")),
    };

    for page in pages {
        let title = page.title.unwrap_or_default();
        let title = title.trim();
        if title.is_empty() {
            // 无标题页面没法定文件名,而文件名是 wikilink 的目标。
            report.push(ImportItem::skipped(
                format!("{relative} → (无标题页面)"),
                SkipReason::Unsupported {
                    extension: "(无标题)".to_string(),
                },
            ));
            continue;
        }
        let source = format!("{relative} → {title}");
        // 指纹按**页面标题**算,不是按 JSON 文件路径 —— 同一个导出重跑时要按页面去重,
        // 而不是整份 JSON 一起跳过(那样新增的页面永远进不来)。
        let key = manifest::fingerprint(&format!("{PROVIDER}::json::{title}"));
        if session.is_known(&key) {
            report.push(ImportItem::skipped(source, SkipReason::AlreadyImported));
            continue;
        }

        let mut body = format!("# {title}\n\n");
        for block in &page.children {
            render_block(block, 0, &mut body);
        }
        let name = format!("{}.md", landing::sanitize_name(title));
        match zip_common::land_bytes(dest_dir, &name, body.as_bytes()) {
            Ok(target) => {
                session.record(key);
                report.push(ImportItem::imported(
                    source,
                    zip_common::dest_relative(PROVIDER, dest_dir, &target),
                ));
            }
            Err(detail) => report.push(ImportItem::failed(source, detail)),
        }
    }
    Handled::Ignored
}

/// 块树 → 嵌套 bullet。
fn render_block(block: &Block, depth: usize, out: &mut String) {
    let text = block.string.as_deref().unwrap_or("").trim_end();
    // heading 块渲染成 markdown 标题,子块从 depth 0 重新缩进 —— 标题下面的内容
    // 不该因为标题本身的层级而多缩进一层。
    if let Some(level) = block.heading.filter(|level| (1..=6).contains(level)) {
        if !text.is_empty() {
            out.push_str(&format!("{} {text}\n\n", "#".repeat(level as usize)));
        }
        for child in &block.children {
            render_block(child, 0, out);
        }
        return;
    }
    let (marker, rest) = split_todo(text);
    if !rest.is_empty() || !block.children.is_empty() {
        out.push_str(&"  ".repeat(depth));
        out.push_str("- ");
        out.push_str(marker);
        out.push_str(&rest);
        out.push('\n');
    }
    for child in &block.children {
        render_block(child, depth + 1, out);
    }
}

/// `{{[[TODO]]}}` / `{{[[DONE]]}}` → markdown 复选框。
fn split_todo(text: &str) -> (&'static str, String) {
    let trimmed = text.trim_start();
    for (needle, marker) in [
        ("{{[[TODO]]}}", "[ ] "),
        ("{{TODO}}", "[ ] "),
        ("{{[[DONE]]}}", "[x] "),
        ("{{DONE}}", "[x] "),
    ] {
        if let Some(rest) = trimmed.strip_prefix(needle) {
            return (marker, rest.trim_start().to_string());
        }
    }
    ("", text.to_string())
}
