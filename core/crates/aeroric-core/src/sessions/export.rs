//! 会话 Markdown 导出（P3-04 第五片）。
//!
//! 逐字迁移自宿主 `src-tauri/src/session/export.rs`，行为一行未改；唯一的形状
//! 变化是把宿主闸门留在宿主：`validate_session_path_for`（依赖 `AgentFamily` 与
//! 允许根，第四片已具名）不进 core，core 入口 [`export_session_markdown`] 收的是
//! **已过校验的 canonical 会话路径**；族别由 [`ExportSource`] 三值表达，宿主在
//! `session/export.rs` 里做一次 `AgentFamily → ExportSource` 映射（唯一真相关系
//! 同第二片）。输出路径校验 [`validate_export_output_path`] 不依赖任何宿主状态，
//! 连文案一起上收——周报 `crate::report` 经宿主 re-export 复用同一份，避免分叉。

use crate::sessions::claude::parse_claude_session;
use crate::sessions::codex::parse_codex_session;
use crate::sessions::dsh::{parse_dsh_session_lines, read_compressed_dsh_lines};
use crate::sessions::omp::parse_omp_session_lines;
use crate::sessions::parser::is_codex_format;
use crate::sessions::{SessionContent, SessionMessage};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

/// Maximum session size accepted by the explicit export flow (200 MiB).
const MAX_SESSION_BYTES_FOR_EXPORT: u64 = 200 * 1024 * 1024;

/// 导出时选的解析面。Claude／Codex 合一变体：宿主原分支 `_ if is_codex_format(...)`
/// 就是「这两族交给内容嗅探」，嗅探的唯一答案源在 [`crate::sessions::parser`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportSource {
    ClaudeOrCodex,
    Dsh,
    Omp,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTaskMeta {
    pub name: Option<String>,
    pub prompt: String,
    pub agent: String,
    pub created_at: i64,
    pub session_id: Option<String>,
    pub worktree_branch: Option<String>,
    pub base_branch: Option<String>,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub failure_reason: Option<String>,
}

/// `canonical` 必须是宿主 `validate_session_path_for` 通过后的路径（见模块头）。
pub fn export_session_markdown(
    canonical: &Path,
    output_path: &str,
    source: ExportSource,
    meta: &ExportTaskMeta,
) -> Result<(), String> {
    let canonical_out = validate_export_output_path(output_path)?;
    let metadata = std::fs::metadata(canonical)
        .map_err(|error| format!("Cannot read session metadata: {}", error))?;
    if metadata.len() > MAX_SESSION_BYTES_FOR_EXPORT {
        return Err(format!(
            "Session file is too large to export ({} MB > {} MB limit)",
            metadata.len() / 1024 / 1024,
            MAX_SESSION_BYTES_FOR_EXPORT / 1024 / 1024
        ));
    }

    // 压缩的 dsh transcript 按字节读只会得到二进制帧,必须先解码成逻辑行。
    let mut lines = match read_compressed_dsh_lines(canonical)? {
        Some(decoded) => decoded,
        None => {
            let session_file = File::open(canonical)
                .map_err(|error| format!("Cannot open session file: {}", error))?;
            BufReader::new(session_file)
                .lines()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("Cannot read session file: {}", error))?
        }
    };
    lines.retain(|line| !line.trim().is_empty());
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let messages = match source {
        ExportSource::Dsh => parse_dsh_session_lines(&line_refs)?,
        ExportSource::Omp => parse_omp_session_lines(&line_refs)?,
        ExportSource::ClaudeOrCodex if is_codex_format(&line_refs) => {
            parse_codex_session(&line_refs)
        }
        ExportSource::ClaudeOrCodex => parse_claude_session(&line_refs),
    };

    let out_file = File::create(&canonical_out)
        .map_err(|error| format!("Cannot create markdown file: {}", error))?;
    let mut writer = BufWriter::new(out_file);
    write_export_markdown(&mut writer, meta, &messages)
        .map_err(|error| format!("Cannot write markdown file: {}", error))?;
    writer
        .flush()
        .map_err(|error| format!("Cannot flush markdown file: {}", error))
}

/// Validate the renderer-provided export target without trusting the save dialog.
pub fn validate_export_output_path(output_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(output_path);
    if !path.is_absolute() {
        return Err("Output path must be absolute".into());
    }
    let has_md_ext = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("md"))
        .unwrap_or(false);
    if !has_md_ext {
        return Err("Output path must end with .md".into());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "Output path has no parent directory".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("Cannot resolve output directory: {}", error))?;
    if !canonical_parent.is_dir() {
        return Err("Output directory does not exist".into());
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| "Output path has no file name".to_string())?;
    Ok(canonical_parent.join(file_name))
}

pub fn write_export_markdown<W: Write>(
    out: &mut W,
    meta: &ExportTaskMeta,
    messages: &[SessionMessage],
) -> std::io::Result<()> {
    let title_raw = meta
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&meta.prompt);
    writeln!(out, "# {}\n", sanitize_md_inline(title_raw))?;

    writeln!(out, "## Metadata\n")?;
    writeln!(out, "- **Agent**: {}", sanitize_md_inline(&meta.agent))?;
    writeln!(
        out,
        "- **Created**: {}",
        format_timestamp_ms(meta.created_at)
    )?;
    if let Some(session_id) = &meta.session_id {
        if !session_id.is_empty() {
            writeln!(
                out,
                "- **Session ID**: `{}`",
                sanitize_md_code_span(session_id)
            )?;
        }
    }
    if let (Some(branch), Some(base)) = (&meta.worktree_branch, &meta.base_branch) {
        writeln!(
            out,
            "- **Branch**: `{}` → `{}`",
            sanitize_md_code_span(branch),
            sanitize_md_code_span(base)
        )?;
    }
    if let (Some(additions), Some(deletions)) = (meta.additions, meta.deletions) {
        writeln!(out, "- **Diff**: +{} / −{}", additions, deletions)?;
    }
    if let Some(reason) = &meta.failure_reason {
        if !reason.is_empty() {
            writeln!(out, "- **Failure reason**: {}", sanitize_md_inline(reason))?;
        }
    }
    writeln!(out)?;

    writeln!(out, "## Prompt\n")?;
    if meta.prompt.trim().is_empty() {
        writeln!(out, "> _(empty)_")?;
    } else {
        for line in meta.prompt.lines() {
            writeln!(out, "> {}", line)?;
        }
    }
    writeln!(out)?;

    writeln!(out, "## Conversation\n")?;
    let mut current_role: Option<&str> = None;
    for message in messages {
        let texts: Vec<&str> = message
            .content
            .iter()
            .filter_map(|content| match content {
                SessionContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if texts.is_empty() {
            continue;
        }
        if current_role != Some(message.role.as_str()) {
            current_role = Some(message.role.as_str());
            match message.role.as_str() {
                "user" => writeln!(out, "### User\n")?,
                "assistant" => writeln!(out, "### Assistant\n")?,
                other => {
                    writeln!(out, "### {}\n", sanitize_md_inline(other))?;
                    continue;
                }
            }
        }
        for text in texts {
            out.write_all(text.as_bytes())?;
            if !text.ends_with('\n') {
                out.write_all(b"\n")?;
            }
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

/// 折叠一切空白与控制字符成单个空格。凡要塞进 md 单行结构(标题、bullet)的文本
/// 都必须先过这里:一个裸换行就能把 bullet 结构撑破。
pub fn sanitize_md_inline(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for character in value.trim().chars() {
        if character.is_whitespace() || character.is_control() {
            if !previous_was_space {
                result.push(' ');
                previous_was_space = true;
            }
        } else {
            result.push(character);
            previous_was_space = false;
        }
    }
    result
}

fn sanitize_md_code_span(value: &str) -> String {
    sanitize_md_inline(value).replace('`', "'")
}

pub fn format_timestamp_ms(milliseconds: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_millis_opt(milliseconds)
        .single()
        .map(|timestamp| timestamp.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| milliseconds.to_string())
}
