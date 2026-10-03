//! zip 型导入共用的遍历。
//!
//! Notion / Bear / Roam 都是「用户选一个归档,把里面的东西搬进 vault」。Markio 在三个
//! 文件里各写了一遍同一个循环(打开、判 `is_dir`、算指纹、读内容、判总量、落盘),而且
//! 三份已经漂移:Roam 那份用 `push_warning_limited`,Notion 和 Bear 那份直接 `warnings.push`
//! (于是这两个的警告**不封顶**,和 `common.rs` 里那条限制自相矛盾)。
//!
//! 这里合成一个:归档遍历、护栏、记账在这一层,「这条 entry 要变成什么」由各 provider 给的
//! 处理函数决定。
//!
//! **路径安全走 `enclosed_name()`。** 这是 zip crate 自己的遍历守卫,也是仓库里已有的做法
//! (`agent_tools.rs:1070`)。Markio 用的是 `raw_name.split('/').last()` —— 那个做法把
//! `../../.zshrc` 洗成 `.zshrc` 然后**照样落盘**,只是落在了目标目录里;它靠的是「反正只取
//! 最后一段」,而这等于把归档里的目录结构整个丢掉(Notion 的资源引用因此全断)。

use std::io::{Read, Seek};
use std::path::Path;

use super::guards::{self, Budget};
use super::landing;
use super::manifest::Session;
use super::report::{ImportItem, ImportReport, SkipReason};

/// 一条 entry 交给 provider 之后的结果。
pub enum Handled {
    /// 已经落盘。带上 vault 相对落点和要附加的问题清单。
    Landed {
        dest: String,
        issues: Vec<super::report::ItemIssue>,
    },
    /// provider 不收这条。
    Skipped(SkipReason),
    /// 收了但失败了。
    Failed(String),
    /// 不收且不记报告(归档里的元数据文件)。
    Ignored,
}

/// provider 处理一条 entry。拿到洗过的相对路径与已读出的字节。
///
/// 字节在这一层就读完:护栏(单体上限、说谎的头)必须在 provider 看到内容之前生效,
/// 否则每个 provider 都要自己记得做一遍。
pub type HandleFn<'a> =
    &'a mut dyn FnMut(&mut Session, &Path, &str, Vec<u8>, &mut ImportReport) -> Handled;

/// 遍历归档。
///
/// `Err` 只在整轮该停时返回(归档打不开)。单条 entry 的问题记进报告继续走。
pub fn walk_archive<R: Read + Seek>(
    reader: R,
    session: &mut Session,
    dest_dir: &Path,
    report: &mut ImportReport,
    handle: HandleFn<'_>,
) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|error| format!("解析归档失败:{error}"))?;
    let mut budget = Budget::new();

    for index in 0..archive.len() {
        if let Err(hit) = budget.check_entry() {
            report.push(ImportItem::skipped(
                "(后续条目)",
                SkipReason::LimitReached { limit: hit.label() },
            ));
            break;
        }
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => {
                report.push(ImportItem::failed(
                    format!("(第 {index} 条)"),
                    format!("读归档条目失败:{error}"),
                ));
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        // 归档里声明的原始名字。报告里用它 —— 用户拿它回归档里对账。
        let raw = entry.name().to_string();

        // 路径守卫。`enclosed_name` 返回 `None` 表示这条 entry 想跑出解压根
        // (`../`、绝对路径、Windows 盘符)。这种归档是恶意的,记下来别静默丢。
        let Some(enclosed) = entry.enclosed_name() else {
            report.push(ImportItem::failed(
                raw,
                "归档条目的路径会跑出目标目录,已拒绝",
            ));
            continue;
        };
        let Some(relative) = landing::sanitize_relative(&enclosed.to_string_lossy()) else {
            report.push(ImportItem::skipped(
                raw,
                SkipReason::Unsupported {
                    extension: "(非法路径)".to_string(),
                },
            ));
            continue;
        };

        let declared = entry.size();
        let bytes = match guards::read_zip_entry_limited(&mut entry) {
            Ok(bytes) => bytes,
            Err(size) => {
                report.push(ImportItem::skipped(
                    raw,
                    SkipReason::TooLarge { bytes: size },
                ));
                continue;
            }
        };
        let actual = bytes.len() as u64;
        debug_assert!(actual <= guards::MAX_ENTRY_BYTES);
        let _ = declared;

        let handled = handle(session, dest_dir, &relative, bytes, report);
        match handled {
            Handled::Ignored => continue,
            Handled::Skipped(reason) => report.push(ImportItem::skipped(raw, reason)),
            Handled::Failed(detail) => report.push(ImportItem::failed(raw, detail)),
            Handled::Landed { dest, issues } => {
                let mut item = ImportItem::imported(raw, dest);
                item.issues = issues;
                report.push(item);
            }
        }
        if let Err(hit) = budget.record(actual) {
            report.push(ImportItem::skipped(
                "(后续条目)",
                SkipReason::LimitReached { limit: hit.label() },
            ));
            break;
        }
    }
    Ok(())
}
