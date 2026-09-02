use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio_tungstenite::tungstenite::handshake::server::Request;

pub(crate) const CLIENT_CONNECT_RATE_LIMIT: u32 = 12;
pub(crate) const CLIENT_CONNECT_RATE_WINDOW: Duration = Duration::from_secs(10);
const CLIENT_RATE_LIMIT_RETENTION: Duration = Duration::from_secs(10 * 60);
const MAX_CLIENT_RATE_LIMIT_ENTRIES: usize = 4096;

pub(crate) struct ClientRateLimit {
    window_started: Instant,
    attempts: u32,
    last_seen: Instant,
}

pub(crate) fn source_ip(peer: IpAddr, request: &Request) -> IpAddr {
    if !peer.is_loopback() {
        return peer;
    }
    ["x-forwarded-for", "x-real-ip"]
        .iter()
        .filter_map(|name| request.headers().get(*name))
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.rsplit(',').next())
        .find_map(|value| value.trim().parse().ok())
        .unwrap_or(peer)
}

pub(crate) fn try_acquire(
    rate_limits: &Mutex<HashMap<IpAddr, ClientRateLimit>>,
    peer: IpAddr,
    now: Instant,
) -> bool {
    let mut limits = rate_limits.lock().unwrap();
    if limits.len() >= MAX_CLIENT_RATE_LIMIT_ENTRIES {
        limits.retain(|_, entry| now.duration_since(entry.last_seen) < CLIENT_RATE_LIMIT_RETENTION);
        if limits.len() >= MAX_CLIENT_RATE_LIMIT_ENTRIES && !limits.contains_key(&peer) {
            if let Some(oldest) = limits
                .iter()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(address, _)| *address)
            {
                limits.remove(&oldest);
            }
        }
    }

    let entry = limits.entry(peer).or_insert(ClientRateLimit {
        window_started: now,
        attempts: 0,
        last_seen: now,
    });
    entry.last_seen = now;
    if now.duration_since(entry.window_started) >= CLIENT_CONNECT_RATE_WINDOW {
        entry.window_started = now;
        entry.attempts = 0;
    }
    if entry.attempts >= CLIENT_CONNECT_RATE_LIMIT {
        return false;
    }
    entry.attempts += 1;
    true
}

/// 限流器的爆破 / 并发压力测试。
///
/// `main.rs` 的测试模块已经覆盖了顺序基本面(逐次消耗额度、换 IP 不受影响、
/// 跨窗口恢复、loopback 才信 `x-forwarded-for`)。这里补的是**对抗面**:
/// 并发下额度会不会被突破、表满时的淘汰能不能被攻击者利用、
/// 以及 `source_ip` 在畸形头部下的行为。
///
/// `try_acquire` 的 `now` 是入参,所以窗口相关的用例全部确定性推进时间,不 sleep。
#[cfg(test)]
mod stress_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Barrier;

    fn limits() -> Arc<Mutex<HashMap<IpAddr, ClientRateLimit>>> {
        Arc::new(Mutex::new(HashMap::new()))
    }

    fn ip(index: u32) -> IpAddr {
        // 198.51.100.0/24 与 203.0.113.0/24 是文档专用段,不会撞上真实地址。
        IpAddr::from(std::net::Ipv4Addr::from(0xc633_6400 + index))
    }

    #[test]
    fn concurrent_attempts_from_one_ip_cannot_exceed_the_window_budget() {
        // 单 IP 并发冲刺:放行次数必须**恰好**等于额度,不能因为
        // 「读 attempts → 判断 → 写回」之间的交错而多放。
        let limits = limits();
        let attackers = 64;
        let now = Instant::now();
        let barrier = Arc::new(Barrier::new(attackers));

        let handles: Vec<_> = (0..attackers)
            .map(|_| {
                let limits = Arc::clone(&limits);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    try_acquire(&limits, ip(1), now)
                })
            })
            .collect();

        let granted = handles
            .into_iter()
            .filter(|_| true)
            .map(|h| h.join().expect("worker panicked"))
            .filter(|allowed| *allowed)
            .count();

        assert_eq!(
            granted, CLIENT_CONNECT_RATE_LIMIT as usize,
            "{attackers} 路并发只应放行 {CLIENT_CONNECT_RATE_LIMIT} 次,实际 {granted} 次"
        );
    }

    #[test]
    fn concurrent_attempts_from_many_ips_each_get_their_own_budget() {
        // 每个 IP 一份额度,并发下也不能互相扣减(否则是把限流做成了全局阀门,
        // 一台设备重连就能把别人挡在外面)。
        let limits = limits();
        let peers = 16;
        let per_peer = CLIENT_CONNECT_RATE_LIMIT as usize + 4;
        let now = Instant::now();
        let barrier = Arc::new(Barrier::new(peers));

        let handles: Vec<_> = (0..peers)
            .map(|index| {
                let limits = Arc::clone(&limits);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..per_peer)
                        .filter(|_| try_acquire(&limits, ip(100 + index as u32), now))
                        .count()
                })
            })
            .collect();

        for granted in handles
            .into_iter()
            .map(|h| h.join().expect("worker panicked"))
        {
            assert_eq!(
                granted, CLIENT_CONNECT_RATE_LIMIT as usize,
                "每个 IP 都应拿到完整额度且不多放"
            );
        }
    }

    #[test]
    fn the_table_stays_bounded_and_evicts_the_least_recently_seen_entry() {
        // 限流表必须有界,否则它自己就是内存放大器。有界的代价是表满时得丢掉
        // 某条计数,实现选的是丢 `last_seen` 最旧的那条。这一条钉住这两点。
        let limits = limits();
        let now = Instant::now();

        // 一条"陈旧"的条目:之后所有流量都比它新。
        let stale = ip(4);
        assert!(try_acquire(&limits, stale, now));

        let fresh_at = now + Duration::from_millis(1);
        for index in 0..(MAX_CLIENT_RATE_LIMIT_ENTRIES as u32 + 512) {
            try_acquire(&limits, ip(10_000 + index), fresh_at);
        }

        let table = limits.lock().unwrap();
        assert!(
            table.len() <= MAX_CLIENT_RATE_LIMIT_ENTRIES,
            "限流表必须保持有界,实际 {} 条",
            table.len()
        );
        assert!(
            !table.contains_key(&stale),
            "表满时应淘汰 last_seen 最旧的条目"
        );
    }

    #[test]
    fn evicting_a_throttled_entry_gives_an_attacker_strictly_less_than_flooding_already_does() {
        // **这一条是把一个「看着像漏洞、其实不是」的行为钉死,免得下一个人误修。**
        //
        // 事实:被限流的攻击者用海量 IP 灌满表,能把自己那条挤掉从而清零计数。
        // 下面先如实复现这个行为(它确实发生),再说明为什么不修:
        //
        //   灌满表需要 4096 个独立 IP,而这 4096 个 IP 本身就有
        //   4096 × 12 = 49152 的连接额度 —— 比"重置一条拿回 12"多 4096 倍。
        //   也就是说走淘汰路径重置自己是**严格更差的攻击**,不给攻击者任何新能力。
        //
        // 反过来,如果改成"表满时拒绝新 IP",灌表者就能把新来的正常客户端全部挡在
        // 门外(真正的 DoS)。当前实现是两害中更轻的一侧。
        let limits = limits();
        let now = Instant::now();
        let attacker = ip(5);

        for _ in 0..CLIENT_CONNECT_RATE_LIMIT {
            assert!(try_acquire(&limits, attacker, now));
        }
        assert!(!try_acquire(&limits, attacker, now), "额度应已耗尽");

        for index in 0..(MAX_CLIENT_RATE_LIMIT_ENTRIES as u32 + 512) {
            try_acquire(&limits, ip(20_000 + index), now + Duration::from_millis(1));
        }

        // 如实记录:确实被重置了。这不是期望行为也不是缺陷,是有界表的固有代价。
        let reset = try_acquire(&limits, attacker, now + Duration::from_millis(2));
        assert!(
            reset,
            "当前实现下攻击者可以靠灌表重置自己的计数 —— 若这条开始失败,\
             说明淘汰策略变了,请重新评估上面那段成本分析"
        );

        // 真正要守住的是这个:重置一次也只拿回一个窗口的额度,不是无限放行。
        let regained = (0..CLIENT_CONNECT_RATE_LIMIT + 5)
            .filter(|_| try_acquire(&limits, attacker, now + Duration::from_millis(2)))
            .count();
        assert_eq!(
            regained,
            CLIENT_CONNECT_RATE_LIMIT as usize - 1,
            "重置之后仍受同一个窗口额度约束(已用掉 1 次)"
        );
    }

    #[test]
    fn a_throttled_peer_stays_throttled_within_the_window_without_table_pressure() {
        // 没有灌表压力时,被限流的 IP 在窗口内反复重试必须一直被拒 ——
        // 尤其是「重试本身会刷新 last_seen」不能变成「重试能续命」。
        let limits = limits();
        let now = Instant::now();
        let peer = ip(6);

        for _ in 0..CLIENT_CONNECT_RATE_LIMIT {
            assert!(try_acquire(&limits, peer, now));
        }
        for step in 1..50 {
            let later = now + Duration::from_millis(step * 10);
            assert!(
                !try_acquire(&limits, peer, later),
                "窗口内第 {step} 次重试仍应被拒"
            );
        }
    }

    #[test]
    fn the_window_is_fixed_not_sliding_and_that_is_documented_here() {
        // **这不是 bug,是固定窗口的固有性质,写下来免得被当成回归。**
        // 攻击者在窗口末尾打满额度、窗口一翻立刻再打满,短时间内能拿到 2 倍额度。
        // 想消掉这个就得换成滑动窗口或令牌桶 —— 那是设计变更,不是修 bug。
        // 这里断言的是"确实是 2 倍,不会更多",给未来改成滑动窗口时留一个对照点。
        let limits = limits();
        let now = Instant::now();
        let peer = ip(3);

        let first_window = (0..CLIENT_CONNECT_RATE_LIMIT + 5)
            .filter(|_| try_acquire(&limits, peer, now))
            .count();
        let boundary = now + CLIENT_CONNECT_RATE_WINDOW;
        let second_window = (0..CLIENT_CONNECT_RATE_LIMIT + 5)
            .filter(|_| try_acquire(&limits, peer, boundary))
            .count();

        assert_eq!(first_window, CLIENT_CONNECT_RATE_LIMIT as usize);
        assert_eq!(second_window, CLIENT_CONNECT_RATE_LIMIT as usize);
        // 窗口边界是 `>=`,所以恰好等于窗长的那一刻就翻窗。差一点则不翻。
        let just_before = boundary + CLIENT_CONNECT_RATE_WINDOW - Duration::from_millis(1);
        assert!(
            !try_acquire(&limits, peer, just_before),
            "还差 1ms 不该翻窗"
        );
    }

    // ── source_ip 的畸形输入 ────────────────────────────────────────────────

    fn request_with(headers: &[(&str, &str)]) -> Request {
        let mut builder = Request::builder().uri("/connect/host");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap()
    }

    #[test]
    fn a_non_loopback_peer_can_never_override_its_own_rate_limit_key() {
        // 最关键的一条:直连的客户端不管怎么伪造头部,限流键都必须是它的真实 IP。
        // 否则攻击者只要每次换一个 `x-forwarded-for` 就能无限重置额度。
        let public: IpAddr = "198.51.100.20".parse().unwrap();
        for headers in [
            vec![("x-forwarded-for", "203.0.113.1")],
            vec![("x-real-ip", "203.0.113.2")],
            vec![
                ("x-forwarded-for", "203.0.113.3"),
                ("x-real-ip", "203.0.113.4"),
            ],
        ] {
            assert_eq!(
                source_ip(public, &request_with(&headers)),
                public,
                "非 loopback 对端的头部必须被忽略: {headers:?}"
            );
        }
    }

    #[test]
    fn malformed_forwarded_headers_fall_back_to_the_peer() {
        // 畸形头部不能让限流键变成某个可控的常量(那等于所有攻击者共享一条计数),
        // 也不能 panic。回落到对端本身是唯一安全的选择。
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        for headers in [
            vec![("x-forwarded-for", "")],
            vec![("x-forwarded-for", "   ")],
            vec![("x-forwarded-for", "not-an-ip")],
            vec![("x-forwarded-for", ",")],
            vec![("x-forwarded-for", "203.0.113.1, garbage")],
            vec![("x-forwarded-for", "999.999.999.999")],
            vec![("x-real-ip", "::gg")],
        ] {
            assert_eq!(
                source_ip(loopback, &request_with(&headers)),
                loopback,
                "畸形头部应回落到对端: {headers:?}"
            );
        }
    }

    #[test]
    fn the_rightmost_forwarded_entry_wins() {
        // 取最右一项(`rsplit().next()`)= 离本机最近的那一跳,也就是我们自己的
        // 反代写进去的值。取最左会直接采信客户端自填的第一段,那是可伪造的。
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        let request =
            request_with(&[("x-forwarded-for", "203.0.113.1, 203.0.113.2, 198.51.100.7")]);
        assert_eq!(
            source_ip(loopback, &request),
            "198.51.100.7".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn an_ipv6_loopback_proxy_is_also_trusted() {
        // 反代可能从 `::1` 连进来。`is_loopback()` 对 `::1` 为真,所以头部同样可信;
        // 漏掉这一支会让 IPv6 部署下所有客户端共享同一条限流计数(全都是 ::1)。
        let v6_loopback: IpAddr = "::1".parse().unwrap();
        let request = request_with(&[("x-forwarded-for", "198.51.100.9")]);
        assert_eq!(
            source_ip(v6_loopback, &request),
            "198.51.100.9".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn an_ipv4_mapped_loopback_is_not_treated_as_a_trusted_proxy() {
        // `::ffff:127.0.0.1` 的 `is_loopback()` 为 **false**(Rust 只认 ::1)。
        // 所以这种对端的头部不被采信,限流键就是它自己 —— 方向是安全的那一侧。
        // 写下来是因为这一点很反直觉,将来有人"顺手修一下"会打开伪造缺口。
        let mapped: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        let request = request_with(&[("x-forwarded-for", "203.0.113.99")]);
        assert_eq!(source_ip(mapped, &request), mapped);
    }
}
