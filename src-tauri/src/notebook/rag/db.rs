//! RAG 索引库的连接与 schema。
//!
//! 落在 `<vault>/.notebook/index.db` —— vault 私有目录里,不入 Git,也不会被
//! 笔记树扫描看见。一个 vault 一个库:索引是 vault 的派生数据,vault 被删就该
//! 跟着消失。
//!
//! 三张实体表 + 两张虚表:
//!
//! ```text
//! docs        一篇笔记。带 status/error,失败的笔记留在库里可重试(见下)。
//! chunks      切块后的正文。同时存原文与逐字切分版(`*_seg`)。
//! chunks_fts  FTS5 外部内容表,索引 chunks 的 `*_seg` 两列。
//! vec_chunks  sqlite-vec 的 vec0 虚表,rowid 对齐 chunks.id。
//! links       笔记间的引用,给图谱用。
//! ```
//!
//! ## 为什么存两份正文
//!
//! `chunks.body` 是原文,`chunks.body_seg` 是 CJK 逐字切分版(见 [`super::cjk`])。
//! 重复存一份的代价换来两件事:FTS5 用外部内容表(而非无内容表)因此
//! `INSERT INTO chunks_fts('rebuild')` 可用 —— 索引损坏时能就地重建,不必重读
//! 文件、更不必重新算 embedding(那是要花钱或花几分钟的一步);以及原文留在库里,
//! 检索结果的高亮可以按原文偏移算,不受切分空格干扰。
//!
//! ## 为什么 status 要落盘
//!
//! 「索引失败可重试」要求失败是**持久**状态:某篇笔记 embedding 超时后,重启
//! 应用仍然知道它没索引成功、错在哪、试过几次。放内存里的话用户重启一次就只剩
//! 「怎么搜不到这篇」而无从查起。

use std::ffi::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::Once;

use rusqlite::{Connection, OptionalExtension};

use crate::notebook::fs_ops::private_dir;

/// schema 版本。加表 / 加列都要 +1 并在 [`migrate`] 里补迁移。
const SCHEMA_VERSION: i64 = 1;

const META_SCHEMA_VERSION: &str = "schema_version";
const META_EMBED_DIM: &str = "embed_dim";
const META_EMBED_MODEL: &str = "embed_model";

/// 一篇笔记在索引里的状态。
///
/// 存成字符串而非整数:出问题时 `sqlite3 index.db 'select status from docs'`
/// 直接可读,不必回头查映射表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocStatus {
    /// 待索引:新发现的,或内容变了需要重切重算的。
    Pending,
    /// 已索引且 embedding 与当前模型一致。
    Indexed,
    /// 上次索引失败,`docs.error` 记了原因。可重试。
    Failed,
    /// 内容没变但 embedding 失效了(换了模型 / 维度)。要重算向量,不必重切块。
    Stale,
}

impl DocStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DocStatus::Pending => "pending",
            DocStatus::Indexed => "indexed",
            DocStatus::Failed => "failed",
            DocStatus::Stale => "stale",
        }
    }

    /// 未知字符串按 `Pending` 处理 —— 让它被重新索引,而不是让整个查询失败。
    /// 库可能被更新的版本写过,或者被人手工改过。
    pub fn from_str(raw: &str) -> Self {
        match raw {
            "indexed" => DocStatus::Indexed,
            "failed" => DocStatus::Failed,
            "stale" => DocStatus::Stale,
            _ => DocStatus::Pending,
        }
    }
}

/// 注册 sqlite-vec 为自动扩展。
///
/// `sqlite3_auto_extension` 是**进程全局**的,登记之后本进程里所有新开的
/// SQLite 连接都会带上 `vec0` 模块 —— 包括数据库浏览器打开的用户库。那只是多
/// 一个虚表模块和几个 `vec_*` 标量函数,不动数据、不改行为,所以可以接受;真正
/// 的替代方案(每条连接手工 load_extension)要把扩展文件释放到磁盘上,那才是
/// 麻烦的一步。
///
/// `Once` 不只是省事:重复登记会让同一个初始化函数被推进 SQLite 的自动扩展表
/// 多次,每开一条连接就多跑一遍。
fn register_vec_extension() {
    static VEC_INIT: Once = Once::new();
    VEC_INIT.call_once(|| {
        // sqlite-vec 暴露的是 `unsafe extern "C" fn()`,而 `sqlite3_auto_extension`
        // 要的是带三个参数的入口签名。两者 ABI 兼容(SQLite 就是这么调的),
        // 但类型上对不上,只能 transmute —— 这也是 sqlite-vec 自己文档里的用法。
        type ExtensionInit = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> c_int;
        unsafe {
            let init = std::mem::transmute::<*const (), ExtensionInit>(
                sqlite_vec::sqlite3_vec_init as *const (),
            );
            rusqlite::ffi::sqlite3_auto_extension(Some(init));
        }
    });
}

/// 索引库的路径。
pub fn db_path(vault: &Path) -> PathBuf {
    private_dir(vault).join("index.db")
}

/// 打开(必要时创建)某个 vault 的索引库。
///
/// `dim` 是当前 embedding 模型的维度。库里记着上次用的维度,不一致时把向量表
/// 重建并把所有笔记标成 `Stale` —— 不同维度的向量放在一张 vec0 表里是查询期
/// 报错,而那时候用户看到的只是「搜索坏了」。
pub fn open(vault: &Path, dim: usize) -> Result<Connection, String> {
    if dim == 0 {
        return Err("Embedding dimension must be positive".to_string());
    }
    register_vec_extension();
    let dir = private_dir(vault);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create notebook index dir: {e}"))?;
    let path = db_path(vault);
    let conn = Connection::open(&path).map_err(|e| format!("Cannot open notebook index: {e}"))?;
    configure(&conn)?;
    init_schema(&conn, dim)?;
    Ok(conn)
}

/// 只读打开:库不存在时不创建。检索路径用这个 —— 用户还没建索引就搜索时不该
/// 凭空在 vault 里留下一个空库。
pub fn open_existing(vault: &Path, dim: usize) -> Result<Option<Connection>, String> {
    let path = db_path(vault);
    if !path.exists() {
        return Ok(None);
    }
    open(vault, dim).map(Some)
}

/// 偷看库里记着的向量维度。没有库、读不出来都返回 `None`。
///
/// 存在的理由是 [`open`] 的 `dim` 参数是**权威的**:传错会让向量表被重建、全库
/// 标 `Stale`。而「读一下索引现状」「搜一下」这两条路本身不知道维度 —— 探测维度
/// 要发网络请求,面板一打开就发请求是不能接受的。所以这两条路先偷看库里记的,
/// 拿它当 `dim` 传回去,`ensure_dim` 那一支就恒不触发。
///
/// 库不存在时返回 `None`,不建库:用户还没建过索引时不该因为点开面板就在 vault
/// 里多一个空库。
pub fn peek_dim(vault: &Path) -> Option<usize> {
    let path = db_path(vault);
    if !path.exists() {
        return None;
    }
    // 不走 `open`:那会跑 schema 初始化(含 `ensure_dim`),而这里的全部意义就是
    // 绕开它。
    //
    // 也**不**用 `SQLITE_OPEN_READ_ONLY`,尽管「偷看」听起来正该只读:库的
    // journal_mode=WAL 记在文件头里,任何连接(只读的也一样)一打开就会把 `-wal`
    // 与 `-shm` 建出来,而只读连接在关闭时没有权限 checkpoint + unlink 它们。
    // 于是每次偷看都会在 `.notebook/` 里留下两个不会被回收的残留文件 —— 而偷看
    // 恰好是最频繁的那条路(面板一打开、每次检索)。读写连接是最后一个连接时会
    // 自己收尾,目录保持干净。
    //
    // 只读换不来「不会写坏」的保证:那由不发任何 DDL/DML 保证,
    // `peek_dim_does_not_touch_the_schema` 盯着。
    let conn = Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    read_meta(&conn, META_EMBED_DIM).ok()?.and_then(|raw| {
        let dim = raw.parse::<usize>().ok()?;
        (dim > 0).then_some(dim)
    })
}

fn configure(conn: &Connection) -> Result<(), String> {
    // WAL:索引写入(长任务)与检索读取会并发,rollback journal 下读会被写堵住。
    conn.pragma_update(None, "journal_mode", "wal")
        .map_err(|e| format!("Cannot enable WAL on notebook index: {e}"))?;
    // NORMAL 而非 FULL:这是可重建的派生数据,断电丢掉最后几个 chunk 的代价是
    // 重索引那一篇,不值得每次提交都等 fsync。
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("Cannot configure notebook index: {e}"))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| format!("Cannot enable foreign keys on notebook index: {e}"))?;
    Ok(())
}

fn init_schema(conn: &Connection, dim: usize) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
             key   TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );",
    )
    .map_err(|e| format!("Cannot init notebook index meta: {e}"))?;

    let existing = read_meta(conn, META_SCHEMA_VERSION)?
        .and_then(|raw| raw.parse::<i64>().ok())
        .unwrap_or(0);
    if existing > SCHEMA_VERSION {
        // 降级运行:新版本可能加了这个版本读不懂的列。宁可报错也不要按旧
        // schema 往里写,那会把用户的索引写坏。
        return Err(format!(
            "Notebook index was written by a newer version (schema {existing} > {SCHEMA_VERSION})"
        ));
    }
    if existing == 0 {
        create_tables(conn, dim)?;
        write_meta(conn, META_SCHEMA_VERSION, &SCHEMA_VERSION.to_string())?;
        write_meta(conn, META_EMBED_DIM, &dim.to_string())?;
    } else {
        migrate(conn, existing)?;
    }
    ensure_dim(conn, dim)?;
    Ok(())
}

fn create_tables(conn: &Connection, dim: usize) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS docs (
             id          INTEGER PRIMARY KEY,
             path        TEXT NOT NULL UNIQUE,
             title       TEXT NOT NULL DEFAULT '',
             mtime_ms    INTEGER NOT NULL DEFAULT 0,
             hash        TEXT NOT NULL DEFAULT '',
             status      TEXT NOT NULL DEFAULT 'pending',
             error       TEXT,
             attempts    INTEGER NOT NULL DEFAULT 0,
             chunk_count INTEGER NOT NULL DEFAULT 0,
             indexed_at  INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS docs_status ON docs(status);

         CREATE TABLE IF NOT EXISTS chunks (
             id          INTEGER PRIMARY KEY,
             doc_id      INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
             ord         INTEGER NOT NULL,
             heading     TEXT NOT NULL DEFAULT '',
             body        TEXT NOT NULL,
             heading_seg TEXT NOT NULL DEFAULT '',
             body_seg    TEXT NOT NULL,
             char_start  INTEGER NOT NULL DEFAULT 0,
             char_end    INTEGER NOT NULL DEFAULT 0,
             token_count INTEGER NOT NULL DEFAULT 0,
             embedded    INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS chunks_doc ON chunks(doc_id, ord);

         CREATE TABLE IF NOT EXISTS links (
             id          INTEGER PRIMARY KEY,
             doc_id      INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
             target      TEXT NOT NULL,
             target_norm TEXT NOT NULL DEFAULT '',
             kind        TEXT NOT NULL DEFAULT 'wikilink'
         );
         CREATE INDEX IF NOT EXISTS links_doc ON links(doc_id);
         CREATE INDEX IF NOT EXISTS links_target ON links(target_norm);",
    )
    .map_err(|e| format!("Cannot create notebook index tables: {e}"))?;

    // FTS5 索引 `*_seg` 两列 —— 那是 CJK 逐字切分后的文本。原文列不进索引:
    // 同一段内容进两次会让 bm25 的词频统计翻倍,而 `unicode61` 对原文里的中文
    // 本来就不出 token(见 `super::cjk` 的模块注释)。
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
             body_seg,
             heading_seg,
             content='chunks',
             content_rowid='id',
             tokenize='{}'
         );",
        super::cjk::FTS_TOKENIZER
    ))
    .map_err(|e| format!("Cannot create notebook index FTS table: {e}"))?;

    // 触发器同步 FTS。手工在 Rust 侧维护也行,但漏一处的表现是「某些笔记搜不到」
    // —— 那种 bug 极难发现,交给数据库更稳。
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
             INSERT INTO chunks_fts(rowid, body_seg, heading_seg)
             VALUES (new.id, new.body_seg, new.heading_seg);
         END;
         CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
             INSERT INTO chunks_fts(chunks_fts, rowid, body_seg, heading_seg)
             VALUES ('delete', old.id, old.body_seg, old.heading_seg);
         END;
         CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
             INSERT INTO chunks_fts(chunks_fts, rowid, body_seg, heading_seg)
             VALUES ('delete', old.id, old.body_seg, old.heading_seg);
             INSERT INTO chunks_fts(rowid, body_seg, heading_seg)
             VALUES (new.id, new.body_seg, new.heading_seg);
         END;",
    )
    .map_err(|e| format!("Cannot create notebook index FTS triggers: {e}"))?;

    create_vec_table(conn, dim)?;
    Ok(())
}

fn create_vec_table(conn: &Connection, dim: usize) -> Result<(), String> {
    // 维度必须写死在建表语句里 —— vec0 的列类型就是 `float[N]`,没有运行时可变
    // 维度这回事。`dim` 来自 provider 探测,不是用户直接输入,但它仍然进了 SQL
    // 字符串,所以在这里断言它是个合理的数(见 `open` 的 dim == 0 检查与下面的上限)。
    if dim > 8192 {
        return Err(format!("Embedding dimension {dim} is out of range"));
    }
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(embedding float[{dim}]);"
    ))
    .map_err(|e| format!("Cannot create notebook vector table: {e}"))?;
    Ok(())
}

/// 老库迁移。目前只有 v1,没有可迁的路径 —— 但这个分支要留着,免得下次加列时
/// 有人图省事去改 `create_tables`(那只对新库生效,老库会静默缺列)。
fn migrate(_conn: &Connection, from: i64) -> Result<(), String> {
    match from {
        1 => Ok(()),
        other => Err(format!("Unknown notebook index schema version {other}")),
    }
}

/// 维度对不上就重建向量表。
///
/// 只动向量,不动 `chunks` —— 切块结果与 embedding 模型无关,重切一遍纯属浪费。
/// 所以状态给 `Stale` 而不是 `Pending`。
fn ensure_dim(conn: &Connection, dim: usize) -> Result<(), String> {
    let recorded = read_meta(conn, META_EMBED_DIM)?.and_then(|raw| raw.parse::<usize>().ok());
    if recorded == Some(dim) {
        // 表可能因为上次崩在重建中途而不存在,补一次。
        create_vec_table(conn, dim)?;
        return Ok(());
    }
    rebuild_vec_table(conn, dim)?;
    write_meta(conn, META_EMBED_DIM, &dim.to_string())?;
    Ok(())
}

/// 丢掉全部向量并把已索引的笔记标成 `Stale`。
///
/// `DROP` 与状态更新必须在同一个事务里:中间断电会留下「向量没了但状态还是
/// indexed」的库,而那个库的检索结果是静默残缺的 —— 搜不到的笔记看起来就像
/// 不存在。
pub fn rebuild_vec_table(conn: &Connection, dim: usize) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Cannot begin notebook index transaction: {e}"))?;
    tx.execute_batch("DROP TABLE IF EXISTS vec_chunks;")
        .map_err(|e| format!("Cannot drop notebook vector table: {e}"))?;
    create_vec_table(&tx, dim)?;
    tx.execute("UPDATE chunks SET embedded = 0", [])
        .map_err(|e| format!("Cannot reset notebook chunk embeddings: {e}"))?;
    tx.execute(
        "UPDATE docs SET status = ?1, error = NULL WHERE status IN ('indexed', 'failed')",
        [DocStatus::Stale.as_str()],
    )
    .map_err(|e| format!("Cannot mark notebook docs stale: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Cannot commit notebook index transaction: {e}"))?;
    Ok(())
}

pub fn read_meta(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM schema_meta WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| format!("Cannot read notebook index meta: {e}"))
}

pub fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO schema_meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .map_err(|e| format!("Cannot write notebook index meta: {e}"))?;
    Ok(())
}

/// 记下当前使用的 embedding 模型标识。
///
/// 与维度分开记:两个不同模型可以同维(都是 1536),此时向量表不必重建,但向量
/// 之间不可比 —— 混用会让检索结果看起来合理却是错的。换模型时调用方据此把笔记
/// 标 `Stale`。
pub fn embed_model(conn: &Connection) -> Result<Option<String>, String> {
    read_meta(conn, META_EMBED_MODEL)
}

pub fn set_embed_model(conn: &Connection, model: &str) -> Result<(), String> {
    write_meta(conn, META_EMBED_MODEL, model)
}

/// 换模型:同维时只让向量作废,不同维时由 [`open`] 的 `ensure_dim` 重建表。
pub fn mark_all_stale(conn: &Connection) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Cannot begin notebook index transaction: {e}"))?;
    tx.execute("DELETE FROM vec_chunks", [])
        .map_err(|e| format!("Cannot clear notebook vectors: {e}"))?;
    tx.execute("UPDATE chunks SET embedded = 0", [])
        .map_err(|e| format!("Cannot reset notebook chunk embeddings: {e}"))?;
    tx.execute(
        "UPDATE docs SET status = ?1, error = NULL WHERE status IN ('indexed', 'failed')",
        [DocStatus::Stale.as_str()],
    )
    .map_err(|e| format!("Cannot mark notebook docs stale: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Cannot commit notebook index transaction: {e}"))?;
    Ok(())
}

/// 删掉一篇笔记的全部派生数据。
///
/// `vec_chunks` 是虚表,不吃外键级联,必须在删 `chunks` **之前**按 rowid 显式
/// 删掉 —— 顺序反了就拿不到 id 列表,残留的向量会以幽灵命中的形式出现在检索
/// 结果里(有分数、有 id,回头查正文却是空的)。
pub fn delete_doc_payload(tx: &Connection, doc_id: i64) -> Result<(), String> {
    let ids: Vec<i64> = {
        let mut stmt = tx
            .prepare("SELECT id FROM chunks WHERE doc_id = ?1")
            .map_err(|e| format!("Cannot list notebook chunks: {e}"))?;
        let rows = stmt
            .query_map([doc_id], |row| row.get::<_, i64>(0))
            .map_err(|e| format!("Cannot list notebook chunks: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Cannot list notebook chunks: {e}"))?
    };
    for id in ids {
        tx.execute("DELETE FROM vec_chunks WHERE rowid = ?1", [id])
            .map_err(|e| format!("Cannot delete notebook vector: {e}"))?;
    }
    tx.execute("DELETE FROM chunks WHERE doc_id = ?1", [doc_id])
        .map_err(|e| format!("Cannot delete notebook chunks: {e}"))?;
    tx.execute("DELETE FROM links WHERE doc_id = ?1", [doc_id])
        .map_err(|e| format!("Cannot delete notebook links: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试一个独立目录,与 `notebook/tests.rs` 的 `temp_vault` 同一套命名 ——
    /// pid + 纳秒 + 计数器,并行跑不会撞。
    fn temp_vault() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(format!("aeroric-rag-db-{unique}"));
        std::fs::create_dir_all(&dir).expect("create temp vault");
        dir
    }

    #[test]
    fn opens_and_creates_schema() {
        let vault = temp_vault();
        let conn = open(&vault, 8).expect("open");
        assert_eq!(
            read_meta(&conn, META_SCHEMA_VERSION).unwrap().as_deref(),
            Some("1")
        );
        assert_eq!(
            read_meta(&conn, META_EMBED_DIM).unwrap().as_deref(),
            Some("8")
        );
    }

    #[test]
    fn db_lives_in_the_private_dir() {
        // 落在 vault 根目录的话会被笔记树扫到,用户会在列表里看到一个 .db。
        let vault = temp_vault();
        let path = db_path(&vault);
        assert!(path.starts_with(private_dir(&vault)));
    }

    #[test]
    fn rejects_zero_dimension() {
        let vault = temp_vault();
        assert!(open(&vault, 0).is_err());
    }

    #[test]
    fn rejects_absurd_dimension() {
        // dim 进了建表 SQL,上限是那处的护栏。
        let vault = temp_vault();
        assert!(open(&vault, 100_000).is_err());
    }

    #[test]
    fn vec_table_accepts_knn_query() {
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, '[1.0, 0.0, 0.0]')",
            [],
        )
        .expect("insert vector");
        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (2, '[0.0, 1.0, 0.0]')",
            [],
        )
        .expect("insert vector");
        let nearest: i64 = conn
            .query_row(
                "SELECT rowid FROM vec_chunks
                 WHERE embedding MATCH '[0.9, 0.1, 0.0]' AND k = 1",
                [],
                |row| row.get(0),
            )
            .expect("knn");
        assert_eq!(nearest, 1);
    }

    #[test]
    fn reopening_with_same_dim_keeps_vectors() {
        let vault = temp_vault();
        {
            let conn = open(&vault, 3).expect("open");
            conn.execute(
                "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, '[1.0, 0.0, 0.0]')",
                [],
            )
            .expect("insert");
        }
        let conn = open(&vault, 3).expect("reopen");
        let count: i64 = conn
            .query_row("SELECT count(*) FROM vec_chunks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1);
    }

    #[test]
    fn dimension_change_rebuilds_vectors_and_marks_stale() {
        let vault = temp_vault();
        {
            let conn = open(&vault, 3).expect("open");
            conn.execute(
                "INSERT INTO docs(path, status, hash) VALUES ('/a.md', 'indexed', 'h')",
                [],
            )
            .expect("doc");
            conn.execute(
                "INSERT INTO chunks(doc_id, ord, body, body_seg, embedded)
                 VALUES (1, 0, 'x', 'x', 1)",
                [],
            )
            .expect("chunk");
            conn.execute(
                "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, '[1.0, 0.0, 0.0]')",
                [],
            )
            .expect("vector");
        }
        // 换个维度重开 —— 混维度的 vec0 表是查询期报错,所以必须重建。
        let conn = open(&vault, 4).expect("reopen with new dim");
        let vectors: i64 = conn
            .query_row("SELECT count(*) FROM vec_chunks", [], |row| row.get(0))
            .expect("count vectors");
        assert_eq!(vectors, 0);
        let status: String = conn
            .query_row("SELECT status FROM docs WHERE id = 1", [], |row| row.get(0))
            .expect("status");
        assert_eq!(status, "stale");
        // 切块与模型无关,不该被丢掉 —— 否则换模型要白重切一遍全库。
        let chunks: i64 = conn
            .query_row("SELECT count(*) FROM chunks", [], |row| row.get(0))
            .expect("count chunks");
        assert_eq!(chunks, 1);
        let embedded: i64 = conn
            .query_row("SELECT embedded FROM chunks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("embedded flag");
        assert_eq!(embedded, 0);
        // 新维度要能真的用起来,不能只是表名在那儿。
        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, '[1.0, 0.0, 0.0, 0.0]')",
            [],
        )
        .expect("insert with new dim");
    }

    #[test]
    fn refuses_to_open_a_newer_schema() {
        let vault = temp_vault();
        {
            let conn = open(&vault, 3).expect("open");
            write_meta(&conn, META_SCHEMA_VERSION, "999").expect("bump");
        }
        let error = open(&vault, 3).expect_err("should refuse");
        assert!(error.contains("newer version"), "{error}");
    }

    #[test]
    fn fts_finds_chinese_through_segmented_column() {
        // 这是整个分词垫片存在的理由。原文列进 FTS 的话这里恒零命中。
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'pending')",
            [],
        )
        .expect("doc");
        let body = "随手记的导出功能已经完成";
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, ?1, ?2)",
            rusqlite::params![body, super::super::cjk::segment(body)],
        )
        .expect("chunk");

        let expr = super::super::cjk::match_expression("导出").expect("expr");
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                [&expr],
                |row| row.get(0),
            )
            .expect("match");
        assert_eq!(hits, 1, "两字中文查询必须命中");
    }

    #[test]
    fn fts_rebuild_works_without_reembedding() {
        // 外部内容表(而非无内容表)的理由:索引损坏能就地重建。
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'pending')",
            [],
        )
        .expect("doc");
        let body = "导出功能";
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, ?1, ?2)",
            rusqlite::params![body, super::super::cjk::segment(body)],
        )
        .expect("chunk");
        conn.execute("INSERT INTO chunks_fts(chunks_fts) VALUES ('rebuild')", [])
            .expect("rebuild");
        let expr = super::super::cjk::match_expression("导出").expect("expr");
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                [&expr],
                |row| row.get(0),
            )
            .expect("match");
        assert_eq!(hits, 1);
    }

    #[test]
    fn deleting_a_chunk_removes_it_from_fts() {
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'pending')",
            [],
        )
        .expect("doc");
        let body = "导出功能";
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, ?1, ?2)",
            rusqlite::params![body, super::super::cjk::segment(body)],
        )
        .expect("chunk");
        conn.execute("DELETE FROM chunks WHERE id = 1", [])
            .expect("delete");
        let expr = super::super::cjk::match_expression("导出").expect("expr");
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                [&expr],
                |row| row.get(0),
            )
            .expect("match");
        assert_eq!(hits, 0, "删掉的 chunk 还能搜到就是幽灵命中");
    }

    #[test]
    fn delete_doc_payload_clears_vectors_before_chunks() {
        // 顺序错了会留下有分数、有 id、查不到正文的幽灵向量。
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'indexed')",
            [],
        )
        .expect("doc");
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, 'x', 'x')",
            [],
        )
        .expect("chunk");
        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, '[1.0, 0.0, 0.0]')",
            [],
        )
        .expect("vector");
        conn.execute("INSERT INTO links(doc_id, target) VALUES (1, 'b')", [])
            .expect("link");

        delete_doc_payload(&conn, 1).expect("delete payload");

        for (table, label) in [
            ("vec_chunks", "向量"),
            ("chunks", "chunk"),
            ("links", "链接"),
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("count");
            assert_eq!(count, 0, "{label}没删干净");
        }
    }

    #[test]
    fn mark_all_stale_drops_vectors_but_keeps_chunks() {
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'indexed')",
            [],
        )
        .expect("doc");
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg, embedded) VALUES (1, 0, 'x', 'x', 1)",
            [],
        )
        .expect("chunk");
        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, '[1.0, 0.0, 0.0]')",
            [],
        )
        .expect("vector");

        mark_all_stale(&conn).expect("mark stale");

        let vectors: i64 = conn
            .query_row("SELECT count(*) FROM vec_chunks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(vectors, 0);
        let chunks: i64 = conn
            .query_row("SELECT count(*) FROM chunks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(chunks, 1);
        let status: String = conn
            .query_row("SELECT status FROM docs WHERE id = 1", [], |row| row.get(0))
            .expect("status");
        assert_eq!(status, "stale");
    }

    #[test]
    fn pending_docs_are_not_reset_by_stale_marking() {
        // pending 是「连切块都没做」,退成 stale 会让它被当成只缺向量而跳过切块。
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'pending')",
            [],
        )
        .expect("doc");
        mark_all_stale(&conn).expect("mark stale");
        let status: String = conn
            .query_row("SELECT status FROM docs WHERE id = 1", [], |row| row.get(0))
            .expect("status");
        assert_eq!(status, "pending");
    }

    #[test]
    fn deleting_a_doc_cascades_to_chunks() {
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'indexed')",
            [],
        )
        .expect("doc");
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, 'x', 'x')",
            [],
        )
        .expect("chunk");
        conn.execute("DELETE FROM docs WHERE id = 1", [])
            .expect("delete doc");
        let chunks: i64 = conn
            .query_row("SELECT count(*) FROM chunks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(chunks, 0, "外键级联没生效(PRAGMA foreign_keys 没开?)");
    }

    #[test]
    fn open_existing_does_not_create_the_db() {
        // 没建索引就搜索时不该在 vault 里凭空留一个空库。
        let vault = temp_vault();
        assert!(open_existing(&vault, 3).expect("probe").is_none());
        assert!(!db_path(&vault).exists());
        open(&vault, 3).expect("open");
        assert!(open_existing(&vault, 3).expect("probe").is_some());
    }

    #[test]
    fn doc_status_round_trips() {
        for status in [
            DocStatus::Pending,
            DocStatus::Indexed,
            DocStatus::Failed,
            DocStatus::Stale,
        ] {
            assert_eq!(DocStatus::from_str(status.as_str()), status);
        }
        // 未知值退回 Pending:让它被重索引,而不是让查询失败。
        assert_eq!(DocStatus::from_str("nonsense"), DocStatus::Pending);
    }

    #[test]
    fn embed_model_is_recorded_separately_from_dim() {
        // 同维不同模型的向量不可比,所以模型要单独记。
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        assert!(embed_model(&conn).unwrap().is_none());
        set_embed_model(&conn, "nomic-embed-text").expect("set");
        assert_eq!(
            embed_model(&conn).unwrap().as_deref(),
            Some("nomic-embed-text")
        );
    }

    #[test]
    fn wal_is_enabled() {
        // 索引是长任务,检索要能在它进行时读。
        let vault = temp_vault();
        let conn = open(&vault, 3).expect("open");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("mode");
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn peek_dim_reads_back_what_open_recorded() {
        let vault = temp_vault();
        assert_eq!(peek_dim(&vault), None, "还没建库时没有维度可读");
        let conn = open(&vault, 384).expect("open");
        drop(conn);
        assert_eq!(peek_dim(&vault), Some(384), "关掉连接之后仍读得出来");
    }

    #[test]
    fn peek_dim_leaves_no_wal_sidecars_behind() {
        // 偷看是最频繁的那条路(面板一打开、每次检索各一次)。库是 WAL,任何连接
        // 打开时都会建出 `-wal`/`-shm`;只读连接关闭时收不掉它们,于是每次偷看都在
        // 用户的 `.notebook/` 里留残留。这里盯的就是「偷看完目录跟偷看前一样」。
        let vault = temp_vault();
        let conn = open(&vault, 8).expect("open");
        drop(conn);

        let db = db_path(&vault);
        let wal = db.with_extension("db-wal");
        let shm = db.with_extension("db-shm");
        assert!(!wal.exists() && !shm.exists(), "前提:正常关闭后没有残留");

        assert_eq!(peek_dim(&vault), Some(8));

        assert!(!wal.exists(), "偷看之后 -wal 不该留在盘上");
        assert!(!shm.exists(), "偷看之后 -shm 不该留在盘上");
    }

    #[test]
    fn peek_dim_does_not_touch_the_schema() {
        // 偷看不能触发 `ensure_dim`:那会把向量表重建、全库标 stale。
        let vault = temp_vault();
        let conn = open(&vault, 5).expect("open");
        conn.execute(
            "INSERT INTO docs(path, title, hash, status) VALUES ('a.md', 'A', 'h', 'indexed')",
            [],
        )
        .expect("insert doc");
        drop(conn);

        assert_eq!(peek_dim(&vault), Some(5));

        let conn = open(&vault, 5).expect("reopen");
        let status: String = conn
            .query_row("SELECT status FROM docs WHERE path = 'a.md'", [], |row| {
                row.get(0)
            })
            .expect("status");
        assert_eq!(status, "indexed");
    }

    #[test]
    fn peek_dim_ignores_a_garbage_value() {
        // 库被手工改过 / 被别的版本写过。0 维会让 `open` 直接报错,当成「读不出来」
        // 更好 —— 调用方会去探测真实维度。
        let vault = temp_vault();
        let conn = open(&vault, 5).expect("open");
        write_meta(&conn, META_EMBED_DIM, "0").expect("write");
        drop(conn);
        assert_eq!(peek_dim(&vault), None);

        let conn = open(&vault, 5).expect("reopen");
        write_meta(&conn, META_EMBED_DIM, "not-a-number").expect("write");
        drop(conn);
        assert_eq!(peek_dim(&vault), None);
    }
}
