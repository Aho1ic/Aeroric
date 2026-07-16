use notify::{RecursiveMode, Watcher};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

use crate::analytics::{self, UsageAgent, UsageRequest};

const INDEX_EVENT: &str = "usage-statistics-updated";
const EVENT_DEBOUNCE: Duration = Duration::from_millis(500);
const FALLBACK_SCAN_INTERVAL: Duration = Duration::from_secs(5);

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

fn load_source_states(connection: &Connection) -> Result<HashMap<String, SourceState>, String> {
    let mut statement = connection
        .prepare("SELECT path, modified_ns, size FROM usage_sources")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                SourceState {
                    modified_ns: row.get(1)?,
                    size: row.get(2)?,
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

fn parse_source(path: &Path) -> Option<Vec<UsageRequest>> {
    let content = fs::read_to_string(path).ok()?;
    if analytics::is_codex_session(&content) {
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
                    match request.agent {
                        UsageAgent::Codex => "codex",
                        UsageAgent::Claude => "claude",
                    },
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

pub(crate) fn refresh_index() -> Result<bool, String> {
    let mut connection = open_database()?;
    let indexed = load_source_states(&connection)?;
    let mut files = HashSet::new();
    for root in analytics::usage_roots() {
        analytics::collect_jsonl_files(&root, &mut files);
    }

    let mut changed = false;
    let mut seen = HashSet::new();
    for path in files {
        let canonical = path.to_string_lossy().into_owned();
        seen.insert(canonical.clone());
        let Some(state) = source_state(&path) else {
            continue;
        };
        if indexed.get(&canonical) == Some(&state) {
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

pub(crate) fn load_requests(
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<UsageRequest>, String> {
    let connection = open_database()?;
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
            let agent = match row.get::<_, String>(2)?.as_str() {
                "codex" => UsageAgent::Codex,
                _ => UsageAgent::Claude,
            };
            Ok(UsageRequest {
                timestamp: row.get(0)?,
                date,
                agent,
                model: row.get(3)?,
                input_tokens: row.get::<_, i64>(4)?.max(0) as u64,
                output_tokens: row.get::<_, i64>(5)?.max(0) as u64,
                cache_creation_tokens: row.get::<_, i64>(6)?.max(0) as u64,
                cache_read_tokens: row.get::<_, i64>(7)?.max(0) as u64,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
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
    let changed = tokio::task::spawn_blocking(refresh_index)
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
            &[request.clone()],
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
}
