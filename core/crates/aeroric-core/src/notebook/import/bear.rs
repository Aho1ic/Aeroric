//! Bear:`.bearbook` 归档。
//!
//! 最直接的 zip 型 —— 归档里就是 `.md` 加资源,语法与随手记兼容,不需要重写。
//!
//! 和 Markio 那份的一处不同:它把非 `.md` 的东西**全部压到 `Assets/` 一层**
//! (`bear.rs:57-62`)。Bear 的图片引用是相对路径,压平之后每一条都断了。这里保留归档里的
//! 目录结构,引用因此仍然有效。

use std::path::Path;

use super::report::ImportReport;
use super::run;
use super::zip_common;
use super::zip_src;

pub const PROVIDER: &str = "bear";

pub fn import(vault: &Path, archive: &Path) -> Result<ImportReport, String> {
    let file =
        std::fs::File::open(archive).map_err(|error| format!("打开 bear 归档失败:{error}"))?;
    run::run(vault, PROVIDER, &mut |session, dest_dir, report| {
        zip_src::walk_archive(
            &file,
            session,
            dest_dir,
            report,
            &mut |session, dest_dir, relative, bytes, _report| {
                zip_common::land_verbatim(PROVIDER, session, dest_dir, relative, bytes)
            },
        )
    })
}
