//! The append-only audit log (`<dir>/audit.jsonl`, Q-B14): who delegated
//! what to whom with which permission, and every step after. Lines are only
//! ever appended; nothing here rewrites or removes one.

use std::io::Write;
use std::path::Path;

use aeroric_protocol::delegation::AuditEntry;

const FILE: &str = "audit.jsonl";

pub fn append(dir: &Path, entry: &AuditEntry) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut line = serde_json::to_vec(entry)?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(FILE))?;
    file.write_all(&line)?;
    file.sync_data()
}

/// Every readable line, oldest first (a torn last line is skipped).
pub fn read(dir: &Path) -> Vec<AuditEntry> {
    std::fs::read_to_string(dir.join(FILE))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
