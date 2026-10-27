//! 命令树（P3-07 Action 1 的逐字落地）。
//!
//! `--data-dir` 是全局必填参数：INV-03 的机器形状——数据根不从 `$HOME` 推导，
//! 只能由调用方给；serve 另在装配边界捕获配置用外部 home。`--help` 的等待语义与退出码表是契约文本
//! （Action 4），不是注释。

use clap::{Parser, Subcommand};

/// 无头 Aeroric 入口。
///
/// 等待语义：`run` 默认等待本轮进入 settled/terminal 再退出；交互请求
/// （审批、问题、composer）以结构化状态返回并以退出码 5 表示「需要人工」，
/// **不冒充完成**。`events`/`session history` 是非 TTY 场景的替代读取面。
///
/// 退出码：0 操作成功（不代表 task done）；2 参数错误；3 连接/认证失败；
/// 4 task failed；5 需要人工交互；130 明确取消。
#[derive(Parser, Debug)]
#[command(name = "aeroric-cli", version, about, long_about = None)]
pub struct Cli {
    /// 数据根（`~/.aeroric` 的等价物）；必填，不从环境变量推导。
    /// 顶层参数（命令树 `aeroric-cli --data-dir PATH <sub>` 的写法）——
    /// clap 的 global 与 required 互斥（debug assert 实测），且 Action 1
    /// 的树形本来就把 `--data-dir` 定在子命令之前。
    #[arg(long, value_name = "PATH", required = true)]
    pub data_dir: std::path::PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 数据目录与 host 可达性的诊断，stdout 只出 JSON。
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// 启动无头 host（lock → compat → runtime → listener → ready）。
    Serve {
        /// 已认证 RPC 之外的 ready 管道写端（supervisor 协议）；不传则不报 ready。
        #[arg(long, value_name = "FD")]
        ready_fd: Option<i32>,
    },
    #[command(subcommand)]
    Project(ProjectCmd),
    /// 连接（或按策略启动）host，创建并运行一个任务轮次。
    Run {
        #[arg(long, value_name = "PATH")]
        project: std::path::PathBuf,
        #[arg(long, value_name = "ID")]
        agent: String,
        /// prompt 从文件读（- 表示 stdin）；不走 argv 是防泄漏与长度限制。
        #[arg(long, value_name = "PATH")]
        prompt_file: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(subcommand)]
    Task(TaskCmd),
    #[command(subcommand)]
    Session(SessionCmd),
    #[command(subcommand)]
    Approval(ApprovalCmd),
    /// 对 host 已注册且通过 schema/capability 校验的方法发一次 RPC。
    /// 不等于任意代码执行：mutations 走同一 idempotency/revision 与确认契约。
    Rpc {
        method: String,
        #[arg(long, value_name = "PATH")]
        params_file: Option<std::path::PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// 拉取 cursor 之后的事件（JSONL）。
    Events {
        #[arg(long, value_name = "CURSOR")]
        after: String,
    },
    #[command(subcommand)]
    Backup(BackupCmd),
    /// 转发真实 PTY 字节（termios raw RAII；SIGWINCH→resize；退出 detach 不 cancel）。
    Attach { task_id: String },
}

#[derive(Subcommand, Debug)]
pub enum ProjectCmd {
    List,
    /// 把一个目录登记为项目（`run --project` 的既有值不登记）。
    Open {
        path: std::path::PathBuf,
    },
}

/// 数据目录的一致性备份 / 校验 / 恢复（AP8 Action 1）。
///
/// **默认只接受 `--data-dir` 指到哪就备到哪**:所有命令都读顶层 `--data-dir`,
/// 没有第二个"真实目录"的缺省。真实 `~/.aeroric` 的停写与备份是一次不可自动回退的
/// 操作(`e3836e62:docs/native/rollback.md`),所以本命令族不提供任何"自动找家目录"的捷径。
#[derive(Subcommand, Debug)]
pub enum BackupCmd {
    /// 建一份备份（含清单）。目标目录必须不存在或为空。
    Create {
        #[arg(long, value_name = "DIR")]
        out: std::path::PathBuf,
        /// 记账用：备份时确实有活写者。库走 backup API、文件走原子写，所以快照自洽。
        #[arg(long)]
        live: bool,
    },
    /// 重算摘要 + `integrity_check` + 行数对账。有任何问题即非零退出。
    Verify {
        #[arg(long, value_name = "DIR")]
        backup: std::path::PathBuf,
    },
    /// 把备份恢复到 `--data-dir`。默认拒绝覆盖备份之后的写入。
    Restore {
        #[arg(long, value_name = "DIR")]
        backup: std::path::PathBuf,
        /// 显式丢弃备份之后的新写入（"回滚"而不是"恢复"，需自己说出口）。
        #[arg(long)]
        discard_newer: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum TaskCmd {
    List {
        #[arg(long, value_name = "PATH")]
        project: Option<std::path::PathBuf>,
    },
    Get {
        task_id: String,
    },
    Cancel {
        task_id: String,
    },
    /// 「用户说这轮结束了」——显式 complete，与 settled 区分。
    Complete {
        task_id: String,
    },
    Resume {
        task_id: String,
    },
    /// 向任务追加输入：PTY 要求有效控制租约；结构化 Agent 走声明能力；
    /// 不支持的协议不猜按键。
    Input {
        task_id: String,
        #[arg(long, value_name = "PATH")]
        text_file: std::path::PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum SessionCmd {
    List {
        #[arg(long, value_name = "PATH")]
        project: Option<std::path::PathBuf>,
    },
    History {
        task_id: String,
    },
    Export {
        task_id: String,
        #[arg(long, value_name = "PATH")]
        out: std::path::PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum ApprovalCmd {
    List,
    Respond {
        event_id: String,
        /// outcome 的 JSON 文件（`{"kind": ...}`）；审批不靠命令行裸值。
        #[arg(long, value_name = "PATH")]
        outcome_file: std::path::PathBuf,
    },
}

/// Action 4 的退出码表。数字属于契约，改动要过 CLI 评审。
/// 常量先于消费者是这里的常态：① 片立全表（数字是验收文本），②③④⑤ 片
/// 逐命令接线时才各取所用——`dead_code` 对「契约表」没有意义，显式豁免。
#[allow(dead_code)]
pub mod exit {
    pub const OK: i32 = 0;
    pub const USAGE: i32 = 2;
    pub const CONNECT: i32 = 3;
    pub const TASK_FAILED: i32 = 4;
    pub const NEEDS_HUMAN: i32 = 5;
    pub const CANCELLED: i32 = 130;
}
