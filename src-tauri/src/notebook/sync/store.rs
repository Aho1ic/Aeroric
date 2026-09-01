//! 同步基线库:`<vault>/.notebook/sync.db`。
//!
//! 存两样东西,都是**持久**的:
//!
//! ```text
//! remote     一个远端目标(云盘 / git / p2p)。带 device_id 与 seq —— 那个逻辑戳
//!            是用来替掉挂钟的,见下。
//! baseline   上次同步成功时,某个路径在本地和远端各是什么样。三方 diff 的第三路输入。
//! tombstone  本地删过什么。**这是本设计相对 Markio 修掉的缺口。**
//! ```
//!
//! 本地当前的扫描结果不进库(每轮重算,见 [`super::scan`])。
//!
//! ## tombstone 为什么必须落盘,且必须在「推断出删除」时就写
//!
//! Markio 那边 `addTombstone` 除了自己的单测没有任何调用点,`tombstones` 在生产里
//! 恒为空对象,于是它的 diff 里三个 tombstone 分支全是死代码。后果:本地删掉一篇、
//! 另一台设备还有,下一轮就判成「远端新文件」把它拉回来 —— 删除被复活。
//!
//! Aeroric 的 vault 上没有 watcher,「本地删了」只能由 diff 推断(基线有、本地无)。
//! 所以要在**推断出来的那一刻**写盘,而不是等远端删除成功之后 —— 后者在中途崩溃时
//! 什么都没留下,下一轮删除照样被复活。
//!
//! ## device_id 与 seq:不比挂钟
//!
//! `newest` 冲突策略如果比两边的 mtime,比的其实是两台机器的两个时钟。差几秒就会
//! 静默挑错边。这里给每台设备一个稳定 id 和一个单调 seq,顺序按 `(device, seq)` 的
//! 因果关系判;真判不出来(两边各自都比共同祖先新)就是**并发**,那是一档独立状态,
//! 要交给用户,不能随便挑一个。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::notebook::fs_ops::private_dir;

/// schema 版本。加表 / 加列都要 +1 并在 [`migrate`] 里补迁移。
const SCHEMA_VERSION: i64 = 1;

const META_SCHEMA_VERSION: &str = "schema_version";
const META_DEVICE_ID: &str = "device_id";

/// tombstone 的保留窗口。
///
/// 与 Markio 的默认值一致(7 天)。太短会让「一台设备离线超过窗口期」的删除复活;
/// 太长会让库里堆着永远用不上的记录。这个值是可以调的,过期判定集中在
/// [`live_tombstones`] 一处。
pub const TOMBSTONE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// 一个远端目标。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTarget {
    /// 调用方给的稳定 id(一个 vault 可以同时挂多个远端)。
    pub id: String,
    /// `cloud` / `git` / `p2p`。存字符串而非整数:出问题时 sqlite3 直接可读。
    pub kind: String,
    /// 远端根(云盘路径 / s3 prefix / git remote)。
    pub root: String,
    /// 上次同步成功的时间。**只用于显示**,不参与任何判定。
    pub last_sync_at: i64,
    /// 本设备在这个目标上的逻辑序号,每轮成功同步 +1。
    pub seq: i64,
}

/// 某个路径上次同步成功时的样子。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    pub path: String,
    /// 上次同步时本地内容的 hash(与 [`super::scan::FileSig::hash`] 同一口径)。
    pub local_hash: String,
    pub local_mtime_ms: i64,
    /// 上次同步时远端内容的 hash。
    ///
    /// 注意这是**内容** hash,不是 provider 的 etag。Aeroric 的 `StorageEntry` 根本
    /// 没有 etag,而拿 mtime 当 etag 用既会漏检(改了但 mtime 没动)又会误报
    /// (touch 一下没改内容)。所以远端那一侧的身份由我们自己写的 sidecar manifest
    /// 提供 —— 见 P8c。
    pub remote_hash: String,
    /// 写下这份远端内容的设备与它当时的 seq。判因果顺序用。
    pub remote_device: String,
    pub remote_seq: i64,
    pub synced_at: i64,
}

/// 本地删除的记录。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    pub path: String,
    pub deleted_at: i64,
    /// 删除时基线里记的远端 hash。用来判「远端有没有在删之后被改过」——
    /// 改过说明别的设备又编辑了,那就不该把删除传播过去。
    pub remote_hash: String,
}

/// 库的路径。
pub fn db_path(vault: &Path) -> PathBuf {
    private_dir(vault).join("sync.db")
}

/// 打开(必要时创建)某个 vault 的同步库。
pub fn open(vault: &Path) -> Result<Connection, String> {
    let dir = private_dir(vault);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create notebook sync dir: {e}"))?;
    let path = db_path(vault);
    let conn = Connection::open(&path).map_err(|e| format!("Cannot open notebook sync db: {e}"))?;
    configure(&conn)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// 库不存在时不创建。「看一下同步状态」这条路用这个 —— 用户还没配过同步就不该因为
/// 点开面板在 vault 里多一个空库。
pub fn open_existing(vault: &Path) -> Result<Option<Connection>, String> {
    if !db_path(vault).exists() {
        return Ok(None);
    }
    open(vault).map(Some)
}

fn configure(conn: &Connection) -> Result<(), String> {
    // WAL:同步是长任务,而 UI 会在它跑着的时候读进度和冲突列表。
    conn.pragma_update(None, "journal_mode", "wal")
        .map_err(|e| format!("Cannot enable WAL on notebook sync db: {e}"))?;
    // FULL 而非 NORMAL —— 与 RAG 索引库刻意不同。
    //
    // 索引是可重建的派生数据,断电丢掉最后几条的代价是重索引。基线不是:丢掉最后
    // 几条 baseline 会让下一轮 diff 把那些路径判成「没有基线的同名文件」从而进冲突,
    // 而丢掉 tombstone 直接让删除复活。这里每次提交等一次 fsync,换的是断电后
    // 「已经同步成功的事实」不会退回去。
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(|e| format!("Cannot configure notebook sync db: {e}"))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| format!("Cannot enable foreign keys on notebook sync db: {e}"))?;
    Ok(())
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
             key   TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );",
    )
    .map_err(|e| format!("Cannot init notebook sync meta: {e}"))?;

    let existing = read_meta(conn, META_SCHEMA_VERSION)?
        .and_then(|raw| raw.parse::<i64>().ok())
        .unwrap_or(0);
    if existing > SCHEMA_VERSION {
        // 降级运行:宁可报错也不要按旧 schema 往里写。基线写坏的后果是同步做出错误
        // 的删除决定。
        return Err(format!(
            "Notebook sync db was written by a newer version (schema {existing} > {SCHEMA_VERSION})"
        ));
    }
    if existing == 0 {
        create_tables(conn)?;
        write_meta(conn, META_SCHEMA_VERSION, &SCHEMA_VERSION.to_string())?;
    } else {
        migrate(conn, existing)?;
    }
    ensure_device_id(conn)?;
    Ok(())
}

fn create_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote (
             id           TEXT PRIMARY KEY,
             kind         TEXT NOT NULL,
             root         TEXT NOT NULL,
             last_sync_at INTEGER NOT NULL DEFAULT 0,
             seq          INTEGER NOT NULL DEFAULT 0
         );

         CREATE TABLE IF NOT EXISTS baseline (
             remote_id      TEXT NOT NULL REFERENCES remote(id) ON DELETE CASCADE,
             path           TEXT NOT NULL,
             local_hash     TEXT NOT NULL,
             local_mtime_ms INTEGER NOT NULL DEFAULT 0,
             remote_hash    TEXT NOT NULL,
             remote_device  TEXT NOT NULL DEFAULT '',
             remote_seq     INTEGER NOT NULL DEFAULT 0,
             synced_at      INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (remote_id, path)
         );

         CREATE TABLE IF NOT EXISTS tombstone (
             remote_id   TEXT NOT NULL REFERENCES remote(id) ON DELETE CASCADE,
             path        TEXT NOT NULL,
             deleted_at  INTEGER NOT NULL,
             remote_hash TEXT NOT NULL DEFAULT '',
             PRIMARY KEY (remote_id, path)
         );",
    )
    .map_err(|e| format!("Cannot create notebook sync tables: {e}"))?;
    Ok(())
}

fn migrate(_conn: &Connection, _from: i64) -> Result<(), String> {
    // 目前只有 v1。加列时在这里按 `from` 逐版补,别用 `CREATE TABLE IF NOT EXISTS`
    // 顶替 —— 那对已存在的表不加列,而缺列要到运行时查询才报错。
    Ok(())
}

/// 本设备的稳定 id。第一次打开库时生成并写进去。
///
/// 存在库里而不是全局设置里:它标识的是「这个 vault 的这份副本」。同一台机器上把
/// vault 复制一份出来当第二个副本用,两份必须是不同身份 —— 否则它们的 seq 会互相
/// 覆盖,而 diff 会以为对方的改动是自己写的。
fn ensure_device_id(conn: &Connection) -> Result<String, String> {
    if let Some(existing) = read_meta(conn, META_DEVICE_ID)? {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let id = new_device_id();
    write_meta(conn, META_DEVICE_ID, &id)?;
    Ok(id)
}

/// 本设备 id。
pub fn device_id(conn: &Connection) -> Result<String, String> {
    ensure_device_id(conn)
}

/// 生成一个设备 id。
///
/// 不引第三方 uuid crate:这个值只要在「同一个 vault 的几份副本」之间不撞就够,
/// 拿时间戳拼进程 id 再拼一个地址熵源即可。真撞了的后果是两份副本被当成同一台设备,
/// 而两份副本同时同步同一个远端本身就要冲突处理。
fn new_device_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    // 栈上一个局部变量的地址:ASLR 下每次进程都不同,补上同一毫秒内起两个进程的情况。
    let entropy = &nanos as *const u128 as usize;
    format!("{nanos:x}-{pid:x}-{entropy:x}")
}

fn read_meta(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM schema_meta WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| format!("Cannot read notebook sync meta: {e}"))
}

fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO schema_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .map_err(|e| format!("Cannot write notebook sync meta: {e}"))?;
    Ok(())
}

/// 登记(或更新)一个远端目标。`seq` 与 `last_sync_at` 不动 —— 它们由同步流程推进,
/// 重新登记一次配置不该把进度清零。
pub fn upsert_remote(conn: &Connection, id: &str, kind: &str, root: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("Remote id must not be empty".to_string());
    }
    conn.execute(
        "INSERT INTO remote (id, kind, root) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, root = excluded.root",
        [id, kind, root],
    )
    .map_err(|e| format!("Cannot save notebook sync remote: {e}"))?;
    Ok(())
}

pub fn get_remote(conn: &Connection, id: &str) -> Result<Option<RemoteTarget>, String> {
    conn.query_row(
        "SELECT id, kind, root, last_sync_at, seq FROM remote WHERE id = ?1",
        [id],
        |row| {
            Ok(RemoteTarget {
                id: row.get(0)?,
                kind: row.get(1)?,
                root: row.get(2)?,
                last_sync_at: row.get(3)?,
                seq: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("Cannot read notebook sync remote: {e}"))
}

pub fn list_remotes(conn: &Connection) -> Result<Vec<RemoteTarget>, String> {
    let mut stmt = conn
        .prepare("SELECT id, kind, root, last_sync_at, seq FROM remote ORDER BY id")
        .map_err(|e| format!("Cannot list notebook sync remotes: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(RemoteTarget {
                id: row.get(0)?,
                kind: row.get(1)?,
                root: row.get(2)?,
                last_sync_at: row.get(3)?,
                seq: row.get(4)?,
            })
        })
        .map_err(|e| format!("Cannot list notebook sync remotes: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Cannot list notebook sync remotes: {e}"))
}

/// 删掉一个远端目标,连同它的基线与 tombstone(靠 `ON DELETE CASCADE`)。
///
/// 这是**用户显式解绑**才该走的路。它会让下一次重新绑定退回冷启动(全量比对),
/// 那是对的 —— 解绑之后远端可能已经被别人改了,留着旧基线反而会做出错误的删除判断。
pub fn remove_remote(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM remote WHERE id = ?1", [id])
        .map_err(|e| format!("Cannot remove notebook sync remote: {e}"))?;
    Ok(())
}

/// 一轮同步成功后推进逻辑序号,并记下完成时间。返回新的 seq。
///
/// seq 单调递增且**只在成功时推进**:被取消或失败的一轮不该留下「我这边更新了」的
/// 痕迹,否则对端会以为我们有更晚的版本。
pub fn bump_seq(conn: &Connection, remote_id: &str, now_ms: i64) -> Result<i64, String> {
    let changed = conn
        .execute(
            "UPDATE remote SET seq = seq + 1, last_sync_at = ?2 WHERE id = ?1",
            rusqlite::params![remote_id, now_ms],
        )
        .map_err(|e| format!("Cannot bump notebook sync seq: {e}"))?;
    if changed == 0 {
        return Err(format!("Unknown notebook sync remote: {remote_id}"));
    }
    conn.query_row("SELECT seq FROM remote WHERE id = ?1", [remote_id], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|e| format!("Cannot read notebook sync seq: {e}"))
}

/// 写一条基线。同时清掉同路径的 tombstone —— 文件又同步上了,「它被删了」不再成立。
///
/// 两件事在一个事务里:分开做的话,崩在中间会留下「基线说它在、tombstone 说它删了」
/// 的库,而那两条记录会让下一轮 diff 对同一个路径同时命中两个分支。
pub fn set_baseline(conn: &Connection, remote_id: &str, base: &Baseline) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Cannot start notebook sync transaction: {e}"))?;
    tx.execute(
        "INSERT INTO baseline
             (remote_id, path, local_hash, local_mtime_ms, remote_hash, remote_device, remote_seq, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(remote_id, path) DO UPDATE SET
             local_hash = excluded.local_hash,
             local_mtime_ms = excluded.local_mtime_ms,
             remote_hash = excluded.remote_hash,
             remote_device = excluded.remote_device,
             remote_seq = excluded.remote_seq,
             synced_at = excluded.synced_at",
        rusqlite::params![
            remote_id,
            base.path,
            base.local_hash,
            base.local_mtime_ms,
            base.remote_hash,
            base.remote_device,
            base.remote_seq,
            base.synced_at,
        ],
    )
    .map_err(|e| format!("Cannot save notebook sync baseline: {e}"))?;
    tx.execute(
        "DELETE FROM tombstone WHERE remote_id = ?1 AND path = ?2",
        rusqlite::params![remote_id, base.path],
    )
    .map_err(|e| format!("Cannot clear notebook sync tombstone: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Cannot commit notebook sync baseline: {e}"))?;
    Ok(())
}

pub fn baselines(conn: &Connection, remote_id: &str) -> Result<Vec<Baseline>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT path, local_hash, local_mtime_ms, remote_hash, remote_device, remote_seq, synced_at
             FROM baseline WHERE remote_id = ?1 ORDER BY path",
        )
        .map_err(|e| format!("Cannot read notebook sync baselines: {e}"))?;
    let rows = stmt
        .query_map([remote_id], |row| {
            Ok(Baseline {
                path: row.get(0)?,
                local_hash: row.get(1)?,
                local_mtime_ms: row.get(2)?,
                remote_hash: row.get(3)?,
                remote_device: row.get(4)?,
                remote_seq: row.get(5)?,
                synced_at: row.get(6)?,
            })
        })
        .map_err(|e| format!("Cannot read notebook sync baselines: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Cannot read notebook sync baselines: {e}"))
}

/// 清掉一条基线,**不**留 tombstone。
///
/// 用在「双方都已确认删除」之后:两边都没了,基线留着会让将来重建同名文件时被判成
/// 本地删除再删一次远端。这跟 [`add_tombstone`] 是两种不同的删除语义,别混用 ——
/// 混用的方向如果错了,要么删除复活,要么新建的文件被当成删除。
pub fn clear_baseline(conn: &Connection, remote_id: &str, path: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM baseline WHERE remote_id = ?1 AND path = ?2",
        rusqlite::params![remote_id, path],
    )
    .map_err(|e| format!("Cannot clear notebook sync baseline: {e}"))?;
    Ok(())
}

/// 记一条本地删除,并把同路径的基线清掉。
///
/// **调用时机是「diff 推断出本地删除」的那一刻,不是「远端删除成功之后」。** 后者在
/// 中途崩溃时什么都没留下,下一轮把远端还在的那份当新文件拉回来 —— 删除被复活。这是
/// Markio 那边的实际缺口(`addTombstone` 除单测外无调用点)。
///
/// 两件事同一个事务,理由同 [`set_baseline`]。
pub fn add_tombstone(conn: &Connection, remote_id: &str, tomb: &Tombstone) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Cannot start notebook sync transaction: {e}"))?;
    tx.execute(
        "INSERT INTO tombstone (remote_id, path, deleted_at, remote_hash) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(remote_id, path) DO UPDATE SET
             deleted_at = excluded.deleted_at,
             remote_hash = excluded.remote_hash",
        rusqlite::params![remote_id, tomb.path, tomb.deleted_at, tomb.remote_hash],
    )
    .map_err(|e| format!("Cannot save notebook sync tombstone: {e}"))?;
    tx.execute(
        "DELETE FROM baseline WHERE remote_id = ?1 AND path = ?2",
        rusqlite::params![remote_id, tomb.path],
    )
    .map_err(|e| format!("Cannot clear notebook sync baseline: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Cannot commit notebook sync tombstone: {e}"))?;
    Ok(())
}

/// 还在窗口期内的 tombstone。
///
/// 过期的**不删**,只是不返回 —— 删除留给 [`prune_tombstones`] 一处做。查询顺手删会
/// 让「读一下状态」变成写操作,而只读连接上那会直接失败。
pub fn live_tombstones(
    conn: &Connection,
    remote_id: &str,
    now_ms: i64,
) -> Result<Vec<Tombstone>, String> {
    let cutoff = now_ms.saturating_sub(TOMBSTONE_TTL_MS);
    let mut stmt = conn
        .prepare(
            "SELECT path, deleted_at, remote_hash FROM tombstone
             WHERE remote_id = ?1 AND deleted_at > ?2 ORDER BY path",
        )
        .map_err(|e| format!("Cannot read notebook sync tombstones: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![remote_id, cutoff], |row| {
            Ok(Tombstone {
                path: row.get(0)?,
                deleted_at: row.get(1)?,
                remote_hash: row.get(2)?,
            })
        })
        .map_err(|e| format!("Cannot read notebook sync tombstones: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Cannot read notebook sync tombstones: {e}"))
}

/// 删掉过期的 tombstone。返回删了几条。
pub fn prune_tombstones(conn: &Connection, now_ms: i64) -> Result<usize, String> {
    let cutoff = now_ms.saturating_sub(TOMBSTONE_TTL_MS);
    conn.execute("DELETE FROM tombstone WHERE deleted_at <= ?1", [cutoff])
        .map_err(|e| format!("Cannot prune notebook sync tombstones: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 epoch 毫秒当基准,不从 0 或 1000 起算。
    ///
    /// 从小数字起算的话 `now - TTL` 会被 saturating 夹到 0,于是「过期」这个分支
    /// 永远测不到;而这类时间值一旦漏到 UI 上,1970 恰好又是「没读到」的哨兵值。
    const NOW: i64 = 1_760_000_000_000;

    fn temp_vault(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-sync-store-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp vault");
        dir
    }

    fn base(path: &str, local: &str, remote: &str) -> Baseline {
        Baseline {
            path: path.to_string(),
            local_hash: local.to_string(),
            local_mtime_ms: NOW,
            remote_hash: remote.to_string(),
            remote_device: "dev-a".to_string(),
            remote_seq: 3,
            synced_at: NOW,
        }
    }

    fn opened(tag: &str) -> (PathBuf, Connection) {
        let vault = temp_vault(tag);
        let conn = open(&vault).expect("open");
        upsert_remote(&conn, "r1", "cloud", "notes/").expect("remote");
        (vault, conn)
    }

    #[test]
    fn the_db_lands_in_the_private_dir() {
        // 不能落在 vault 根:那会被同步扫描看见,两台机器互相同步彼此的基线库。
        let vault = temp_vault("path");
        let path = db_path(&vault);
        assert!(path.starts_with(private_dir(&vault)));
        assert!(path.ends_with("sync.db"));
    }

    #[test]
    fn opening_twice_keeps_the_device_id() {
        // device_id 是身份。每次打开都换一个的话,同一台机器会被自己的基线当成
        // 「别的设备写的」,于是每轮都判成远端变更。
        let vault = temp_vault("devid");
        let first = {
            let conn = open(&vault).expect("open");
            device_id(&conn).expect("id")
        };
        let second = {
            let conn = open(&vault).expect("reopen");
            device_id(&conn).expect("id")
        };
        assert!(!first.is_empty());
        assert_eq!(first, second);
    }

    #[test]
    fn two_vault_copies_get_different_device_ids() {
        // 同一台机器上复制一份 vault 当第二个副本用,两份必须是不同身份 ——
        // 否则它们的 seq 互相覆盖,diff 会以为对方的改动是自己写的。
        let a = temp_vault("dev-a");
        let b = temp_vault("dev-b");
        let id_a = device_id(&open(&a).expect("open a")).expect("id a");
        let id_b = device_id(&open(&b).expect("open b")).expect("id b");
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn open_existing_does_not_create_the_db() {
        // 用户还没配同步就点开面板,不该因此在 vault 里多一个空库。
        let vault = temp_vault("existing");
        assert!(open_existing(&vault).expect("probe").is_none());
        assert!(!db_path(&vault).exists());
        drop(open(&vault).expect("open"));
        assert!(open_existing(&vault).expect("probe").is_some());
    }

    #[test]
    fn re_registering_a_remote_keeps_its_progress() {
        // 改一下远端配置(比如换个 root 拼法)不该把 seq 和上次同步时间清零 ——
        // seq 归零会让对端把我们的历史版本当成最新。
        let (_vault, conn) = opened("reregister");
        let seq = bump_seq(&conn, "r1", NOW).expect("bump");
        assert_eq!(seq, 1);
        upsert_remote(&conn, "r1", "cloud", "notes2/").expect("re-register");
        let remote = get_remote(&conn, "r1").expect("get").expect("present");
        assert_eq!(remote.root, "notes2/");
        assert_eq!(remote.seq, 1, "重新登记不该把 seq 清零");
        assert_eq!(remote.last_sync_at, NOW);
    }

    #[test]
    fn bumping_an_unknown_remote_is_an_error() {
        // 静默成功的话调用方会以为自己推进了 seq,而实际上没有 —— 对端永远看不到
        // 我们的更新。
        let (_vault, conn) = opened("unknown");
        assert!(bump_seq(&conn, "nope", NOW).is_err());
    }

    #[test]
    fn a_baseline_round_trips() {
        let (_vault, conn) = opened("roundtrip");
        set_baseline(&conn, "r1", &base("a.md", "111", "222")).expect("set");
        let all = baselines(&conn, "r1").expect("read");
        assert_eq!(all, vec![base("a.md", "111", "222")]);
    }

    #[test]
    fn baselines_are_scoped_per_remote() {
        // 一个 vault 可以同时挂多个远端。串了的话 A 远端的同步会拿 B 的基线做判断,
        // 表现是「刚同步完 A,B 那边突然要全量重传」。
        let (_vault, conn) = opened("scoped");
        upsert_remote(&conn, "r2", "git", "origin").expect("remote2");
        set_baseline(&conn, "r1", &base("a.md", "111", "222")).expect("set r1");
        set_baseline(&conn, "r2", &base("a.md", "333", "444")).expect("set r2");
        assert_eq!(baselines(&conn, "r1").expect("r1")[0].local_hash, "111");
        assert_eq!(baselines(&conn, "r2").expect("r2")[0].local_hash, "333");
    }

    #[test]
    fn writing_a_baseline_clears_any_tombstone_for_that_path() {
        // 文件又同步上了,「它被删了」不再成立。两条记录同时在库里的话,下一轮 diff
        // 会对同一个路径同时命中基线分支和 tombstone 分支。
        let (_vault, conn) = opened("clears-tomb");
        add_tombstone(
            &conn,
            "r1",
            &Tombstone {
                path: "a.md".to_string(),
                deleted_at: NOW,
                remote_hash: "222".to_string(),
            },
        )
        .expect("tomb");
        assert_eq!(live_tombstones(&conn, "r1", NOW).expect("live").len(), 1);

        set_baseline(&conn, "r1", &base("a.md", "111", "222")).expect("set");
        assert!(live_tombstones(&conn, "r1", NOW).expect("live").is_empty());
        assert_eq!(baselines(&conn, "r1").expect("read").len(), 1);
    }

    #[test]
    fn adding_a_tombstone_clears_the_baseline() {
        // 反向也必须成立,理由同上。
        let (_vault, conn) = opened("tomb-clears");
        set_baseline(&conn, "r1", &base("a.md", "111", "222")).expect("set");
        add_tombstone(
            &conn,
            "r1",
            &Tombstone {
                path: "a.md".to_string(),
                deleted_at: NOW,
                remote_hash: "222".to_string(),
            },
        )
        .expect("tomb");
        assert!(baselines(&conn, "r1").expect("read").is_empty());
        assert_eq!(live_tombstones(&conn, "r1", NOW).expect("live").len(), 1);
    }

    #[test]
    fn a_tombstone_survives_reopening_the_db() {
        // **这是 Markio 那个缺口的回归测试。** 那边 addTombstone 除单测外没有调用点,
        // tombstones 恒为空,于是删除会被复活。这里盯的是「写下去之后,重开进程还在」。
        let vault = temp_vault("tomb-persist");
        {
            let conn = open(&vault).expect("open");
            upsert_remote(&conn, "r1", "cloud", "notes/").expect("remote");
            add_tombstone(
                &conn,
                "r1",
                &Tombstone {
                    path: "gone.md".to_string(),
                    deleted_at: NOW,
                    remote_hash: "222".to_string(),
                },
            )
            .expect("tomb");
        }
        let conn = open(&vault).expect("reopen");
        let live = live_tombstones(&conn, "r1", NOW).expect("live");
        assert_eq!(live.len(), 1, "重开之后 tombstone 必须还在");
        assert_eq!(live[0].path, "gone.md");
        assert_eq!(live[0].remote_hash, "222");
    }

    #[test]
    fn an_expired_tombstone_is_not_live_but_is_still_on_disk() {
        // 过期判定与删除是两件事:查询顺手删会让「读一下状态」变成写操作。
        let (_vault, conn) = opened("expiry");
        let old = NOW - TOMBSTONE_TTL_MS - 1;
        add_tombstone(
            &conn,
            "r1",
            &Tombstone {
                path: "old.md".to_string(),
                deleted_at: old,
                remote_hash: "222".to_string(),
            },
        )
        .expect("tomb");
        assert!(live_tombstones(&conn, "r1", NOW).expect("live").is_empty());
        assert_eq!(prune_tombstones(&conn, NOW).expect("prune"), 1);
        assert_eq!(prune_tombstones(&conn, NOW).expect("prune again"), 0);
    }

    #[test]
    fn the_expiry_boundary_is_exclusive_on_both_sides() {
        // 边界:正好卡在 TTL 上的算过期,差 1 毫秒的还活着。两侧各断言一次,只测
        // 一侧的话把 `>` 写成 `>=` 不会被发现。
        let (_vault, conn) = opened("boundary");
        add_tombstone(
            &conn,
            "r1",
            &Tombstone {
                path: "edge.md".to_string(),
                deleted_at: NOW - TOMBSTONE_TTL_MS,
                remote_hash: String::new(),
            },
        )
        .expect("tomb");
        add_tombstone(
            &conn,
            "r1",
            &Tombstone {
                path: "inside.md".to_string(),
                deleted_at: NOW - TOMBSTONE_TTL_MS + 1,
                remote_hash: String::new(),
            },
        )
        .expect("tomb");
        let live = live_tombstones(&conn, "r1", NOW).expect("live");
        let paths: Vec<&str> = live.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(paths, vec!["inside.md"], "正好等于 TTL 的算过期");
    }

    #[test]
    fn clearing_a_baseline_leaves_no_tombstone() {
        // 「双方都确认删了」和「本地删了要传播」是两种语义。这条走前者:两边都没了,
        // 基线清掉但不留 tombstone,否则将来重建同名文件会被判成删除。
        let (_vault, conn) = opened("clear");
        set_baseline(&conn, "r1", &base("a.md", "111", "222")).expect("set");
        clear_baseline(&conn, "r1", "a.md").expect("clear");
        assert!(baselines(&conn, "r1").expect("read").is_empty());
        assert!(live_tombstones(&conn, "r1", NOW).expect("live").is_empty());
    }

    #[test]
    fn removing_a_remote_takes_its_baselines_and_tombstones_with_it() {
        // 靠 ON DELETE CASCADE,而那需要 foreign_keys=ON —— 忘了开的话孤儿行会留在
        // 库里,重新绑同一个 id 时把陈旧基线当成有效的。
        let (_vault, conn) = opened("cascade");
        set_baseline(&conn, "r1", &base("a.md", "111", "222")).expect("set");
        add_tombstone(
            &conn,
            "r1",
            &Tombstone {
                path: "b.md".to_string(),
                deleted_at: NOW,
                remote_hash: String::new(),
            },
        )
        .expect("tomb");
        remove_remote(&conn, "r1").expect("remove");
        assert!(get_remote(&conn, "r1").expect("get").is_none());
        assert!(baselines(&conn, "r1").expect("read").is_empty());
        assert!(live_tombstones(&conn, "r1", NOW).expect("live").is_empty());
    }

    #[test]
    fn an_empty_remote_id_is_refused() {
        // 空 id 会和「没指定远端」混在一起,而那时候所有基线会挤在同一个键下。
        let (_vault, conn) = opened("empty-id");
        assert!(upsert_remote(&conn, "", "cloud", "notes/").is_err());
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_written_to() {
        // 按旧 schema 往里写会把基线写坏,而基线写坏的后果是同步做出错误的删除决定。
        let vault = temp_vault("schema");
        {
            let conn = open(&vault).expect("open");
            write_meta(
                &conn,
                META_SCHEMA_VERSION,
                &(SCHEMA_VERSION + 1).to_string(),
            )
            .expect("bump schema");
        }
        let err = open(&vault).expect_err("应该拒绝");
        assert!(err.contains("newer version"), "错误信息要说清原因: {err}");
    }

    #[test]
    fn the_db_is_configured_for_durability_not_speed() {
        // 与 RAG 索引库刻意不同:索引丢了能重建,基线丢了会让删除复活。
        let (_vault, conn) = opened("pragma");
        let sync: String = conn
            .query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
            .map(|v| v.to_string())
            .expect("read pragma");
        assert_eq!(sync, "2", "synchronous 要是 FULL(2),不是 NORMAL(1)");
        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("read journal");
        assert_eq!(journal.to_lowercase(), "wal");
    }
}
