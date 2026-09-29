//! dsh 族的路径层（P3-04 第二片的 dsh 半边）。
//!
//! 逐字迁移自宿主 `src-tauri/src/session_dsh.rs` 的常量、`dsh_project_key`、
//! `dsh_encode_segment`、`dsh_transcript_in`。目录布局与"两形态并存"的原因
//! 保留原文；读侧解压（`read_compressed_dsh_lines`）与转录解析属 dsh 片，
//! **core 今天不新增 zstd 依赖**——手写约束 8 要求先核对宿主用的是哪个 crate
//! 再决定收编，见补记二百四十二。

use std::path::{Path, PathBuf};

/// 支持的 dsh 会话日志格式版本(`SESSION_FORMAT_VERSION`,dev preview 期为 0)。
pub const DSH_SESSION_FORMAT_VERSION: u64 = 0;

/// 明文 transcript 文件名(`compression: none`)。
pub const DSH_TRANSCRIPT_RAW: &str = "session.jsonl";
/// 压缩 transcript 文件名(插件默认 `compression: zstd`)。
pub const DSH_TRANSCRIPT_ZSTD: &str = "session.jsonl.zstd";

/// dsh `projectKey` 的移植:分隔符(`/` `\` `:`)折叠为 `-`,安全字符
/// (ASCII 字母数字与 `._-`,不含 `~`)保留,其余按 UTF-16 code unit 转义为
/// `~XXXX`;去掉前导 `-`,空则用 `root`,截断到 251,再包上 `--`。
pub fn dsh_project_key(cwd: &str) -> String {
    let mut readable = String::new();
    let mut separator_run = false;
    for code in cwd.encode_utf16() {
        match char::from_u32(u32::from(code)) {
            Some('/' | '\\' | ':') => {
                if !separator_run {
                    readable.push('-');
                }
                separator_run = true;
            }
            Some(ch)
                if ch != '~' && (ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) =>
            {
                readable.push(ch);
                separator_run = false;
            }
            _ => {
                readable.push('~');
                readable.push_str(&format!("{code:04X}"));
                separator_run = false;
            }
        }
    }
    let slug = readable.trim_start_matches('-');
    let slug = if slug.is_empty() { "root" } else { slug };
    // readable 只含 ASCII(非安全字符已转义),按字节截断即等价于 JS 的 slice(0, 251)。
    let truncated = &slug[..slug.len().min(251)];
    format!("--{truncated}--")
}

/// dsh `encodeSegment` 的移植:会话 id → 安全目录名(`.`/`..` 特判,
/// 安全字符保留,其余 `~XXXX`)。
pub fn dsh_encode_segment(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if raw == "." {
        return "~002E".to_string();
    }
    if raw == ".." {
        return "~002E~002E".to_string();
    }
    let mut out = String::new();
    for code in raw.encode_utf16() {
        match char::from_u32(u32::from(code)) {
            Some(ch)
                if ch != '~' && (ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) =>
            {
                out.push(ch);
            }
            _ => {
                out.push('~');
                out.push_str(&format!("{code:04X}"));
            }
        }
    }
    out
}

/// 会话目录里实际存在的 transcript。
///
/// 一个 root 只属于一种编码(插件在发现阶段就会拒绝相反的后缀),但 Aeroric
/// 把 `compression` 从 zstd 切到 none 之后,同一 root 里旧的压缩产物仍然留在
/// 磁盘上,所以这里按"存在即唯一"逐会话判定:明文优先,其次压缩,都不在返回
/// `None`。
pub fn dsh_transcript_in(session_dir: &Path) -> Option<PathBuf> {
    let raw = session_dir.join(DSH_TRANSCRIPT_RAW);
    if raw.is_file() {
        return Some(raw);
    }
    let compressed = session_dir.join(DSH_TRANSCRIPT_ZSTD);
    compressed.is_file().then_some(compressed)
}
