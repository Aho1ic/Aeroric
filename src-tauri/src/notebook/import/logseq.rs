//! Logseq:graph 目录(`pages/` + `journals/` + `assets/`)。
//!
//! 和 Obsidian 的差别有两处。
//!
//! **一:`logseq/` 是配置目录,不是内容。** 里面是 `config.edn`、自定义 CSS、插件设置。
//! 它不以点开头,所以 `walk` 那条「点开头不进」拦不住它,要单独判。
//!
//! **二:`.org` 文件。** Logseq 同时支持 markdown 和 org-mode。Markio 的做法是记一条警告
//! 然后**不复制**(`logseq.rs:71-76`),于是那些笔记既不在 vault 里、也只在一条会被截断的
//! 警告里留个名字 —— 用户很可能不知道自己丢了东西。
//!
//! 这里改成**照原样复制,并记一条 `Degraded`**:
//!
//! - 内容进来了,不会丢。用户之后可以自己转,或者等 org 解析真做出来。
//! - 扩展名保持 `.org`,于是 `is_note_file` 不认它 —— 它不会以一篇满是 `* TODO` 的
//!   坏格式笔记的样子出现在笔记列表里。
//! - 报告里它是「导入成功 + 格式降级」,而不是「跳过」。这两件事对用户是不同的动作:
//!   前者是「去转一下」,后者是「回源端再找」。

use std::path::Path;

use super::guards::Budget;
use super::landing;
use super::report::ImportReport;
use super::run;
use super::walk::{self, Take, WalkCtx};

pub const PROVIDER: &str = "logseq";

/// graph 里的配置目录。不是用户的笔记,也不该出现在报告的明细里。
const CONFIG_DIR: &str = "logseq";

pub fn import(vault: &Path, src: &Path) -> Result<ImportReport, String> {
    if !src.is_dir() {
        return Err("Logseq 导入需要选择 graph 目录".to_string());
    }
    let src = src
        .canonicalize()
        .map_err(|error| format!("源目录不可用:{error}"))?;
    let dest_prefix = landing::provider_dir(PROVIDER);

    run::run(vault, PROVIDER, &mut |session, dest_dir, report| {
        let mut budget = Budget::new();
        let mut ctx = WalkCtx {
            provider: PROVIDER,
            session,
            report,
            budget: &mut budget,
            root: &src,
            dest_prefix: &dest_prefix,
        };
        let _ = walk::copy_dir(&mut ctx, &src, dest_dir, 0, &|path| decide(&src, path));
        Ok(())
    })
}

/// graph 里存附件的目录。这下面什么类型都收 —— 附件本来就是任意二进制。
const ASSETS_DIR: &str = "assets";

/// `src` 是 graph 根,用来判断某个文件是不是落在 `assets/` 下面。
fn decide(src: &Path, path: &Path) -> Take {
    // `logseq/` 下面的一律不收,连报告都不记。判的是**路径里有没有这一段**而不是
    // 直接父目录 —— 配置目录下面还有 `bak/`、`version-files/` 这些子目录。
    if path
        .components()
        .any(|component| component.as_os_str() == CONFIG_DIR)
    {
        return Take::Ignore;
    }
    // `assets/` 下面全收。附件是任意类型,按扩展名筛会把 `.zip`、`.webm` 这些
    // 正常附件挡在外面,而正文里的引用还指着它们。
    let under_assets = path
        .strip_prefix(src)
        .ok()
        .and_then(|rel| {
            rel.components()
                .next()
                .map(|first| first.as_os_str() == ASSETS_DIR)
        })
        .unwrap_or(false);
    if under_assets {
        return Take::Copy;
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("md") | Some("markdown") | Some("mdx") => Take::Copy,
        Some("org") => Take::CopyDegraded("org-mode 语法未转换,文件按原扩展名保留"),
        // graph 根上还会有 `custom.css`、`export.json`、`logseq.edn` 这类东西。它们既不是
        // 笔记也不是被引用的附件,复制进 vault 只是把垃圾搬进用户的笔记库。
        // 记成 `Skipped` 而不是 `Ignore`:用户挑的是自己的 graph 目录,里面有什么东西
        // 没进来他有权知道。
        _ => Take::Skip,
    }
}
