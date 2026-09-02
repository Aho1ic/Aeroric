//! Evernote `.enex` 导入的测试。
//!
//! 重点在四处比 Markio 多做的事上:ENML 保结构、附件真落盘、`<en-media>` 按 MD5 精确
//! 配对、`<en-todo>` 变成扫得出来的待办。每一条都断言**可观测的后果**(文件在不在、
//! 报告里那一条是什么、`tasks.rs` 扫不扫得到),不是断言中间函数的返回值。

use std::path::{Path, PathBuf};

use super::evernote;
use super::report::{ImportItem, ImportReport, ItemIssue, ItemStatus, SkipReason};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aeroric-import-enex-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 建一对 (vault, 源目录)。
fn pair(tag: &str) -> (PathBuf, PathBuf) {
    let root = temp_dir(tag);
    let vault = root.join("vault");
    let src = root.join("src");
    std::fs::create_dir_all(&vault).expect("建 vault");
    std::fs::create_dir_all(&src).expect("建源目录");
    (vault, src)
}

fn cleanup(vault: &Path) {
    if let Some(root) = vault.parent() {
        let _ = std::fs::remove_dir_all(root);
    }
}

fn write_enex(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("写 .enex");
    path
}

/// 包一层 ENEX 骨架。
fn enex(notes: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<en-export>\n{notes}\n</en-export>\n")
}

/// 一篇最简笔记。`content` 原样进 CDATA。
fn note(title: &str, content: &str) -> String {
    format!(
        "<note><title>{title}</title><content><![CDATA[<en-note>{content}</en-note>]]></content></note>"
    )
}

/// base64 的 `<resource>`。
fn resource(bytes: &[u8], mime: &str, file_name: Option<&str>) -> String {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let attrs = match file_name {
        Some(name) => {
            format!("<resource-attributes><file-name>{name}</file-name></resource-attributes>")
        }
        None => String::new(),
    };
    format!(
        "<resource><data encoding=\"base64\">{data}</data><mime>{mime}</mime>{attrs}</resource>"
    )
}

fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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

/// 读一篇导进来的笔记。
fn read_note(vault: &Path, name: &str) -> String {
    let path = vault.join(format!("imports/evernote/{name}"));
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("读不到 {}", path.display()))
}

/// 全库任务扫描的结果摊平成 (文本, 是否完成)。
///
/// 走真正的 `scan_vault_tasks` 而不是自己数 `- [ ]`:要断言的正是「导出来的形状**那一个
/// 扫描器**认不认」,自己数等于把被测的判断重写一遍。
fn all_tasks(vault: &Path) -> Vec<(String, bool)> {
    let sources = super::super::tasks::scan_vault_tasks(vault).expect("扫任务");
    sources
        .into_iter()
        .flat_map(|source| source.tasks)
        .map(|task| (task.text, task.checked))
        .collect()
}

/// 全库标签扫描出来的标签名。
fn all_tags(vault: &Path) -> Vec<String> {
    let sources = super::super::tags::scan_vault_tags(vault).expect("扫标签");
    let mut out = Vec::new();
    for source in sources {
        for tag in source.tags {
            out.push(tag.raw);
        }
    }
    out
}

// ─── ENML 保结构 ─────────────────────────────────────────────────────────

#[test]
fn enml_keeps_structure_instead_of_flattening_to_text() {
    // 这是和 Markio 那份 `enml_to_markdown` 最要紧的一处不同:它剥掉所有标签、合并
    // 空白,于是标题变正文、列表变一行、链接只剩文字。
    let (vault, src) = pair("structure");
    let body = "<h1>大标题</h1><ul><li>第一项</li><li>第二项</li></ul>\
                <p><b>粗</b>与<i>斜</i>,还有<a href=\"https://example.com\">链接</a></p>";
    let file = write_enex(&src, "a.enex", &enex(&note("结构", body)));

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1, "报告:{:?}", report.items);

    let text = read_note(&vault, "结构.md");
    assert!(text.contains("# 大标题"), "标题应是 markdown 标题:{text}");
    assert!(text.contains("- 第一项"), "列表项应保留:{text}");
    assert!(text.contains("- 第二项"), "列表项应保留:{text}");
    assert!(text.contains("**粗**"), "加粗应保留:{text}");
    assert!(text.contains("*斜*"), "斜体应保留:{text}");
    assert!(
        text.contains("[链接](https://example.com)"),
        "链接应保留 href:{text}"
    );
    cleanup(&vault);
}

#[test]
fn cdata_html_entities_are_not_decoded_twice() {
    // CDATA 里的 `&lt;p&gt;` 是**字面**的 `<p>` 文本,不是标签。在 XML 层提前解码会
    // 让它变成真标签,于是用户笔记里演示 HTML 的那段被当成排版指令执行掉。
    let (vault, src) = pair("entities");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&note("实体", "<p>写法是 &amp;lt;p&amp;gt; 这样</p>")),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "实体.md");
    assert!(
        text.contains("<p>") || text.contains("&lt;p&gt;"),
        "字面的标签文本应留下来,而不是被解析成排版:{text}"
    );
    cleanup(&vault);
}

// ─── 附件 ────────────────────────────────────────────────────────────────

#[test]
fn resources_land_on_disk_and_the_body_points_at_them() {
    // Markio 完全不看 `<resource>`,每张图都无声无息地没了。
    let (vault, src) = pair("res-land");
    let png = b"\x89PNG\r\n\x1a\nfake";
    let hash = md5_hex(png);
    let body = format!("<p>看图:</p><en-media type=\"image/png\" hash=\"{hash}\"/>");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>带图</title><content><![CDATA[<en-note>{body}</en-note>]]></content>{}</note>",
            resource(png, "image/png", Some("photo.png"))
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1, "报告:{:?}", report.items);
    assert_eq!(report.resource_lost, 0, "配上了就不该记丢失");

    let landed = vault.join("imports/evernote/resources/photo.png");
    assert!(landed.is_file(), "附件应真的落盘");
    assert_eq!(std::fs::read(&landed).expect("读附件"), png, "内容应一致");

    let text = read_note(&vault, "带图.md");
    assert!(
        text.contains("![photo.png](resources/photo.png)"),
        "正文应引用落点(笔记相对):{text}"
    );
    cleanup(&vault);
}

#[test]
fn an_en_media_with_no_matching_resource_is_reported_and_left_visible() {
    // 抹成空白会把「这里本来有个附件」也抹掉,而报告里这一条的意义正是让用户能回
    // Evernote 找回它。
    let (vault, src) = pair("res-missing");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&note(
            "缺图",
            "<en-media type=\"image/png\" hash=\"deadbeefdeadbeefdeadbeefdeadbeef\"/>",
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1, "笔记本身要进来");
    assert_eq!(report.resource_lost, 1, "同时要记一条资源丢失");

    let item = item_for(&report, "缺图");
    assert!(
        matches!(item.status, ItemStatus::Imported),
        "状态是「导入成功」而不是失败 —— 这两层是正交的:{:?}",
        item.status
    );
    assert!(
        item.issues.iter().any(|issue| matches!(
            issue,
            ItemIssue::ResourceLost { target, .. } if target.contains("deadbeef")
        )),
        "issue 里应带上 hash,用户拿它回源端对账:{:?}",
        item.issues
    );

    let text = read_note(&vault, "缺图.md");
    assert!(text.contains("附件丢失"), "正文里要留看得见的痕迹:{text}");
    cleanup(&vault);
}

#[test]
fn the_same_image_embedded_twice_resolves_both_references() {
    // 这一条是按 MD5 配对而不是按出现顺序的理由:一篇笔记嵌同一张图两次时只有**一个**
    // `<resource>` 而有两个引用。顺序匹配会在第二个引用上报「丢了」,或者更糟 —— 给它
    // 配上别人的附件。
    let (vault, src) = pair("res-twice");
    let png = b"same-bytes";
    let hash = md5_hex(png);
    let body = format!(
        "<en-media type=\"image/png\" hash=\"{hash}\"/><p>中间</p><en-media type=\"image/png\" hash=\"{hash}\"/>"
    );
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>两次</title><content><![CDATA[<en-note>{body}</en-note>]]></content>{}</note>",
            resource(png, "image/png", Some("one.png"))
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.resource_lost, 0, "两处都该配上:{:?}", report.items);
    let text = read_note(&vault, "两次.md");
    assert_eq!(
        text.matches("resources/one.png").count(),
        2,
        "两处引用都该指向同一个落点:{text}"
    );
    cleanup(&vault);
}

#[test]
fn two_different_images_each_resolve_to_their_own_file() {
    // 上面那条同图两次的用例,**按顺序配也能过** —— 一篇笔记里只有一个 `<resource>` 时,
    // 「按 hash 找」和「拿第一个」结果一样。真正要 hash 的是这一条:两张不同的图各配各的。
    //
    // 按顺序配会给第二个引用配上第一张图,于是笔记里两处都指向同一个文件,而另一张图
    // 成了没人引用的孤儿。报告里 `resource_lost` 是 0,一切看起来正常 —— 静默配错。
    let (vault, src) = pair("res-two-distinct");
    let first = b"bytes-of-the-first-image";
    let second = b"bytes-of-the-second-image";
    let (h1, h2) = (md5_hex(first), md5_hex(second));
    let body = format!(
        "<p>甲</p><en-media type=\"image/png\" hash=\"{h1}\"/>\
         <p>乙</p><en-media type=\"image/png\" hash=\"{h2}\"/>"
    );
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>两张</title><content><![CDATA[<en-note>{body}</en-note>]]></content>{}{}</note>",
            resource(first, "image/png", Some("alpha.png")),
            resource(second, "image/png", Some("beta.png")),
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.resource_lost, 0, "两张都该配上:{:?}", report.items);
    let text = read_note(&vault, "两张.md");

    // 关键断言:两个引用指向**不同**的文件,且各自对应自己那份字节。
    assert_eq!(
        text.matches("resources/alpha.png").count(),
        1,
        "第一处应指向 alpha:{text}"
    );
    assert_eq!(
        text.matches("resources/beta.png").count(),
        1,
        "第二处应指向 beta,而不是又一次 alpha:{text}"
    );
    let alpha =
        std::fs::read(vault.join("imports/evernote/resources/alpha.png")).expect("读 alpha");
    let beta = std::fs::read(vault.join("imports/evernote/resources/beta.png")).expect("读 beta");
    assert_eq!(alpha, first, "落地的字节要和 hash 对应的那份一致");
    assert_eq!(beta, second);
    // 顺序错配的另一个可观察后果:配错时另一张图没人引用,会被列进「未引用」小节。
    assert!(
        !text.contains("未在正文中引用的附件"),
        "两张都被引用了,不该有孤儿小节:{text}"
    );
    cleanup(&vault);
}

#[test]
fn two_resources_with_different_bytes_do_not_share_a_landing_spot() {
    // 两个都叫 `img.png` 但内容不同:`unique_path` 会把第二个改名,而正文的引用必须
    // 跟着改到**实际**落点上,否则两处引用指向同一个文件(一张图静默变成另一张)。
    let (vault, src) = pair("res-dup-name");
    let first = b"first-bytes";
    let second = b"second-bytes";
    let body = format!(
        "<en-media type=\"image/png\" hash=\"{}\"/><en-media type=\"image/png\" hash=\"{}\"/>",
        md5_hex(first),
        md5_hex(second)
    );
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>同名</title><content><![CDATA[<en-note>{body}</en-note>]]></content>{}{}</note>",
            resource(first, "image/png", Some("img.png")),
            resource(second, "image/png", Some("img.png"))
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.resource_lost, 0, "报告:{:?}", report.items);

    let dir = vault.join("imports/evernote/resources");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("读附件目录")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 2, "两份不同内容应落成两个文件:{names:?}");

    let text = read_note(&vault, "同名.md");
    for name in &names {
        assert!(
            text.contains(&format!("resources/{name}")),
            "正文应引用实际落点 {name}:{text}"
        );
    }
    cleanup(&vault);
}

#[test]
fn a_hash_like_attribute_name_is_not_mistaken_for_hash() {
    // `attribute` 要求属性名前面是空白,否则 `data-hash="…"` 里的 `hash` 也会命中。
    // 命中了的话读到的是**另一个**属性的值,那个值不是任何附件的 MD5,于是这处引用被
    // 判成「附件丢了」—— 而附件其实好好地在文件里,只是没人去要它。
    let (vault, src) = pair("attr-prefix");
    let png = b"real-image-bytes";
    let hash = md5_hex(png);
    // `data-hash` 排在真 `hash` **前面**:按从左到右扫的话先撞上的是它。
    let body = format!("<en-media data-hash=\"deadbeef\" type=\"image/png\" hash=\"{hash}\"/>");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>属性</title><content><![CDATA[<en-note>{body}</en-note>]]></content>{}</note>",
            resource(png, "image/png", Some("real.png"))
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(
        report.resource_lost, 0,
        "真 hash 应该被读到,不该报丢:{:?}",
        report.items
    );
    let text = read_note(&vault, "属性.md");
    assert!(text.contains("resources/real.png"), "{text}");
    cleanup(&vault);
}

#[test]
fn a_tag_whose_name_merely_starts_with_en_media_is_left_alone() {
    // `starts_with_tag_name` 在标签名之后还要求一个分隔符。只 `starts_with` 的话
    // `<en-mediax>` 会被当成 `<en-media>` 处理 —— 它没有 hash,于是凭空多出一条
    // 「附件丢失」,而正文里那个标签本来只是个普通的未知标签。
    let (vault, src) = pair("tag-delim");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&note("相似标签", "<p>甲<en-mediax/>乙</p>")),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    assert_eq!(
        report.resource_lost, 0,
        "不该凭空报一条附件丢失:{:?}",
        report.items
    );
    let text = read_note(&vault, "相似标签.md");
    assert!(!text.contains("附件丢失"), "{text}");
    assert!(
        text.contains('甲') && text.contains('乙'),
        "两侧文字都要在:{text}"
    );
    cleanup(&vault);
}

#[test]
fn a_resource_nobody_references_is_listed_at_the_end() {
    // 不列的话文件在磁盘上、笔记里却没有任何入口 —— 对用户来说和丢了一样,而且更糟:
    // 占着空间还找不到。
    let (vault, src) = pair("res-orphan");
    let pdf = b"%PDF-1.4 fake";
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>孤儿</title><content><![CDATA[<en-note><p>正文没引用</p></en-note>]]></content>{}</note>",
            resource(pdf, "application/pdf", Some("spec.pdf"))
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.resource_lost, 0, "它没丢,只是没被引用");
    assert!(vault.join("imports/evernote/resources/spec.pdf").is_file());
    let text = read_note(&vault, "孤儿.md");
    assert!(text.contains("未在正文中引用的附件"), "应有兜底小节:{text}");
    assert!(
        text.contains("[spec.pdf](resources/spec.pdf)"),
        "非图片走普通链接,不是 `![]()`:{text}"
    );
    cleanup(&vault);
}

#[test]
fn a_non_image_resource_is_a_link_not_an_image() {
    // `![](x.pdf)` 只会渲染成一个坏掉的图片框。
    let (vault, src) = pair("res-pdf");
    let pdf = b"%PDF fake";
    let hash = md5_hex(pdf);
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>附件</title><content><![CDATA[<en-note><en-media type=\"application/pdf\" hash=\"{hash}\"/></en-note>]]></content>{}</note>",
            resource(pdf, "application/pdf", Some("doc.pdf"))
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "附件.md");
    assert!(
        text.contains("[doc.pdf](resources/doc.pdf)"),
        "应是链接:{text}"
    );
    assert!(!text.contains("![doc.pdf]"), "不该是图片:{text}");
    cleanup(&vault);
}

#[test]
fn a_resource_without_a_file_name_gets_an_extension_from_its_mime() {
    let (vault, src) = pair("res-noname");
    let gif = b"GIF89a fake";
    let hash = md5_hex(gif);
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "<note><title>无名</title><content><![CDATA[<en-note><en-media type=\"image/gif\" hash=\"{hash}\"/></en-note>]]></content>{}</note>",
            resource(gif, "image/gif", None)
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let dir = vault.join("imports/evernote/resources");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .expect("读附件目录")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].ends_with(".gif"), "应按 mime 补扩展名:{names:?}");
    cleanup(&vault);
}

#[test]
fn undecodable_base64_is_reported_as_a_lost_resource() {
    let (vault, src) = pair("res-bad-b64");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(
            "<note><title>坏图</title><content><![CDATA[<en-note><p>x</p></en-note>]]></content>\
             <resource><data encoding=\"base64\">!!!not-base64!!!</data><mime>image/png</mime>\
             <resource-attributes><file-name>broken.png</file-name></resource-attributes></resource></note>",
        ),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1, "笔记本身要进来");
    assert_eq!(report.resource_lost, 1, "报告:{:?}", report.items);
    let item = item_for(&report, "坏图");
    assert!(
        item.issues.iter().any(|issue| matches!(
            issue,
            ItemIssue::ResourceLost { target, .. } if target == "broken.png"
        )),
        "issue 应指名是哪个附件:{:?}",
        item.issues
    );
    cleanup(&vault);
}

// ─── en-todo ─────────────────────────────────────────────────────────────

#[test]
fn en_todo_becomes_a_task_the_scanner_can_find() {
    // `<en-todo>` 是 ENML 的复选框。剥标签之后什么都不剩,于是导进来的清单在
    // 「全库待办」里一条都看不到。
    let (vault, src) = pair("todo");
    let body = "<div><en-todo checked=\"false\"/>买牛奶</div>\
                <div><en-todo checked=\"true\"/>交报销</div>";
    let file = write_enex(&src, "a.enex", &enex(&note("清单", body)));

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "清单.md");
    assert!(text.contains("- [ ] 买牛奶"), "未完成:{text}");
    assert!(text.contains("- [x] 交报销"), "已完成:{text}");

    // 断言的是**可观测的后果**:全库任务扫描真的数得出这两条。只验字符串的话,
    // 一个 `tasks.rs` 不认的形状(比如少了 `- ` 前面的空格)照样能过。
    let scanned = all_tasks(&vault);
    let checked_of = |want: &str| {
        scanned
            .iter()
            .find(|(text, _)| text == want)
            .map(|(_, checked)| *checked)
    };
    assert_eq!(checked_of("买牛奶"), Some(false), "未完成:{scanned:?}");
    assert_eq!(checked_of("交报销"), Some(true), "已完成:{scanned:?}");
    cleanup(&vault);
}

#[test]
fn a_mid_line_en_todo_does_not_fake_a_task() {
    // `tasks.rs::task_line` 要求列表标记在行首。一个出现在行中间的 `<en-todo>` 转成
    // `- [ ]` 只会得到一行扫不出来的假待办,所以那种情况如实用字符表示。
    let (vault, src) = pair("todo-mid");
    let body = "<div>先做这个 <en-todo checked=\"false\"/> 再做那个</div>";
    let file = write_enex(&src, "a.enex", &enex(&note("行中", body)));

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "行中.md");
    assert!(text.contains('☐'), "行中间的复选框用字符表示:{text}");
    // 这一条必须走真的扫描器。只断言「正文里没有 `- [ ] 再做那个`」是空的:放宽行首判断
    // 之后,标记会加在**行首**(`- [ ] 先做这个 ☐ 再做那个`),那个子串照样不出现,而
    // 一条把整行当描述的假待办已经产生了 —— 正是这条用例要拦的东西。
    let scanned = all_tasks(&vault);
    assert!(
        scanned.is_empty(),
        "行中间的复选框不该变成任务:{scanned:?} / {text}"
    );
    cleanup(&vault);
}

#[test]
fn an_empty_en_todo_does_not_become_an_empty_task_shell() {
    // 空壳 `- [ ]` 后面没内容,`tasks.rs` 和 marked 都不认它 —— 写出去只是噪声。
    let (vault, src) = pair("todo-empty");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&note(
            "空壳",
            "<div><en-todo checked=\"false\"/></div><p>正文</p>",
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "空壳.md");
    let scanned = all_tasks(&vault);
    assert!(scanned.is_empty(), "不该有任务:{scanned:?} / {text}");
    // 扫描器这一侧拦不住空壳退化:`tasks.rs` 本来就不认没内容的 `- [ ]`,所以去掉退回
    // 字符那一支之后扫出来还是零条,断言照样过。差别在**正文**里 —— 要么是可见的 ☐,
    // 要么是一行光秃秃的 `- [ ]`。后者在预览里渲染成一个空复选框,是纯噪声。
    assert!(text.contains('☐'), "空壳应退回可见字符:{text}");
    assert!(
        !text.lines().any(|line| line.trim() == "- [ ]"),
        "不该留一行空复选框:{text}"
    );
    cleanup(&vault);
}

// ─── 标签 ────────────────────────────────────────────────────────────────

#[test]
fn tags_land_inline_so_the_tag_cloud_sees_them() {
    // `tags.rs` 只索引正文里的行内 `#tag`(它开头那段注释写明 frontmatter 的 `tags:`
    // 是另一套机制)。写进 frontmatter 的话标签云里一个都看不到。
    let (vault, src) = pair("tags");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(
            "<note><title>带标签</title><content><![CDATA[<en-note><p>x</p></en-note>]]></content>\
             <tag>工作</tag><tag>读书</tag></note>",
        ),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "带标签.md");
    assert!(text.contains("#工作"), "{text}");
    assert!(text.contains("#读书"), "{text}");

    let names = all_tags(&vault);
    assert!(
        names.iter().any(|tag| tag == "工作"),
        "标签云应看到:{names:?}"
    );
    assert!(
        names.iter().any(|tag| tag == "读书"),
        "标签云应看到:{names:?}"
    );
    cleanup(&vault);
}

#[test]
fn a_tag_with_a_space_is_rewritten_and_the_change_is_reported() {
    // Evernote 的标签允许空格,而 `tags.rs::is_tag_char` 不认。原样写 `#读书 笔记` 会被
    // 索引成 `#读书`,后半截静默变成正文 —— 用户以为标签导进来了,其实缺了一半。
    let (vault, src) = pair("tags-space");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(
            "<note><title>空格标签</title><content><![CDATA[<en-note><p>x</p></en-note>]]></content>\
             <tag>读书 笔记</tag></note>",
        ),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.degraded, 1, "改写过要记账:{:?}", report.items);
    let item = item_for(&report, "空格标签");
    assert!(
        item.issues.iter().any(
            |issue| matches!(issue, ItemIssue::Degraded { detail } if detail.contains("读书"))
        ),
        "issue 里要说清改成了什么:{:?}",
        item.issues
    );

    let names = all_tags(&vault);
    assert!(
        names.iter().any(|tag| tag == "读书-笔记"),
        "整个标签都该在,而不是只剩前半截:{names:?}"
    );
    cleanup(&vault);
}

// ─── 去重与幂等 ──────────────────────────────────────────────────────────

#[test]
fn importing_the_same_file_twice_skips_instead_of_duplicating() {
    let (vault, src) = pair("idem");
    let file = write_enex(&src, "a.enex", &enex(&note("一次", "<p>正文</p>")));

    let first = evernote::import(&vault, &file).expect("第一次");
    assert_eq!(first.imported, 1);
    let second = evernote::import(&vault, &file).expect("第二次");
    assert_eq!(second.imported, 0, "第二次不该再导:{:?}", second.items);
    assert_eq!(second.skipped, 1);
    assert!(matches!(
        item_for(&second, "一次").status,
        ItemStatus::Skipped {
            reason: SkipReason::AlreadyImported
        }
    ));
    assert!(
        !vault.join("imports/evernote/一次-2.md").exists(),
        "不该留下副本"
    );
    cleanup(&vault);
}

#[test]
fn two_notes_sharing_a_title_both_arrive() {
    // Evernote 里同名笔记很常见(十篇「会议记录」)。只按标题算指纹会把后面九篇当成
    // 重复跳掉 —— 那是静默丢数据。
    let (vault, src) = pair("same-title");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "{}{}",
            note("会议记录", "<p>周一:讨论 A</p>"),
            note("会议记录", "<p>周二:讨论 B</p>")
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 2, "两篇都该进来:{:?}", report.items);
    assert!(vault.join("imports/evernote/会议记录.md").is_file());
    assert!(
        vault.join("imports/evernote/会议记录-2.md").is_file(),
        "第二篇改名落下,后缀在扩展名**之前**否则它不再是笔记"
    );
    cleanup(&vault);
}

// ─── 源端形状与失败 ──────────────────────────────────────────────────────

#[test]
fn a_directory_of_enex_files_imports_all_of_them() {
    // Evernote 是按笔记本导出的,一个用户通常拿到十几个 `.enex`。
    let (vault, src) = pair("dir");
    write_enex(&src, "one.enex", &enex(&note("甲", "<p>1</p>")));
    write_enex(&src, "two.enex", &enex(&note("乙", "<p>2</p>")));
    std::fs::write(src.join("readme.txt"), "不是 enex").expect("写干扰文件");

    let report = evernote::import(&vault, &src).expect("导入成功");
    assert_eq!(report.imported, 2, "报告:{:?}", report.items);
    assert!(vault.join("imports/evernote/甲.md").is_file());
    assert!(vault.join("imports/evernote/乙.md").is_file());
    cleanup(&vault);
}

#[test]
fn one_broken_file_does_not_stop_the_others() {
    // 用户可能有十个笔记本,其中一个坏掉不该让另外九个也进不来。
    let (vault, src) = pair("partial");
    write_enex(&src, "aaa.enex", &enex(&note("好的", "<p>ok</p>")));
    write_enex(&src, "bbb.enex", "<en-export><note><title>没闭合");

    let report = evernote::import(&vault, &src).expect("整轮不该失败");
    assert_eq!(report.failed, 1, "坏的那份记成失败:{:?}", report.items);
    assert!(
        matches!(
            item_for(&report, "bbb.enex").status,
            ItemStatus::Failed { .. }
        ),
        "失败要记在**文件**上,用户拿它回源端找那一份"
    );
    assert!(
        vault.join("imports/evernote/好的.md").is_file(),
        "另一份必须照常进来"
    );
    cleanup(&vault);
}

#[test]
fn a_truncated_file_still_yields_the_half_note_it_did_parse() {
    // 报了「这份坏了」之后再把解出来的内容一起丢掉,等于让用户既知道坏了、又拿不到
    // 任何东西。半篇笔记比没有好,而「文件失败」和「这半篇导进来了」在报告里是两条。
    let (vault, src) = pair("truncated");
    let file = write_enex(
        &src,
        "cut.enex",
        "<en-export><note><title>被截断的</title><content><![CDATA[<en-note><p>正文还在</p></en-note>]]></content>",
    );

    let report = evernote::import(&vault, &file).expect("整轮不该失败");
    assert_eq!(report.failed, 1, "文件要记一条失败:{:?}", report.items);
    assert_eq!(report.imported, 1, "半篇要救回来:{:?}", report.items);
    let text = read_note(&vault, "被截断的.md");
    assert!(text.contains("正文还在"), "救回来的正文应完整:{text}");
    cleanup(&vault);
}

#[test]
fn a_mismatched_end_tag_is_a_parse_failure() {
    // `check_end_names` 那道:`<title>a</note>` 这种错乱不该被静默接受,否则后面每篇
    // 笔记的字段归属都是错的,而报告里一切正常。
    let (vault, src) = pair("mismatch");
    let file = write_enex(
        &src,
        "bad.enex",
        "<en-export><note><title>错乱</note></title></en-export>",
    );

    let report = evernote::import(&vault, &file).expect("整轮不该失败");
    assert_eq!(report.failed, 1, "应记成失败:{:?}", report.items);
    assert_eq!(
        report.imported, 0,
        "别把错乱的内容当成功:{:?}",
        report.items
    );
    cleanup(&vault);
}

#[test]
fn xml_entities_in_a_title_survive() {
    // 0.41 把实体拆成独立的 `GeneralRef` 事件,不在 `Text` 里。漏掉那一支的话每个
    // 实体都被静默丢掉 —— 标题里的引号和 `&` 就那样消失,而且不报任何错。
    let (vault, src) = pair("entity-title");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&note("A &amp; B &quot;引号&quot; &#39;单&#39;", "<p>x</p>")),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1, "报告:{:?}", report.items);
    let source = &report.items[0].source;
    assert!(source.contains('&'), "`&amp;` 应还原成 `&`:{source}");
    assert!(source.contains('"'), "`&quot;` 应还原成 `\"`:{source}");
    assert!(source.contains('\''), "数字引用应还原:{source}");
    cleanup(&vault);
}

#[test]
fn an_unknown_entity_is_kept_verbatim_instead_of_vanishing() {
    // 认不出就删掉会静默改变正文。留回 `&name;` 至少是可见的。
    let (vault, src) = pair("entity-unknown");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&note("含 &weird; 的标题", "<p>x</p>")),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let source = &report.items[0].source;
    assert!(source.contains("&weird;"), "应原样留着:{source}");
    cleanup(&vault);
}

#[test]
fn a_directory_with_no_enex_fails_the_whole_round() {
    // 这一条是**整轮**没法进行,不是某一条的失败 —— 用户选错了目录,该立刻知道。
    let (vault, src) = pair("empty-dir");
    std::fs::write(src.join("a.md"), "x").expect("写文件");
    let error = evernote::import(&vault, &src).expect_err("应当失败");
    assert!(error.contains(".enex"), "错误要说清缺什么:{error}");
    cleanup(&vault);
}

#[test]
fn a_missing_source_fails_before_touching_the_vault() {
    let (vault, src) = pair("missing");
    let error = evernote::import(&vault, &src.join("nope.enex")).expect_err("应当失败");
    assert!(!error.is_empty());
    cleanup(&vault);
}

// ─── frontmatter ─────────────────────────────────────────────────────────

#[test]
fn front_matter_carries_the_title_and_timestamps() {
    let (vault, src) = pair("front");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(
            "<note><title>元数据</title><content><![CDATA[<en-note><p>x</p></en-note>]]></content>\
             <created>20240131T091500Z</created><updated>20240201T101500Z</updated></note>",
        ),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let text = read_note(&vault, "元数据.md");
    assert!(text.starts_with("---\n"), "应有 frontmatter:{text}");
    assert!(text.contains("title: \"元数据\""), "{text}");
    assert!(text.contains("created: 2024-01-31T09:15:00Z"), "{text}");
    assert!(text.contains("updated: 2024-02-01T10:15:00Z"), "{text}");
    assert!(text.contains("source: evernote"), "{text}");
    cleanup(&vault);
}

#[test]
fn a_title_with_yaml_metacharacters_stays_parseable() {
    // 标题里的冒号和引号直接拼进 frontmatter 会生成解析不了的 YAML,于是**整块**
    // frontmatter 失效 —— 标题、时间全读不出来。
    let (vault, src) = pair("front-quote");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&note("复盘: 他说&quot;好&quot;", "<p>x</p>")),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1);
    let dest = item_for(&report, "复盘: 他说\"好\"")
        .dest
        .as_ref()
        .expect("有落点");
    let text = std::fs::read_to_string(vault.join(dest)).expect("读笔记");

    // 走真正的 frontmatter 扫描器:它是判断「这块 YAML 有没有解析成功」的那个口径。
    // 自己 split 一遍等于把被测的判断重写一遍。
    let sources = super::super::fields::scan_vault_fields(&vault).expect("扫字段");
    let fields = sources
        .iter()
        .find(|source| source.path.ends_with(".md") && source.path.contains("复盘"))
        .unwrap_or_else(|| panic!("应能扫到这篇的 frontmatter:{text}"));
    let title = fields
        .fields
        .iter()
        .find(|field| field.key == "title")
        .unwrap_or_else(|| panic!("frontmatter 应能解析出 title:{text}"));
    assert_eq!(
        title.values,
        vec!["复盘: 他说\"好\"".to_string()],
        "值应原样还原:{text}"
    );
    cleanup(&vault);
}

#[test]
fn a_title_with_two_backslashes_round_trips() {
    // 上面那条**测不出转义有没有做**,这一条是补它的。
    //
    // `vault_index::unquote_scalar` 是宽松的:它只脱最外层那一对引号,再把 `\"` 和 `\\`
    // 还原。于是好几种「其实没转义」的写法都能歪打正着地读回原值:
    //
    //  - 引号:转义过的 `"他说\"好\""` 和没转义的 `"他说"好""`,脱掉最外层之后都是
    //    `他说"好"` —— 一模一样。而后者是一块严格 YAML 解析器读不了的 frontmatter。
    //  - 单个末尾反斜杠:`"路径 C:\"` 里那个反斜杠看着像把结束引号转义掉了,但
    //    `strip_suffix` 不认转义,照样把最后那个引号脱掉,剩下 `路径 C:\` —— 又是原值。
    //
    // 真正能把两者分开的是**两个连续**反斜杠。转义过的写成四个,还原后是两个;没转义的
    // 写成两个,还原后变成**一个** —— 少了一个字符,而这是标题,标题是要显示给人看的。
    let (vault, src) = pair("front-backslash");
    let title = "路径 C:\\\\x"; // 字面上是:路径 C: 反斜杠 反斜杠 x
    let file = write_enex(&src, "a.enex", &enex(&note(title, "<p>x</p>")));

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1, "{:?}", report.items);
    let dest = item_for(&report, title).dest.as_ref().expect("有落点");
    let text = std::fs::read_to_string(vault.join(dest)).expect("读笔记");

    // 按落点认这一篇。不能只 `find(key == "title")` —— 每一轮都会写一篇**导入报告**,
    // 它也有 title,而且排在前面,于是断言拿到的是报告的标题。
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
    let (vault, src) = pair("untitled");
    let file = write_enex(
        &src,
        "a.enex",
        &enex("<note><content><![CDATA[<en-note><p>没标题</p></en-note>]]></content></note>"),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 1, "报告:{:?}", report.items);
    let dest = item_for(&report, "(无标题笔记)")
        .dest
        .as_ref()
        .expect("有落点");
    assert!(vault.join(dest).is_file(), "落点应真的存在:{dest}");
    assert!(dest.ends_with(".md"), "必须还是笔记文件:{dest}");
    cleanup(&vault);
}

#[test]
fn a_windows_reserved_title_does_not_abort_the_round() {
    // `CON.md` 在 Windows 上创建不出来,而 `?` 会把整轮带走 —— 后面每篇笔记都进不来。
    let (vault, src) = pair("reserved");
    let file = write_enex(
        &src,
        "a.enex",
        &enex(&format!(
            "{}{}",
            note("CON", "<p>1</p>"),
            note("之后", "<p>2</p>")
        )),
    );

    let report = evernote::import(&vault, &file).expect("导入成功");
    assert_eq!(report.imported, 2, "两篇都要进来:{:?}", report.items);
    assert!(
        vault.join("imports/evernote/之后.md").is_file(),
        "保留名之后的笔记必须还在"
    );
    let dest = item_for(&report, "CON").dest.as_ref().expect("有落点");
    assert!(!dest.ends_with("/CON.md"), "保留名应被改写:{dest}");
    cleanup(&vault);
}

// ─── 报告 ────────────────────────────────────────────────────────────────

#[test]
fn the_round_writes_a_report_note_into_the_vault() {
    let (vault, src) = pair("report");
    let file = write_enex(&src, "a.enex", &enex(&note("有报告", "<p>x</p>")));
    let report = evernote::import(&vault, &file).expect("导入成功");
    let path = report.report_path.as_ref().expect("应写出报告笔记");
    assert!(vault.join(path).is_file(), "报告笔记应存在:{path}");
    cleanup(&vault);
}
