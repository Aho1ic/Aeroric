//! `attach TASK_ID` —— 真实 PTY 字节的终端中继（P3-07 ⑤）。
//!
//! 形状是 tmux 式的最小可用：
//! - **termios raw 的恢复有两条路径，不是一条**：正常路径（EOF、Ctrl-]、
//!   panic 展开）经 `RawMode::Drop`；**SIGTERM/SIGHUP 走安装的信号处理器**
//!   （`signal_restore_termios`：恢复＋重设默认处置＋re-raise，进程仍以
//!   信号死法收场）——默认处置直接终止进程，`Drop` 不会跑，这正是
//!   续跑验证 2 的具名遗留。处理器只碰信号前已发布到静态量的存档
//!   （`AtomicPtr`＋`AtomicI32`），不加锁不分配。
//! - **退出 = detach，不 cancel**：离开 attach 从不给任务发取消；任务在
//!   host 侧继续跑（再 attach 从 `nextOffset` 追平或按 gap 走 history）。
//! - **非 TTY 拒绝**：stdin/stdout 任一不是终端就退出并提示
//!   `events` / `session history`——管道里灌 ANSI 字节流是事故不是功能。
//! - 退出键 **Ctrl-]**（0x1d，本地消费不转发）；raw 下 Ctrl-C 是普通字节，
//!   原样进 PTY 由 agent 自己处置。
//! - SIGWINCH → `ioctl(TIOCGWINSZ)` → `task.resize` RPC（lease 闸在 host 侧）。

use base64::Engine as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::args::exit;
use crate::client::{self, HostClient};

/// raw 模式守卫：构造时改 termios，Drop 时恢复。
pub struct RawMode {
    saved: libc::termios,
    fd: i32,
}

impl RawMode {
    /// 进入 raw。`Ok(None)` = 不是终端（调用方走拒绝路径）。
    pub fn enter(fd: i32) -> Option<Self> {
        if unsafe { libc::isatty(fd) } != 1 {
            return None;
        }
        let mut saved = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, saved.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: tcgetattr 返回 0 时已写入完整结构。
        let saved = unsafe { saved.assume_init() };
        let mut raw = saved;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        Some(Self { saved, fd })
    }

    /// raw 前的存档副本（信号恢复钩子要用它）。
    fn termios_archive(&self) -> libc::termios {
        unsafe { std::ptr::read(&self.saved) }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // 恢复用 TCSADRAIN：把在途输出冲完再翻设置，切回的一瞬不吞字符。
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSADRAIN, &self.saved);
        }
    }
}

// ── SIGTERM/SIGHUP 的 termios 恢复（续跑验证 2 的具名遗留）────────────────
// 默认处置直接终止进程、`Drop` 不跑；处理器恢复后重设默认处置并 re-raise，
// 进程仍以信号死法收场（退出状态对 shell 可见为 128+信号）。
// 恢复用 TCSANOW 而非 Drop 里的 TCSADRAIN：drain 要等输出排空，而 pty 对端
// （tmux 切走、测试脚手架）没人读时 drain 永不完成——死亡路径会挂死进程
// （本机实测复现）。进程反正将死，不排水；正常退出仍走排水。
// 安全性：处理器只读信号前已发布的静态存档（Atomic load 无锁无分配）；
// 存档 Box 泄漏到进程生命周期，释放无从发生。
static RESTORE_TERMIOS: std::sync::atomic::AtomicPtr<libc::termios> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static RESTORE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn signal_restore_termios(sig: libc::c_int) {
    use std::sync::atomic::Ordering;
    unsafe {
        let saved = RESTORE_TERMIOS.load(Ordering::SeqCst);
        let fd = RESTORE_FD.load(Ordering::SeqCst);
        if fd >= 0 && !saved.is_null() {
            libc::tcsetattr(fd, libc::TCSANOW, saved);
        }
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// 把 raw 前的存档发布给处理器并装钩子（unix）。fd 由调用方以 `RawMode`
/// 的进入 fd 传入——存档与恢复必须落在同一个终端上。
#[cfg(unix)]
fn arm_signal_restore(saved: &libc::termios, fd: i32) {
    use std::sync::atomic::Ordering;
    // ptr::read 绕开 termios 是否 Copy 的平台差：按位复制一份。
    let leaked = Box::into_raw(Box::new(unsafe { std::ptr::read(saved) }));
    RESTORE_TERMIOS.store(leaked, Ordering::SeqCst);
    RESTORE_FD.store(fd, Ordering::SeqCst);
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = signal_restore_termios as extern "C" fn(libc::c_int) as usize;
        libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGHUP, &action, std::ptr::null_mut());
    }
}

fn current_winsize(fd: i32) -> Option<(u32, u32)> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as _, &mut ws) } != 0 {
        return None;
    }
    Some((ws.ws_col as u32, ws.ws_row as u32))
}

const DETACH_BYTE: u8 = 0x1d; // Ctrl-]

pub async fn attach(data_dir: &std::path::Path, task_id: &str) -> i32 {
    // 非 TTY 拒绝（先于连接——不给无终端的调用方留半截副作用）。
    if RawMode::enter(0).is_none() || unsafe { libc::isatty(1) } != 1 {
        eprintln!(
            "aeroric-cli attach: stdin/stdout must be a terminal. For non-interactive reads \
             use `events --after CURSOR` or `session history TASK_ID`."
        );
        return exit::USAGE;
    }
    // 真正进入 raw（上面的 enter 只是探测；这里取守卫，Drop 负责恢复）。
    let Some(_guard) = RawMode::enter(0) else {
        eprintln!("aeroric-cli attach: terminal entered raw mode and exited; re-run in a tty");
        return exit::USAGE;
    };
    // SIGTERM/SIGHUP 的恢复钩子尽早装上：raw 已生效而钩子未装的窗口里，
    // 一记默认处置的信号会把终端留在 raw（续跑验证 2 的具名遗留）。
    #[cfg(unix)]
    arm_signal_restore(&_guard.termios_archive(), 0);
    let client_id = uuid::Uuid::new_v4().to_string();
    let mut client = match HostClient::connect(data_dir, &client_id).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("aeroric-cli attach: {e}");
            return e.exit_code();
        }
    };
    // 起跑尺寸同步（attach 前窗口可能已经 resize 过）。
    if let Some((cols, rows)) = current_winsize(0) {
        let _ = client
            .call(
                "task.resize",
                serde_json::json!({ "taskId": task_id, "cols": cols, "rows": rows }),
            )
            .await;
    }

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut offset: u64 = 0;
    let mut chunk = [0u8; 4096];
    let mut winch = winch_stream();
    let mut poll = tokio::time::interval(std::time::Duration::from_millis(16));
    loop {
        tokio::select! {
            _ = winch.recv() => {
                if let Some((cols, rows)) = current_winsize(0) {
                    let _ = client
                        .call(
                            "task.resize",
                            serde_json::json!({ "taskId": task_id, "cols": cols, "rows": rows }),
                        )
                        .await;
                }
            }
            read = stdin.read(&mut chunk) => {
                match read {
                    Ok(0) => return exit::OK, // EOF = detach
                    Ok(n) => {
                        if let Some(pos) = chunk[..n].iter().position(|b| *b == DETACH_BYTE) {
                            if pos > 0 {
                                let _ = input(&mut client, task_id, &chunk[..pos]).await;
                            }
                            return exit::OK; // detach：不 cancel
                        }
                        if let Err(e) = input(&mut client, task_id, &chunk[..n]).await {
                            eprintln!("aeroric-cli attach: input failed: {e}");
                            return e.exit_code();
                        }
                    }
                    Err(_) => return exit::OK,
                }
            }
            _ = poll.tick() => {
                match client
                    .call("task.output", serde_json::json!({ "taskId": task_id, "offset": offset }))
                    .await
                {
                    Ok(response) => {
                        if response["gap"].as_bool().unwrap_or(false) {
                            let reason = response["reason"].as_str().unwrap_or("stream gone");
                            let _ = stdout
                                .write_all(format!("\r\n[aeroric] {reason}\r\n").as_bytes())
                                .await;
                            let _ = stdout.flush().await;
                            return exit::OK;
                        }
                        if let Some(b64) = response["bytes"].as_str() {
                            if !b64.is_empty() {
                                match base64::engine::general_purpose::STANDARD.decode(b64) {
                                    Ok(bytes) => {
                                        let _ = stdout.write_all(&bytes).await;
                                        let _ = stdout.flush().await;
                                        offset = response["nextOffset"].as_u64().unwrap_or(offset);
                                    }
                                    Err(_) => return exit::CONNECT,
                                }
                            } else {
                                offset = response["nextOffset"].as_u64().unwrap_or(offset);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("aeroric-cli attach: output poll failed: {e}");
                        return e.exit_code();
                    }
                }
            }
        }
    }
}

async fn input(
    client: &mut HostClient,
    task_id: &str,
    bytes: &[u8],
) -> Result<(), client::ClientError> {
    // base64 通道：任意按键序列（Alt、NUL、Latin-1）无损到 host 再落 PTY。
    client
        .call(
            "task.input",
            serde_json::json!({
                "taskId": task_id,
                "bytes": base64::engine::general_purpose::STANDARD.encode(bytes),
            }),
        )
        .await
        .map(|_| ())
}

/// SIGWINCH 流（unix）；非 unix 平台永不触发（resize 只剩起跑同步）。
fn winch_stream() -> tokio::sync::mpsc::Receiver<()> {
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sig) = signal(SignalKind::window_change()) {
            tokio::spawn(async move {
                while sig.recv().await.is_some() {
                    if tx.send(()).await.is_err() {
                        break;
                    }
                }
            });
        }
    }
    #[cfg(not(unix))]
    {
        drop(tx);
    }
    rx
}
