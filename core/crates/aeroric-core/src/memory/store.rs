//! The SQLite side of the memory store. Rows are only inserted or have
//! their state flipped; text never changes. `forget` is the one real delete
//! (items, evidence and index together) and it writes the barrier.

use std::path::Path;

use aeroric_protocol::memory::{
    Assertion, Confidence, Evidence, ExperienceDetail, MemoryCategory, MemoryDraft, MemoryItem,
    MemoryKind, MemoryState, MemoryStats,
};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::barrier::Barrier;
use super::{normalize, MemoryError};
use crate::notebook::rag::cjk;

const SCHEMA_VERSION: i64 = 2;
const COLUMNS: &str = "id, project, kind, category, text, assertion, state, pinned, \
                       supersedes, superseded_by, created_ms, updated_ms, detail, confidence, \
                       retrieved, injected, helpful, wrong";
/// B5-3a (schema 2): columns added to `items`.
const V2_COLUMNS: [(&str, &str); 6] = [
    ("detail", "TEXT"),
    ("confidence", "TEXT"),
    ("retrieved", "INTEGER NOT NULL DEFAULT 0"),
    ("injected", "INTEGER NOT NULL DEFAULT 0"),
    ("helpful", "INTEGER NOT NULL DEFAULT 0"),
    ("wrong", "INTEGER NOT NULL DEFAULT 0"),
];

pub(super) struct Store {
    conn: Connection,
    barrier: Barrier,
}

fn enc<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn dec<T: DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(text.to_string())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn row_item(row: &Row<'_>) -> rusqlite::Result<MemoryItem> {
    let project: String = row.get(1)?;
    let category: Option<String> = row.get(3)?;
    Ok(MemoryItem {
        id: row.get(0)?,
        project_id: (!project.is_empty()).then_some(project),
        kind: dec::<MemoryKind>(&row.get::<_, String>(2)?)?,
        category: category.map(|c| dec::<MemoryCategory>(&c)).transpose()?,
        text: row.get(4)?,
        assertion: dec::<Assertion>(&row.get::<_, String>(5)?)?,
        state: dec::<MemoryState>(&row.get::<_, String>(6)?)?,
        pinned: row.get::<_, i64>(7)? != 0,
        evidence: Vec::new(),
        supersedes: row.get(8)?,
        superseded_by: row.get(9)?,
        created_ms: row.get::<_, i64>(10)? as u64,
        updated_ms: row.get::<_, i64>(11)? as u64,
        experience: row
            .get::<_, Option<String>>(12)?
            .and_then(|text| serde_json::from_str(&text).ok()),
        confidence: row
            .get::<_, Option<String>>(13)?
            .map(|c| dec::<Confidence>(&c))
            .transpose()?,
        stats: MemoryStats {
            retrieved: row.get::<_, i64>(14)? as u32,
            injected: row.get::<_, i64>(15)? as u32,
            helpful: row.get::<_, i64>(16)? as u32,
            wrong: row.get::<_, i64>(17)? as u32,
        },
    })
}

impl Store {
    pub(super) fn open(dir: &Path) -> Result<Self, MemoryError> {
        std::fs::create_dir_all(dir).map_err(|e| MemoryError::Unavailable(e.to_string()))?;
        let conn = Connection::open(dir.join("memory.db"))?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Forgotten words are overwritten on disk, not just unlinked.
        conn.pragma_update(None, "secure_delete", "ON")?;
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS items(
                id TEXT PRIMARY KEY, project TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL,
                category TEXT, text TEXT NOT NULL, norm TEXT NOT NULL, assertion TEXT NOT NULL,
                state TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, supersedes TEXT,
                superseded_by TEXT, created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS items_scope ON items(project, state);
             CREATE TABLE IF NOT EXISTS evidence(
                item_id TEXT NOT NULL, task_id TEXT, harness TEXT, model TEXT,
                excerpt TEXT NOT NULL, hash TEXT NOT NULL, at_ms INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS evidence_item ON evidence(item_id);
             CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
                id UNINDEXED, body, tokenize = '{}');
             CREATE TABLE IF NOT EXISTS barrier(sig TEXT PRIMARY KEY, at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS projects(project_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', {SCHEMA_VERSION});
             CREATE TABLE IF NOT EXISTS kv(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, at_ms INTEGER NOT NULL, body TEXT NOT NULL,
                counted INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS runs_at ON runs(at_ms);
             CREATE TABLE IF NOT EXISTS extracted(task_id TEXT PRIMARY KEY, hash TEXT NOT NULL,
                at_ms INTEGER NOT NULL);",
            cjk::FTS_TOKENIZER
        ))?;
        // B5-3a: schema 1 → 2 only adds columns.
        let existing: Vec<String> = conn
            .prepare("PRAGMA table_info(items)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<_, _>>()?;
        for (name, kind) in V2_COLUMNS {
            if !existing.iter().any(|c| c == name) {
                conn.execute_batch(&format!("ALTER TABLE items ADD COLUMN {name} {kind}"))?;
            }
        }
        conn.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'schema_version' AND value < ?1",
            [SCHEMA_VERSION],
        )?;
        Ok(Self {
            conn,
            barrier: Barrier::open(dir)?,
        })
    }

    /// B5-3a: an experience's detail and confidence; its index body takes
    /// the problem, fix, keys and environments too.
    pub(super) fn set_experience(
        &mut self,
        id: &str,
        title: &str,
        detail: &ExperienceDetail,
        confidence: Confidence,
    ) -> Result<(), MemoryError> {
        let body = format!(
            "{title} {} {} {} {}",
            detail.problem,
            detail.solution,
            detail.search_keys.join(" "),
            detail.envs.join(" ")
        );
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE items SET detail = ?2, confidence = ?3 WHERE id = ?1",
            params![
                id,
                serde_json::to_string(detail).unwrap_or_default(),
                enc(&confidence)
            ],
        )?;
        tx.execute("DELETE FROM items_fts WHERE id = ?1", [id])?;
        tx.execute(
            "INSERT INTO items_fts(id, body) VALUES (?1, ?2)",
            params![id, cjk::segment(&body)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The active session clue a task's extraction left, if any.
    pub(super) fn session_fact_of(&self, task_id: &str) -> Result<Option<String>, MemoryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT i.id FROM items i JOIN evidence e ON e.item_id = i.id
                 WHERE i.kind = 'sessionFact' AND i.state = 'active' AND e.task_id = ?1
                 ORDER BY i.created_ms DESC LIMIT 1",
                [task_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// B5-3a: one more `retrieved` / `injected` for each id.
    pub(super) fn bump(&self, ids: &[String], column: &str) -> Result<(), MemoryError> {
        debug_assert!(matches!(column, "retrieved" | "injected"));
        for id in ids {
            self.conn.execute(
                &format!("UPDATE items SET {column} = {column} + 1 WHERE id = ?1"),
                [id],
            )?;
        }
        Ok(())
    }

    /// B5-3a: count the vote; an experience's confidence follows it (Q-M19).
    pub(super) fn feedback(
        &self,
        item: &MemoryItem,
        helpful: bool,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        let mut stats = item.stats;
        if helpful {
            stats.helpful += 1;
        } else {
            stats.wrong += 1;
        }
        let mut state = item.state;
        let confidence = match (item.kind, item.confidence) {
            (MemoryKind::Experience, Some(level)) => Some(if helpful {
                match stats.helpful {
                    n if n >= 3 => Confidence::High,
                    _ => level.max(Confidence::Medium),
                }
            } else {
                match level {
                    Confidence::High => Confidence::Medium,
                    Confidence::Medium => Confidence::Low,
                    Confidence::Low => {
                        state = MemoryState::Archived;
                        Confidence::Low
                    }
                }
            }),
            (_, level) => level,
        };
        self.conn.execute(
            "UPDATE items SET helpful = ?2, wrong = ?3, confidence = ?4, state = ?5, updated_ms = ?6
             WHERE id = ?1",
            params![
                item.id,
                stats.helpful as i64,
                stats.wrong as i64,
                confidence.as_ref().map(enc),
                enc(&state),
                now_ms as i64
            ],
        )?;
        self.must(&item.id)
    }

    fn with_evidence(&self, mut item: MemoryItem) -> Result<MemoryItem, MemoryError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT task_id, harness, model, excerpt, hash, at_ms FROM evidence
             WHERE item_id = ?1 ORDER BY at_ms",
        )?;
        item.evidence = statement
            .query_map([&item.id], |row| {
                Ok(Evidence {
                    task_id: row.get(0)?,
                    harness: row.get(1)?,
                    model: row.get(2)?,
                    excerpt: row.get(3)?,
                    hash: row.get(4)?,
                    at_ms: row.get::<_, i64>(5)? as u64,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(item)
    }

    fn query(
        &self,
        filter: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<MemoryItem>, MemoryError> {
        let sql = format!("SELECT {COLUMNS} FROM items {filter}");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement
            .query_map(args, row_item)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|item| self.with_evidence(item))
            .collect()
    }

    pub(super) fn get(&self, id: &str) -> Result<Option<MemoryItem>, MemoryError> {
        let item = self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM items WHERE id = ?1"),
                [id],
                row_item,
            )
            .optional()?;
        item.map(|item| self.with_evidence(item)).transpose()
    }

    fn must(&self, id: &str) -> Result<MemoryItem, MemoryError> {
        self.get(id)?.ok_or(MemoryError::UnknownItem)
    }

    /// A new item, or — when the same words are already kept or proposed in
    /// this scope — that item with one more piece of evidence (and the user
    /// saying it makes a proposal theirs).
    pub(super) fn insert(
        &mut self,
        draft: &MemoryDraft,
        text: &str,
        assertion: Assertion,
        state: MemoryState,
        evidence: Evidence,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        let project = draft.project_id.clone().unwrap_or_default();
        let norm = normalize(text);
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM items WHERE project = ?1 AND norm = ?2
                 AND state IN ('active', 'pending') AND kind = ?3",
                params![project, norm, enc(&draft.kind)],
                |row| row.get(0),
            )
            .optional()?;
        let tx = self.conn.transaction()?;
        let id = if let Some(id) = existing {
            tx.execute(
                "INSERT INTO evidence(item_id, task_id, harness, model, excerpt, hash, at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    evidence.task_id,
                    evidence.harness,
                    evidence.model,
                    evidence.excerpt,
                    evidence.hash,
                    evidence.at_ms as i64
                ],
            )?;
            if assertion == Assertion::UserExplicit {
                tx.execute(
                    "UPDATE items SET state = 'active', assertion = 'userExplicit', updated_ms = ?2
                     WHERE id = ?1",
                    params![id, now_ms as i64],
                )?;
            }
            id
        } else {
            let id = format!("mem-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
            tx.execute(
                "INSERT INTO items(id, project, kind, category, text, norm, assertion, state,
                                   pinned, supersedes, created_ms, updated_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?10)",
                params![
                    id,
                    project,
                    enc(&draft.kind),
                    draft.category.as_ref().map(enc),
                    text,
                    norm,
                    enc(&assertion),
                    enc(&state),
                    draft.supersedes,
                    now_ms as i64
                ],
            )?;
            tx.execute(
                "INSERT INTO items_fts(id, body) VALUES (?1, ?2)",
                params![id, cjk::segment(text)],
            )?;
            tx.execute(
                "INSERT INTO evidence(item_id, task_id, harness, model, excerpt, hash, at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    evidence.task_id,
                    evidence.harness,
                    evidence.model,
                    evidence.excerpt,
                    evidence.hash,
                    evidence.at_ms as i64
                ],
            )?;
            if let (Some(old), MemoryState::Active) = (&draft.supersedes, state) {
                // The old item keeps its text; it is only marked replaced.
                let changed = tx.execute(
                    "UPDATE items SET state = 'superseded', superseded_by = ?2, updated_ms = ?3
                     WHERE id = ?1",
                    params![old, id, now_ms as i64],
                )?;
                if changed == 0 {
                    return Err(MemoryError::UnknownItem);
                }
            }
            id
        };
        tx.commit()?;
        self.must(&id)
    }

    pub(super) fn activate(
        &self,
        item: &MemoryItem,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        self.conn.execute(
            "UPDATE items SET state = 'active', assertion = 'agentConfirmed', updated_ms = ?2
             WHERE id = ?1",
            params![item.id, now_ms as i64],
        )?;
        // B5-2: a proposal that replaces an item takes effect only now.
        if let Some(old) = &item.supersedes {
            self.conn.execute(
                "UPDATE items SET state = 'superseded', superseded_by = ?2, updated_ms = ?3
                 WHERE id = ?1 AND state = 'active'",
                params![old, item.id, now_ms as i64],
            )?;
        }
        self.must(&item.id)
    }

    pub(super) fn kv(&self, key: &str) -> Result<Option<String>, MemoryError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub(super) fn set_kv(&self, key: &str, value: &str) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO kv(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// B5-2: one 整理记录 row; `counted` ones use up the day's allowance.
    pub(super) fn add_run(
        &self,
        run: &aeroric_protocol::memory::MemoryRun,
    ) -> Result<(), MemoryError> {
        let counted = run.state != aeroric_protocol::memory::MemoryRunState::Skipped;
        self.conn.execute(
            "INSERT OR REPLACE INTO runs(id, at_ms, body, counted) VALUES (?1, ?2, ?3, ?4)",
            params![
                run.id,
                run.at_ms as i64,
                serde_json::to_string(run).unwrap_or_default(),
                counted as i64
            ],
        )?;
        Ok(())
    }

    pub(super) fn runs(
        &self,
        limit: usize,
    ) -> Result<Vec<aeroric_protocol::memory::MemoryRun>, MemoryError> {
        let mut statement = self
            .conn
            .prepare("SELECT body FROM runs ORDER BY at_ms DESC LIMIT ?1")?;
        let rows = statement
            .query_map([limit as i64], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .iter()
            .filter_map(|body| serde_json::from_str(body).ok())
            .collect())
    }

    pub(super) fn counted_since(&self, since_ms: u64) -> Result<u32, MemoryError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM runs WHERE counted = 1 AND at_ms >= ?1",
            [since_ms as i64],
            |row| row.get::<_, i64>(0),
        )? as u32)
    }

    pub(super) fn extracted(&self, task_id: &str) -> Result<Option<String>, MemoryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT hash FROM extracted WHERE task_id = ?1",
                [task_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(super) fn mark_extracted(
        &self,
        task_id: &str,
        hash: &str,
        now_ms: u64,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO extracted(task_id, hash, at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id) DO UPDATE SET hash = excluded.hash, at_ms = excluded.at_ms",
            params![task_id, hash, now_ms as i64],
        )?;
        Ok(())
    }

    pub(super) fn set_state(
        &self,
        id: &str,
        state: MemoryState,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        self.conn.execute(
            "UPDATE items SET state = ?2, updated_ms = ?3 WHERE id = ?1",
            params![id, enc(&state), now_ms as i64],
        )?;
        self.must(id)
    }

    pub(super) fn set_pinned(
        &self,
        id: &str,
        pinned: bool,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        self.conn.execute(
            "UPDATE items SET pinned = ?2, updated_ms = ?3 WHERE id = ?1",
            params![id, pinned as i64, now_ms as i64],
        )?;
        self.must(id)
    }

    pub(super) fn set_project(
        &self,
        id: &str,
        project_id: Option<&str>,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        self.conn.execute(
            "UPDATE items SET project = ?2, updated_ms = ?3 WHERE id = ?1",
            params![id, project_id.unwrap_or(""), now_ms as i64],
        )?;
        self.must(id)
    }

    /// The item and the chain of versions it replaced.
    fn chain(&self, item: &MemoryItem) -> Result<Vec<MemoryItem>, MemoryError> {
        let mut out = vec![item.clone()];
        let mut next = item.supersedes.clone();
        while let Some(id) = next {
            let Some(old) = self.get(&id)? else { break };
            next = old.supersedes.clone();
            out.push(old);
        }
        Ok(out)
    }

    pub(super) fn delete(&mut self, item: &MemoryItem) -> Result<(), MemoryError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM items WHERE id = ?1", [&item.id])?;
        tx.execute("DELETE FROM evidence WHERE item_id = ?1", [&item.id])?;
        tx.execute("DELETE FROM items_fts WHERE id = ?1", [&item.id])?;
        // A newer version that pointed here now starts the chain.
        tx.execute(
            "UPDATE items SET supersedes = NULL WHERE supersedes = ?1",
            [&item.id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Delete the item and its older versions; keep only their signatures.
    pub(super) fn forget(&mut self, item: &MemoryItem, now_ms: u64) -> Result<usize, MemoryError> {
        let chain = self.chain(item)?;
        for old in &chain {
            let sig = self.barrier.sign(&normalize(&old.text));
            self.conn.execute(
                "INSERT OR IGNORE INTO barrier(sig, at_ms) VALUES (?1, ?2)",
                params![sig, now_ms as i64],
            )?;
            self.delete(old)?;
        }
        // Old pages in the write-ahead log go too.
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(chain.len())
    }

    pub(super) fn barred(&self, norm: &str) -> Result<bool, MemoryError> {
        let sig = self.barrier.sign(norm);
        Ok(self
            .conn
            .query_row("SELECT 1 FROM barrier WHERE sig = ?1", [sig], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub(super) fn lift_barrier(&self, norm: &str) -> Result<(), MemoryError> {
        let sig = self.barrier.sign(norm);
        self.conn
            .execute("DELETE FROM barrier WHERE sig = ?1", [sig])?;
        Ok(())
    }

    pub(super) fn list(
        &self,
        project_id: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<MemoryItem>, MemoryError> {
        let project = project_id.unwrap_or("");
        match query.and_then(cjk::match_expression) {
            Some(expr) => self.query(
                "WHERE project = ?1 AND id IN (SELECT id FROM items_fts WHERE items_fts MATCH ?2)
                 ORDER BY updated_ms DESC",
                &[&project, &expr],
            ),
            None => self.query("WHERE project = ?1 ORDER BY updated_ms DESC", &[&project]),
        }
    }

    pub(super) fn pending(&self) -> Result<Vec<MemoryItem>, MemoryError> {
        self.query("WHERE state = 'pending' ORDER BY created_ms", &[])
    }

    /// Active items of one scope, pinned first, then newest.
    pub(super) fn active(&self, project_id: Option<&str>) -> Result<Vec<MemoryItem>, MemoryError> {
        self.query(
            "WHERE project = ?1 AND state = 'active' ORDER BY pinned DESC, updated_ms DESC",
            &[&project_id.unwrap_or("")],
        )
    }

    /// Active items of the project and global matching `query` (any word),
    /// with their bm25 rank (lower is better).
    pub(super) fn search(
        &self,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(i64, MemoryItem)>, MemoryError> {
        let Some(expr) = cjk::match_expression(query) else {
            return Ok(Vec::new());
        };
        let expr = expr.replace(" AND ", " OR ");
        let mut statement = self.conn.prepare(
            "SELECT id, CAST(bm25(items_fts) * 1000 AS INTEGER) FROM items_fts
             WHERE items_fts MATCH ?1 ORDER BY bm25(items_fts) LIMIT ?2",
        )?;
        let hits = statement
            .query_map(params![expr, (limit * 4) as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (id, rank) in hits {
            let Some(item) = self.get(&id)? else { continue };
            let in_scope =
                item.project_id.is_none() || item.project_id.as_deref() == Some(project_id);
            if in_scope && item.state == MemoryState::Active {
                out.push((rank, item));
            }
            if out.len() == limit {
                break;
            }
        }
        Ok(out)
    }

    pub(super) fn enabled(&self, project_id: &str) -> Result<bool, MemoryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT enabled FROM projects WHERE project_id = ?1",
                [project_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some_and(|v| v != 0))
    }

    pub(super) fn set_enabled(&self, project_id: &str, enabled: bool) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO projects(project_id, enabled) VALUES (?1, ?2)
             ON CONFLICT(project_id) DO UPDATE SET enabled = excluded.enabled",
            params![project_id, enabled as i64],
        )?;
        Ok(())
    }
}
