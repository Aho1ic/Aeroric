//! localStorage → 磁盘的一次性迁移。
//!
//! 这是整个随手记文件化里唯一可能让用户丢数据的地方,所以顺序是刻意的:
//!
//! 1. 先把原始 JSON 备份到 vault 私有目录 —— **备份失败就不开始迁移**
//! 2. 全部笔记先在内存里转换完、校验完,再落盘
//! 3. 任何一条失败 → 删掉本次已写的文件,报错退出,localStorage 保持原样
//! 4. 前端只在收到成功结果后才把 localStorage 的键改名(不删,留一个版本的
//!    回退余地)
//!
//! 幂等靠 frontmatter 里的 `legacyId`:已经迁过的笔记再迁一次会被识别成
//! "同一条",跳过而不是产生 `title-2.md`。
//!
//! **迁移无损**:正文一个字节都不改。富文本笔记的 HTML 原样保留,只在
//! frontmatter 标 `editor: richtext`,面板照旧用富文本编辑器打开。HTML →
//! Markdown 的转换推到 P1 —— 那时 WYSIWYG 编辑器已经能接管富文本体验,
//! 用户不会经历"格式丢了、又没有替代编辑器"的中间状态。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::fs_ops::{atomic_write, private_dir, VAULT_PRIVATE_DIR};

/// 前端从 localStorage 原样读出来的一条笔记。字段全部可选:这份数据在
/// localStorage 里躺了多个版本,不能假设结构完整。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyNote {
    pub id: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub format: Option<String>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigratedNote {
    pub legacy_id: String,
    pub path: String,
    pub title: String,
    /// 原始格式(`markdown` / `richtext` / `txt`),用于前端展示迁移报告。
    pub source_format: String,
    /// 该条是否用富文本编辑器打开(即 frontmatter 里标了 `editor: richtext`)。
    /// 迁移本身不做任何格式转换。
    pub richtext: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub vault: String,
    pub backup_path: String,
    pub migrated: Vec<MigratedNote>,
    /// 因为 `legacyId` 已存在而跳过的条目(重复运行时走这里)。
    pub skipped: Vec<String>,
    pub total_input: usize,
}

/// 文件名 slug。目标是"能落盘、可读、跨三个平台都合法",不追求还原标题 ——
/// 真实标题另存在 frontmatter 的 `title` 里,UI 显示的是那个。
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;

    for ch in title.chars() {
        let mapped = match ch {
            // Windows 禁用字符 + 路径分隔符,全部压成连字符。
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            // 控制字符同样不能进文件名。
            c if (c as u32) < 0x20 => '-',
            c if c.is_whitespace() => '-',
            c => c,
        };
        if mapped == '-' {
            // 连续的分隔符折叠成一个,避免 `a???b` 变成 `a---b`。
            if !last_was_dash && !out.is_empty() {
                out.push('-');
                last_was_dash = true;
            }
            continue;
        }
        out.push(mapped);
        last_was_dash = false;
    }

    // Windows 会静默吃掉结尾的空格和点,于是磁盘上的名字和我们记录的不一致。
    let trimmed = out.trim_matches(|c: char| c == '-' || c == '.' || c.is_whitespace());
    let mut slug = trimmed.to_string();

    // 255 字节是三个平台的共同上限。按字符边界截断,不能切裂 UTF-8。
    // 留 16 字节给 `-NN.md` 后缀和潜在的去重编号。
    const MAX_STEM_BYTES: usize = 200;
    if slug.len() > MAX_STEM_BYTES {
        let mut end = MAX_STEM_BYTES;
        while end > 0 && !slug.is_char_boundary(end) {
            end -= 1;
        }
        slug.truncate(end);
        slug = slug.trim_end_matches(['-', '.']).to_string();
    }

    if slug.is_empty() {
        return "untitled".to_string();
    }

    // Windows 的 DOS 设备名即使带扩展名也不能用(`CON.md` 一样打不开)。
    // 三个平台统一加前缀,免得同一个 vault 在 Windows 上打不开。
    let stem_upper = slug
        .split_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(&slug)
        .to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
        "LPT9",
    ];
    if RESERVED.contains(&stem_upper.as_str()) {
        return format!("note-{slug}");
    }
    slug
}

/// YAML 标量转义。标题里可能有引号、冒号、`#`,直接拼进 frontmatter 会生成
/// 解析不了的 YAML。统一用双引号包裹并转义。
fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    // frontmatter 是单行 key: value,换行会截断这一项。
    let single_line = escaped.replace('\n', " ").replace('\r', "");
    format!("\"{single_line}\"")
}

fn ms_to_iso8601(ms: i64) -> String {
    // 只用来写 frontmatter 的可读时间戳。chrono 已是依赖。
    use chrono::TimeZone;
    match chrono::Utc.timestamp_millis_opt(ms).single() {
        Some(time) => time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        None => String::new(),
    }
}

/// 组装一条笔记的最终文件内容。
///
/// **迁移是无损的:正文一个字节都不动。** 富文本笔记的正文保持 HTML,只在
/// frontmatter 里标 `editor: richtext` —— 面板据此仍然用富文本编辑器打开它。
///
/// 为什么不在这里做 HTML → Markdown:那个转换有损(内联 style、颜色、嵌套
/// 表格会丢)。等 P1 的 WYSIWYG 编辑器到位、能真正接管富文本编辑体验之后再转,
/// 用户才不会在中间那个版本里既丢了格式又没有替代编辑器。
fn build_note_file(note: &LegacyNote, legacy_id: &str, title: &str, is_richtext: bool) -> String {
    let body = note.body.clone().unwrap_or_default();

    let updated = note.updated_at.unwrap_or(0);
    let mut front = String::from("---\n");
    front.push_str(&format!("title: {}\n", yaml_quote(title)));
    if updated > 0 {
        let iso = ms_to_iso8601(updated);
        if !iso.is_empty() {
            front.push_str(&format!("updated: {iso}\n"));
        }
    }
    front.push_str(&format!("legacyId: {}\n", yaml_quote(legacy_id)));
    // `editor` 是面板读的字段:决定用富文本还是 Markdown 编辑器打开。
    // 缺省(不写)即 Markdown。
    if is_richtext {
        front.push_str("editor: richtext\n");
    }
    front.push_str("---\n\n");

    let mut out = front;
    out.push_str(body.trim_end());
    out.push('\n');
    out
}

/// 收集 vault 里已存在的 `legacyId`,用于幂等判断。
fn existing_legacy_ids(vault: &Path) -> HashSet<String> {
    let mut found = HashSet::new();
    let Ok(entries) = std::fs::read_dir(vault) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !super::fs_ops::is_note_file(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // 只看文件开头的 frontmatter,不做完整 YAML 解析。
        if let Some(id) = read_legacy_id(&text) {
            found.insert(id);
        }
    }
    found
}

/// 从 frontmatter 里抠出 `legacyId`。故意只做最小解析 —— 这里不需要一个
/// YAML 解析器。
fn read_legacy_id(source: &str) -> Option<String> {
    let rest = source.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        let Some(value) = line.strip_prefix("legacyId:") else {
            continue;
        };
        let trimmed = value.trim();
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(trimmed);
        // 还原 yaml_quote 的转义。
        return Some(unquoted.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    None
}

/// 执行迁移。
///
/// `vault` 必须已经存在(调用方负责 `create_dir_all`)。`raw_json` 是
/// localStorage 里的原始字符串 —— 传原始字符串而不是解析后的结构,是为了
/// 备份能存下真正的原文,包括我们可能识别不了的字段。
pub fn migrate_legacy_notes(vault: &Path, raw_json: &str) -> Result<MigrationReport, String> {
    if !vault.is_dir() {
        return Err(format!(
            "Notebook vault does not exist: {}",
            vault.display()
        ));
    }

    let notes: Vec<LegacyNote> = serde_json::from_str(raw_json)
        .map_err(|e| format!("Legacy quick notes are not valid JSON: {e}"))?;

    // ── 第 1 步:备份。失败就不往下走。 ──────────────────────────────────
    let private = private_dir(vault);
    std::fs::create_dir_all(&private)
        .map_err(|e| format!("Cannot create {}: {e}", private.display()))?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup_path = private.join(format!("legacy-backup-{stamp}.json"));
    atomic_write(&backup_path, raw_json)
        .map_err(|e| format!("Refusing to migrate: backup failed ({e})"))?;

    let already = existing_legacy_ids(vault);
    let mut report = MigrationReport {
        vault: vault.to_string_lossy().to_string(),
        backup_path: backup_path.to_string_lossy().to_string(),
        migrated: Vec::new(),
        skipped: Vec::new(),
        total_input: notes.len(),
    };

    // ── 第 2 步:全部在内存里转换 + 定名,先不落盘。 ──────────────────────
    // 这样命名冲突和转换失败都在写任何文件之前暴露出来。
    let mut planned: Vec<(PathBuf, String, MigratedNote)> = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();

    // vault 里已有的文件名也要占位,否则迁移会覆盖用户手写的笔记。
    if let Ok(entries) = std::fs::read_dir(vault) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                taken.insert(name.to_ascii_lowercase());
            }
        }
    }

    for (index, note) in notes.iter().enumerate() {
        // 没有 id 的条目自己造一个稳定 id:用序号。同一份 JSON 重跑会得到
        // 同样的 id,幂等因此仍然成立。
        let legacy_id = note
            .id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("legacy-index-{index}"));

        if already.contains(&legacy_id) {
            report.skipped.push(legacy_id);
            continue;
        }

        let title = note
            .title
            .clone()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Untitled quick note".to_string());

        // 老数据里 `txt` 是 `richtext` 的前身(见前端 normalizeFormat)。
        let source_format = note
            .format
            .clone()
            .unwrap_or_else(|| "markdown".to_string());
        let is_richtext = source_format == "richtext" || source_format == "txt";

        let stem = slugify(&title);
        let mut file_name = format!("{stem}.md");
        let mut suffix = 2;
        while taken.contains(&file_name.to_ascii_lowercase()) {
            file_name = format!("{stem}-{suffix}.md");
            suffix += 1;
        }
        taken.insert(file_name.to_ascii_lowercase());

        let content = build_note_file(note, &legacy_id, &title, is_richtext);
        planned.push((
            vault.join(&file_name),
            content,
            MigratedNote {
                legacy_id,
                path: vault.join(&file_name).to_string_lossy().to_string(),
                title,
                source_format,
                richtext: is_richtext,
            },
        ));
    }

    // ── 第 3 步:落盘。任一失败 → 回滚本次已写的文件。 ────────────────────
    let mut written: Vec<PathBuf> = Vec::new();
    for (path, content, meta) in planned {
        match atomic_write(&path, &content) {
            Ok(()) => {
                written.push(path);
                report.migrated.push(meta);
            }
            Err(error) => {
                for done in &written {
                    let _ = std::fs::remove_file(done);
                }
                return Err(format!(
                    "Migration rolled back after {} note(s): {error}. Backup kept at {}",
                    written.len(),
                    backup_path.display()
                ));
            }
        }
    }

    Ok(report)
}

/// 全局默认 vault:`~/.aeroric/notebook`。localStorage 迁移的落点。
///
/// 为什么是全局而不是项目级:今天随手记的数据在 localStorage 里,是跨项目
/// 共享的。迁移后必须还是跨项目共享,否则用户切个项目就以为笔记丢了。
pub fn default_vault_path() -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot resolve home directory".to_string())?;
    Ok(home.join(".aeroric").join("notebook"))
}

/// 确保默认 vault 存在,顺带把私有目录建出来并放好 `.gitignore`。
pub fn ensure_default_vault() -> Result<PathBuf, String> {
    let vault = default_vault_path()?;
    std::fs::create_dir_all(&vault)
        .map_err(|e| format!("Cannot create {}: {e}", vault.display()))?;
    let private = private_dir(&vault);
    std::fs::create_dir_all(&private)
        .map_err(|e| format!("Cannot create {}: {e}", private.display()))?;

    // 用户很可能把这个 vault 变成 Git 仓库。历史快照、索引、备份都不该入库。
    let ignore = private.join(".gitignore");
    if !ignore.exists() {
        let _ = atomic_write(&ignore, "# Managed by Aeroric. Notebook-private data.\n*\n");
    }
    Ok(vault)
}

/// 为一个标题在 vault 里分配还没被占用的文件名,返回绝对路径。
///
/// 命名放在后端而不是前端:slug 规则里有 Windows 保留设备名、UTF-8 边界截断
/// 这类平台细节,两份实现迟早会漂。前端只管传标题。
pub fn allocate_note_path(vault: &Path, title: &str) -> Result<PathBuf, String> {
    let mut taken: HashSet<String> = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(vault) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                taken.insert(name.to_ascii_lowercase());
            }
        }
    }

    let stem = slugify(title);
    let mut file_name = format!("{stem}.md");
    let mut suffix = 2;
    while taken.contains(&file_name.to_ascii_lowercase()) {
        file_name = format!("{stem}-{suffix}.md");
        suffix += 1;
        // 同一个 stem 撞到上千次说明有别的问题(比如 vault 被当成了下载目录),
        // 与其无限循环,不如报错。
        if suffix > 1000 {
            return Err(format!("Too many notes named like {stem:?}"));
        }
    }
    Ok(vault.join(file_name))
}

/// 一条富文本笔记的转换结果。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertedNote {
    pub path: String,
    pub title: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichtextConversionReport {
    pub vault: String,
    /// 转换前整个 vault 的备份目录。出问题时用户能整体捞回来。
    pub backup_dir: String,
    pub converted: Vec<ConvertedNote>,
    /// 扫到但不需要转的笔记数(没有 `editor: richtext` 标记)。
    pub skipped: usize,
}

/// 把 vault 里所有 `editor: richtext` 的笔记转成 Markdown。
///
/// 这是 P1 的收尾迁移:P0 为了无损,把富文本的 HTML 原样落进了 `.md`,并在
/// frontmatter 标 `editor: richtext`。WYSIWYG 到位之后,这些笔记该变成真正的
/// Markdown —— 否则它们参与不了双链、RAG 分块、导出。
///
/// 安全等级与 P0 迁移一致:
/// 1. 先把每个待转文件备份到 `.notebook/richtext-backup-<ts>/`,**备份失败就不转**
/// 2. 全部在内存里转换完再落盘
/// 3. 任一失败 → 回滚本次已写的文件,报错退出
///
/// 幂等:转完的笔记没有 `editor: richtext` 了,重跑会被跳过。
pub fn convert_richtext_notes(vault: &Path) -> Result<RichtextConversionReport, String> {
    if !vault.is_dir() {
        return Err(format!(
            "Notebook vault does not exist: {}",
            vault.display()
        ));
    }

    // ── 收集待转文件 ──────────────────────────────────────────────────────
    let mut pending: Vec<(PathBuf, String)> = Vec::new();
    let mut skipped = 0usize;
    let entries = std::fs::read_dir(vault).map_err(|e| format!("Cannot read vault: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !super::fs_ops::is_note_file(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if read_frontmatter_field(&text, "editor").as_deref() == Some("richtext") {
            pending.push((path, text));
        } else {
            skipped += 1;
        }
    }

    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup_dir = private_dir(vault).join(format!("richtext-backup-{stamp}"));

    let mut report = RichtextConversionReport {
        vault: vault.to_string_lossy().to_string(),
        backup_dir: backup_dir.to_string_lossy().to_string(),
        converted: Vec::new(),
        skipped,
    };
    if pending.is_empty() {
        return Ok(report);
    }

    // ── 第 1 步:备份。失败就不转。 ────────────────────────────────────────
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("Cannot create {}: {e}", backup_dir.display()))?;
    for (path, text) in &pending {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "Invalid note file name".to_string())?;
        atomic_write(&backup_dir.join(name), text)
            .map_err(|e| format!("Refusing to convert: backup failed ({e})"))?;
    }

    // ── 第 2 步:内存内转换 ───────────────────────────────────────────────
    let mut planned: Vec<(PathBuf, String, ConvertedNote)> = Vec::new();
    for (path, text) in &pending {
        let (front, body) = split_frontmatter(text);
        let markdown = super::html2md::html_to_markdown(body, false);
        // 去掉 `editor: richtext` 那一行,其余 frontmatter 原样保留。
        let next_front: Vec<&str> = front
            .lines()
            .filter(|line| !line.trim_start().starts_with("editor:"))
            .collect();
        let mut out = String::new();
        if !next_front.is_empty() {
            out.push_str("---\n");
            for line in &next_front {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("---\n\n");
        }
        out.push_str(markdown.trim_end());
        out.push('\n');

        let title = read_frontmatter_field(text, "title").unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });
        planned.push((
            path.clone(),
            out,
            ConvertedNote {
                path: path.to_string_lossy().to_string(),
                title,
            },
        ));
    }

    // ── 第 3 步:落盘,失败回滚 ───────────────────────────────────────────
    let mut written: Vec<(PathBuf, String)> = Vec::new();
    for (path, content, meta) in planned {
        // 记下原文用于回滚 —— 这里是覆盖写,不是新建,删掉文件等于毁掉笔记。
        let original = pending
            .iter()
            .find(|(candidate, _)| candidate == &path)
            .map(|(_, text)| text.clone())
            .unwrap_or_default();
        match atomic_write(&path, &content) {
            Ok(()) => {
                written.push((path, original));
                report.converted.push(meta);
            }
            Err(error) => {
                for (done, text) in &written {
                    let _ = atomic_write(done, text);
                }
                return Err(format!(
                    "Conversion rolled back after {} note(s): {error}. Backup kept at {}",
                    written.len(),
                    backup_dir.display()
                ));
            }
        }
    }

    Ok(report)
}

/// 读 frontmatter 里某个字段的值(去引号)。没有 frontmatter 或没这个字段返回 None。
fn read_frontmatter_field(source: &str, key: &str) -> Option<String> {
    let rest = source.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        let Some(value) = line.strip_prefix(&format!("{key}:")) else {
            continue;
        };
        let trimmed = value.trim();
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(trimmed);
        return Some(unquoted.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    None
}

/// 把文件拆成 (frontmatter 内容, 正文)。没有 frontmatter 时前者为空。
fn split_frontmatter(source: &str) -> (&str, &str) {
    let Some(rest) = source.strip_prefix("---\n") else {
        return ("", source);
    };
    let Some(end) = rest.find("\n---") else {
        return ("", source);
    };
    let front = &rest[..end];
    // 跳过闭合的 `---` 及其后的换行
    let after = &rest[end + 4..];
    (front, after.trim_start_matches('\n'))
}

/// 项目级 vault:`<project>/.aeroric/notes`。默认不开启,用户在设置里启用。
pub fn project_vault_path(project_path: &str) -> PathBuf {
    Path::new(project_path).join(".aeroric").join("notes")
}

/// 项目级 vault 的私有目录必须进项目的 `.gitignore`,否则索引和快照会被
/// 提交进用户的项目仓库。
pub fn ensure_project_vault(project_path: &str) -> Result<PathBuf, String> {
    let vault = project_vault_path(project_path);
    std::fs::create_dir_all(&vault)
        .map_err(|e| format!("Cannot create {}: {e}", vault.display()))?;
    let private = private_dir(&vault);
    std::fs::create_dir_all(&private)
        .map_err(|e| format!("Cannot create {}: {e}", private.display()))?;
    let ignore = private.join(".gitignore");
    if !ignore.exists() {
        let _ = atomic_write(
            &ignore,
            &format!(
                "# Managed by Aeroric. {VAULT_PRIVATE_DIR} holds local-only notebook data.\n*\n"
            ),
        );
    }
    Ok(vault)
}
