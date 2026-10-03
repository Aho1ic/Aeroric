//! 目录型导入共用的递归复制。
//!
//! Obsidian 和 Logseq 都是「用户选一个目录,把里面的东西搬进 vault」。Markio 在 Logseq
//! 那边写了两个各 55 行、只差一个 `if` 的递归函数(`copy_logseq_markdown_dir` 与
//! `copy_logseq_assets_dir`,分别只收 markdown / 收全部),Obsidian 那边又是第三个
//! (`copy_dir_incremental`)。这里合成一个,收什么由调用方给的判定函数决定。
//!
//! 合并不只是省行数:那三份各自维护着「跳软链」「跳点开头」「深度上限」「增量指纹」这几条
//! 规则,而它们已经开始漂移了 —— `copy_dir_incremental` 遇到软链直接 `continue`(用户完全
//! 看不到),另两个会记一条警告。合成一个之后这类规则只有一处。

use std::path::Path;

use super::guards::{self, Budget, LimitHit};
use super::landing;
use super::manifest::Session;
use super::report::{ImportItem, ImportReport, ItemIssue, SkipReason};

/// 一个文件该怎么处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Take {
    /// 收下,逐字节复制。
    Copy,
    /// 收下,但**表达能力掉了一档**:落点是原样字节,格式没转(`.org` 走这条)。
    CopyDegraded(&'static str),
    /// 不收,记一条 `Unsupported`。
    Skip,
    /// 不收,而且**连报告都不记** —— 用于源端的配置/元数据目录(Logseq 的 `logseq/`)。
    ///
    /// 和 `Skip` 分开是因为报告是给人看的:把 `config.edn`、`.DS_Store` 逐条列进去会把
    /// 「需要留意」那一节淹掉,而它们本来就不是用户的笔记。
    Ignore,
}

/// 一轮目录复制的共享状态。绑在一起传是因为递归的每一层都要全部用到,分开传就是
/// Markio 那种 9 个参数的签名。
pub struct WalkCtx<'a> {
    pub provider: &'a str,
    pub session: &'a mut Session,
    pub report: &'a mut ImportReport,
    pub budget: &'a mut Budget,
    /// 源端的根。指纹按**相对**这个根的路径算,所以用户把同一个 vault 从别的绝对
    /// 路径再导一次时,增量仍然认得出来。
    pub root: &'a Path,
    /// 落点的 vault 相对前缀,用来在报告里写 `dest`。
    pub dest_prefix: &'a str,
}

/// 递归复制 `src` 到 `dst`。
///
/// `decide` 拿到的是文件的绝对路径。返回 `Err` 只在**整轮该停**的时候(撞上限),
/// 单个文件的失败记进报告继续走。
pub fn copy_dir(
    ctx: &mut WalkCtx<'_>,
    src: &Path,
    dst: &Path,
    depth: usize,
    decide: &dyn Fn(&Path) -> Take,
) -> Result<(), LimitHit> {
    if depth > guards::MAX_DEPTH {
        ctx.report.push(ImportItem::skipped(
            rel_of(ctx.root, src),
            SkipReason::LimitReached {
                limit: "目录深度上限",
            },
        ));
        return Ok(());
    }
    let entries = match std::fs::read_dir(src) {
        Ok(entries) => entries,
        Err(error) => {
            // 读不开一个目录不该让整轮失败:一个权限不足的子目录旁边可能还有几千篇
            // 正常笔记。
            ctx.report.push(ImportItem::failed(
                rel_of(ctx.root, src),
                format!("读目录失败:{error}"),
            ));
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let from = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            ctx.report.push(ImportItem::failed(
                rel_of(ctx.root, &from),
                "读不到文件类型",
            ));
            continue;
        };

        // 软链在判目录/文件**之前**处理:`is_dir()` 会跟随软链,于是一个指向上层的
        // 软链会被当成普通目录递归下去,自己成环。
        if file_type.is_symlink() {
            ctx.report.push(ImportItem::skipped(
                rel_of(ctx.root, &from),
                SkipReason::Symlink,
            ));
            continue;
        }
        // 点开头的一律不进:`.git`、`.obsidian`、`.DS_Store`。不记报告 —— 它们不是内容。
        if name.starts_with('.') {
            continue;
        }

        if file_type.is_dir() {
            copy_dir(ctx, &from, &dst.join(&name), depth + 1, decide)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        copy_file(ctx, &from, &dst.join(&name), decide(&from))?;
    }
    Ok(())
}

fn copy_file(ctx: &mut WalkCtx<'_>, from: &Path, to: &Path, take: Take) -> Result<(), LimitHit> {
    let source = rel_of(ctx.root, from);
    let degraded = match take {
        Take::Ignore => return Ok(()),
        Take::Skip => {
            let extension = from
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            ctx.report.push(ImportItem::skipped(
                source,
                SkipReason::Unsupported { extension },
            ));
            return Ok(());
        }
        Take::Copy => None,
        Take::CopyDegraded(detail) => Some(detail),
    };

    // 增量:同一份源端内容(按相对路径认)不重复落。
    let key = super::manifest::fingerprint(&format!("{}::{source}", ctx.provider));
    if ctx.session.is_known(&key) {
        ctx.report
            .push(ImportItem::skipped(source, SkipReason::AlreadyImported));
        return Ok(());
    }

    // 条目数上限:撞上就停整轮 —— 后面还有多少不知道,继续走只是把时间花在
    // 一定会被拒的条目上。
    if let Err(hit) = ctx.budget.check_entry() {
        ctx.report.push(ImportItem::skipped(
            source,
            SkipReason::LimitReached { limit: hit.label() },
        ));
        return Err(hit);
    }

    let bytes = match std::fs::metadata(from) {
        Ok(meta) => meta.len(),
        Err(error) => {
            ctx.report
                .push(ImportItem::failed(source, format!("读不到大小:{error}")));
            return Ok(());
        }
    };
    if guards::entry_too_large(bytes) {
        ctx.report
            .push(ImportItem::skipped(source, SkipReason::TooLarge { bytes }));
        return Ok(());
    }

    // 落点重名不覆盖。目录已经由这一层建出来,`unique_path` 的 `exists()` 才有意义。
    if let Some(parent) = to.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            ctx.report
                .push(ImportItem::failed(source, format!("建目录失败:{error}")));
            return Ok(());
        }
    }
    let target = landing::unique_path(
        to.parent().unwrap_or(to),
        &to.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "imported".to_string()),
    );
    if let Err(error) = std::fs::copy(from, &target) {
        ctx.report
            .push(ImportItem::failed(source, format!("复制失败:{error}")));
        return Ok(());
    }

    // 记账顺序:指纹只在**写成功之后**记。写失败还记的话,重试那一轮会把它当成
    // 「导过了」跳掉,那篇笔记就永远进不来了。
    ctx.session.record(key);
    let over = ctx.budget.record(bytes);

    let dest_rel = dest_rel_of(ctx.dest_prefix, ctx.root, from, &target);
    let mut item = ImportItem::imported(source.clone(), dest_rel);
    if let Some(detail) = degraded {
        item = item.with_issue(ItemIssue::Degraded {
            detail: detail.to_string(),
        });
    }
    ctx.report.push(item);

    if let Err(hit) = over {
        // 总量超了。这一条已经落地并记账,停的是**后面**的。
        ctx.report.push(ImportItem::skipped(
            "(后续条目)",
            SkipReason::LimitReached { limit: hit.label() },
        ));
        return Err(hit);
    }
    Ok(())
}

/// 源端相对路径。给报告用 —— 用户拿它回源端对账,绝对路径既长又泄露目录结构。
fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 落点的 vault 相对路径。
///
/// 不能直接用源端相对路径拼:`unique_path` 可能把文件名改成了 `a-2.md`,而报告里
/// 那个 `dest` 是用户要拿去在 vault 里找文件的,写成没改名的样子就找不到。
fn dest_rel_of(prefix: &str, root: &Path, from: &Path, target: &Path) -> String {
    let parent_rel = from
        .strip_prefix(root)
        .ok()
        .and_then(|rel| rel.parent())
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if parent_rel.is_empty() {
        format!("{prefix}/{name}")
    } else {
        format!("{prefix}/{parent_rel}/{name}")
    }
}
