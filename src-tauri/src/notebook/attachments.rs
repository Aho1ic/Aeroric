//! 附件:图片和其他非笔记文件的落盘、枚举与读取。
//!
//! 落点是 `<vault>/attachments/`,**平铺**、不按笔记分目录。理由是附件会被多条
//! 笔记引用(同一张图贴进两处很常见),按笔记分目录的话第二条笔记引到的是别人
//! 目录下的文件 —— 删掉第一条笔记时那张图跟着走,第二条笔记就断图了。
//!
//! 插入笔记的 markdown 路径是**相对笔记所在目录**的,而不是相对 vault 根:
//! `attachments/x.png` 只在根下的笔记里成立,子目录里的笔记要写
//! `../attachments/x.png`。这样产出的 markdown 在别的编辑器 / 静态站点生成器里
//! 也是对的 —— 随手记不是这些文件唯一的读者。
//!
//! 文件名是 `<笔记名>-<时间戳>.<ext>`,抢名字用 `create_new`。和回收站一样:
//! "先 exists 再写"在同毫秒粘两张图时会让两边都看到空位,后写的覆盖先写的。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::fs_ops;

/// 附件目录(相对 vault 根)。
pub const ATTACHMENT_DIR: &str = "attachments";

/// 单个附件的上限。再大的文件不该塞进笔记 —— 读进 WebView 做 blob 会吃掉
/// 同等大小的内存。
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

/// 同毫秒内最多能塞多少张。抢到这个数还没空位就报错,而不是无限循环。
const MAX_SAME_MS_FILES: u32 = 1000;

/// 枚举附件的默认上限。
pub const DEFAULT_LIST_LIMIT: usize = 500;

const MAX_SCAN_DEPTH: usize = 12;

/// 附件列表里的一条。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// 绝对路径。前端读取 / 在文件管理器里显示都用它。
    pub path: String,
    pub name: String,
    /// 相对 vault 根的路径,`/` 分隔。UI 用它显示"这个附件在哪"。
    pub relative_path: String,
    pub size: u64,
    pub modified_ms: i64,
    /// `image` / `svg` / `pdf` / `video` / `audio` / `word` / `sheet` /
    /// `slides` / `archive`。
    pub kind: &'static str,
}

/// 刚存下来的附件。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAttachment {
    pub path: String,
    pub name: String,
    /// 相对**笔记所在目录**的路径,可直接写进 markdown。
    pub link: String,
    /// 现成的 markdown 片段。图片带 `!` 前缀,其余是普通链接 —— 把
    /// `![](x.pdf)` 插进笔记只会渲染成一个坏掉的图片框。
    pub markdown: String,
    pub size: u64,
}

/// 按扩展名判附件类型。认不出的返回 `None` —— 附件面板只列认得出的类型,
/// 否则 vault 里任何一个 `.DS_Store` 或 `.tmp` 都会跑进列表。
pub fn kind_of(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.')?.1;
    Some(match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "heic" | "avif" => {
            "image"
        }
        "svg" => "svg",
        "pdf" => "pdf",
        "mp4" | "mov" | "m4v" | "webm" | "avi" | "mkv" => "video",
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" => "audio",
        "docx" | "doc" | "pages" | "rtf" => "word",
        "xlsx" | "xls" | "numbers" | "csv" => "sheet",
        "pptx" | "ppt" | "key" => "slides",
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" => "archive",
        _ => return None,
    })
}

/// 附件目录的绝对路径。
pub fn dir(vault: &Path) -> PathBuf {
    vault.join(ATTACHMENT_DIR)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|delta| delta.as_millis() as u64)
        .unwrap_or(0)
}

/// 把任意串洗成安全的文件名主干。
///
/// 只留字母数字和 `-_`,其余(空格、CJK 标点、路径分隔符、控制字符)一律换成
/// `-`。CJK 本身**保留** —— 中文笔记名占多数,洗成 `----` 会让附件目录里全是
/// 认不出来的文件。
fn sanitize_stem(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        let keep = ch.is_alphanumeric() || ch == '-' || ch == '_';
        if keep {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    // 上限不是美观问题:多数文件系统的单段名上限是 255 **字节**,而一个 CJK
    // 字符占 3 字节。按字符截断到 40 个,最坏 120 字节,留足给时间戳和扩展名。
    let truncated: String = out.chars().take(40).collect();
    if truncated.is_empty() {
        "attachment".to_string()
    } else {
        truncated
    }
}

/// 从 MIME 或原文件名推扩展名。两者都认不出时给 `bin`。
fn extension_for(mime: &str, file_name: Option<&str>) -> String {
    let from_name = file_name
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| {
            !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|ch| ch.is_ascii_alphanumeric())
        });
    if let Some(ext) = from_name {
        return ext;
    }
    match mime.to_ascii_lowercase().trim() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        "image/tiff" => "tiff",
        "image/heic" => "heic",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        _ => "bin",
    }
    .to_string()
}

/// 笔记目录 → 附件目录的相对前缀(`../` 若干个)。
///
/// 笔记在 `<vault>/a/b/note.md` 时附件要写 `../../attachments/x.png`。数的是
/// 笔记**目录**相对 vault 根的层数,不是笔记路径本身的层数。
fn relative_prefix(vault: &Path, note: &Path) -> Result<String, String> {
    let parent = note
        .parent()
        .ok_or_else(|| "Cannot resolve the note directory".to_string())?;
    let relative = parent
        .strip_prefix(vault)
        .map_err(|_| format!("{} is outside the vault", note.display()))?;
    let depth = relative
        .components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count();
    Ok("../".repeat(depth))
}

/// 抢一个没被占用的文件名并把内容写进去,返回最终路径。
///
/// 抢名字的原子性靠 `create_new`。直接写进抢到的那个 handle(不是 tmp →
/// rename):附件的源还在别处(剪贴板 / 磁盘上的原文件),崩在中间留下的半张图
/// 顶多渲染不出来,而笔记正文是没有第二份的,那里才需要 tmp → rename 那套。
/// 时间戳是参数而不是就地取:两次真实保存之间隔着一次 `sync_all`,现实里几乎
/// 永远落在不同毫秒,于是"同毫秒撞名"这条路径在测试里根本走不到。把 stamp 交出来
/// 才能确定性地验它 —— 这也是这个函数对模块内可见的原因。
pub(super) fn write_claimed(
    dir: &Path,
    stem: &str,
    ext: &str,
    stamp: u64,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    for suffix in 0..MAX_SAME_MS_FILES {
        let name = if suffix == 0 {
            format!("{stem}-{stamp}.{ext}")
        } else {
            format!("{stem}-{stamp}-{suffix}.{ext}")
        };
        let path = dir.join(&name);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot write {}: {error}", path.display())),
        };
        let written = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("Cannot write {}: {e}", path.display()));
        if let Err(error) = written {
            // 写坏了就把占的名字让出来,别在附件目录里留一个空壳文件。
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        return Ok(path);
    }
    Err("Too many attachments were added in the same millisecond".to_string())
}

/// 把一段文字洗成能安全放进 `![...]` 的 alt。
///
/// `]` 会提前闭合 alt,换行会把链接语法整个截断 —— 一个叫 `a]b.png` 的文件足以
/// 产出一条渲染成字面文本的坏链接。
fn sanitize_alt(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .map(|ch| match ch {
            '[' | ']' => '-',
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect();
    cleaned.trim().to_string()
}

fn finish(
    vault: &Path,
    note: &Path,
    path: PathBuf,
    alt_source: &str,
    bytes: usize,
) -> Result<SavedAttachment, String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "Invalid attachment file name".to_string())?;
    let prefix = relative_prefix(vault, note)?;
    let link = format!("{prefix}{ATTACHMENT_DIR}/{name}");
    // alt 用**原始**主干而不是最终文件名:最终文件名里带着毫秒时间戳,图渲染
    // 不出来时页面上就会显示一串数字,用户看不出那本来是什么。
    let alt = match sanitize_alt(alt_source) {
        alt if alt.is_empty() => sanitize_alt(&name),
        alt => alt,
    };
    let image = matches!(kind_of(&name), Some("image") | Some("svg"));
    let markdown = if image {
        format!("![{alt}]({link})")
    } else {
        // 非图片走普通链接:`![](x.pdf)` 只会渲染成一个坏掉的图片框。
        format!("[{name}]({link})")
    };
    Ok(SavedAttachment {
        path: path.to_string_lossy().into_owned(),
        name,
        link,
        markdown,
        size: bytes as u64,
    })
}

/// 存一份字节流(剪贴板粘贴走这条)。
pub fn save_bytes(
    vault: &Path,
    note: &Path,
    file_name: Option<&str>,
    mime: &str,
    bytes: &[u8],
) -> Result<SavedAttachment, String> {
    if bytes.is_empty() {
        return Err("The attachment is empty".to_string());
    }
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "The attachment is too large ({} bytes, limit {MAX_ATTACHMENT_BYTES})",
            bytes.len()
        ));
    }
    if !note.starts_with(vault) {
        return Err(format!("{} is outside the vault", note.display()));
    }
    let stem = sanitize_stem(
        note.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("note"),
    );
    let ext = extension_for(mime, file_name);
    let path = write_claimed(&dir(vault), &stem, &ext, now_ms(), bytes)?;
    // alt 优先用来源文件名的主干,剪贴板里的图没有文件名,退回笔记名。
    let alt_source = file_name
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or(&stem);
    finish(vault, note, path, alt_source, bytes.len())
}

/// 把磁盘上的一个文件复制进附件目录(从文件管理器拖入走这条)。
///
/// Rust 直读源文件,不让前端先 base64 —— 一张 8MB 的图编码成 base64 是 11MB
/// 的字符串,穿过 IPC 再解回来纯属浪费。
pub fn save_from_path(vault: &Path, note: &Path, src: &Path) -> Result<SavedAttachment, String> {
    let meta = std::fs::symlink_metadata(src)
        .map_err(|e| format!("Cannot read {}: {e}", src.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a symbolic link", src.display()));
    }
    if !meta.is_file() {
        return Err(format!("{} is not a file", src.display()));
    }
    if meta.len() as usize > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "The attachment is too large ({} bytes, limit {MAX_ATTACHMENT_BYTES})",
            meta.len()
        ));
    }
    let bytes = std::fs::read(src).map_err(|e| format!("Cannot read {}: {e}", src.display()))?;
    let file_name = src.file_name().and_then(|name| name.to_str());
    save_bytes(vault, note, file_name, "", &bytes)
}

/// 列出 vault 里的附件,新的在前。
///
/// 扫的是**整个 vault**而不只是 `attachments/`:用户从别处导入的笔记会把图片
/// 放在笔记旁边,只列自己写下的那些等于对导入的内容视而不见。
pub fn list(vault: &Path, max: usize) -> Result<Vec<Attachment>, String> {
    let max = max.clamp(1, DEFAULT_LIST_LIMIT);
    let mut out = Vec::new();
    scan(vault, vault, 0, max, &mut out);
    out.sort_by(|left, right| {
        right
            .modified_ms
            .cmp(&left.modified_ms)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    out.truncate(max);
    Ok(out)
}

fn scan(vault: &Path, current: &Path, depth: usize, max: usize, out: &mut Vec<Attachment>) {
    if depth >= MAX_SCAN_DEPTH || out.len() >= max {
        return;
    }
    let Ok(read) = std::fs::read_dir(current) else {
        return;
    };
    for entry in read.flatten() {
        if out.len() >= max {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // 不跟随符号链接:指向父目录的链接会把扫描拖进无限循环。
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if fs_ops::is_scan_skip_dir(&name) {
                continue;
            }
            scan(vault, &path, depth + 1, max, out);
            continue;
        }
        let Some(kind) = kind_of(&name) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(vault) else {
            continue;
        };
        let relative_path = relative
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        out.push(Attachment {
            path: path.to_string_lossy().into_owned(),
            name,
            relative_path,
            size: meta.len(),
            modified_ms: meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|delta| delta.as_millis() as i64)
                .unwrap_or(0),
            kind,
        });
    }
}

/// 读一个附件的原始字节。
///
/// 前端拿它做 blob URL 显示图片。为什么不开 Tauri 的 asset 协议:那要给
/// WebView 开一整棵目录的读权限并放宽 CSP,而这里每一次读都还是走
/// `resolve_in_vaults` 那道 allowlist —— 和笔记读写同一把锁。
pub fn read(resolved: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(resolved)
        .map_err(|e| format!("Cannot read {}: {e}", resolved.display()))?;
    if meta.is_dir() {
        return Err(format!("{} is a directory", resolved.display()));
    }
    if meta.len() as usize > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "The attachment is too large to open ({} bytes, limit {MAX_ATTACHMENT_BYTES})",
            meta.len()
        ));
    }
    std::fs::read(resolved).map_err(|e| format!("Cannot read {}: {e}", resolved.display()))
}
