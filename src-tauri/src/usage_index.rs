use notify::{RecursiveMode, Watcher};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

use crate::analytics::{self, UsageAgent, UsageRequest};

const INDEX_EVENT: &str = "usage-statistics-updated";
const EVENT_DEBOUNCE: Duration = Duration::from_millis(500);
/// 漏事件的兜底重扫间隔。正常路径走 FSEvents,不依赖这个值。
///
/// 原来是 5 s。每次扫描要遍历 usage 根下上千个 jsonl 并逐个 canonicalize + stat
/// （本机实测 1404 个文件、约 1.8 s 墙钟）,5 s 一轮意味着这个线程基本没停过。
/// 兜底不需要这么勤。
const FALLBACK_SCAN_INTERVAL: Duration = Duration::from_secs(30);
/// 同一个来源被重解析的最小间隔,见 `should_reparse`。
const REPARSE_THROTTLE: Duration = Duration::from_secs(30);
const MAX_DECOMPRESSED_USAGE_LOG_BYTES: usize = 512 * 1024 * 1024;
const MAX_USAGE_LOG_LINE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceState {
    modified_ns: i64,
    size: i64,
}

fn database_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("usage-statistics.sqlite3"))
}

fn open_database() -> Result<Connection, String> {
    crate::storage::ensure_aeroric_dirs()?;
    let connection = Connection::open(database_path()?).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|error| error.to_string())?;
    initialize_database(&connection)?;
    Ok(connection)
}

fn initialize_database(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS usage_sources (
                path TEXT PRIMARY KEY,
                modified_ns INTEGER NOT NULL,
                size INTEGER NOT NULL,
                indexed_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS usage_requests (
                source_path TEXT NOT NULL,
                request_index INTEGER NOT NULL,
                timestamp REAL NOT NULL,
                date TEXT NOT NULL,
                agent TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                PRIMARY KEY (source_path, request_index)
            );
            CREATE INDEX IF NOT EXISTS usage_requests_date_agent
                ON usage_requests (date, agent);
            ",
        )
        .map_err(|error| error.to_string())
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn source_state(path: &Path) -> Option<SourceState> {
    let metadata = fs::metadata(path).ok()?;
    let modified_ns = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64;
    Some(SourceState {
        modified_ns,
        size: metadata.len().min(i64::MAX as u64) as i64,
    })
}

/// 库里记着的一条来源:内容指纹 + 上次入库时刻。
///
/// `indexed_at` 是节流用的:活跃会话的 jsonl 每条消息都在追加,指纹每次都不同,
/// 而重解析是整文件（当前最大的会话文件 85 MB,实测读+解析约 1.2 s）。没有节流
/// 时它会被反复整份重解析,把一个核吃满。
#[derive(Clone, Copy, Debug)]
struct IndexedSource {
    state: SourceState,
    indexed_at: i64,
}

fn load_source_states(connection: &Connection) -> Result<HashMap<String, IndexedSource>, String> {
    let mut statement = connection
        .prepare("SELECT path, modified_ns, size, indexed_at FROM usage_sources")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                IndexedSource {
                    state: SourceState {
                        modified_ns: row.get(1)?,
                        size: row.get(2)?,
                    },
                    indexed_at: row.get(3)?,
                },
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut states = HashMap::new();
    for row in rows {
        let (path, state) = row.map_err(|error| error.to_string())?;
        states.insert(path, state);
    }
    Ok(states)
}

fn read_limited_line<R: BufRead>(reader: &mut R, buffer: &mut Vec<u8>) -> io::Result<usize> {
    buffer.clear();
    let mut limited = reader.take((MAX_USAGE_LOG_LINE_BYTES + 1) as u64);
    let read = limited.read_until(b'\n', buffer)?;
    if read > MAX_USAGE_LOG_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "usage log line exceeds safety limit",
        ));
    }
    Ok(read)
}

fn parse_dsh_usage_reader<R: BufRead>(
    mut reader: R,
    max_bytes: usize,
) -> io::Result<Option<Vec<UsageRequest>>> {
    let mut line = Vec::new();
    let first_bytes = read_limited_line(&mut reader, &mut line)?;
    if first_bytes == 0 {
        return Ok(None);
    }
    if first_bytes > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "usage log exceeds decompression safety limit",
        ));
    }
    let first_line = std::str::from_utf8(&line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if !analytics::is_dsh_session(first_line) {
        return Ok(None);
    }

    let mut requests = Vec::new();
    let mut current_model = String::new();
    let mut total_bytes = first_bytes;
    if let Some(request) = analytics::parse_dsh_usage_line(first_line, &mut current_model) {
        requests.push(request);
    }
    loop {
        let read = read_limited_line(&mut reader, &mut line)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes
            .checked_add(read)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "usage log size overflow"))?;
        if total_bytes > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "usage log exceeds decompression safety limit",
            ));
        }
        let line = std::str::from_utf8(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(request) = analytics::parse_dsh_usage_line(line, &mut current_model) {
            requests.push(request);
        }
    }
    Ok(Some(requests))
}

fn parse_source(path: &Path) -> Option<Vec<UsageRequest>> {
    if analytics::is_zstd_usage_log(path) {
        let file = fs::File::open(path).ok()?;
        let decoder = zstd::stream::read::Decoder::new(file).ok()?;
        return parse_dsh_usage_reader(BufReader::new(decoder), MAX_DECOMPRESSED_USAGE_LOG_BYTES)
            .ok()
            .flatten();
    }

    let file = fs::File::open(path).ok()?;
    match parse_dsh_usage_reader(BufReader::new(file), MAX_DECOMPRESSED_USAGE_LOG_BYTES) {
        Ok(Some(requests)) => return Some(requests),
        Ok(None) => {}
        Err(_) => return None,
    }
    if fs::metadata(path).ok()?.len() > MAX_DECOMPRESSED_USAGE_LOG_BYTES as u64 {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    // omp 的判定谓词更具体(title 槽/version:3),放在 dsh(接受任意数字
    // version 的 {type:"session"} 头)之前,避免无 title 槽的 omp 文件被 dsh 吞掉。
    if analytics::is_omp_session(&content) {
        Some(analytics::parse_omp_usage_requests(&content))
    } else if analytics::is_dsh_session(&content) {
        Some(analytics::parse_dsh_usage_requests(&content))
    } else if analytics::is_codex_session(&content) {
        Some(analytics::parse_codex_usage_requests(&content))
    } else {
        Some(analytics::parse_claude_usage_requests(
            &content,
            &path.to_string_lossy(),
        ))
    }
}

fn as_sql_integer(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

/// [`UsageAgent`] → `agent` 列的字面量。与 [`parse_usage_agent`] 互为逆向。
///
/// 这两个 match 是库里 `agent` 列的唯一值域来源:写入只可能产出这四个字面量,所以读取
/// 认不出来就一定是旧行或坏行,不是新家族。
fn usage_agent_column(agent: UsageAgent) -> &'static str {
    match agent {
        UsageAgent::Codex => "codex",
        UsageAgent::Claude => "claude",
        UsageAgent::Dsh => "dsh",
        UsageAgent::Omp => "omp",
    }
}

fn replace_source(
    connection: &mut Connection,
    path: &str,
    state: SourceState,
    requests: &[UsageRequest],
) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM usage_requests WHERE source_path = ?1",
            params![path],
        )
        .map_err(|error| error.to_string())?;
    {
        let mut insert = transaction
            .prepare(
                "
                INSERT INTO usage_requests (
                    source_path,
                    request_index,
                    timestamp,
                    date,
                    agent,
                    model,
                    input_tokens,
                    output_tokens,
                    cache_creation_tokens,
                    cache_read_tokens
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ",
            )
            .map_err(|error| error.to_string())?;
        for (index, request) in requests.iter().enumerate() {
            insert
                .execute(params![
                    path,
                    index.min(i64::MAX as usize) as i64,
                    request.timestamp,
                    request.date.to_string(),
                    usage_agent_column(request.agent),
                    request.model,
                    as_sql_integer(request.input_tokens),
                    as_sql_integer(request.output_tokens),
                    as_sql_integer(request.cache_creation_tokens),
                    as_sql_integer(request.cache_read_tokens),
                ])
                .map_err(|error| error.to_string())?;
        }
    }
    transaction
        .execute(
            "
            INSERT INTO usage_sources (path, modified_ns, size, indexed_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(path) DO UPDATE SET
                modified_ns = excluded.modified_ns,
                size = excluded.size,
                indexed_at = excluded.indexed_at
            ",
            params![path, state.modified_ns, state.size, unix_millis()],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn remove_sources(connection: &mut Connection, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    for path in paths {
        transaction
            .execute(
                "DELETE FROM usage_requests WHERE source_path = ?1",
                params![path],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute("DELETE FROM usage_sources WHERE path = ?1", params![path])
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

/// 这一轮要不要重解析这个来源。
///
/// 三档:
/// - 库里没有 → 必须解析。
/// - 指纹没变 → 内容没动,跳过。
/// - 指纹变了 → 只有距上次入库超过 `REPARSE_THROTTLE` 才解析。
///
/// 为什么"变了也可能跳过":统计口径是「按天累计的 token」,晚几十秒入库对读数没有
/// 可见影响;而活跃会话每条消息都在追加,不节流就是每 500 ms 重解析一次整个文件。
/// 跳过不会丢数据 —— run_loop 的兜底扫描会再来,届时节流窗口已过。
///
/// `force = true` 时无条件解析,给显式刷新命令用（用户点了"刷新"就该立刻看到）。
fn should_reparse(
    indexed: Option<&IndexedSource>,
    current: SourceState,
    now: i64,
    force: bool,
) -> bool {
    let Some(previous) = indexed else {
        return true;
    };
    if previous.state == current {
        return false;
    }
    if force {
        return true;
    }
    now.saturating_sub(previous.indexed_at) >= REPARSE_THROTTLE.as_millis() as i64
}

pub(crate) fn refresh_index() -> Result<bool, String> {
    refresh_index_inner(false)
}

fn refresh_index_inner(force: bool) -> Result<bool, String> {
    let mut connection = open_database()?;
    let indexed = load_source_states(&connection)?;
    let mut files = HashSet::new();
    for root in analytics::usage_roots() {
        analytics::collect_jsonl_files(&root, &mut files);
    }

    let now = unix_millis();
    let mut changed = false;
    let mut seen = HashSet::new();
    for path in files {
        let canonical = path.to_string_lossy().into_owned();
        seen.insert(canonical.clone());
        let Some(state) = source_state(&path) else {
            continue;
        };
        if !should_reparse(indexed.get(&canonical), state, now, force) {
            continue;
        }
        let Some(requests) = parse_source(&path) else {
            continue;
        };
        replace_source(&mut connection, &canonical, state, &requests)?;
        changed = true;
    }

    let removed = indexed
        .keys()
        .filter(|path| !seen.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    if !removed.is_empty() {
        remove_sources(&mut connection, &removed)?;
        changed = true;
    }
    Ok(changed)
}

/// `agent` 列的字面量 → [`UsageAgent`]。与 [`replace_source`] 里写库的那个 match 互为
/// 逆向,两边必须同时改。
///
/// 认不出来回 `None`,调用方**跳过这一行**而不是归到某一族。原来这里是
/// `_ => UsageAgent::Claude`:库里只可能出现上面那四个字面量(写入侧同一个 match 产出),
/// 所以认不出来意味着降级回滚留下的旧行或库被写坏 —— 那种行记到 claude 名下,用户看到的
/// 是 claude 用量凭空变多,而且和 breakdown 四族之和对不上。宁可少算不可错算。
fn parse_usage_agent(value: &str) -> Option<UsageAgent> {
    match value {
        "codex" => Some(UsageAgent::Codex),
        "claude" => Some(UsageAgent::Claude),
        "dsh" => Some(UsageAgent::Dsh),
        "omp" => Some(UsageAgent::Omp),
        _ => None,
    }
}

pub(crate) fn load_requests(
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<UsageRequest>, String> {
    let connection = open_database()?;
    load_requests_from(&connection, from, to)
}

fn load_requests_from(
    connection: &Connection,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<UsageRequest>, String> {
    let mut statement = connection
        .prepare(
            "
            SELECT
                timestamp,
                date,
                agent,
                model,
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens
            FROM usage_requests
            WHERE date >= ?1 AND date <= ?2
            ORDER BY timestamp ASC
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![from.to_string(), to.to_string()], |row| {
            let date = row
                .get::<_, String>(1)?
                .parse::<chrono::NaiveDate>()
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            let Some(agent) = parse_usage_agent(&row.get::<_, String>(2)?) else {
                return Ok(None);
            };
            Ok(Some(UsageRequest {
                timestamp: row.get(0)?,
                date,
                agent,
                model: row.get(3)?,
                input_tokens: row.get::<_, i64>(4)?.max(0) as u64,
                output_tokens: row.get::<_, i64>(5)?.max(0) as u64,
                cache_creation_tokens: row.get::<_, i64>(6)?.max(0) as u64,
                cache_read_tokens: row.get::<_, i64>(7)?.max(0) as u64,
            }))
        })
        .map_err(|error| error.to_string())?;
    let mut requests = Vec::new();
    for row in rows {
        // 认不出 agent 的行在这里被丢掉,不进任何一族的统计。
        if let Some(request) = row.map_err(|error| error.to_string())? {
            requests.push(request);
        }
    }
    Ok(requests)
}

pub(crate) fn latest_updated_at() -> Result<i64, String> {
    let connection = open_database()?;
    connection
        .query_row("SELECT MAX(indexed_at) FROM usage_sources", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()
        .map(|value| value.flatten().unwrap_or(0))
        .map_err(|error| error.to_string())
}

fn emit_updated(app: &AppHandle) {
    let _ = app.emit(
        INDEX_EVENT,
        serde_json::json!({ "updatedAt": latest_updated_at().unwrap_or_default() }),
    );
}

fn sync_watched_roots(
    watcher: &mut Option<notify::RecommendedWatcher>,
    watched: &mut HashSet<PathBuf>,
) {
    let Some(watcher) = watcher.as_mut() else {
        return;
    };
    for root in analytics::usage_roots() {
        if watched.insert(root.clone()) && watcher.watch(&root, RecursiveMode::Recursive).is_err() {
            watched.remove(&root);
        }
    }
}

fn run_loop(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default()).ok();
    let mut watched = HashSet::new();

    loop {
        sync_watched_roots(&mut watcher, &mut watched);
        if refresh_index().unwrap_or(false) {
            emit_updated(&app);
        }

        if watcher.is_some() {
            match rx.recv_timeout(FALLBACK_SCAN_INTERVAL) {
                Ok(_) => thread::sleep(EVENT_DEBOUNCE),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => watcher = None,
            }
            while rx.try_recv().is_ok() {}
        } else {
            thread::sleep(FALLBACK_SCAN_INTERVAL);
        }
    }
}

pub(crate) fn start(app: AppHandle) {
    thread::spawn(move || run_loop(app));
}

#[tauri::command]
pub(crate) async fn refresh_usage_statistics_index(app: AppHandle) -> Result<bool, String> {
    // 显式刷新绕过节流:用户点了刷新就该立刻看到最新数字。
    let changed = tokio::task::spawn_blocking(|| refresh_index_inner(true))
        .await
        .map_err(|error| format!("refresh_usage_statistics_index join error: {error}"))??;
    if changed {
        emit_updated(&app);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn indexed(size: i64, indexed_at: i64) -> IndexedSource {
        IndexedSource {
            state: SourceState {
                modified_ns: size * 1000,
                size,
            },
            indexed_at,
        }
    }

    #[test]
    fn a_new_source_is_always_parsed() {
        let state = SourceState {
            modified_ns: 1,
            size: 10,
        };
        assert!(should_reparse(None, state, 0, false));
    }

    #[test]
    fn an_unchanged_source_is_never_reparsed() {
        let previous = indexed(10, 0);
        // 指纹相同 → 跳过,连 force 也不必解析(内容确实没动)。
        assert!(!should_reparse(
            Some(&previous),
            previous.state,
            10_000_000,
            false
        ));
        assert!(!should_reparse(
            Some(&previous),
            previous.state,
            10_000_000,
            true
        ));
    }

    /// 活跃会话每条消息都在追加。节流窗口内的变化要压住,否则就是反复整文件重解析。
    #[test]
    fn a_growing_source_is_throttled_then_reparsed() {
        let previous = indexed(10, 1_000);
        let grown = SourceState {
            modified_ns: 99_000,
            size: 20,
        };
        let window = REPARSE_THROTTLE.as_millis() as i64;

        assert!(!should_reparse(
            Some(&previous),
            grown,
            1_000 + window - 1,
            false
        ));
        assert!(should_reparse(
            Some(&previous),
            grown,
            1_000 + window,
            false
        ));
    }

    /// 显式刷新不受节流影响。
    #[test]
    fn force_bypasses_the_throttle_for_changed_sources() {
        let previous = indexed(10, 1_000);
        let grown = SourceState {
            modified_ns: 99_000,
            size: 20,
        };
        assert!(should_reparse(Some(&previous), grown, 1_001, true));
    }

    #[test]
    fn source_state_changes_when_file_grows() {
        let path = std::env::temp_dir().join(format!("aeroric-usage-{}.jsonl", Uuid::new_v4()));
        fs::write(&path, "{}\n").unwrap();
        let before = source_state(&path).unwrap();
        fs::write(&path, "{}\n{}\n").unwrap();
        let after = source_state(&path).unwrap();
        assert!(after.size > before.size);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn parses_zstd_compressed_dsh_sessions() {
        let path = std::env::temp_dir().join(format!(
            "aeroric-usage-{}.session.jsonl.zstd",
            Uuid::new_v4()
        ));
        let content = concat!(
            r#"{"type":"session","version":0,"id":"s1","createdAt":1755100000000}"#,
            "\n",
            r#"{"type":"request/context","seq":0,"time":1755100000100,"data":{"provider":"deepseek-official","model":"deepseek-v4"}}"#,
            "\n",
            r#"{"type":"assistant/message","seq":1,"time":1755100001000,"data":{"usage":{"inputTokens":100,"outputTokens":20,"cacheReadTokens":50,"cacheWriteTokens":5,"reasoningTokens":7}}}"#,
            "\n",
        );
        let compressed = zstd::stream::encode_all(content.as_bytes(), 1).unwrap();
        fs::write(&path, compressed).unwrap();

        let requests = parse_source(&path).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].agent, UsageAgent::Dsh);
        assert_eq!(requests[0].input_tokens, 100);
        assert_eq!(requests[0].output_tokens, 20);
        assert_eq!(requests[0].cache_read_tokens, 50);
        assert_eq!(requests[0].cache_creation_tokens, 5);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn streamed_dsh_parser_rejects_excessive_decompressed_data() {
        let content = concat!(
            r#"{"type":"session","version":0,"id":"s1","createdAt":1755100000000}"#,
            "\n",
            r#"{"type":"request/context","seq":0,"time":1755100000100,"data":{"provider":"deepseek-official","model":"deepseek-v4"}}"#,
            "\n",
            r#"{"type":"assistant/message","seq":1,"time":1755100001000,"data":{"usage":{"inputTokens":100,"outputTokens":20}}}"#,
            "\n",
        );
        let result = parse_dsh_usage_reader(
            BufReader::new(std::io::Cursor::new(content.as_bytes())),
            content.len() - 1,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_corrupt_zstd_without_producing_empty_usage() {
        let path = std::env::temp_dir().join(format!(
            "aeroric-usage-{}.session.jsonl.zstd",
            Uuid::new_v4()
        ));
        fs::write(&path, b"not-zstd").unwrap();
        assert!(parse_source(&path).is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_rewrite_does_not_replace_previously_indexed_requests() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = std::env::temp_dir().join(format!(
            "aeroric-usage-{}.session.jsonl.zstd",
            Uuid::new_v4()
        ));
        let source_path = path.to_string_lossy().into_owned();
        let request = UsageRequest {
            timestamp: 1.0,
            date: chrono::NaiveDate::from_ymd_opt(2026, 8, 19).unwrap(),
            agent: UsageAgent::Dsh,
            model: "deepseek-v4".to_owned(),
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 2,
        };
        replace_source(
            &mut connection,
            &source_path,
            SourceState {
                modified_ns: 1,
                size: 100,
            },
            &[request],
        )
        .unwrap();

        fs::write(&path, b"partial-zstd-frame").unwrap();
        assert!(parse_source(&path).is_none());
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE source_path = ?1",
                params![source_path],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn replacing_a_source_does_not_duplicate_requests() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let state = SourceState {
            modified_ns: 1,
            size: 100,
        };
        let request = UsageRequest {
            timestamp: 1.0,
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 16).unwrap(),
            agent: UsageAgent::Codex,
            model: "gpt-5.5".to_owned(),
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 2,
        };

        replace_source(
            &mut connection,
            "/tmp/session.jsonl",
            state,
            std::slice::from_ref(&request),
        )
        .unwrap();
        replace_source(
            &mut connection,
            "/tmp/session.jsonl",
            SourceState {
                modified_ns: 2,
                size: 120,
            },
            &[request],
        )
        .unwrap();

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── agent 列的读写映射 ─────────────────────────────────────────────────

    /// 下一个 [`UsageAgent`]。穷尽 `match`:新增一族时这里不编译,于是下面那条往返测试
    /// 一定会覆盖到它。
    fn next_agent(agent: UsageAgent) -> Option<UsageAgent> {
        match agent {
            UsageAgent::Codex => Some(UsageAgent::Claude),
            UsageAgent::Claude => Some(UsageAgent::Dsh),
            UsageAgent::Dsh => Some(UsageAgent::Omp),
            UsageAgent::Omp => None,
        }
    }

    fn all_agents() -> Vec<UsageAgent> {
        let mut agents = vec![UsageAgent::Codex];
        while let Some(next) = next_agent(*agents.last().expect("至少有 Codex")) {
            assert!(!agents.contains(&next), "next_agent 成环:{next:?}");
            agents.push(next);
        }
        agents
    }

    /// 写进去的每一族都要原样读回来。两个 match 反向不一致 = 用量记到别人名下。
    #[test]
    fn every_agent_survives_a_database_round_trip() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();

        let agents = all_agents();
        assert_eq!(agents.len(), 4, "清单漏了 agent");
        for (index, agent) in agents.iter().enumerate() {
            replace_source(
                &mut connection,
                &format!("/tmp/session-{index}.jsonl"),
                SourceState {
                    modified_ns: 1,
                    size: 10,
                },
                &[UsageRequest {
                    timestamp: index as f64,
                    date: day,
                    agent: *agent,
                    model: "m".to_owned(),
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                }],
            )
            .unwrap();
        }

        let loaded = load_requests_from(&connection, day, day).unwrap();
        assert_eq!(
            loaded
                .iter()
                .map(|request| request.agent)
                .collect::<Vec<_>>(),
            agents,
            "读回来的 agent 与写进去的不一致"
        );
    }

    /// 认不出的 agent 必须被跳过,而不是记到 claude 名下。
    ///
    /// 这种行只可能来自降级回滚留下的旧库或写坏的库(写入侧只产出四个已知字面量)。
    /// 原来的 `_ => UsageAgent::Claude` 会让它变成 claude 的用量:面板上 claude 凭空多出
    /// 一截,且总计与四族之和对不上。用裸 SQL 造这一行 —— 类型安全的写入路径造不出来。
    #[test]
    fn an_unrecognized_agent_is_skipped_instead_of_counted_as_claude() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();

        replace_source(
            &mut connection,
            "/tmp/known.jsonl",
            SourceState {
                modified_ns: 1,
                size: 10,
            },
            &[UsageRequest {
                timestamp: 1.0,
                date: day,
                agent: UsageAgent::Claude,
                model: "claude-sonnet-5".to_owned(),
                input_tokens: 7,
                output_tokens: 3,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            }],
        )
        .unwrap();
        connection
            .execute(
                "
                INSERT INTO usage_requests (
                    source_path, request_index, timestamp, date, agent, model,
                    input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens
                ) VALUES ('/tmp/mystery.jsonl', 0, 2.0, ?1, 'gemini', 'gemini-3', 999, 999, 0, 0)
                ",
                params![day.to_string()],
            )
            .unwrap();

        let loaded = load_requests_from(&connection, day, day).unwrap();
        assert_eq!(loaded.len(), 1, "认不出的行不该进结果");
        assert_eq!(loaded[0].agent, UsageAgent::Claude);
        assert_eq!(loaded[0].input_tokens, 7, "claude 的 input 不该被那行污染");
        assert_eq!(loaded[0].model, "claude-sonnet-5");
    }

    #[test]
    fn the_agent_column_mapping_is_reversible() {
        for agent in all_agents() {
            assert_eq!(
                parse_usage_agent(usage_agent_column(agent)),
                Some(agent),
                "{agent:?} 的读写映射不自洽"
            );
        }
        assert_eq!(parse_usage_agent("gemini"), None);
        assert_eq!(parse_usage_agent(""), None);
    }
}
