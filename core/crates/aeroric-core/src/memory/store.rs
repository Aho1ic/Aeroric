//! The SQLite side of the memory store. Rows are only inserted or have
//! their state flipped; text never changes. `forget` is the one real delete
//! (items, evidence and index together) and it writes the barrier.

use std::collections::HashSet;
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
/// C2: how long withdrawn words are kept for runs still going.
const WITHDRAWN_KEPT_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// C3: how long the writes a run could not make wait to be tried again.
/// They hold the words, so they do not stay longer than a run's record of
/// what was withdrawn.
pub(super) const RETRY_KEPT_MS: u64 = WITHDRAWN_KEPT_MS;
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
                at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS proposals(id TEXT PRIMARY KEY, experience_id TEXT NOT NULL UNIQUE,
                body TEXT NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS dream_proposals(id TEXT PRIMARY KEY, project TEXT NOT NULL,
                run_id TEXT NOT NULL, state TEXT NOT NULL, body TEXT NOT NULL, created_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS chain(project TEXT NOT NULL, branch TEXT, path TEXT NOT NULL,
                symbol TEXT, task_id TEXT NOT NULL, turn_id TEXT NOT NULL, at_ms INTEGER NOT NULL,
                additions INTEGER NOT NULL, deletions INTEGER NOT NULL, state TEXT NOT NULL,
                diff TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS chain_turn ON chain(task_id, turn_id);
             CREATE INDEX IF NOT EXISTS chain_project ON chain(project, at_ms);
             CREATE TABLE IF NOT EXISTS vectors(item_id TEXT PRIMARY KEY, vec BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS withdrawn(seq INTEGER PRIMARY KEY AUTOINCREMENT,
                sig TEXT NOT NULL, at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS retries(run_id TEXT PRIMARY KEY, at_ms INTEGER NOT NULL,
                body TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS uses(item_id TEXT NOT NULL, task_id TEXT NOT NULL,
                kind TEXT NOT NULL, PRIMARY KEY(item_id, task_id, kind));
             CREATE TABLE IF NOT EXISTS demoted(item_id TEXT PRIMARY KEY, at_ms INTEGER NOT NULL,
                retrieved INTEGER NOT NULL, helpful INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS placements(seq INTEGER PRIMARY KEY AUTOINCREMENT,
                item_id TEXT NOT NULL, at_ms INTEGER NOT NULL, action TEXT NOT NULL,
                reason TEXT NOT NULL);",
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
        // C1 (2026-11-19): where the evidence's words were found, as JSON.
        let evidence_columns: Vec<String> = conn
            .prepare("PRAGMA table_info(evidence)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<_, _>>()?;
        if !evidence_columns.iter().any(|c| c == "location") {
            conn.execute_batch("ALTER TABLE evidence ADD COLUMN location TEXT")?;
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
        // B6: the words changed, so the vector is stale; the next sweep
        // embeds them again.
        tx.execute("DELETE FROM vectors WHERE item_id = ?1", [id])?;
        tx.execute(
            "INSERT INTO items_fts(id, body) VALUES (?1, ?2)",
            params![id, cjk::segment(&body)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// B5-3b: skill proposals, stored whole.
    pub(super) fn put_proposal(
        &self,
        p: &aeroric_protocol::memory::SkillProposal,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO proposals(id, experience_id, body, updated_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET body = excluded.body, updated_ms = excluded.updated_ms",
            params![
                p.id,
                p.experience_id,
                serde_json::to_string(p).unwrap_or_default(),
                p.updated_ms as i64
            ],
        )?;
        Ok(())
    }

    fn proposal_where(
        &self,
        filter: &str,
        arg: &str,
    ) -> Result<Option<aeroric_protocol::memory::SkillProposal>, MemoryError> {
        let body: Option<String> = self
            .conn
            .query_row(
                &format!("SELECT body FROM proposals WHERE {filter} = ?1"),
                [arg],
                |row| row.get(0),
            )
            .optional()?;
        Ok(body.and_then(|b| serde_json::from_str(&b).ok()))
    }

    pub(super) fn proposal(
        &self,
        id: &str,
    ) -> Result<Option<aeroric_protocol::memory::SkillProposal>, MemoryError> {
        self.proposal_where("id", id)
    }

    pub(super) fn proposal_for(
        &self,
        experience_id: &str,
    ) -> Result<Option<aeroric_protocol::memory::SkillProposal>, MemoryError> {
        self.proposal_where("experience_id", experience_id)
    }

    pub(super) fn proposals(
        &self,
    ) -> Result<Vec<aeroric_protocol::memory::SkillProposal>, MemoryError> {
        let mut statement = self
            .conn
            .prepare("SELECT body FROM proposals ORDER BY updated_ms DESC")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .iter()
            .filter_map(|b| serde_json::from_str(b).ok())
            .collect())
    }

    /// B6 代码版本链: one turn's blocks, replacing what it had.
    pub(super) fn put_changes(
        &mut self,
        task_id: &str,
        turn_id: &str,
        changes: &[aeroric_protocol::memory::CodeChange],
    ) -> Result<(), MemoryError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM chain WHERE task_id = ?1 AND turn_id = ?2",
            params![task_id, turn_id],
        )?;
        for c in changes {
            tx.execute(
                "INSERT INTO chain(project, branch, path, symbol, task_id, turn_id, at_ms,
                                   additions, deletions, state, diff)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    c.project_id,
                    c.branch,
                    c.path,
                    c.symbol,
                    task_id,
                    turn_id,
                    c.at_ms as i64,
                    c.additions as i64,
                    c.deletions as i64,
                    enc(&c.state),
                    c.diff
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(super) fn has_changes(&self, task_id: &str, turn_id: &str) -> Result<bool, MemoryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM chain WHERE task_id = ?1 AND turn_id = ?2 LIMIT 1",
                params![task_id, turn_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// The newest blocks of these sessions.
    pub(super) fn changes_of(
        &self,
        task_ids: &[String],
        limit: usize,
    ) -> Result<Vec<aeroric_protocol::memory::CodeChange>, MemoryError> {
        let marks = vec!["?"; task_ids.len()].join(",");
        let mut statement = self.conn.prepare(&format!(
            "SELECT project, branch, path, symbol, task_id, turn_id, at_ms, additions, deletions,
                    state, diff
             FROM chain WHERE task_id IN ({marks}) ORDER BY at_ms DESC, rowid LIMIT {limit}"
        ))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(task_ids.iter()), |row| {
                Ok(aeroric_protocol::memory::CodeChange {
                    project_id: row.get(0)?,
                    branch: row.get(1)?,
                    path: row.get(2)?,
                    symbol: row.get(3)?,
                    task_id: row.get(4)?,
                    turn_id: row.get(5)?,
                    at_ms: row.get::<_, i64>(6)? as u64,
                    additions: row.get::<_, i64>(7)? as u32,
                    deletions: row.get::<_, i64>(8)? as u32,
                    state: dec(&row.get::<_, String>(9)?)?,
                    diff: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// A project's blocks older than `before_ms`, and beyond its newest
    /// `keep`, go.
    pub(super) fn prune_changes(
        &self,
        project: &str,
        before_ms: u64,
        keep: usize,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "DELETE FROM chain WHERE project = ?1 AND at_ms < ?2",
            params![project, before_ms as i64],
        )?;
        self.conn.execute(
            "DELETE FROM chain WHERE project = ?1 AND rowid NOT IN
               (SELECT rowid FROM chain WHERE project = ?1 ORDER BY at_ms DESC LIMIT ?2)",
            params![project, keep as i64],
        )?;
        Ok(())
    }

    /// B6: a dream's proposal, stored whole.
    pub(super) fn put_dream_proposal(
        &self,
        p: &aeroric_protocol::memory::DreamProposal,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO dream_proposals(id, project, run_id, state, body, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state, body = excluded.body",
            params![
                p.id,
                p.project_id.clone().unwrap_or_default(),
                p.run_id,
                enc(&p.state),
                serde_json::to_string(p).unwrap_or_default(),
                p.created_ms as i64
            ],
        )?;
        Ok(())
    }

    /// B6: a scope's dream proposals, newest first (`None` is global).
    pub(super) fn dream_proposals(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<aeroric_protocol::memory::DreamProposal>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT body FROM dream_proposals WHERE project = ?1 ORDER BY created_ms DESC",
        )?;
        let rows = statement
            .query_map([project_id.unwrap_or("")], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .iter()
            .filter_map(|b| serde_json::from_str(b).ok())
            .collect())
    }

    /// B6 可选 embedding: drop every vector (the model changed).
    pub(super) fn clear_vectors(&self) -> Result<(), MemoryError> {
        self.conn.execute("DELETE FROM vectors", [])?;
        Ok(())
    }

    /// Active items without a vector, newest first.
    pub(super) fn unembedded(&self, limit: usize) -> Result<Vec<MemoryItem>, MemoryError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM items WHERE state = 'active'
               AND id NOT IN (SELECT item_id FROM vectors)
             ORDER BY updated_ms DESC LIMIT ?1"
        ))?;
        let rows = statement
            .query_map([limit as i64], row_item)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub(super) fn put_vector(&self, id: &str, vector: &[f32]) -> Result<(), MemoryError> {
        let bytes: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.conn.execute(
            "INSERT OR REPLACE INTO vectors(item_id, vec) VALUES (?1, ?2)",
            params![id, bytes],
        )?;
        Ok(())
    }

    /// Active items with a vector, and without.
    pub(super) fn vector_counts(&self) -> Result<(u32, u32), MemoryError> {
        let count = |sql: &str| -> Result<u32, MemoryError> {
            Ok(self.conn.query_row(sql, [], |row| row.get::<_, i64>(0))? as u32)
        };
        Ok((
            count(
                "SELECT COUNT(*) FROM items WHERE state = 'active'
                   AND id IN (SELECT item_id FROM vectors)",
            )?,
            count(
                "SELECT COUNT(*) FROM items WHERE state = 'active'
                   AND id NOT IN (SELECT item_id FROM vectors)",
            )?,
        ))
    }

    /// The vectors of a scope's active items (`None`: global).
    pub(super) fn vectors(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<(String, Vec<f32>)>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT v.item_id, v.vec FROM vectors v JOIN items i ON i.id = v.item_id
             WHERE i.state = 'active' AND i.project = ?1",
        )?;
        let rows = statement
            .query_map([project_id.unwrap_or("")], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(id, bytes)| {
                let v = bytes
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                (id, v)
            })
            .collect())
    }

    /// B6 second batch: the proposals waiting in every scope, newest first.
    pub(super) fn pending_dream_proposals(
        &self,
    ) -> Result<Vec<aeroric_protocol::memory::DreamProposal>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT body FROM dream_proposals WHERE state = ?1 ORDER BY created_ms DESC",
        )?;
        let rows = statement
            .query_map(
                [enc(&aeroric_protocol::memory::DreamProposalState::Pending)],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .iter()
            .filter_map(|b| serde_json::from_str(b).ok())
            .collect())
    }

    pub(super) fn dream_proposal(
        &self,
        id: &str,
    ) -> Result<Option<aeroric_protocol::memory::DreamProposal>, MemoryError> {
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM dream_proposals WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(body.and_then(|b| serde_json::from_str(&b).ok()))
    }

    /// B6: one 整理记录 row by id.
    pub(super) fn run(
        &self,
        id: &str,
    ) -> Result<Option<aeroric_protocol::memory::MemoryRun>, MemoryError> {
        let body: Option<String> = self
            .conn
            .query_row("SELECT body FROM runs WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(body.and_then(|b| serde_json::from_str(&b).ok()))
    }

    /// B6: mark an active item replaced by `keep`; false when it is no
    /// longer active.
    pub(super) fn retire(&self, id: &str, keep: &str, now_ms: u64) -> Result<bool, MemoryError> {
        let text = self.text_of(id)?;
        let retired = self.conn.execute(
            "UPDATE items SET state = 'superseded', superseded_by = ?2, updated_ms = ?3
             WHERE id = ?1 AND state = 'active'",
            params![id, keep, now_ms as i64],
        )? == 1;
        if let (true, Some(text)) = (retired, text) {
            self.withdraw(&[&text], now_ms)?;
        }
        Ok(retired)
    }

    /// An item's words, if it is there.
    fn text_of(&self, id: &str) -> Result<Option<String>, MemoryError> {
        Ok(self
            .conn
            .query_row("SELECT text FROM items WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// B6: take a dream's merge back — only while the item is still the one
    /// the dream replaced by `keep`.
    pub(super) fn unretire(&self, id: &str, keep: &str, now_ms: u64) -> Result<bool, MemoryError> {
        Ok(self.conn.execute(
            "UPDATE items SET state = 'active', superseded_by = NULL, updated_ms = ?3
             WHERE id = ?1 AND state = 'superseded' AND superseded_by = ?2",
            params![id, keep, now_ms as i64],
        )? == 1)
    }

    /// B6: items of these kinds added to a scope since `since_ms`.
    pub(super) fn added_since(
        &self,
        project_id: Option<&str>,
        kinds: &[MemoryKind],
        since_ms: u64,
    ) -> Result<u32, MemoryError> {
        let mut total = 0;
        for kind in kinds {
            total += self.conn.query_row(
                "SELECT COUNT(*) FROM items WHERE project = ?1 AND kind = ?2 AND created_ms > ?3",
                params![project_id.unwrap_or(""), enc(kind), since_ms as i64],
                |row| row.get::<_, i64>(0),
            )? as u32;
        }
        Ok(total)
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

    /// B5-3a: one more `retrieved` / `injected` for each id. C3: within a
    /// task, once per item — the tenth recall in one session is not ten uses.
    pub(super) fn bump(
        &self,
        ids: &[String],
        column: &str,
        task_id: Option<&str>,
    ) -> Result<(), MemoryError> {
        debug_assert!(matches!(column, "retrieved" | "injected"));
        for id in ids {
            if let Some(task) = task_id {
                if !self.first_use(id, task, column)? {
                    continue;
                }
            }
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
            "SELECT task_id, harness, model, excerpt, hash, at_ms, location FROM evidence
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
                    location: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|json| serde_json::from_str(&json).ok()),
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
        // C2: the words an item replaced (recorded once the write is in).
        let mut replaced: Option<String> = None;
        let tx = self.conn.transaction()?;
        let id = if let Some(id) = existing {
            tx.execute(
                "INSERT INTO evidence(item_id, task_id, harness, model, excerpt, hash, at_ms,
                                      location)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    evidence.task_id,
                    evidence.harness,
                    evidence.model,
                    evidence.excerpt,
                    evidence.hash,
                    evidence.at_ms as i64,
                    location_json(&evidence)
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
                "INSERT INTO evidence(item_id, task_id, harness, model, excerpt, hash, at_ms,
                                      location)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    evidence.task_id,
                    evidence.harness,
                    evidence.model,
                    evidence.excerpt,
                    evidence.hash,
                    evidence.at_ms as i64,
                    location_json(&evidence)
                ],
            )?;
            if let (Some(old), MemoryState::Active) = (&draft.supersedes, state) {
                // The old item keeps its text; it is only marked replaced.
                replaced = tx
                    .query_row("SELECT text FROM items WHERE id = ?1", [old], |row| {
                        row.get(0)
                    })
                    .optional()?;
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
        if let Some(text) = replaced {
            self.withdraw(&[&text], now_ms)?;
        }
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
            let text = self.text_of(old)?;
            let replaced = self.conn.execute(
                "UPDATE items SET state = 'superseded', superseded_by = ?2, updated_ms = ?3
                 WHERE id = ?1 AND state = 'active'",
                params![old, item.id, now_ms as i64],
            )? == 1;
            if let (true, Some(text)) = (replaced, text) {
                self.withdraw(&[&text], now_ms)?;
            }
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
        let counted = counted(run);
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

    /// C3: rows still queued or running (the state lives in the body).
    pub(super) fn open_runs(
        &self,
    ) -> Result<Vec<aeroric_protocol::memory::MemoryRun>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT body FROM runs
             WHERE body LIKE '%\"state\":\"queued\"%' OR body LIKE '%\"state\":\"running\"%'",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .iter()
            .filter_map(|body| serde_json::from_str(body).ok())
            .collect())
    }

    pub(super) fn delete_run(&self, id: &str) -> Result<(), MemoryError> {
        self.conn.execute("DELETE FROM runs WHERE id = ?1", [id])?;
        Ok(())
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

    pub(super) fn finish_run(
        &mut self,
        run: &aeroric_protocol::memory::MemoryRun,
        task_id: &str,
        hash: &str,
        now_ms: u64,
    ) -> Result<(), MemoryError> {
        let tx = self.conn.transaction()?;
        let counted = counted(run);
        tx.execute(
            "INSERT OR REPLACE INTO runs(id, at_ms, body, counted) VALUES (?1, ?2, ?3, ?4)",
            params![
                run.id,
                run.at_ms as i64,
                serde_json::to_string(run).unwrap_or_default(),
                counted as i64
            ],
        )?;
        tx.execute(
            "INSERT INTO extracted(task_id, hash, at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id) DO UPDATE SET hash = excluded.hash, at_ms = excluded.at_ms",
            params![task_id, hash, now_ms as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn execute_for_test(&self, sql: &str) -> Result<(), MemoryError> {
        self.conn.execute_batch(sql)?;
        Ok(())
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

    /// The item and the chain of versions it replaced — C2: with the items
    /// a dream merged into it (they say the same; undoing that dream would
    /// otherwise bring a forgotten memory back).
    fn chain(&self, item: &MemoryItem) -> Result<Vec<MemoryItem>, MemoryError> {
        let mut out = vec![item.clone()];
        let mut seen: HashSet<String> = [item.id.clone()].into();
        let mut queue = vec![item.clone()];
        while let Some(current) = queue.pop() {
            let mut older: Vec<String> = self
                .conn
                .prepare_cached("SELECT id FROM items WHERE superseded_by = ?1")?
                .query_map([&current.id], |row| row.get::<_, String>(0))?
                .collect::<Result<_, _>>()?;
            older.extend(current.supersedes.clone());
            for id in older {
                if !seen.insert(id.clone()) {
                    continue;
                }
                if let Some(old) = self.get(&id)? {
                    queue.push(old.clone());
                    out.push(old);
                }
            }
        }
        Ok(out)
    }

    /// C2: words deleted, archived or replaced, as signatures numbered in
    /// order; an extraction compares what it is about to write with those
    /// numbered after it started. Kept a week (no run lasts that long).
    pub(super) fn withdraw(&self, texts: &[&str], now_ms: u64) -> Result<(), MemoryError> {
        for text in texts {
            self.conn.execute(
                "INSERT INTO withdrawn(sig, at_ms) VALUES (?1, ?2)",
                params![self.barrier.sign(&normalize(text)), now_ms as i64],
            )?;
        }
        self.conn.execute(
            "DELETE FROM withdrawn WHERE at_ms < ?1",
            [now_ms.saturating_sub(WITHDRAWN_KEPT_MS) as i64],
        )?;
        Ok(())
    }

    /// C2: the number of the last withdrawal (0: none yet).
    pub(super) fn epoch(&self) -> Result<u64, MemoryError> {
        let seq: Option<i64> = self
            .conn
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'withdrawn'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(seq.unwrap_or(0).max(0) as u64)
    }

    /// C2: the signatures withdrawn after `epoch`.
    pub(super) fn withdrawn_since(&self, epoch: u64) -> Result<HashSet<String>, MemoryError> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT sig FROM withdrawn WHERE seq > ?1")?;
        let sigs = statement
            .query_map([epoch as i64], |row| row.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(sigs)
    }

    /// C2: a text's signature, as [`Self::withdraw`] keeps it.
    pub(super) fn signature(&self, text: &str) -> String {
        self.barrier.sign(&normalize(text))
    }

    /// C2: skill proposals made from these experiences marked 依据已撤回
    /// (`Some`), or back (`None`); the names of those changed.
    pub(super) fn mark_skill_sources(
        &self,
        experience_ids: &[String],
        withdrawn_ms: Option<u64>,
    ) -> Result<Vec<String>, MemoryError> {
        let mut names = Vec::new();
        for id in experience_ids {
            let Some(mut proposal) = self.proposal_for(id)? else {
                continue;
            };
            if proposal.source_withdrawn_ms.is_some() == withdrawn_ms.is_some() {
                continue;
            }
            proposal.source_withdrawn_ms = withdrawn_ms;
            self.put_proposal(&proposal)?;
            names.push(proposal.name);
        }
        Ok(names)
    }

    /// C2: pending dream proposals about any of these items, withdrawn (a
    /// merge of what is gone cannot be decided); how many.
    pub(super) fn withdraw_dreams(&self, ids: &[String]) -> Result<u32, MemoryError> {
        let mut statement = self
            .conn
            .prepare("SELECT body FROM dream_proposals WHERE state = 'pending'")?;
        let pending: Vec<aeroric_protocol::memory::DreamProposal> = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .filter_map(|b| serde_json::from_str(b).ok())
            .collect();
        let mut count = 0;
        for mut proposal in pending {
            if proposal.ids.iter().any(|id| ids.contains(id)) {
                proposal.state = aeroric_protocol::memory::DreamProposalState::Withdrawn;
                self.put_dream_proposal(&proposal)?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub(super) fn delete(&mut self, item: &MemoryItem) -> Result<(), MemoryError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM items WHERE id = ?1", [&item.id])?;
        tx.execute("DELETE FROM evidence WHERE item_id = ?1", [&item.id])?;
        tx.execute("DELETE FROM items_fts WHERE id = ?1", [&item.id])?;
        tx.execute("DELETE FROM vectors WHERE item_id = ?1", [&item.id])?;
        tx.execute("DELETE FROM uses WHERE item_id = ?1", [&item.id])?;
        // A newer version that pointed here now starts the chain.
        tx.execute(
            "UPDATE items SET supersedes = NULL WHERE supersedes = ?1",
            [&item.id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Delete the item and its older versions; keep only their signatures.
    /// Returns the items deleted.
    pub(super) fn forget(
        &mut self,
        item: &MemoryItem,
        now_ms: u64,
    ) -> Result<Vec<MemoryItem>, MemoryError> {
        let chain = self.chain(item)?;
        let texts: Vec<&str> = chain.iter().map(|i| i.text.as_str()).collect();
        self.withdraw(&texts, now_ms)?;
        for old in &chain {
            let sig = self.barrier.sign(&normalize(&old.text));
            self.conn.execute(
                "INSERT OR IGNORE INTO barrier(sig, at_ms) VALUES (?1, ?2)",
                params![sig, now_ms as i64],
            )?;
            self.delete(old)?;
        }
        // C3: a write waiting to be retried with the same words goes too.
        self.purge_retries()?;
        // Old pages in the write-ahead log go too.
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(chain)
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

    /// H7: items taken out of the opening, with their counts then
    /// (`retrieved`, `helpful`).
    pub(super) fn demoted(
        &self,
    ) -> Result<std::collections::HashMap<String, (u32, u32)>, MemoryError> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT item_id, retrieved, helpful FROM demoted")?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, i64>(1)? as u32, row.get::<_, i64>(2)? as u32),
                ))
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// H7: take `item` out of the opening (`demote`) or put it back, and
    /// keep the reason.
    pub(super) fn place(
        &self,
        item: &MemoryItem,
        demote: bool,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), MemoryError> {
        if demote {
            self.conn.execute(
                "INSERT OR REPLACE INTO demoted(item_id, at_ms, retrieved, helpful) VALUES (?1, ?2, ?3, ?4)",
                params![
                    item.id,
                    now_ms as i64,
                    item.stats.retrieved as i64,
                    item.stats.helpful as i64
                ],
            )?;
        } else {
            self.conn
                .execute("DELETE FROM demoted WHERE item_id = ?1", [&item.id])?;
        }
        self.conn.execute(
            "INSERT INTO placements(item_id, at_ms, action, reason) VALUES (?1, ?2, ?3, ?4)",
            params![
                item.id,
                now_ms as i64,
                if demote { "demote" } else { "promote" },
                reason
            ],
        )?;
        Ok(())
    }

    /// H7: the latest moves in and out of the opening, newest first.
    pub(super) fn placements(
        &self,
        limit: usize,
    ) -> Result<Vec<aeroric_protocol::memory::MemoryPlacement>, MemoryError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT item_id, at_ms, action, reason FROM placements ORDER BY seq DESC LIMIT ?1",
        )?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(aeroric_protocol::memory::MemoryPlacement {
                    item_id: row.get(0)?,
                    at_ms: row.get::<_, i64>(1)? as u64,
                    demoted: row.get::<_, String>(2)? == "demote",
                    reason: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
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
            "SELECT items.id, CAST(bm25(items_fts) * 1000 AS INTEGER)
             FROM items_fts JOIN items ON items.id = items_fts.id
             WHERE items_fts MATCH ?1 AND items.state = 'active'
               AND (items.project = '' OR items.project = ?2)
             ORDER BY bm25(items_fts) LIMIT ?3",
        )?;
        let hits = statement
            .query_map(params![expr, project_id, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (id, rank) in hits {
            if let Some(item) = self.get(&id)? {
                out.push((rank, item));
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

/// C1: an evidence's place as stored (JSON), `None` when not known.
fn location_json(evidence: &Evidence) -> Option<String> {
    evidence
        .location
        .as_ref()
        .and_then(|place| serde_json::to_string(place).ok())
}

/// Whether a 整理记录 row uses up the day's allowance: one that asked the
/// model, or will. B6 second batch: a dream that called no model (过时检测
/// only) does not; C3: nor a queued row, nor one canceled before its call.
fn counted(run: &aeroric_protocol::memory::MemoryRun) -> bool {
    use aeroric_protocol::memory::{MemoryRunKind, MemoryRunState};
    if run.kind == MemoryRunKind::Reindex {
        return false;
    }
    match run.state {
        MemoryRunState::Skipped | MemoryRunState::Queued => false,
        MemoryRunState::Canceled => run.model.is_some(),
        _ => !(run.kind == MemoryRunKind::Dream && run.model.is_none()),
    }
}

/// C3 (2026-11-20): writes a run could not make, the use counts per task,
/// and rebuilding the index.
impl Store {
    pub(super) fn put_retry(
        &self,
        run_id: &str,
        at_ms: u64,
        body: &str,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO retries(run_id, at_ms, body) VALUES (?1, ?2, ?3)",
            params![run_id, at_ms as i64, body],
        )?;
        Ok(())
    }

    pub(super) fn retry(&self, run_id: &str) -> Result<Option<String>, MemoryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT body FROM retries WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(super) fn delete_retry(&self, run_id: &str) -> Result<(), MemoryError> {
        self.conn
            .execute("DELETE FROM retries WHERE run_id = ?1", [run_id])?;
        Ok(())
    }

    /// The runs with writes waiting, oldest first; those kept past
    /// [`RETRY_KEPT_MS`] are dropped first.
    pub(super) fn retry_ids(&self, now_ms: u64) -> Result<Vec<String>, MemoryError> {
        self.conn.execute(
            "DELETE FROM retries WHERE at_ms < ?1",
            [now_ms.saturating_sub(RETRY_KEPT_MS) as i64],
        )?;
        let mut statement = self
            .conn
            .prepare("SELECT run_id FROM retries ORDER BY at_ms")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Words forgotten since a write was saved leave it (they are refused
    /// on retry anyway; this keeps them from waiting on disk).
    pub(super) fn purge_retries(&self) -> Result<(), MemoryError> {
        let mut statement = self.conn.prepare("SELECT run_id, body FROM retries")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for (run_id, body) in rows {
            let Ok(mut retry) = serde_json::from_str::<super::runs::Retry>(&body) else {
                continue;
            };
            let before = retry.writes.len();
            let mut keep = Vec::new();
            for (index, write) in retry.writes {
                if !self.barred(&normalize(write.text()))? {
                    keep.push((index, write));
                }
            }
            retry.writes = keep;
            // Kept even when empty: the next retry settles the run's row.
            if retry.writes.len() != before {
                self.conn.execute(
                    "UPDATE retries SET body = ?2 WHERE run_id = ?1",
                    params![run_id, serde_json::to_string(&retry).unwrap_or_default()],
                )?;
            }
        }
        Ok(())
    }

    /// Whether this is the item's first `kind` in this task (and record it).
    pub(super) fn first_use(
        &self,
        item_id: &str,
        task_id: &str,
        kind: &str,
    ) -> Result<bool, MemoryError> {
        Ok(self.conn.execute(
            "INSERT OR IGNORE INTO uses(item_id, task_id, kind) VALUES (?1, ?2, ?3)",
            params![item_id, task_id, kind],
        )? == 1)
    }

    /// Whether the full-text index answers and holds every item once.
    pub(super) fn index_healthy(&self) -> bool {
        let check = self.conn.execute(
            "INSERT INTO items_fts(items_fts) VALUES ('integrity-check')",
            [],
        );
        let count = |sql: &str| {
            self.conn
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .ok()
        };
        check.is_ok()
            && count("SELECT COUNT(*) FROM items")
                == count("SELECT COUNT(DISTINCT id) FROM items_fts")
            && count("SELECT COUNT(*) FROM items_fts") == count("SELECT COUNT(*) FROM items")
    }

    /// The full-text index written again from the items; vectors of items
    /// no longer there dropped. `(indexed, vectors dropped, active items without a
    /// vector)` — the background embeds those.
    pub(super) fn rebuild_index(&mut self) -> Result<(u32, u32, u32), MemoryError> {
        let rows: Vec<(String, String, Option<String>)> = {
            let mut statement = self.conn.prepare("SELECT id, text, detail FROM items")?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM items_fts", [])?;
        for (id, text, detail) in &rows {
            let body = match detail
                .as_deref()
                .and_then(|d| serde_json::from_str::<ExperienceDetail>(d).ok())
            {
                Some(detail) => format!(
                    "{text} {} {} {} {}",
                    detail.problem,
                    detail.solution,
                    detail.search_keys.join(" "),
                    detail.envs.join(" ")
                ),
                None => text.clone(),
            };
            tx.execute(
                "INSERT INTO items_fts(id, body) VALUES (?1, ?2)",
                params![id, cjk::segment(&body)],
            )?;
        }
        let dropped = tx.execute(
            "DELETE FROM vectors WHERE item_id NOT IN (SELECT id FROM items)",
            [],
        )? as u32;
        tx.commit()?;
        let missing = self.conn.query_row(
            "SELECT COUNT(*) FROM items WHERE state = 'active'
             AND id NOT IN (SELECT item_id FROM vectors)",
            [],
            |row| row.get::<_, i64>(0),
        )? as u32;
        Ok((rows.len() as u32, dropped, missing))
    }
}
