//! `aeroric-cli` —— 唯一无头入口（P3-07）。`serve` 是 host 的进程形态；
//! 其余子命令都是 host 的 RPC 客户端。桌面 Tauri 不经这里（它是另一个前端）。

mod args;

use args::{exit, Cli, Command};
use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // clap 的参数错误默认 exit 2（= USAGE），与 Action 4 对齐；这里不覆盖。
    let cli = Cli::parse();
    let code = dispatch(cli).await;
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(1))
}

async fn dispatch(cli: Cli) -> i32 {
    match cli.command {
        // ②/③/④/⑤ 片的落地面。未接线前每命令显式失败——exit 2 的老桩
        // 语义（「不是可用入口」）保留到接线片，数字用 CONNECT：能力在
        // 计划里，不在树里。
        Command::Doctor { .. }
        | Command::Serve { .. }
        | Command::Project(_)
        | Command::Run { .. }
        | Command::Task(_)
        | Command::Session(_)
        | Command::Approval(_)
        | Command::Rpc { .. }
        | Command::Events { .. }
        | Command::Attach { .. } => {
            eprintln!(
                "aeroric-cli: this subcommand is not wired yet (P3-07 lands it slice by slice; \
                 see MIGRATION_PLAN.md)"
            );
            exit::CONNECT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::args::Cli;
    use clap::{CommandFactory, Parser};

    /// 命令树是 Action 1 的逐字契约：`--help` 列真实参数、每个具名子命令
    /// 可解析、全局 `--data-dir` 必填。
    #[test]
    fn the_named_command_tree_parses() {
        let base = ["aeroric-cli", "--data-dir", "/tmp/x"];
        // 12 组逐条过解析器（不启动任何进程）。
        let cases: &[&[&str]] = &[
            &["doctor", "--json"],
            &["serve"],
            &["serve", "--ready-fd", "7"],
            &["project", "list"],
            &["project", "open", "/tmp/p"],
            &[
                "run",
                "--project",
                "/tmp/p",
                "--agent",
                "claude",
                "--prompt-file",
                "-",
                "--json",
            ],
            &["task", "list"],
            &["task", "get", "t1"],
            &["task", "cancel", "t1"],
            &["task", "complete", "t1"],
            &["task", "resume", "t1"],
            &["task", "input", "t1", "--text-file", "/tmp/t"],
            &["session", "list"],
            &["session", "history", "t1"],
            &["session", "export", "t1", "--out", "/tmp/o.md"],
            &["approval", "list"],
            &["approval", "respond", "e1", "--outcome-file", "/tmp/o.json"],
            &["rpc", "task.list", "--params-file", "/tmp/p.json", "--json"],
            &["events", "--after", "42"],
            &["attach", "t1"],
        ];
        for tail in cases {
            let mut argv: Vec<&str> = base.to_vec();
            argv.extend_from_slice(tail);
            assert!(
                Cli::try_parse_from(&argv).is_ok(),
                "must parse: {:?}",
                argv.join(" ")
            );
        }
    }

    #[test]
    fn data_dir_is_mandatory_and_usage_errors_exit_two() {
        // 缺 `--data-dir` = 参数错误：clap 的默认退出码必须是 2（Action 4）。
        let err = Cli::try_parse_from(["aeroric-cli", "doctor"]).unwrap_err();
        assert_eq!(
            err.exit_code(),
            crate::args::exit::USAGE,
            "clap 默认退出码漂移会静默破坏 CLI 契约"
        );
    }

    #[test]
    fn the_exit_code_table_is_the_contract() {
        // Action 4 的数字本身：改动要过 CLI 评审，先在这里钉死。
        assert_eq!(
            (
                crate::args::exit::OK,
                crate::args::exit::USAGE,
                crate::args::exit::CONNECT,
                crate::args::exit::TASK_FAILED,
                crate::args::exit::NEEDS_HUMAN,
                crate::args::exit::CANCELLED
            ),
            (0, 2, 3, 4, 5, 130)
        );
    }

    #[test]
    fn help_lists_the_real_parameters() {
        let help = Cli::command().render_help().to_string().to_lowercase();
        // 顶层 help 的契约面＝子命令全集＋全局参数；各子命令自己的参数
        // （prompt-file/params-file/text-file…）由 `the_named_command_tree_parses`
        // 的逐条 parse 钉住，不靠字符串包含。
        for token in [
            "--data-dir",
            "doctor",
            "serve",
            "project",
            "run",
            "task",
            "session",
            "approval",
            "rpc",
            "events",
            "attach",
        ] {
            assert!(help.contains(token), "help must list `{token}`");
        }
    }
}
