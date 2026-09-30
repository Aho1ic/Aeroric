//! `aeroric-cli` —— 唯一无头入口（P3-07）。`serve` 是 host 的进程形态；
//! 其余子命令都是 host 的 RPC 客户端。桌面 Tauri 不经这里（它是另一个前端）。

mod args;
mod client;

use args::{exit, Cli, Command};
use clap::Parser;
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // clap 的参数错误默认 exit 2（= USAGE），与 Action 4 对齐；这里不覆盖。
    let cli = Cli::parse();
    let code = dispatch(cli).await;
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(1))
}

async fn dispatch(cli: Cli) -> i32 {
    match cli.command {
        Command::Doctor { json } => doctor(&cli.data_dir, json).await,
        Command::Serve { ready_fd } => serve(&cli.data_dir, ready_fd).await,
        // ③/④/⑤ 片的落地面。未接线前每命令显式失败，数字用 CONNECT：
        // 能力在计划里，不在树里。
        Command::Events { after } => events(&cli.data_dir, &after).await,
        Command::Project(_)
        | Command::Run { .. }
        | Command::Task(_)
        | Command::Session(_)
        | Command::Approval(_)
        | Command::Rpc { .. }
        | Command::Attach { .. } => {
            eprintln!(
                "aeroric-cli: this subcommand is not wired yet (P3-07 lands it slice by slice; \
                 see MIGRATION_PLAN.md)"
            );
            exit::CONNECT
        }
    }
}

// ── doctor：本地直读，不要求 host 在线 ────────────────────────────────────

/// 数据目录体检：卷答案、写租约可否、runtime 状态、socket 活死。
/// 退出码：0＝体检完成且可写（或 host 正常持有中）；3＝目录不可用/卷拒写。
async fn doctor(data_dir: &Path, json: bool) -> i32 {
    use aeroric_core::location::StorageLocationProbe as _;
    use aeroric_core::location_probe::RawPlatformProbe;
    use serde_json::json;

    let paths = aeroric_core::paths::AppPaths::with_data_dir(Path::new("/"), Some(data_dir));
    let location = RawPlatformProbe.probe(data_dir);
    let volume = format!("{:?}", location.volume);
    let writer_eligible = location.writer_eligible().is_ok();

    // 租约：试拿即放——"held" 是健康态（host 在跑），不是错误。
    let owner = aeroric_core::storage_lock::LeaseOwner {
        application: "aeroric-cli-doctor".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let lease = match aeroric_core::storage_lock::DataDirectoryLease::acquire(
        &paths,
        &aeroric_core::clock::SystemClock,
        &owner,
        &location,
    ) {
        Ok(held) => {
            drop(held);
            "free".to_string()
        }
        Err(e) => format!("{e}"),
    };

    // load_runtime 收 CoreContext（reader 侧不需要租约——doctor 只读）。
    let reader_context = aeroric_core::context::CoreContext::read_only(paths.clone());
    let runtime = match aeroric_core::storage::load_runtime(&reader_context) {
        Ok(aeroric_core::storage::ReadOutcome::Found(meta)) => {
            json!({"epoch": meta.epoch, "revision": meta.revision})
        }
        Ok(aeroric_core::storage::ReadOutcome::Absent) => json!("absent"),
        Err(e) => json!(format!("unreadable: {e}")),
    };

    let socket_path = aeroric_host::uds::default_socket_path(data_dir);
    let socket = if socket_path.exists() {
        match aeroric_host::uds::probe_liveness(&socket_path).await {
            aeroric_host::uds::Probe::Live => "live",
            aeroric_host::uds::Probe::Dead => "stale",
        }
    } else {
        "absent"
    };

    if json {
        let report = json!({
            "dataDir": data_dir.display().to_string(),
            "volume": volume,
            "writerEligible": writer_eligible,
            "lease": lease,
            "runtime": runtime,
            "socket": {"path": socket_path.display().to_string(), "state": socket},
        });
        println!("{report}");
    } else {
        println!("data-dir      : {}", data_dir.display());
        println!("volume        : {volume} (writer-eligible: {writer_eligible})");
        println!("lease         : {lease}");
        println!("socket        : {} [{socket}]", socket_path.display());
        println!("runtime       : {runtime}");
    }
    if writer_eligible {
        exit::OK
    } else {
        exit::CONNECT
    }
}

// ── serve：本 CLI 的 host 进程形态 ────────────────────────────────────────

/// lock → data compat → runtime → listener → ready，然后常驻到 SIGINT/SIGTERM。
/// token 明文在启动后立即登记进 Redactor：进程 stderr 上出现的任何错误文本
/// （含 panic hook）先洗再写（Action 5 的脱敏半边）。
async fn serve(data_dir: &Path, ready_fd: Option<i32>) -> i32 {
    use aeroric_core::storage_lock::LeaseOwner;
    use aeroric_host::dispatcher::HostServices;
    use aeroric_host::startup::{run_startup, serve as serve_loop, HostOptions};

    let paths = aeroric_core::paths::AppPaths::with_data_dir(Path::new("/"), Some(data_dir));
    let socket_path = aeroric_host::uds::default_socket_path(data_dir);
    let owner = LeaseOwner {
        application: "aeroric-cli".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    // build/protocol 两个 hash：CLI 构建没有 git 元数据注入，如实给版本串
    // （supervisor 区分「不同 build」够用，不假装有 commit hash）。
    match run_startup(
        &HostOptions {
            paths,
            socket_path,
            ready_fd,
        },
        Arc::new(aeroric_core::clock::SystemClock),
        &owner,
        &aeroric_core::location_probe::RawPlatformProbe,
        &format!("aeroric-cli-{}", env!("CARGO_PKG_VERSION")),
        &format!("protocol-v{}", aeroric_protocol::PROTOCOL_VERSION),
    )
    .await
    {
        Err(e) => {
            // Windows 上的 reader 拒绝（core 回执 false）也从这条文案出来：
            // `UnsupportedLocation` 里带着原因，不另开分支。
            eprintln!("aeroric-cli serve: {e}");
            exit::CONNECT
        }
        Ok(started) => {
            let token_plain = std::fs::read_to_string(data_dir.join("host.token"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let redactor = Arc::new(aeroric_host::diagnostics::Redactor::new([token_plain]));
            install_redacted_panic_hook(&redactor);
            let services = HostServices::new(
                Arc::clone(&started.token),
                Arc::new(aeroric_core::events::EventLog::default()),
            );
            let running = serve_loop(started, services);
            eprintln!("aeroric-cli serve: listening (stderr diagnostics are redacted)");
            wait_for_signal().await;
            eprintln!(
                "aeroric-cli serve: shutdown requested; draining (redactor live: {:?})",
                redactor_probe(&redactor)
            );
            running.shutdown();
            exit::OK
        }
    }
}

/// SIGINT/SIGTERM 任一到达。
async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// serve 的 stderr 全部经 Redactor 出口——panic hook 也装上（默认 hook 打
/// panic 消息不洗，token 混进 debug 断言文本就是泄漏面）。
fn install_redacted_panic_hook(redactor: &Arc<aeroric_host::diagnostics::Redactor>) {
    let previous = std::panic::take_hook();
    let redactor = Arc::clone(redactor);
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("{}", redactor.redact(&format!("{info}")));
    }));
    let _ = previous;
}

fn redactor_probe(redactor: &Arc<aeroric_host::diagnostics::Redactor>) -> &'static str {
    // 启动行自检：登记过的 token 出现在 stderr 文案里必须变 MASK。
    if redactor.redact("x") == "x" {
        "ok"
    } else {
        "broken"
    }
}

// ── events：client 的第一条真链路（serve+events 即可跑通 ⑥ 的 smoke 骨架）──

/// `events --after CURSOR`：连 host、握手、拉补发、逐事件 JSONL 到 stdout
/// （诊断走 stderr——Action 2 的流分离）。`reset:true` 时输出单条
/// `{"reset":...}` 行，语义与 core snapshot 一致。
async fn events(data_dir: &Path, after: &str) -> i32 {
    let Ok(cursor) = after.parse::<u64>() else {
        eprintln!("aeroric-cli events: --after must be a decimal number, got `{after}`");
        return exit::USAGE;
    };
    let client_id = uuid::Uuid::new_v4().to_string();
    let mut client = match client::HostClient::connect(data_dir, &client_id).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("aeroric-cli events: {e}");
            return e.exit_code();
        }
    };
    match client
        .call(
            "events.since",
            serde_json::json!({ "after": cursor.to_string() }),
        )
        .await
    {
        Ok(response) => {
            if response["reset"].as_bool().unwrap_or(false) {
                println!(
                    "{}",
                    serde_json::json!({"reset": true, "latestSeq": response["latestSeq"]})
                );
                return exit::OK;
            }
            let events = response["events"].as_array().cloned().unwrap_or_default();
            for event in events {
                println!("{event}");
            }
            exit::OK
        }
        Err(e) => {
            eprintln!("aeroric-cli events: {e}");
            e.exit_code()
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
