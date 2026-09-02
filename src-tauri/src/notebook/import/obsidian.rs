//! Obsidian:vault 目录递归。
//!
//! 最简单的一个 —— Obsidian 的 vault 本来就是一棵 `.md` 加附件的目录树,`[[wiki]]` 语法
//! 和随手记一致,不需要重写链接。
//!
//! 附件**跟着复制并保持相对目录结构**。Obsidian 的图片引用是相对路径
//! (`![](assets/img.png)`),把附件挪到别处(比如 vault 根的 `attachments/`)会让每一条
//! 引用都失效,那就得改写正文 —— 而正文改写是有损的,不如保持原样。

use std::path::Path;

use super::guards::Budget;
use super::landing;
use super::report::ImportReport;
use super::run;
use super::walk::{self, Take, WalkCtx};

pub const PROVIDER: &str = "obsidian";

pub fn import(vault: &Path, src: &Path) -> Result<ImportReport, String> {
    if !src.is_dir() {
        return Err("Obsidian 导入需要选择 vault 目录".to_string());
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
        // 撞上限不是错误:已经进来的那些是有效的,报告里也记了为什么停。
        let _ = walk::copy_dir(&mut ctx, &src, dest_dir, 0, &decide);
        Ok(())
    })
}

/// Obsidian vault 里什么都收 —— 笔记和附件都是用户的内容。
///
/// 唯一挡掉的是 `.obsidian/` 里的配置,但那由 `walk` 的「点开头不进」统一处理,
/// 所以这里不必再判。
fn decide(_path: &Path) -> Take {
    Take::Copy
}
