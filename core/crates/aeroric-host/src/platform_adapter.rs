//! 宿主侧的 [`PlatformServices`] 实现：OS 调用走子进程，**没有窗口依赖**。
//! macOS 的剪贴板读取经 `osascript`（AppKit/NSPasteboard），与 Tauri 适配层
//! 同一脚本逐字；非 macOS 平台按端口契约返回**空列表**（"平台没有对应能力"
//! 的中性答案），不是错误。`reveal_in_file_manager` 本片无 wire 方法消费，
//! 先按 macOS `open -R` 实现放在端口后面，等它的第一个调用者。

use std::path::Path;
use std::process::Command;

use aeroric_core::runtime::{PlatformError, PlatformServices};

pub struct HostOsServices;

impl PlatformServices for HostOsServices {
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), PlatformError> {
        reveal_sync(path)
    }

    fn read_clipboard_file_paths(&self) -> Result<Vec<String>, PlatformError> {
        read_clipboard_file_paths_sync().map_err(PlatformError::Io)
    }
}

#[cfg(target_os = "macos")]
fn reveal_sync(path: &Path) -> Result<(), PlatformError> {
    let mut cmd = Command::new("open");
    if path.is_dir() {
        cmd.arg(path);
    } else {
        cmd.arg("-R").arg(path);
    }
    cmd.stdin(std::process::Stdio::null());
    let output = cmd
        .output()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(PlatformError::Io(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn reveal_sync(_path: &Path) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported("reveal_in_file_manager"))
}

#[cfg(any(target_os = "macos", test))]
fn parse_clipboard_file_paths_output(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "missing value")
        .map(ToString::to_string)
        .collect()
}

#[cfg(target_os = "macos")]
fn read_clipboard_file_paths_sync() -> Result<Vec<String>, String> {
    let script = r#"
use framework "AppKit"
use scripting additions
set pb to current application's NSPasteboard's generalPasteboard()
set urls to pb's readObjectsForClasses:{current application's NSURL} options:(missing value)
if urls is missing value then return ""
set out to {}
repeat with u in urls
  set p to (u's |path|()) as text
  if p is not "missing value" then set end of out to p
end repeat
set AppleScript's text item delimiters to linefeed
return out as text
"#;
    let mut cmd = Command::new("osascript");
    for line in script.lines().map(str::trim).filter(|l| !l.is_empty()) {
        cmd.arg("-e").arg(line);
    }
    cmd.stdin(std::process::Stdio::null());
    let output = cmd.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_clipboard_file_paths_output(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(not(target_os = "macos"))]
fn read_clipboard_file_paths_sync() -> Result<Vec<String>, String> {
    // 端口契约：平台没有对应能力时返回空列表，与 Tauri 适配层的非 macOS
    // 分支一致——不是错误。
    Ok(Vec::new())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::parse_clipboard_file_paths_output;

    #[test]
    fn parser_takes_file_urls_and_drops_missing_value() {
        let raw = "/Users/x/a.png\nmissing value\n\n  /Users/x/b folder/c.txt  \n";
        assert_eq!(
            parse_clipboard_file_paths_output(raw),
            vec![
                "/Users/x/a.png".to_string(),
                "/Users/x/b folder/c.txt".to_string()
            ]
        );
    }

    #[test]
    fn parser_returns_empty_for_blank_output() {
        assert!(parse_clipboard_file_paths_output("").is_empty());
        assert!(parse_clipboard_file_paths_output("missing value").is_empty());
    }
}
