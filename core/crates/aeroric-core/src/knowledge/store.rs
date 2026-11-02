//! One project's `wiki.db`: whole pages as JSON, revisions only appended,
//! issues, an FTS5 index, and the Markdown projection beside it.

use std::path::{Path, PathBuf};

use aeroric_protocol::wiki::{
    RevisionReason, WikiHit, WikiIssue, WikiIssueKind, WikiPage, WikiPageKind, WikiRevision,
    SEARCH_LIMIT,
};
use rusqlite::{params, Connection, OptionalExtension};

use super::{render, WikiError};
use crate::notebook::rag::cjk;

pub(super) struct Store {
    conn: Connection,
    pages_dir: PathBuf,
}

impl Store {
    pub(super) fn open(dir: &Path) -> Result<Self, WikiError> {
        std::fs::create_dir_all(dir.join("pages"))
            .map_err(|e| WikiError::Unavailable(e.to_string()))?;
        let conn = Connection::open(dir.join("wiki.db"))?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS pages(slug TEXT PRIMARY KEY, body TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS revisions(slug TEXT NOT NULL, n INTEGER NOT NULL, body TEXT NOT NULL,
                PRIMARY KEY(slug, n));
             CREATE TABLE IF NOT EXISTS issues(id TEXT PRIMARY KEY, slug TEXT NOT NULL, body TEXT NOT NULL,
                open INTEGER NOT NULL, at_ms INTEGER NOT NULL);
             CREATE VIRTUAL TABLE IF NOT EXISTS pages_fts USING fts5(slug UNINDEXED, body, tokenize = '{}');
             CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
            cjk::FTS_TOKENIZER
        ))?;
        Ok(Self {
            conn,
            pages_dir: dir.join("pages"),
        })
    }

    pub(super) fn page(&self, slug: &str) -> Result<Option<WikiPage>, WikiError> {
        let body: Option<String> = self
            .conn
            .query_row("SELECT body FROM pages WHERE slug = ?1", [slug], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(body.and_then(|b| serde_json::from_str(&b).ok()))
    }

    /// Index first, then the runbook, then decisions by category and title.
    pub(super) fn pages(&self) -> Result<Vec<WikiPage>, WikiError> {
        let mut statement = self.conn.prepare("SELECT body FROM pages")?;
        let mut pages: Vec<WikiPage> = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .filter_map(|b| serde_json::from_str(b).ok())
            .collect();
        let rank = |p: &WikiPage| match p.kind {
            WikiPageKind::Index => 0,
            WikiPageKind::Runbook => 1,
            WikiPageKind::Decision => 2,
        };
        pages.sort_by(|a, b| {
            (rank(a), a.category.clone(), a.title.clone()).cmp(&(
                rank(b),
                b.category.clone(),
                b.title.clone(),
            ))
        });
        Ok(pages)
    }

    /// Store a page as its next revision; the index and Markdown follow.
    pub(super) fn save(
        &mut self,
        mut page: WikiPage,
        reason: RevisionReason,
        now: u64,
    ) -> Result<WikiPage, WikiError> {
        page.revision += 1;
        page.updated_ms = now;
        let markdown = render(&page);
        let revision = WikiRevision {
            slug: page.slug.clone(),
            n: page.revision,
            reason,
            markdown: markdown.clone(),
            at_ms: now,
        };
        let body = serde_json::to_string(&page).map_err(|e| WikiError::Storage(e.to_string()))?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO pages(slug, body) VALUES (?1, ?2) ON CONFLICT(slug) DO UPDATE SET body = excluded.body",
            params![page.slug, body],
        )?;
        tx.execute(
            "INSERT INTO revisions(slug, n, body) VALUES (?1, ?2, ?3)",
            params![
                page.slug,
                page.revision,
                serde_json::to_string(&revision).unwrap_or_default()
            ],
        )?;
        tx.execute("DELETE FROM pages_fts WHERE slug = ?1", [&page.slug])?;
        tx.execute(
            "INSERT INTO pages_fts(slug, body) VALUES (?1, ?2)",
            params![page.slug, cjk::segment(&markdown)],
        )?;
        tx.commit()?;
        std::fs::write(self.pages_dir.join(format!("{}.md", page.slug)), markdown)
            .map_err(|e| WikiError::Storage(e.to_string()))?;
        Ok(page)
    }

    /// A page whose sources are gone; its revisions stay.
    pub(super) fn remove(&mut self, slug: &str) -> Result<(), WikiError> {
        self.conn
            .execute("DELETE FROM pages WHERE slug = ?1", [slug])?;
        self.conn
            .execute("DELETE FROM pages_fts WHERE slug = ?1", [slug])?;
        let _ = std::fs::remove_file(self.pages_dir.join(format!("{slug}.md")));
        Ok(())
    }

    pub(super) fn revisions(&self, slug: &str) -> Result<Vec<WikiRevision>, WikiError> {
        let mut statement = self
            .conn
            .prepare("SELECT body FROM revisions WHERE slug = ?1 ORDER BY n DESC")?;
        let rows = statement
            .query_map([slug], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .iter()
            .filter_map(|b| serde_json::from_str(b).ok())
            .collect())
    }

    pub(super) fn put_issue(&self, issue: &WikiIssue) -> Result<(), WikiError> {
        self.conn.execute(
            "INSERT INTO issues(id, slug, body, open, at_ms) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET body = excluded.body, open = excluded.open",
            params![
                issue.id,
                issue.slug,
                serde_json::to_string(issue).unwrap_or_default(),
                issue.open as i64,
                issue.at_ms as i64
            ],
        )?;
        Ok(())
    }

    /// Open first, newest first.
    pub(super) fn issues(&self) -> Result<Vec<WikiIssue>, WikiError> {
        let mut statement = self
            .conn
            .prepare("SELECT body FROM issues ORDER BY open DESC, at_ms DESC")?;
        let rows = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .iter()
            .filter_map(|b| serde_json::from_str(b).ok())
            .collect())
    }

    pub(super) fn open_issue_for(
        &self,
        slug: &str,
        kind: WikiIssueKind,
    ) -> Result<bool, WikiError> {
        Ok(self
            .issues()?
            .iter()
            .any(|i| i.open && i.slug == slug && i.kind == kind && i.block_key.is_none()))
    }

    pub(super) fn search(&self, query: &str) -> Result<Vec<WikiHit>, WikiError> {
        let Some(expr) = cjk::match_expression(query) else {
            return Ok(Vec::new());
        };
        let expr = expr.replace(" AND ", " OR ");
        let mut statement = self.conn.prepare(
            "SELECT slug FROM pages_fts WHERE pages_fts MATCH ?1 ORDER BY bm25(pages_fts) LIMIT ?2",
        )?;
        let slugs = statement
            .query_map(params![expr, SEARCH_LIMIT as i64], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut hits = Vec::new();
        for slug in slugs {
            let Some(page) = self.page(&slug)? else {
                continue;
            };
            let text = render(&page);
            let line = text
                .lines()
                .find(|l| words.iter().any(|w| l.to_lowercase().contains(w)))
                .unwrap_or_else(|| text.lines().nth(2).unwrap_or(""));
            hits.push(WikiHit {
                slug,
                title: page.title,
                excerpt: line.chars().take(160).collect(),
            });
        }
        Ok(hits)
    }

    pub(super) fn fingerprint(&self) -> Result<Option<String>, WikiError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'fingerprint'",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub(super) fn gaps(&self) -> Result<aeroric_protocol::wiki::WikiSourceGaps, WikiError> {
        let text: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'gaps'", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(text
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default())
    }

    pub(super) fn set_gaps(
        &self,
        gaps: &aeroric_protocol::wiki::WikiSourceGaps,
    ) -> Result<(), WikiError> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES ('gaps', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(gaps).unwrap_or_default()],
        )?;
        Ok(())
    }

    pub(super) fn set_fingerprint(&self, value: &str) -> Result<(), WikiError> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES ('fingerprint', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [value],
        )?;
        Ok(())
    }
}
