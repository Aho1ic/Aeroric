//! P3-03 验收第二条:一次运行**只结算一次**,而且结算成什么是 core 的决策面说了算。
//!
//! 五条来源各自成条:正常退出 / 非零退出 / SIGTERM 致死 / EOF / 迟到 hook。
//! 替身 agent(`tests/fixtures/fake-agent.mjs`)在**真 PTY** 里跑真进程,测试这一侧
//! 照宿主的次序走同三段生产守卫:
//!
//! 1. [`TaskRegistry::remove_pty_handles_if_current`] —— 已被取代的代不得参与销账
//!    (ADR-030 决策 3);
//! 2. [`TaskRegistry::settle_marks_for_exit`] —— 还有别的代在飞就不消费标记、不发状态
//!    (ADR-030 决策 2);
//! 3. [`aeroric_core::terminal::settle_exit`] —— 家族差异的唯一一张表(P3-03 切片三)。
//!
//! 与宿主 `finalize_task_exit` 的差别只有一处:`report_task_status` 换成了记账,因为
//! "结算了几次"这件事没法从返回值看出来。这条同形关系由 `terminal-core` 门禁的源码守卫
//! 钉住(宿主那一侧必须恰好一次 `settle_exit` 调用、且不再自己判状态)。
//!
//! 迟到 hook 那条走的是**真的** `TaskService::observe`(ADR-034 的落点),存活与否由
//! 真进程的实际状态回答(ADR-036 的 `LiveRuns` 端口),不是测试预先摆好的一张表。

mod common;

use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aeroric_core::runtime::registry::{ExitSettlement as MarksGate, RunKey, TaskRegistry};
#[cfg(unix)]
use aeroric_core::runtime::subprocess::signal_process_group;
use aeroric_core::tasks::executor::{LiveRuns, ObservationOrigin, TaskEvent};
use aeroric_core::tasks::EventOutcome;
use aeroric_core::terminal::{
    settle_exit, ExitDecision, ExitFacts, ExitReason, SessionEvidence, TerminalIdentity,
    TerminalService, Utf8Projection,
};
use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtySize};

use common::{build_service_with_liveness, harness, task};

type ChildHandle = Arc<Mutex<Box<dyn Child + Send + Sync>>>;
type WriterHandle = Arc<Mutex<Box<dyn std::io::Write + Send>>>;

const READ_BUFFER: usize = 32 * 1024;
const WAIT_TIMEOUT: Duration = Duration::from_secs(20);

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent.mjs")
}

fn node_program() -> String {
    std::env::var("AERORIC_FAKE_AGENT_NODE").unwrap_or_else(|_| "node".to_string())
}

// ── 记账:结算发生了几次、每次结出什么 ────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
struct Settlement {
    generation: u64,
    decision: ExitDecision,
}

#[derive(Default)]
struct Ledger {
    settled: Mutex<Vec<Settlement>>,
}

impl Ledger {
    fn record(&self, run: &RunKey, decision: ExitDecision) {
        self.settled.lock().push(Settlement {
            generation: run.generation(),
            decision,
        });
    }

    /// 结算次数。**不设去重**——两次结算就是两条,否则这个计数器本身就把缺陷吞掉了。
    fn settles(&self) -> Vec<Settlement> {
        self.settled.lock().clone()
    }

    /// 会发到用户面前的状态词。取消 / 手动完成不该出现在这里。
    fn statuses(&self) -> Vec<TaskStatus> {
        self.settles()
            .iter()
            .filter_map(|entry| entry.decision.task_status())
            .collect()
    }

    fn reason_of(&self, index: usize) -> Option<String> {
        match &self.settles()[index].decision {
            ExitDecision::Failed { reason } => Some(reason.host_sentence()),
            _ => None,
        }
    }
}

// ── 真进程 ─────────────────────────────────────────────────────────────

/// ADR-036 的端口:存活与否由**真进程**回答。测试在进程退出那一刻把它翻下去,
/// 于是 ADR-034 的守卫不是被一张预先摆好的表喂出来的。
#[derive(Default)]
struct ProcessLiveness {
    live: AtomicBool,
}

impl ProcessLiveness {
    fn set(&self, live: bool) {
        self.live.store(live, Ordering::SeqCst);
    }
}

impl LiveRuns for ProcessLiveness {
    fn has_live_run(&self, _task_id: &str) -> bool {
        self.live.load(Ordering::SeqCst)
    }
}

/// 宿主那一侧的同形物:注册表 + 字节流 + 记账。
struct Harness {
    task_id: String,
    registry: Arc<TaskRegistry>,
    terminal: Arc<TerminalService>,
    ledger: Arc<Ledger>,
}

impl Harness {
    fn new(tag: &str) -> Self {
        Self {
            task_id: format!("exit-{tag}"),
            registry: Arc::new(TaskRegistry::default()),
            terminal: Arc::new(TerminalService::new()),
            ledger: Arc::new(Ledger::default()),
        }
    }

    fn spawn(&self, generation: u64, mode: &str, env: &[(&str, &str)]) -> Agent {
        let fixture = fixture_path();
        assert!(fixture.is_file(), "缺少替身 agent: {fixture:?}");

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open a pty");

        let mut cmd = CommandBuilder::new(node_program());
        cmd.arg(&fixture);
        cmd.env("FAKE_MODE", mode);
        for (key, value) in env {
            cmd.env(key, value);
        }
        let child: ChildHandle = {
            let spawned = pair
                .slave
                .spawn_command(cmd)
                .unwrap_or_else(|error| panic!("启动 {mode} 替身失败: {error}"));
            Arc::new(Mutex::new(spawned))
        };
        // 我们这一侧的 slave 副本必须关掉,否则 master 永远不会读到 EOF。
        drop(pair.slave);

        let pid = child.lock().process_id().expect("the fake agent has a pid");
        let run = RunKey::minted(&self.task_id, generation);
        let identity = TerminalIdentity::new(self.task_id.clone(), generation);

        // 与 core 的 TaskRegistry::register_pty_session 同形:句柄进表、代进 runs(ADR-025 靠它寻址)。
        self.registry
            .child_handles
            .lock()
            .insert(run.clone(), Arc::clone(&child));
        // `take_writer` 而不是 clone:一个 PTY 只有一个写侧,这正是控制租约那条规则的形状。
        let master_box = pair.master;
        let writer: WriterHandle = Arc::new(Mutex::new(
            master_box.take_writer().expect("take the master writer"),
        ));
        self.registry
            .pty_writers
            .lock()
            .insert(run.clone(), Arc::clone(&writer));
        let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(master_box));
        self.registry
            .pty_masters
            .lock()
            .insert(run.clone(), Arc::clone(&master));
        self.registry.runs.register(&self.task_id, generation);

        let text = Arc::new(Mutex::new(String::new()));
        let eof_seen = Arc::new(AtomicBool::new(false));
        let live = Arc::new(AtomicBool::new(true));
        let terminal = Arc::clone(&self.terminal);
        let reader = {
            let mut reader = master.lock().try_clone_reader().expect("clone reader");
            let text = Arc::clone(&text);
            let eof_seen = Arc::clone(&eof_seen);
            let live = Arc::clone(&live);
            let identity_for_thread = identity.clone();
            std::thread::spawn(move || {
                let mut projection = Utf8Projection::new();
                let mut buf = vec![0u8; READ_BUFFER];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            // 与宿主同一条次序:原始 bytes 先落流,字符串只是投影。
                            terminal.publish(&identity_for_thread, &buf[..n]);
                            text.lock().push_str(&projection.push(&buf[..n]));
                        }
                    }
                }
                text.lock().push_str(&projection.finish());
                terminal.mark_finished(&identity_for_thread);
                eof_seen.store(true, Ordering::SeqCst);
                live.store(false, Ordering::SeqCst);
            })
        };

        Agent {
            run,
            identity,
            terminal: Arc::clone(&self.terminal),
            child,
            writer,
            pid,
            text,
            eof_seen,
            live,
            reader: Some(reader),
        }
    }

    /// 宿主 `finalize_task_exit` 的同形移植:同样三段守卫,同样把决定交给 core。
    fn finalize(
        &self,
        agent: &Agent,
        family: ProtocolFamily,
        evidence: SessionEvidence,
        status: &ExitStatus,
    ) -> Finalized {
        if !self
            .registry
            .remove_pty_handles_if_current(&agent.run, &agent.child)
        {
            return Finalized::SkippedSupersededRun;
        }
        let (cancelled, manually_completed) =
            match self.registry.settle_marks_for_exit(&self.task_id) {
                MarksGate::WaitingForOtherRuns => return Finalized::SkippedAnotherRunLive,
                MarksGate::Settle {
                    cancelled,
                    manually_completed,
                } => (cancelled, manually_completed),
            };
        let exit_ok = status.success();
        let decision = settle_exit(&ExitFacts {
            family,
            exit_ok,
            exit_code: if exit_ok {
                None
            } else {
                Some(status.exit_code())
            },
            signal: status.signal().map(str::to_string),
            session: evidence,
            cancelled,
            manually_completed,
        });
        // Q1=B 之后 `ExitDecision` 不再 `Copy`(它可能持有信号名),记账走克隆,返回值仍是原件。
        self.ledger.record(&agent.run, decision.clone());
        Finalized::Settled(decision)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Finalized {
    /// 守卫 1:这一代的句柄已经不属于它了 —— 迟到的监视器。
    SkippedSupersededRun,
    /// 守卫 2:还有别的代在飞。
    SkippedAnotherRunLive,
    Settled(ExitDecision),
}

struct Agent {
    run: RunKey,
    identity: TerminalIdentity,
    terminal: Arc<TerminalService>,
    child: ChildHandle,
    writer: WriterHandle,
    pid: u32,
    text: Arc<Mutex<String>>,
    eof_seen: Arc<AtomicBool>,
    live: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Agent {
    fn text(&self) -> String {
        self.text.lock().clone()
    }

    fn live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }

    fn write_input(&self, bytes: &[u8]) {
        use std::io::Write;
        let mut writer = self.writer.lock();
        writer.write_all(bytes).expect("write to the pty master");
        writer.flush().expect("flush the pty master");
    }

    fn wait_until(&self, what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "等不到 {what}。\n进程状态 live={}\n已读到:\n{}",
            self.live(),
            self.text()
        );
    }

    fn wait_for_text(&self, needle: &str) {
        self.wait_until(&format!("输出里出现 {needle:?}"), || {
            self.text().contains(needle)
        });
    }

    fn wait_eof(&self) {
        self.wait_until("reader 读到 EOF", || self.eof_seen.load(Ordering::SeqCst));
    }

    /// 等到进程真的结束。**必须 wait**:不 wait 的话它是个僵尸,还留在自己的进程组里,
    /// 后面那句"整个组已经没了"就无从谈起。
    fn wait_exit(&self) -> ExitStatus {
        let status = self.child.lock().wait().expect("wait for the fake agent");
        self.live.store(false, Ordering::SeqCst);
        status
    }

    fn join_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }

    fn stream_bytes(&self) -> Vec<u8> {
        // 从 0 起读:若已被压缩掉就是 Gap(本文件的量不会触发)。
        self.terminal
            .read_from(&self.identity, 0, usize::MAX)
            .expect("read the stream")
            .bytes
    }
}

// ── 五条来源 ───────────────────────────────────────────────────────────

#[test]
fn a_clean_exit_of_a_real_process_settles_done_exactly_once() {
    let h = Harness::new("clean");
    let mut agent = h.spawn(1, "exit", &[("FAKE_EXIT_CODE", "0")]);
    agent.wait_for_text("[fake-agent] exiting code=0");
    agent.wait_eof();
    let status = agent.wait_exit();
    agent.join_reader();

    let decision = h.finalize(
        &agent,
        ProtocolFamily::Codex,
        SessionEvidence::Absent,
        &status,
    );
    assert_eq!(decision, Finalized::Settled(ExitDecision::Done));
    assert_eq!(h.ledger.statuses(), vec![TaskStatus::Done]);

    // 同一次退出的第二条通知:守卫 1 拦下,不产生第二次结算。
    let duplicate = h.finalize(
        &agent,
        ProtocolFamily::Codex,
        SessionEvidence::Absent,
        &status,
    );
    assert_eq!(duplicate, Finalized::SkippedSupersededRun);
    assert_eq!(
        h.ledger.settles().len(),
        1,
        "重复的退出通知必须被吞掉,记账只能有一条"
    );
    // 流也到了 core 这一侧:原始字节里能看到替身那行标记。
    assert!(String::from_utf8_lossy(&agent.stream_bytes()).contains("exiting code=0"));
}

#[test]
fn a_nonzero_exit_settles_failed_once_and_quotes_the_code_the_process_actually_returned() {
    let h = Harness::new("nonzero");
    let mut agent = h.spawn(1, "exit", &[("FAKE_EXIT_CODE", "3")]);
    agent.wait_for_text("[fake-agent] exiting code=3");
    agent.wait_eof();
    let status = agent.wait_exit();
    agent.join_reader();

    assert!(!status.success());
    assert_eq!(status.exit_code(), 3);
    let decision = h.finalize(
        &agent,
        ProtocolFamily::Codex,
        SessionEvidence::Absent,
        &status,
    );
    assert_eq!(
        decision,
        Finalized::Settled(ExitDecision::Failed {
            reason: ExitReason::Code(3)
        })
    );
    assert_eq!(h.ledger.settles().len(), 1);
    assert_eq!(
        h.ledger.reason_of(0).as_deref(),
        Some("Process exited with code 3"),
        "用户可见的文案保持宿主原样"
    );
}

#[test]
fn a_sigterm_death_settles_once_as_failed_and_the_signal_is_flattened_to_a_code() {
    let h = Harness::new("sigterm");
    let mut agent = h.spawn(1, "signal", &[("FAKE_SIGNAL", "SIGTERM")]);
    agent.wait_for_text("[fake-agent] signalling self SIGTERM");
    agent.wait_eof();
    let status = agent.wait_exit();
    agent.join_reader();

    // 替身没有走到自己的 process.exit:它死于信号,这是"另一种来源"的前提。
    assert!(
        !agent.text().contains("[fake-agent] exiting code="),
        "替身自己退出了就不是信号致死:\n{}",
        agent.text()
    );
    assert!(!status.success());
    // 实测口径(与直觉相反,所以写下依据):portable-pty 把"死于信号"折成
    // `code = 1` —— `src-tauri/vendor/portable-pty/src/lib.rs:197-223` 里
    // `status.code()` 对信号死亡是 None,于是 `unwrap_or(1)`。
    // Q1 裁决(B,待追认)之前信号只活在 `Display` 里,core 的决策面只看得到一个 1;
    // 现在 vendor 有了公开访问器,**事实进 core,折平的码与用户可见文案一字未动**。
    assert_eq!(
        status.exit_code(),
        1,
        "portable-pty 的信号致死码口径变了(实测 {}),要重查整条 reason 文案",
        status.exit_code()
    );
    // 信号名在 macOS 上经 `strsignal` 是 "Terminated: 15":Display 记得它,访问器也记得它。
    assert!(
        format!("{status}").contains("Terminated: 15"),
        "Display 里连信号编号都没了,说明依赖换了形状:实际 {status}"
    );
    assert_eq!(
        status.signal(),
        Some("Terminated: 15"),
        "vendor 的信号访问器不再报出平台原名,Q1=B 的事实面就断了"
    );
    assert_eq!(
        h.finalize(
            &agent,
            ProtocolFamily::Omp,
            SessionEvidence::Absent,
            &status
        ),
        Finalized::Settled(ExitDecision::Failed {
            reason: ExitReason::TerminatedBySignal {
                name: "Terminated: 15".to_string(),
                legacy_code: Some(1),
            }
        })
    );
    assert_eq!(h.ledger.settles().len(), 1, "信号致死同样只结算一次");
    assert_eq!(
        h.ledger.reason_of(0).as_deref(),
        Some("Process exited with code 1"),
        "用户今天看到的仍是这句:信号被抹平成 code 1。Q1=B 只把事实交给 core,不改旧文案;如实渲染归原生客户端(P4/P5)。"
    );
}

#[cfg(unix)]
#[test]
fn the_exit_settles_once_even_while_the_reader_has_not_seen_eof_yet() {
    // 组长先退、后代继承这个 tty 继续活着 —— EOF 被推到很久之后。这一条钉住
    // "结算只由退出监控产生:不等 EOF,EOF 之后也不会再来一次"。
    let h = Harness::new("orphan");
    let mut agent = h.spawn(
        1,
        "orphan",
        &[("FAKE_LIVE_MS", "150"), ("FAKE_EXIT_CODE", "0")],
    );
    agent.wait_for_text("[fake-agent] leader=");
    let holder = agent
        .text()
        .split_whitespace()
        .find_map(|token| token.strip_prefix("holder="))
        .and_then(|raw| raw.parse::<u32>().ok())
        .expect("替身报出了后代 pid");

    let status = agent.wait_exit();
    assert_eq!(
        h.finalize(
            &agent,
            ProtocolFamily::Codex,
            SessionEvidence::Absent,
            &status
        ),
        Finalized::Settled(ExitDecision::Done),
        "进程一退出就该结算,不等 reader 收尾"
    );
    assert_eq!(h.ledger.settles().len(), 1);
    // 句柄表已随结算清空,但流必须还在册:重连的订阅者还要靠这段窗口追平。
    assert!(
        h.terminal.bounds(&agent.identity).is_some(),
        "reader 还没收尾时不得把流摘掉"
    );
    assert!(String::from_utf8_lossy(&agent.stream_bytes()).contains("leader="));

    // 把继承 tty 的后代连同整组带走,reader 就此看到 EOF。
    assert!(
        signal_process_group(holder, 9),
        "SIGKILL 必须打到持有 tty 的后代"
    );
    agent.wait_eof();
    agent.join_reader();
    assert_eq!(
        h.ledger.settles().len(),
        1,
        "EOF 不是第二个结算来源:整条链上只能有退出那一次"
    );
    assert_eq!(h.ledger.statuses(), vec![TaskStatus::Done]);
}

#[test]
fn a_late_hook_report_after_a_real_exit_cannot_resurrect_the_task() {
    let (root, context, executor) = harness("late-hook", &[task("t1", "p1", "running")]);
    let liveness = Arc::new(ProcessLiveness::default());
    let service = build_service_with_liveness(context, &executor, liveness.clone());

    let h = Harness::new("hook");
    let mut agent = h.spawn(1, "exit", &[("FAKE_EXIT_CODE", "0")]);
    liveness.set(true);
    agent.wait_for_text("[fake-agent] exiting code=0");
    agent.wait_eof();
    let status = agent.wait_exit();
    agent.join_reader();

    // 反空转对照:进程还活着的时候,同一种来源的报告必须被采纳 —— 否则"丢弃"这条
    // 规则可以把一切都丢掉而仍然全绿。
    let (outcome, _) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 0,
                status: TaskStatus::InputRequired,
                attention_requested_at_ms: Some(1_700_000_000_001),
                failure_reason: None,
                origin: ObservationOrigin::Hook,
            },
        )
        .expect("live hook applied");
    assert!(
        matches!(outcome, EventOutcome::Applied { changed: true, .. }),
        "存活时的 hook 报告必须被采纳,实际 {outcome:?}"
    );

    // 真进程的退出结算。
    assert_eq!(
        h.finalize(
            &agent,
            ProtocolFamily::Codex,
            SessionEvidence::Absent,
            &status
        ),
        Finalized::Settled(ExitDecision::Done)
    );
    assert_eq!(h.ledger.settles().len(), 1);

    // 进程没了(ADR-036 的端口如实回答),迟到的 hook 报告不得再点亮这张卡。
    liveness.set(false);
    let (outcome, _) = service
        .observe(
            "p1",
            TaskEvent::Status {
                task_id: "t1".into(),
                generation: 0,
                status: TaskStatus::Running,
                attention_requested_at_ms: None,
                failure_reason: None,
                origin: ObservationOrigin::Hook,
            },
        )
        .expect("late hook observed");
    assert_eq!(
        outcome,
        EventOutcome::Dropped("no live run for this task"),
        "ADR-034:迟到的观察不得改状态"
    );
    assert_eq!(h.ledger.settles().len(), 1, "迟到 hook 不得再结算一次");
    assert_eq!(h.ledger.statuses(), vec![TaskStatus::Done]);

    common::cleanup(root);
}

#[test]
fn overlapping_generations_settle_only_when_the_last_one_exits() {
    let h = Harness::new("overlap");
    let mut first = h.spawn(1, "exit", &[("FAKE_EXIT_CODE", "0")]);
    let mut second = h.spawn(2, "exit", &[("FAKE_EXIT_CODE", "0")]);

    for agent in [&mut first, &mut second] {
        agent.wait_for_text("[fake-agent] exiting code=0");
        agent.wait_eof();
    }
    let first_status = first.wait_exit();
    first.join_reader();
    let second_status = second.wait_exit();
    second.join_reader();

    // 先退出那代:还有别的代在飞 → 不消费、不发状态、不记结算。
    assert_eq!(
        h.finalize(
            &first,
            ProtocolFamily::Codex,
            SessionEvidence::Absent,
            &first_status
        ),
        Finalized::SkippedAnotherRunLive
    );
    assert!(h.ledger.settles().is_empty());
    assert!(
        h.registry.runs.is_live(&h.task_id, 2),
        "另一代仍登记在案,这正是它不结算的理由"
    );

    // 最后退出那代:一次,且是它给出结论。
    assert_eq!(
        h.finalize(
            &second,
            ProtocolFamily::Codex,
            SessionEvidence::Absent,
            &second_status
        ),
        Finalized::Settled(ExitDecision::Done)
    );
    assert_eq!(h.ledger.settles().len(), 1);
    assert_eq!(h.ledger.settles()[0].generation, 2);
    assert!(!h.registry.has_live_run(&h.task_id));
}

#[cfg(unix)]
#[test]
fn cancelling_kills_the_whole_process_group_and_that_exit_settles_no_status() {
    let h = Harness::new("cancel");
    let mut agent = h.spawn(1, "child", &[]);
    agent.wait_for_text("[fake-agent] self=");
    // 替身自己是组长(forkpty 会新建会话),所以 kill(-pid) 只打这一支,不打测试进程。
    assert!(
        signal_process_group(agent.pid, 0),
        "替身必须自成进程组,否则 kill(-pgid) 会打到别人身上"
    );

    h.registry.cancelled_tasks.lock().insert(h.task_id.clone());
    assert!(
        signal_process_group(agent.pid, 15),
        "SIGTERM 打给整个进程组必须成功"
    );

    agent.wait_eof();
    let status = agent.wait_exit();
    agent.join_reader();

    // **先 reap 再查组**:组长没被 reap 时它仍是僵尸、仍在组里,那时"组还在"是假红。
    // 反过来,只要组还在就说明有活口 —— 后代是这条用例真正要带走的那个进程。
    let gone = Instant::now() + WAIT_TIMEOUT;
    while Instant::now() < gone {
        if !signal_process_group(agent.pid, 0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !signal_process_group(agent.pid, 0),
        "进程组里还有活口:后代没被一起带走(pgid={})",
        agent.pid
    );

    assert_eq!(
        h.finalize(
            &agent,
            ProtocolFamily::Claude,
            SessionEvidence::Absent,
            &status
        ),
        Finalized::Settled(ExitDecision::NoStatus),
        "取消过的运行不得由退出改状态"
    );
    assert_eq!(h.ledger.settles().len(), 1, "取消路径同样只结算一次");
    assert!(
        h.ledger.statuses().is_empty(),
        "取消不给状态词,实际 {:?}",
        h.ledger.statuses()
    );
    assert!(
        h.registry.cancelled_tasks.lock().is_empty(),
        "标记必须被消费掉,否则下一次运行会凭空被算成取消"
    );
}

#[test]
fn the_agent_reports_its_pty_size_and_answers_input_on_the_current_generation() {
    // §4-4 要用的两项能力(报告尺寸、等待输入)在这里先立住,不留没人跑的后门。
    let h = Harness::new("interactive");
    let mut agent = h.spawn(1, "interactive", &[]);
    agent.wait_for_text("[fake-agent] size=24x80");
    assert_eq!(
        h.registry
            .resource_run(&h.task_id)
            .expect("a registered run")
            .generation(),
        1,
        "资源型操作必须打到已注册的最大代"
    );

    let writer = h
        .registry
        .writer_for(&h.task_id)
        .expect("writer for the run");
    {
        use std::io::Write;
        writer.lock().write_all(b"hello\n").expect("write input");
        writer.lock().flush().expect("flush input");
    }
    agent.wait_for_text("[fake-agent] ack=hello");

    let before = agent.stream_bytes().len() as u64;
    agent.write_input(b"quit\n");
    agent.wait_eof();
    let status = agent.wait_exit();
    agent.join_reader();
    assert!(
        agent.stream_bytes().len() as u64 > before,
        "写进去的东西换来了回显,流里的字节必须增加"
    );
    assert_eq!(
        h.finalize(
            &agent,
            ProtocolFamily::Codex,
            SessionEvidence::Absent,
            &status
        ),
        Finalized::Settled(ExitDecision::Done)
    );
    assert_eq!(h.ledger.settles().len(), 1);
}

#[test]
fn the_family_difference_survives_a_real_nonzero_exit() {
    // 验收点名的反例:不许把"有 session 就 done"统一套到所有家族上。
    // 同一份真事实(替身以 137 退出 + 这一代确实留下了会话条目),四个家族两种结论。
    let h = Harness::new("family");
    let mut agent = h.spawn(1, "exit", &[("FAKE_EXIT_CODE", "137")]);
    agent.wait_for_text("[fake-agent] exiting code=137");
    agent.wait_eof();
    let status = agent.wait_exit();
    agent.join_reader();

    let mut outcomes = Vec::new();
    for family in [
        ProtocolFamily::Claude,
        ProtocolFamily::Codex,
        ProtocolFamily::Dsh,
        ProtocolFamily::Omp,
    ] {
        // 每一族都从"这一代留下过真实会话条目"这个同一事实出发。
        outcomes.push((
            family,
            settle_exit(&ExitFacts {
                family,
                exit_ok: status.success(),
                exit_code: Some(status.exit_code()),
                signal: status.signal().map(str::to_string),
                session: SessionEvidence::Real,
                cancelled: false,
                manually_completed: false,
            }),
        ));
    }
    let done_or_failed = |family: ProtocolFamily| {
        outcomes
            .iter()
            .find(|(item, _)| *item == family)
            .and_then(|(_, decision)| decision.task_status())
    };
    assert_eq!(
        done_or_failed(ProtocolFamily::Claude),
        Some(TaskStatus::Done)
    );
    assert_eq!(
        done_or_failed(ProtocolFamily::Codex),
        Some(TaskStatus::Done)
    );
    assert_eq!(
        done_or_failed(ProtocolFamily::Dsh),
        Some(TaskStatus::Failed)
    );
    assert_eq!(
        done_or_failed(ProtocolFamily::Omp),
        Some(TaskStatus::Failed)
    );
    assert_eq!(h.ledger.settles().len(), 0, "本用例只比对决策表,不跑结算链");
}

#[test]
fn the_fake_agent_can_actually_be_asked_for_a_specific_exit_code() {
    // 替身跑不起来就是环境缺陷,必须报出来,而不是让上面那些用例以别的方式"通过"。
    let fixture = fixture_path();
    let output = std::process::Command::new(node_program())
        .arg(&fixture)
        .env("FAKE_MODE", "exit")
        .env("FAKE_EXIT_CODE", "41")
        .output()
        .expect("node 必须在 PATH 上(P3-03 的终端替身 agent 需要它)");
    assert_eq!(output.status.code(), Some(41));
    assert!(String::from_utf8_lossy(&output.stdout).contains("exiting code=41"));
}
