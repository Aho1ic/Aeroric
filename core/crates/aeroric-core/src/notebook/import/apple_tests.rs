//! Apple Notes 导入的测试。
//!
//! 真正调 `osascript` 那一步不能进测试:它会弹系统授权框、读用户真实的备忘录。所以被测的
//! 入口是 [`super::apple_notes::import_from_output`] —— 它拿的正是 osascript 会吐出来的
//! 那个字符串,于是「切片、锁定笔记记账、落盘、去重」整条链路在**所有平台**上都验得到。
//!
//! 这里同时盯住一件编译期看不出来的事:脚本里的哨兵和 Rust 这侧解析用的必须是同一串。
//! 那两处对不上的话导入会安静地什么都不产出。

use std::path::{Path, PathBuf};

use super::apple_notes;
use super::report::{ImportItem, ImportReport, ItemIssue, ItemStatus, SkipReason};

fn temp_vault(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aeroric-import-apple-{tag}-{}-{nanos}",
        std::process::id()
    ));
    let vault = root.join("vault");
    std::fs::create_dir_all(&vault).expect("建 vault");
    vault
}

fn cleanup(vault: &Path) {
    if let Some(root) = vault.parent() {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// 拼一条 osascript 会输出的记录。
fn record(
    title: &str,
    created: &str,
    updated: &str,
    folder: &str,
    locked: bool,
    body: &str,
) -> String {
    format!(
        "{}{title}{f}{created}{f}{updated}{f}{folder}{f}{}{f}{body}",
        apple_notes::SENTINEL,
        if locked { "1" } else { "0" },
        f = apple_notes::FIELD
    )
}

/// 最常见的那种:没时间、没文件夹、没锁。
fn simple(title: &str, body: &str) -> String {
    record(title, "", "", "", false, body)
}

fn item_for<'a>(report: &'a ImportReport, source: &str) -> &'a ImportItem {
    report
        .items
        .iter()
        .find(|item| item.source == source)
        .unwrap_or_else(|| {
            let all: Vec<&str> = report.items.iter().map(|i| i.source.as_str()).collect();
            panic!("报告里应有 {source}:{all:?}")
        })
}

fn read_note(vault: &Path, relative: &str) -> String {
    let path = vault.join(format!("imports/apple-notes/{relative}"));
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("读不到 {}", path.display()))
}

// ─── 哨兵与脚本的一致性 ──────────────────────────────────────────────────

#[test]
fn the_script_uses_the_same_sentinels_the_parser_expects() {
    // 这两处对不上的话导入会安静地什么都不产出:切片全落在一整块上,而报告显示
    // 「0 篇」——那读起来像「备忘录是空的」。编译期看不出来,所以在这里锁住。
    let script = apple_notes::apple_script();
    assert!(script.contains(apple_notes::SENTINEL), "脚本里要有记录哨兵");
    assert!(script.contains(apple_notes::FIELD), "脚本里要有字段哨兵");
    assert!(
        script.contains(apple_notes::SENTINEL_ESCAPE),
        "脚本要把正文里的哨兵替换掉,否则不变量不成立"
    );
    assert!(
        script.contains(apple_notes::FIELD_ESCAPE),
        "字段哨兵同样要转义"
    );
}

#[test]
fn a_body_containing_the_sentinel_does_not_split_the_stream() {
    // Markio 用固定的 `---MK-NOTE-SEP---` 切片,一篇正文里**写了**那串字的笔记会把
    // 整个解析切错位 —— 那之后每篇的标题和正文都是错的,而报告里一切正常。
    //
    // AppleScript 那侧会先把正文里的哨兵替换成转义形式,所以到这里时哨兵只出现在
    // 记录边界。这一条模拟的就是「正文里本来有哨兵、已被 AppleScript 转义」。
    let vault = temp_vault("sentinel-in-body");
    let sneaky = format!("正文里写了 {} 这串字", apple_notes::SENTINEL_ESCAPE);
    let raw = format!(
        "{}{}",
        simple("狡猾的", &sneaky),
        simple("后面的", "<p>还在</p>")
    );

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(
        report.imported, 2,
        "两篇都该在,顺序不该被切乱:{:?}",
        report.items
    );
    let text = read_note(&vault, "狡猾的.md");
    assert!(
        text.contains(apple_notes::SENTINEL),
        "转义要还原回字面哨兵,否则正文丢字:{text}"
    );
    assert!(
        read_note(&vault, "后面的.md").contains("还在"),
        "后一篇的正文不能被前一篇吃掉"
    );
    cleanup(&vault);
}

#[test]
fn a_body_containing_the_field_separator_keeps_its_text() {
    let vault = temp_vault("field-in-body");
    let body = format!("前 {} 后", apple_notes::FIELD_ESCAPE);
    let raw = simple("含字段符", &body);

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1, "{:?}", report.items);
    let text = read_note(&vault, "含字段符.md");
    assert!(text.contains(apple_notes::FIELD), "字段符要还原:{text}");
    assert!(
        text.contains('前') && text.contains('后'),
        "两侧文字都要在:{text}"
    );
    cleanup(&vault);
}

// ─── 锁定笔记 ────────────────────────────────────────────────────────────

#[test]
fn a_locked_note_is_recorded_instead_of_silently_dropped() {
    // Markio 的 `on error` 分支是空的(注释还写着「用户不会被静默漏掉」——恰好相反)。
    // 于是用户看到「导入 2 篇」,而实际有 3 篇,少的那篇没人提。
    let vault = temp_vault("locked");
    let raw = format!(
        "{}{}",
        record("被锁的", "", "", "", true, ""),
        simple("正常的", "<p>x</p>")
    );

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1);
    assert_eq!(
        report.skipped, 1,
        "锁定的那篇要**数得出来**:{:?}",
        report.items
    );

    let item = item_for(&report, "被锁的");
    assert!(
        matches!(
            &item.status,
            ItemStatus::Skipped {
                reason: SkipReason::Unreadable { detail }
            } if detail.contains("锁定")
        ),
        "理由要说清是锁定,而不是笼统的读不出来:{:?}",
        item.status
    );
    assert!(item.dest.is_none(), "跳过的条目不该有落点");
    cleanup(&vault);
}

#[test]
fn a_locked_note_still_reports_which_note_it_was() {
    // 分两层 try 的理由:先单独取标题、再取正文,于是锁定笔记至少能报出是哪一篇。
    // 只有连标题都取不到时才退回「(无标题备忘录)」。
    let vault = temp_vault("locked-title");
    let raw = record("季度财务", "", "", "工作", true, "");
    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert!(
        report.items.iter().any(|item| item.source == "季度财务"),
        "报告里要指名是哪一篇:{:?}",
        report.items
    );
    cleanup(&vault);
}

#[test]
fn a_locked_note_without_a_readable_title_is_still_accounted_for() {
    // 标题的第一层 try 也可能失败。那时如果空记录守卫不豁免 locked,这一条会在解析阶段
    // 被丢掉,正好回到 Markio 的问题:用户知道「应有一篇」,报告却没有任何东西能对账。
    let vault = temp_vault("locked-untitled");
    let raw = record("", "", "", "", true, "");
    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(
        (report.imported, report.skipped),
        (0, 1),
        "{:?}",
        report.items
    );
    let item = item_for(&report, "(无标题备忘录)");
    assert!(
        matches!(
            item.status,
            ItemStatus::Skipped {
                reason: SkipReason::Unreadable { .. }
            }
        ),
        "没有标题也要按读不出记账:{:?}",
        item.status
    );
    cleanup(&vault);
}

// ─── HTML → markdown ─────────────────────────────────────────────────────

#[test]
fn html_body_keeps_its_structure() {
    // Markio 拿 `enml_to_markdown` 剥标签,列表和加粗全平掉。
    let vault = temp_vault("structure");
    let body = "<h1>标题</h1><ul><li>甲</li><li>乙</li></ul><p><b>粗</b></p>";
    let raw = simple("结构", body);

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "结构.md");
    assert!(text.contains("# 标题"), "{text}");
    assert!(text.contains("- 甲") && text.contains("- 乙"), "{text}");
    assert!(text.contains("**粗**"), "{text}");
    cleanup(&vault);
}

#[test]
fn an_object_placeholder_is_reported_as_a_lost_resource() {
    // 附件本体不在正文里,这一版不取 —— 但要让用户知道有东西没跟过来。
    let vault = temp_vault("attachment");
    let raw = simple("有附件", "<p>看这个</p><object data=\"x\"></object>");

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1, "笔记本身要进来");
    assert_eq!(
        report.resource_lost, 1,
        "同时记一条资源丢失:{:?}",
        report.items
    );
    let item = item_for(&report, "有附件");
    assert!(
        matches!(item.status, ItemStatus::Imported),
        "状态和 issue 是两层,正交:{:?}",
        item.status
    );
    assert!(
        item.issues
            .iter()
            .any(|issue| matches!(issue, ItemIssue::ResourceLost { .. })),
        "{:?}",
        item.issues
    );
    cleanup(&vault);
}

#[test]
fn a_note_with_no_attachments_reports_none_lost() {
    // 反向那一半:没有附件时不该凭空记一条 —— 那会把报告里「需要留意」的信号冲淡。
    let vault = temp_vault("no-attachment");
    let raw = simple("没附件", "<p>纯文字</p>");
    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.resource_lost, 0, "{:?}", report.items);
    assert!(item_for(&report, "没附件").issues.is_empty());
    cleanup(&vault);
}

// ─── 文件夹 ──────────────────────────────────────────────────────────────

#[test]
fn a_folder_becomes_a_subdirectory() {
    // Notes.app 里的文件夹是用户自己的组织方式,平铺掉等于把它扔了。
    let vault = temp_vault("folder");
    let raw = record("周报", "", "", "工作", false, "<p>x</p>");

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1, "{:?}", report.items);
    assert!(
        vault.join("imports/apple-notes/工作/周报.md").is_file(),
        "应落在文件夹同名子目录下"
    );
    let dest = item_for(&report, "周报").dest.as_ref().expect("有落点");
    assert_eq!(dest, "imports/apple-notes/工作/周报.md");
    assert!(vault.join(dest).is_file(), "报告里的落点必须真的存在");
    cleanup(&vault);
}

#[test]
fn a_folder_named_like_a_skipped_dir_does_not_hide_the_note() {
    // `.notebook` 这类名字会让索引跳过整棵子树 —— 笔记落进去之后在 vault 里根本看不到,
    // 用户会以为导入漏了文件。落到根上是对的。
    let vault = temp_vault("folder-skipped");
    let raw = record("藏起来", "", "", ".notebook", false, "<p>x</p>");

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1);
    assert!(
        vault.join("imports/apple-notes/藏起来.md").is_file(),
        "应退回落在根上,而不是落进一个索引不看的目录"
    );
    cleanup(&vault);
}

#[test]
fn a_folder_that_tries_to_escape_is_refused() {
    let vault = temp_vault("folder-escape");
    let raw = record("越界", "", "", "../../evil", false, "<p>x</p>");

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1);
    let dest = item_for(&report, "越界").dest.as_ref().expect("有落点");
    assert!(
        vault
            .join(dest)
            .starts_with(vault.join("imports/apple-notes")),
        "落点必须还在导入目录里:{dest}"
    );
    cleanup(&vault);
}

// ─── frontmatter ─────────────────────────────────────────────────────────

#[test]
fn front_matter_carries_title_times_and_folder() {
    let vault = temp_vault("front");
    let raw = record(
        "元数据",
        "2024-01-31T09:15:00",
        "2024-02-01T10:15:00",
        "工作",
        false,
        "<p>x</p>",
    );

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "工作/元数据.md");
    assert!(text.starts_with("---\n"), "{text}");
    assert!(text.contains("title: \"元数据\""), "{text}");
    assert!(text.contains("created: 2024-01-31T09:15:00Z"), "{text}");
    assert!(text.contains("updated: 2024-02-01T10:15:00Z"), "{text}");
    assert!(text.contains("folder: \"工作\""), "{text}");
    assert!(text.contains("source: apple-notes"), "{text}");
    cleanup(&vault);
}

#[test]
fn a_title_with_yaml_metacharacters_stays_parseable() {
    // 标题里的引号直接拼进 frontmatter 会让**整块** YAML 失效,于是标题、时间全读不出来。
    let vault = temp_vault("front-quote");
    let raw = simple("复盘: 他说\"好\"", "<p>x</p>");

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1, "{:?}", report.items);
    let dest = item_for(&report, "复盘: 他说\"好\"")
        .dest
        .as_ref()
        .expect("有落点");
    let text = std::fs::read_to_string(vault.join(dest)).expect("读笔记");

    // 按落点认这一篇:每一轮都会写一篇**导入报告**,它也有 title 且排在前面,
    // 只 `find(key == "title")` 会拿到报告的标题,断言就变成在验别的东西。
    // `scan_vault_fields` 给的是绝对路径,`dest` 是 vault 相对路径,拼起来再比。
    let absolute = vault.join(dest);
    let sources = super::super::fields::scan_vault_fields(&vault).expect("扫字段");
    let mine = sources
        .iter()
        .find(|source| Path::new(&source.path) == absolute)
        .unwrap_or_else(|| {
            let all: Vec<&str> = sources.iter().map(|s| s.path.as_str()).collect();
            panic!("应能扫到 {dest} 的 frontmatter:{all:?} / {text}")
        });
    let found = mine
        .fields
        .iter()
        .find(|field| field.key == "title")
        .unwrap_or_else(|| panic!("frontmatter 应能解析出 title:{text}"));
    assert_eq!(
        found.values,
        vec!["复盘: 他说\"好\"".to_string()],
        "值应原样还原:{text}"
    );
    cleanup(&vault);
}

#[test]
fn a_title_with_two_backslashes_round_trips() {
    // 上面那条引号的用例**测不出转义有没有做**:`vault_index::unquote_scalar` 只脱最外层
    // 那一对引号,所以转义过的和没转义的脱完之后一样。两个连续反斜杠才能把两者分开 ——
    // 转义过的写成四个、还原成两个;没转义的写成两个、还原成**一个**,少一个字符。
    let vault = temp_vault("front-backslash");
    let title = "路径 C:\\\\x"; // 字面上是:路径 C: 反斜杠 反斜杠 x
    let raw = simple(title, "<p>x</p>");

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1, "{:?}", report.items);
    let dest = item_for(&report, title).dest.as_ref().expect("有落点");
    let text = std::fs::read_to_string(vault.join(dest)).expect("读笔记");

    let absolute = vault.join(dest);
    let sources = super::super::fields::scan_vault_fields(&vault).expect("扫字段");
    let mine = sources
        .iter()
        .find(|source| Path::new(&source.path) == absolute)
        .unwrap_or_else(|| panic!("应能扫到 {dest} 的 frontmatter:{text}"));
    let field = mine
        .fields
        .iter()
        .find(|field| field.key == "title")
        .unwrap_or_else(|| panic!("frontmatter 应能解析出 title:{text}"));
    assert_eq!(
        field.values,
        vec![title.to_string()],
        "两个反斜杠要原样还原,不能少一个:{text}"
    );
    cleanup(&vault);
}

#[test]
fn an_untitled_note_still_lands_with_a_usable_name() {
    let vault = temp_vault("untitled");
    let raw = simple("", "<p>没标题但有正文</p>");

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1, "{:?}", report.items);
    let dest = item_for(&report, "(无标题备忘录)")
        .dest
        .as_ref()
        .expect("有落点");
    assert!(vault.join(dest).is_file(), "{dest}");
    assert!(dest.ends_with(".md"), "必须还是笔记文件:{dest}");
    cleanup(&vault);
}

// ─── 去重 ────────────────────────────────────────────────────────────────

#[test]
fn the_same_output_imported_twice_skips_the_second_time() {
    let vault = temp_vault("idem");
    let raw = simple("一次", "<p>正文</p>");

    let first = apple_notes::import_from_output(&vault, &raw).expect("第一次");
    assert_eq!(first.imported, 1);
    let second = apple_notes::import_from_output(&vault, &raw).expect("第二次");
    assert_eq!(second.imported, 0, "{:?}", second.items);
    assert_eq!(second.skipped, 1);
    assert!(
        !vault.join("imports/apple-notes/一次-2.md").exists(),
        "不该留副本"
    );
    cleanup(&vault);
}

#[test]
fn two_notes_sharing_a_title_both_arrive() {
    // 同名备忘录很常见。只按标题算指纹会把后一篇当重复跳掉 —— 那是静默丢数据。
    let vault = temp_vault("same-title");
    let raw = format!(
        "{}{}",
        simple("购物清单", "<p>牛奶</p>"),
        simple("购物清单", "<p>鸡蛋</p>")
    );

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 2, "两篇都该进来:{:?}", report.items);
    assert!(vault.join("imports/apple-notes/购物清单.md").is_file());
    assert!(
        vault.join("imports/apple-notes/购物清单-2.md").is_file(),
        "后缀要在扩展名**之前**,否则它不再是笔记文件"
    );
    cleanup(&vault);
}

// ─── 输出形状 ────────────────────────────────────────────────────────────

#[test]
fn empty_output_produces_an_empty_report_not_an_error() {
    // Notes.app 真的空着是一种正常状态。报错会让用户以为出了问题。
    let vault = temp_vault("empty");
    let report = apple_notes::import_from_output(&vault, "").expect("不该报错");
    assert_eq!(report.imported, 0);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.failed, 0);
    assert!(report.items.is_empty(), "{:?}", report.items);
    cleanup(&vault);
}

#[test]
fn a_body_containing_the_field_separator_is_not_truncated() {
    // 正文用「剩下的全部」拼回来,不是 `splitn`:字段数将来变了的话,`splitn` 会静默把
    // 正文截掉一段,而这里剩下的部分本来就只有正文。
    let vault = temp_vault("body-rest");
    // 手工造一条**多**一个字段分隔符的记录,模拟字段数漂移。
    let raw = format!(
        "{}标题{f}{f}{f}{f}0{f}前半{f}后半",
        apple_notes::SENTINEL,
        f = apple_notes::FIELD
    );

    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    assert_eq!(report.imported, 1, "{:?}", report.items);
    let text = read_note(&vault, "标题.md");
    assert!(text.contains("前半"), "正文前半要在:{text}");
    assert!(text.contains("后半"), "正文后半不能被截掉:{text}");
    cleanup(&vault);
}

#[test]
fn the_round_writes_a_report_note() {
    let vault = temp_vault("report");
    let raw = simple("有报告", "<p>x</p>");
    let report = apple_notes::import_from_output(&vault, &raw).expect("导入成功");
    let path = report.report_path.as_ref().expect("应写出报告笔记");
    assert!(vault.join(path).is_file(), "{path}");
    cleanup(&vault);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn on_other_platforms_the_entry_point_says_so_instead_of_reporting_zero() {
    // 空报告会读成「备忘录里没东西」,而真实情况是这个平台根本没有 Notes.app。
    let vault = temp_vault("not-macos");
    let error = apple_notes::import(&vault).expect_err("应当报错");
    assert!(error.contains("macOS"), "{error}");
    cleanup(&vault);
}
