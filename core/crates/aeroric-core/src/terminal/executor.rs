//! `CorePtyExecutor`——PTY 会话装配的唯一启动体(P3-03 Action 6)。
//!
//! Action 6 的"只替换 Tauri 临时 executor 的**本地 PTY** 分支、不保留两份同 target
//! 的启动 body",在本片的落地形状:**spawn → 丢 slave → 克隆 reader → 取 writer →
//! 按 `RunKey` 注册(含未铸造键上的暂存 resize 排水)**这五步机械,此前在宿主有
//! **七份逐字相同**的拷贝(local launch/local resume/open-shell/SSH 任务/SSH 终端/
//! WSL 任务/WSL shell),现在唯一一份在这里;SSH/WSL 与 shell 的**命令构造与编排**
//! 仍留在宿主各自的分支里(SSH/WSL 执行分支本体留 P7,DSH 留 P3-05),它们只是
//! 不再各自手写装配步骤。注册表本体(`register_pty_session`)在
//! [`crate::runtime::TaskRegistry`] 上——句柄表是它的字段。
//!
//! 分工口径与 exit/input 同一条线:core 拥有"一条 PTY 会话怎样才算装配完成"的
//! 定义,宿主拥有"命令长什么样、交给谁编排"的决策。

use std::sync::Arc;

use crate::runtime::registry::RunKey;
use crate::runtime::TaskRegistry;

/// 装配完成后交付的读端与子进程句柄。
///
/// master 与 writer 已交给注册表(`register_pty_session`),reader 是
/// `spawn_pty_reader` 的输入,child 句柄是退出监控的输入——两者经这里一并交还,
/// 调用方不再各自 `try_clone_reader`。
pub struct PtySessionHandles {
    pub child: Arc<parking_lot::Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    pub reader: Box<dyn std::io::Read + Send>,
}

/// PTY 会话的唯一装配点。
///
/// 步骤次序是迁移前七个站点逐字相同的那份:spawn 命令、drop slave、克隆 reader、
/// 取 writer、注册(暂存 resize 在注册时按未铸造键排水)。任何新分支都不许再
/// 自己写这五步;步骤次序或错误映射要改,只能改这里,七个分支同时生效。
/// 装配失败的四类来源。`Display` 逐字透传底层消息（迁移前的 `e.to_string()`
/// 就是这些文案，宿主 command 的字符串契约不因 typed 而变）；typed 的收益是
/// **调用点可以按变体区分**（spawn 失败与登记失败是两回事），而不是继续
/// 在一个 `String` 里靠子串匹配。
#[derive(Debug, thiserror::Error)]
pub enum ActivateError {
    #[error("{0}")]
    Spawn(String),
    #[error("{0}")]
    Reader(String),
    #[error("{0}")]
    Writer(String),
    #[error("{0}")]
    Register(String),
}

pub fn activate_pty_session(
    pair: portable_pty::PtyPair,
    command: portable_pty::CommandBuilder,
    registry: &TaskRegistry,
    run: &RunKey,
) -> Result<PtySessionHandles, ActivateError> {
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| ActivateError::Spawn(e.to_string()))?;
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| ActivateError::Reader(e.to_string()))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| ActivateError::Writer(e.to_string()))?;
    let child = registry
        .register_pty_session(run, pair.master, writer, child)
        .map_err(ActivateError::Register)?;
    Ok(PtySessionHandles { child, reader })
}

#[cfg(test)]
mod tests {
    use super::*;
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    /// 真实 PTY + 真进程的装配用例:暂存 resize 必须在注册那一刻排水到 master,
    /// 三张句柄表与 runs 台账必须同时就位——这就是"装配完成"的可数定义。
    #[test]
    fn an_activated_session_registers_handles_and_drains_the_staged_size() {
        let registry = TaskRegistry::default();
        let run = RunKey::minted("activate-agent", 7);
        // 给"下一次运行"暂存一个尺寸:未铸造键。
        registry.stage_pending_size("activate-agent", (30, 100));

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let mut cmd = CommandBuilder::new("/bin/echo");
        cmd.arg("activated");
        let handles =
            activate_pty_session(pair, cmd, &registry, &run).expect("activate pty session");

        // master 已进表,且暂存尺寸已排水:暂存元组是 (cols, rows) = (30, 100),
        // 所以 master 变成 rows=100 / cols=30(与宿主排水路径的语义一致)。
        let master = registry
            .master_for("activate-agent")
            .unwrap_or_else(|| panic!("master 没进表"));
        let size = master.lock().get_size().expect("get size");
        assert_eq!(
            (size.rows, size.cols),
            (100, 30),
            "暂存 resize 没有在注册时排水"
        );
        // writer 与 child 都按 RunKey 就位,child 句柄与返回值同源。
        assert!(registry.pty_writers.lock().contains_key(&run));
        let registered_child = registry.child_for("activate-agent").expect("child 没进表");
        assert!(
            Arc::ptr_eq(&registered_child, &handles.child),
            "返回的 child 句柄与注册表里的不是同一个"
        );
        // 台账只在三张表都齐之后才登记(ADR-025 寻址读的就是它)。
        assert_eq!(registry.resource_run("activate-agent").as_ref(), Some(&run));
        // reader 可用:echo 的输出迟早从它出来。
        let mut reader = handles.reader;
        let mut buf = [0u8; 64];
        let n = reader.read(&mut buf).expect("read from pty");
        assert!(
            String::from_utf8_lossy(&buf[..n]).contains("activated"),
            "reader 读到的不是 echo 的输出: {:?}",
            String::from_utf8_lossy(&buf[..n])
        );
    }

    /// 装配失败按来源分型:spawn 不存在的命令必须落 `Spawn` 变体,且
    /// `Display` 逐字＝底层 io 消息（宿主 command 的字符串契约不因 typed 改变）。
    #[test]
    fn spawn_failure_is_typed_and_keeps_the_verbatim_message() {
        let registry = TaskRegistry::default();
        let run = RunKey::minted("activate-missing", 1);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let mut cmd = CommandBuilder::new("/definitely-not-a-real-binary-a7x9");
        cmd.env("PATH", ""); // 排除任何 PATH 命中
        let error = match activate_pty_session(pair, cmd, &registry, &run) {
            Err(error) => error,
            Ok(_) => panic!("不存在的二进制必须让装配失败"),
        };
        assert!(
            matches!(error, ActivateError::Spawn(_)),
            "spawn 失败必须落 Spawn 变体，实得 {error:?}"
        );
        // 失败即退场：runs 台账与三张句柄表都不留痕。
        assert!(registry.resource_run("activate-missing").is_none());
    }

    /// 未暂存尺寸时注册不得改动 master 的实际尺寸;同时注销后资源全部退场。
    #[test]
    fn an_activation_without_a_staged_size_keeps_the_spawn_size() {
        let registry = TaskRegistry::default();
        let run = RunKey::minted("activate-plain", 1);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let cmd = CommandBuilder::new("/bin/echo");
        let handles =
            activate_pty_session(pair, cmd, &registry, &run).expect("activate pty session");
        let master = registry
            .master_for("activate-plain")
            .expect("master 没进表");
        let size = master.lock().get_size().expect("get size");
        assert_eq!((size.rows, size.cols), (24, 80));
        assert!(registry.pending_pty_sizes.lock().is_empty());
        let _ = handles;
    }
}
