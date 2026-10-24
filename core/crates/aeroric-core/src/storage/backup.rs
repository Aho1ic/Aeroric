//! 数据目录的一致性备份 / 校验 / 恢复(AP8 Action 1)。
//!
//! ## 为什么不能是 `cp -r`
//!
//! 数据目录里有三个 **WAL 模式**的 SQLite 库:`notebook/rag/db.rs` 的 `index.db`、
//! `notebook/sync/store.rs` 的 `sync.db`、`metrics/mod.rs` 的 `usage-statistics.sqlite3`
//! (都在 vault 的 `.notebook/` 或数据根下)。直接复制一个正在写的 WAL 库会得到
//! **看起来完整、实际缺最后一次提交**的副本:主库文件落后于 `-wal`,而 `-wal` 与主库
//! 可能是不同时刻的快照。恢复时它要么打开成旧状态,要么直接 `database disk image is
//! malformed` —— 两种都是"备份里明明有文件"的静默数据损失。
//!
//! 所以数据库一律走 **rusqlite 的 backup API**(`Connection::backup`):它在源库上加锁、
//! 逐页复制、写进目标,产出的副本**自带一致的 WAL 内容**,并自动 checkpoint。这与
//! `database/dbx_connections.rs` 里服务 DBX 用户连接的那条路径是同一种机制。
//!
//! 非数据库文件走普通复制,但它们本来就是**原子写**(`storage::atomic` 的 temp+rename),
//! 所以读者永远看不到半个文件 —— 这正是备份可以不留停写窗口的前提。
//!
//! ## 不复制的东西,以及各自不进备份的理由
//!
//! 见 [`EPHEMERAL`]。这不是"顺手过滤":`.write.lock` 是**进程租约**,把它恢复出来等于
//! 伪造一个不存在的写者(ADR-027 的机器形状);`host.token` 是**凭据**,备份目录可能落在
//! 比数据目录更松的位置,复制它等于扩散密钥。
//!
//! ## 恢复不许静默覆盖
//!
//! 清单里每个文件都带备份当时的 sha256。恢复前逐个比对目标现状:对不上就是**备份之后
//! 有新写入**,此时拒绝并指名(pol. `docs/native/rollback.md` 的"never restore a stale
//! full-memory snapshot over newer writes"),除非调用方显式 `discard_newer`。
//! 这条是"恢复"与"回滚"的分界线,不能靠使用者记得。

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::atomic::{write_atomic, FileMode, StorageError};

/// 清单文件名(落在备份目录根部)。
pub const MANIFEST_NAME: &str = "manifest.json";

/// 清单格式版本。加字段要 +1,读侧对更高版本拒绝(与 `runtime.json` 同规矩)。
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// SQLite 文件头(POSIX/官方文档固定 16 字节)。**按魔数识别而不是按扩展名**:
/// 数据目录里三个库的命名各不相同(`index.db` / `sync.db` / `usage-statistics.sqlite3`),
/// 而扩展名清单会在下一个库出现时静默漏掉它 —— 漏掉的后果正是本模块要消灭的那种。
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// 不进备份的文件,以及各自不进的理由。**理由是一等公民**:下一个读者要能判断
/// 某个新文件该不该进这个表,而不是照着现有条目猜。
pub const EPHEMERAL: &[(&str, &str)] = &[
    (
        ".write.lock",
        "进程租约:恢复一个旧租约等于伪造一个不存在的写者(ADR-027)",
    ),
    (
        "host.sock",
        "unix socket:不是数据,复制出来的是一个连不上的名字",
    ),
    (
        "host.token",
        "凭据:备份目录可能落在比数据目录更松的位置,复制它等于扩散密钥",
    ),
];

/// 备份里的一个普通文件。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// 相对数据根的路径,用 `/` 分隔(清单可跨平台比对)。
    pub rel_path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// 一个 SQLite 库的**一致性证据**。没有这三项,清单只能证明"复制过",不能证明
/// "复制出来的是个能打开的库"。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseCheck {
    pub rel_path: String,
    /// `PRAGMA integrity_check` 的结果;通过时恰为 `ok`。
    pub integrity: String,
    /// 每张用户表的行数(`sqlite_%` 系统表除外)。恢复后要能对上。
    pub tables: BTreeMap<String, i64>,
}

/// 一份备份的自描述。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub schema_version: u32,
    pub created_at_ms: u64,
    /// 备份时的数据根(绝对路径,只作溯源;恢复目标由调用方给)。
    pub source_dir: String,
    /// 备份时是否**有活写者**。诚实记账:`live=true` 的真实含义是"每个库都走了
    /// backup API、每个文件都走原子写,所以快照内部自洽",而不是"当时没有并发写"。
    pub live: bool,
    #[serde(default)]
    pub entries: Vec<BackupEntry>,
    #[serde(default)]
    pub databases: Vec<DatabaseCheck>,
}

impl BackupManifest {
    /// 清单覆盖的全部相对路径(普通文件 + 数据库)。
    pub fn covered_paths(&self) -> Vec<&str> {
        self.entries
            .iter()
            .map(|e| e.rel_path.as_str())
            .chain(self.databases.iter().map(|d| d.rel_path.as_str()))
            .collect()
    }

    /// 按相对路径取普通文件的摘要。
    pub fn entry(&self, rel_path: &str) -> Option<&BackupEntry> {
        self.entries.iter().find(|e| e.rel_path == rel_path)
    }
}

fn io_err(context: &str, path: &Path, error: std::io::Error) -> StorageError {
    StorageError::WriteFailed {
        path: path.to_path_buf(),
        reason: format!("{context}: {error}"),
    }
}

fn read_err(context: &str, path: &Path, error: std::io::Error) -> StorageError {
    StorageError::Unreadable {
        path: path.to_path_buf(),
        reason: format!("{context}: {error}"),
    }
}

fn is_ephemeral(rel_path: &str) -> Option<&'static str> {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    EPHEMERAL
        .iter()
        .find(|(excluded, _)| *excluded == name)
        .map(|(_, reason)| *reason)
}

/// `x.db-wal` / `x.db-shm`:SQLite 的写前日志与共享内存索引,**不是独立数据**。
///
/// 它们必须被跳过,而理由不是"重复":`backup API` 产出的副本已经**自带**一致的 WAL
/// 内容。再把一份来自另一个时刻的 `-wal` 放进备份目录,恢复时它就会被当成那只库的
/// 日志来重放 —— 版本对不上正是 `database disk image is malformed` 的经典成因。
/// 所以处置方式是"**丢掉侧车、信 backup API**",而不是"连侧车一起复制"。
fn is_wal_sidecar(rel_path: &str) -> bool {
    rel_path.ends_with("-wal") || rel_path.ends_with("-shm")
}

/// 原子写留下的临时文件(`.name.<pid>-<nanos>.tmp`)不是数据,是垃圾。
fn is_temp(rel_path: &str) -> bool {
    rel_path
        .rsplit('/')
        .next()
        .is_some_and(|n| n.ends_with(".tmp") && (n.starts_with('.') || n.contains(".tmp.")))
}

fn to_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// 递归列出数据根下的全部文件(排序 → 确定性清单)。
fn walk_files(root: &Path) -> Result<Vec<PathBuf>, StorageError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = fs::read_dir(&dir).map_err(|e| read_err("cannot list directory", &dir, e))?;
        for entry in read {
            let entry = entry.map_err(|e| read_err("cannot read directory entry", &dir, e))?;
            let path = entry.path();
            let meta =
                fs::symlink_metadata(&path).map_err(|e| read_err("cannot stat", &path, e))?;
            if meta.file_type().is_symlink() {
                // 软链不跟着走:数据目录里的链接指向外部转录,备份它们会把
                // "只读的外部家目录"拉进这份副本,且可能成环。
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn sha256_file(path: &Path) -> Result<String, StorageError> {
    let mut file = fs::File::open(path).map_err(|e| read_err("cannot open", path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| read_err("cannot read", path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn looks_like_sqlite(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0u8; 16];
    file.read_exact(&mut header).is_ok() && &header == SQLITE_MAGIC
}

/// 用 **backup API** 把一个 SQLite 库复制到 `dest`,然后核对它能打开。
///
/// `src` 以只读打开:备份是读操作,不该因为这份副本而改动源库的 WAL 状态。
fn copy_database(src: &Path, dest: &Path) -> Result<DatabaseCheck, StorageError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| io_err("cannot create parent", parent, e))?;
    }
    let source =
        rusqlite::Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| StorageError::Unreadable {
                path: src.to_path_buf(),
                reason: format!("cannot open for backup: {e}"),
            })?;
    source
        .backup(rusqlite::DatabaseName::Main, dest, None)
        .map_err(|e| StorageError::ReplaceFailed {
            path: dest.to_path_buf(),
            reason: format!("backup API failed: {e}"),
        })?;
    let check = inspect_database(dest)?;
    // `inspect_database` 用**只读**连接打开副本,而 SQLite 对 WAL 模式的库**任何**连接
    // 一打开就会 materialize `-wal`/`-shm`(`notebook/rag/db.rs:174-179` 记着这条)。
    // 于是"检查一下备份"这个动作会在备份目录里留下两个没有清单认领的文件 —— 而备份
    // 目录里出现清单之外的东西,正是"这份备份到底是什么"开始说不清的地方。
    // backup API 返回时事务已提交、连接已关闭(收尾会 checkpoint),所以此刻主库文件
    // 自成一体,删掉刚被 materialize 的侧车不丢任何东西(discard-sidecars 的判据由
    // `a_wal_database_is_backed_up_through_the_backup_api_not_by_copying_bytes` 钉住)。
    remove_sidecars(dest);
    Ok(check)
}

/// 删掉某个库文件旁边的 `-wal`/`-shm`(只用于备份目录;源目录的侧车不属于本模块)。
fn remove_sidecars(database: &Path) {
    for suffix in ["-wal", "-shm"] {
        let mut name = database.as_os_str().to_os_string();
        name.push(suffix);
        let path = PathBuf::from(name);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
}

/// 打开一份**已落盘**的库,取出可对账的证据:`integrity_check` + 每张用户表的行数。
pub fn inspect_database(path: &Path) -> Result<DatabaseCheck, StorageError> {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| StorageError::Unreadable {
                path: path.to_path_buf(),
                reason: format!("cannot open copy: {e}"),
            })?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| StorageError::Unreadable {
            path: path.to_path_buf(),
            reason: format!("integrity_check failed: {e}"),
        })?;
    let mut tables = BTreeMap::new();
    if integrity == "ok" {
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| StorageError::Unreadable {
                path: path.to_path_buf(),
                reason: format!("cannot list tables: {e}"),
            })?;
        let names: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .and_then(|rows| rows.collect())
            .map_err(|e| StorageError::Unreadable {
                path: path.to_path_buf(),
                reason: format!("cannot read table names: {e}"),
            })?;
        for name in names {
            // 表名来自 sqlite_master,不是用户输入;双引号转义仍是必须的,
            // 因为建表语句里的引号会原样留在名字里。
            let sql = format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\""));
            let count: i64 = connection
                .query_row(&sql, [], |row| row.get(0))
                .map_err(|e| StorageError::Unreadable {
                    path: path.to_path_buf(),
                    reason: format!("cannot count {name}: {e}"),
                })?;
            tables.insert(name, count);
        }
    }
    Ok(DatabaseCheck {
        rel_path: String::new(),
        integrity,
        tables,
    })
}

/// 建一份备份:`source` → `dest`(含清单)。
///
/// `dest` 必须**不存在或为空**:往里合并是"两份备份叠在一起",清单会互相撒谎。
pub fn create(
    source: &Path,
    dest: &Path,
    now_ms: u64,
    live: bool,
) -> Result<BackupManifest, StorageError> {
    if !source.is_dir() {
        return Err(StorageError::Unreadable {
            path: source.to_path_buf(),
            reason: "backup source is not a directory".to_string(),
        });
    }
    if dest.exists() {
        let empty = fs::read_dir(dest)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !empty {
            return Err(StorageError::ReplaceFailed {
                path: dest.to_path_buf(),
                reason:
                    "backup destination is not empty; merging two backups lets one manifest lie about the other"
                        .to_string(),
            });
        }
    }
    fs::create_dir_all(dest).map_err(|e| io_err("cannot create backup dir", dest, e))?;

    let mut entries = Vec::new();
    let mut databases = Vec::new();
    for path in walk_files(source)? {
        let rel = to_rel(source, &path);
        if is_ephemeral(&rel).is_some() || is_temp(&rel) || is_wal_sidecar(&rel) {
            continue;
        }
        let target = dest.join(&rel);
        if looks_like_sqlite(&path) {
            let mut check = copy_database(&path, &target)?;
            check.rel_path = rel;
            databases.push(check);
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| io_err("cannot create parent", parent, e))?;
            }
            fs::copy(&path, &target).map_err(|e| io_err("cannot copy", &path, e))?;
            let bytes = fs::metadata(&target)
                .map_err(|e| read_err("cannot stat copy", &target, e))?
                .len();
            entries.push(BackupEntry {
                rel_path: rel,
                bytes,
                sha256: sha256_file(&target)?,
            });
        }
    }

    let manifest = BackupManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        created_at_ms: now_ms,
        source_dir: source.to_string_lossy().to_string(),
        live,
        entries,
        databases,
    };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| StorageError::WriteFailed {
        path: dest.join(MANIFEST_NAME),
        reason: format!("cannot serialize manifest: {e}"),
    })?;
    write_atomic(&dest.join(MANIFEST_NAME), &bytes, FileMode::Private)?;
    Ok(manifest)
}

/// 读一份清单。更高的 `schema_version` 直接拒绝(与 `runtime.json` 同规矩)。
pub fn read_manifest(backup_dir: &Path) -> Result<BackupManifest, StorageError> {
    let path = backup_dir.join(MANIFEST_NAME);
    let bytes = fs::read(&path).map_err(|e| read_err("cannot read manifest", &path, e))?;
    let manifest: BackupManifest =
        serde_json::from_slice(&bytes).map_err(|e| StorageError::Corrupt {
            path: path.clone(),
            reason: format!("manifest is not valid JSON: {e}"),
        })?;
    if manifest.schema_version > MANIFEST_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            path,
            found: manifest.schema_version,
            supported: MANIFEST_SCHEMA_VERSION,
        });
    }
    Ok(manifest)
}

/// 备份目录内的一个文件与清单对不上时,由谁造成的。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyProblem {
    pub rel_path: String,
    pub problem: String,
}

/// 重算备份目录里每个文件的 sha256,并对每个库再跑一遍 `integrity_check` + 行数。
///
/// 返回的列表**空 = 通过**。写成"返回问题列表"而不是 `Result<()>`,是因为使用者
/// 需要看到**全部**问题,而不是第一个。
pub fn verify(backup_dir: &Path) -> Result<Vec<VerifyProblem>, StorageError> {
    let manifest = read_manifest(backup_dir)?;
    let mut problems = Vec::new();

    if manifest.entries.is_empty() && manifest.databases.is_empty() {
        // 一份"什么都没覆盖"的清单是最危险的通过形态:它能让所有检查都成立。
        problems.push(VerifyProblem {
            rel_path: MANIFEST_NAME.to_string(),
            problem: "manifest covers no files at all — an empty backup is not a verified backup"
                .to_string(),
        });
        return Ok(problems);
    }

    for entry in &manifest.entries {
        let path = backup_dir.join(&entry.rel_path);
        match fs::metadata(&path) {
            Err(_) => problems.push(VerifyProblem {
                rel_path: entry.rel_path.clone(),
                problem: "listed in the manifest but missing from the backup".to_string(),
            }),
            Ok(meta) => {
                if meta.len() != entry.bytes {
                    problems.push(VerifyProblem {
                        rel_path: entry.rel_path.clone(),
                        problem: format!(
                            "size changed: manifest says {} bytes, file on disk has {}",
                            entry.bytes,
                            meta.len()
                        ),
                    });
                    continue;
                }
                let actual = sha256_file(&path)?;
                if actual != entry.sha256 {
                    problems.push(VerifyProblem {
                        rel_path: entry.rel_path.clone(),
                        problem: format!(
                            "content changed: manifest sha256 {} != recomputed {actual}",
                            entry.sha256
                        ),
                    });
                }
            }
        }
    }

    for recorded in &manifest.databases {
        let path = backup_dir.join(&recorded.rel_path);
        if !path.exists() {
            problems.push(VerifyProblem {
                rel_path: recorded.rel_path.clone(),
                problem: "database listed in the manifest but missing from the backup".to_string(),
            });
            continue;
        }
        let actual = inspect_database(&path)?;
        // 与 `copy_database` 同因:只读打开会把侧车 materialize 到备份目录里,
        // 而清单没认领它们。验证是只读动作,不该在产物里留下新文件。
        remove_sidecars(&path);
        if actual.integrity != "ok" {
            problems.push(VerifyProblem {
                rel_path: recorded.rel_path.clone(),
                problem: format!(
                    "integrity_check on the backup copy says {:?}, not ok — a copy that cannot be opened is not a backup",
                    actual.integrity
                ),
            });
            continue;
        }
        for (table, expected) in &recorded.tables {
            match actual.tables.get(table) {
                None => problems.push(VerifyProblem {
                    rel_path: recorded.rel_path.clone(),
                    problem: format!("table {table} recorded in the manifest is absent"),
                }),
                Some(found) if found != expected => problems.push(VerifyProblem {
                    rel_path: recorded.rel_path.clone(),
                    problem: format!(
                        "table {table} has {found} rows, manifest recorded {expected}"
                    ),
                }),
                Some(_) => {}
            }
        }
    }

    Ok(problems)
}

/// 恢复时拒绝覆盖的文件(备份之后有成规模的新写入)。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Divergence {
    pub rel_path: String,
    pub reason: String,
}

/// 恢复的结果:写回的文件数,以及**这次没被动过**的新写入。
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub written: usize,
    pub diverged: Vec<Divergence>,
}

/// 把备份恢复到 `dest`。
///
/// `discard_newer = false`(默认)时,任何一个目标文件与备份当时的摘要不同,就**整体拒绝**
/// 并把分歧全部列出来 —— "恢复"与"回滚掉别人刚写的东西"不能是同一句话(`rollback.md`
/// 规则 4)。要看清单里没提到、但目标里存在的新文件,`Divergence` 只覆盖**同名文件**;
/// 恢复也不删除任何清单外文件(删除是数据丢失,不是恢复)。
pub fn restore(
    backup_dir: &Path,
    dest: &Path,
    discard_newer: bool,
) -> Result<RestoreOutcome, StorageError> {
    let manifest = read_manifest(backup_dir)?;
    let mut diverged = Vec::new();
    for entry in &manifest.entries {
        let target = dest.join(&entry.rel_path);
        match fs::metadata(&target) {
            Ok(meta) if meta.is_file() => {
                let actual = sha256_file(&target)?;
                if actual != entry.sha256 {
                    diverged.push(Divergence {
                        rel_path: entry.rel_path.clone(),
                        reason:
                            "target differs from the backup — it was written after the backup was taken"
                                .to_string(),
                    });
                }
            }
            Ok(_) => diverged.push(Divergence {
                rel_path: entry.rel_path.clone(),
                reason: "target exists but is not a file".to_string(),
            }),
            Err(_) => {}
        }
    }
    for recorded in &manifest.databases {
        let target = dest.join(&recorded.rel_path);
        if target.exists() {
            // 库不可能按字节比对(backup API 的产物页序与源库不同),所以这里只回答
            // "它有没有被改过"这一件事 —— 用可对账的证据(行数 + integrity)。
            let actual = inspect_database(&target)?;
            if actual.integrity != "ok" {
                diverged.push(Divergence {
                    rel_path: recorded.rel_path.clone(),
                    reason: format!(
                        "existing database is not openable ({:?}); restoring over it would also destroy whatever it now holds",
                        actual.integrity
                    ),
                });
            } else if actual.tables != recorded.tables {
                diverged.push(Divergence {
                    rel_path: recorded.rel_path.clone(),
                    reason:
                        "existing database has different row counts than the backup — it has been written to since"
                            .to_string(),
                });
            }
        }
    }

    if !diverged.is_empty() && !discard_newer {
        return Err(StorageError::ContentDiverged {
            path: dest.to_path_buf(),
            expected: format!(
                "{} files as recorded in {}",
                manifest.covered_paths().len(),
                MANIFEST_NAME
            ),
            found: format!(
                "{} file(s) written after the backup: {}",
                diverged.len(),
                diverged
                    .iter()
                    .map(|d| d.rel_path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    let mut written = 0;
    for entry in &manifest.entries {
        let source = backup_dir.join(&entry.rel_path);
        let target = dest.join(&entry.rel_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err("cannot create parent", parent, e))?;
        }
        fs::copy(&source, &target).map_err(|e| io_err("cannot restore", &source, e))?;
        written += 1;
    }
    for recorded in &manifest.databases {
        let source = backup_dir.join(&recorded.rel_path);
        let target = dest.join(&recorded.rel_path);
        // 库也走 backup API:它写出的是一份自洽的库,而不是"主库文件 + 来源不明的 -wal"。
        copy_database(&source, &target)?;
        written += 1;
    }

    Ok(RestoreOutcome { written, diverged })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        use crate::clock::Clock as _;
        let root = std::env::temp_dir().join(format!(
            "aeroric-backup-{tag}-{}-{}",
            std::process::id(),
            crate::clock::SystemClock.now_ms()
        ));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// 造一个真 WAL 库,`rows` 行**已提交但未 checkpoint**:它们只在 `-wal` 里。
    ///
    /// 因此调用方必须**握着这个连接**去备份 —— 那正是"库正在被写"的真实形状。
    /// 关掉连接再备份是没有意义的:SQLite 会在最后一个连接关闭时 checkpoint,
    /// 行就回到主库文件里了,`cp` 也就"看起来能用"。
    fn wal_database(path: &Path, rows: i64) -> rusqlite::Connection {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .pragma_update(None, "journal_mode", "wal")
            .unwrap();
        connection
            .execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT)")
            .unwrap();
        for id in 0..rows {
            connection
                .execute(
                    "INSERT INTO notes (id, body) VALUES (?1, ?2)",
                    rusqlite::params![id, format!("row {id}")],
                )
                .unwrap();
        }
        connection
    }

    #[test]
    fn a_wal_database_is_backed_up_through_the_backup_api_not_by_copying_bytes() {
        let root = scratch("wal");
        let source = root.join("src");
        // 活连接:未 checkpoint 的 40 行留在 `-wal` 里。
        let live = wal_database(&source.join("index.db"), 40);

        // 前提:源库的主文件里确实看不到那 40 行(否则本用例证明不了任何事)。
        let raw = fs::read(source.join("index.db")).unwrap();
        assert!(
            !raw.windows(3).any(|w| w == b"row"),
            "the test fixture must keep its rows in the WAL, not in the main file"
        );

        let dest = root.join("out");
        let manifest = create(&source, &dest, 1_700_000_000_000, true).unwrap();
        let check = manifest.databases.first().expect("the db must be recorded");
        assert_eq!(check.integrity, "ok");
        assert_eq!(
            check.tables.get("notes"),
            Some(&40),
            "backup API must carry the WAL rows"
        );

        // 侧车不得进备份:副本自带一致的 WAL,再叠一份别人时刻的 `-wal` 会让恢复
        // 把旧日志重放到新库上(`database disk image is malformed` 的经典成因)。
        assert!(
            !dest.join("index.db-wal").exists(),
            "-wal must not be copied alongside the backup API output"
        );
        assert!(
            !dest.join("index.db-shm").exists(),
            "-shm must not be copied either"
        );

        assert!(
            verify(&dest).unwrap().is_empty(),
            "a fresh backup must verify clean"
        );
        drop(live);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_ephemeral_lease_and_the_credential_never_enter_the_backup() {
        let root = scratch("ephemeral");
        let source = root.join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("projects.json"), b"[]").unwrap();
        fs::write(source.join(".write.lock"), b"{\"pid\":1}").unwrap();
        fs::write(source.join("host.token"), b"secret").unwrap();

        let dest = root.join("out");
        let manifest = create(&source, &dest, 1, false).unwrap();
        assert_eq!(manifest.covered_paths(), vec!["projects.json"]);
        assert!(!dest.join(".write.lock").exists());
        assert!(
            !dest.join("host.token").exists(),
            "a credential must not be copied"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verification_names_a_backup_file_that_changed_after_the_fact() {
        let root = scratch("tamper");
        let source = root.join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("projects.json"), b"[{\"id\":\"p1\"}]").unwrap();
        let dest = root.join("out");
        create(&source, &dest, 1, false).unwrap();
        assert!(verify(&dest).unwrap().is_empty());

        fs::write(dest.join("projects.json"), b"[{\"id\":\"p2\"}]").unwrap();
        let problems = verify(&dest).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].problem.contains("content changed"),
            "{problems:?}"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn restoring_refuses_to_overwrite_a_write_that_landed_after_the_backup() {
        let root = scratch("diverged");
        let source = root.join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("projects.json"), b"[{\"id\":\"p1\"}]").unwrap();
        let dest = root.join("out");
        create(&source, &dest, 1, false).unwrap();

        // 备份之后,源目录又写了一次 —— 恢复不能把它抹掉。
        fs::write(source.join("projects.json"), b"[{\"id\":\"p2\"}]").unwrap();
        let error = restore(&dest, &source, false).expect_err("a newer write must be refused");
        assert!(
            matches!(error, StorageError::ContentDiverged { .. }),
            "{error:?}"
        );
        assert_eq!(
            fs::read(source.join("projects.json")).unwrap(),
            b"[{\"id\":\"p2\"}]",
            "the refusal must leave the newer write untouched"
        );

        // 显式丢弃时才真的回滚,且分歧被如实报出来。
        let outcome = restore(&dest, &source, true).unwrap();
        assert_eq!(outcome.written, 1);
        assert_eq!(outcome.diverged.len(), 1);
        assert_eq!(
            fs::read(source.join("projects.json")).unwrap(),
            b"[{\"id\":\"p1\"}]"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_restored_database_opens_with_the_same_rows_as_the_backup() {
        let root = scratch("restoredb");
        let source = root.join("src");
        let live = wal_database(&source.join(".notebook/sync.db"), 7);
        let dest = root.join("out");
        create(&source, &dest, 1, true).unwrap();
        drop(live);

        fs::remove_dir_all(&source).unwrap();
        let outcome = restore(&dest, &source, false).unwrap();
        assert_eq!(outcome.written, 1);
        let check = inspect_database(&source.join(".notebook/sync.db")).unwrap();
        assert_eq!(check.integrity, "ok");
        assert_eq!(check.tables.get("notes"), Some(&7));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_empty_manifest_is_a_problem_not_a_pass() {
        let root = scratch("empty");
        let source = root.join("src");
        fs::create_dir_all(&source).unwrap();
        let dest = root.join("out");
        create(&source, &dest, 1, false).unwrap();
        let problems = verify(&dest).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].problem.contains("covers no files"),
            "{problems:?}"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_backup_will_not_be_merged_into_a_non_empty_destination() {
        let root = scratch("merge");
        let source = root.join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("projects.json"), b"[]").unwrap();
        let dest = root.join("out");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("already-here"), b"x").unwrap();
        let error = create(&source, &dest, 1, false).expect_err("merging must be refused");
        assert!(
            matches!(error, StorageError::ReplaceFailed { .. }),
            "{error:?}"
        );
        fs::remove_dir_all(root).ok();
    }
}
