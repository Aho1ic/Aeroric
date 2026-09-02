//! Apple Notes / 备忘录(macOS 专属):走 `osascript` 让 Notes.app 自己交出笔记。
//!
//! 为什么不直接读 `~/Library/Group Containers/.../NoteStore.sqlite`:那里面是
//! protobuf + zlib,格式随系统版本变,而且绕过沙盒。让 Notes.app 自己导出还有一个
//! 副作用是好的 —— 加密笔记它解不开就报错,我们于是知道「有一篇没拿到」。
//!
//! ## 比 Markio 那份多做的三件事
//!
//! **一:分隔符不再能被正文冲垮。** Markio 用固定的 `---MK-NOTE-SEP---` 切片
//! (`apple_notes.rs:63`),一篇正文里**写了**这串字的笔记会把整个解析切错位 ——
//! 那之后每一篇的标题和正文都是错的,而且报告里一切正常。这里让 AppleScript 在拼输出
//! **之前**先把正文里出现的哨兵替换掉,于是「哨兵只出现在记录边界」这条不变量由构造
//! 保证,不是靠赌正文里没有它。
//!
//! **二:锁定的笔记记成 `Skipped`,不是静默丢掉。** Markio 的 `on error` 分支是空的
//! (`apple_notes.rs:41`,注释还写着「用户不会被静默漏掉」——恰好相反:那个分支什么
//! 都不记)。这里分两层 try:先单独取标题,再取正文,于是锁定笔记至少能报出**是哪一篇**。
//!
//! **三:正文走 `html_to_markdown`。** Notes.app 给的 `body` 是 HTML,Markio 拿
//! `enml_to_markdown` 剥标签,列表和加粗全平掉。
//!
//! ## 附件
//!
//! Notes.app 的 `body` 里附件是 `<object>` 之类的占位,附件本体在另一个集合里,取它要
//! 逐个 `save`,而那会对每个附件弹一次授权。这一版**不取附件**,但在正文里认出 `<object>`
//! 占位时记一条 `ResourceLost` —— 用户于是知道该回 Notes.app 找哪一篇的附件。
//!
//! 已知欠账:只认 `<object>`。我没有对着真实 Notes.app 的 HTML 核对过附件还会以哪些形状
//! 出现,所以别的形状会**漏报**。宁可漏报也不按猜的规则数 —— 数错了报告里那句「有 3 个
//! 附件没跟过来」本身就是假的,而这一节的交付物就是那份报告。

use std::path::Path;

use super::super::html2md::html_to_markdown;
use super::guards;
use super::landing;
use super::manifest::{self, Session};
use super::report::{ImportItem, ImportReport, ItemIssue, SkipReason};
use super::run;
use super::zip_common;

pub const PROVIDER: &str = "apple-notes";

/// 记录之间的哨兵。
///
/// AppleScript 那侧会先把正文里出现的这串字替换成 [`SENTINEL_ESCAPE`],所以它在输出里
/// **只**出现在记录边界。解析这侧再把转义还原回来 —— 于是一篇正文里真的写了这串字的
/// 笔记既不会切错位,也不会丢字。
pub(super) const SENTINEL: &str = "\u{1}AERORIC-NOTE\u{1}";
/// 正文里出现哨兵时替换成的东西。还原时换回 [`SENTINEL`]。
pub(super) const SENTINEL_ESCAPE: &str = "\u{1}AERORIC-ESC\u{1}";
/// 标题 / 正文 / 时间之间的字段分隔符。同样会被转义。
pub(super) const FIELD: &str = "\u{2}AERORIC-FIELD\u{2}";
pub(super) const FIELD_ESCAPE: &str = "\u{2}AERORIC-ESC\u{2}";

/// 一条从 Notes.app 读出来的记录。
#[derive(Debug, Default, PartialEq, Eq)]
struct RawNote {
    title: String,
    /// HTML。锁定笔记这里是空的,`locked` 为真。
    body: String,
    created: String,
    updated: String,
    folder: String,
    /// Notes.app 读不出正文(加密 / 锁定)。
    locked: bool,
}

#[cfg(target_os = "macos")]
pub fn import(vault: &Path) -> Result<ImportReport, String> {
    let raw = run_osascript()?;
    import_from_output(vault, &raw)
}

/// 非 macOS 上明确报错,而不是返回一份空报告。
///
/// 空报告会让用户看到「导入成功,0 篇」——那读起来像「Notes.app 里没东西」,而真实
/// 情况是这个平台根本没有 Notes.app。
#[cfg(not(target_os = "macos"))]
pub fn import(_vault: &Path) -> Result<ImportReport, String> {
    Err("Apple Notes 导入仅在 macOS 可用".to_string())
}

/// 调 `osascript` 拿原始输出。
#[cfg(target_os = "macos")]
fn run_osascript() -> Result<String, String> {
    let mut cmd = std::process::Command::new("osascript");
    // 按行传 `-e`,和 `fs.rs::read_clipboard_file_paths_sync` 同一个写法。
    for line in apple_script().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        cmd.arg("-e").arg(line);
    }
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd
        .output()
        .map_err(|error| format!("调用 osascript 失败:{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        // 权限被拒是最常见的一种,单独给一句能照着做的话。
        if stderr.contains("-1743") || stderr.contains("Not authorized") {
            return Err(
                "没有访问「备忘录」的权限。请在「系统设置 → 隐私与安全性 → 自动化」里\
                 允许 Aeroric 控制「备忘录」,然后重试"
                    .to_string(),
            );
        }
        return Err(format!("读取备忘录失败:{stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 拼给 `osascript` 的脚本。
///
/// 单独一个函数是为了让它在**所有平台**上都能被测试看到 —— 脚本里的哨兵和 Rust 这侧
/// 解析用的必须是同一串,而那是编译期看不出来的一致性。测试拿这个函数比对常量。
pub(super) fn apple_script() -> String {
    // AppleScript 的字符串替换用 `text item delimiters`:它是原生实现,比逐字符循环
    // 快几个数量级,一千篇笔记的量级下这个区别是「几秒」和「几分钟」。
    format!(
        r#"on replaceText(theText, oldText, newText)
  set savedDelims to AppleScript's text item delimiters
  set AppleScript's text item delimiters to oldText
  set theItems to every text item of theText
  set AppleScript's text item delimiters to newText
  set theResult to theItems as text
  set AppleScript's text item delimiters to savedDelims
  return theResult
end replaceText

on clean(theText)
  set t to my replaceText(theText, "{SENTINEL}", "{SENTINEL_ESCAPE}")
  set t to my replaceText(t, "{FIELD}", "{FIELD_ESCAPE}")
  return t
end clean

on isoDate(d)
  try
    set y to year of d as integer
    set m to (month of d as integer)
    set dd to day of d as integer
    set hh to hours of d as integer
    set mm to minutes of d as integer
    set ss to seconds of d as integer
    return (y as text) & "-" & my pad(m) & "-" & my pad(dd) & "T" & my pad(hh) & ":" & my pad(mm) & ":" & my pad(ss)
  on error
    return ""
  end try
end isoDate

on pad(n)
  if n < 10 then return "0" & (n as text)
  return n as text
end pad

on run
  set out to ""
  tell application "Notes"
    set allNotes to every note
    repeat with n in allNotes
      set noteTitle to ""
      set noteBody to ""
      set noteCreated to ""
      set noteUpdated to ""
      set noteFolder to ""
      set isLocked to "0"
      try
        set noteTitle to my clean(name of n as text)
      on error
        set noteTitle to ""
      end try
      try
        set noteFolder to my clean(name of container of n as text)
      on error
        set noteFolder to ""
      end try
      try
        set noteCreated to my isoDate(creation date of n)
      on error
        set noteCreated to ""
      end try
      try
        set noteUpdated to my isoDate(modification date of n)
      on error
        set noteUpdated to ""
      end try
      try
        set noteBody to my clean(body of n as text)
      on error
        set isLocked to "1"
      end try
      set out to out & "{SENTINEL}" & noteTitle & "{FIELD}" & noteCreated & "{FIELD}" & noteUpdated & "{FIELD}" & noteFolder & "{FIELD}" & isLocked & "{FIELD}" & noteBody
    end repeat
  end tell
  return out
end run
"#
    )
}

/// 从 `osascript` 的原始输出走完整轮导入。
///
/// 和 [`import`] 分开是为了让整条链路在**所有平台**上都可测:真正调 osascript 的那一步
/// 会弹系统授权框、要读用户真实的备忘录,那不能进测试。
pub(super) fn import_from_output(vault: &Path, raw: &str) -> Result<ImportReport, String> {
    let notes = parse_output(raw);
    run::run(vault, PROVIDER, &mut |session, dest_dir, report| {
        let mut budget = guards::Budget::new();
        for note in &notes {
            if budget.check_entry().is_err() {
                report.push(ImportItem::skipped(
                    display_of(note),
                    SkipReason::LimitReached {
                        limit: guards::LimitHit::Entries.label(),
                    },
                ));
                break;
            }
            let _ = budget.record(note.body.len() as u64);
            land_note(session, dest_dir, note, report);
        }
        Ok(())
    })
}

/// 报告里怎么称呼这一条。
fn display_of(note: &RawNote) -> String {
    let title = note.title.trim();
    if title.is_empty() {
        "(无标题备忘录)".to_string()
    } else {
        title.to_string()
    }
}

/// 切开 `osascript` 的输出。
///
/// 哨兵在 AppleScript 那侧已经从正文里替换掉了,所以这里按哨兵切是安全的;切完再把
/// 转义还原 —— 一篇正文里真的写了哨兵的笔记因此既不会切错位,也不会丢字。
fn parse_output(raw: &str) -> Vec<RawNote> {
    let mut out = Vec::new();
    for chunk in raw.split(SENTINEL) {
        let mut fields = chunk.split(FIELD);
        let title = unescape(fields.next().unwrap_or_default());
        let created = fields.next().unwrap_or_default().trim().to_string();
        let updated = fields.next().unwrap_or_default().trim().to_string();
        let folder = unescape(fields.next().unwrap_or_default());
        let locked = fields.next().unwrap_or_default().trim() == "1";
        // 正文用 `next()` 之后剩下的**全部**:`splitn` 那种写法在字段数变化时会静默
        // 把正文截掉一段,而这里剩下的部分本来就只有正文。
        let body = unescape(&fields.collect::<Vec<_>>().join(FIELD));
        // 这一条同时兜住两件事,所以前面**不再**单独判「这块是不是空的」:
        //  - `split` 在首尾哨兵处各给一个空块,那时标题和正文都是空的;
        //  - 一条什么都没读出来的记录也是同样的形状。
        // 两道守卫时任何一道都能单独兜住(变异测试里去掉前一道全绿),那种冗余会让
        // 「哪一道是承重的」查不出来。锁定的笔记正文本来就是空的,所以 `!locked`。
        if title.trim().is_empty() && body.trim().is_empty() && !locked {
            continue;
        }
        out.push(RawNote {
            title: title.trim().to_string(),
            body,
            created,
            updated,
            folder: folder.trim().to_string(),
            locked,
        });
    }
    out
}

/// 把 AppleScript 那侧做的转义还原。
fn unescape(text: &str) -> String {
    text.replace(SENTINEL_ESCAPE, SENTINEL)
        .replace(FIELD_ESCAPE, FIELD)
}

/// 一条落盘。
fn land_note(session: &mut Session, dest_dir: &Path, note: &RawNote, report: &mut ImportReport) {
    let display = display_of(note);

    if note.locked {
        // 锁定的笔记**记一条**。Markio 的空 `on error` 分支让这些笔记连数都数不出来,
        // 于是用户看到「导入 40 篇」而实际有 43 篇,少的那 3 篇没人提。
        report.push(ImportItem::skipped(
            display,
            SkipReason::Unreadable {
                detail: "备忘录被锁定,Notes.app 未交出正文。解锁后可重新导入".to_string(),
            },
        ));
        return;
    }

    // 指纹:标题 + 正文长度 + 正文前 200 字。Notes.app 的 `id` 形如
    // `x-coredata://.../p123`,看着稳定,但换机 / 重建索引之后会变 —— 拿它做指纹会让
    // 一次迁移之后所有笔记重导一遍。内容指纹在「编辑过的笔记算新条目」上有代价,
    // 但那个代价是多一份文件,而不是少一篇笔记。
    let prefix: String = note.body.chars().take(200).collect();
    let key = manifest::fingerprint(&format!(
        "{PROVIDER}::{}::{}::{prefix}",
        note.title,
        note.body.len()
    ));
    if session.is_known(&key) {
        report.push(ImportItem::skipped(display, SkipReason::AlreadyImported));
        return;
    }

    let mut issues: Vec<ItemIssue> = Vec::new();
    let attachments = count_attachment_placeholders(&note.body);
    if attachments > 0 {
        issues.push(ItemIssue::ResourceLost {
            target: format!("{attachments} 个附件"),
            detail: "备忘录的附件本体不在正文里,这一版不取。请在「备忘录」中手动导出".to_string(),
        });
    }

    let body = html_to_markdown(&note.body, false);
    let content = format!("{}\n{}\n", front_matter(note, &display), body.trim_end());

    // 有文件夹就落到同名子目录下:Notes.app 里的文件夹是用户自己的组织方式,平铺掉
    // 等于把它扔了。子目录名过 `sanitize_relative`,于是 `.notebook` 这类会让索引跳过
    // 整棵子树的名字进不来。
    let relative = match sanitize_folder(&note.folder) {
        Some(folder) => format!("{folder}/{}", note_file_name(&note.title)),
        None => note_file_name(&note.title),
    };

    match zip_common::land_bytes(dest_dir, &relative, content.as_bytes()) {
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

/// 文件夹名洗成一段可用的相对路径。认不出或不该落的返回 `None`(那时笔记落在根上)。
fn sanitize_folder(folder: &str) -> Option<String> {
    let folder = folder.trim();
    if folder.is_empty() {
        return None;
    }
    landing::sanitize_relative(folder)
}

/// 笔记文件名。文件名同时是 wikilink 的目标,所以尽量保住标题的可读形态。
fn note_file_name(title: &str) -> String {
    let stem = landing::sanitize_name(if title.trim().is_empty() {
        "无标题备忘录"
    } else {
        title.trim()
    });
    if stem.to_ascii_lowercase().ends_with(".md") {
        return stem;
    }
    format!("{stem}.md")
}

fn front_matter(note: &RawNote, display: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("title: {}\n", yaml_quote(display)));
    if !note.created.is_empty() {
        out.push_str(&format!("created: {}Z\n", note.created));
    }
    if !note.updated.is_empty() {
        out.push_str(&format!("updated: {}Z\n", note.updated));
    }
    if !note.folder.is_empty() {
        out.push_str(&format!("folder: {}\n", yaml_quote(&note.folder)));
    }
    out.push_str("source: apple-notes\n");
    out.push_str("---\n");
    out
}

/// YAML 标量转义。和 `evernote.rs` / `migrate.rs` 同一个口径。
fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let single_line = escaped.replace('\n', " ").replace('\r', "");
    format!("\"{single_line}\"")
}

/// 正文里有多少个附件占位。
///
/// 只数 `<object>` —— 那是 Notes.app 表示附件的标签。**不猜别的形状**:我没有对着真实
/// Notes.app 的输出核对过 `<img>` 在哪些情况下是附件、哪些情况下是内联图,而按猜的规则
/// 数出来的数字会让报告里那句「有 3 个附件没跟过来」本身就是错的。数不到的附件因此会
/// 漏报,那是已知的欠账,写在模块头上。
fn count_attachment_placeholders(html: &str) -> usize {
    html.to_ascii_lowercase().matches("<object").count()
}
