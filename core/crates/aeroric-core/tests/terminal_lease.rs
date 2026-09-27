//! P3-03 切片四B 的验收面:**20 次 resize 最终尺寸正确、隐藏终端无写权限、
//! 被顶替的控制者写入必须真的到不了 agent**。
//!
//! 这些都跑真 PTY + 真进程(`tests/fixtures/fake-agent.mjs` 的 `interactive` 模式:
//! 它自己报尺寸、每次 SIGWINCH 再报一次、按行回显 ack)。理由和切片二一样:
//! 许可面是个 *决定*,而决定的证据只能是"对面那个进程真的看到了什么"。

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aeroric_core::terminal::{
    check_write, decide_resize, take_control, InputRejection, PtySizeRequest, ResizeDecision,
    TerminalIdentity, TerminalService, ViewFocus,
};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

const READ_BUFFER: usize = 32 * 1024;
const WAIT_TIMEOUT: Duration = Duration::from_secs(20);

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent.mjs")
}

fn node_program() -> String {
    std::env::var("AERORIC_FAKE_AGENT_NODE").unwrap_or_else(|_| "node".to_string())
}

struct Agent {
    service: Arc<TerminalService>,
    identity: TerminalIdentity,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    text: Arc<Mutex<String>>,
    _child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

impl Agent {
    fn spawn(rows: u16, cols: u16) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open a pty");
        let mut cmd = CommandBuilder::new(node_program());
        cmd.arg(fixture_path());
        cmd.env("FAKE_MODE", "interactive");
        let child = pair
            .slave
            .spawn_command(cmd)
            .expect("spawn interactive agent");
        drop(pair.slave);

        let master = Arc::new(Mutex::new(pair.master));
        let writer = Arc::new(Mutex::new(
            master.lock().take_writer().expect("take writer"),
        ));
        let mut reader = master.lock().try_clone_reader().expect("clone reader");
        let service = Arc::new(TerminalService::new());
        let identity = TerminalIdentity::new("lease-agent", 1);
        let text = Arc::new(Mutex::new(String::new()));

        {
            let service = Arc::clone(&service);
            let identity = identity.clone();
            let text = Arc::clone(&text);
            std::thread::spawn(move || {
                let mut projection = aeroric_core::terminal::Utf8Projection::new();
                let mut buf = vec![0u8; READ_BUFFER];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            service.publish(&identity, &buf[..n]);
                            text.lock().push_str(&projection.push(&buf[..n]));
                        }
                    }
                }
                text.lock().push_str(&projection.finish());
                service.mark_finished(&identity);
            });
        }

        let agent = Agent {
            service,
            identity,
            master,
            writer,
            text,
            _child: Arc::new(Mutex::new(child)),
        };
        agent.wait_for("[fake-agent] size=");
        agent
    }

    fn text(&self) -> String {
        self.text.lock().clone()
    }

    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if self.text().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("等不到 {needle:?},已读到:\n{}", self.text());
    }

    /// 经许可面写入:被拒就当什么都没发生(宿主命令层也是这么处理的)。
    fn gated_write(&self, handle: &aeroric_core::terminal::LeaseHandle, bytes: &[u8]) -> bool {
        use std::io::Write;
        if check_write(&self.service, &self.identity, handle, ViewFocus::Foreground).is_err() {
            return false;
        }
        let mut writer = self.writer.lock();
        writer.write_all(bytes).expect("write");
        writer.flush().expect("flush");
        true
    }

    fn current_size(&self) -> (u16, u16) {
        let size = self.master.lock().get_size().expect("get size");
        (size.rows, size.cols)
    }
}

fn size(rows: u16, cols: u16) -> PtySizeRequest {
    PtySizeRequest { rows, cols }
}

#[test]
fn twenty_resizes_leave_the_last_foreground_size_and_never_a_rejected_one() {
    let agent = Agent::spawn(24, 80);
    let holder = take_control(&agent.service, &agent.identity, ViewFocus::Foreground)
        .expect("foreground holder");

    // 20 次里混进隐藏视图、0×0、畸形,以及合法尺寸;最后一次合法的是 50x200。
    let sequence: Vec<(PtySizeRequest, ViewFocus)> = vec![
        (size(30, 100), ViewFocus::Foreground),
        (size(0, 0), ViewFocus::Foreground),
        (size(24, 80), ViewFocus::Hidden),
        (size(31, 101), ViewFocus::Foreground),
        (size(24, 1), ViewFocus::Foreground),
        (size(0, 80), ViewFocus::Hidden),
        (size(32, 102), ViewFocus::Foreground),
        (size(40, 120), ViewFocus::Foreground),
        (size(10_001, 80), ViewFocus::Foreground),
        (size(41, 121), ViewFocus::Foreground),
        (size(24, 80), ViewFocus::Hidden),
        (size(42, 122), ViewFocus::Foreground),
        (size(43, 123), ViewFocus::Foreground),
        (size(0, 0), ViewFocus::Hidden),
        (size(44, 124), ViewFocus::Foreground),
        (size(1, 1), ViewFocus::Foreground),
        (size(45, 125), ViewFocus::Foreground),
        (size(46, 126), ViewFocus::Foreground),
        (size(47, 127), ViewFocus::Hidden),
        (size(50, 200), ViewFocus::Foreground),
    ];
    assert_eq!(sequence.len(), 20);

    let mut applied = 0usize;
    let mut ignored = 0usize;
    for (request, focus) in &sequence {
        match decide_resize(*request, *focus) {
            ResizeDecision::Apply(accepted) => {
                assert_eq!(&accepted, request);
                check_write(&agent.service, &agent.identity, &holder, *focus)
                    .expect("写入方必须是当前持有者");
                let pty_size = PtySize {
                    rows: accepted.rows,
                    cols: accepted.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                };
                agent.master.lock().resize(pty_size).expect("resize");
                applied += 1;
            }
            ResizeDecision::Ignored(_) => ignored += 1,
        }
    }
    // 表数出来的是 11 应用 / 9 忽略(20 条里 4 条隐藏、2 条 0 边、3 条越界)。
    // 这个断言存在的意义:如果裁决表哪天把某类形状改判,这里先红,而不是让下面
    // 那句"最终尺寸 = 最后一次前台尺寸"悄悄换含义。
    assert_eq!(applied, 11, "应用次数与裁决表不符");
    assert_eq!(ignored, 9);

    // 内核侧:PTY 的实际尺寸 == 最后一次被接受的前台尺寸。
    assert_eq!(agent.current_size(), (50, 200));

    // 进程侧:agent 自己报出来的最后一尺寸也必须一致(它靠 SIGWINCH 收到)。
    agent.wait_for("resized=50x200");
    let text = agent.text();
    let last_report = text
        .lines()
        .rfind(|line| line.contains("resized=") || line.contains("] size="))
        .expect("agent 没报过任何尺寸")
        .to_string();
    assert!(last_report.contains("50x200"), "最后报的是 {last_report}");

    // 被拒的那些形状**一次都没出现过** —— 否则就是"隐藏 view 改到了 PTY"。
    for rejected in ["0x0", "24x1", "1x1", "10001x80", "80x24"] {
        assert!(
            !text.contains(&format!("resized={rejected}")),
            "被拒尺寸 {rejected} 竟然打到了 PTY"
        );
    }
}

#[test]
fn a_superseded_controller_cannot_reach_the_agent_while_the_current_one_can() {
    let agent = Agent::spawn(24, 80);
    let first =
        take_control(&agent.service, &agent.identity, ViewFocus::Foreground).expect("first holder");
    assert!(agent.gated_write(&first, b"one\n"));
    agent.wait_for("[fake-agent] ack=one");

    // 第二个窗口顶替:旧手柄必须既过不了闸门,也真的写不进去。
    let second = take_control(&agent.service, &agent.identity, ViewFocus::Foreground)
        .expect("second holder");
    assert_eq!(
        check_write(
            &agent.service,
            &agent.identity,
            &first,
            ViewFocus::Foreground
        ),
        Err(InputRejection::NotHolder)
    );
    assert!(
        !agent.gated_write(&first, b"ghost\n"),
        "被顶替的手柄竟然过了闸门"
    );

    // 用当前持有者写两次并等到回显,以此证明替身已经把中间的一切都处理完了
    // —— 那时 `ghost` 仍不在输出里,就是"真的没到达进程",不是时序运气。
    assert!(agent.gated_write(&second, b"two\n"));
    agent.wait_for("[fake-agent] ack=two");
    assert!(agent.gated_write(&second, b"probe\n"));
    agent.wait_for("[fake-agent] ack=probe");
    assert!(
        !agent.text().contains("ack=ghost"),
        "被顶替的写入到达了 agent:\n{}",
        agent.text()
    );
}

#[test]
fn a_refused_hidden_attempt_does_not_steal_the_lease() {
    // 这条防的是一个非常具体的错法:先 acquire 再判 focus —— 那样隐藏窗口虽然拿不到
    // 返回值,却已经把前台窗口的控制权顶掉了。
    let agent = Agent::spawn(24, 80);
    let holder = take_control(&agent.service, &agent.identity, ViewFocus::Foreground)
        .expect("foreground holder");
    assert_eq!(
        take_control(&agent.service, &agent.identity, ViewFocus::Hidden),
        Err(InputRejection::HiddenView)
    );
    assert_eq!(
        check_write(
            &agent.service,
            &agent.identity,
            &holder,
            ViewFocus::Foreground
        ),
        Ok(()),
        "一次被拒的取得动作不得动到现有控制权"
    );
    assert!(agent.gated_write(&holder, b"still-mine\n"));
    agent.wait_for("[fake-agent] ack=still-mine");
}

#[test]
fn two_readers_share_one_stream_without_touching_the_lease() {
    // Action 3 的后半句:多个只读订阅者,单一个控制者。两个读者各按自己的游标读,
    // 都不需要租约,也不影响持有者写入。
    use aeroric_core::terminal::{plan_replay, ResumePoint};

    let agent = Agent::spawn(24, 80);
    let holder =
        take_control(&agent.service, &agent.identity, ViewFocus::Foreground).expect("holder");
    assert!(agent.gated_write(&holder, b"shared\n"));
    agent.wait_for("[fake-agent] ack=shared");

    let mut a: Vec<u8> = Vec::new();
    let mut b: Vec<u8> = Vec::new();
    let mut cursor_a = ResumePoint::Bytes(0);
    let mut cursor_b = ResumePoint::Bytes(0);
    for _ in 0..64 {
        for (buffer, cursor) in [(&mut a, &mut cursor_a), (&mut b, &mut cursor_b)] {
            let plan = plan_replay(&agent.service, &agent.identity, *cursor, 7);
            if let aeroric_core::terminal::ReplayPlan::Seamless { bytes, .. } = &plan {
                buffer.extend_from_slice(bytes);
            }
            *cursor = plan.resume_after();
            std::thread::sleep(Duration::from_millis(5));
        }
        if (a.len() as u64) >= agent.service.bounds(&agent.identity).expect("bounds").1
            && (b.len() as u64) >= agent.service.bounds(&agent.identity).expect("bounds").1
        {
            break;
        }
    }
    assert_eq!(a, b, "两个只读订阅者读到的是同一条流");
    assert!(
        String::from_utf8_lossy(&a).contains("ack=shared"),
        "读到的内容里没有回显,说明流与 PTY 断了关系"
    );
    assert_eq!(
        check_write(
            &agent.service,
            &agent.identity,
            &holder,
            ViewFocus::Foreground
        ),
        Ok(()),
        "两次读不该把控制权弄丢"
    );
}
