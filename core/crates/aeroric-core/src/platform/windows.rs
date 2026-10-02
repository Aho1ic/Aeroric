//! P7-02 Action 1：windows 分支（cfg 隔离；**不删除 Windows 分支**——跨平台
//! 实现继续编译；Windows 专用实现在此）。
/// Windows 平台的路径探测（.exe/.cmd/.bat）。
pub fn candidate_paths(binary: &str) -> Vec<String> {
    vec![
        format!("{binary}.exe"),
        format!("{binary}.cmd"),
        format!("{binary}.bat"),
    ]
}

/// WSL 命令前缀（Windows 下经 `wsl.exe` 调用；保留 Windows 专用实现）。
pub fn wsl_command_prefix() -> Vec<String> {
    vec!["wsl.exe".to_string()]
}

#[cfg(test)]
mod tests {
    #[test]
    fn wsl_command_prefix_present() {
        let prefix = super::wsl_command_prefix();
        assert_eq!(prefix[0], "wsl.exe", "Windows 专用 wsl.exe 前缀保留");
    }
}
