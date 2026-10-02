//! P7-02 Action 1：WSL git（cfg 隔离；与 `src-tauri/src/wsl_git.rs` 同源语义
//! 上收；git 命令经 wsl.exe 前缀）。
/// WSL git 命令构建（Windows 下经 `wsl.exe -d <distro> -- git ...`；纯函数）。
pub fn build_wsl_git_command(distribution: &str, cwd: &str, args: &[&str]) -> Vec<String> {
    let mut command = vec![
        "wsl.exe".to_string(),
        "-d".to_string(),
        distribution.to_string(),
    ];
    if !cwd.is_empty() {
        command.push("--cd".to_string());
        command.push(cwd.to_string());
    }
    command.push("--".to_string());
    command.push("git".to_string());
    for arg in args {
        command.push(arg.to_string());
    }
    command
}
