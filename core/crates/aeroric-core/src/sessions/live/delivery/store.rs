//! Where the queue lives (R1-2): `delivery.db` in the data directory, one
//! row per item with its JSON. Settled items are kept 7 days for the
//! timeline; open ones until they settle.

use std::collections::BTreeMap;
use std::path::Path;

use aeroric_protocol::delivery::{Delivery, DeliveryState};
use rusqlite::{params, Connection};

const KEEP_SETTLED_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Default)]
pub(super) struct Store {
    pub(super) items: BTreeMap<String, Delivery>,
    db: Option<Connection>,
}

impl Store {
    pub(super) fn open(dir: &Path, now_ms: u64) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(dir.join("delivery.db")).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS deliveries (
                id TEXT PRIMARY KEY, task_id TEXT NOT NULL, state TEXT NOT NULL,
                updated_ms INTEGER NOT NULL, body TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS deliveries_task ON deliveries(task_id, state);",
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM deliveries WHERE state IN ('delivered', 'withdrawn') AND updated_ms < ?1",
            params![now_ms.saturating_sub(KEEP_SETTLED_MS) as i64],
        )
        .map_err(|e| e.to_string())?;
        let mut items = BTreeMap::new();
        {
            let mut stmt = conn
                .prepare("SELECT body FROM deliveries")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            for body in rows.flatten() {
                if let Ok(item) = serde_json::from_str::<Delivery>(&body) {
                    items.insert(item.id.clone(), item);
                }
            }
        }
        Ok(Self {
            items,
            db: Some(conn),
        })
    }

    /// Keep `item`; a failed write is said, the item stays in memory.
    pub(super) fn save(&mut self, item: &Delivery) {
        self.items.insert(item.id.clone(), item.clone());
        let Some(db) = &self.db else {
            return;
        };
        let state = serde_json::to_value(item.state)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let body = serde_json::to_string(item).unwrap_or_default();
        if let Err(error) = db.execute(
            "INSERT OR REPLACE INTO deliveries (id, task_id, state, updated_ms, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![item.id, item.task_id, state, item.updated_ms as i64, body],
        ) {
            eprintln!("delivery: saving {} failed: {error}", item.id);
        }
    }

    pub(super) fn of_task(&self, task_id: &str) -> Vec<Delivery> {
        let mut list: Vec<_> = self
            .items
            .values()
            .filter(|d| d.task_id == task_id)
            .cloned()
            .collect();
        list.sort_by_key(|d| d.created_ms);
        list
    }

    pub(super) fn in_state(&self, state: DeliveryState) -> Vec<Delivery> {
        self.items
            .values()
            .filter(|d| d.state == state)
            .cloned()
            .collect()
    }
}
