use super::{RouterAgent, RouterError};
use crate::sse::find_sse_delimiter;
use brotli::Decompressor as BrotliDecoder;
use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zstd::stream::Decoder as ZstdDecoder;

const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_CHARS: usize = 512;

/// E7 (2026-11-22): what a request was for, from the task its token names.
/// The host says how a task maps to one ([`set_purpose_resolver`]); a
/// request without a task token (a client using the access token) is
/// [`purpose::UNATTRIBUTED`]; rows from before E7 have none (`""`).
pub mod purpose {
    pub const ANSWER: &str = "answer";
    pub const REVIEW: &str = "review";
    pub const DELEGATION: &str = "delegation";
    pub const DISCUSSION: &str = "discussion";
    pub const RERUN: &str = "rerun";
    pub const SCHEDULE: &str = "schedule";
    pub const UNATTRIBUTED: &str = "unattributed";
    /// E7 step 2: Aeroric's own calls to the analysis model, not through
    /// the gateway — memory consolidation and the Wiki's module notes.
    pub const MEMORY: &str = "memory";
    pub const WIKI: &str = "wiki";
}

type PurposeResolver = Arc<dyn Fn(&str) -> String + Send + Sync>;

static RESOLVER: parking_lot::RwLock<Option<PurposeResolver>> = parking_lot::RwLock::new(None);

/// E7: how the host maps a task id to its purpose.
pub fn set_purpose_resolver(resolver: PurposeResolver) {
    *RESOLVER.write() = Some(resolver);
}

/// E7: the purpose of a request for `task_id` (none: unattributed; no
/// resolver: the answer itself).
pub fn purpose_of(task_id: Option<&str>) -> String {
    let Some(task) = task_id else {
        return purpose::UNATTRIBUTED.to_string();
    };
    match RESOLVER.read().clone() {
        Some(resolve) => resolve(task),
        None => purpose::ANSWER.to_string(),
    }
}

/// E7: one purpose's requests, and the tries that failed or were replaced
/// (counted apart, so the one that answered does not hide them).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurposeUsage {
    pub purpose: String,
    pub requests: u64,
    pub failed_requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub unknown_usage_requests: u64,
    pub failed_attempts: u64,
    pub failed_attempt_input_tokens: u64,
    pub failed_attempt_output_tokens: u64,
    /// E7 step 2: Aeroric called the model directly (not through the
    /// gateway); read from its own logs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub direct: bool,
}

/// C4 (2026-11-19): where a request's usage came from. `Unknown`: the
/// upstream reported none (a stream cut short, a provider that leaves it
/// out); its token columns hold 0 but say nothing, and totals leave it out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSource {
    #[default]
    Reported,
    /// Reserved: Aeroric's own estimate (nothing estimates yet).
    Estimated,
    Unknown,
}

impl UsageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Estimated => "estimated",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "estimated" => Self::Estimated,
            "unknown" => Self::Unknown,
            _ => Self::Reported,
        }
    }
}

/// C4: one try of a request at one upstream target (or a target the
/// circuit breaker skipped). The request row stays the sum of the story.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterAttempt {
    /// 1, 2, 3 … in the order the targets were met.
    pub seq: u32,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub outbound_model: Option<String>,
    /// The upstream's status; unset when none answered (or skipped).
    pub status_code: Option<u16>,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub usage_source: UsageSource,
    pub error_summary: Option<String>,
    /// The circuit breaker kept the request away from this target.
    pub skipped: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    /// C4: the upstream sent a usage object at all.
    pub reported: bool,
    /// C4: Anthropic's split of cache writes by lifetime, when given.
    pub cache_write_5m: Option<u64>,
    pub cache_write_1h: Option<u64>,
    pub model: Option<String>,
    pub response_id: Option<String>,
    /// 首字延迟(毫秒)。只有流式响应且真的收到过 SSE 数据事件时才有值。
    pub ttft_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterRequestRecord {
    pub request_id: String,
    pub session_id: Option<String>,
    pub response_id: Option<String>,
    pub agent: RouterAgent,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub endpoint: String,
    pub attempt_count: u32,
    pub model: String,
    pub outbound_model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub status_code: u16,
    pub latency_ms: u64,
    pub started_at: i64,
    pub completed_at: i64,
    pub is_streaming: bool,
    pub success: bool,
    pub error_summary: Option<String>,
    /// 首字延迟(毫秒)。非流式请求、以及没收到任何 SSE 数据事件的流式请求为 None。
    pub ttft_ms: Option<u64>,
    /// C4: whether the token columns were reported; old rows read `reported`.
    pub usage_source: UsageSource,
    /// C4: cache writes by lifetime; unset when the upstream gave only the total.
    pub cache_write_5m_tokens: Option<u64>,
    pub cache_write_1h_tokens: Option<u64>,
    /// C4: each try, oldest first; empty on rows from before C4.
    pub attempts: Vec<RouterAttempt>,
    /// E7: the task whose token the request carried.
    pub task_id: Option<String>,
    /// E7: what it was for ([`purpose`]); `""` on rows from before E7.
    pub purpose: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterUsageSummary {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub last_request_at: Option<i64>,
    /// C4: requests whose usage nobody reported (left out of the sums).
    pub unknown_usage_requests: u64,
    pub cache_write_5m_tokens: u64,
    pub cache_write_1h_tokens: u64,
}

#[derive(Clone)]
pub(crate) struct UsageStore {
    database_path: Arc<PathBuf>,
}

/// How long request rows are kept (2026-10-29).
const RETENTION_MS: i64 = 90 * 24 * 60 * 60 * 1000;

impl UsageStore {
    pub(crate) fn new(database_path: PathBuf) -> Self {
        Self {
            database_path: Arc::new(database_path),
        }
    }

    pub(crate) async fn initialize(&self) -> Result<(), RouterError> {
        let path = self.database_path.clone();
        tokio::task::spawn_blocking(move || open_database(path.as_ref()).map(|_| ()))
            .await
            .map_err(|error| {
                RouterError::storage(format!("router database task failed: {error}"))
            })?
    }

    pub(crate) async fn insert(&self, request: RouterRequestRecord) -> Result<(), RouterError> {
        let path = self.database_path.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = open_database(path.as_ref())?;
            let connection = connection
                .transaction()
                .map_err(|error| RouterError::storage(error.to_string()))?;
            connection
                .execute(
                    "
                    INSERT INTO router_requests (
                        request_id,
                        session_id,
                        response_id,
                        agent,
                        target_id,
                        target_name,
                        endpoint,
                        attempt_count,
                        model,
                        outbound_model,
                        input_tokens,
                        output_tokens,
                        cache_creation_tokens,
                        cache_read_tokens,
                        status_code,
                        latency_ms,
                        started_at,
                        completed_at,
                        is_streaming,
                        success,
                        error_summary,
                        ttft_ms,
                        usage_source,
                        cache_write_5m_tokens,
                        cache_write_1h_tokens,
                        task_id,
                        purpose
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                        ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                        ?16, ?17, ?18, ?19, ?20, ?21, ?22,
                        ?23, ?24, ?25, ?26, ?27
                    )
                    ON CONFLICT(request_id) DO NOTHING
                    ",
                    params![
                        request.request_id,
                        request.session_id,
                        request.response_id,
                        request.agent.as_str(),
                        request.target_id,
                        request.target_name,
                        request.endpoint,
                        i64::from(request.attempt_count),
                        request.model,
                        request.outbound_model,
                        as_sql_integer(request.input_tokens),
                        as_sql_integer(request.output_tokens),
                        as_sql_integer(request.cache_creation_tokens),
                        as_sql_integer(request.cache_read_tokens),
                        i64::from(request.status_code),
                        as_sql_integer(request.latency_ms),
                        request.started_at,
                        request.completed_at,
                        i64::from(request.is_streaming),
                        i64::from(request.success),
                        request.error_summary,
                        request.ttft_ms.map(as_sql_integer),
                        request.usage_source.as_str(),
                        request.cache_write_5m_tokens.map(as_sql_integer),
                        request.cache_write_1h_tokens.map(as_sql_integer),
                        request.task_id,
                        request.purpose,
                    ],
                )
                .map_err(|error| RouterError::storage(error.to_string()))?;
            for attempt in &request.attempts {
                connection
                    .execute(
                        "
                        INSERT OR REPLACE INTO router_attempts (
                            request_id, seq, target_id, target_name, outbound_model,
                            status_code, latency_ms, input_tokens, output_tokens,
                            cache_creation_tokens, cache_read_tokens, usage_source,
                            error_summary, skipped
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                        ",
                        params![
                            request.request_id,
                            i64::from(attempt.seq),
                            attempt.target_id,
                            attempt.target_name,
                            attempt.outbound_model,
                            attempt.status_code.map(i64::from),
                            as_sql_integer(attempt.latency_ms),
                            as_sql_integer(attempt.input_tokens),
                            as_sql_integer(attempt.output_tokens),
                            as_sql_integer(attempt.cache_creation_tokens),
                            as_sql_integer(attempt.cache_read_tokens),
                            attempt.usage_source.as_str(),
                            attempt.error_summary,
                            i64::from(attempt.skipped),
                        ],
                    )
                    .map_err(|error| RouterError::storage(error.to_string()))?;
            }
            // Keep 90 days (2026-10-29: recording is on now, so the table grows).
            let cutoff = request.completed_at.saturating_sub(RETENTION_MS);
            connection
                .execute(
                    "DELETE FROM router_attempts WHERE request_id IN
                        (SELECT request_id FROM router_requests WHERE completed_at < ?1)",
                    params![cutoff],
                )
                .map_err(|error| RouterError::storage(error.to_string()))?;
            connection
                .execute(
                    "DELETE FROM router_requests WHERE completed_at < ?1",
                    params![cutoff],
                )
                .map_err(|error| RouterError::storage(error.to_string()))?;
            connection
                .commit()
                .map_err(|error| RouterError::storage(error.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|error| RouterError::storage(format!("router database task failed: {error}")))?
    }

    pub(crate) async fn summary(&self) -> Result<RouterUsageSummary, RouterError> {
        let path = self.database_path.clone();
        tokio::task::spawn_blocking(move || {
            let connection = open_database(path.as_ref())?;
            connection
                .query_row(
                    "
                    SELECT
                        COUNT(*),
                        COALESCE(SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN usage_source = 'unknown' THEN 0 ELSE input_tokens END), 0),
                        COALESCE(SUM(CASE WHEN usage_source = 'unknown' THEN 0 ELSE output_tokens END), 0),
                        COALESCE(SUM(CASE WHEN usage_source = 'unknown' THEN 0 ELSE cache_creation_tokens END), 0),
                        COALESCE(SUM(CASE WHEN usage_source = 'unknown' THEN 0 ELSE cache_read_tokens END), 0),
                        MAX(completed_at),
                        COALESCE(SUM(CASE WHEN usage_source = 'unknown' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(cache_write_5m_tokens), 0),
                        COALESCE(SUM(cache_write_1h_tokens), 0)
                    FROM router_requests
                    ",
                    [],
                    |row| {
                        Ok(RouterUsageSummary {
                            total_requests: nonnegative(row.get(0)?),
                            successful_requests: nonnegative(row.get(1)?),
                            failed_requests: nonnegative(row.get(2)?),
                            input_tokens: nonnegative(row.get(3)?),
                            output_tokens: nonnegative(row.get(4)?),
                            cache_creation_tokens: nonnegative(row.get(5)?),
                            cache_read_tokens: nonnegative(row.get(6)?),
                            last_request_at: row.get(7)?,
                            unknown_usage_requests: nonnegative(row.get(8)?),
                            cache_write_5m_tokens: nonnegative(row.get(9)?),
                            cache_write_1h_tokens: nonnegative(row.get(10)?),
                        })
                    },
                )
                .map_err(|error| RouterError::storage(error.to_string()))
        })
        .await
        .map_err(|error| RouterError::storage(format!("router database task failed: {error}")))?
    }

    /// E7: requests by purpose since `since_ms`, with the tries that failed
    /// or were replaced counted apart; the most requested first.
    pub(crate) async fn purposes(&self, since_ms: i64) -> Result<Vec<PurposeUsage>, RouterError> {
        let path = self.database_path.clone();
        tokio::task::spawn_blocking(move || {
            let connection = open_database(path.as_ref())?;
            let mut statement = connection
                .prepare(
                    "
                    SELECT
                        r.purpose,
                        COUNT(*),
                        COALESCE(SUM(CASE WHEN r.success = 0 THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN r.usage_source = 'unknown' THEN 0 ELSE r.input_tokens END), 0),
                        COALESCE(SUM(CASE WHEN r.usage_source = 'unknown' THEN 0 ELSE r.output_tokens END), 0),
                        COALESCE(SUM(CASE WHEN r.usage_source = 'unknown' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(f.tries), 0),
                        COALESCE(SUM(f.input), 0),
                        COALESCE(SUM(f.output), 0)
                    FROM router_requests r
                    LEFT JOIN (
                        SELECT request_id, COUNT(*) AS tries,
                               SUM(input_tokens) AS input, SUM(output_tokens) AS output
                        FROM router_attempts
                        WHERE skipped = 0
                          AND (status_code IS NULL OR status_code >= 400 OR error_summary IS NOT NULL)
                        GROUP BY request_id
                    ) f ON f.request_id = r.request_id
                    WHERE r.completed_at >= ?1
                    GROUP BY r.purpose
                    ORDER BY COUNT(*) DESC, r.purpose
                    ",
                )
                .map_err(|error| RouterError::storage(error.to_string()))?;
            let rows = statement
                .query_map(params![since_ms], |row| {
                    Ok(PurposeUsage {
                        purpose: row.get(0)?,
                        requests: nonnegative(row.get(1)?),
                        failed_requests: nonnegative(row.get(2)?),
                        input_tokens: nonnegative(row.get(3)?),
                        output_tokens: nonnegative(row.get(4)?),
                        unknown_usage_requests: nonnegative(row.get(5)?),
                        failed_attempts: nonnegative(row.get(6)?),
                        failed_attempt_input_tokens: nonnegative(row.get(7)?),
                        failed_attempt_output_tokens: nonnegative(row.get(8)?),
                        direct: false,
                    })
                })
                .map_err(|error| RouterError::storage(error.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| RouterError::storage(error.to_string()))
        })
        .await
        .map_err(|error| RouterError::storage(format!("router database task failed: {error}")))?
    }

    pub(crate) async fn recent_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<RouterRequestRecord>, RouterError> {
        let path = self.database_path.clone();
        let limit = limit.clamp(1, 500) as i64;
        tokio::task::spawn_blocking(move || {
            let connection = open_database(path.as_ref())?;
            let mut statement = connection
                .prepare(
                    "
                    SELECT
                        request_id,
                        session_id,
                        response_id,
                        agent,
                        target_id,
                        target_name,
                        endpoint,
                        attempt_count,
                        model,
                        outbound_model,
                        input_tokens,
                        output_tokens,
                        cache_creation_tokens,
                        cache_read_tokens,
                        status_code,
                        latency_ms,
                        started_at,
                        completed_at,
                        is_streaming,
                        success,
                        error_summary,
                        ttft_ms,
                        usage_source,
                        cache_write_5m_tokens,
                        cache_write_1h_tokens,
                        task_id,
                        purpose
                    FROM router_requests
                    ORDER BY completed_at DESC, rowid DESC
                    LIMIT ?1
                    ",
                )
                .map_err(|error| RouterError::storage(error.to_string()))?;
            let rows = statement
                .query_map(params![limit], |row| {
                    let agent = match row.get::<_, String>(3)?.as_str() {
                        "codex" => RouterAgent::Codex,
                        _ => RouterAgent::Claude,
                    };
                    Ok(RouterRequestRecord {
                        request_id: row.get(0)?,
                        session_id: row.get(1)?,
                        response_id: row.get(2)?,
                        agent,
                        target_id: row.get(4)?,
                        target_name: row.get(5)?,
                        endpoint: row.get(6)?,
                        attempt_count: row.get::<_, i64>(7)?.clamp(0, u32::MAX as i64) as u32,
                        model: row.get(8)?,
                        outbound_model: row.get(9)?,
                        input_tokens: nonnegative(row.get(10)?),
                        output_tokens: nonnegative(row.get(11)?),
                        cache_creation_tokens: nonnegative(row.get(12)?),
                        cache_read_tokens: nonnegative(row.get(13)?),
                        status_code: row.get::<_, i64>(14)?.clamp(0, u16::MAX as i64) as u16,
                        latency_ms: nonnegative(row.get(15)?),
                        started_at: row.get(16)?,
                        completed_at: row.get(17)?,
                        is_streaming: row.get::<_, i64>(18)? != 0,
                        success: row.get::<_, i64>(19)? != 0,
                        error_summary: row.get(20)?,
                        ttft_ms: row.get::<_, Option<i64>>(21)?.map(nonnegative),
                        usage_source: UsageSource::parse(&row.get::<_, String>(22)?),
                        cache_write_5m_tokens: row.get::<_, Option<i64>>(23)?.map(nonnegative),
                        cache_write_1h_tokens: row.get::<_, Option<i64>>(24)?.map(nonnegative),
                        attempts: Vec::new(),
                        task_id: row.get(25)?,
                        purpose: row.get::<_, Option<String>>(26)?.unwrap_or_default(),
                    })
                })
                .map_err(|error| RouterError::storage(error.to_string()))?;
            let mut records = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| RouterError::storage(error.to_string()))?;
            for record in &mut records {
                record.attempts = attempts_of(&connection, &record.request_id)?;
            }
            Ok(records)
        })
        .await
        .map_err(|error| RouterError::storage(format!("router database task failed: {error}")))?
    }
}

fn open_database(path: &PathBuf) -> Result<Connection, RouterError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| RouterError::storage(error.to_string()))?;
    }
    let connection =
        Connection::open(path).map_err(|error| RouterError::storage(error.to_string()))?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|error| RouterError::storage(error.to_string()))?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| RouterError::storage(error.to_string()))?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|error| RouterError::storage(error.to_string()))?;
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS router_requests (
                request_id TEXT PRIMARY KEY,
                session_id TEXT,
                response_id TEXT,
                agent TEXT NOT NULL CHECK (agent IN ('claude', 'codex')),
                target_id TEXT,
                target_name TEXT,
                endpoint TEXT NOT NULL DEFAULT '',
                attempt_count INTEGER NOT NULL DEFAULT 0,
                model TEXT NOT NULL,
                outbound_model TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                status_code INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                completed_at INTEGER NOT NULL,
                is_streaming INTEGER NOT NULL CHECK (is_streaming IN (0, 1)),
                success INTEGER NOT NULL CHECK (success IN (0, 1)),
                error_summary TEXT,
                ttft_ms INTEGER
            );
            CREATE INDEX IF NOT EXISTS router_requests_completed_at
                ON router_requests (completed_at DESC);
            CREATE INDEX IF NOT EXISTS router_requests_agent_completed_at
                ON router_requests (agent, completed_at DESC);
            CREATE INDEX IF NOT EXISTS router_requests_response_id
                ON router_requests (agent, response_id)
                WHERE response_id IS NOT NULL;
            ",
        )
        .map_err(|error| RouterError::storage(error.to_string()))?;
    ensure_column(&connection, "session_id", "TEXT")?;
    ensure_column(&connection, "target_id", "TEXT")?;
    ensure_column(&connection, "target_name", "TEXT")?;
    ensure_column(&connection, "endpoint", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(&connection, "attempt_count", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(&connection, "outbound_model", "TEXT")?;
    ensure_column(&connection, "ttft_ms", "INTEGER")?;
    // C4: rows from before read as reported (they cannot be told apart).
    ensure_column(
        &connection,
        "usage_source",
        "TEXT NOT NULL DEFAULT 'reported'",
    )?;
    ensure_column(&connection, "cache_write_5m_tokens", "INTEGER")?;
    ensure_column(&connection, "cache_write_1h_tokens", "INTEGER")?;
    // E7 (2026-11-22).
    ensure_column(&connection, "task_id", "TEXT")?;
    ensure_column(&connection, "purpose", "TEXT NOT NULL DEFAULT ''")?;
    connection
        .execute_batch(
            "
            CREATE INDEX IF NOT EXISTS router_requests_target_completed_at
                ON router_requests (agent, target_id, completed_at DESC);
            CREATE TABLE IF NOT EXISTS router_attempts (
                request_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                target_id TEXT,
                target_name TEXT,
                outbound_model TEXT,
                status_code INTEGER,
                latency_ms INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                usage_source TEXT NOT NULL DEFAULT 'unknown',
                error_summary TEXT,
                skipped INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (request_id, seq)
            );
            ",
        )
        .map_err(|error| RouterError::storage(error.to_string()))?;
    Ok(connection)
}

/// C4: one request's tries, oldest first.
fn attempts_of(
    connection: &Connection,
    request_id: &str,
) -> Result<Vec<RouterAttempt>, RouterError> {
    let mut statement = connection
        .prepare(
            "
            SELECT seq, target_id, target_name, outbound_model, status_code, latency_ms,
                   input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                   usage_source, error_summary, skipped
            FROM router_attempts WHERE request_id = ?1 ORDER BY seq
            ",
        )
        .map_err(|error| RouterError::storage(error.to_string()))?;
    let rows = statement
        .query_map(params![request_id], |row| {
            Ok(RouterAttempt {
                seq: row.get::<_, i64>(0)?.clamp(0, u32::MAX as i64) as u32,
                target_id: row.get(1)?,
                target_name: row.get(2)?,
                outbound_model: row.get(3)?,
                status_code: row
                    .get::<_, Option<i64>>(4)?
                    .map(|code| code.clamp(0, u16::MAX as i64) as u16),
                latency_ms: nonnegative(row.get(5)?),
                input_tokens: nonnegative(row.get(6)?),
                output_tokens: nonnegative(row.get(7)?),
                cache_creation_tokens: nonnegative(row.get(8)?),
                cache_read_tokens: nonnegative(row.get(9)?),
                usage_source: UsageSource::parse(&row.get::<_, String>(10)?),
                error_summary: row.get(11)?,
                skipped: row.get::<_, i64>(12)? != 0,
            })
        })
        .map_err(|error| RouterError::storage(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| RouterError::storage(error.to_string()))
}

fn ensure_column(
    connection: &Connection,
    column: &str,
    definition: &str,
) -> Result<(), RouterError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(router_requests)")
        .map_err(|error| RouterError::storage(error.to_string()))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| RouterError::storage(error.to_string()))?
        .filter_map(Result::ok)
        .any(|name| name == column);
    drop(statement);
    if exists {
        return Ok(());
    }
    connection
        .execute_batch(&format!(
            "ALTER TABLE router_requests ADD COLUMN {column} {definition};"
        ))
        .map_err(|error| RouterError::storage(error.to_string()))
}

fn nonnegative(value: i64) -> u64 {
    value.max(0) as u64
}

fn as_sql_integer(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

pub(crate) fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(crate) fn sanitize_summary(message: &str) -> String {
    let collapsed = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let value = if collapsed.is_empty() {
        "local router request failed"
    } else {
        &collapsed
    };
    value.chars().take(MAX_ERROR_CHARS).collect()
}

pub(crate) struct UsageCapture {
    agent: RouterAgent,
    mode: CaptureMode,
}

enum CaptureMode {
    Json {
        bytes: Vec<u8>,
        truncated: bool,
        content_encoding: Option<String>,
    },
    Sse {
        pending: Vec<u8>,
        usage: TokenUsage,
        discarding_oversized_event: bool,
        /// 请求发起时刻。由 `with_request_start` 注入,缺失时不统计首字延迟。
        request_start: Option<Instant>,
    },
}

impl UsageCapture {
    pub(crate) fn new(agent: RouterAgent, streaming: bool, content_encoding: Option<&str>) -> Self {
        let mode = if streaming {
            CaptureMode::Sse {
                pending: Vec::new(),
                usage: TokenUsage::default(),
                discarding_oversized_event: false,
                request_start: None,
            }
        } else {
            CaptureMode::Json {
                bytes: Vec::new(),
                truncated: false,
                content_encoding: content_encoding.map(str::to_ascii_lowercase),
            }
        };
        Self { agent, mode }
    }

    /// 注入请求发起时刻,让流式捕获能算出首字延迟。与 `latency_ms` 共用同一个
    /// `Instant` 基准,因此 TTFT 必然不大于总延迟。非流式模式没有首字概念,忽略。
    pub(crate) fn with_request_start(mut self, started: Option<Instant>) -> Self {
        if let CaptureMode::Sse { request_start, .. } = &mut self.mode {
            *request_start = started;
        }
        self
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) {
        match &mut self.mode {
            CaptureMode::Json {
                bytes, truncated, ..
            } => {
                if *truncated {
                    return;
                }
                if bytes.len().saturating_add(chunk.len()) > MAX_CAPTURE_BYTES {
                    bytes.clear();
                    *truncated = true;
                } else {
                    bytes.extend_from_slice(chunk);
                }
            }
            CaptureMode::Sse {
                pending,
                usage,
                discarding_oversized_event,
                request_start,
            } => {
                pending.extend_from_slice(chunk);
                let parsed_data_event =
                    drain_sse_events(self.agent, pending, usage, discarding_oversized_event);
                // 首字延迟按"第一个真正带 JSON 数据的 SSE 事件抵达时"计,心跳注释和
                // `[DONE]` 不算首字。
                if parsed_data_event && usage.ttft_ms.is_none() {
                    if let Some(started) = request_start {
                        usage.ttft_ms = Some(elapsed_millis(*started));
                    }
                }
            }
        }
    }

    pub(crate) fn finish(mut self) -> TokenUsage {
        match &mut self.mode {
            CaptureMode::Json {
                bytes,
                truncated,
                content_encoding,
            } => {
                if *truncated {
                    return TokenUsage::default();
                }
                let decoded = decode_for_inspection(bytes, content_encoding.as_deref());
                let Some(decoded) = decoded else {
                    return TokenUsage::default();
                };
                let Ok(value) = serde_json::from_slice::<Value>(&decoded) else {
                    return TokenUsage::default();
                };
                let mut usage = TokenUsage::default();
                absorb_value(self.agent, &value, &mut usage);
                usage
            }
            CaptureMode::Sse { pending, usage, .. } => {
                if !pending.is_empty() {
                    // 收尾时补解析残留字节。这里的时间是流结束时刻而非事件抵达时刻,
                    // 故意不用它标记首字延迟。
                    parse_sse_event(self.agent, pending, usage);
                }
                usage.clone()
            }
        }
    }
}

pub(crate) fn decode_for_inspection(bytes: &[u8], encoding: Option<&str>) -> Option<Vec<u8>> {
    let mut decoded = Vec::new();
    match encoding.unwrap_or("identity").trim() {
        "" | "identity" => return Some(bytes.to_vec()),
        "gzip" | "x-gzip" => {
            GzDecoder::new(bytes)
                .take((MAX_CAPTURE_BYTES + 1) as u64)
                .read_to_end(&mut decoded)
                .ok()?;
        }
        "deflate" => {
            if ZlibDecoder::new(bytes)
                .take((MAX_CAPTURE_BYTES + 1) as u64)
                .read_to_end(&mut decoded)
                .is_err()
            {
                decoded.clear();
                DeflateDecoder::new(bytes)
                    .take((MAX_CAPTURE_BYTES + 1) as u64)
                    .read_to_end(&mut decoded)
                    .ok()?;
            }
        }
        "br" | "brotli" => {
            let decoder = BrotliDecoder::new(bytes, 4096);
            decoder
                .take((MAX_CAPTURE_BYTES + 1) as u64)
                .read_to_end(&mut decoded)
                .ok()?;
        }
        "zstd" => {
            ZstdDecoder::with_buffer(bytes)
                .ok()?
                .take((MAX_CAPTURE_BYTES + 1) as u64)
                .read_to_end(&mut decoded)
                .ok()?;
        }
        _ => return None,
    };
    (decoded.len() <= MAX_CAPTURE_BYTES).then_some(decoded)
}

/// 返回本次是否解析到过带 JSON 数据的事件,供调用方标记首字延迟。
fn drain_sse_events(
    agent: RouterAgent,
    pending: &mut Vec<u8>,
    usage: &mut TokenUsage,
    discarding_oversized_event: &mut bool,
) -> bool {
    let mut parsed_data_event = false;
    while let Some((index, delimiter_len)) = find_sse_delimiter(pending) {
        let event = pending.drain(..index + delimiter_len).collect::<Vec<_>>();
        if *discarding_oversized_event {
            *discarding_oversized_event = false;
            continue;
        }
        parsed_data_event |= parse_sse_event(agent, &event[..index], usage);
    }

    if pending.len() > MAX_CAPTURE_BYTES {
        pending.clear();
        *discarding_oversized_event = true;
    }
    parsed_data_event
}

/// 返回事件里是否有可解析的 JSON 数据(注释/空事件/`[DONE]` 返回 false)。
fn parse_sse_event(agent: RouterAgent, bytes: &[u8], usage: &mut TokenUsage) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data.trim() == "[DONE]" {
        return false;
    }
    match serde_json::from_str::<Value>(&data) {
        Ok(value) => {
            absorb_value(agent, &value, usage);
            true
        }
        Err(_) => false,
    }
}

fn absorb_value(agent: RouterAgent, value: &Value, destination: &mut TokenUsage) {
    absorb_envelope(agent, value, destination);
    for key in ["response", "message"] {
        if let Some(nested) = value.get(key) {
            absorb_envelope(agent, nested, destination);
        }
    }
}

fn absorb_envelope(_agent: RouterAgent, envelope: &Value, destination: &mut TokenUsage) {
    if let Some(model) = nonempty_string(envelope.get("model")) {
        destination.model = Some(model.to_string());
    }
    if let Some(response_id) = nonempty_string(envelope.get("id")) {
        destination.response_id = Some(response_id.to_string());
    }

    // `"usage": null` (OpenAI's stream chunks before the last) is not a report.
    let Some(usage) = envelope.get("usage").filter(|usage| usage.is_object()) else {
        return;
    };
    destination.reported = true;
    if let Some(split) = usage.get("cache_creation") {
        if let Some(value) = split
            .get("ephemeral_5m_input_tokens")
            .and_then(Value::as_u64)
        {
            destination.cache_write_5m = Some(value);
        }
        if let Some(value) = split
            .get("ephemeral_1h_input_tokens")
            .and_then(Value::as_u64)
        {
            destination.cache_write_1h = Some(value);
        }
    }
    if let Some(value) = first_u64(usage, &["input_tokens", "prompt_tokens"]) {
        destination.input_tokens = value;
    }
    if let Some(value) = first_u64(usage, &["output_tokens", "completion_tokens"]) {
        destination.output_tokens = value;
    }
    if let Some(value) = first_u64(usage, &["cache_read_input_tokens", "cached_tokens"])
        .or_else(|| {
            usage
                .pointer("/input_tokens_details/cached_tokens")
                .and_then(Value::as_u64)
        })
        .or_else(|| {
            usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(Value::as_u64)
        })
        // DeepSeek Chat 的文档化缓存命中字段,放在链末兜底。官方端点目前把同值镜像进
        // 未文档化的 prompt_tokens_details.cached_tokens(上面已命中),所以只有当上游
        // 只发文档字段时(部分中转)这一步才生效;上游发任一标准字段时行为零变化。
        .or_else(|| usage.get("prompt_cache_hit_tokens").and_then(Value::as_u64))
    {
        destination.cache_read_tokens = value;
    }
    if let Some(value) = first_u64(
        usage,
        &["cache_creation_input_tokens", "cache_write_input_tokens"],
    )
    .or_else(|| {
        usage
            .pointer("/input_tokens_details/cache_write_tokens")
            .and_then(Value::as_u64)
    })
    .or_else(|| {
        usage
            .pointer("/prompt_tokens_details/cache_write_tokens")
            .and_then(Value::as_u64)
    }) {
        destination.cache_creation_tokens = value;
    }
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn first_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use uuid::Uuid;

    fn temp_database_path() -> PathBuf {
        crate::test_support::raise_open_file_limit();
        std::env::temp_dir().join(format!("aeroric-router-{}.sqlite3", Uuid::new_v4()))
    }

    fn remove_database(path: &Path) {
        for suffix in ["", "-wal", "-shm"] {
            let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
            let _ = fs::remove_file(candidate);
        }
    }

    #[test]
    fn parses_claude_json_usage() {
        let value = json!({
            "id": "msg_123",
            "model": "claude-sonnet-4-5",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 5,
                "cache_read_input_tokens": 7,
                "cache_creation_input_tokens": 3
            }
        });
        let mut capture = UsageCapture::new(RouterAgent::Claude, false, None);
        capture.push(serde_json::to_string(&value).unwrap().as_bytes());
        let usage = capture.finish();
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, 7);
        assert_eq!(usage.cache_creation_tokens, 3);
        assert_eq!(usage.response_id.as_deref(), Some("msg_123"));
        assert_eq!(usage.ttft_ms, None);
    }

    #[test]
    fn parses_split_codex_sse_usage() {
        let first = b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-5.6\"}}\n\n";
        let second = b"event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-5.6\",\"usage\":{\"input_tokens\":20,\"output_tokens\":8,\"input_tokens_details\":{\"cached_tokens\":4}}}}\r\n\r\n";
        let mut capture = UsageCapture::new(RouterAgent::Codex, true, None);
        capture.push(&first[..31]);
        capture.push(&first[31..]);
        capture.push(&second[..57]);
        capture.push(&second[57..]);
        let usage = capture.finish();
        assert_eq!(usage.input_tokens, 20);
        assert_eq!(usage.output_tokens, 8);
        assert_eq!(usage.cache_read_tokens, 4);
        assert_eq!(usage.response_id.as_deref(), Some("resp_123"));
        assert_eq!(usage.model.as_deref(), Some("gpt-5.6"));
    }

    #[test]
    fn parses_deepseek_documented_cache_hit_tokens() {
        // 只发 DeepSeek 文档字段、不镜像 prompt_tokens_details.cached_tokens 的上游
        // (部分中转如此),缓存命中不能被记成 0。
        let value = json!({
            "id": "chat_123",
            "model": "deepseek-chat",
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 100,
                "prompt_cache_hit_tokens": 600,
                "prompt_cache_miss_tokens": 400
            }
        });
        let mut capture = UsageCapture::new(RouterAgent::Codex, false, None);
        capture.push(serde_json::to_string(&value).unwrap().as_bytes());
        let usage = capture.finish();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.cache_read_tokens, 600);
        assert_eq!(usage.cache_creation_tokens, 0);
    }

    #[test]
    fn prefers_standard_cache_field_over_deepseek_fallback() {
        // 上游同时发标准字段与文档字段时,标准字段优先 —— 兜底位于链末,不改变既有路径。
        let value = json!({
            "model": "deepseek-chat",
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 100,
                "prompt_tokens_details": {"cached_tokens": 512},
                "prompt_cache_hit_tokens": 600
            }
        });
        let mut capture = UsageCapture::new(RouterAgent::Codex, false, None);
        capture.push(serde_json::to_string(&value).unwrap().as_bytes());
        let usage = capture.finish();
        assert_eq!(usage.cache_read_tokens, 512);
    }

    /// C4: a usage object is a report; `"usage": null` (OpenAI's stream
    /// chunks before the last) and no usage at all are not; Anthropic's
    /// cache-write split is kept apart.
    #[test]
    fn only_a_usage_object_counts_as_reported_and_cache_writes_split() {
        let mut usage = TokenUsage::default();
        absorb_value(
            RouterAgent::Codex,
            &json!({"id": "c", "usage": null}),
            &mut usage,
        );
        assert!(!usage.reported);
        absorb_value(
            RouterAgent::Claude,
            &json!({"message": {"usage": {
                "input_tokens": 3,
                "cache_creation_input_tokens": 9,
                "cache_creation": {"ephemeral_5m_input_tokens": 4, "ephemeral_1h_input_tokens": 5}
            }}}),
            &mut usage,
        );
        assert!(usage.reported);
        assert_eq!(usage.cache_creation_tokens, 9);
        assert_eq!(
            (usage.cache_write_5m, usage.cache_write_1h),
            (Some(4), Some(5))
        );

        let mut total_only = TokenUsage::default();
        absorb_value(
            RouterAgent::Claude,
            &json!({"usage": {"cache_creation_input_tokens": 9}}),
            &mut total_only,
        );
        assert_eq!(
            (total_only.cache_write_5m, total_only.cache_write_1h),
            (None, None),
            "only the total: not split by guess"
        );
    }

    fn record(id: &str, completed_at: i64, source: UsageSource, input: u64) -> RouterRequestRecord {
        RouterRequestRecord {
            request_id: id.to_string(),
            session_id: None,
            response_id: None,
            agent: RouterAgent::Claude,
            target_id: Some("b".to_string()),
            target_name: Some("B".to_string()),
            endpoint: "/v1/messages".to_string(),
            attempt_count: 2,
            model: "m".to_string(),
            outbound_model: Some("m".to_string()),
            input_tokens: input,
            output_tokens: 1,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            status_code: 200,
            latency_ms: 5,
            started_at: completed_at - 5,
            completed_at,
            is_streaming: false,
            success: true,
            error_summary: None,
            ttft_ms: None,
            usage_source: source,
            cache_write_5m_tokens: Some(1),
            cache_write_1h_tokens: None,
            attempts: vec![
                RouterAttempt {
                    seq: 1,
                    target_id: Some("a".to_string()),
                    status_code: Some(429),
                    usage_source: UsageSource::Unknown,
                    error_summary: Some("slow down".to_string()),
                    ..RouterAttempt::default()
                },
                RouterAttempt {
                    seq: 2,
                    target_id: Some("b".to_string()),
                    status_code: Some(200),
                    input_tokens: input,
                    output_tokens: 1,
                    usage_source: source,
                    ..RouterAttempt::default()
                },
            ],
            task_id: None,
            purpose: String::new(),
        }
    }

    /// E7: requests grouped by purpose; the try that failed before the
    /// answering one is counted apart, with its tokens.
    #[tokio::test]
    async fn usage_is_grouped_by_purpose_with_failed_tries_apart() {
        let dir = std::env::temp_dir().join(format!("aeroric-purpose-{}", uuid::Uuid::new_v4()));
        let store = UsageStore::new(dir.join("router.db"));
        store.initialize().await.unwrap();
        for (id, purpose, task) in [
            ("r1", purpose::ANSWER, Some("t1")),
            ("r2", purpose::ANSWER, Some("t1")),
            ("r3", purpose::REVIEW, Some("t2")),
            ("r4", purpose::UNATTRIBUTED, None),
        ] {
            let mut row = record(id, 1_000, UsageSource::Reported, 10);
            row.purpose = purpose.to_string();
            row.task_id = task.map(str::to_string);
            row.attempts[0].input_tokens = 7;
            store.insert(row).await.unwrap();
        }
        let purposes = store.purposes(0).await.unwrap();
        let answer = &purposes[0];
        assert_eq!(answer.purpose, purpose::ANSWER);
        assert_eq!((answer.requests, answer.input_tokens), (2, 20));
        assert_eq!(
            (answer.failed_attempts, answer.failed_attempt_input_tokens),
            (2, 14),
            "the 429 try before each answer"
        );
        assert_eq!(purposes.len(), 3);
        assert!(store.purposes(2_000).await.unwrap().is_empty());
        let back = store.recent_requests(10).await.unwrap();
        let review = back.iter().find(|r| r.request_id == "r3").unwrap();
        assert_eq!(
            (review.purpose.as_str(), review.task_id.as_deref()),
            (purpose::REVIEW, Some("t2"))
        );
    }

    #[test]
    fn a_request_without_a_task_is_unattributed() {
        assert_eq!(purpose_of(None), purpose::UNATTRIBUTED);
    }

    /// C4: each try is stored with its request and read back in order; the
    /// sums leave out requests whose usage is unknown and count them.
    #[tokio::test]
    async fn tries_are_kept_and_unknown_usage_is_counted_not_summed() {
        let path = temp_database_path();
        let store = UsageStore::new(path.clone());
        store.initialize().await.unwrap();
        let now = unix_millis();
        store
            .insert(record("known", now, UsageSource::Reported, 10))
            .await
            .unwrap();
        store
            .insert(record("lost", now + 1, UsageSource::Unknown, 0))
            .await
            .unwrap();

        let summary = store.summary().await.unwrap();
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.input_tokens, 10);
        assert_eq!(summary.unknown_usage_requests, 1);
        assert_eq!(summary.cache_write_5m_tokens, 2);
        assert_eq!(summary.cache_write_1h_tokens, 0);

        let recent = store.recent_requests(10).await.unwrap();
        let lost = recent.iter().find(|r| r.request_id == "lost").unwrap();
        assert_eq!(lost.usage_source, UsageSource::Unknown);
        let seqs: Vec<_> = lost
            .attempts
            .iter()
            .map(|a| (a.seq, a.status_code, a.usage_source))
            .collect();
        assert_eq!(
            seqs,
            [
                (1, Some(429), UsageSource::Unknown),
                (2, Some(200), UsageSource::Unknown)
            ]
        );
        let known = recent.iter().find(|r| r.request_id == "known").unwrap();
        assert_eq!(known.attempts[1].input_tokens, 10);
        assert_eq!(
            known.attempts[0].error_summary.as_deref(),
            Some("slow down")
        );
        remove_database(&path);
    }

    #[tokio::test]
    async fn stores_only_request_metadata_and_summarizes_it() {
        let path = temp_database_path();
        let store = UsageStore::new(path.clone());
        store.initialize().await.unwrap();
        store
            .insert(RouterRequestRecord {
                request_id: "request-1".to_string(),
                session_id: Some("session-1".to_string()),
                response_id: Some("resp-1".to_string()),
                agent: RouterAgent::Codex,
                target_id: Some("codex".to_string()),
                target_name: Some("Codex".to_string()),
                endpoint: "/v1/responses".to_string(),
                attempt_count: 1,
                model: "gpt-5.6".to_string(),
                outbound_model: Some("gpt-5.6".to_string()),
                input_tokens: 10,
                output_tokens: 2,
                cache_creation_tokens: 0,
                cache_read_tokens: 4,
                status_code: 200,
                latency_ms: 15,
                started_at: 1,
                completed_at: 16,
                is_streaming: true,
                success: true,
                error_summary: None,
                ttft_ms: Some(6),
                usage_source: UsageSource::Reported,
                cache_write_5m_tokens: None,
                cache_write_1h_tokens: None,
                attempts: Vec::new(),
                task_id: None,
                purpose: String::new(),
            })
            .await
            .unwrap();

        let summary = store.summary().await.unwrap();
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.input_tokens, 10);
        assert_eq!(summary.cache_read_tokens, 4);
        let recent = store.recent_requests(10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].session_id.as_deref(), Some("session-1"));
        assert_eq!(recent[0].ttft_ms, Some(6));

        let connection = Connection::open(&path).unwrap();
        let mut statement = connection
            .prepare("PRAGMA table_info(router_requests)")
            .unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!columns.iter().any(|column| matches!(
            column.as_str(),
            "body" | "request_body" | "response_body" | "api_key" | "authorization"
        )));
        drop(statement);
        drop(connection);
        remove_database(&path);
    }

    #[test]
    fn error_summaries_are_single_line_and_bounded() {
        let summary = sanitize_summary(&format!("upstream\n failed {}", "x".repeat(600)));
        assert!(!summary.contains('\n'));
        assert!(summary.chars().count() <= MAX_ERROR_CHARS);
    }

    fn compressed_json_usage(agent: RouterAgent, encoding: &str, payload: &[u8]) -> TokenUsage {
        let mut capture = UsageCapture::new(agent, false, Some(encoding));
        capture.push(payload);
        capture.finish()
    }

    #[test]
    fn decompresses_gzip_usage_response() {
        let json = br#"{"usage":{"input_tokens":11,"output_tokens":7}}"#;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        encoder.write_all(json).unwrap();
        let payload = encoder.finish().unwrap();
        let usage = compressed_json_usage(RouterAgent::Claude, "gzip", &payload);
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 7);
    }

    #[test]
    fn decompresses_brotli_usage_response() {
        use std::io::Write;
        let json = br#"{"usage":{"input_tokens":42,"output_tokens":3}}"#;
        let mut writer = brotli::CompressorWriter::new(Vec::new(), 4096, 6, 22);
        writer.write_all(json).unwrap();
        writer.flush().unwrap();
        let payload = writer.into_inner();
        let usage = compressed_json_usage(RouterAgent::Claude, "br", &payload);
        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.output_tokens, 3);
    }

    #[test]
    fn decompresses_zstd_usage_response() {
        let json = br#"{"usage":{"input_tokens":99,"output_tokens":1}}"#;
        let payload = zstd::encode_all(json.as_slice(), 3).unwrap();
        let usage = compressed_json_usage(RouterAgent::Claude, "zstd", &payload);
        assert_eq!(usage.input_tokens, 99);
        assert_eq!(usage.output_tokens, 1);
    }

    fn streaming_record(
        request_id: &str,
        completed_at: i64,
        ttft_ms: Option<u64>,
    ) -> RouterRequestRecord {
        RouterRequestRecord {
            request_id: request_id.to_string(),
            session_id: None,
            response_id: None,
            agent: RouterAgent::Claude,
            target_id: Some("claude".to_string()),
            target_name: Some("Claude".to_string()),
            endpoint: "/v1/messages".to_string(),
            attempt_count: 1,
            model: "claude-sonnet-4-5".to_string(),
            outbound_model: None,
            input_tokens: 9,
            output_tokens: 7,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            status_code: 200,
            latency_ms: 80,
            started_at: completed_at - 80,
            completed_at,
            is_streaming: true,
            success: true,
            error_summary: None,
            ttft_ms,
            usage_source: UsageSource::Reported,
            cache_write_5m_tokens: None,
            cache_write_1h_tokens: None,
            attempts: Vec::new(),
            task_id: None,
            purpose: String::new(),
        }
    }

    #[test]
    fn records_ttft_at_first_streaming_data_event() {
        // 首字延迟从请求发起时刻起算,落在第一个带数据的事件上:心跳注释不算首字,
        // 之后继续到达的事件也不能把它推后。
        let started = Instant::now();
        let mut capture =
            UsageCapture::new(RouterAgent::Claude, true, None).with_request_start(Some(started));
        std::thread::sleep(Duration::from_millis(20));
        capture.push(b": ping\n\n");
        std::thread::sleep(Duration::from_millis(20));
        capture.push(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":9,\"output_tokens\":0}}}\n\n",
        );
        std::thread::sleep(Duration::from_millis(40));
        capture.push(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\n",
        );
        let latency_ms = elapsed_millis(started);
        let usage = capture.finish();

        assert_eq!(usage.output_tokens, 7);
        let ttft_ms = usage.ttft_ms.expect("流式响应必须记下首字延迟");
        assert!(
            ttft_ms >= 40,
            "首字延迟不能早于第一个数据事件抵达时刻:{ttft_ms}"
        );
        assert!(
            ttft_ms <= latency_ms,
            "首字延迟({ttft_ms})不能超过总延迟({latency_ms})"
        );
        assert!(
            ttft_ms < latency_ms,
            "首字延迟被记成了整段延迟:ttft={ttft_ms} latency={latency_ms}"
        );
    }

    #[test]
    fn omits_ttft_for_non_streaming_capture() {
        // 非流式响应一次性返回,没有首字概念,该列必须留空。
        let mut capture = UsageCapture::new(RouterAgent::Claude, false, None)
            .with_request_start(Some(Instant::now()));
        capture.push(br#"{"usage":{"input_tokens":4,"output_tokens":2}}"#);
        let usage = capture.finish();
        assert_eq!(usage.output_tokens, 2);
        assert_eq!(usage.ttft_ms, None);
    }

    #[test]
    fn omits_ttft_when_request_start_is_unknown() {
        // 拿不到计时基准(请求已收尾)时不能凭空造一个首字延迟。
        let mut capture = UsageCapture::new(RouterAgent::Claude, true, None);
        capture.push(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":3}}\n\n",
        );
        let usage = capture.finish();
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.ttft_ms, None);
    }

    #[tokio::test]
    async fn upgrades_legacy_database_without_ttft_column() {
        // 老库没有 ttft_ms 列:升级后读写都要正常,老记录该字段为 None。
        let path = temp_database_path();
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "
                    CREATE TABLE router_requests (
                        request_id TEXT PRIMARY KEY,
                        session_id TEXT,
                        response_id TEXT,
                        agent TEXT NOT NULL CHECK (agent IN ('claude', 'codex')),
                        target_id TEXT,
                        target_name TEXT,
                        endpoint TEXT NOT NULL DEFAULT '',
                        attempt_count INTEGER NOT NULL DEFAULT 0,
                        model TEXT NOT NULL,
                        outbound_model TEXT,
                        input_tokens INTEGER NOT NULL DEFAULT 0,
                        output_tokens INTEGER NOT NULL DEFAULT 0,
                        cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                        cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                        status_code INTEGER NOT NULL,
                        latency_ms INTEGER NOT NULL,
                        started_at INTEGER NOT NULL,
                        completed_at INTEGER NOT NULL,
                        is_streaming INTEGER NOT NULL CHECK (is_streaming IN (0, 1)),
                        success INTEGER NOT NULL CHECK (success IN (0, 1)),
                        error_summary TEXT
                    );
                    INSERT INTO router_requests (
                        request_id, agent, endpoint, attempt_count, model,
                        input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                        status_code, latency_ms, started_at, completed_at, is_streaming, success
                    ) VALUES (
                        'legacy-1', 'claude', '/v1/messages', 1, 'claude-sonnet-4-5',
                        5, 3, 0, 0, 200, 40, 1, 41, 1, 1
                    );
                    ",
                )
                .unwrap();
        }

        let store = UsageStore::new(path.clone());
        store.initialize().await.unwrap();
        store
            .insert(streaming_record("upgraded-1", 200, Some(12)))
            .await
            .unwrap();

        let recent = store.recent_requests(10).await.unwrap();
        assert_eq!(recent.len(), 2);
        let legacy = recent
            .iter()
            .find(|record| record.request_id == "legacy-1")
            .expect("老记录必须仍然读得出来");
        assert_eq!(legacy.ttft_ms, None);
        assert_eq!(legacy.output_tokens, 3);
        // C4: an old row cannot say where its usage came from: it reads as
        // reported, with no tries and no cache split.
        assert_eq!(legacy.usage_source, UsageSource::Reported);
        assert!(legacy.attempts.is_empty());
        assert_eq!(legacy.cache_write_5m_tokens, None);
        let upgraded = recent
            .iter()
            .find(|record| record.request_id == "upgraded-1")
            .expect("升级后必须能写入带首字延迟的新记录");
        assert_eq!(upgraded.ttft_ms, Some(12));

        remove_database(&path);
    }
}
