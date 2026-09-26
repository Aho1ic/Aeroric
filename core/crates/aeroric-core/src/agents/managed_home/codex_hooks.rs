//! Codex hooks 的受管 TOML 块构造(P3-02 切片四自宿主 hooks.rs 迁入的纯半边):
//! `~/.codex/config.toml` 注入/清理的文本变换。I/O 留在宿主。

pub const CODEX_BEGIN: &str = "# >>> aeroric-managed-begin (do not edit; managed by Aeroric) >>>";
pub const CODEX_END: &str = "# <<< aeroric-managed-end <<<";

pub const CODEX_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PermissionRequest",
    // 见 CLAUDE_EVENTS 的 PostToolUse 说明;Codex 自 0.124 起同样支持。
    "PostToolUse",
    "Stop",
    "SubagentStop",
];

use super::claude_hooks::hook_command;

pub fn build_codex_block(node_path: &str, script: &str) -> String {
    let mut out = String::new();
    out.push_str(CODEX_BEGIN);
    out.push('\n');
    for event in CODEX_EVENTS {
        out.push_str(&format!("[[hooks.{}]]\n", event));
        out.push_str(&format!("[[hooks.{}.hooks]]\n", event));
        out.push_str("type = \"command\"\n");
        // Codex 的 `command` 只能是字符串(无 args 数组),在 Windows 上经
        // toml_quote 负责把命令中的引号与路径反斜杠转义成合法 TOML。
        out.push_str(&format!(
            "command = {}\n",
            toml_quote(&hook_command(node_path, script))
        ));
        out.push('\n');
    }
    out.push_str(CODEX_END);
    out.push('\n');
    out
}

/// 安全地把字符串转成 TOML basic string 字面量。
pub fn toml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 自定义 codex-like Agent 的 hook 片段文件。
///
/// 这些 Agent 各有隔离的 `CODEX_HOME`(`agent-homes/{id}`),其 `config.toml`
/// 由 Aeroric 生成的包装脚本在**每次启动时整体重写**,因此安装器写进
/// `~/.codex/config.toml` 的 hook 块到不了它们。包装脚本改为在写完 config 后
/// `cat` 追加此片段——内容由 Aeroric 单点维护,node 路径变化无需重刷脚本。
pub fn inject_codex_text(existing: &str, node_path: &str, script: &str) -> String {
    let block = build_codex_block(node_path, script);
    if let (Some(begin), Some(end)) = (existing.find(CODEX_BEGIN), existing.find(CODEX_END)) {
        if begin < end {
            let end_line_end = existing[end..]
                .find('\n')
                .map(|n| end + n + 1)
                .unwrap_or(existing.len());
            // 计算 begin 之前需要保留的部分(剔除紧邻的换行让结果整洁)
            let before = &existing[..begin];
            let after = &existing[end_line_end..];
            let mut out = String::with_capacity(before.len() + block.len() + after.len());
            out.push_str(before);
            if !before.is_empty() && !before.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&block);
            if !after.is_empty() && !after.starts_with('\n') {
                out.push('\n');
            }
            out.push_str(after);
            return out;
        }
    }

    // 没有 marker,追加在文件末尾
    let mut out = String::with_capacity(existing.len() + block.len() + 2);
    out.push_str(existing);
    if !existing.is_empty() && !existing.ends_with('\n') {
        out.push('\n');
    }
    if !existing.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    out
}
/// 从 TOML 内容里移除 Aeroric 块。
pub fn uninject_codex_text(existing: &str) -> String {
    let (Some(begin), Some(end)) = (existing.find(CODEX_BEGIN), existing.find(CODEX_END)) else {
        return existing.to_string();
    };
    if begin >= end {
        return existing.to_string();
    }
    let end_line_end = existing[end..]
        .find('\n')
        .map(|n| end + n + 1)
        .unwrap_or(existing.len());
    let before = &existing[..begin];
    let after = &existing[end_line_end..];
    let mut out = String::with_capacity(before.len() + after.len());
    out.push_str(before);
    // 跳过 before 末尾若有多余空行,保持文件整洁
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !after.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(after.trim_start_matches('\n'));
        if !out.ends_with('\n') {
            out.push('\n');
        }
    } else if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}
