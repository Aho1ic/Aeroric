//! §3.8 前置检查: the user's own shell command, run in the project before a
//! rule wakes a model. Exit 0 passes. It runs as the user, through their
//! login shell (so their PATH and tools are there), with a time limit.
//!
//! 2026-11-15 待办第二批 runs its 验收检查 here too ([`run_tail`]): a test
//! suite says what failed at its end, so that output keeps its last part.
//!
//! 目检 2026-11-15: `zsh -lc` does not read `.zshrc`, where many people put
//! Homebrew on their PATH, and `gh` was "command not found". The command
//! gets the PATH the agents get (`login_shell_path`, read once from an
//! interactive login shell).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aeroric_protocol::schedule::CHECK_OUTPUT_CHARS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub passed: bool,
    /// stdout then stderr, cut to `CHECK_OUTPUT_CHARS`.
    pub output: String,
}

fn shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/bin/zsh".into())
}

fn cut(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= CHECK_OUTPUT_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(CHECK_OUTPUT_CHARS).collect();
    out.push_str("\n…（已截断）");
    out
}

/// The last `CHECK_OUTPUT_CHARS` of `text`.
fn cut_tail(text: &str) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= CHECK_OUTPUT_CHARS {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - CHECK_OUTPUT_CHARS).collect();
    format!("…（前面已截断）\n{tail}")
}

/// Run `command` in `dir`; a command still running after `timeout` is
/// killed and fails. The output keeps its first part.
pub fn run(command: &str, dir: &Path, timeout: Duration) -> CheckResult {
    run_with(command, dir, timeout, false)
}

/// [`run`], keeping the last part of the output (after the exit code).
pub fn run_tail(command: &str, dir: &Path, timeout: Duration) -> CheckResult {
    run_with(command, dir, timeout, true)
}

fn run_with(command: &str, dir: &Path, timeout: Duration, tail: bool) -> CheckResult {
    // `head` says how it ended; `body` is what it printed.
    let finish = |head: String, body: &str| {
        let body = body.trim();
        let joined = if body.is_empty() {
            head.clone()
        } else if head.is_empty() {
            body.to_string()
        } else {
            format!("{head}：{body}")
        };
        if !tail {
            return cut(&joined);
        }
        match (head.is_empty(), body.is_empty()) {
            (_, true) => head,
            (true, false) => cut_tail(body),
            (false, false) => format!("{head}：{}", cut_tail(body)),
        }
    };
    let spawned = Command::new(shell())
        .arg("-lc")
        .arg(command)
        .env("PATH", crate::platform::login_shell_path())
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return CheckResult {
                passed: false,
                output: format!("没能运行检查命令：{error}"),
            }
        }
    };
    // Read both pipes on their own threads, so a chatty command cannot fill
    // one and hang while we wait.
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let out = reader(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = reader(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };
    let mut text = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    if !stderr.trim().is_empty() {
        if !text.trim().is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    match status {
        Some(status) => CheckResult {
            passed: status.success(),
            output: if status.success() {
                finish(String::new(), &text)
            } else {
                finish(
                    format!(
                        "退出码 {}",
                        status.code().map_or("未知".into(), |c| c.to_string())
                    ),
                    &text,
                )
            },
        },
        None => CheckResult {
            passed: false,
            output: finish(
                format!("超过 {} 秒没结束，已停止", timeout.as_secs()),
                &text,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_zero_passes_with_its_output_and_the_rest_say_why() {
        let dir = std::env::temp_dir();
        let ok = run(
            "echo 有 2 个新提交; pwd >/dev/null",
            &dir,
            Duration::from_secs(10),
        );
        assert_eq!(
            ok,
            CheckResult {
                passed: true,
                output: "有 2 个新提交".into()
            }
        );
        let no = run("echo 没有新提交 >&2; exit 3", &dir, Duration::from_secs(10));
        assert!(!no.passed);
        assert_eq!(no.output, "退出码 3：没有新提交");
        let slow = run("sleep 5", &dir, Duration::from_millis(300));
        assert!(!slow.passed && slow.output.contains("没结束"), "{slow:?}");
        let long = run("yes | head -c 9000", &dir, Duration::from_secs(10));
        assert!(long.passed && long.output.ends_with("（已截断）"));
    }

    /// 待办的验收检查: a failing suite's last lines survive, after the exit code.
    #[test]
    fn the_tail_runner_keeps_the_end_of_a_long_failure() {
        let dir = std::env::temp_dir();
        let failed = run_tail(
            "yes 通过 | head -n 3000; echo 'test auth::login ... FAILED'; exit 101",
            &dir,
            Duration::from_secs(10),
        );
        assert!(!failed.passed);
        assert!(
            failed.output.starts_with("退出码 101：…（前面已截断）\n"),
            "{}",
            failed.output.chars().take(40).collect::<String>()
        );
        assert!(failed.output.ends_with("test auth::login ... FAILED"));
        let short = run_tail("echo ok", &dir, Duration::from_secs(10));
        assert_eq!((short.passed, short.output.as_str()), (true, "ok"));
        let quiet = run_tail("exit 2", &dir, Duration::from_secs(10));
        assert_eq!(quiet.output, "退出码 2");
    }
}
