//! Evernote / 印象笔记:`.enex`(XML,一个文件含多篇笔记)。
//!
//! 四处比 Markio 那份做得多,前两处是这一节准入条件直接要求的。
//!
//! **一:ENML 走 `html_to_markdown`,不是剥标签。** Markio 的 `enml_to_markdown`
//! (`evernote.rs:97`)是「删掉所有 `<tag>`、合并空白」—— 标题、列表、链接、加粗、表格
//! 全变成一堆纯文本行。随手记这边已经有 `notebook/html2md.rs`,保结构。
//!
//! **二:附件真的落盘,配不上才记账。** ENEX 的附件在 `<resource>` 里(base64 的
//! `<data>` + `<mime>`),正文用 `<en-media hash="...">` 引用。Markio 完全不看
//! `<resource>`,于是每张图都无声无息地没了。
//!
//! **三:按 MD5 精确配对,不按出现顺序猜。** `hash` 属性就是附件内容的 MD5。顺序匹配在
//! 「同一张图嵌两次」时会给第二个引用配上别人的附件 —— 那比报「丢了」糟得多。
//!
//! **四:`<en-todo>` 变成真待办。** 它是 ENML 的复选框,剥标签之后什么都不剩。这里转成
//! `- [ ]` / `- [x]`,于是导进来的清单能被 `tasks.rs` 扫到。
//!
//! ## 解析分两层
//!
//! 外层 XML 走 `quick-xml`(ENEX 是良构 XML),内层 `<content>` 是 CDATA 包着的 HTML,
//! 走 `html2md` 的容错 tokenizer —— ENML 里未闭合的 `<br>`、`<div>` 很常见。

use std::collections::HashMap;
use std::path::Path;

use super::super::html2md::html_to_markdown;
use super::guards::{self, Budget};
use super::landing;
use super::manifest::{self, Session};
use super::report::{ImportItem, ImportReport, ItemIssue, SkipReason};
use super::run;
use super::zip_common;

pub const PROVIDER: &str = "evernote";

/// 附件在落点目录下的子目录名。
///
/// 不用 vault 根的 `attachments/`:那里是随手记自己的附件区,导入的东西混进去之后用户
/// 没法把「这次导入」整个撤掉。放在 `imports/evernote/resources/` 下,连同笔记构成一个
/// 可以整体删掉的单元。附件链接是**笔记相对**的(见 `attachments::relative_prefix`),
/// 所以笔记里写 `resources/x.png` 就能解析到。
const RESOURCE_DIR: &str = "resources";

/// 整份 `.enex` 的字节上限。
///
/// 单独一条上限而不是复用 `MAX_ENTRY_BYTES`:`quick-xml` 把一个 `<data>` 元素当**一个**
/// 文本事件交出来,所以单个附件的 base64 会整块进内存,没法在事件层面流式封顶。先按文件
/// 大小拒掉明显过大的,再在累积 base64 时按单体上限二次判 —— 后者才拦得住「文件不大但
/// 单个 resource 巨大」。
const MAX_ENEX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// 私有区里的哨兵,用来在 HTML 转换前后标住 `<en-todo>` 的位置。
///
/// 用 Unicode 私有使用区而不是 `<!-- -->` 或某个自造标签:未知标签会被 `html2md` 当成
/// 透明容器**丢掉**,注释也不保留,而私有区字符是纯文本,原样穿过转换。它同时不会出现在
/// 真实笔记里,所以不会误伤。
const TODO_OPEN: char = '\u{E000}';
const TODO_DONE: char = '\u{E001}';

/// 一篇笔记在 XML 里累积出来的东西。
#[derive(Default)]
struct Note {
    title: String,
    content: String,
    created: String,
    updated: String,
    tags: Vec<String>,
    resources: Vec<Resource>,
    /// 正在读的那个 `<tag>` 的文本。标签文本会被实体拆成多个 Text 事件
    /// (`A &amp; B` 是三个),所以要攒到 `</tag>` 才算一个标签。
    tag_scratch: String,
}

/// ENEX 里的一个 `<resource>`。
#[derive(Default)]
struct Resource {
    /// base64 的 `<data>`。原样存着,落盘时才解 —— 解不出来的要记成资源丢失。
    data: String,
    mime: String,
    /// `<resource-attributes><file-name>`。可能没有。
    file_name: Option<String>,
    /// 累积 base64 时就超了单体上限。超了之后不再往 `data` 里追加,否则一个巨型附件
    /// 照样能把内存顶掉 —— 上限的意义就是**不**把它读完。
    too_large: bool,
}

/// 落好的附件:`hash → 笔记相对链接`,外加它是不是图片。
struct Landed {
    link: String,
    image: bool,
    /// 正文里有没有引用过它。没被引用的最后要在笔记末尾列出来,否则文件在磁盘上而
    /// 笔记里没有任何入口,等于丢了。
    referenced: bool,
}

/// 导入一个 `.enex` 文件,或一个装着若干 `.enex` 的目录。
///
/// 支持目录是因为 Evernote 是**按笔记本**导出的 —— 一个用户通常拿到十几个 `.enex`,
/// 让他一个一个导等于把批处理的活推回给他。
pub fn import(vault: &Path, source: &Path) -> Result<ImportReport, String> {
    let files = collect_enex(source)?;
    run::run(vault, PROVIDER, &mut |session, dest_dir, report| {
        let mut budget = Budget::new();
        for file in &files {
            if let Err(hit) = budget.check_entry() {
                report.push(ImportItem::skipped(
                    display_of(source, file),
                    SkipReason::LimitReached { limit: hit.label() },
                ));
                break;
            }
            let label = display_of(source, file);
            let size = std::fs::metadata(file).map(|meta| meta.len()).unwrap_or(0);
            if size > MAX_ENEX_BYTES {
                report.push(ImportItem::skipped(
                    label,
                    SkipReason::TooLarge { bytes: size },
                ));
                continue;
            }
            if budget.record(size).is_err() {
                report.push(ImportItem::skipped(
                    label,
                    SkipReason::LimitReached {
                        limit: guards::LimitHit::TotalBytes.label(),
                    },
                ));
                break;
            }
            let text = match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(error) => {
                    report.push(ImportItem::skipped(
                        label,
                        SkipReason::Unreadable {
                            detail: error.to_string(),
                        },
                    ));
                    continue;
                }
            };
            match parse_enex(&text) {
                Ok(parsed) => {
                    if parsed.truncated {
                        // 文件本身记一条 `Failed`,同时把解出来的笔记照常落地。这两件事
                        // 不矛盾:用户既该知道这份导出是坏的,也该拿到能救回来的部分。
                        report.push(ImportItem::failed(
                            label,
                            "文件在标签闭合前就结束了(可能被截断),只导入了能解析的部分",
                        ));
                    }
                    for note in parsed.notes {
                        land_note(session, dest_dir, note, report);
                    }
                }
                // 解析失败记在**这个文件**上,不中断整轮:用户可能有十个笔记本,
                // 其中一个坏掉不该让另外九个也进不来。
                Err(detail) => report.push(ImportItem::failed(label, detail)),
            }
        }
        Ok(())
    })
}

/// 收集要处理的 `.enex`。
///
/// 目录只扫一层半:`read_dir` 递归下去意义不大(Evernote 导出是平铺的),而递归会把
/// 上限、软链、深度那一套又拉进来。这里的选择是**只看直接子项**,并在报告里体现不到
/// 深层 —— 与其做半套遍历,不如让范围一眼可知。
fn collect_enex(source: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let meta = std::fs::metadata(source).map_err(|error| format!("读不到源端:{error}"))?;
    if meta.is_file() {
        return Ok(vec![source.to_path_buf()]);
    }
    let entries = std::fs::read_dir(source).map_err(|error| format!("读不到目录:{error}"))?;
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .map(|ext| ext.eq_ignore_ascii_case("enex"))
                    .unwrap_or(false)
        })
        .collect();
    if files.is_empty() {
        return Err("这个目录里没有 .enex 文件".to_string());
    }
    // 排序让同一个目录两次导入的顺序一致 —— 报告里的条目顺序因此可比。
    files.sort();
    Ok(files)
}

/// 报告里怎么称呼这个源文件。
fn display_of(source: &Path, file: &Path) -> String {
    match file.strip_prefix(source) {
        Ok(rest) if !rest.as_os_str().is_empty() => rest.to_string_lossy().into_owned(),
        _ => file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string_lossy().into_owned()),
    }
}

/// 一份 `.enex` 解析出来的东西。
struct Parsed {
    notes: Vec<Note>,
    /// 文件在标签还没闭合时就结束了(截断 / 传输中断)。
    ///
    /// 单独一个标志而不是直接 `Err`:已经解出来的笔记是有价值的,整份判失败会把它们
    /// 一起丢掉。调用方拿它记一条 `Failed`,同时照常落地解出来的部分。
    truncated: bool,
}

/// 解析 ENEX,拿出每篇笔记。
///
/// 用标签**栈**而不是「当前标签」一个变量:`<content>` 里嵌着 HTML 子标签,子标签的
/// Start 会把「当前标签」覆盖掉,那之后的正文全部丢失。栈保证「只要还在 content 里面,
/// 文本就归 content」。
fn parse_enex(text: &str) -> Result<Parsed, String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(text);
    // 闭标签要和开标签对得上,否则 `<title>a</note>` 这种错乱会被静默接受。
    reader.config_mut().check_end_names = true;
    // 不开全局 `trim_text`:那会连 `<content>` 里的空白一起修掉,而 HTML 的行内空白
    // 是有意义的(`a<b>b</b>` 和 `a <b>b</b>` 不一样)。要 trim 的字段各自 trim。
    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut notes: Vec<Note> = Vec::new();
    let mut current = Note::default();
    let mut resource = Resource::default();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(start)) => {
                let name = local_name(start.name().as_ref());
                match name.as_str() {
                    "note" => current = Note::default(),
                    "resource" => resource = Resource::default(),
                    _ => {}
                }
                stack.push(name);
            }
            Ok(Event::End(end)) => {
                let name = local_name(end.name().as_ref());
                match name.as_str() {
                    "note" => notes.push(std::mem::take(&mut current)),
                    "resource" => current.resources.push(std::mem::take(&mut resource)),
                    // 一个 `<tag>` 结束时才把累积的文本收进列表:标签文本会被实体拆成
                    // 多个事件(`A &amp; B` 是三个),收早了会变成三个标签。
                    "tag" => {
                        let tag = std::mem::take(&mut current.tag_scratch);
                        let tag = tag.trim();
                        if !tag.is_empty() {
                            current.tags.push(tag.to_string());
                        }
                    }
                    _ => {}
                }
                stack.pop();
            }
            Ok(Event::Text(event)) => {
                let chunk = event
                    .decode()
                    .map(|text| text.into_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(event.as_ref()).into_owned());
                absorb(&stack, &mut current, &mut resource, &chunk);
            }
            // **实体是独立事件,不在 Text 里。** 0.41 把 `他说&quot;好&quot;` 拆成
            // Text("他说") + GeneralRef("quot") + Text("好") + GeneralRef("quot")。
            // 不处理这一支的话每个实体都被静默丢掉 —— 标题里的引号、标签里的 `&`
            // 就那样消失,而且不报任何错。
            Ok(Event::GeneralRef(event)) => {
                let chunk = resolve_entity(&event);
                absorb(&stack, &mut current, &mut resource, &chunk);
            }
            Ok(Event::CData(event)) => {
                // `<content>` 是 CDATA 包着的 HTML。CDATA 里的内容**不做实体解码** ——
                // 那是 HTML 层的事,提前解会把 `&lt;p&gt;` 变成真标签。
                let chunk = String::from_utf8_lossy(&event.into_inner()).into_owned();
                absorb(&stack, &mut current, &mut resource, &chunk);
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(format!("解析 .enex 失败:{error}")),
            _ => {}
        }
        buf.clear();
    }

    // EOF 时栈非空 = 文件截断。`check_end_names` 抓不到这一种(它比的是闭标签和栈顶,
    // 而截断的文件根本没有那个闭标签),所以要在这里自己判。
    let truncated = !stack.is_empty();
    if truncated && (!current.title.trim().is_empty() || !current.content.is_empty()) {
        // 半篇也留下。反正已经报了「这份文件坏了」,把解出来的内容一起丢掉只是让
        // 用户既知道坏了、又拿不到任何东西。
        notes.push(current);
    }
    Ok(Parsed { notes, truncated })
}

/// 把一个实体引用解成它代表的文本。
///
/// 顺序:数字引用(`&#39;` / `&#x27;`)→ XML 的五个预定义实体 → 一小张 ENEX 里常见的
/// HTML 实体表。
///
/// 不开 quick-xml 的 `escape-html` feature 去拿完整 HTML5 表:那个 `match` 巨大,crate
/// 自己的注释说编译时间要多 10 秒以上(5 倍)。ENEX 是 XML,除那五个之外的实体严格说
/// 就是不合法的,只有个别导出工具会写 `&nbsp;` —— 为几个常见的多花 10 秒不值。
///
/// 认不出的**原样留回** `&name;`。删掉会静默改变正文。
fn resolve_entity(event: &quick_xml::events::BytesRef<'_>) -> String {
    if let Ok(Some(ch)) = event.resolve_char_ref() {
        return ch.to_string();
    }
    let name = event
        .decode()
        .map(|text| text.into_owned())
        .unwrap_or_default();
    if let Some(text) = quick_xml::escape::resolve_xml_entity(&name) {
        return text.to_string();
    }
    let extra = match name.as_str() {
        "nbsp" => "\u{00A0}",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "ldquo" => "\u{201C}",
        "rdquo" => "\u{201D}",
        "middot" => "·",
        "bull" => "•",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "deg" => "°",
        _ => return format!("&{name};"),
    };
    extra.to_string()
}

/// 去掉命名空间前缀。ENEX 一般不带,但带了也不该因此认不出 `<note>`。
fn local_name(raw: &[u8]) -> String {
    let name = String::from_utf8_lossy(raw);
    match name.rsplit(':').next() {
        Some(local) => local.to_ascii_lowercase(),
        None => name.to_ascii_lowercase(),
    }
}

/// 栈里有没有这个标签。
fn inside(stack: &[String], tag: &str) -> bool {
    stack.iter().any(|name| name == tag)
}

/// 把一段文本归到它该去的字段。
fn absorb(stack: &[String], note: &mut Note, resource: &mut Resource, chunk: &str) {
    // resource 优先判:`<resource>` 里也有 `<data>` 和 `<mime>`,而 `<note>` 是它的
    // 祖先,顺序反了 resource 的 base64 会被当成正文追加进去。
    if inside(stack, "resource") {
        if inside(stack, "data") {
            if !resource.too_large {
                resource.data.push_str(chunk.trim());
                // base64 每 4 字符 3 字节。按字符数估算够了 —— 这里要的是「别把
                // 巨型附件读完」,不是精确计量。
                if resource.data.len() as u64 / 4 * 3 > guards::MAX_ENTRY_BYTES {
                    resource.too_large = true;
                    resource.data.clear();
                }
            }
        } else if inside(stack, "mime") {
            resource.mime.push_str(chunk.trim());
        } else if inside(stack, "file-name") {
            resource
                .file_name
                .get_or_insert_with(String::new)
                .push_str(chunk.trim());
        }
        return;
    }
    if inside(stack, "content") {
        note.content.push_str(chunk);
    } else if inside(stack, "title") {
        note.title.push_str(chunk);
    } else if inside(stack, "tag") {
        note.tag_scratch.push_str(chunk);
    } else if inside(stack, "created") {
        note.created.push_str(chunk.trim());
    } else if inside(stack, "updated") {
        note.updated.push_str(chunk.trim());
    }
}

/// 一篇笔记落盘。
fn land_note(session: &mut Session, dest_dir: &Path, note: Note, report: &mut ImportReport) {
    let title = note.title.trim().to_string();
    let display = if title.is_empty() {
        "(无标题笔记)".to_string()
    } else {
        title.clone()
    };

    // 指纹:标题 + 正文长度 + 正文前 200 字。**不能只用标题** —— Evernote 里同名笔记
    // 很常见(十篇「会议记录」),只按标题去重会让后九篇被当成重复跳掉。
    let prefix: String = note.content.chars().take(200).collect();
    let key = manifest::fingerprint(&format!(
        "{PROVIDER}::{title}::{}::{prefix}",
        note.content.len()
    ));
    if session.is_known(&key) {
        report.push(ImportItem::skipped(display, SkipReason::AlreadyImported));
        return;
    }

    let mut issues: Vec<ItemIssue> = Vec::new();
    // 先落附件:正文里的 `<en-media>` 要换成指向它们的链接,所以得先知道落点。
    let mut landed = land_resources(dest_dir, &note.resources, &mut issues);

    let enml = rewrite_enml(&note.content, &mut landed, &mut issues);
    let markdown = html_to_markdown(&enml, false);
    let mut body = restore_todos(&markdown);
    append_orphans(&mut body, &landed);

    let tags = tag_line(&note.tags, &mut issues);
    if !tags.is_empty() {
        // 标签走**行内** `#tag` 而不是 frontmatter 的 `tags:`:`tags.rs` 只索引行内那种
        // (它开头那段注释写明了 frontmatter 是另一套机制)。写进 frontmatter 的话标签云
        // 里一个都看不到,用户会以为标签没导进来。
        body = format!("{tags}\n\n{body}");
    }

    let content = format!("{}\n{}\n", front_matter(&display, &note), body.trim_end());
    let name = note_file_name(&title);
    match zip_common::land_bytes(dest_dir, &name, content.as_bytes()) {
        Ok(target) => {
            session.record(key);
            let mut item = ImportItem::imported(
                display,
                zip_common::dest_relative(PROVIDER, dest_dir, &target),
            );
            item.issues = issues;
            report.push(item);
        }
        Err(detail) => report.push(ImportItem::failed(display, detail)),
    }
}

/// 笔记的文件名。
///
/// 文件名同时是 wikilink 的目标(见 `notebook-filename-is-a-link-target`),所以尽量
/// 保住标题的可读形态;清洗与保留名的处理都在 `landing::sanitize_name` 里。
fn note_file_name(title: &str) -> String {
    let stem = landing::sanitize_name(if title.is_empty() {
        "无标题笔记"
    } else {
        title
    });
    // 标题本身就以 `.md` 结尾时不再加一次,否则得到 `a.md.md`。
    if stem.to_ascii_lowercase().ends_with(".md") {
        return stem;
    }
    format!("{stem}.md")
}

/// frontmatter。
fn front_matter(display: &str, note: &Note) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("title: {}\n", yaml_quote(display)));
    if let Some(iso) = enex_time_to_iso(&note.created) {
        out.push_str(&format!("created: {iso}\n"));
    }
    if let Some(iso) = enex_time_to_iso(&note.updated) {
        out.push_str(&format!("updated: {iso}\n"));
    }
    out.push_str("source: evernote\n");
    out.push_str("---\n");
    out
}

/// YAML 标量转义。和 `migrate.rs::yaml_quote` 同一个口径 —— 标题里可能有冒号、引号、
/// `#`,直接拼进 frontmatter 会生成解析不了的 YAML。
fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let single_line = escaped.replace('\n', " ").replace('\r', "");
    format!("\"{single_line}\"")
}

/// ENEX 的时间戳(`20240131T091500Z`)转 RFC3339。认不出就返回 `None` —— 编一个时间
/// 比没有时间糟。
fn enex_time_to_iso(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed = chrono::NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%SZ").ok()?;
    Some(
        parsed
            .and_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

/// Evernote 的标签转成一行行内 `#tag`。
///
/// Evernote 的标签**允许空格**(「读书 笔记」是一个标签),而 `tags.rs::is_tag_char` 只
/// 认字母数字(含 CJK)、`_`、`-`、`/`。原样写 `#读书 笔记` 会被索引成 `#读书`,后半截
/// 静默变成正文。所以非法字符压成 `-`,并把改过的那些记成 `Degraded` —— 用户据此知道
/// vault 里的标签和 Evernote 里的不是逐字一致。
fn tag_line(tags: &[String], issues: &mut Vec<ItemIssue>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut rewritten: Vec<String> = Vec::new();
    for tag in tags {
        let clean = sanitize_tag(tag);
        if clean.is_empty() {
            rewritten.push(format!("{tag} → (丢弃)"));
            continue;
        }
        if clean != *tag {
            rewritten.push(format!("{tag} → {clean}"));
        }
        out.push(format!("#{clean}"));
    }
    if !rewritten.is_empty() {
        // 一条 issue 汇总所有改动,不是一条标签一条:一篇笔记的标签被改写是**一件**
        // 需要用户知道的事,拆成十条只会把报告冲淡。
        issues.push(ItemIssue::Degraded {
            detail: format!("标签含标签语法不支持的字符,已改写:{}", rewritten.join("、")),
        });
    }
    out.join(" ")
}

/// 一个标签洗成 `tags.rs` 认得的形状。
fn sanitize_tag(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut last_dash = false;
    for ch in tag.trim().chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '/' {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            // 连续的非法字符压成一个 `-`,否则「a  b」会变成「a--b」。
            out.push('-');
            last_dash = true;
        }
    }
    // 首尾的 `-` 和 `/` 摘掉:`normalize_tag` 会摘尾部的,首部的 `-` 则会让
    // `#-a` 里的标签正文变成 `-a`,不好看也不好搜。
    let trimmed = out.trim_matches(['-', '/']).to_string();
    // 纯数字不算标签(`normalize_tag` 明确拒了),写出去等于没写。
    if !trimmed.is_empty() && trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return String::new();
    }
    trimmed
}

/// 落附件,返回 `MD5 十六进制 → 落点` 的映射。
///
/// key 用 `<data>` 解码后内容的 MD5,因为 `<en-media hash>` 就是那个值。自己算而不是
/// 信 ENEX 里可能带的 `<recognition>`:算出来的一定和正文里的引用同源。
fn land_resources(
    dest_dir: &Path,
    resources: &[Resource],
    issues: &mut Vec<ItemIssue>,
) -> HashMap<String, Landed> {
    let mut map: HashMap<String, Landed> = HashMap::new();
    for (index, resource) in resources.iter().enumerate() {
        let label = resource
            .file_name
            .clone()
            .unwrap_or_else(|| format!("附件{}", index + 1));
        if resource.too_large {
            issues.push(ItemIssue::ResourceLost {
                target: label,
                detail: format!("附件超过单体上限({} 字节)", guards::MAX_ENTRY_BYTES),
            });
            continue;
        }
        let Some(bytes) = decode_base64(&resource.data) else {
            issues.push(ItemIssue::ResourceLost {
                target: label,
                detail: "base64 解不开".to_string(),
            });
            continue;
        };
        let hash = md5_hex(&bytes);
        // 同一份内容出现两次(Evernote 偶尔会重复写 resource):落一次就够,第二次
        // 直接复用映射。否则磁盘上会有两份一样的图,而正文只引用得到一份。
        if map.contains_key(&hash) {
            continue;
        }
        let name = resource_name(&label, &resource.mime, index);
        match zip_common::land_bytes(dest_dir, &format!("{RESOURCE_DIR}/{name}"), &bytes) {
            Ok(target) => {
                // 用**实际**落点的文件名:`unique_path` 可能把它改成了 `x-2.png`,
                // 拿原名拼链接会指向一个不存在的文件。
                let landed_name = target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or(name);
                map.insert(
                    hash,
                    Landed {
                        link: format!("{RESOURCE_DIR}/{landed_name}"),
                        image: is_image_mime(&resource.mime),
                        referenced: false,
                    },
                );
            }
            Err(detail) => issues.push(ItemIssue::ResourceLost {
                target: label,
                detail,
            }),
        }
    }
    map
}

fn is_image_mime(mime: &str) -> bool {
    mime.trim().to_ascii_lowercase().starts_with("image/")
}

/// 附件文件名。有 `<file-name>` 就用它,否则按 mime 猜一个扩展名。
fn resource_name(label: &str, mime: &str, index: usize) -> String {
    let clean = landing::sanitize_name(label);
    if Path::new(&clean).extension().is_some() {
        return clean;
    }
    let extension = match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "application/pdf" => "pdf",
        // 认不出的 mime 不编扩展名:错的扩展名会让系统用错的程序去开它,而没有
        // 扩展名至少是诚实的。加序号是为了两个都没名字的附件不撞在一起。
        _ => return format!("{clean}-{}", index + 1),
    };
    format!("{clean}.{extension}")
}

/// base64 解码。走仓库已有的 `base64` 依赖。
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    // ENEX 里的 base64 是折行的,先把空白去掉。
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(compact)
        .ok()
}

/// 内容的 MD5,小写十六进制 —— `<en-media hash>` 的格式。
fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    let digest = Md5::digest(bytes);
    let mut out = String::with_capacity(32);
    for byte in digest.iter() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// ENML 预处理:把 `<en-media>` 和 `<en-todo>` 换成 `html2md` 认得的东西。
///
/// **必须在转 markdown 之前做。** `html2md` 把未知标签当透明容器丢掉,所以转完之后再找
/// `<en-media>` 是找不到的 —— 那正是 Markio 那份丢图的机制。这里换成 `<img>` / `<a>`,
/// 于是链接文本的转义、alt 的处理都由 `html2md` 统一负责。
fn rewrite_enml(
    enml: &str,
    landed: &mut HashMap<String, Landed>,
    issues: &mut Vec<ItemIssue>,
) -> String {
    let mut out = String::with_capacity(enml.len());
    let mut rest = enml;
    loop {
        let Some(found) = next_en_tag(rest) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..found.start]);
        let tag = &rest[found.start..found.end];
        match found.kind {
            EnTag::Media => rewrite_media(tag, landed, issues, &mut out),
            EnTag::Todo => {
                let done = attribute(tag, "checked")
                    .map(|value| {
                        let value = value.trim().to_ascii_lowercase();
                        value == "true" || value == "checked"
                    })
                    .unwrap_or(false);
                out.push(if done { TODO_DONE } else { TODO_OPEN });
            }
        }
        rest = &rest[found.end..];
    }
    out
}

/// 一处 `<en-*>` 的位置。
struct EnFound {
    start: usize,
    end: usize,
    kind: EnTag,
}

enum EnTag {
    Media,
    Todo,
}

/// 找下一个 `<en-media>` 或 `<en-todo>`。
///
/// 手写而不是用 `regex`:这两个标签的形状固定,而正文可能很长,一个跨行的贪婪正则在
/// 未闭合的 HTML 上很容易吃掉一整段。
fn next_en_tag(text: &str) -> Option<EnFound> {
    let mut from = 0usize;
    while let Some(offset) = text[from..].find("<en-") {
        let start = from + offset;
        let after = &text[start + 4..];
        let kind = if starts_with_tag_name(after, "media") {
            EnTag::Media
        } else if starts_with_tag_name(after, "todo") {
            EnTag::Todo
        } else {
            // 别的 `<en-*>`(`<en-crypt>` 加密块)交给 `html2md` 当透明容器处理。
            from = start + 4;
            continue;
        };
        // 找闭合的 `>`。没有的话整段照原样留着 —— 截断会丢正文。
        let end = text[start..].find('>')? + start + 1;
        return Some(EnFound { start, end, kind });
    }
    None
}

/// `after` 是不是以这个标签名开头,且名字之后是**分隔符**。
///
/// 不能只 `starts_with`:那样 `<en-mediax>` 也会被当成 `<en-media>`。
fn starts_with_tag_name(after: &str, name: &str) -> bool {
    let lower = after.to_ascii_lowercase();
    if !lower.starts_with(name) {
        return false;
    }
    match after[name.len()..].chars().next() {
        Some(ch) => ch.is_whitespace() || ch == '/' || ch == '>',
        None => false,
    }
}

/// 一处 `<en-media>` 换成图片或链接。
fn rewrite_media(
    tag: &str,
    landed: &mut HashMap<String, Landed>,
    issues: &mut Vec<ItemIssue>,
    out: &mut String,
) {
    let hash = attribute(tag, "hash").unwrap_or_default();
    let mime = attribute(tag, "type").unwrap_or_default();
    match landed.get_mut(&hash.to_ascii_lowercase()) {
        Some(entry) => {
            entry.referenced = true;
            let name = entry.link.rsplit('/').next().unwrap_or(&entry.link);
            if entry.image || is_image_mime(&mime) {
                out.push_str(&format!("<img src=\"{}\" alt=\"{}\">", entry.link, name));
            } else {
                out.push_str(&format!("<a href=\"{}\">{}</a>", entry.link, name));
            }
        }
        None => {
            // 正文引用了一个 resource,而 `<resource>` 里没有对应的那份(或者它没落成
            // 功)。引用**留一个看得见的痕迹**:抹成空白会把「这里本来有个附件」这件事
            // 也抹掉,而报告里这一条的意义正是让用户能回 Evernote 找回它。
            let shown = if hash.is_empty() { "未知" } else { &hash };
            out.push_str(&format!("<code>[附件丢失 hash={shown}]</code>"));
            issues.push(ItemIssue::ResourceLost {
                target: if hash.is_empty() {
                    "en-media".to_string()
                } else {
                    hash
                },
                detail: "正文引用的附件在 .enex 的 resource 里找不到".to_string(),
            });
        }
    }
}

/// 从一段标签文本里取属性值。只处理双引号和单引号 —— ENEX 是机器生成的,属性总带引号。
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(offset) = lower[from..].find(name) {
        let at = from + offset;
        // 名字前面必须是空白(否则 `data-hash` 里的 `hash` 也会命中),后面必须是 `=`。
        let prefix_ok = at > 0
            && lower[..at]
                .chars()
                .next_back()
                .map(char::is_whitespace)
                .unwrap_or(false);
        let rest = &tag[at + name.len()..];
        let trimmed = rest.trim_start();
        if prefix_ok && trimmed.starts_with('=') {
            let value = trimmed[1..].trim_start();
            let quote = value.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let body = &value[1..];
            let end = body.find(quote)?;
            return Some(body[..end].to_string());
        }
        from = at + name.len();
    }
    None
}

/// 把哨兵换回 markdown 的复选框。
///
/// **只有落在行首的哨兵才变成 `- [ ]`。** `tasks.rs::task_line` 要求列表标记在行首(允许
/// 缩进),所以一个出现在行中间的 `<en-todo>`(「买牛奶 ☐ 记得看保质期」这种)转成
/// `- [ ]` 只会得到一行扫不出来的假待办。那种情况用字符 `☐` / `☑` 如实表示 —— 它不是
/// 待办,但它确实在那儿。
///
/// 空壳也要处理:`- [ ]` 后面没内容时 `tasks.rs` 不认(marked 也不认),所以哨兵后面没有
/// 正文时同样退回字符形式。
fn restore_todos(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    for (index, line) in markdown.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        let first = trimmed.chars().next();
        if matches!(first, Some(TODO_OPEN) | Some(TODO_DONE)) {
            let indent = &line[..line.len() - trimmed.len()];
            let done = first == Some(TODO_DONE);
            let body = trimmed[first.map(char::len_utf8).unwrap_or(0)..].trim_start();
            if body.is_empty() {
                // 空壳:退回字符。
                out.push_str(indent);
                out.push_str(glyphs(done));
                continue;
            }
            let marker = if done { "- [x] " } else { "- [ ] " };
            out.push_str(indent);
            out.push_str(marker);
            // 一行里可能有多个 `<en-todo>`(ENML 的清单常常整段在一个 `<div>` 里),
            // 后面那些不在行首,按上面的规则用字符。
            out.push_str(&replace_glyphs(body));
            continue;
        }
        out.push_str(&replace_glyphs(line));
    }
    out
}

/// 行中间的哨兵换成可见字符。
fn replace_glyphs(text: &str) -> String {
    if !text.contains(TODO_OPEN) && !text.contains(TODO_DONE) {
        // 绝大多数行不含哨兵,不必为它们各分配一个 String。
        return text.to_string();
    }
    text.replace(TODO_OPEN, glyphs(false))
        .replace(TODO_DONE, glyphs(true))
}

fn glyphs(done: bool) -> &'static str {
    if done {
        "☑"
    } else {
        "☐"
    }
}

/// 没被正文引用的附件列在笔记末尾。
///
/// 不列的话文件在磁盘上、笔记里却没有任何入口 —— 对用户来说和丢了一样,而且比丢了更糟
/// (占着空间还找不到)。这不记 `Degraded`:内容一点没少,只是位置和 Evernote 里不同,
/// 而报告里的 `dest` 已经指到笔记了。
fn append_orphans(body: &mut String, landed: &HashMap<String, Landed>) {
    let mut orphans: Vec<&Landed> = landed
        .values()
        .filter(|entry| !entry.referenced)
        .collect::<Vec<_>>();
    if orphans.is_empty() {
        return;
    }
    // HashMap 的迭代顺序不稳定,按链接排一下,同一份 ENEX 两次导入的结果才一致。
    orphans.sort_by(|left, right| left.link.cmp(&right.link));
    body.push_str("\n\n## 未在正文中引用的附件\n\n");
    for entry in orphans {
        let name = entry.link.rsplit('/').next().unwrap_or(&entry.link);
        if entry.image {
            body.push_str(&format!("- ![{name}]({})\n", entry.link));
        } else {
            body.push_str(&format!("- [{name}]({})\n", entry.link));
        }
    }
}
