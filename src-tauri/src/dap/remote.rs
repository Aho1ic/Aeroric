//! 通过 SSH 在远端跑调试相关命令。
//!
//! 从 `dap.rs` 整块搬出来,内容一行没改。这里拼的是要发到远端 shell 的命令串,
//! 所以**每个插进去的值都必须先过引号转义**;路径部分的校验在兄弟模块
//! `paths.rs`(`normalize_remote_debug_root` / `join_remote_debug_path`),
//! 调用顺序是先校验再拼。
//!
//! 四个 `build_*_command` 分别对应:读配置、写配置、起 Node inspector、起 debugpy。

use super::*;

pub(super) fn build_remote_debug_read_configs_command(remote_root: &str) -> String {
    let config_path = remote_debug_config_path(remote_root);
    let script = "path=$1; if [ -f \"$path\" ]; then cat -- \"$path\"; fi";
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(&config_path)
    )
}

pub(super) fn build_remote_debug_write_configs_command(remote_root: &str) -> String {
    let config_path = remote_debug_config_path(remote_root);
    let parent = if let Some((parent, _)) = config_path.rsplit_once('/') {
        if parent.is_empty() {
            "/"
        } else {
            parent
        }
    } else {
        "."
    };
    format!(
        "mkdir -p -- {} && cat > {}",
        crate::ssh::shell_quote_posix(parent),
        crate::ssh::shell_quote_posix(&config_path)
    )
}

pub(super) fn build_remote_node_launch_command(
    cwd: &str,
    program: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> String {
    let env_args = env
        .iter()
        .map(|(key, value)| crate::ssh::shell_quote_posix(&format!("{key}={value}")))
        .collect::<Vec<_>>();
    let mut command_parts = Vec::new();
    if env_args.is_empty() {
        command_parts.push("node".to_string());
    } else {
        command_parts.push("env".to_string());
        command_parts.extend(env_args);
        command_parts.push("node".to_string());
    }
    command_parts.push("--inspect-brk=127.0.0.1:0".to_string());
    command_parts.push(crate::ssh::shell_quote_posix(program));
    command_parts.extend(args.iter().map(|arg| crate::ssh::shell_quote_posix(arg)));
    format!(
        "cd -- {} && {}",
        crate::ssh::shell_quote_posix(cwd),
        command_parts.join(" ")
    )
}

pub(super) fn build_remote_python_launch_command(
    cwd: &str,
    program: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> String {
    let env_args = env
        .iter()
        .map(|(key, value)| crate::ssh::shell_quote_posix(&format!("{key}={value}")))
        .collect::<Vec<_>>();
    let script = concat!(
        "import debugpy, runpy, sys\n",
        "program = sys.argv[1]\n",
        "sys.argv = sys.argv[1:]\n",
        "host, port = debugpy.listen(('127.0.0.1', 0))\n",
        "print(f'AERORIC_DEBUGPY_PORT={port}', file=sys.stderr, flush=True)\n",
        "debugpy.wait_for_client()\n",
        "runpy.run_path(program, run_name='__main__')\n"
    );
    let mut command_parts = Vec::new();
    if env_args.is_empty() {
        command_parts.push("python3".to_string());
    } else {
        command_parts.push("env".to_string());
        command_parts.extend(env_args);
        command_parts.push("python3".to_string());
    }
    command_parts.push("-u".to_string());
    command_parts.push("-c".to_string());
    command_parts.push(crate::ssh::shell_quote_posix(script));
    command_parts.push(crate::ssh::shell_quote_posix(program));
    command_parts.extend(args.iter().map(|arg| crate::ssh::shell_quote_posix(arg)));
    format!(
        "cd -- {} && {}",
        crate::ssh::shell_quote_posix(cwd),
        command_parts.join(" ")
    )
}

pub(super) fn run_remote_debug_output(
    connection: &SshConnection,
    remote_command: String,
) -> Result<Vec<u8>, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, remote_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

pub(super) fn read_remote_debug_configs_from_root(
    connection: &SshConnection,
    remote_root: &str,
) -> Result<DebugConfigDocument, String> {
    let stdout = run_remote_debug_output(
        connection,
        build_remote_debug_read_configs_command(remote_root),
    )?;
    if stdout.is_empty() {
        return Ok(DebugConfigDocument::default());
    }
    let raw = String::from_utf8(stdout).map_err(|err| err.to_string())?;
    let mut document: DebugConfigDocument =
        serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    if document.version == 0 {
        document.version = DEBUG_CONFIG_VERSION;
    }
    validate_remote_debug_configs(remote_root, &document)?;
    Ok(document)
}

pub(super) fn write_remote_debug_configs_from_root(
    connection: &SshConnection,
    remote_root: &str,
    mut document: DebugConfigDocument,
) -> Result<DebugConfigDocument, String> {
    document.version = DEBUG_CONFIG_VERSION;
    validate_remote_debug_configs(remote_root, &document)?;
    let raw = serde_json::to_string_pretty(&document).map_err(|err| err.to_string())?;
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_debug_write_configs_command(remote_root),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open ssh stdin".to_string())?;
        stdin
            .write_all(raw.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_remote_node_launch_config_and_quotes_command() {
        let mut env = BTreeMap::new();
        env.insert("NODE_ENV".to_string(), "test run".to_string());
        let config = DebugConfig {
            id: "remote-node-launch".to_string(),
            name: "Remote Node Launch".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Launch,
            program: "node_modules/vitest/vitest.mjs".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![
                "run".to_string(),
                "src/math test.ts".to_string(),
                "-t".to_string(),
                "adds numbers".to_string(),
            ],
            env,
            breakpoints: vec![DebugBreakpoint {
                file: "src/math test.ts".to_string(),
                line: 4,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/app repo", &config).unwrap();
        let command = build_remote_node_launch_command(
            "/srv/app repo",
            "/srv/app repo/node_modules/vitest/vitest.mjs",
            &config.args,
            &config.env,
        );

        assert_eq!(
            command,
            "cd -- '/srv/app repo' && env 'NODE_ENV=test run' node --inspect-brk=127.0.0.1:0 '/srv/app repo/node_modules/vitest/vitest.mjs' 'run' 'src/math test.ts' '-t' 'adds numbers'"
        );
    }

    #[test]
    fn validates_remote_python_launch_config_and_builds_command() {
        let mut env = BTreeMap::new();
        env.insert("PYTHONPATH".to_string(), "lib path".to_string());
        let config = DebugConfig {
            id: "remote-python-launch".to_string(),
            name: "Remote Python Launch".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Launch,
            program: "app/main.py".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec!["--name".to_string(), "Ada Lovelace".to_string()],
            env,
            breakpoints: vec![DebugBreakpoint {
                file: "app/main.py".to_string(),
                line: 8,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/app repo", &config).unwrap();
        let command = build_remote_python_launch_command(
            "/srv/app repo",
            "/srv/app repo/app/main.py",
            &config.args,
            &config.env,
        );

        assert!(command
            .starts_with("cd -- '/srv/app repo' && env 'PYTHONPATH=lib path' python3 -u -c "));
        assert!(command.contains("debugpy.listen"));
        assert!(command.contains("AERORIC_DEBUGPY_PORT"));
        assert!(command.ends_with("'/srv/app repo/app/main.py' '--name' 'Ada Lovelace'"));
    }

    #[test]
    fn builds_remote_debug_config_commands_with_quoted_paths() {
        assert_eq!(
            build_remote_debug_read_configs_command("/srv/app repo"),
            "sh -c 'path=$1; if [ -f \"$path\" ]; then cat -- \"$path\"; fi' sh '/srv/app repo/.aeroric/debug-configs.json'"
        );
        assert_eq!(
            build_remote_debug_write_configs_command("/srv/app repo"),
            "mkdir -p -- '/srv/app repo/.aeroric' && cat > '/srv/app repo/.aeroric/debug-configs.json'"
        );
    }
}
