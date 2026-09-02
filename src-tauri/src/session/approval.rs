//! 判断一次工具调用要不要先问用户。
//!
//! 从 `session.rs` 整块搬出来,内容一行没改(连原来那行分节注释一起)。
//! 全是纯函数:只看工具名 + 参数 JSON + 项目根路径,不读盘、不发事件。
//!
//! 这是**安全相关**的一段:判错成「不用问」就等于让 agent 未经确认改了文件。
//! 两条主线是 `exec_command_requires_confirmation`(命令是不是只读 —— 要挡住
//! shell 重定向,否则 `cat a > b` 会被当成只读)和
//! `apply_patch_requires_confirmation`(补丁目标是否落在项目根内)。

use super::*;

// ── 权限判断 ──────────────────────────────────────────────────────────────────

pub(super) fn tool_call_requires_confirmation(
    name: &str,
    arguments: &str,
    project_path: &Path,
) -> bool {
    match name {
        "exec_command" => exec_command_requires_confirmation(arguments),
        "apply_patch" => apply_patch_requires_confirmation(arguments, project_path),
        _ => false,
    }
}

pub(super) fn exec_command_requires_confirmation(arguments: &str) -> bool {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return false;
    };

    if args
        .get("sandbox_permissions")
        .and_then(serde_json::Value::as_str)
        == Some("require_escalated")
    {
        return true;
    }

    let Some(cmd) = args.get("cmd").and_then(serde_json::Value::as_str) else {
        return false;
    };

    !looks_like_read_only_command(cmd)
}

pub(super) fn looks_like_read_only_command(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    if trimmed.is_empty() || contains_shell_redirection(trimmed) {
        return false;
    }

    trimmed
        .split([';', '|', '&', '\n'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .all(is_read_only_segment)
}

pub(super) fn contains_shell_redirection(cmd: &str) -> bool {
    cmd.contains(" >")
        || cmd.contains(">>")
        || cmd.contains("<<")
        || cmd.contains(" 2>")
        || cmd.starts_with('>')
        || cmd.contains("| tee")
}

pub(super) fn is_read_only_segment(segment: &str) -> bool {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let Some(command) = tokens.first().copied() else {
        return true;
    };

    match command {
        "pwd" | "ls" | "rg" | "grep" | "cat" | "head" | "tail" | "wc" | "stat" | "which"
        | "type" | "uname" | "date" | "ps" | "env" | "printenv" | "echo" | "printf"
        | "Get-Location" | "Get-ChildItem" | "Get-Content" | "Select-String" | "Get-Process"
        | "Get-Date" | "Get-Command" | "Test-Path" | "Resolve-Path" | "Where-Object"
        | "Measure-Object" | "Sort-Object" | "Select-Object" => true,
        "sed" => tokens.contains(&"-n") && !tokens.iter().any(|token| token.starts_with("-i")),
        "find" => !tokens
            .iter()
            .any(|token| matches!(*token, "-delete" | "-exec" | "-ok")),
        "git.exe" => matches!(
            tokens.get(1).copied(),
            Some("status")
                | Some("diff")
                | Some("show")
                | Some("log")
                | Some("branch")
                | Some("rev-parse")
                | Some("remote")
        ),
        "git" => matches!(
            tokens.get(1).copied(),
            Some("status")
                | Some("diff")
                | Some("show")
                | Some("log")
                | Some("branch")
                | Some("rev-parse")
                | Some("remote")
        ),
        _ => false,
    }
}

pub(super) fn apply_patch_requires_confirmation(arguments: &str, project_path: &Path) -> bool {
    arguments.lines().any(|line| {
        extract_patch_path(line)
            .map(|path| patch_target_requires_confirmation(path, project_path))
            .unwrap_or(false)
    })
}

pub(super) fn extract_patch_path(line: &str) -> Option<&str> {
    line.strip_prefix("*** Add File: ")
        .or_else(|| line.strip_prefix("*** Update File: "))
        .or_else(|| line.strip_prefix("*** Delete File: "))
        .or_else(|| line.strip_prefix("*** Move to: "))
        .map(str::trim)
}

pub(super) fn patch_target_requires_confirmation(path: &str, project_path: &Path) -> bool {
    let target = Path::new(path);
    if !target.is_absolute() {
        return false;
    }

    let temp_dir = std::env::temp_dir();
    !target.starts_with(project_path) && !target.starts_with(&temp_dir)
}

pub(super) fn assistant_message_requests_user_input(payload: Option<&serde_json::Value>) -> bool {
    let Some(payload) = payload else {
        return false;
    };

    let phase = payload.get("phase").and_then(serde_json::Value::as_str);
    if !matches!(phase, Some("final") | Some("final_answer")) {
        return false;
    }

    let Some(content) = payload.get("content").and_then(serde_json::Value::as_array) else {
        return false;
    };

    let text = content
        .iter()
        .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
        .collect::<String>();
    let text = text.trim();

    text.ends_with('?') || text.ends_with('？')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_command_detection_is_conservative() {
        assert!(looks_like_read_only_command("pwd && rg -n session src"));
        assert!(looks_like_read_only_command(
            "sed -n '1,120p' src-tauri/src/lib.rs"
        ));
        assert!(!looks_like_read_only_command(
            "cargo test --manifest-path src-tauri/Cargo.toml"
        ));
        assert!(!looks_like_read_only_command("echo hello > out.txt"));
    }

    #[test]
    fn powershell_read_only_commands_are_treated_as_safe() {
        assert!(looks_like_read_only_command(
            "Get-ChildItem -Force | Select-String -Pattern session"
        ));
        assert!(looks_like_read_only_command(
            "Get-Content README.md | Select-Object -First 20"
        ));
        assert!(looks_like_read_only_command("git.exe status --short"));
    }

    #[test]
    fn exec_command_confirmation_detection_matches_escalation_and_write_commands() {
        assert!(exec_command_requires_confirmation(
            r#"{"cmd":"rg -n session src","sandbox_permissions":"require_escalated"}"#
        ));
        assert!(exec_command_requires_confirmation(
            r#"{"cmd":"cargo test --manifest-path src-tauri/Cargo.toml --lib"}"#
        ));
        assert!(!exec_command_requires_confirmation(
            r#"{"cmd":"git status --short"}"#
        ));
    }

    #[test]
    fn apply_patch_confirmation_detection_only_flags_external_absolute_paths() {
        let project_root = Path::new("/repo");

        assert!(!apply_patch_requires_confirmation(
            "*** Begin Patch\n*** Update File: src/main.rs\n*** End Patch",
            project_root,
        ));
        assert!(!apply_patch_requires_confirmation(
            "*** Begin Patch\n*** Update File: /repo/src/main.rs\n*** End Patch",
            project_root,
        ));
        assert!(apply_patch_requires_confirmation(
            "*** Begin Patch\n*** Update File: /var/aeroric-outside.rs\n*** End Patch",
            project_root,
        ));
    }

    #[test]
    fn final_assistant_question_is_treated_as_input_required() {
        let payload = serde_json::json!({
            "role": "assistant",
            "phase": "final_answer",
            "content": [
                { "type": "output_text", "text": "继续按这个方案改吗？" }
            ]
        });

        assert!(assistant_message_requests_user_input(Some(&payload)));
    }
}
