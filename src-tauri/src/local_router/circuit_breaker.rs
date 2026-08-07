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
