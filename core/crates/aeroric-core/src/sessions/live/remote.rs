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

/// Run `inner` (a shell command line) under the remote user's login shell, so
/// PATH is what they get in a terminal there.
fn login_shell(inner: &str) -> String {
    format!("exec \"${{SHELL:-/bin/sh}}\" -lc {}", quote(inner))
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

/// One line per agent, `<name>\t<first line of --version or empty>`.
pub fn probe_agents_command() -> String {
    login_shell(
        "for a in claude codex omp; do v=''; if command -v \"$a\" >/dev/null 2>&1; then \
         v=$(\"$a\" --version 2>/dev/null | head -n 1); fi; printf '%s\\t%s\\n' \"$a\" \"$v\"; done",
    )
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
        assert_eq!(
            command,
            "exec \"${SHELL:-/bin/sh}\" -lc 'cd '\\''/srv/it'\\''\\'\\'''\\''s'\\'' && exec '\\''claude'\\'' '\\''--resume'\\'' '\\''a b'\\'''"
        );
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
        assert!(version_command("omp").ends_with("'exec '\\''omp'\\'' --version'"));
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
    fn only_password_prompts_are_answered() {
        assert!(is_password_prompt("me@dev.example's password: "));
        assert!(is_password_prompt("Password:"));
        assert!(!is_password_prompt(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])?"
        ));
        assert!(!is_password_prompt("Enter passphrase for key '/k/id': "));
    }
}
