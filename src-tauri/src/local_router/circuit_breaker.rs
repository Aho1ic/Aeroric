use super::RouterAgent;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout_seconds: u64,
    pub error_rate_percent: u8,
    pub min_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 4,
            success_threshold: 2,
            timeout_seconds: 60,
            error_rate_percent: 60,
            min_requests: 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CircuitPermit {
    pub allowed: bool,
    pub used_half_open_permit: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CircuitBreakerStats {
    pub state: CircuitState,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub total_requests: u32,
    pub failed_requests: u32,
    pub last_success_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_error: Option<String>,
}

struct CircuitInner {
    state: CircuitState,
    config: CircuitBreakerConfig,
    consecutive_failures: u32,
    consecutive_successes: u32,
    total_requests: u32,
    failed_requests: u32,
    opened_at: Option<Instant>,
    half_open_in_flight: bool,
    last_success_at: Option<i64>,
    last_failure_at: Option<i64>,
    last_error: Option<String>,
}

impl CircuitInner {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            config,
            consecutive_failures: 0,
            consecutive_successes: 0,
            total_requests: 0,
            failed_requests: 0,
            opened_at: None,
            half_open_in_flight: false,
            last_success_at: None,
            last_failure_at: None,
            last_error: None,
        }
    }

    // `half_open_in_flight` 一共在 5 处被清零,变异测试实测只有 2 处承重:
    //
    //   承重 · `transition_to_half_open`   —— 进入 HalfOpen 的唯一入口,而这个标志
    //                                        只在 HalfOpen 分支被读
    //   承重 · `record_success` 的守卫      —— 未达 success_threshold 时不发生任何
    //                                        transition,不清就永久卡住
    //   冗余 · `transition_to_open`
    //   冗余 · `transition_to_closed`
    //   冗余 · `record_failure` 的守卫      —— HalfOpen 下失败必经 transition_to_open
    //
    // **三处冗余是刻意留的,不要"优化"掉。** 令牌泄漏的后果是熔断器永久停在半开且
    // 无人可试探 —— 上游早恢复了也回不到 Closed,是本文件最严重的失效模式。
    // 这个状态机被 6 个方法改写,让每次状态转移都自己恢复不变量,比依赖"当前调用图
    // 恰好不会走到那里"要稳。谁再跑变异测试看到这三处存活,是这个原因,不是缺测试。
    fn transition_to_open(&mut self) {
        self.state = CircuitState::Open;
        self.opened_at = Some(Instant::now());
        self.consecutive_successes = 0;
        self.half_open_in_flight = false;
    }

    fn transition_to_half_open(&mut self) {
        self.state = CircuitState::HalfOpen;
        self.consecutive_successes = 0;
        self.half_open_in_flight = false;
    }

    fn transition_to_closed(&mut self) {
        self.state = CircuitState::Closed;
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
        self.total_requests = 0;
        self.failed_requests = 0;
        self.opened_at = None;
        self.half_open_in_flight = false;
    }

    fn stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            state: self.state,
            consecutive_failures: self.consecutive_failures,
            consecutive_successes: self.consecutive_successes,
            total_requests: self.total_requests,
            failed_requests: self.failed_requests,
            last_success_at: self.last_success_at,
            last_failure_at: self.last_failure_at,
            last_error: self.last_error.clone(),
        }
    }
}

pub struct CircuitBreaker {
    inner: Mutex<CircuitInner>,
}

impl CircuitBreaker {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            inner: Mutex::new(CircuitInner::new(config)),
        }
    }

    pub async fn allow_request(&self, config: CircuitBreakerConfig) -> CircuitPermit {
        let mut inner = self.inner.lock().await;
        inner.config = config;

        if inner.state == CircuitState::Open
            && inner
                .opened_at
                .is_some_and(|opened| opened.elapsed().as_secs() >= inner.config.timeout_seconds)
        {
            inner.transition_to_half_open();
        }

        match inner.state {
            CircuitState::Closed => CircuitPermit {
                allowed: true,
                used_half_open_permit: false,
            },
            CircuitState::Open => CircuitPermit::default(),
            CircuitState::HalfOpen if !inner.half_open_in_flight => {
                inner.half_open_in_flight = true;
                CircuitPermit {
                    allowed: true,
                    used_half_open_permit: true,
                }
            }
            CircuitState::HalfOpen => CircuitPermit::default(),
        }
    }

    pub async fn record_success(&self, config: CircuitBreakerConfig, used_half_open_permit: bool) {
        let mut inner = self.inner.lock().await;
        inner.config = config;
        if used_half_open_permit {
            inner.half_open_in_flight = false;
        }

        inner.last_success_at = Some(chrono::Utc::now().timestamp_millis());
        inner.last_error = None;
        inner.consecutive_failures = 0;
        inner.total_requests = inner.total_requests.saturating_add(1);

        if inner.state == CircuitState::HalfOpen {
            inner.consecutive_successes = inner.consecutive_successes.saturating_add(1);
            if inner.consecutive_successes >= inner.config.success_threshold {
                inner.transition_to_closed();
            }
        }
    }

    pub async fn record_failure(
        &self,
        config: CircuitBreakerConfig,
        used_half_open_permit: bool,
        error: impl AsRef<str>,
    ) {
        let mut inner = self.inner.lock().await;
        inner.config = config;
        if used_half_open_permit {
            inner.half_open_in_flight = false;
        }

        inner.last_failure_at = Some(chrono::Utc::now().timestamp_millis());
        inner.last_error = Some(super::usage::sanitize_summary(error.as_ref()));
        inner.consecutive_failures = inner.consecutive_failures.saturating_add(1);
        inner.consecutive_successes = 0;
        inner.total_requests = inner.total_requests.saturating_add(1);
        inner.failed_requests = inner.failed_requests.saturating_add(1);

        if inner.state == CircuitState::HalfOpen {
            inner.transition_to_open();
            return;
        }

        if inner.state != CircuitState::Closed {
            return;
        }

        let failure_threshold_reached =
            inner.consecutive_failures >= inner.config.failure_threshold;
        let error_rate_reached = inner.total_requests >= inner.config.min_requests
            && u64::from(inner.failed_requests).saturating_mul(100)
                >= u64::from(inner.total_requests)
                    .saturating_mul(u64::from(inner.config.error_rate_percent));
        if failure_threshold_reached || error_rate_reached {
            inner.transition_to_open();
        }
    }

    pub async fn release_neutral(&self, used_half_open_permit: bool) {
        if !used_half_open_permit {
            return;
        }
        self.inner.lock().await.half_open_in_flight = false;
    }

    pub async fn reset(&self) {
        let mut inner = self.inner.lock().await;
        inner.transition_to_closed();
        inner.last_error = None;
    }

    pub async fn stats(&self) -> CircuitBreakerStats {
        self.inner.lock().await.stats()
    }
}

#[derive(Default)]
pub struct CircuitBreakerRegistry {
    breakers: RwLock<HashMap<(RouterAgent, String), Arc<CircuitBreaker>>>,
}

impl CircuitBreakerRegistry {
    async fn get_or_create(
        &self,
        agent: RouterAgent,
        target_id: &str,
        config: CircuitBreakerConfig,
    ) -> Arc<CircuitBreaker> {
        let key = (agent, target_id.to_string());
        if let Some(breaker) = self.breakers.read().await.get(&key).cloned() {
            return breaker;
        }

        let mut breakers = self.breakers.write().await;
        breakers
            .entry(key)
            .or_insert_with(|| Arc::new(CircuitBreaker::new(config)))
            .clone()
    }

    pub async fn allow_request(
        &self,
        agent: RouterAgent,
        target_id: &str,
        config: CircuitBreakerConfig,
    ) -> CircuitPermit {
        self.get_or_create(agent, target_id, config.clone())
            .await
            .allow_request(config)
            .await
    }

    pub async fn record_success(
        &self,
        agent: RouterAgent,
        target_id: &str,
        config: CircuitBreakerConfig,
        used_half_open_permit: bool,
    ) {
        self.get_or_create(agent, target_id, config.clone())
            .await
            .record_success(config, used_half_open_permit)
            .await;
    }

    pub async fn record_failure(
        &self,
        agent: RouterAgent,
        target_id: &str,
        config: CircuitBreakerConfig,
        used_half_open_permit: bool,
        error: impl AsRef<str>,
    ) {
        self.get_or_create(agent, target_id, config.clone())
            .await
            .record_failure(config, used_half_open_permit, error)
            .await;
    }

    pub async fn release_neutral(
        &self,
        agent: RouterAgent,
        target_id: &str,
        config: CircuitBreakerConfig,
        used_half_open_permit: bool,
    ) {
        self.get_or_create(agent, target_id, config)
            .await
            .release_neutral(used_half_open_permit)
            .await;
    }

    pub async fn reset(&self, agent: RouterAgent, target_id: &str, config: CircuitBreakerConfig) {
        self.get_or_create(agent, target_id, config)
            .await
            .reset()
            .await;
    }

    pub async fn stats(
        &self,
        agent: RouterAgent,
        target_id: &str,
        config: CircuitBreakerConfig,
    ) -> CircuitBreakerStats {
        self.get_or_create(agent, target_id, config)
            .await
            .stats()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_after_threshold_and_recovers_through_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 1,
            timeout_seconds: 0,
            error_rate_percent: 100,
            min_requests: 100,
        };
        let breaker = CircuitBreaker::new(config.clone());

        breaker.record_failure(config.clone(), false, "one").await;
        breaker.record_failure(config.clone(), false, "two").await;
        assert_eq!(breaker.stats().await.state, CircuitState::Open);

        let permit = breaker.allow_request(config.clone()).await;
        assert!(permit.allowed);
        assert!(permit.used_half_open_permit);
        breaker
            .record_success(config, permit.used_half_open_permit)
            .await;
        assert_eq!(breaker.stats().await.state, CircuitState::Closed);
    }

    #[tokio::test]
    async fn half_open_allows_only_one_probe() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..CircuitBreakerConfig::default()
        };
        let breaker = CircuitBreaker::new(config.clone());
        breaker
            .record_failure(config.clone(), false, "failed")
            .await;

        assert!(breaker.allow_request(config.clone()).await.allowed);
        assert!(!breaker.allow_request(config).await.allowed);
    }

    #[tokio::test]
    async fn neutral_result_releases_the_half_open_probe() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..CircuitBreakerConfig::default()
        };
        let breaker = CircuitBreaker::new(config.clone());
        breaker
            .record_failure(config.clone(), false, "failed")
            .await;

        let first = breaker.allow_request(config.clone()).await;
        assert!(first.allowed && first.used_half_open_permit);
        breaker.release_neutral(first.used_half_open_permit).await;

        let second = breaker.allow_request(config).await;
        assert!(second.allowed && second.used_half_open_permit);
    }
}

/// 熔断器的并发压力测试。
///
/// 上面 `tests` 覆盖了顺序状态机(阈值打开、半开单探针、中性释放)。这里补并发面 ——
/// 熔断器是整个 local_router 唯一的**共享可变状态**,所有上游请求都会撞上它。
///
/// **不测「注册表被撑爆」**:`target_id` 来自用户自己配的上游列表
/// (`server.rs:201` 的 `target.id()`),不是请求里的数据,条数由配置规模决定。
/// 那张 map 无上限是可以接受的,写在这儿免得后来人当成漏洞去"修"。
#[cfg(test)]
mod stress_tests {
    use super::*;

    fn fast_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            // 0 秒超时:Open 状态下一次 allow_request 就翻到 HalfOpen,不用 sleep。
            timeout_seconds: 0,
            error_rate_percent: 100,
            min_requests: u32::MAX,
        }
    }

    /// 打到 Open 再要一个半开探针许可。
    async fn breaker_in_half_open(config: &CircuitBreakerConfig) -> Arc<CircuitBreaker> {
        let breaker = Arc::new(CircuitBreaker::new(config.clone()));
        for i in 0..config.failure_threshold {
            breaker
                .record_failure(config.clone(), false, format!("f{i}"))
                .await;
        }
        assert_eq!(breaker.stats().await.state, CircuitState::Open);
        breaker
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn only_one_of_many_concurrent_requests_gets_the_half_open_probe() {
        // 半开态的**全部意义**是"只放一个试探请求过去"。如果并发下能放行多个,
        // 一个已经挂掉的上游会在恢复瞬间被打满,熔断器等于没有。
        // `half_open_in_flight` 的读改写都在同一把 tokio Mutex 内,这一条钉住它。
        let config = fast_config();
        let breaker = breaker_in_half_open(&config).await;

        let mut handles = Vec::new();
        for _ in 0..64 {
            let breaker = Arc::clone(&breaker);
            let config = config.clone();
            handles.push(tokio::spawn(
                async move { breaker.allow_request(config).await },
            ));
        }

        let mut allowed = 0;
        let mut with_permit = 0;
        for handle in handles {
            let permit = handle.await.expect("task panicked");
            if permit.allowed {
                allowed += 1;
            }
            if permit.used_half_open_permit {
                with_permit += 1;
            }
        }

        assert_eq!(allowed, 1, "64 路并发只应有 1 个被放行,实际 {allowed} 个");
        assert_eq!(
            with_permit, 1,
            "被放行的那个必须带 used_half_open_permit(否则它完成时不会归还令牌)"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn a_half_open_permit_is_never_leaked_by_any_completion_path() {
        // 令牌只有三条归还路径:成功 / 失败 / 中性。任何一条漏了归还,
        // 熔断器就**永久卡在半开且无人可试探** —— 上游早就恢复了也再也回不到 Closed。
        // 这是本文件里最危险的失效模式,所以三条路径逐个走一遍。
        let config = fast_config();

        // 路径一:失败归还 —— 失败会把状态推回 Open,再要一次应当又能拿到探针。
        let breaker = breaker_in_half_open(&config).await;
        let probe = breaker.allow_request(config.clone()).await;
        assert!(probe.allowed && probe.used_half_open_permit);
        breaker
            .record_failure(config.clone(), probe.used_half_open_permit, "probe failed")
            .await;
        assert_eq!(breaker.stats().await.state, CircuitState::Open);
        let next = breaker.allow_request(config.clone()).await;
        assert!(
            next.allowed && next.used_half_open_permit,
            "失败归还后应能再次试探,否则令牌泄漏了"
        );

        // 路径二:成功归还但还没到 success_threshold —— 仍在半开,令牌必须已归还。
        let breaker = breaker_in_half_open(&config).await;
        let probe = breaker.allow_request(config.clone()).await;
        breaker
            .record_success(config.clone(), probe.used_half_open_permit)
            .await;
        let stats = breaker.stats().await;
        assert_eq!(
            stats.state,
            CircuitState::HalfOpen,
            "success_threshold=2,一次成功还不该闭合"
        );
        assert_eq!(stats.consecutive_successes, 1);
        let second = breaker.allow_request(config.clone()).await;
        assert!(
            second.allowed && second.used_half_open_permit,
            "半开态下第一次成功之后必须还能再放一个探针,否则令牌泄漏了"
        );

        // 路径三:中性归还(上面 `tests` 已覆盖顺序场景,这里确认它也不改状态)。
        let breaker = breaker_in_half_open(&config).await;
        let probe = breaker.allow_request(config.clone()).await;
        breaker.release_neutral(probe.used_half_open_permit).await;
        assert_eq!(
            breaker.stats().await.state,
            CircuitState::HalfOpen,
            "中性释放不应改变状态"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_failures_open_the_circuit_exactly_once() {
        // 大量并发失败时,状态必须收敛到 Open,且计数不能因为竞态而漏记。
        // `total_requests` / `failed_requests` 都用 saturating_add,所以这里
        // 断言的是"每一次失败都被记上了" —— 少记就说明有失败被丢在竞态里。
        let config = fast_config();
        let breaker = Arc::new(CircuitBreaker::new(config.clone()));
        let failures = 128;

        let mut handles = Vec::new();
        for i in 0..failures {
            let breaker = Arc::clone(&breaker);
            let config = config.clone();
            handles.push(tokio::spawn(async move {
                breaker
                    .record_failure(config, false, format!("err-{i}"))
                    .await;
            }));
        }
        for handle in handles {
            handle.await.expect("task panicked");
        }

        let stats = breaker.stats().await;
        assert_eq!(stats.state, CircuitState::Open);
        assert_eq!(
            stats.failed_requests, failures,
            "并发失败不能丢记账:期望 {failures},实际 {}",
            stats.failed_requests
        );
        assert_eq!(stats.total_requests, failures);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn a_closed_circuit_never_throttles_concurrent_traffic() {
        // 反向保证:闭合态不能因为那把锁而拒绝任何请求。
        // 如果这条挂了,说明熔断器在正常路径上变成了阀门(把并发压成串行是性能问题,
        // 但**拒绝**请求是正确性问题)。
        let config = fast_config();
        let breaker = Arc::new(CircuitBreaker::new(config.clone()));

        let mut handles = Vec::new();
        for _ in 0..256 {
            let breaker = Arc::clone(&breaker);
            let config = config.clone();
            handles.push(tokio::spawn(async move {
                let permit = breaker.allow_request(config.clone()).await;
                breaker
                    .record_success(config, permit.used_half_open_permit)
                    .await;
                permit
            }));
        }

        for handle in handles {
            let permit = handle.await.expect("task panicked");
            assert!(permit.allowed, "闭合态必须放行每一个请求");
            assert!(
                !permit.used_half_open_permit,
                "闭合态不该发半开令牌 —— 发了会在完成时错误地清 half_open_in_flight"
            );
        }
        assert_eq!(breaker.stats().await.state, CircuitState::Closed);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn the_error_rate_path_opens_the_circuit_without_a_failure_streak() {
        // 两条打开条件是 `||`:连续失败数**或**错误率。这一条只走错误率那半 ——
        // 把失败阈值设得高到够不着,靠成败交替把错误率顶上去。
        // 交替能保证 consecutive_failures 永远回落到 1,不会误触另一半。
        let config = CircuitBreakerConfig {
            failure_threshold: u32::MAX,
            success_threshold: 2,
            timeout_seconds: 0,
            error_rate_percent: 50,
            min_requests: 10,
        };
        let breaker = CircuitBreaker::new(config.clone());

        for i in 0..12 {
            if i % 2 == 0 {
                breaker.record_failure(config.clone(), false, "boom").await;
            } else {
                breaker.record_success(config.clone(), false).await;
            }
            // 记完账立刻看:一旦打开就停手,好断言"是错误率把它打开的"。
            if breaker.stats().await.state == CircuitState::Open {
                break;
            }
        }

        let stats = breaker.stats().await;
        assert_eq!(
            stats.state,
            CircuitState::Open,
            "50% 错误率 + 达到 min_requests 应当打开熔断"
        );
        assert!(
            stats.consecutive_failures < config.failure_threshold,
            "必须是错误率那一半触发的,连续失败数远够不着阈值"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn reset_clears_the_probe_token_and_the_counters() {
        // `reset` 走 `transition_to_closed`,必须连 `half_open_in_flight` 一起清 ——
        // 漏清的话手动重置之后第一个请求会被当成"已有探针在飞"而拒掉。
        let config = fast_config();
        let breaker = breaker_in_half_open(&config).await;
        let probe = breaker.allow_request(config.clone()).await;
        assert!(probe.used_half_open_permit, "先占住探针令牌");

        breaker.reset().await;

        let stats = breaker.stats().await;
        assert_eq!(stats.state, CircuitState::Closed);
        assert_eq!(stats.consecutive_failures, 0);
        assert_eq!(stats.failed_requests, 0);
        assert_eq!(stats.total_requests, 0);
        assert!(stats.last_error.is_none(), "reset 必须清掉错误文案");

        let after = breaker.allow_request(config).await;
        assert!(after.allowed, "重置后第一个请求必须放行");
        assert!(!after.used_half_open_permit);
    }
}
