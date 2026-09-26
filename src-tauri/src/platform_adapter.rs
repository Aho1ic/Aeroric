//! 平台服务适配器(P3-01 切片五,计划 Action 4):宿主对 core
//! [`PlatformServices`](aeroric_core::runtime::PlatformServices) 端口的实现。
//! OS 调用体自宿主 `fs.rs` 的命令体原样迁入——reveal 的三种 OS 形状
//! (macOS `open[-R]`、Windows `explorer[/select]`、Linux `xdg-open` 父目录)与
//! 剪贴板文件路径的 osascript 读取,逐字节同语义。路径校验(`validate_path_within`)
//! 是 webview 请求的安全边界,留在命令体,不进端口实现。

use std::path::Path;
use std::process::Command;

use aeroric_core::runtime::{PlatformError, PlatformServices};

pub struct HostPlatformServices;

impl HostPlatformServices {
    /// reveal 的参数表抽成纯函数:三种 OS 各自的"程序 + 参数"可以从源码断言,
    /// 不必真的拉起文件管理器。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn reveal_args(
        target: &Path,
        is_dir: bool,
    ) -> Result<(&'static str, Vec<String>), PlatformError> {
        #[cfg(target_os = "macos")]
        {
            let mut args = vec![];
            if is_dir {
                args.push(target.to_string_lossy().into_owned());
            } else {
                args.push("-R".to_string());
                args.push(target.to_string_lossy().into_owned());
            }
            Ok(("open", args))
        }
        #[cfg(target_os = "windows")]
        {
            let mut args = vec![];
            if is_dir {
                args.push(target.to_string_lossy().into_owned());
            } else {
                args.push(format!("/select,{}", target.display()));
            }
            Ok(("explorer", args))
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if is_dir {
                Ok(("xdg-open", vec![target.to_string_lossy().into_owned()]))
            } else {
                let folder = target
                    .parent()
                    .ok_or_else(|| PlatformError::Io("Cannot resolve parent directory".into()))?
                    .to_path_buf();
                Ok(("xdg-open", vec![folder.to_string_lossy().into_owned()]))
            }
        }
    }

    fn reveal_sync(target: &Path, is_dir: bool) -> Result<(), PlatformError> {
        let (program, args) = Self::reveal_args(target, is_dir)?;
        let mut command = Command::new(program);
        command.args(&args);
        let status = command.status().map_err(|e| {
            PlatformError::Io(format!("Failed to launch system file manager: {}", e))
        })?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::Io(format!(
                "System file manager exited with status {}",
                status
            )))
        }
    }
}

impl PlatformServices for HostPlatformServices {
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), PlatformError> {
        let is_dir = path.is_dir();
        Self::reveal_sync(path, is_dir)
    }

    fn read_clipboard_file_paths(&self) -> Result<Vec<String>, PlatformError> {
        read_clipboard_file_paths_sync().map_err(PlatformError::Io)
    }
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
    for line in script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        cmd.arg("-e").arg(line);
    }
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_clipboard_file_paths_output(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(not(target_os = "macos"))]
fn read_clipboard_file_paths_sync() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_reveal_selects_files_and_opens_directories() {
        let (program, args) =
            HostPlatformServices::reveal_args(Path::new("/tmp/notes/a.txt"), false).expect("args");
        assert_eq!(program, "open");
        assert_eq!(args, vec!["-R".to_string(), "/tmp/notes/a.txt".to_string()]);

        let (program, args) =
            HostPlatformServices::reveal_args(Path::new("/tmp/notes"), true).expect("args");
        assert_eq!(program, "open");
        assert_eq!(args, vec!["/tmp/notes".to_string()], "目录直接打开,不带 -R");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_reveal_selects_files_and_opens_directories() {
        let (program, args) =
            HostPlatformServices::reveal_args(Path::new("C:\\notes\\a.txt"), false).expect("args");
        assert_eq!(program, "explorer");
        assert_eq!(args, vec!["/select,C:\\notes\\a.txt".to_string()]);

        let (program, args) =
            HostPlatformServices::reveal_args(Path::new("C:\\notes"), true).expect("args");
        assert_eq!(program, "explorer");
        assert_eq!(args, vec!["C:\\notes".to_string()]);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_reveal_opens_the_parent_of_files() {
        let (program, args) =
            HostPlatformServices::reveal_args(Path::new("/tmp/notes"), true).expect("args");
        assert_eq!(program, "xdg-open");
        assert_eq!(args, vec!["/tmp/notes".to_string()]);

        let (program, args) =
            HostPlatformServices::reveal_args(Path::new("/tmp/notes/a.txt"), false).expect("args");
        assert_eq!(program, "xdg-open");
        assert_eq!(args, vec!["/tmp/notes".to_string()], "文件打开其父目录");
    }

    #[cfg(any(target_os = "macos", test))]
    #[test]
    fn parses_clipboard_file_paths_output() {
        assert_eq!(
            parse_clipboard_file_paths_output("/Users/me/a.txt\n/Users/me/Folder\n\n"),
            vec![
                "/Users/me/a.txt".to_string(),
                "/Users/me/Folder".to_string()
            ]
        );
    }
}
