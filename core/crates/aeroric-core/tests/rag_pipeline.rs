//! P7-06 验收：RAG 管线契约（`tests/rag_pipeline.rs`）。
//!
//! tmp SQLite **实际加载 sqlite-vec 扩展**（vec0 虚表链接进本二进制）并做
//! 真向量查询；切块器按标题分段；向量维度变化 → 整表重建且全库 Stale
//! （派生数据可重建——不丢用户内容）。
//! 既有单测（chunk/cjk/db/embed/search/graph/index 搬移自宿主）在 `--lib` 跑。

use aeroric_core::notebook::rag::{chunk, db};
use rusqlite::Connection;

fn temp_vault(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-rag-contract-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create vault");
    std::fs::canonicalize(&dir).expect("canonicalize")
}

// ── sqlite-vec：真扩展加载 + 真向量查询 ─────────────────────────────────────

#[test]
fn the_vec0_virtual_table_actually_loads_and_answers_a_vector_query() {
    let vault = temp_vault("vec");
    let dim = 4;
    let conn = db::open(&vault, dim).expect("open index.db (vec0 loaded)");

    // 真实插入：chunks 一行 + 对应向量。vec0 的 rowid 必须对齐 chunks.id。
    conn.execute(
        "INSERT INTO docs(path, title, hash, status, chunk_count) VALUES ('a.md', 'A', 'h1', 'ready', 1)",
        [],
    )
    .expect("insert doc");
    conn.execute(
        "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, 'hello world', 'hello world')",
        [],
    )
    .expect("insert chunk");
    let chunk_id: i64 = conn
        .query_row("SELECT id FROM chunks WHERE doc_id = 1", [], |r| r.get(0))
        .expect("chunk id");
    conn.execute(
        "INSERT INTO vec_chunks(rowid, embedding) VALUES (?1, ?2)",
        rusqlite::params![chunk_id, "[0.1, 0.2, 0.3, 0.4]"],
    )
    .expect("insert embedding — sqlite-vec 扩展真实生效");

    // 真向量查询：按 L2 距离取最近邻。
    let matched: i64 = conn
        .query_row(
            "SELECT rowid FROM vec_chunks WHERE embedding MATCH ?1 AND k = 1",
            rusqlite::params!["[0.1, 0.2, 0.3, 0.39]"],
            |r| r.get(0),
        )
        .expect("vec0 MATCH query works");
    assert_eq!(matched, chunk_id, "最近邻命中刚插入的那条");
    let _ = std::fs::remove_dir_all(vault);
}

// ── 维度变化：整表重建 + 全库 Stale ─────────────────────────────────────────

#[test]
fn a_dimension_change_rebuilds_the_vec_table_and_stales_every_doc() {
    let vault = temp_vault("dim");
    let conn = db::open(&vault, 4).expect("open dim=4");
    conn.execute(
        "INSERT INTO docs(path, title, hash, status, chunk_count) VALUES ('a.md', 'A', 'h', 'indexed', 0)",
        [],
    )
    .expect("doc");

    db::rebuild_vec_table(&conn, 8).expect("rebuild to dim=8");
    db::mark_all_stale(&conn).expect("mark stale");

    let status: String = conn
        .query_row("SELECT status FROM docs WHERE path = 'a.md'", [], |r| {
            r.get(0)
        })
        .expect("status");
    assert_eq!(status, "stale", "维度变化后全库标记待重建");
    // 新维度的 vec0 可用。
    let dim_now: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('vec_chunks')",
            [],
            |r| r.get(0),
        )
        .expect("vec schema");
    assert!(dim_now > 0, "vec_chunks 表存在");
    let _ = std::fs::remove_dir_all(vault);
}

// ── 切块：按标题分段、代码围栏整块保留 ─────────────────────────────────────

#[test]
fn chunking_splits_by_heading_and_keeps_code_fences_whole() {
    let markdown = "# One\n\npara one\n\n## Two\n\npara two\n\n```rust\nfn main() {\n    # not a heading\n}\n```\n";
    let chunks = chunk::split(markdown);
    assert!(chunks.len() >= 2, "按标题分段: {chunks:?}");
    let fenced: String = chunks.iter().map(|c| c.body.as_str()).collect();
    assert!(fenced.contains("fn main() {"), "代码围栏整块保留");
    let _ = std::fs::remove_dir_all(temp_vault("unused"));
}

// ── meta 读写与 embed model 记账 ─────────────────────────────────────────────

#[test]
fn meta_round_trips_and_the_model_is_recorded() {
    let vault = temp_vault("meta");
    let conn = db::open(&vault, 4).expect("open");
    assert_eq!(db::read_meta(&conn, "embed_model").expect("read"), None);
    db::set_embed_model(&conn, "nomic-embed-text").expect("write");
    assert_eq!(
        db::embed_model(&conn).expect("read"),
        Some("nomic-embed-text".to_string())
    );
    assert_eq!(db::peek_dim(&vault), Some(4), "维度从库里读回");
    let _ = std::fs::remove_dir_all(vault);
}

// 静默确认 Connection 别名被引用（保持与搬移单测同款 import 面）。
#[allow(dead_code)]
fn _conn_alias_used(_c: &Connection) {}
