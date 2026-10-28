//! SSH remote native sessions (2026-10-27): the agent runs on the remote
//! machine under the user's own login shell, its stdio carried by one `ssh`
//! process (no TTY). Pure command building; the host spawns and authenticates.
//!
//! The remote side uses the user's own configuration and login there; nothing
//! managed is installed remotely. Unknown hosts are refused
//! (`StrictHostKeyChecking=yes`), so password auto-login never answers a host
//! key question.

/// Where to connect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub identity_file: Option<String>,
    /// Known-hosts files trusted for this host (the user's and Aeroric's own).
    pub known_hosts: Vec<String>,
}

/// One POSIX-shell word: single-quoted, `'` written as `'\''`.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// A remote path as a shell word, `~` and `~/…` expanded on the remote side.
pub fn remote_path(path: &str) -> String {
    if path == "~" {
        "\"$HOME\"".to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("\"$HOME\"/{}", quote(rest))
    } else {
        quote(path)
    }
}

/// Where offline installs put agents (O3, 2026-10-28); always first on PATH
/// for what Aeroric runs remotely, the user's shell files are never edited.
pub const OFFLINE_BIN: &str = "$HOME/.local/bin";

/// Run `inner` (a shell command line) under the remote user's login shell, so
/// PATH is what they get in a terminal there (plus [`OFFLINE_BIN`] first).
fn login_shell(inner: &str) -> String {
    format!(
        "exec \"${{SHELL:-/bin/sh}}\" -lc {}",
        quote(&format!(
            "PATH=\"{OFFLINE_BIN}:$PATH\"; export PATH; {inner}"
        ))
    )
}

/// `cd <cwd> && exec <program> <args…>` under the login shell.
pub fn remote_command(cwd: &str, program: &str, args: &[String]) -> String {
    let mut inner = format!("cd {} && exec {}", remote_path(cwd), quote(program));
    for arg in args {
        inner.push(' ');
        inner.push_str(&quote(arg));
    }
    login_shell(&inner)
}

/// `<program> --version`, on its own (never appended to a session command).
pub fn version_command(program: &str) -> String {
    login_shell(&format!("exec {} --version", quote(program)))
}

/// Hosts a direct (remote-side) install downloads from; the probe checks
/// the remote machine can reach each (O1).
pub const DIRECT_HOSTS: [&str; 3] = [
    "registry.npmjs.org",
    "downloads.claude.ai",
    "build.stencil.so",
];

/// One line per agent and tool, `<name>\t<first line of --version or empty>`
/// (npm and node decide how a missing agent can be installed), then
/// `platform\t<uname -s> <uname -m> <musl|libc>` and `net\t<host>\t<ok|no>`
/// for each of [`DIRECT_HOSTS`] (unknown without curl).
pub fn probe_agents_command() -> String {
    let mut line = String::from(
        "for a in claude codex omp npm node; do v=''; if command -v \"$a\" >/dev/null 2>&1; then \
         v=$(\"$a\" --version 2>/dev/null | head -n 1); fi; printf '%s\\t%s\\n' \"$a\" \"$v\"; done; \
         l=libc; if [ -f /etc/alpine-release ] || { command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; }; \
         then l=musl; fi; printf 'platform\\t%s %s %s\\n' \"$(uname -s)\" \"$(uname -m)\" \"$l\"; \
         if command -v curl >/dev/null 2>&1; then",
    );
    for host in DIRECT_HOSTS {
        line.push_str(&format!(
            " (if curl -sS -o /dev/null -m 6 -I https://{host}/ >/dev/null 2>&1; then r=ok; else r=no; fi; \
             printf 'net\\t%s\\t%s\\n' {host} \"$r\") &"
        ));
    }
    line.push_str(" wait; fi");
    login_shell(&line)
}

/// The remote machine's platform per the probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemotePlatform {
    /// `darwin` or `linux`.
    pub os: String,
    /// `arm64` or `x64`.
    pub arch: String,
    pub musl: bool,
}

/// The probe's `platform` line; `None` for an OS / CPU no agent ships for.
pub fn parse_platform_probe(stdout: &str) -> Option<RemotePlatform> {
    let rest = stdout
        .lines()
        .find_map(|line| line.strip_prefix("platform\t"))?;
    let mut words = rest.split_whitespace();
    let os = match words.next()? {
        "Darwin" => "darwin",
        "Linux" => "linux",
        _ => return None,
    };
    let arch = match words.next()? {
        "arm64" | "aarch64" => "arm64",
        "x86_64" | "amd64" => "x64",
        _ => return None,
    };
    Some(RemotePlatform {
        os: os.into(),
        arch: arch.into(),
        musl: os == "linux" && words.next() == Some("musl"),
    })
}

/// The probe's `net` lines: host → reachable.
pub fn parse_net_probe(stdout: &str) -> Vec<(String, bool)> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.strip_prefix("net\t")?.split('\t');
            Some((parts.next()?.to_string(), parts.next()? == "ok"))
        })
        .collect()
}

/// The host a direct install of `agent` downloads from.
pub fn direct_host(agent: &str, has_npm: bool) -> &'static str {
    match (agent, has_npm) {
        (_, true) | ("codex", _) => "registry.npmjs.org",
        ("claude", false) => "downloads.claude.ai",
        _ => "build.stencil.so",
    }
}

/// The vendor's name for `platform` in `agent`'s standalone builds.
pub fn package_platform(agent: &str, platform: &RemotePlatform) -> String {
    let RemotePlatform { os, arch, musl } = platform;
    match agent {
        "claude" => format!("{os}-{arch}{}", if *musl { "-musl" } else { "" }),
        "codex" => {
            let cpu = if arch == "arm64" { "aarch64" } else { "x86_64" };
            if os == "darwin" {
                format!("{cpu}-apple-darwin")
            } else {
                format!("{cpu}-unknown-linux-musl")
            }
        }
        _ => {
            let cpu = if arch == "arm64" { "arm64" } else { "x86_64" };
            match (os.as_str(), musl) {
                ("darwin", _) => format!("macos-{cpu}"),
                (_, true) => format!("linux-musl-{cpu}"),
                _ => format!("linux-{cpu}"),
            }
        }
    }
}

/// Receive one standalone build on stdin, check it and put it in place
/// (O3/O6): nothing is left behind when a step fails. `archive` = a
/// `.tar.gz` holding one executable (Codex). Prints `AERORIC_STEP <name>`
/// as it goes and the agent's `--version` at the end.
pub fn offline_install_command(agent: &str, version: &str, sha256: &str, archive: bool) -> String {
    let place = match agent {
        "claude" => format!(
            "d=\"$HOME/.local/share/claude/versions\"; mkdir -p \"$d\"; chmod +x \"$f\"; \
             mv -f \"$f\" \"$d\"/{v}; ln -sfn \"$d\"/{v} \"{OFFLINE_BIN}/claude\"",
            v = quote(version)
        ),
        _ => format!("chmod +x \"$f\"; mv -f \"$f\" \"{OFFLINE_BIN}/{agent}\""),
    };
    let unpack = if archive {
        "mkdir \"$t/x\" && tar -xzf \"$t/pkg\" -C \"$t/x\" || { echo '解压失败' >&2; exit 4; }; \
         f=$(find \"$t/x\" -type f -name 'codex*' | head -n 1); [ -n \"$f\" ] || { echo '压缩包里没有可执行文件' >&2; exit 4; }; "
    } else {
        "f=\"$t/pkg\"; "
    };
    login_shell(&format!(
        "set -u; t=$(mktemp -d \"${{TMPDIR:-/tmp}}/aeroric-install.XXXXXX\") || exit 4; trap 'rm -rf \"$t\"' EXIT; \
         echo 'AERORIC_STEP upload'; cat > \"$t/pkg\" || {{ echo '接收安装包失败' >&2; exit 4; }}; \
         echo 'AERORIC_STEP verify'; \
         if command -v sha256sum >/dev/null 2>&1; then h=$(sha256sum \"$t/pkg\" | cut -d' ' -f1); \
         elif command -v shasum >/dev/null 2>&1; then h=$(shasum -a 256 \"$t/pkg\" | cut -d' ' -f1); \
         else echo '远程没有 sha256sum 或 shasum，无法校验' >&2; exit 4; fi; \
         [ \"$h\" = {sha} ] || {{ echo '远程校验 sha256 不一致：传输可能被损坏' >&2; exit 4; }}; \
         {unpack}mkdir -p \"{OFFLINE_BIN}\"; {place}; \
         echo 'AERORIC_STEP check'; \"{OFFLINE_BIN}/{agent}\" --version",
        sha = quote(sha256)
    ))
}

/// The probe's npm / node versions.
pub fn parse_tool_probe(stdout: &str) -> (Option<String>, Option<String>) {
    let mut npm = None;
    let mut node = None;
    for line in stdout.lines() {
        if let Some((name, version)) = line.split_once('\t') {
            let version = version.trim();
            let version = (!version.is_empty()).then(|| version.to_string());
            match name.trim() {
                "npm" => npm = version,
                "node" => node = version,
                _ => {}
            }
        }
    }
    (npm, node)
}

/// What installs or upgrades `agent` on a remote machine (R1/R3, 2026-10-27):
/// npm when it is there, else the agent's own installer; `None` when the
/// machine needs Node.js first. `installed` picks an upgrade where the agent
/// can update itself without npm.
pub fn install_command(agent: &str, has_npm: bool, installed: bool) -> Option<String> {
    let npm = |package: &str| format!("npm install -g {package}@latest");
    match (agent, has_npm, installed) {
        ("claude", true, _) => Some(npm("@anthropic-ai/claude-code")),
        ("claude", false, true) => Some("claude update".into()),
        ("claude", false, false) => Some("curl -fsSL https://claude.ai/install.sh | bash".into()),
        ("codex", true, _) => Some(npm("@openai/codex")),
        ("omp", true, _) => Some(npm("@oh-my-pi/pi-coding-agent")),
        ("omp", false, true) => Some("omp update".into()),
        ("omp", false, false) => Some("curl -fsSL https://omp.sh/install | sh".into()),
        _ => None,
    }
}

/// Run a shell command line under the remote login shell (installs).
pub fn shell_command(line: &str) -> String {
    login_shell(line)
}

/// `<name>\t<version>` lines → (agent, version or None).
pub fn parse_agent_probe(stdout: &str) -> Vec<(String, Option<String>)> {
    stdout
        .lines()
        .filter_map(|line| {
            let (name, version) = line.split_once('\t')?;
            let version = version.trim();
            Some((
                name.trim().to_string(),
                (!version.is_empty()).then(|| version.to_string()),
            ))
        })
        .filter(|(name, _)| matches!(name.as_str(), "claude" | "codex" | "omp"))
        .collect()
}

/// Print a remote file, at most its last `max_bytes` (history, B4).
pub fn read_command(path: &str, max_bytes: u64) -> String {
    let file = remote_path(path);
    login_shell(&format!(
        "f={file}; [ -f \"$f\" ] || exit 3; s=$(wc -c < \"$f\"); \
         if [ \"$s\" -gt {max_bytes} ]; then tail -c {max_bytes} \"$f\"; else cat \"$f\"; fi"
    ))
}

/// `ssh` arguments for one session: no TTY, keepalives, strict host keys.
pub fn ssh_args(target: &SshTarget, remote_command: &str) -> Vec<String> {
    let mut args = vec![
        "-T".to_string(),
        "-p".to_string(),
        target.port.to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=3".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=15".to_string(),
    ];
    if !target.known_hosts.is_empty() {
        args.push("-o".to_string());
        args.push(format!(
            "UserKnownHostsFile={}",
            target
                .known_hosts
                .iter()
                .map(|path| path.replace(' ', "\\ "))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if let Some(identity) = target
        .identity_file
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        args.push("-i".to_string());
        args.push(identity.to_string());
    }
    args.push(format!("{}@{}", target.username, target.host));
    args.push(remote_command.to_string());
    args
}

// ── Through this Mac's gateway (G1–G6, 2026-10-28) ─────────────────────────

/// One value exported to a remote agent through the gateway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteValue {
    Text(String),
    /// `~/…`, expanded on the remote side.
    Path(String),
    /// The task token, read from the session's first stdin line (G1): never
    /// in argv, where `ps` on the remote machine would show it.
    Token,
}

/// What a remote session through the gateway needs on the remote machine:
/// its managed home (G2) and the environment pointing it at the forward.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatewayLaunch {
    pub env: Vec<(String, RemoteValue)>,
    pub unset: Vec<String>,
    /// Private directories to create (`~/…`).
    pub dirs: Vec<String>,
    /// (path `~/…`, content, overwrite) written privately before the agent starts.
    pub files: Vec<(String, String, bool)>,
}

/// A configuration's managed home on the remote machine (G2), mirroring
/// `<data>/agent-homes/<name>` here.
pub fn remote_home(home_dir_name: &str) -> Result<String, String> {
    let safe = !home_dir_name.is_empty()
        && !home_dir_name.starts_with('.')
        && home_dir_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if safe {
        Ok(format!("~/.aeroric/agent-homes/{home_dir_name}"))
    } else {
        Err(format!("Agent 预设的目录名 {home_dir_name} 不能用在远程"))
    }
}

/// `ssh` options forwarding the remote machine's `127.0.0.1:remote_port` to
/// this Mac's gateway (G3); ssh exits instead of running without it.
pub fn forward_args(remote_port: u16, local_port: u16) -> Vec<String> {
    vec![
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-R".into(),
        format!("127.0.0.1:{remote_port}:127.0.0.1:{local_port}"),
    ]
}

/// Whether ssh's stderr says the remote forward could not be set up.
pub fn is_forward_failure(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("remote port forwarding failed")
        || lower.contains("forwarding failed for listen port")
        || lower.contains("administratively prohibited")
}

/// A random port for the remote end of the forward.
pub fn pick_remote_port(seed: u128) -> u16 {
    20000 + (seed % 40000) as u16
}

/// The session command through the gateway: read the token line, prepare the
/// managed home, export the environment, then `cd` and exec as usual.
pub fn gateway_command(
    cwd: &str,
    program: &str,
    args: &[String],
    launch: &GatewayLaunch,
) -> String {
    let mut line = String::from("IFS= read -r AERORIC_T || exit 5; ");
    if !launch.dirs.is_empty() || !launch.files.is_empty() {
        let mut prepare = vec!["umask 077".to_string()];
        if !launch.dirs.is_empty() {
            let dirs: Vec<String> = launch.dirs.iter().map(|d| remote_path(d)).collect();
            prepare.push(format!("mkdir -p {}", dirs.join(" ")));
        }
        for (path, content, overwrite) in &launch.files {
            let write = format!("printf '%s' {} > {}", quote(content), remote_path(path));
            prepare.push(if *overwrite {
                write
            } else {
                format!("{{ [ -f {} ] || {write}; }}", remote_path(path))
            });
        }
        line.push_str(&format!(
            "( {} ) || {{ echo '无法在远程准备 Agent 预设目录' >&2; exit 5; }}; ",
            prepare.join(" && ")
        ));
    }
    for (name, value) in &launch.env {
        let value = match value {
            RemoteValue::Text(text) => quote(text),
            RemoteValue::Path(path) => remote_path(path),
            RemoteValue::Token => "\"$AERORIC_T\"".to_string(),
        };
        line.push_str(&format!("{name}={value}; export {name}; "));
    }
    if !launch.unset.is_empty() {
        line.push_str(&format!("unset {}; ", launch.unset.join(" ")));
    }
    line.push_str(
        "NO_PROXY=\"127.0.0.1,localhost,::1${NO_PROXY:+,$NO_PROXY}\"; \
         no_proxy=\"127.0.0.1,localhost,::1${no_proxy:+,$no_proxy}\"; export NO_PROXY no_proxy; unset AERORIC_T; ",
    );
    line.push_str(&format!(
        "cd {} && exec {}",
        remote_path(cwd),
        quote(program)
    ));
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    login_shell(&line)
}

/// What to tell the user when a remote agent is too old (B5: copy, no remote upgrade).
pub fn upgrade_hint(agent: &str) -> &'static str {
    match agent {
        "claude" => "npm install -g @anthropic-ai/claude-code@latest",
        "codex" => "npm install -g @openai/codex@latest",
        "omp" => "omp update",
        _ => "",
    }
}

/// A Claude transcript path on the remote machine (expanded there).
pub fn remote_claude_transcript(cwd: &str, session_id: &str) -> String {
    format!(
        "~/.claude/projects/{}/{session_id}.jsonl",
        crate::sessions::discovery::encode_claude_project_path(cwd)
    )
}

/// Whether an askpass prompt asks for a password (anything else — a host key
/// question, a passphrase for an unknown key — is refused).
pub fn is_password_prompt(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    lower.contains("password") && !lower.contains("yes/no") && !lower.contains("fingerprint")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> SshTarget {
        SshTarget {
            host: "dev.example".into(),
            port: 2222,
            username: "me".into(),
            identity_file: Some("/k/id".into()),
            known_hosts: vec![
                "/Users/me/.ssh/known_hosts".into(),
                "/data dir/known_hosts".into(),
            ],
        }
    }

    #[test]
    fn commands_run_under_the_login_shell_with_every_word_quoted() {
        let command = remote_command("/srv/it's", "claude", &["--resume".into(), "a b".into()]);
        assert!(command.starts_with(
            "exec \"${SHELL:-/bin/sh}\" -lc 'PATH=\"$HOME/.local/bin:$PATH\"; export PATH; cd "
        ));
        assert!(command.ends_with("&& exec '\\''claude'\\'' '\\''--resume'\\'' '\\''a b'\\'''"));
        // Round-trip through a real shell: the inner line is what sh -lc gets.
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "printf '%s' {}",
                quote(&format!(
                    "cd {} && exec {}",
                    remote_path("/srv/it's"),
                    quote("x y")
                ))
            ))
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "cd '/srv/it'\\''s' && exec 'x y'"
        );
    }

    #[test]
    fn home_relative_paths_expand_remotely() {
        assert_eq!(remote_path("~"), "\"$HOME\"");
        assert_eq!(remote_path("~/w/p"), "\"$HOME\"/'w/p'");
        assert_eq!(remote_path("/abs"), "'/abs'");
    }

    #[test]
    fn ssh_args_are_strict_and_tty_free() {
        let args = ssh_args(&target(), "CMD");
        assert_eq!(&args[..3], ["-T", "-p", "2222"]);
        assert!(args.contains(&"StrictHostKeyChecking=yes".to_string()));
        assert!(args.contains(
            &"UserKnownHostsFile=/Users/me/.ssh/known_hosts /data\\ dir/known_hosts".to_string()
        ));
        assert_eq!(&args[args.len() - 2..], ["me@dev.example", "CMD"]);
        assert!(!args.iter().any(|a| a == "-tt"));
    }

    #[test]
    fn reading_a_file_keeps_its_tail_and_the_version_runs_alone() {
        let read = read_command("~/.claude/projects/x/s.jsonl", 1024);
        assert!(read.contains("tail -c 1024"));
        assert!(read.contains("\"$HOME\"/"));
        assert!(version_command("omp").ends_with("exec '\\''omp'\\'' --version'"));
    }

    #[test]
    fn the_agent_probe_runs_in_a_shell_and_parses() {
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(
                probe_agents_command().replace("exec \"${SHELL:-/bin/sh}\" -lc", "exec /bin/sh -c"),
            )
            .output()
            .unwrap();
        let agents = parse_agent_probe(&String::from_utf8_lossy(&out.stdout));
        assert_eq!(
            agents.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(),
            ["claude", "codex", "omp"]
        );
        assert_eq!(
            parse_agent_probe("claude\t2.1.311 (Claude Code)\ncodex\t\nother\t1\n"),
            vec![
                ("claude".into(), Some("2.1.311 (Claude Code)".into())),
                ("codex".into(), None)
            ]
        );
    }

    #[test]
    fn installs_prefer_npm_and_fall_back_to_each_agents_installer() {
        assert_eq!(
            install_command("claude", true, false).as_deref(),
            Some("npm install -g @anthropic-ai/claude-code@latest")
        );
        assert_eq!(
            install_command("claude", false, true).as_deref(),
            Some("claude update")
        );
        assert!(install_command("claude", false, false)
            .unwrap()
            .starts_with("curl -fsSL https://claude.ai/install.sh"));
        assert_eq!(
            install_command("codex", false, false),
            None,
            "Codex needs Node.js"
        );
        assert_eq!(
            install_command("omp", false, true).as_deref(),
            Some("omp update")
        );
        assert_eq!(
            parse_tool_probe("npm\t10.8.2\nnode\tv22.1.0\nclaude\t\n"),
            (Some("10.8.2".into()), Some("v22.1.0".into()))
        );
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tempdir() -> TempDir {
        let dir = std::env::temp_dir().join(format!("aeroric-remote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// The script under `sh -c` with HOME in a temp dir (no login shell).
    fn run_offline(script: &str, home: &std::path::Path, payload: &[u8]) -> std::process::Output {
        use std::io::Write;
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script.replace("exec \"${SHELL:-/bin/sh}\" -lc", "exec /bin/sh -c"))
            .env("HOME", home)
            .env("TMPDIR", home)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload).unwrap();
        child.wait_with_output().unwrap()
    }

    fn sha(bytes: &[u8]) -> String {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(bytes))
    }

    #[test]
    fn offline_installs_check_the_sum_place_the_binary_and_leave_nothing_behind() {
        let home = tempdir();
        let binary = b"#!/bin/sh\necho 'omp/9.9.9'\n";
        let out = run_offline(
            &offline_install_command("omp", "9.9.9", &sha(binary), false),
            home.path(),
            binary,
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(stdout.contains("AERORIC_STEP verify") && stdout.ends_with("omp/9.9.9\n"));
        assert!(home.path().join(".local/bin/omp").is_file());

        let claude = b"#!/bin/sh\necho '2.1.312 (Claude Code)'\n";
        let out = run_offline(
            &offline_install_command("claude", "2.1.312", &sha(claude), false),
            home.path(),
            claude,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(home
            .path()
            .join(".local/share/claude/versions/2.1.312")
            .is_file());
        assert!(
            std::fs::symlink_metadata(home.path().join(".local/bin/claude"))
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let out = run_offline(
            &offline_install_command("codex", "1.0.0", &sha(b"other"), false),
            home.path(),
            b"corrupted",
        );
        assert_eq!(out.status.code(), Some(4));
        assert!(String::from_utf8_lossy(&out.stderr).contains("sha256"));
        assert!(!home.path().join(".local/bin/codex").exists());
        let leftovers = std::fs::read_dir(home.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("aeroric-install")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn codex_archives_are_unpacked_on_the_remote_side() {
        let home = tempdir();
        let work = tempdir();
        std::fs::write(
            work.path().join("codex-aarch64-apple-darwin"),
            "#!/bin/sh\necho 'codex-cli 0.165.0'\n",
        )
        .unwrap();
        let archive = work.path().join("pkg.tar.gz");
        assert!(std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(work.path())
            .arg("codex-aarch64-apple-darwin")
            .status()
            .unwrap()
            .success());
        let bytes = std::fs::read(&archive).unwrap();
        let out = run_offline(
            &offline_install_command("codex", "0.165.0", &sha(&bytes), true),
            home.path(),
            &bytes,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).ends_with("codex-cli 0.165.0\n"));
    }

    #[test]
    fn platforms_map_to_each_vendors_build_names() {
        let probe = "claude\t\nplatform\tLinux aarch64 musl\nnet\tregistry.npmjs.org\tno\nnet\tbuild.stencil.so\tok\n";
        let linux = parse_platform_probe(probe).unwrap();
        assert_eq!(
            linux,
            RemotePlatform {
                os: "linux".into(),
                arch: "arm64".into(),
                musl: true
            }
        );
        assert_eq!(package_platform("claude", &linux), "linux-arm64-musl");
        assert_eq!(
            package_platform("codex", &linux),
            "aarch64-unknown-linux-musl"
        );
        assert_eq!(package_platform("omp", &linux), "linux-musl-arm64");
        let mac = parse_platform_probe("platform\tDarwin x86_64 libc\n").unwrap();
        assert_eq!(package_platform("claude", &mac), "darwin-x64");
        assert_eq!(package_platform("codex", &mac), "x86_64-apple-darwin");
        assert_eq!(package_platform("omp", &mac), "macos-x86_64");
        assert_eq!(parse_platform_probe("platform\tFreeBSD amd64 libc\n"), None);
        assert_eq!(
            parse_net_probe(probe),
            vec![
                ("registry.npmjs.org".into(), false),
                ("build.stencil.so".into(), true)
            ]
        );
        assert_eq!(direct_host("codex", false), "registry.npmjs.org");
        assert_eq!(direct_host("claude", false), "downloads.claude.ai");
        assert_eq!(direct_host("omp", false), "build.stencil.so");
    }

    #[test]
    fn gateway_sessions_take_the_token_from_stdin_and_prepare_their_home() {
        let home = tempdir();
        let launch = GatewayLaunch {
            env: vec![
                (
                    "CLAUDE_CONFIG_DIR".into(),
                    RemoteValue::Path("~/.aeroric/agent-homes/p".into()),
                ),
                (
                    "ANTHROPIC_BASE_URL".into(),
                    RemoteValue::Text("http://127.0.0.1:20001/claude".into()),
                ),
                ("ANTHROPIC_AUTH_TOKEN".into(), RemoteValue::Token),
            ],
            unset: vec!["ANTHROPIC_API_KEY".into()],
            dirs: vec!["~/.aeroric/agent-homes/p/tmp".into()],
            files: vec![
                (
                    "~/.aeroric/agent-homes/p/a.yml".into(),
                    "it's\n".into(),
                    true,
                ),
                ("~/.aeroric/agent-homes/p/keep".into(), "new".into(), false),
            ],
        };
        std::fs::create_dir_all(home.path().join(".aeroric/agent-homes/p")).unwrap();
        std::fs::write(home.path().join(".aeroric/agent-homes/p/keep"), "old").unwrap();
        let script = gateway_command(
            "~",
            "sh",
            &["-c".into(), "printf '%s|%s|%s|%s' \"$CLAUDE_CONFIG_DIR\" \"$ANTHROPIC_AUTH_TOKEN\" \"${ANTHROPIC_API_KEY-unset}\" \"${AERORIC_T-gone}\"; cat".into()],
            &launch,
        );
        assert!(!script.contains("tok-123"));
        let out = run_offline(&script, home.path(), b"tok-123\n{\"type\":\"user\"}\n");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            stdout,
            format!(
                "{}/.aeroric/agent-homes/p|tok-123|unset|gone{{\"type\":\"user\"}}\n",
                home.path().display()
            ),
            "the token line is consumed; the protocol that follows reaches the agent"
        );
        let dir = home.path().join(".aeroric/agent-homes/p");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.yml")).unwrap(),
            "it's\n"
        );
        assert_eq!(std::fs::read_to_string(dir.join("keep")).unwrap(), "old");
        assert!(dir.join("tmp").is_dir());
    }

    #[test]
    fn forwards_are_strict_and_home_names_safe() {
        assert_eq!(
            forward_args(20001, 51234),
            [
                "-o",
                "ExitOnForwardFailure=yes",
                "-R",
                "127.0.0.1:20001:127.0.0.1:51234"
            ]
        );
        assert!(is_forward_failure(
            "Error: remote port forwarding failed for listen port 20001"
        ));
        assert!(!is_forward_failure("Permission denied"));
        assert!((20000..60000).contains(&pick_remote_port(u128::MAX)));
        assert_eq!(
            remote_home("custom-ab12").unwrap(),
            "~/.aeroric/agent-homes/custom-ab12"
        );
        assert!(remote_home("../x").is_err() && remote_home("a b").is_err());
    }

    #[test]
    fn only_password_prompts_are_answered() {
        assert!(is_password_prompt("me@dev.example's password: "));
        assert!(is_password_prompt("Password:"));
        assert!(!is_password_prompt(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])?"
        ));
        assert!(!is_password_prompt("Enter passphrase for key '/k/id': "));
    }
}
