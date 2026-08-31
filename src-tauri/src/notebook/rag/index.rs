//! 建索引:走一遍 vault,把变动的笔记重新切块并算向量。
//!
//! ## 三种状态转移
//!
//! 每篇笔记的处理方式由「内容有没有变」和「向量还算不算数」两个问题决定:
//!
//! ```text
//! hash 相同 + status=indexed  → 跳过。什么都不做。
//! hash 相同 + status=stale    → 只重算向量。切块结果与模型无关,重切纯属浪费。
//! hash 不同                   → 重切 + 重算。
//! status=failed              → 按上次失败的那一步重来(见 attempts 上限)。
//! ```
//!
//! `stale` 这一档是换 embedding 模型时省下的大头:一个几百篇笔记的库换模型,
//! 切块可以一篇都不用重做。
//!
//! ## 进度、取消、重试
//!
//! 这三件是计划里明确要求的验收项(也是 Markio ROADMAP 里没关掉的一条)。
//!
//! - **进度**通过 [`ProgressSink`] 回调往外报,不直接依赖 Tauri —— 这一层因此
//!   可以在没有 AppHandle 的测试里跑。发事件的活儿留给命令层。
//! - **取消**靠 [`CancelToken`],在每篇笔记与每个 embedding 批次之间检查。
//!   取消不回滚:已经算好的向量留在库里,下次接着往前走。
//! - **重试**靠 `docs.status`/`attempts` 落盘。失败的笔记不挡住其他笔记 ——
//!   一次运行里某篇超时,其余照常索引完,那一篇留在 `failed` 等下次。
//!
//! ## 事务粒度
//!
//! 一篇笔记的切块是一个事务,一个 embedding 批次的入库是一个事务。不做「整次
//! 运行一个大事务」:那样中途崩掉会把已经花掉的 embedding 全部丢掉,而 embedding
//! 是这条流水线上唯一花钱又花时间的一步。

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension};

use super::chunk;
use super::db::{self, DocStatus};
use super::embed::{self, EmbedConfig, EmbedError};
use super::graph;
use crate::notebook::state::hash64;
use crate::notebook::vault_index::split_frontmatter;
use crate::notebook::vault_walk::{walk_notes, WalkNext, MAX_FILES};

/// 单篇笔记的失败重试上限。
///
/// 超过之后常规运行不再碰它 —— 一篇因为内容本身有问题(比如超长到 provider 拒收)
/// 而永远失败的笔记,不该让每次索引都白花几次请求。用户显式点「重试失败项」
/// 或者笔记内容变了会重新开始计数。
const MAX_DOC_ATTEMPTS: i64 = 5;

/// 进度事件。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub phase: IndexPhase,
    /// 本次要处理的笔记总数(已跳过的不算)。
    pub total: usize,
    /// 已处理完的笔记数(含失败的)。
    pub done: usize,
    pub failed: usize,
    /// 正在处理的笔记路径。收尾阶段为 None。
    pub current: Option<String>,
    /// 整次运行失败时的原因。单篇失败不进这里 —— 那种失败记在 `docs.error`。
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IndexPhase {
    /// 正在走 vault、比对指纹。
    Scanning,
    /// 正在切块。
    Chunking,
    /// 正在算向量。
    Embedding,
    Done,
    Cancelled,
    /// 整次运行没能进行下去(库打不开、provider 配置不对)。
    Failed,
}

/// 进度回调。
///
/// 用 trait 而不是直接收 `AppHandle`:这一层的逻辑(状态转移、事务边界、失败
/// 隔离)是最该被测的部分,而它不需要知道 Tauri 存在。
///
/// `Sync` 是必须的:`&dyn ProgressSink` 要跨 await 点活着,而 `&T` 只有在
/// `T: Sync` 时才是 `Send` —— 不加的话 `index_vault` 的 future 不是 `Send`,
/// `#[tauri::command]` 直接编译不过。
pub trait ProgressSink: Sync {
    fn report(&self, progress: &IndexProgress);
}

/// 丢弃一切进度。
///
/// 只有测试在用 —— 命令层给的是往前端发事件的 sink。所以 `cfg(test)`:不加的话
/// lib target 编译时它没有构造点,clippy 报一条永远在那儿的 dead_code 警告,而
/// 长期挂着的警告会把真正的新警告淹掉。
#[cfg(test)]
pub struct NoProgress;

#[cfg(test)]
impl ProgressSink for NoProgress {
    fn report(&self, _progress: &IndexProgress) {}
}

/// 取消标志。克隆共享同一个底层 flag。
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 一次索引运行的结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexOutcome {
    pub indexed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub removed: usize,
    pub cancelled: bool,
}

/// 本次运行要覆盖哪些笔记。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IndexScope {
    /// 全库:按指纹决定每篇做什么。
    #[default]
    All,
    /// 只重试上次失败的与没做完的。用户点「重试」走这一条。
    ///
    /// 这一档会把 `attempts` 归零 —— 用户显式要求重试,意思就是「再给它一次
    /// 完整的机会」,而不是「在上次快用完的额度上再试一下」。
    FailedOnly,
}

/// 一篇待处理的笔记。
struct Pending {
    doc_id: i64,
    path: String,
    /// 需要重新切块。false 表示切块还有效,只缺向量。
    rechunk: bool,
    /// 正文(已剥掉 frontmatter)。`rechunk` 为 false 时不需要,置空。
    body: String,
    // 不带 title:`docs.title` 已经在 `insert_doc` / `touch_doc` 里写了。hash 覆盖
    // 整个文件(含 frontmatter),所以内容没变时标题也没变 —— 再带一份只会让人
    // 以为下游还要用它。
}

/// 建索引。
///
/// `config` 决定 provider 与模型;维度由调用方先探好并传进 `dim`(库的 schema
/// 需要它,而探测要发网络请求 —— 放在这里会让「打开库」这个动作变成可能失败的
/// 网络操作)。
pub async fn index_vault(
    vault: &Path,
    dim: usize,
    config: &EmbedConfig,
    scope: IndexScope,
    cancel: &CancelToken,
    sink: &dyn ProgressSink,
) -> Result<IndexOutcome, String> {
    let conn = db::open(vault, dim)?;
    // 换了模型但维度相同时向量不可比 —— 维度变化由 `db::open` 兜住,同维换模型
    // 只能在这里发现。
    let model = config.model.trim();
    if db::embed_model(&conn)?.as_deref() != Some(model) {
        db::mark_all_stale(&conn)?;
        db::set_embed_model(&conn, model)?;
    }

    report(sink, IndexPhase::Scanning, 0, 0, 0, None, None);

    let mut pending: Vec<Pending> = Vec::new();
    let mut skipped = 0usize;
    let mut seen: HashSet<String> = HashSet::new();
    let mut aborted_scan = false;

    // 第一趟:走 vault,决定每篇做什么。这一趟只读盘 + 比对指纹,不发网络请求,
    // 所以很快 —— 进度条能立刻显示出总数。
    let scan = walk_notes(vault, &mut |path, content| {
        if cancel.is_cancelled() {
            aborted_scan = true;
            return WalkNext::Stop;
        }
        let path_string = path.to_string_lossy().to_string();
        seen.insert(path_string.clone());
        match plan_doc(&conn, &path_string, content, scope) {
            Ok(Some(item)) => pending.push(item),
            Ok(None) => skipped += 1,
            // 单篇规划失败(库写不动)不该让整次扫描停下 —— 其余笔记仍该索引。
            // 这里没有可报的地方,留给它下次再来。
            Err(_) => {}
        }
        WalkNext::Continue
    });
    if let Err(error) = scan {
        report(sink, IndexPhase::Failed, 0, 0, 0, None, Some(error.clone()));
        return Err(error);
    }

    // 扫描被取消时直接收工。**不能**往下走到 prune —— 见下面那段。
    if aborted_scan || cancel.is_cancelled() {
        report(sink, IndexPhase::Cancelled, pending.len(), 0, 0, None, None);
        return Ok(IndexOutcome {
            indexed: 0,
            skipped,
            failed: 0,
            removed: 0,
            cancelled: true,
        });
    }

    // vault 里已经没有的笔记,连带它的派生数据一起删掉。
    //
    // 只在**扫描完整走完**时才做。`seen` 是「这次走到的文件」,而 prune 的判据是
    // 「不在 seen 里 = 已从 vault 消失」—— 这个推断只有在扫描没被截断时成立。
    // 扫描被取消或撞上 `MAX_FILES` 上限时 `seen` 是残缺的,拿它去 prune 会把
    // 还没走到的笔记全部从索引里删掉:用户按一下取消,索引就没了。
    let truncated = seen.len() >= MAX_FILES;
    let removed = if truncated {
        0
    } else {
        prune_missing(&conn, &seen)?
    };

    let total = pending.len();
    let mut done = 0usize;
    let mut failed = 0usize;
    let mut indexed = 0usize;

    for item in pending {
        if cancel.is_cancelled() {
            report(sink, IndexPhase::Cancelled, total, done, failed, None, None);
            return Ok(IndexOutcome {
                indexed,
                skipped,
                failed,
                removed,
                cancelled: true,
            });
        }
        report(
            sink,
            if item.rechunk {
                IndexPhase::Chunking
            } else {
                IndexPhase::Embedding
            },
            total,
            done,
            failed,
            Some(item.path.clone()),
            None,
        );

        match index_one(&conn, &item, config, cancel, sink, total, done, failed).await {
            Ok(true) => indexed += 1,
            // 取消:已经算完的留在库里,下次接着走。
            Ok(false) => {
                report(sink, IndexPhase::Cancelled, total, done, failed, None, None);
                return Ok(IndexOutcome {
                    indexed,
                    skipped,
                    failed,
                    removed,
                    cancelled: true,
                });
            }
            Err(error) => {
                // 单篇失败被隔离在这里:记下原因,继续下一篇。整次运行不因此中断
                // —— 一篇笔记超时不该让另外三百篇白等。
                failed += 1;
                let _ = mark_failed(&conn, item.doc_id, &error);
            }
        }
        done += 1;
    }

    report(sink, IndexPhase::Done, total, done, failed, None, None);
    Ok(IndexOutcome {
        indexed,
        skipped,
        failed,
        removed,
        cancelled: false,
    })
}

/// 决定一篇笔记要不要处理、怎么处理。
fn plan_doc(
    conn: &Connection,
    path: &str,
    content: &str,
    scope: IndexScope,
) -> Result<Option<Pending>, String> {
    // frontmatter 不进索引:它是元数据,把 `tags: [a, b]` 喂进 embedding 只会
    // 稀释正文的语义。标题另算(它进 chunk 的 heading)。
    let normalized = content.replace("\r\n", "\n");
    let (_front, body) = split_frontmatter(&normalized);
    // hash 按**原始**字节算,不按归一化后的:换行风格变化也是一次真实改动,
    // 而且这个 hash 要和 `state::signature_for` 对同一份内容给出同一个值。
    let hash = hash64(content.as_bytes()).to_string();
    let title = title_of(content, path);

    let existing: Option<(i64, String, String, i64)> = conn
        .query_row(
            "SELECT id, hash, status, attempts FROM docs WHERE path = ?1",
            [path],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("Cannot read notebook index doc: {e}"))?;

    let (doc_id, rechunk) = match existing {
        None => {
            // 新笔记。
            if matches!(scope, IndexScope::FailedOnly) {
                // 「只重试失败项」不该顺手把新笔记也拉进来 —— 用户点重试时期待的是
                // 一次小操作,而不是一次全库索引。
                return Ok(None);
            }
            let id = insert_doc(conn, path, &title, &hash)?;
            (id, true)
        }
        Some((id, old_hash, status, attempts)) => {
            let status = DocStatus::from_str(&status);
            let changed = old_hash != hash;
            match scope {
                IndexScope::FailedOnly => {
                    if !matches!(
                        status,
                        DocStatus::Failed | DocStatus::Pending | DocStatus::Stale
                    ) {
                        return Ok(None);
                    }
                    // 显式重试:额度归零,给它完整的一次机会。
                    reset_attempts(conn, id)?;
                }
                IndexScope::All => {
                    if !changed && matches!(status, DocStatus::Indexed) {
                        return Ok(None);
                    }
                    // 内容变了就重新开始计数 —— 上次失败可能正是因为那份旧内容。
                    if changed {
                        reset_attempts(conn, id)?;
                    } else if matches!(status, DocStatus::Failed) && attempts >= MAX_DOC_ATTEMPTS {
                        // 反复失败且内容没变:常规运行跳过它,别每次都白花请求。
                        return Ok(None);
                    }
                }
            }
            if changed {
                touch_doc(conn, id, &title, &hash)?;
            }
            // 内容没变时切块还有效,只缺向量。
            (id, changed)
        }
    };

    Ok(Some(Pending {
        doc_id,
        path: path.to_string(),
        rechunk,
        body: if rechunk {
            body.to_string()
        } else {
            String::new()
        },
    }))
}

/// 显示标题。与 `vault_index::derive_title` 同一套优先级,但那个函数是私有的,
/// 而这里只需要 frontmatter title 或文件名 —— chunk 的 heading 已经带了正文标题。
pub(super) fn title_of(content: &str, path: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    let (front, _body) = split_frontmatter(&normalized);
    for line in front.lines() {
        if let Some(value) = line.strip_prefix("title:") {
            let title = crate::notebook::vault_index::unquote_scalar(value.trim());
            if !title.trim().is_empty() {
                return title.trim().to_string();
            }
        }
    }
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn insert_doc(conn: &Connection, path: &str, title: &str, hash: &str) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO docs(path, title, hash, status, mtime_ms) VALUES (?1, ?2, ?3, ?4, 0)",
        rusqlite::params![path, title, hash, DocStatus::Pending.as_str()],
    )
    .map_err(|e| format!("Cannot insert notebook index doc: {e}"))?;
    Ok(conn.last_insert_rowid())
}

fn touch_doc(conn: &Connection, doc_id: i64, title: &str, hash: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE docs SET title = ?2, hash = ?3, status = ?4, error = NULL WHERE id = ?1",
        rusqlite::params![doc_id, title, hash, DocStatus::Pending.as_str()],
    )
    .map_err(|e| format!("Cannot update notebook index doc: {e}"))?;
    Ok(())
}

fn reset_attempts(conn: &Connection, doc_id: i64) -> Result<(), String> {
    conn.execute("UPDATE docs SET attempts = 0 WHERE id = ?1", [doc_id])
        .map_err(|e| format!("Cannot reset notebook index attempts: {e}"))?;
    Ok(())
}

fn mark_failed(conn: &Connection, doc_id: i64, error: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE docs SET status = ?2, error = ?3, attempts = attempts + 1 WHERE id = ?1",
        rusqlite::params![doc_id, DocStatus::Failed.as_str(), error],
    )
    .map_err(|e| format!("Cannot record notebook index failure: {e}"))?;
    Ok(())
}

/// 删掉 vault 里已经不存在的笔记。返回删了几篇。
fn prune_missing(conn: &Connection, seen: &HashSet<String>) -> Result<usize, String> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, path FROM docs")
            .map_err(|e| format!("Cannot list notebook index docs: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("Cannot list notebook index docs: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Cannot list notebook index docs: {e}"))?
    };
    let mut removed = 0usize;
    for (id, path) in rows {
        if seen.contains(&path) {
            continue;
        }
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("Cannot begin notebook index transaction: {e}"))?;
        db::delete_doc_payload(&tx, id)?;
        tx.execute("DELETE FROM docs WHERE id = ?1", [id])
            .map_err(|e| format!("Cannot delete notebook index doc: {e}"))?;
        tx.commit()
            .map_err(|e| format!("Cannot commit notebook index transaction: {e}"))?;
        removed += 1;
    }
    Ok(removed)
}

/// 处理一篇。返回 `false` 表示被取消。
#[allow(clippy::too_many_arguments)]
async fn index_one(
    conn: &Connection,
    item: &Pending,
    config: &EmbedConfig,
    cancel: &CancelToken,
    sink: &dyn ProgressSink,
    total: usize,
    done: usize,
    failed: usize,
) -> Result<bool, String> {
    if item.rechunk {
        rechunk_doc(conn, item)?;
    }

    // 取出还没有向量的块。重切之后是全部,只缺向量时是上次没做完的那些。
    let missing: Vec<(i64, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, heading, body FROM chunks
                 WHERE doc_id = ?1 AND embedded = 0 ORDER BY ord",
            )
            .map_err(|e| format!("Cannot list notebook chunks: {e}"))?;
        let rows = stmt
            .query_map([item.doc_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| format!("Cannot list notebook chunks: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Cannot list notebook chunks: {e}"))?
    };

    if missing.is_empty() {
        // 空笔记(或全是空白)。标成已索引,免得每次运行都来看一眼。
        mark_indexed(conn, item.doc_id)?;
        return Ok(true);
    }

    let inputs: Vec<String> = missing
        .iter()
        .map(|(_, heading, body)| embed_input(heading, body))
        .collect();

    let is_cancelled = || cancel.is_cancelled();
    for range in embed::batches(config.provider, inputs.len()) {
        if cancel.is_cancelled() {
            return Ok(false);
        }
        report(
            sink,
            IndexPhase::Embedding,
            total,
            done,
            failed,
            Some(item.path.clone()),
            None,
        );
        let vectors = embed::embed_batch(config, &inputs[range.clone()], &is_cancelled)
            .await
            .map_err(|error| describe_embed_error(&error))?;
        if cancel.is_cancelled() {
            return Ok(false);
        }
        // 一批入库一个事务:中途崩掉只丢这一批,已经花掉的 embedding 不白费。
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("Cannot begin notebook index transaction: {e}"))?;
        for (offset, vector) in vectors.iter().enumerate() {
            let chunk_id = missing[range.start + offset].0;
            store_vector(&tx, chunk_id, vector)?;
        }
        tx.commit()
            .map_err(|e| format!("Cannot commit notebook vectors: {e}"))?;
    }

    mark_indexed(conn, item.doc_id)?;
    Ok(true)
}

/// 取消在 embedding 层被表达成 `Transient("cancelled")`。转成本层的错误信息时
/// 要保留原文 —— 但取消不该被记成失败,所以调用方靠 `cancel` 自己判断,这里只
/// 负责把消息弄干净。
fn describe_embed_error(error: &EmbedError) -> String {
    error.message().to_string()
}

/// 喂给 embedding 的文本。
///
/// 标题路径拼在正文前面:一个「用 sqlite」的块单独看不出在讲什么,带上
/// 「设计 > 存储」之后语义完整得多,检索质量的差别很明显。
fn embed_input(heading: &str, body: &str) -> String {
    if heading.trim().is_empty() {
        return body.to_string();
    }
    format!("{heading}\n\n{body}")
}

fn store_vector(tx: &Connection, chunk_id: i64, vector: &[f32]) -> Result<(), String> {
    // vec0 吃 JSON 数组字面量。手拼而不是 serde:f32 的 Display 已经够用,
    // 而这条路每个 chunk 都要走一次。
    let mut json = String::with_capacity(vector.len() * 12 + 2);
    json.push('[');
    for (index, value) in vector.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&value.to_string());
    }
    json.push(']');
    // 先删再插:重试同一个 chunk 时 rowid 已存在,直接插会报唯一约束冲突。
    tx.execute("DELETE FROM vec_chunks WHERE rowid = ?1", [chunk_id])
        .map_err(|e| format!("Cannot replace notebook vector: {e}"))?;
    tx.execute(
        "INSERT INTO vec_chunks(rowid, embedding) VALUES (?1, ?2)",
        rusqlite::params![chunk_id, json],
    )
    .map_err(|e| format!("Cannot store notebook vector: {e}"))?;
    tx.execute("UPDATE chunks SET embedded = 1 WHERE id = ?1", [chunk_id])
        .map_err(|e| format!("Cannot mark notebook chunk embedded: {e}"))?;
    Ok(())
}

fn mark_indexed(conn: &Connection, doc_id: i64) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    conn.execute(
        "UPDATE docs SET status = ?2, error = NULL, indexed_at = ?3,
             chunk_count = (SELECT count(*) FROM chunks WHERE doc_id = ?1)
         WHERE id = ?1",
        rusqlite::params![doc_id, DocStatus::Indexed.as_str(), now],
    )
    .map_err(|e| format!("Cannot mark notebook doc indexed: {e}"))?;
    Ok(())
}

/// 重切一篇:丢掉旧的派生数据,写入新的块与链接。
fn rechunk_doc(conn: &Connection, item: &Pending) -> Result<(), String> {
    let chunks = chunk::split(&item.body);
    let links = extract_links(&item.body);
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Cannot begin notebook index transaction: {e}"))?;
    db::delete_doc_payload(&tx, item.doc_id)?;
    for (ord, piece) in chunks.iter().enumerate() {
        tx.execute(
            "INSERT INTO chunks(doc_id, ord, heading, body, heading_seg, body_seg,
                                char_start, char_end, token_count, embedded)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
            rusqlite::params![
                item.doc_id,
                ord as i64,
                piece.heading,
                piece.body,
                piece.heading_seg,
                piece.body_seg,
                piece.char_start as i64,
                piece.char_end as i64,
                piece.token_count as i64,
            ],
        )
        .map_err(|e| format!("Cannot insert notebook chunk: {e}"))?;
    }
    for target in links {
        // `target_norm` 与 `target` 必须同时写:反链走的是 `target_norm` 上的索引,
        // 漏写会让这条边在图里不存在(而不是报错)。
        let norm = graph::normalize_target(&target);
        tx.execute(
            "INSERT INTO links(doc_id, target, target_norm, kind)
             VALUES (?1, ?2, ?3, 'wikilink')",
            rusqlite::params![item.doc_id, target, norm],
        )
        .map_err(|e| format!("Cannot insert notebook link: {e}"))?;
    }
    tx.execute(
        "UPDATE docs SET chunk_count = ?2 WHERE id = ?1",
        rusqlite::params![item.doc_id, chunks.len() as i64],
    )
    .map_err(|e| format!("Cannot update notebook chunk count: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Cannot commit notebook chunks: {e}"))?;
    Ok(())
}

/// 抽出正文里的 wikilink 目标。
///
/// 复用 `links::scan_line` 而不是自己再找一遍 `[[` —— 那个函数是 wikilink 的
/// 定义所在(含前端正则的回溯行为)。自己写一遍会分叉,而分叉的表现是图谱里
/// 的边和反链面板里的条目不一致。
///
/// 连带继承了它**不认代码围栏**这一点:代码块里的 `[[x]]` 也算一条边。这是
/// 刻意与反链面板保持一致 —— 两处对「什么是一条链接」给出不同答案,比两处
/// 都偏宽松更让人困惑。
fn extract_links(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for line in body.lines() {
        if !line.contains("[[") {
            continue;
        }
        for hit in crate::notebook::links::scan_line(line) {
            // `[[目标|别名]]` 与 `[[目标#小节]]` 都指向同一篇。
            let target = hit
                .raw
                .split('|')
                .next()
                .unwrap_or(&hit.raw)
                .split('#')
                .next()
                .unwrap_or(&hit.raw)
                .trim()
                .to_string();
            if target.is_empty() {
                continue;
            }
            if seen.insert(target.clone()) {
                out.push(target);
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn report(
    sink: &dyn ProgressSink,
    phase: IndexPhase,
    total: usize,
    done: usize,
    failed: usize,
    current: Option<String>,
    error: Option<String>,
) {
    sink.report(&IndexProgress {
        phase,
        total,
        done,
        failed,
        current,
        error,
    });
}

/// 索引现状。给面板显示「已索引 N 篇 / 失败 M 篇」用。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub docs: usize,
    pub indexed: usize,
    pub pending: usize,
    pub failed: usize,
    pub stale: usize,
    pub chunks: usize,
    /// 失败笔记的路径与原因,给「哪些没成功」那个列表用。
    pub failures: Vec<IndexFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexFailure {
    pub path: String,
    pub error: String,
    pub attempts: i64,
}

/// 读索引现状。库不存在时返回全零 —— 那是「还没建过索引」,不是错误。
pub fn stats(vault: &Path, dim: usize) -> Result<IndexStats, String> {
    let Some(conn) = db::open_existing(vault, dim)? else {
        return Ok(IndexStats::default());
    };
    let mut stats = IndexStats::default();
    {
        let mut stmt = conn
            .prepare("SELECT status, count(*) FROM docs GROUP BY status")
            .map_err(|e| format!("Cannot read notebook index stats: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .map_err(|e| format!("Cannot read notebook index stats: {e}"))?;
        for row in rows {
            let (status, count) =
                row.map_err(|e| format!("Cannot read notebook index stats: {e}"))?;
            stats.docs += count;
            match DocStatus::from_str(&status) {
                DocStatus::Indexed => stats.indexed += count,
                DocStatus::Pending => stats.pending += count,
                DocStatus::Failed => stats.failed += count,
                DocStatus::Stale => stats.stale += count,
            }
        }
    }
    stats.chunks = conn
        .query_row("SELECT count(*) FROM chunks", [], |row| {
            row.get::<_, i64>(0).map(|v| v as usize)
        })
        .map_err(|e| format!("Cannot read notebook index stats: {e}"))?;
    {
        let mut stmt = conn
            .prepare(
                "SELECT path, coalesce(error, ''), attempts FROM docs
                 WHERE status = 'failed' ORDER BY path LIMIT 200",
            )
            .map_err(|e| format!("Cannot read notebook index failures: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(IndexFailure {
                    path: row.get(0)?,
                    error: row.get(1)?,
                    attempts: row.get(2)?,
                })
            })
            .map_err(|e| format!("Cannot read notebook index failures: {e}"))?;
        stats.failures = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Cannot read notebook index failures: {e}"))?;
    }
    Ok(stats)
}

/// 丢掉整个索引。用户换 provider 又不想留旧数据时用。
pub fn clear(vault: &Path) -> Result<(), String> {
    let path = db::db_path(vault);
    for suffix in ["", "-wal", "-shm"] {
        let target = if suffix.is_empty() {
            path.clone()
        } else {
            std::path::PathBuf::from(format!("{}{suffix}", path.to_string_lossy()))
        };
        if target.exists() {
            std::fs::remove_file(&target)
                .map_err(|e| format!("Cannot remove notebook index: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn temp_vault() -> std::path::PathBuf {
        use std::sync::atomic::AtomicU32;
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
        let dir = std::env::temp_dir().join(format!("aeroric-rag-index-{unique}"));
        std::fs::create_dir_all(&dir).expect("create temp vault");
        dir
    }

    /// 记下所有进度事件,供断言「进度确实报出来了」。
    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<IndexProgress>>,
    }

    impl ProgressSink for Recorder {
        fn report(&self, progress: &IndexProgress) {
            self.events.lock().unwrap().push(progress.clone());
        }
    }

    impl Recorder {
        fn phases(&self) -> Vec<IndexPhase> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.phase)
                .collect()
        }
    }

    fn write_note(vault: &Path, name: &str, body: &str) {
        std::fs::write(vault.join(name), body).expect("write note");
    }

    #[test]
    fn extracts_wikilink_targets() {
        let links = extract_links("见 [[计划]] 和 [[设计|架构]]。\n");
        assert_eq!(links, vec!["计划", "设计"]);
    }

    #[test]
    fn strips_section_anchors_from_link_targets() {
        // `[[目标#小节]]` 指向的仍是那一篇。
        assert_eq!(extract_links("[[计划#里程碑]]"), vec!["计划"]);
    }

    #[test]
    fn deduplicates_repeated_links() {
        // 图谱里一篇指向另一篇只该有一条边。
        assert_eq!(extract_links("[[a]] [[a]] [[a]]"), vec!["a"]);
    }

    #[test]
    fn ignores_lines_without_links() {
        assert!(extract_links("普通正文,没有链接。").is_empty());
    }

    #[test]
    fn embed_input_prepends_the_heading_path() {
        // 「用 sqlite」单独看不出在讲什么。
        assert_eq!(
            embed_input("设计 > 存储", "用 sqlite"),
            "设计 > 存储\n\n用 sqlite"
        );
    }

    #[test]
    fn embed_input_omits_empty_heading() {
        assert_eq!(embed_input("", "正文"), "正文");
        assert_eq!(embed_input("  ", "正文"), "正文");
    }

    #[test]
    fn title_prefers_frontmatter() {
        let content = "---\ntitle: 周报\n---\n\n# 别的标题\n";
        assert_eq!(title_of(content, "/x/cao-gao.md"), "周报");
    }

    #[test]
    fn title_falls_back_to_file_stem() {
        assert_eq!(title_of("没有 frontmatter\n", "/x/cao-gao.md"), "cao-gao");
    }

    #[test]
    fn stats_on_a_missing_db_are_all_zero() {
        // 「还没建过索引」不是错误。
        let vault = temp_vault();
        let stats = stats(&vault, 8).expect("stats");
        assert_eq!(stats.docs, 0);
        assert_eq!(stats.chunks, 0);
        assert!(stats.failures.is_empty());
    }

    #[test]
    fn clear_removes_the_db_and_its_wal() {
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        drop(conn);
        assert!(db::db_path(&vault).exists());
        clear(&vault).expect("clear");
        assert!(!db::db_path(&vault).exists());
    }

    #[test]
    fn clear_on_a_missing_db_is_not_an_error() {
        let vault = temp_vault();
        clear(&vault).expect("clear should tolerate a missing db");
    }

    #[test]
    fn plan_skips_unchanged_indexed_docs() {
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        let content = "# 标题\n\n正文\n";
        let path = "/vault/a.md";
        // 第一次:新笔记,要处理。
        let first = plan_doc(&conn, path, content, IndexScope::All)
            .expect("plan")
            .expect("should be pending");
        assert!(first.rechunk);
        mark_indexed(&conn, first.doc_id).expect("mark");
        // 第二次:内容没变且已索引 → 跳过。
        assert!(plan_doc(&conn, path, content, IndexScope::All)
            .expect("plan")
            .is_none());
    }

    #[test]
    fn plan_reembeds_stale_docs_without_rechunking() {
        // 换模型省下的大头:切块不用重做。
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        let content = "# 标题\n\n正文\n";
        let path = "/vault/a.md";
        let first = plan_doc(&conn, path, content, IndexScope::All)
            .expect("plan")
            .expect("pending");
        mark_indexed(&conn, first.doc_id).expect("mark");
        db::mark_all_stale(&conn).expect("stale");

        let again = plan_doc(&conn, path, content, IndexScope::All)
            .expect("plan")
            .expect("stale docs must be picked up");
        assert!(!again.rechunk, "内容没变却要重切,白白浪费");
    }

    #[test]
    fn plan_rechunks_when_content_changes() {
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        let path = "/vault/a.md";
        let first = plan_doc(&conn, path, "原来的内容\n", IndexScope::All)
            .expect("plan")
            .expect("pending");
        mark_indexed(&conn, first.doc_id).expect("mark");
        let again = plan_doc(&conn, path, "改过的内容\n", IndexScope::All)
            .expect("plan")
            .expect("changed docs must be picked up");
        assert!(again.rechunk);
    }

    #[test]
    fn failed_only_scope_ignores_new_docs() {
        // 点「重试失败项」不该变成一次全库索引。
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        assert!(
            plan_doc(&conn, "/vault/new.md", "内容\n", IndexScope::FailedOnly)
                .expect("plan")
                .is_none()
        );
    }

    #[test]
    fn failed_only_scope_picks_up_failed_docs_and_resets_attempts() {
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        let path = "/vault/a.md";
        let first = plan_doc(&conn, path, "内容\n", IndexScope::All)
            .expect("plan")
            .expect("pending");
        // 撞满重试额度。
        for _ in 0..MAX_DOC_ATTEMPTS {
            mark_failed(&conn, first.doc_id, "boom").expect("fail");
        }
        // 常规运行已经放弃它了。
        assert!(plan_doc(&conn, path, "内容\n", IndexScope::All)
            .expect("plan")
            .is_none());
        // 但用户显式重试要给它完整的一次机会。
        let retried = plan_doc(&conn, path, "内容\n", IndexScope::FailedOnly)
            .expect("plan")
            .expect("explicit retry must pick it up");
        let attempts: i64 = conn
            .query_row(
                "SELECT attempts FROM docs WHERE id = ?1",
                [retried.doc_id],
                |row| row.get(0),
            )
            .expect("attempts");
        assert_eq!(attempts, 0, "显式重试没有重置额度");
    }

    #[test]
    fn changed_content_resets_the_attempt_budget() {
        // 上次失败可能正是因为那份旧内容。
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        let path = "/vault/a.md";
        let first = plan_doc(&conn, path, "旧内容\n", IndexScope::All)
            .expect("plan")
            .expect("pending");
        for _ in 0..MAX_DOC_ATTEMPTS {
            mark_failed(&conn, first.doc_id, "boom").expect("fail");
        }
        assert!(plan_doc(&conn, path, "新内容\n", IndexScope::All)
            .expect("plan")
            .is_some());
        let attempts: i64 = conn
            .query_row(
                "SELECT attempts FROM docs WHERE id = ?1",
                [first.doc_id],
                |row| row.get(0),
            )
            .expect("attempts");
        assert_eq!(attempts, 0);
    }

    #[test]
    fn rechunk_replaces_old_chunks_and_links() {
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        let path = "/vault/a.md";
        let first = plan_doc(&conn, path, "见 [[旧目标]]\n", IndexScope::All)
            .expect("plan")
            .expect("pending");
        rechunk_doc(&conn, &first).expect("rechunk");
        let item = Pending {
            doc_id: first.doc_id,
            path: path.to_string(),
            rechunk: true,
            body: "见 [[新目标]]\n".to_string(),
        };
        rechunk_doc(&conn, &item).expect("rechunk again");
        let targets: Vec<String> = {
            let mut stmt = conn.prepare("SELECT target FROM links").expect("prepare");
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query");
            rows.collect::<Result<Vec<_>, _>>().expect("collect")
        };
        assert_eq!(targets, vec!["新目标"], "旧链接没被清掉");
    }

    #[test]
    fn store_vector_is_idempotent() {
        // 重试同一个 chunk 时 rowid 已存在,直接插会撞唯一约束。
        let vault = temp_vault();
        let conn = db::open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'pending')",
            [],
        )
        .expect("doc");
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, 'x', 'x')",
            [],
        )
        .expect("chunk");
        store_vector(&conn, 1, &[1.0, 0.0, 0.0]).expect("first");
        store_vector(&conn, 1, &[0.0, 1.0, 0.0]).expect("second must not conflict");
        let count: i64 = conn
            .query_row("SELECT count(*) FROM vec_chunks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1);
    }

    #[test]
    fn store_vector_marks_the_chunk_embedded() {
        let vault = temp_vault();
        let conn = db::open(&vault, 3).expect("open");
        conn.execute(
            "INSERT INTO docs(path, status) VALUES ('/a.md', 'pending')",
            [],
        )
        .expect("doc");
        conn.execute(
            "INSERT INTO chunks(doc_id, ord, body, body_seg) VALUES (1, 0, 'x', 'x')",
            [],
        )
        .expect("chunk");
        store_vector(&conn, 1, &[1.0, 0.0, 0.0]).expect("store");
        let embedded: i64 = conn
            .query_row("SELECT embedded FROM chunks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("flag");
        assert_eq!(embedded, 1);
    }

    #[test]
    fn prune_removes_docs_that_left_the_vault() {
        let vault = temp_vault();
        let conn = db::open(&vault, 8).expect("open");
        let gone = plan_doc(&conn, "/vault/gone.md", "内容\n", IndexScope::All)
            .expect("plan")
            .expect("pending");
        rechunk_doc(&conn, &gone).expect("rechunk");
        let kept = plan_doc(&conn, "/vault/kept.md", "内容\n", IndexScope::All)
            .expect("plan")
            .expect("pending");
        rechunk_doc(&conn, &kept).expect("rechunk");

        let mut seen = HashSet::new();
        seen.insert("/vault/kept.md".to_string());
        let removed = prune_missing(&conn, &seen).expect("prune");
        assert_eq!(removed, 1);
        let paths: Vec<String> = {
            let mut stmt = conn.prepare("SELECT path FROM docs").expect("prepare");
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .expect("q");
            rows.collect::<Result<Vec<_>, _>>().expect("collect")
        };
        assert_eq!(paths, vec!["/vault/kept.md"]);
        // 派生数据要跟着走,否则留下查不到正文的幽灵块。
        let chunks: i64 = conn
            .query_row("SELECT count(*) FROM chunks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(chunks, 1);
    }

    #[tokio::test]
    async fn cancelling_before_the_run_reports_cancelled() {
        let vault = temp_vault();
        write_note(&vault, "a.md", "# 标题\n\n正文\n");
        let cancel = CancelToken::new();
        cancel.cancel();
        let recorder = Recorder::default();
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            // 取消要在发请求之前就生效,所以这个地址不会被用到。
            base_url: "http://127.0.0.1:1".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let outcome = index_vault(&vault, 8, &config, IndexScope::All, &cancel, &recorder)
            .await
            .expect("run");
        assert!(outcome.cancelled);
        assert_eq!(outcome.indexed, 0);
        assert!(recorder.phases().contains(&IndexPhase::Cancelled));
    }

    #[tokio::test]
    async fn cancelling_does_not_prune_the_existing_index() {
        // prune 的判据是「不在 seen 里 = 已从 vault 消失」,而取消会让 seen 残缺。
        // 少了这道闸门,用户按一下取消就把整个索引删了 —— 而且悄无声息。
        let vault = temp_vault();
        write_note(&vault, "a.md", "# A\n\n正文\n");
        write_note(&vault, "b.md", "# B\n\n正文\n");
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        // 先建出两篇的记录。
        let cancel = CancelToken::new();
        index_vault(&vault, 8, &config, IndexScope::All, &cancel, &NoProgress)
            .await
            .expect("first run");
        assert_eq!(stats(&vault, 8).expect("stats").docs, 2);

        // 再跑一次,但一开始就取消。
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let outcome = index_vault(&vault, 8, &config, IndexScope::All, &cancelled, &NoProgress)
            .await
            .expect("cancelled run");
        assert!(outcome.cancelled);
        assert_eq!(outcome.removed, 0, "取消的运行不该删任何东西");
        assert_eq!(
            stats(&vault, 8).expect("stats").docs,
            2,
            "取消把已有索引删掉了"
        );
    }

    #[tokio::test]
    async fn a_failing_provider_does_not_abort_the_whole_run() {
        // 一篇笔记连不上 provider,其余笔记仍该被处理完,失败记在库里。
        let vault = temp_vault();
        write_note(&vault, "a.md", "# A\n\n正文一\n");
        write_note(&vault, "b.md", "# B\n\n正文二\n");
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            // 关着的端口:连不上,归类为可重试,但重试完仍然失败。
            base_url: "http://127.0.0.1:1".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let cancel = CancelToken::new();
        let recorder = Recorder::default();
        let outcome = index_vault(&vault, 8, &config, IndexScope::All, &cancel, &recorder)
            .await
            .expect("run should finish despite per-doc failures");
        assert!(!outcome.cancelled);
        assert_eq!(outcome.failed, 2, "两篇都该失败并被记下");
        assert_eq!(outcome.indexed, 0);
        // 失败落盘,重启后仍然知道是哪篇、错在哪。
        let stats = stats(&vault, 8).expect("stats");
        assert_eq!(stats.failed, 2);
        assert_eq!(stats.failures.len(), 2);
        assert!(!stats.failures[0].error.is_empty());
        assert_eq!(stats.failures[0].attempts, 1);
        // 进度里要出现 Done 而不是 Failed —— 单篇失败不是整次运行失败。
        assert!(recorder.phases().contains(&IndexPhase::Done));
        assert!(!recorder.phases().contains(&IndexPhase::Failed));
    }

    #[tokio::test]
    async fn progress_reports_a_total_before_working() {
        // 进度条要能立刻显示总数 —— 第一趟只读盘,不发请求。
        let vault = temp_vault();
        write_note(&vault, "a.md", "# A\n\n正文\n");
        write_note(&vault, "b.md", "# B\n\n正文\n");
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let cancel = CancelToken::new();
        let recorder = Recorder::default();
        index_vault(&vault, 8, &config, IndexScope::All, &cancel, &recorder)
            .await
            .expect("run");
        let events = recorder.events.lock().unwrap().clone();
        assert_eq!(events.first().map(|e| e.phase), Some(IndexPhase::Scanning));
        assert!(
            events.iter().any(|e| e.total == 2),
            "没有报出总数,进度条无从画起"
        );
    }

    #[tokio::test]
    async fn empty_notes_are_marked_indexed_not_retried_forever() {
        // 全是空白的笔记切不出块。不标 indexed 的话每次运行都来看一眼。
        let vault = temp_vault();
        write_note(&vault, "empty.md", "---\ntitle: 空\n---\n\n   \n");
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let cancel = CancelToken::new();
        let outcome = index_vault(&vault, 8, &config, IndexScope::All, &cancel, &NoProgress)
            .await
            .expect("run");
        assert_eq!(outcome.failed, 0, "空笔记不该算失败");
        assert_eq!(outcome.indexed, 1);
        let stats = stats(&vault, 8).expect("stats");
        assert_eq!(stats.indexed, 1);
        assert_eq!(stats.chunks, 0);
    }

    #[tokio::test]
    async fn switching_model_marks_everything_stale() {
        // 同维不同模型的向量不可比,混用会让检索结果看起来合理却是错的。
        let vault = temp_vault();
        write_note(&vault, "a.md", "# A\n\n正文\n");
        let cancel = CancelToken::new();
        let mut config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".into(),
            model: "first-model".into(),
            api_key: String::new(),
        };
        index_vault(&vault, 8, &config, IndexScope::All, &cancel, &NoProgress)
            .await
            .expect("run");
        // 手工把它标成已索引(provider 连不上,真跑不出 indexed)。
        {
            let conn = db::open(&vault, 8).expect("open");
            conn.execute("UPDATE docs SET status = 'indexed'", [])
                .expect("mark");
        }
        config.model = "second-model".into();
        index_vault(&vault, 8, &config, IndexScope::All, &cancel, &NoProgress)
            .await
            .expect("run");
        let conn = db::open(&vault, 8).expect("open");
        let model = db::embed_model(&conn).expect("model");
        assert_eq!(model.as_deref(), Some("second-model"));
        // 换模型后这篇不该还是 indexed —— 它的向量来自旧模型。
        let status: String = conn
            .query_row("SELECT status FROM docs LIMIT 1", [], |row| row.get(0))
            .expect("status");
        assert_ne!(status, "indexed");
    }

    #[tokio::test]
    async fn notes_deleted_from_the_vault_are_pruned() {
        let vault = temp_vault();
        write_note(&vault, "a.md", "# A\n\n正文\n");
        write_note(&vault, "b.md", "# B\n\n正文\n");
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let cancel = CancelToken::new();
        index_vault(&vault, 8, &config, IndexScope::All, &cancel, &NoProgress)
            .await
            .expect("run");
        assert_eq!(stats(&vault, 8).expect("stats").docs, 2);

        std::fs::remove_file(vault.join("b.md")).expect("remove");
        let outcome = index_vault(&vault, 8, &config, IndexScope::All, &cancel, &NoProgress)
            .await
            .expect("run");
        assert_eq!(outcome.removed, 1);
        assert_eq!(stats(&vault, 8).expect("stats").docs, 1);
    }
}
