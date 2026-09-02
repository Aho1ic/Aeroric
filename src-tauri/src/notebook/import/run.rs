//! 一轮导入的外壳:开清单 → 交给 provider 落文件 → 存清单 → 写报告笔记。
//!
//! 七个 provider 的**中间**那一步各不相同(zip 遍历 / 目录递归 / XML 解析 / osascript),
//! 但外面这四步一模一样。抽出来不是为了少写几行,是为了让「清单一定被保存」和「报告一定
//! 被写」不依赖每个 provider 记得做 —— 漏掉前者会让下次导入重导一遍,漏掉后者会让这个
//! 导入器**过不了这一节的准入条件**,而两种漏法都不会让任何测试变红。

use std::path::Path;

use super::super::fs_ops::atomic_write;
use super::landing;
use super::manifest::Session;
use super::report::ImportReport;

/// provider 的落地逻辑。拿到会话和落点目录,自己决定怎么遍历源端,往报告里记账。
///
/// 返回 `Err` 表示**整轮都没法进行**(归档打不开、源目录不存在)。单个条目的失败不该
/// 走这条 —— 那是报告里的一条 `Failed`,而不是让已经成功的部分一起丢掉。
pub type LandFn<'a> =
    &'a mut dyn FnMut(&mut Session, &Path, &mut ImportReport) -> Result<(), String>;

/// 跑一轮导入。
///
/// `provider` 同时是清单里的命名空间和落点目录名,所以它必须稳定 —— 改一次会让之前
/// 记的指纹全部失效,下次导入把一切重导一遍。
pub fn run(vault: &Path, provider: &str, land: LandFn<'_>) -> Result<ImportReport, String> {
    let relative_dir = landing::provider_dir(provider);
    // 先把落点目录建出来:provider 里面用 `unique_path` 判重名,那要求目录已经存在
    // (不存在时 `exists()` 恒假,于是所有同名条目都会算出同一个"没占用"的落点)。
    let dest = landing::resolve_in_vault(vault, &format!("{relative_dir}/.keep"))?;
    let dest_dir = dest
        .parent()
        .ok_or_else(|| "落点目录不可用".to_string())?
        .to_path_buf();

    let mut session = Session::open(vault, provider);
    let mut report = ImportReport::new(provider, &relative_dir);

    let outcome = land(&mut session, &dest_dir, &mut report);

    // 清单先存。provider 中途返回 `Err` 时前面已经落地的那些**也已经写进 vault 了**,
    // 不存清单的话下一轮会把它们再落一遍(变成 `-2`、`-3` 的副本)。
    let saved = session.save();

    // 报告也要写。整轮失败时它更有用 —— 那时用户最需要知道失败之前进来了什么。
    write_report_note(vault, &relative_dir, &mut report);

    outcome?;
    saved?;
    Ok(report)
}

/// 报告落成 vault 里的一篇笔记。
///
/// 写不进去**不算导入失败**:笔记已经在 vault 里了,报告只是那一轮的说明。所以这里
/// 不返回 `Result`,而是把失败记在报告自己身上(`report_path` 留 `None`)—— 结构化那份
/// 报告照样通过 IPC 回给前端,面板不受影响。
fn write_report_note(vault: &Path, relative_dir: &str, report: &mut ImportReport) {
    let name = landing::report_name(&report.provider);
    let relative = format!("{relative_dir}/{name}");
    let Ok(path) = landing::resolve_in_vault(vault, &relative) else {
        return;
    };
    if atomic_write(&path, &report.to_markdown()).is_ok() {
        report.report_path = Some(relative);
    }
}

#[cfg(test)]
mod tests {
    use super::super::report::{ImportItem, SkipReason};
    use super::*;
    use std::path::PathBuf;

    fn temp_vault(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "aeroric-import-run-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("建临时 vault");
        dir
    }

    #[test]
    fn a_successful_round_lands_notes_the_manifest_and_the_report() {
        let vault = temp_vault("happy");
        let report = run(&vault, "notion", &mut |session, dir, report| {
            let path = landing::unique_path(dir, "a.md");
            std::fs::write(&path, "# A").expect("写笔记");
            session.record("k1".to_string());
            report.push(ImportItem::imported("a.md", "imports/notion/a.md"));
            Ok(())
        })
        .expect("整轮成功");

        assert_eq!(report.imported, 1);
        assert_eq!(report.dest, "imports/notion");
        assert!(vault.join("imports/notion/a.md").is_file());
        // 报告是一篇真笔记,而且路径记回了报告自己。
        let report_rel = report.report_path.as_ref().expect("报告路径");
        assert!(vault.join(report_rel).is_file());
        assert!(Session::open(&vault, "notion").is_known("k1"));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn a_mid_round_failure_still_saves_the_manifest() {
        // 已经落地的笔记就在 vault 里。不存清单的话下一轮会把它们再落一遍,
        // 变成 `-2`、`-3` 的副本 —— 而副本是真笔记,只能手工清。
        let vault = temp_vault("midfail");
        let error = run(&vault, "notion", &mut |session, dir, report| {
            std::fs::write(landing::unique_path(dir, "a.md"), "# A").expect("写笔记");
            session.record("k1".to_string());
            report.push(ImportItem::imported("a.md", "imports/notion/a.md"));
            Err("归档中途损坏".to_string())
        })
        .expect_err("整轮应失败");

        assert_eq!(error, "归档中途损坏");
        assert!(
            Session::open(&vault, "notion").is_known("k1"),
            "中途失败也要存清单"
        );
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn a_mid_round_failure_still_writes_the_report() {
        // 整轮失败时报告最有用:用户要知道失败之前进来了什么。
        let vault = temp_vault("midfail-report");
        let _ = run(&vault, "bear", &mut |_session, _dir, report| {
            report.push(ImportItem::imported("a.md", "imports/bear/a.md"));
            Err("boom".to_string())
        });
        let dir = vault.join("imports/bear");
        let notes: Vec<_> = std::fs::read_dir(&dir)
            .expect("读落点目录")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("导入报告-"))
            .collect();
        assert_eq!(notes.len(), 1, "失败的一轮也应留下报告");
        let body = std::fs::read_to_string(notes[0].path()).expect("读报告");
        assert!(body.contains("| 导入成功 | 1 |"));
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn the_dest_dir_exists_before_the_provider_runs() {
        // provider 里用 `unique_path` 判重名,而目录不存在时 `exists()` 恒假 ——
        // 于是所有同名条目都会算出同一个"没占用"的落点,后写的覆盖先写的。
        let vault = temp_vault("destdir");
        run(&vault, "logseq", &mut |_session, dir, _report| {
            assert!(dir.is_dir(), "落点目录应在 provider 跑之前就建好");
            Ok(())
        })
        .expect("成功");
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn two_rounds_of_the_same_provider_keep_both_reports() {
        // 报告名带时间戳的理由。固定名字的话第二轮要么覆盖第一轮,要么变成
        // `-2` 那种看不出时间的名字。
        let vault = temp_vault("two-reports");
        for _ in 0..2 {
            run(&vault, "roam", &mut |_s, _d, report| {
                report.push(ImportItem::skipped("x", SkipReason::AlreadyImported));
                Ok(())
            })
            .expect("成功");
            // 时间戳精度到秒,两轮之间要跨过一秒才能有不同的名字。
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let count = std::fs::read_dir(vault.join("imports/roam"))
            .expect("读目录")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("导入报告-"))
            .count();
        assert_eq!(count, 2);
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn the_provider_name_becomes_the_landing_dir() {
        let vault = temp_vault("dirname");
        let report = run(&vault, "apple-notes", &mut |_s, _d, _r| Ok(())).expect("成功");
        assert_eq!(report.dest, "imports/apple-notes");
        assert!(vault.join("imports/apple-notes").is_dir());
        let _ = std::fs::remove_dir_all(vault);
    }
}
