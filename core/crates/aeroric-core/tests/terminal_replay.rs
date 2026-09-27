//! P3-03 切片四的验收面:断线重订阅**不重不漏**、缓冲过期得到**明确的 gap/reset**、
//! 首订阅的 snapshot 与 live 边界由单线程序列化保证。
//!
//! 两种证据:①真 PTY 上的真输出(fidelity 模式,替身自己会把多字节序列劈在帧边界上),
//! ②一条持续发布的流上并发取帧 —— 后者钉的是"边界由锁/单线程序列化保证"这句话本身,
//! 不是"看着没丢"。

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::terminal::{
    plan_replay, ReplayPlan, ResetReason, ResumePoint, TerminalIdentity, TerminalService,
    Utf8Projection, TERMINAL_MAX_FINISHED_STREAMS, TERMINAL_REPLAY_WINDOW_BYTES,
};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const READ_BUFFER: usize = 32 * 1024;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent.mjs")
}

fn node_program() -> String {
    std::env::var("AERORIC_FAKE_AGENT_NODE").unwrap_or_else(|_| "node".to_string())
}

/// 真 PTY → core 字节流:与宿主 reader 同一条次序(raw bytes 先 publish)。
/// 返回测试侧自己记的 oracle(从 master 读到的每个字节);服务由调用方持有。
fn pump_real_pty(
    service: &TerminalService,
    identity: &TerminalIdentity,
    mode: &str,
    stress_mib: &str,
) -> Vec<u8> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open a pty");
    let mut cmd = CommandBuilder::new(node_program());
    cmd.arg(fixture_path());
    cmd.env("FAKE_MODE", mode);
    cmd.env("FAKE_STRESS_MIB", stress_mib);
    let mut child = pair.slave.spawn_command(cmd).expect("spawn fake agent");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let mut projection = Utf8Projection::new();
    let mut oracle = Vec::new();
    let mut buf = vec![0u8; READ_BUFFER];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                service.publish(identity, &buf[..n]);
                projection.push(&buf[..n]);
                oracle.extend_from_slice(&buf[..n]);
            }
        }
    }
    projection.finish();
    service.mark_finished(identity);
    assert!(
        child.wait().expect("wait").success(),
        "替身 agent 退出码非 0"
    );
    oracle
}

#[test]
fn replaying_a_real_pty_stream_from_the_catchup_point_loses_and_repeats_nothing() {
    // 2 MiB 真输出 > 1 MiB 窗口 ⇒ 前面必然被压缩掉,正好走完整路径:
    // 过期请求给明确 gap,追平点之后不重不漏。
    let identity = TerminalIdentity::new("replay-real", 4);
    let service = TerminalService::new();
    let oracle = pump_real_pty(&service, &identity, "fidelity", "2");
    let total = oracle.len() as u64;

    let (earliest, next) = service.bounds(&identity).expect("stream registered");
    assert_eq!(next, total);
    assert!(earliest > 0, "超过窗口却没压缩?");

    // ① 拿最早的 offset 0 去续看:必须是明确的 Compacted 重置 + 追平点。
    let stale = plan_replay(&service, &identity, ResumePoint::Bytes(0), 1 << 20);
    match &stale {
        ReplayPlan::Reset {
            from,
            reason,
            bytes,
        } => {
            assert_eq!(*from, earliest);
            assert_eq!(*reason, ResetReason::Compacted { earliest });
            assert!(!bytes.is_empty(), "重置的那一帧要把现存内容给出来");
        }
        other => panic!("落后一整段必须重置:{other:?}"),
    }

    // ② 从追平点起逐帧取到末端:拼起来必须**逐字节**等于真 PTY 输出的最后一段。
    let mut cursor = ResumePoint::Bytes(earliest);
    let mut replayed: Vec<u8> = Vec::new();
    let mut frames = 0usize;
    while replayed.len() as u64 + earliest < total {
        let plan = plan_replay(&service, &identity, cursor, 64 * 1024);
        match &plan {
            ReplayPlan::Seamless { offset, bytes } => {
                assert_eq!(
                    *offset,
                    earliest + replayed.len() as u64,
                    "帧 {frames} 出现了洞或重复"
                );
                replayed.extend_from_slice(bytes);
            }
            other => panic!("追平点之后不该再出现重置:{other:?}"),
        }
        frames += 1;
        cursor = plan.resume_after();
    }
    assert!(frames > 8, "帧数太少说明没真走增量取帧:{frames}");
    assert_eq!(
        replayed,
        oracle[earliest as usize..],
        "重放出来的字节必须与真 PTY 读到的字节逐字节一致"
    );

    // ③ 取完之后再来一次:合法的末端空帧,而不是"谎称还有历史"。
    assert_eq!(
        plan_replay(&service, &identity, cursor, 4096),
        ReplayPlan::Seamless {
            offset: total,
            bytes: Vec::new()
        }
    );
}

#[test]
fn the_first_snapshot_and_the_live_edge_cannot_lose_a_byte_between_them() {
    // Action 4 的"首订阅 snapshot 与 live 的边界用锁/单线程序列化保证":
    // 一边持续发布,一边只靠 `resume_after()` 推进游标。任何交错下,已交付的字节
    // 都必须正好是"已发布的前缀",一处不重、一处不漏。
    let service = Arc::new(TerminalService::new());
    let identity = TerminalIdentity::new("atomic-edge", 1);
    let publisher = {
        let service = Arc::clone(&service);
        let identity = identity.clone();
        std::thread::spawn(move || {
            let mut written: Vec<u8> = Vec::new();
            for index in 0..400u32 {
                let block = format!("frame-{index:04}-😀中文\u{7}\u{0}tail").into_bytes();
                service.publish(&identity, &block);
                written.extend_from_slice(&block);
            }
            written
        })
    };

    // 流是在**第一次 publish** 时才建立的:在此之前它"不在册",core 如实回答
    // UnknownStream。所以订阅者先等到流存在,再开始按游标取帧。
    let mut waits = 0usize;
    while service.bounds(&identity).is_none() {
        std::thread::yield_now();
        waits += 1;
        assert!(waits < 2_000_000, "发布线程没建流");
    }

    let mut delivered: Vec<u8> = Vec::new();
    let mut cursor = ResumePoint::Bytes(0);
    let mut rounds = 0usize;
    let mut spins = 0usize;
    while delivered.len() < 400 * 4 {
        let plan = plan_replay(&service, &identity, cursor, 32);
        match &plan {
            ReplayPlan::Seamless { offset, bytes } => {
                assert_eq!(*offset as usize, delivered.len(), "边界处漏读或多读");
                delivered.extend_from_slice(bytes);
                if !bytes.is_empty() {
                    rounds += 1;
                }
            }
            ReplayPlan::Reset { reason, .. } => {
                panic!("流已建立且总量不到 20 KiB(远没到压缩线),这里不该出现 {reason:?}")
            }
        }
        cursor = plan.resume_after();
        spins += 1;
        if spins > 200_000 {
            panic!(
                "取帧循环没在推进:delivered={} rounds={}",
                delivered.len(),
                rounds
            );
        }
    }
    let published = publisher.join().expect("publisher");
    assert!(
        rounds > 1,
        "只取到一帧说明没有真在并发下取过:rounds={rounds}"
    );
    assert_eq!(
        &delivered[..],
        &published[..delivered.len()],
        "已交付的字节必须是发布过的前缀"
    );
    // 收尾:追到发布末端,拿到的东西与发布总量逐字节相等。
    while (delivered.len() as u64) < service.bounds(&identity).expect("bounds").1 {
        let plan = plan_replay(&service, &identity, cursor, 1 << 14);
        if let ReplayPlan::Seamless { bytes, .. } = &plan {
            delivered.extend_from_slice(bytes);
        }
        cursor = plan.resume_after();
    }
    assert_eq!(
        delivered, published,
        "并发取帧最终必须与整条发布序列一字不差"
    );
}

#[test]
fn a_retired_stream_is_reported_as_unknown_instead_of_a_lie_about_history() {
    // FIFO 退场只发生在**已收尾**的流上;退场之后订阅者必须拿到 UnknownStream,
    // 而不是"从 0 起一切完整"的假无缝。
    let service = TerminalService::new();
    let evicted = TerminalIdentity::new("evicted", 1);
    for index in 0..=TERMINAL_MAX_FINISHED_STREAMS {
        let identity = TerminalIdentity::new(format!("finished-{index}"), 1);
        service.publish(&identity, b"x");
        service.mark_finished(&identity);
    }
    let oldest = TerminalIdentity::new("finished-0", 1);
    service.publish(&evicted, "尾巴".as_bytes());
    service.mark_finished(&evicted);
    assert!(
        service.retired_count() > 0,
        "没触发退场就说明这条用例没测到不在册的情形"
    );
    // FIFO 挤掉的是**最老的已收尾流**;最后插入那条还在册。两侧都断,免得退场方向
    // 反过来时用例仍然全绿。
    assert!(
        service.bounds(&oldest).is_none(),
        "`finished-0` 必须真的已不在册,否则下面断的不是 UnknownStream"
    );
    assert!(
        service.bounds(&evicted).is_some(),
        "新近收尾的流不得被挤掉——那是'谎报历史完整'的另一半"
    );

    for cursor in [ResumePoint::Bytes(0), ResumePoint::UnitsFromStart(1)] {
        assert_eq!(
            plan_replay(&service, &oldest, cursor, 1024),
            ReplayPlan::Reset {
                from: 0,
                bytes: Vec::new(),
                reason: ResetReason::UnknownStream
            },
            "退场过的流:两种游标都得拿到同一个如实的理由"
        );
    }
}

#[test]
fn a_window_that_started_mid_character_is_handed_over_as_bytes_not_as_text() {
    // 保留线会落在字符中间。构造是算出来的:发布 A×W + `😀中` + B×(W-5),
    // 总量 2W+2 ⇒ 保留线 = W+2,正好落在 `😀` 的第 3 个字节上。
    let window = TERMINAL_REPLAY_WINDOW_BYTES;
    let service = TerminalService::new();
    let identity = TerminalIdentity::new("mid-char-window", 2);
    let mut published = vec![b'A'; window];
    published.extend_from_slice("😀中".as_bytes());
    published.extend_from_slice(&vec![b'B'; window - 5]);
    service.publish(&identity, &published);

    let (earliest, next) = service.bounds(&identity).expect("in registry");
    assert_eq!(next, published.len() as u64);
    assert_eq!(earliest, (window + 2) as u64, "保留线必须正好在 emoji 中间");

    let plan = plan_replay(
        &service,
        &identity,
        ResumePoint::Bytes(earliest),
        usize::MAX,
    );
    match &plan {
        ReplayPlan::Seamless { offset, bytes } => {
            assert_eq!(*offset, earliest);
            assert_eq!(
                bytes,
                &published[earliest as usize..],
                "窗口里剩下什么就原样给什么:不重新对齐、不替换、不吞"
            );
            assert_eq!(bytes.len(), window);
            assert_eq!(
                &bytes[..2],
                &"😀".as_bytes()[2..],
                "前两字节是半截 emoji 的续字节"
            );
            assert!(
                std::str::from_utf8(bytes).is_err(),
                "窗口起点在字符中间,单独解不开 —— 解码只能是消费方的投影"
            );
        }
        other => panic!("从保留线起读应当无缝:{other:?}"),
    }
}

#[test]
fn a_live_stream_refuses_a_units_cursor_before_any_conversion_is_tried() {
    // 四A 记下的覆盖不均:集成侧当时只在**已退场**流上测码元游标——那里
    // `read_from` 本来就先失败,拒收分支根本没执行到。这条在**活流**(多字节内容、
    // 未收尾)上断同一件事:码元游标永远拿不到字节、拿不到猜出来的位置,
    // `resume_after()` 落回字节车道 0,客户端重置视图后照常续看。
    let service = TerminalService::new();
    let identity = TerminalIdentity::new("live-units-cursor", 1);
    let mut published: Vec<u8> = Vec::new();
    for index in 0..40u32 {
        let block = format!("frame-{index:02}-😀中文\r\n").into_bytes();
        service.publish(&identity, &block);
        published.extend_from_slice(&block);
    }
    // 流还活着:没 mark_finished,bounds 在册且末端就是已发布量。
    assert_eq!(
        service
            .bounds(&identity)
            .expect("live stream in registry")
            .1,
        published.len() as u64
    );

    // 拒收的形状:重置视图**并**交出现存窗口的全部内容供客户端重绘(from=0、
    // 理由=UnitsNotAnchorable)。绝不做的是换算出一个字节位置假装无缝。
    // 本流 ~900 B 远小于 1 MiB 窗口,所以交出的就是整条发布序列。
    for units in [1u64, 5, 100] {
        let plan = plan_replay(
            &service,
            &identity,
            ResumePoint::UnitsFromStart(units),
            4096,
        );
        match &plan {
            ReplayPlan::Reset {
                from,
                bytes,
                reason,
            } => {
                assert_eq!(*from, 0);
                assert_eq!(*reason, ResetReason::UnitsNotAnchorable);
                assert_eq!(bytes, &published, "重置必须带上现存窗口供重绘");
            }
            other => panic!("码元游标 {units} 绝不能被当成无缝续看:{other:?}"),
        }
        // 重绘之后订阅者已经站在窗口末端:resume_after() 落在发布末端,
        // 从那里按字节续看得到合法的末端空帧(而不是再次 Reset)。
        let cursor = plan.resume_after();
        assert_eq!(
            cursor,
            ResumePoint::Bytes(published.len() as u64),
            "拒收后的续看凭证 = 已重绘字节数之后的字节 offset"
        );
        let plan = plan_replay(&service, &identity, cursor, 64);
        assert!(
            matches!(plan, ReplayPlan::Seamless { ref bytes, .. } if bytes.is_empty()),
            "活流末端应答合法空帧:{plan:?}"
        );
    }

    // 唯一诚实的无缝例外:码元 0 且保留线仍在 0(客户端什么都没渲染过)。
    assert_eq!(
        plan_replay(&service, &identity, ResumePoint::UnitsFromStart(0), 4096),
        ReplayPlan::Seamless {
            offset: 0,
            bytes: published.clone(),
        },
        "units=0 且整段历史都在:字节 0 就是真凭证,如实无缝"
    );
}
