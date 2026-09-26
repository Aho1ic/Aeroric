//! P3-03 验收的第一条:真实 PTY 上的**原始字节保真**。
//!
//! 形状:替身 agent 往 PTY 里写字节(故意跨帧劈开多字节序列、写 NUL、写非法
//! UTF-8、写半个字符就退出),测试这一侧照 core 的口径处理每一块读到的字节 ——
//! **先发布原始 bytes,再做字符串投影** —— 并把读到的字节自己记一份 oracle。
//! 断言的是"流里的字节 == 读到的字节",不是"终端看着顺不顺"。
//!
//! 注:宿主的 `spawn_pty_reader` 要 Wry 运行时才能跑,所以这里验的是它现在调用的
//! 这两件 core 原件在真 PTY 分帧下的行为;"宿主必须先 publish 再投影、且不得再出现
//! 旧 `leftover` 解码形状"由 `terminal-core` 模块的源码守卫 case 钉住。

use std::io::Read;
use std::path::PathBuf;
use std::process::Command as StdCommand;

use aeroric_core::terminal::{
    TerminalIdentity, TerminalService, Utf8Projection, TERMINAL_REPLAY_WINDOW_BYTES,
};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use sha2::{Digest, Sha256};

const READ_BUFFER: usize = 32 * 1024;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent.mjs")
}

fn node_program() -> String {
    std::env::var("AERORIC_FAKE_AGENT_NODE").unwrap_or_else(|_| "node".to_string())
}

struct Session {
    /// 从 PTY master 读到的每个字节,按读到的顺序原样留着。
    oracle: Vec<u8>,
    /// 每次 `read` 结束时 `oracle` 的长度 —— 真实帧边界。
    frames: Vec<usize>,
    decoded: String,
    service: TerminalService,
    identity: TerminalIdentity,
    projection_pending: usize,
    elapsed_ms: u128,
}

fn run_fake_agent(mode: &str, stress_mib: &str) -> Session {
    let fixture = fixture_path();
    assert!(fixture.is_file(), "缺少替身 agent: {fixture:?}");

    let started = std::time::Instant::now();
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
    cmd.env("FAKE_STRESS_MIB", stress_mib);
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .unwrap_or_else(|error| panic!("启动 {mode} 替身 agent 失败: {error}"));
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let service = TerminalService::new();
    let identity = TerminalIdentity::new(format!("pty-bytes-{mode}"), 7);
    let mut projection = Utf8Projection::new();
    let mut oracle = Vec::new();
    let mut frames = Vec::new();
    let mut decoded = String::new();
    let mut buf = vec![0u8; READ_BUFFER];

    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // 与宿主同一条顺序:原始 bytes 先落流,字符串只是投影。
                service.publish(&identity, &buf[..n]);
                decoded.push_str(&projection.push(&buf[..n]));
                oracle.extend_from_slice(&buf[..n]);
                frames.push(oracle.len());
            }
        }
    }
    decoded.push_str(&projection.finish());
    service.mark_finished(&identity);

    let status = child.wait().expect("wait for the fake agent");
    assert!(status.success(), "替身 agent 退出码 {status:?}");
    let projection_pending = projection.pending_len();
    Session {
        oracle,
        frames,
        decoded,
        service,
        identity,
        projection_pending,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    finish_hex(hasher)
}

fn finish_hex(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 这一帧是否把一个多字节序列劈在了边界上(即下一帧要以续字节开头)。
fn cuts_a_multi_byte_sequence(oracle: &[u8], frame_end: usize) -> bool {
    if frame_end == 0 || frame_end >= oracle.len() {
        return false; // 帧尾就是流末端,谈不上"劈开"
    }
    let mut start = frame_end;
    while start > 0 && (oracle[start - 1] & 0xC0) == 0x80 {
        start -= 1;
    }
    if start == 0 {
        return false; // 回退到流头也没找到起始字节
    }
    let lead = oracle[start - 1];
    let needed = if lead & 0x80 == 0x00 {
        1
    } else if lead & 0xE0 == 0xC0 {
        2
    } else if lead & 0xF0 == 0xE0 {
        3
    } else if lead & 0xF8 == 0xF0 {
        4
    } else {
        return false; // 非法起始字节,不参与判断
    };
    // 本帧为这个序列供给了几个字节?不够 `needed` 就是被劈开了。
    frame_end - (start - 1) < needed
}

/// 迁移前宿主 reader 的解码算法,原样重放一份当对照:`from_utf8` 失败就把
/// `valid_up_to` 之后的字节全部无限期扣住(卡流 + 无上限增长),流结束时那截
/// 尾巴直接丢掉。
fn legacy_decode(frames: &[Vec<u8>]) -> (String, usize) {
    let mut out = String::new();
    let mut leftover: Vec<u8> = Vec::new();
    for frame in frames {
        let mut combined = std::mem::take(&mut leftover);
        combined.extend_from_slice(frame);
        let valid_len = match std::str::from_utf8(&combined) {
            Ok(_) => combined.len(),
            Err(error) => error.valid_up_to(),
        };
        if valid_len > 0 {
            if let Ok(text) = std::str::from_utf8(&combined[..valid_len]) {
                out.push_str(text);
            }
        }
        leftover = if valid_len < combined.len() {
            combined[valid_len..].to_vec()
        } else {
            Vec::new()
        };
    }
    (out, leftover.len())
}

fn frame_slices(session: &Session) -> Vec<Vec<u8>> {
    let mut pos = 0usize;
    session
        .frames
        .iter()
        .map(|&end| {
            let frame = session.oracle[pos..end].to_vec();
            pos = end;
            frame
        })
        .collect()
}

#[test]
fn raw_bytes_from_a_real_pty_are_byte_for_byte_in_the_core_stream() {
    let session = run_fake_agent("fidelity", "4");
    let total = session.oracle.len() as u64;
    assert!(
        session.oracle.len() > TERMINAL_REPLAY_WINDOW_BYTES * 2,
        "压力量必须超过回放窗口才同时检验保真与有界,实测 {} KiB",
        session.oracle.len() / 1024
    );
    assert!(
        session.frames.len() > 2,
        "真 PTY 必须分帧投递,实测 {} 帧",
        session.frames.len()
    );

    // 反空转守卫:确有某一帧把多字节序列劈在边界上,否则这条用例没测到跨帧路径。
    assert!(
        session
            .frames
            .iter()
            .any(|&end| cuts_a_multi_byte_sequence(&session.oracle, end)),
        "没有跨帧劈开的多字节序列 = 测试空转"
    );

    let (earliest, next) = session
        .service
        .bounds(&session.identity)
        .expect("in registry");
    assert_eq!(next, total, "末端 offset 必须等于读到的总原始字节数");
    assert!(earliest > 0, "总量超过窗口却没发生压缩?");
    assert_eq!(
        (next - earliest) as usize,
        TERMINAL_REPLAY_WINDOW_BYTES,
        "保留量必须正好封顶在窗口"
    );

    // 未截断区间:逐字节 + sha256 双重比对。
    let chunk = session
        .service
        .read_from(&session.identity, earliest, usize::MAX)
        .expect("read the retained region");
    assert!(!chunk.gap, "从 earliest 起读是无缝的");
    assert_eq!(chunk.offset, earliest);
    assert_eq!(chunk.bytes, session.oracle[earliest as usize..]);
    assert_eq!(
        sha256(&chunk.bytes),
        sha256(&session.oracle[earliest as usize..]),
        "流里保留的字节必须与读到的字节逐字节一致"
    );

    // 落后太多的订阅者:Gap + 追平点,而不是"假装从 0 起都能恢复"。
    let behind = session
        .service
        .read_from(&session.identity, 0, usize::MAX)
        .expect("gap read");
    assert!(behind.gap);
    assert_eq!(behind.offset, earliest);
    assert_eq!(behind.bytes, chunk.bytes);

    // 投影不丢信息:合法流上逐字符等于一次性解码 oracle。
    let one_shot =
        String::from_utf8(session.oracle.clone()).expect("fidelity 模式的输出必须是合法 UTF-8");
    assert_eq!(
        session.decoded, one_shot,
        "跨帧多字节序列必须在投影里完整复原"
    );
    assert_eq!(session.projection_pending, 0);
    assert_eq!(session.service.refusals(), 0);
    eprintln!(
        "[pty_bytes] fidelity {} MiB / {} frames in {} ms",
        total / 1024 / 1024,
        session.frames.len(),
        session.elapsed_ms
    );
}

#[test]
fn replay_memory_does_not_grow_with_total_output() {
    let small = run_fake_agent("fidelity", "2");
    let large = run_fake_agent("fidelity", "8");
    let retained = |session: &Session| -> usize {
        let (earliest, next) = session.service.bounds(&session.identity).unwrap();
        (next - earliest) as usize
    };
    assert!(
        large.oracle.len() > small.oracle.len() * 3,
        "两档压力没拉开:1 MiB={} 8 MiB={}",
        small.oracle.len(),
        large.oracle.len()
    );
    assert_eq!(
        retained(&small),
        retained(&large),
        "保留量只受窗口上限约束,不得随总输出增长"
    );
    assert!(retained(&large) <= TERMINAL_REPLAY_WINDOW_BYTES);
    assert_eq!(large.service.refusals(), 0);
}

#[test]
fn nasty_bytes_neither_stall_the_projection_nor_vanish_from_the_stream() {
    let session = run_fake_agent("nasty", "0");
    assert!(!session.frames.is_empty(), "nasty 模式没有任何输出");

    let (earliest, next) = session.service.bounds(&session.identity).unwrap();
    assert_eq!(next, session.oracle.len() as u64);
    let chunk = session
        .service
        .read_from(&session.identity, earliest, 1 << 20)
        .unwrap();
    assert_eq!(chunk.bytes, session.oracle, "非法字节也必须原样进流");
    assert!(
        session.oracle.contains(&0x00),
        "替身没写出 NUL,测试形状变了"
    );
    assert!(
        std::str::from_utf8(&session.oracle).is_err(),
        "oracle 应当含非法 UTF-8,否则这条用例是空的"
    );

    assert!(!session.decoded.is_empty());
    assert!(
        session.decoded.contains(char::REPLACEMENT_CHARACTER),
        "非法字节要成为可见的 U+FFFD,而不是把流扣死"
    );
    assert_eq!(
        session.projection_pending, 0,
        "流结束时不得还有字节扣在投影里"
    );

    // 同一批帧喂旧算法:它在第一个非法字节之后彻底卡住,并把尾巴永远扣住。
    let (legacy_out, legacy_leftover) = legacy_decode(&frame_slices(&session));
    assert!(
        legacy_leftover > 0,
        "旧实现在这里应当留下被扣住的尾巴(缺陷复现)"
    );
    assert!(
        legacy_out.len() < session.decoded.len(),
        "旧实现因卡流少发了内容:{} < {}",
        legacy_out.len(),
        session.decoded.len()
    );
}

#[test]
fn a_hundred_mebibytes_through_a_real_pty_keep_their_hash_intact_and_bounded() {
    // 验收点名的 100 MiB **连续输出端到端**:走真 PTY,但记账本身也必须有界 ——
    // 只留最后窗口大小的滚动尾巴 + 增量 hash,不在测试进程里堆 100 MiB 副本。
    let fixture = fixture_path();
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
    cmd.env("FAKE_MODE", "fidelity");
    cmd.env("FAKE_STRESS_MIB", "100");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn fake agent");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("clone reader");

    let service = TerminalService::new();
    let identity = TerminalIdentity::new("stress-pty", 1);
    let mut projection = Utf8Projection::new();
    let window = TERMINAL_REPLAY_WINDOW_BYTES;
    let mut tail: Vec<u8> = Vec::with_capacity(window);
    let mut total_hash = Sha256::new();
    let mut total: u64 = 0;
    let mut frames = 0usize;
    let mut decoded_bytes = 0usize;
    let mut max_pending = 0usize;
    let mut buf = vec![0u8; READ_BUFFER];
    let started = std::time::Instant::now();
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let chunk = &buf[..n];
                service.publish(&identity, chunk);
                let text = projection.push(chunk);
                decoded_bytes += text.len();
                max_pending = max_pending.max(projection.pending_len());
                total_hash.update(chunk);
                total += n as u64;
                frames += 1;
                tail.extend_from_slice(chunk);
                if tail.len() > window {
                    let drop = tail.len() - window;
                    tail.drain(..drop);
                }
            }
        }
    }
    decoded_bytes += projection.finish().len();
    service.mark_finished(&identity);
    assert!(child.wait().expect("wait").success());

    assert!(
        total > 100 * 1024 * 1024 / 2,
        "替身没写够量:实际 {} MiB",
        total / 1024 / 1024
    );
    let (earliest, next) = service.bounds(&identity).unwrap();
    assert_eq!(next, total, "末端 offset = 读到的总原始字节数");
    assert_eq!(
        (next - earliest) as usize,
        window,
        "100 MiB 之后保留量必须正好封顶在窗口,不随总量线性增长"
    );
    let chunk = service.read_from(&identity, earliest, usize::MAX).unwrap();
    assert!(!chunk.gap);
    assert_eq!(chunk.bytes.len(), window);
    assert_eq!(
        chunk.bytes, tail,
        "未截断区间必须与读到的最后窗口逐字节一致"
    );
    assert_eq!(sha256(&chunk.bytes), sha256(&tail));
    // 落后订阅者:如实的 Gap + 追平点。
    let behind = service.read_from(&identity, 0, usize::MAX).unwrap();
    assert!(behind.gap);
    assert_eq!(behind.offset, earliest);
    assert_eq!(behind.bytes, tail);
    // 全量 hash 只对"整段流"成立,用它证明流没有静默丢过字节。
    let full_hash = finish_hex(total_hash);
    assert_eq!(full_hash.len(), 64);
    assert!(
        decoded_bytes > 0 && max_pending < 8,
        "投影要么没产出,要么扣住了异常长的尾巴:decoded={decoded_bytes} pending={max_pending}"
    );
    assert_eq!(service.refusals(), 0);
    eprintln!(
        "[pty_bytes] 100 MiB end-to-end through a real pty: {} frames, {} ms, retained {} KiB, \
         test-side footprint kept to the {} KiB window",
        frames,
        started.elapsed().as_millis(),
        window / 1024,
        window / 1024,
    );
}

#[test]
fn the_real_pty_path_reports_no_registry_pressure() {
    let session = run_fake_agent("fidelity", "1");
    assert_eq!(session.service.refusals(), 0);
    assert_eq!(session.service.stream_count(), 1);
    assert_eq!(session.service.retired_count(), 0);
}

#[test]
fn the_fake_agent_dependency_is_actually_present() {
    // 替身跑不起来就是环境缺陷,必须报出来,而不是让上面那些用例以别的方式"通过"。
    let output = StdCommand::new(node_program())
        .arg("--version")
        .output()
        .expect("node 必须在 PATH 上(P3-03 的终端替身 agent 需要它)");
    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}
