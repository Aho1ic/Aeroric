//! 混合检索:向量 top-K + FTS5 关键词 top-K,按 Reciprocal Rank Fusion 融合,
//! 再按引用图补几篇关联笔记。
//!
//! ## 为什么是 RRF 而不是加权分数
//!
//! 两路的分数不可比:向量给的是 L2 距离(越小越好、量纲取决于模型),FTS5 给的是
//! bm25(负数、量纲取决于语料词频)。要归一化就得知道两边的分布,而那随库变化。
//! RRF 只用**名次**:第 r 名贡献 `1/(k + r)`,两路都命中的块自然浮到前面。
//!
//! `RRF_K = 60` 沿用原始论文(Cormack et al. 2009)与 Markio 的取值。k 越大,靠前
//! 名次之间的差距越平;60 在「两路都中的排第一」和「一路中得很靠前」之间给出的
//! 折中在实践中够用。
//!
//! ## 与 Markio 的三处差异
//!
//! 1. **名次并列时的顺序是确定的。** Markio 把候选收在 `HashMap` 里再 `sort_by`
//!    (非稳定排序),同分块的相对顺序既取决于哈希迭代顺序、也取决于排序实现 ——
//!    同一个查询两次跑出来的顺序可以不同。这里同分按 `chunk_id` 升序兜底。
//! 2. **图谱扩展占用的是预留额度,不是额外名额。** Markio 把扩展来的块 `push` 到
//!    `ranked` 尾部再 `take(limit)`,所以扩展只在候选不足 limit 时才可见,而 rerank
//!    那一路又会把它挤掉。这里先给扩展留 `GRAPH_SLOTS` 个位置。
//! 3. **降级会上报。** Markio 在 embedding 失败时 `eprintln!` 一行就退化成纯 FTS,
//!    用户看到的是「结果变差了」而不知道为什么。这里把降级原因放进
//!    [`SearchOutcome::degraded`],由 UI 明说「语义检索不可用,当前只有关键词」。

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::cjk;
use super::db;
use super::embed::{self, EmbedConfig};
use super::graph::{self, LinkIndex};
use super::rerank::{self, RerankConfig};

/// RRF 的平滑常数。见模块注释。
const RRF_K: f64 = 60.0;

/// 每一路取 `limit * CANDIDATE_FACTOR` 个候选再融合。
///
/// 只取 limit 个的话,两路各自的第 limit+1 名永远进不了融合 —— 而「一路排第 12、
/// 另一路排第 3」恰恰是 RRF 最该捞出来的那种块。
const CANDIDATE_FACTOR: usize = 3;

/// 同一篇笔记最多贡献几块。
///
/// 不设上限时,一篇长笔记的连续几块会占满整个结果 —— 它们内容高度重叠(块之间还
/// 有 overlap),对「这个问题的答案在哪几篇笔记里」几乎没有信息量。
const DEFAULT_PER_DOC: usize = 3;

/// 给图谱扩展预留的名额。
const GRAPH_SLOTS: usize = 2;

/// 从前几名往外扩展的源笔记数。
const GRAPH_SOURCES: usize = 3;

/// 图谱扩展来的块的占位分数。见 [`expand_by_graph`]。
const GRAPH_SCORE: f64 = -1.0;

/// 高亮片段的上限。一块正文里同一个词出现几十次时,全标出来对 UI 没有帮助。
const MAX_SPANS: usize = 32;

/// 一个字符区间。左闭右开。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// 命中来源。三路可以同时成立。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sources {
    pub vector: bool,
    pub fts: bool,
    pub graph: bool,
    /// 经过了 cross-encoder 精排。与前三个不互斥 —— 它记的是「分数来自哪里」,而
    /// 覆盖掉的原始来源仍然值得让用户看到。
    pub rerank: bool,
}

impl Sources {
    /// 序列化成 `vector+fts` 这样的串给前端。顺序固定,便于测试与展示。
    pub fn as_label(self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.vector {
            parts.push("vector");
        }
        if self.fts {
            parts.push("fts");
        }
        if self.graph {
            parts.push("graph");
        }
        if self.rerank {
            parts.push("rerank");
        }
        parts.join("+")
    }
}

/// 融合后的一个候选。
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub chunk_id: i64,
    pub score: f64,
    pub sources: Sources,
}

/// 一条检索结果。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// 笔记绝对路径,与笔记列表里的 `id` 同一个值。
    pub path: String,
    pub title: String,
    /// 标题路径,形如 `设计 > 存储`。
    pub heading: String,
    /// 块的原文。引用预览显示这一份。
    pub body: String,
    pub score: f64,
    /// `vector+fts` 这样的来源标签。
    pub source: String,
    /// 块在**源文**里的字符区间。跳回原文按它定位。
    pub char_start: usize,
    pub char_end: usize,
    /// 高亮区间,**相对 `body`**。预览里标出来用。
    pub body_spans: Vec<Span>,
    /// 高亮区间,**相对源文**。跳回原文并在编辑器里选中用。
    ///
    /// 与 `body_spans` 不是一一对应:`body` 前面可能带上一块的 overlap 尾巴,落在
    /// 那一段里的命中在源文里属于上一块,这里会被夹到 `char_start`。
    pub source_spans: Vec<Span>,
}

/// 哪一路降级了。
///
/// 用列表而不是一个 `Option<String>`:向量与重排两路互不相干,可以同时挂 —— 本地
/// Ollama 没开着的时候它们通常就是一起挂的。挤在一个字段里会丢掉其中一条。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Degraded {
    /// `vector` 或 `rerank`。UI 据此选文案。
    pub stage: String,
    /// 原始错误信息,给「详情」用。
    pub detail: String,
}

/// 一次检索的全部产出。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOutcome {
    pub hits: Vec<SearchHit>,
    /// 哪几路降级了。非空时 UI 必须说明 —— 静默降级的表现是「结果莫名变差」,
    /// 用户无从判断是索引没建好还是模型没连上。
    pub degraded: Vec<Degraded>,
    /// 索引里是否一个向量都没有(还没建索引 / 刚换模型)。
    ///
    /// 与 `degraded` 分开:这不是故障,是「还没准备好」,UI 该提示的是「建索引」而
    /// 不是「检查模型配置」。
    pub vectors_missing: bool,
}

/// 检索参数。
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    /// 是否按引用图补几篇关联笔记。
    pub expand_links: bool,
    /// 同一篇笔记最多贡献几块。0 表示不限。
    pub per_doc: usize,
    /// 配了就走 cross-encoder 精排。它失败不影响出结果,只会记一条降级。
    pub rerank: Option<RerankConfig>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 8,
            expand_links: true,
            per_doc: DEFAULT_PER_DOC,
            rerank: None,
        }
    }
}

/// 按 RRF 融合两路名次。
///
/// 入参是两路各自**按名次排好**的 chunk id。同一路里重复的 id 只算第一次出现的
/// 名次 —— 否则一个 id 出现两次会拿到两份分数。
pub fn fuse(vector_ranked: &[i64], fts_ranked: &[i64]) -> Vec<Candidate> {
    let mut merged: HashMap<i64, Candidate> = HashMap::new();
    let mut add = |ids: &[i64], mark: fn(&mut Sources)| {
        let mut seen = HashSet::new();
        let mut rank = 0usize;
        for id in ids {
            if !seen.insert(*id) {
                continue;
            }
            let entry = merged.entry(*id).or_insert(Candidate {
                chunk_id: *id,
                score: 0.0,
                sources: Sources::default(),
            });
            entry.score += 1.0 / (RRF_K + rank as f64 + 1.0);
            mark(&mut entry.sources);
            rank += 1;
        }
    };
    add(vector_ranked, |s| s.vector = true);
    add(fts_ranked, |s| s.fts = true);

    let mut ranked: Vec<Candidate> = merged.into_values().collect();
    // 同分按 chunk_id 升序兜底:`HashMap` 的迭代顺序不定,不兜底的话同一个查询
    // 两次跑出来的顺序可以不同。
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.chunk_id.cmp(&b.chunk_id))
    });
    ranked
}

/// 限制每篇笔记的块数,保持原有顺序。`per_doc == 0` 时原样返回。
pub fn cap_per_doc(
    ranked: Vec<Candidate>,
    doc_of: &HashMap<i64, i64>,
    per_doc: usize,
) -> Vec<Candidate> {
    if per_doc == 0 {
        return ranked;
    }
    let mut used: HashMap<i64, usize> = HashMap::new();
    let mut out = Vec::with_capacity(ranked.len());
    for cand in ranked {
        // 查不到所属笔记的块不该被这条规则吃掉 —— 那是数据残缺,不是「同一篇太多」。
        let Some(doc_id) = doc_of.get(&cand.chunk_id) else {
            out.push(cand);
            continue;
        };
        let slot = used.entry(*doc_id).or_insert(0);
        if *slot < per_doc {
            *slot += 1;
            out.push(cand);
        }
    }
    out
}

/// 把查询拆成用于高亮的词。
///
/// 按空白拆,不做 CJK 逐字切分 —— 高亮要的是「用户输入的那一串在正文里的位置」,
/// 逐字切开会把「导出」标成两段,而中间要是隔着别的字就成了误标。
pub fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in query.split_whitespace() {
        let term: String = raw.chars().filter(|c| !c.is_control()).collect();
        if term.is_empty() || terms.contains(&term) {
            continue;
        }
        terms.push(term);
    }
    terms
}

/// 找出 `terms` 在 `text` 里的全部位置(字符区间),大小写不敏感,重叠的合并。
///
/// 按**字符**逐个比而不是先 `to_lowercase()` 整串:有些字符小写化后长度会变
/// (`İ` → `i̇`),那会让区间偏移错位并可能切在字符中间
/// ([[rewrite-verifies-prose-not-candidate]])。
pub fn find_spans(text: &str, terms: &[String]) -> Vec<Span> {
    if terms.is_empty() {
        return Vec::new();
    }
    let haystack: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    for term in terms {
        let needle: Vec<char> = term.chars().collect();
        if needle.is_empty() || needle.len() > haystack.len() {
            continue;
        }
        let mut at = 0usize;
        while at + needle.len() <= haystack.len() {
            let hit = needle
                .iter()
                .zip(&haystack[at..at + needle.len()])
                .all(|(a, b)| chars_eq_ignore_case(*a, *b));
            if hit {
                spans.push(Span {
                    start: at,
                    end: at + needle.len(),
                });
                at += needle.len();
            } else {
                at += 1;
            }
        }
    }
    merge_spans(spans)
}

fn chars_eq_ignore_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// 排序并合并重叠 / 相邻的区间。
fn merge_spans(mut spans: Vec<Span>) -> Vec<Span> {
    if spans.is_empty() {
        return spans;
    }
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start <= last.end => {
                last.end = last.end.max(span.end);
            }
            _ => out.push(span),
        }
        if out.len() >= MAX_SPANS {
            break;
        }
    }
    out
}

/// 把 `body` 内的区间换算到源文坐标。
///
/// `body` 前面可能带着上一块的 overlap 尾巴,而 `char_start..char_end` 只覆盖本块
/// **新增**的正文,所以 `body` 的字符数可以大于区间宽度,差值就是 overlap 长度。
/// 落在 overlap 里的命中在源文里属于上一块:减到 `char_start` 之后区间退化成空的,
/// 于是被丢掉 —— 跳到上一块去比不跳更糟,而放任减法走成负数会绕回巨大的下标。
///
/// 只有 `start < end` 这一道闸门。不必再夹 `char_end`:`span.end <= body_len` 而
/// `body_len - overlap <= width`,所以 `end` 恒不超过 `char_end`,再夹一次是死代码。
pub fn to_source_spans(
    body_spans: &[Span],
    body: &str,
    char_start: usize,
    char_end: usize,
) -> Vec<Span> {
    let body_len = body.chars().count();
    let width = char_end.saturating_sub(char_start);
    let overlap = body_len.saturating_sub(width);
    let mut out = Vec::with_capacity(body_spans.len());
    for span in body_spans {
        let mapped = Span {
            start: char_start + span.start.saturating_sub(overlap),
            end: char_start + span.end.saturating_sub(overlap),
        };
        if mapped.start < mapped.end {
            out.push(mapped);
        }
    }
    out
}

/// 向量 top-K。
fn vector_topk(conn: &Connection, query_vec: &[f32], k: usize) -> Result<Vec<i64>, String> {
    if k == 0 || query_vec.is_empty() {
        return Ok(Vec::new());
    }
    let blob: Vec<u8> = query_vec.iter().flat_map(|f| f.to_le_bytes()).collect();
    let mut stmt = conn
        .prepare(
            "SELECT rowid FROM vec_chunks
             WHERE embedding MATCH ?1 AND k = ?2
             ORDER BY distance",
        )
        .map_err(|e| format!("Cannot prepare notebook vector query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![blob, k as i64], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| format!("Cannot run notebook vector query: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Cannot read notebook vector row: {e}"))?);
    }
    Ok(out)
}

/// FTS top-K。
///
/// `bm25` 的两个权重对应建表时的列序(`body_seg`, `heading_seg`)。标题给 2.0:
/// 命中标题通常比命中正文更贴题,而标题短、bm25 本来就会因为字段长度归一化给它
/// 更高的分,这个权重只是再推一把。
fn fts_topk(conn: &Connection, query: &str, k: usize) -> Result<Vec<i64>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let Some(expression) = cjk::match_expression(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn
        .prepare(
            "SELECT rowid FROM chunks_fts
             WHERE chunks_fts MATCH ?1
             ORDER BY bm25(chunks_fts, 1.0, 2.0)
             LIMIT ?2",
        )
        .map_err(|e| format!("Cannot prepare notebook keyword query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![expression, k as i64], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| format!("Cannot run notebook keyword query: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Cannot read notebook keyword row: {e}"))?);
    }
    Ok(out)
}

/// 查一批块各自属于哪篇笔记。
fn docs_of_chunks(conn: &Connection, ids: &[i64]) -> Result<HashMap<i64, i64>, String> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, doc_id FROM chunks WHERE id IN ({placeholders})"
        ))
        .map_err(|e| format!("Cannot prepare notebook chunk doc query: {e}"))?;
    let params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(params.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| format!("Cannot run notebook chunk doc query: {e}"))?;
    for row in rows {
        let (id, doc_id) = row.map_err(|e| format!("Cannot read notebook chunk doc row: {e}"))?;
        out.insert(id, doc_id);
    }
    Ok(out)
}

/// 一篇笔记的第一块。图谱扩展拿它当代表。
fn first_chunk_of(conn: &Connection, doc_id: i64) -> Result<Option<i64>, String> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT id FROM chunks WHERE doc_id = ?1 ORDER BY ord LIMIT 1",
        rusqlite::params![doc_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| format!("Cannot read notebook doc first chunk: {e}"))
}

/// 按引用图补几篇关联笔记的代表块。
///
/// 只从前 `GRAPH_SOURCES` 名往外走一跳。两跳在小库上会把半个 vault 都拉进来,而
/// 上下文预算是有限的。
fn expand_by_graph(
    conn: &Connection,
    ranked: &[Candidate],
    doc_of: &HashMap<i64, i64>,
    slots: usize,
) -> Result<Vec<Candidate>, String> {
    if slots == 0 || ranked.is_empty() {
        return Ok(Vec::new());
    }
    let docs = graph::load_docs(conn)?;
    if docs.is_empty() {
        return Ok(Vec::new());
    }
    let index = LinkIndex::build(&docs);
    let by_id: HashMap<i64, &graph::DocRef> = docs.iter().map(|d| (d.id, d)).collect();

    let mut seen_chunks: HashSet<i64> = ranked.iter().map(|c| c.chunk_id).collect();
    let mut seen_docs: HashSet<i64> = ranked
        .iter()
        .filter_map(|c| doc_of.get(&c.chunk_id).copied())
        .collect();

    let mut source_docs: Vec<i64> = Vec::new();
    for cand in ranked {
        if let Some(doc_id) = doc_of.get(&cand.chunk_id) {
            if !source_docs.contains(doc_id) {
                source_docs.push(*doc_id);
            }
        }
        if source_docs.len() >= GRAPH_SOURCES {
            break;
        }
    }

    let mut out = Vec::new();
    for source in source_docs {
        let Some(doc) = by_id.get(&source) else {
            continue;
        };
        // 多要一些:邻居里可能有已经命中的笔记,按 `slots` 取正好会因为去重而填不满。
        for neighbour in graph::neighbors(conn, &index, doc, slots + seen_docs.len())? {
            if !seen_docs.insert(neighbour) {
                continue;
            }
            if let Some(chunk_id) = first_chunk_of(conn, neighbour)? {
                if seen_chunks.insert(chunk_id) {
                    out.push(Candidate {
                        chunk_id,
                        // 扩展来的块没有参与融合,分数只是个排序占位。取负数而不是
                        // 0:RRF 分数恒为正,负数保证「数组顺序 == 分数顺序」,前端
                        // 按分数重排也不会把关联笔记混进真正的命中里。
                        score: GRAPH_SCORE,
                        sources: Sources {
                            graph: true,
                            ..Sources::default()
                        },
                    });
                    if out.len() >= slots {
                        return Ok(out);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// 把候选补齐成可展示的结果。
fn materialize(
    conn: &Connection,
    cands: &[Candidate],
    terms: &[String],
) -> Result<Vec<SearchHit>, String> {
    if cands.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = cands.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT c.id, c.heading, c.body, c.char_start, c.char_end, d.path, d.title
             FROM chunks c JOIN docs d ON d.id = c.doc_id
             WHERE c.id IN ({placeholders})"
        ))
        .map_err(|e| format!("Cannot prepare notebook hit query: {e}"))?;
    let ids: Vec<i64> = cands.iter().map(|c| c.chunk_id).collect();
    let params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(params.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| format!("Cannot run notebook hit query: {e}"))?;

    let mut fetched: HashMap<i64, SearchHit> = HashMap::new();
    for row in rows {
        let (id, heading, body, char_start, char_end, path, title) =
            row.map_err(|e| format!("Cannot read notebook hit row: {e}"))?;
        let char_start = char_start.max(0) as usize;
        let char_end = char_end.max(0) as usize;
        let body_spans = find_spans(&body, terms);
        let source_spans = to_source_spans(&body_spans, &body, char_start, char_end);
        fetched.insert(
            id,
            SearchHit {
                path,
                title,
                heading,
                body,
                score: 0.0,
                source: String::new(),
                char_start,
                char_end,
                body_spans,
                source_spans,
            },
        );
    }

    // 按候选的顺序输出 —— `IN (...)` 的返回顺序由 SQLite 决定,不是名次。
    let mut hits = Vec::with_capacity(cands.len());
    for cand in cands {
        if let Some(mut hit) = fetched.remove(&cand.chunk_id) {
            hit.score = cand.score;
            hit.source = cand.sources.as_label();
            hits.push(hit);
        }
    }
    Ok(hits)
}

/// 取一批块用于重排的文本。与入库时喂给 embedding 的拼法一致。
fn rerank_inputs(conn: &Connection, ids: &[i64]) -> Result<HashMap<i64, String>, String> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, heading, body FROM chunks WHERE id IN ({placeholders})"
        ))
        .map_err(|e| format!("Cannot prepare notebook rerank input query: {e}"))?;
    let params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(params.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("Cannot run notebook rerank input query: {e}"))?;
    for row in rows {
        let (id, heading, body) =
            row.map_err(|e| format!("Cannot read notebook rerank input row: {e}"))?;
        let text = if heading.trim().is_empty() {
            body
        } else {
            format!("{heading}\n\n{body}")
        };
        out.insert(id, text);
    }
    Ok(out)
}

/// 用 cross-encoder 重排候选池,原地改写 `ranked` 的顺序。
///
/// 返回降级原因(如果有)。重排是可选增强:它失败就沿用融合名次,不该让整次检索
/// 失败。没被服务端提到的候选按原顺序缀在后面 —— 那是「没进 top_n」,不是「不相关」,
/// 丢掉它们会让结果凭空变少。
async fn rerank_pool(
    conn: &Connection,
    ranked: &mut Vec<Candidate>,
    query: &str,
    config: &RerankConfig,
    pool_size: usize,
) -> Result<Option<String>, String> {
    let pool_size = pool_size.min(ranked.len()).min(rerank::MAX_DOCUMENTS);
    if pool_size < 2 {
        return Ok(None);
    }
    let ids: Vec<i64> = ranked[..pool_size].iter().map(|c| c.chunk_id).collect();
    let texts = rerank_inputs(conn, &ids)?;
    // 每个候选都要有文本,否则下标会对不上服务端返回的 index。查不到的(块刚被删)
    // 直接放弃重排:剩下的池子已经不是我们送出去的那一批了。
    let mut documents = Vec::with_capacity(ids.len());
    for id in &ids {
        match texts.get(id) {
            Some(text) => documents.push(text.clone()),
            None => return Ok(None),
        }
    }

    match rerank::rerank(config, query, &documents, pool_size).await {
        Ok(order) if !order.is_empty() => {
            let mut reordered: Vec<Candidate> = Vec::with_capacity(ranked.len());
            let mut taken = vec![false; pool_size];
            for entry in &order {
                taken[entry.index] = true;
                let mut cand = ranked[entry.index].clone();
                // 覆盖 RRF 分数:重排的相关度是另一个量纲,混在一起没有意义。数组
                // 顺序与分数顺序保持一致,前端按分数重排也不会打乱。
                cand.score = entry.score as f64;
                cand.sources.rerank = true;
                reordered.push(cand);
            }
            for (index, keep) in taken.iter().enumerate() {
                if !keep {
                    reordered.push(ranked[index].clone());
                }
            }
            reordered.extend_from_slice(&ranked[pool_size..]);
            *ranked = reordered;
            Ok(None)
        }
        // 服务端认为一条都不相关时沿用融合名次。那不是故障,不记降级。
        Ok(_) => Ok(None),
        Err(err) => Ok(Some(err.message().to_string())),
    }
}

/// 混合检索。
///
/// 向量那一路失败时降级为纯 FTS 并在 [`SearchOutcome::degraded`] 里说明原因 ——
/// 本地 Ollama 没开着是最常见的情况,而纯关键词检索仍然是有用的。
pub async fn search(
    vault: &std::path::Path,
    dim: usize,
    config: &EmbedConfig,
    query: &str,
    options: &SearchOptions,
) -> Result<SearchOutcome, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() || options.limit == 0 {
        return Ok(SearchOutcome {
            hits: Vec::new(),
            degraded: Vec::new(),
            vectors_missing: false,
        });
    }
    let Some(conn) = db::open_existing(vault, dim)? else {
        return Ok(SearchOutcome {
            hits: Vec::new(),
            degraded: Vec::new(),
            vectors_missing: true,
        });
    };

    let vector_count: i64 = conn
        .query_row("SELECT count(*) FROM vec_chunks", [], |row| row.get(0))
        .map_err(|e| format!("Cannot count notebook vectors: {e}"))?;

    // 图谱扩展占预留额度,主检索因此少取 GRAPH_SLOTS 个 —— 但不能把主检索压到 0,
    // limit 很小(比如 1)时扩展让位。
    let graph_slots = if options.expand_links {
        GRAPH_SLOTS.min(options.limit.saturating_sub(1))
    } else {
        0
    };
    let primary_limit = options.limit - graph_slots;
    let candidates = primary_limit.saturating_mul(CANDIDATE_FACTOR).max(1);

    let mut degraded: Vec<Degraded> = Vec::new();
    let vector_ranked = if vector_count == 0 {
        Vec::new()
    } else {
        match embed::embed_batch(config, &[trimmed.to_string()], &|| false).await {
            Ok(vectors) => match vectors.first() {
                // 维度对不上时**必须**降级而不是往下走:vec0 会拒收长度不对的向量,
                // 那是一个从 `search` 里返回的硬错误,而用户看到的是「搜索坏了」。
                // 走到这里的典型情形是用户换了 embedding 模型还没重建索引 ——
                // 那种时候纯关键词检索仍然有用,而提示该说的是「去重建索引」。
                Some(vector) if vector.len() != dim => {
                    degraded.push(Degraded {
                        stage: "vector".to_string(),
                        detail: format!(
                            "Embedding dimension changed ({} now, {dim} in the index); rebuild the index to search by vector",
                            vector.len()
                        ),
                    });
                    Vec::new()
                }
                Some(vector) => vector_topk(&conn, vector, candidates)?,
                None => {
                    degraded.push(Degraded {
                        stage: "vector".to_string(),
                        detail: "Embedding provider returned no vector".to_string(),
                    });
                    Vec::new()
                }
            },
            Err(err) => {
                degraded.push(Degraded {
                    stage: "vector".to_string(),
                    detail: err.message().to_string(),
                });
                Vec::new()
            }
        }
    };

    let fts_ranked = fts_topk(&conn, trimmed, candidates)?;

    let ranked = fuse(&vector_ranked, &fts_ranked);
    let all_ids: Vec<i64> = ranked.iter().map(|c| c.chunk_id).collect();
    let doc_of = docs_of_chunks(&conn, &all_ids)?;
    let mut ranked = cap_per_doc(ranked, &doc_of, options.per_doc);

    // 重排在截断**之前**做 —— 它的用途正是「在前 20 名里挑前 5 名」,截完再排就没有
    // 可挑的了。图谱扩展来的块不进池子:它们不是命中,让 cross-encoder 给它们打分
    // 会把关联笔记排到真正的命中前面。
    if let Some(rerank_config) = options.rerank.as_ref() {
        let pool = primary_limit.saturating_mul(CANDIDATE_FACTOR);
        if let Some(detail) = rerank_pool(&conn, &mut ranked, trimmed, rerank_config, pool).await? {
            degraded.push(Degraded {
                stage: "rerank".to_string(),
                detail,
            });
        }
    }

    // 主检索先切到预留之后的长度,扩展从这一段往外走 —— 从会被丢掉的尾巴往外扩展
    // 没有意义,那些笔记本来就不出现在结果里。
    let tail = ranked.split_off(primary_limit.min(ranked.len()));
    let extra = if graph_slots > 0 {
        expand_by_graph(&conn, &ranked, &doc_of, graph_slots)?
    } else {
        Vec::new()
    };

    // 扩展没填满预留额度时,把余下的名额还给主检索 —— 一个没有任何链接的 vault
    // 不该因为「预留了两个位置」就少给两条结果。
    let unused = graph_slots.saturating_sub(extra.len());
    if unused > 0 {
        let claimed: HashSet<i64> = extra.iter().map(|c| c.chunk_id).collect();
        for cand in tail.into_iter().filter(|c| !claimed.contains(&c.chunk_id)) {
            ranked.push(cand);
            if ranked.len() >= options.limit - extra.len() {
                break;
            }
        }
    }
    ranked.extend(extra);

    let terms = query_terms(trimmed);
    let hits = materialize(&conn, &ranked, &terms)?;
    Ok(SearchOutcome {
        hits,
        degraded,
        vectors_missing: vector_count == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let dir = std::env::temp_dir().join(format!("aeroric-rag-search-{unique}"));
        std::fs::create_dir_all(&dir).expect("create temp vault");
        dir
    }

    /// 一篇笔记的一块。`body` 会自动生成切分版进 FTS。
    struct Row<'a> {
        doc_id: i64,
        path: &'a str,
        title: &'a str,
        heading: &'a str,
        body: &'a str,
    }

    /// 建一个装好 docs / chunks / links 的库。chunk id 从 1 起按 `rows` 的顺序分配。
    fn seeded(rows: &[Row<'_>], links: &[(i64, &str)]) -> (std::path::PathBuf, Connection) {
        let vault = temp_vault();
        let conn = db::open(&vault, 4).expect("open db");
        let mut docs_done: HashSet<i64> = HashSet::new();
        let mut ord: HashMap<i64, i64> = HashMap::new();
        for row in rows {
            if docs_done.insert(row.doc_id) {
                conn.execute(
                    "INSERT INTO docs(id, path, title, status) VALUES (?1, ?2, ?3, 'indexed')",
                    rusqlite::params![row.doc_id, row.path, row.title],
                )
                .expect("insert doc");
            }
            let slot = ord.entry(row.doc_id).or_insert(0);
            let start = *slot * 100;
            conn.execute(
                "INSERT INTO chunks(doc_id, ord, heading, body, heading_seg, body_seg,
                                    char_start, char_end, embedded)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
                rusqlite::params![
                    row.doc_id,
                    *slot,
                    row.heading,
                    row.body,
                    cjk::segment(row.heading),
                    cjk::segment(row.body),
                    start,
                    start + row.body.chars().count() as i64,
                ],
            )
            .expect("insert chunk");
            *slot += 1;
        }
        for (doc_id, target) in links {
            conn.execute(
                "INSERT INTO links(doc_id, target, target_norm, kind)
                 VALUES (?1, ?2, ?3, 'wikilink')",
                rusqlite::params![doc_id, target, graph::normalize_target(target)],
            )
            .expect("insert link");
        }
        (vault, conn)
    }

    fn ids(cands: &[Candidate]) -> Vec<i64> {
        cands.iter().map(|c| c.chunk_id).collect()
    }

    #[test]
    fn fusion_puts_chunks_found_by_both_arms_first() {
        // 3 在两路都中(向量第 2、关键词第 2),单路第一名都比不过它。
        let ranked = fuse(&[1, 3], &[2, 3]);
        assert_eq!(ranked[0].chunk_id, 3);
        assert!(ranked[0].sources.vector && ranked[0].sources.fts);
    }

    #[test]
    fn fusion_score_matches_the_rrf_formula() {
        let ranked = fuse(&[7], &[]);
        assert!((ranked[0].score - 1.0 / 61.0).abs() < 1e-12);
        let both = fuse(&[7], &[7]);
        assert!((both[0].score - 2.0 / 61.0).abs() < 1e-12);
    }

    #[test]
    fn fusion_ranks_by_position_not_by_arm() {
        // 关键词的第一名要排在向量的第三名前面 —— 名次跨路可比,这正是 RRF 的用途。
        // 两路的第一名(8 与 5)同分,它们之间的先后由 chunk_id 兜底,不是这里要测的。
        let ranked = fuse(&[8, 9, 10], &[5]);
        assert_eq!(*ids(&ranked).last().expect("last"), 10);
        let position = |id: i64| ids(&ranked).iter().position(|x| *x == id).expect("present");
        assert!(position(5) < position(9), "关键词第一名要压过向量第二名");
    }

    #[test]
    fn fusion_breaks_ties_deterministically() {
        // 两路各自的第一名同分。不兜底的话顺序取决于 HashMap 迭代顺序。
        for _ in 0..8 {
            assert_eq!(ids(&fuse(&[42], &[7])), vec![7, 42]);
        }
    }

    #[test]
    fn fusion_counts_a_repeated_id_once_per_arm() {
        // 同一路里重复出现不该拿两份分数。
        let once = fuse(&[5], &[]);
        let twice = fuse(&[5, 5], &[]);
        assert_eq!(twice.len(), 1);
        assert!((twice[0].score - once[0].score).abs() < 1e-12);
    }

    #[test]
    fn fusion_of_nothing_is_nothing() {
        assert!(fuse(&[], &[]).is_empty());
    }

    #[test]
    fn source_label_lists_every_arm() {
        let ranked = fuse(&[1], &[1]);
        assert_eq!(ranked[0].sources.as_label(), "vector+fts");
        assert_eq!(Sources::default().as_label(), "");
    }

    #[test]
    fn per_doc_cap_keeps_the_best_chunks_of_each_note() {
        let ranked = fuse(&[1, 2, 3, 4], &[]);
        let doc_of: HashMap<i64, i64> = [(1, 10), (2, 10), (3, 10), (4, 20)].into_iter().collect();
        assert_eq!(ids(&cap_per_doc(ranked, &doc_of, 2)), vec![1, 2, 4]);
    }

    #[test]
    fn per_doc_cap_of_zero_is_unlimited() {
        let ranked = fuse(&[1, 2, 3], &[]);
        let doc_of: HashMap<i64, i64> = [(1, 10), (2, 10), (3, 10)].into_iter().collect();
        assert_eq!(ids(&cap_per_doc(ranked, &doc_of, 0)), vec![1, 2, 3]);
    }

    #[test]
    fn per_doc_cap_keeps_chunks_with_no_known_doc() {
        // 查不到所属笔记是数据残缺,不是「同一篇太多」,不该被这条规则吃掉。
        let ranked = fuse(&[1, 2], &[]);
        let doc_of: HashMap<i64, i64> = [(1, 10)].into_iter().collect();
        assert_eq!(ids(&cap_per_doc(ranked, &doc_of, 1)), vec![1, 2]);
    }

    #[test]
    fn query_terms_dedupe_and_drop_blanks() {
        assert_eq!(query_terms("导出  PDF 导出"), vec!["导出", "PDF"]);
        assert!(query_terms("   ").is_empty());
    }

    #[test]
    fn spans_find_every_occurrence_case_insensitively() {
        let spans = find_spans("Export and export", &["export".to_string()]);
        assert_eq!(
            spans,
            vec![Span { start: 0, end: 6 }, Span { start: 11, end: 17 }]
        );
    }

    #[test]
    fn spans_are_character_indexed_not_byte_indexed() {
        // 「随手记」占 9 个字节、3 个字符。按字节算会指到字符中间。
        let spans = find_spans("随手记的导出功能", &["导出".to_string()]);
        assert_eq!(spans, vec![Span { start: 4, end: 6 }]);
    }

    #[test]
    fn spans_merge_when_they_overlap() {
        let spans = find_spans("abcd", &["abc".to_string(), "bcd".to_string()]);
        assert_eq!(spans, vec![Span { start: 0, end: 4 }]);
    }

    #[test]
    fn spans_ignore_terms_longer_than_the_text() {
        assert!(find_spans("ab", &["abcdef".to_string()]).is_empty());
        assert!(find_spans("ab", &[]).is_empty());
    }

    #[test]
    fn source_spans_shift_past_the_overlap() {
        // body 比区间宽 3 个字符,那 3 个字符是上一块的尾巴。
        let body = "xyz导出";
        let spans = find_spans(body, &["导出".to_string()]);
        assert_eq!(spans, vec![Span { start: 3, end: 5 }]);
        assert_eq!(
            to_source_spans(&spans, body, 100, 102),
            vec![Span {
                start: 100,
                end: 102
            }]
        );
    }

    #[test]
    fn source_spans_drop_hits_entirely_inside_the_overlap() {
        // body 比区间宽 2 个字符,「导出」整个落在那 2 个字符里 —— 在源文里它属于
        // 上一块,跳到上一块去比不跳更糟,所以丢掉。
        let body = "导出abc";
        let spans = find_spans(body, &["导出".to_string()]);
        assert_eq!(spans, vec![Span { start: 0, end: 2 }], "body 里确实命中了");
        assert!(
            to_source_spans(&spans, body, 100, 103).is_empty(),
            "落在 overlap 里的命中不该映射出区间"
        );
    }

    #[test]
    fn source_spans_keep_the_part_that_straddles_the_overlap() {
        // 命中跨在 overlap 边界上:前半属于上一块,后半是本块的开头。留下后半。
        let body = "xy导出z";
        let spans = find_spans(body, &["y导".to_string()]);
        assert_eq!(spans, vec![Span { start: 1, end: 3 }]);
        assert_eq!(
            to_source_spans(&spans, body, 100, 103),
            vec![Span {
                start: 100,
                end: 101
            }]
        );
    }

    #[test]
    fn source_spans_stay_inside_the_chunk() {
        let body = "导出";
        let spans = find_spans(body, &["导出".to_string()]);
        let mapped = to_source_spans(&spans, body, 0, 2);
        assert_eq!(mapped, vec![Span { start: 0, end: 2 }]);
    }

    #[test]
    fn keyword_search_finds_chinese() {
        // 这是 unicode61 直接建索引时恒零命中的那一类查询。
        let (_vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "随手记的导出功能已经完成",
            }],
            &[],
        );
        assert_eq!(fts_topk(&conn, "导出", 8).expect("fts"), vec![1]);
        assert_eq!(fts_topk(&conn, "完成", 8).expect("fts"), vec![1]);
    }

    #[test]
    fn keyword_search_requires_adjacency() {
        // 「导能」两个字都在正文里,但不相邻 —— 短语查询不该命中。退化成 OR 的话
        // 会把所有含「导」或「能」的笔记都捞回来。
        let (_vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "导出功能",
            }],
            &[],
        );
        assert!(fts_topk(&conn, "导能", 8).expect("fts").is_empty());
    }

    #[test]
    fn keyword_search_matches_the_heading() {
        let (_vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "存储设计",
                body: "无关正文",
            }],
            &[],
        );
        assert_eq!(fts_topk(&conn, "存储", 8).expect("fts"), vec![1]);
    }

    #[test]
    fn a_heading_hit_outranks_an_equivalent_body_hit() {
        // bm25 的第二个权重(heading_seg)给 2.0 才有这个顺序。两块的字段长度刻意
        // 做成一样,免得 bm25 自带的长度归一化替权重把结果做对。
        let (_vault, conn) = seeded(
            &[
                Row {
                    doc_id: 1,
                    path: "/v/body.md",
                    title: "正文命中",
                    heading: "无关标题",
                    body: "存储设计",
                },
                Row {
                    doc_id: 2,
                    path: "/v/heading.md",
                    title: "标题命中",
                    heading: "存储设计",
                    body: "无关正文",
                },
            ],
            &[],
        );
        assert_eq!(
            fts_topk(&conn, "存储", 8).expect("fts"),
            vec![2, 1],
            "命中标题的那块要排在前面"
        );
    }

    #[test]
    fn keyword_search_of_punctuation_only_is_empty() {
        let (_vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "正文",
            }],
            &[],
        );
        assert!(fts_topk(&conn, "!!!", 8).expect("fts").is_empty());
        assert!(fts_topk(&conn, "导出", 0).expect("fts").is_empty());
    }

    #[test]
    fn materialize_returns_hits_in_candidate_order() {
        let (_vault, conn) = seeded(
            &[
                Row {
                    doc_id: 1,
                    path: "/v/a.md",
                    title: "甲",
                    heading: "一",
                    body: "导出功能",
                },
                Row {
                    doc_id: 2,
                    path: "/v/b.md",
                    title: "乙",
                    heading: "二",
                    body: "别的正文",
                },
            ],
            &[],
        );
        // 候选顺序 2、1;`IN (...)` 的返回顺序由 SQLite 定,不是名次。
        let cands = fuse(&[2, 1], &[]);
        let hits = materialize(&conn, &cands, &["导出".to_string()]).expect("materialize");
        assert_eq!(
            hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
            vec!["/v/b.md", "/v/a.md"]
        );
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn materialize_fills_highlight_spans() {
        let (_vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "随手记的导出功能",
            }],
            &[],
        );
        let hits =
            materialize(&conn, &fuse(&[1], &[]), &["导出".to_string()]).expect("materialize");
        assert_eq!(hits[0].body_spans, vec![Span { start: 4, end: 6 }]);
        // char_start 是 0,body 与区间同宽,所以源文坐标就是 body 坐标。
        assert_eq!(hits[0].source_spans, vec![Span { start: 4, end: 6 }]);
        assert_eq!(hits[0].source, "vector");
    }

    #[test]
    fn materialize_skips_chunks_that_vanished() {
        // 幽灵命中:向量还在、块已经被删。有分数、有 id,查正文却是空的。
        let (_vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "正文",
            }],
            &[],
        );
        let hits = materialize(&conn, &fuse(&[1, 999], &[]), &[]).expect("materialize");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn graph_expansion_adds_a_linked_note() {
        let (_vault, conn) = seeded(
            &[
                Row {
                    doc_id: 1,
                    path: "/v/a.md",
                    title: "甲",
                    heading: "",
                    body: "导出功能",
                },
                Row {
                    doc_id: 2,
                    path: "/v/b.md",
                    title: "乙",
                    heading: "",
                    body: "关联笔记",
                },
            ],
            &[(1, "b")],
        );
        let ranked = fuse(&[1], &[]);
        let doc_of = docs_of_chunks(&conn, &[1]).expect("doc map");
        let extra = expand_by_graph(&conn, &ranked, &doc_of, 2).expect("expand");
        assert_eq!(ids(&extra), vec![2]);
        assert!(extra[0].sources.graph);
    }

    #[test]
    fn graph_expansion_skips_notes_already_ranked() {
        let (_vault, conn) = seeded(
            &[
                Row {
                    doc_id: 1,
                    path: "/v/a.md",
                    title: "甲",
                    heading: "",
                    body: "导出功能",
                },
                Row {
                    doc_id: 2,
                    path: "/v/b.md",
                    title: "乙",
                    heading: "",
                    body: "也命中导出",
                },
            ],
            &[(1, "b")],
        );
        let ranked = fuse(&[1, 2], &[]);
        let doc_of = docs_of_chunks(&conn, &[1, 2]).expect("doc map");
        assert!(expand_by_graph(&conn, &ranked, &doc_of, 2)
            .expect("expand")
            .is_empty());
    }

    #[test]
    fn graph_expansion_respects_its_slots() {
        let rows: Vec<Row<'_>> = vec![
            Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "导出",
            },
            Row {
                doc_id: 2,
                path: "/v/b.md",
                title: "乙",
                heading: "",
                body: "二",
            },
            Row {
                doc_id: 3,
                path: "/v/c.md",
                title: "丙",
                heading: "",
                body: "三",
            },
            Row {
                doc_id: 4,
                path: "/v/d.md",
                title: "丁",
                heading: "",
                body: "四",
            },
        ];
        let (_vault, conn) = seeded(&rows, &[(1, "b"), (1, "c"), (1, "d")]);
        let ranked = fuse(&[1], &[]);
        let doc_of = docs_of_chunks(&conn, &[1]).expect("doc map");
        assert_eq!(
            expand_by_graph(&conn, &ranked, &doc_of, 1)
                .expect("expand")
                .len(),
            1
        );
        assert!(expand_by_graph(&conn, &ranked, &doc_of, 0)
            .expect("expand")
            .is_empty());
    }

    #[test]
    fn graph_expansion_scores_below_every_real_hit() {
        // 扩展来的块必须排在真正的命中后面。最小的 RRF 贡献是 1/(60+名次)。
        let (_vault, conn) = seeded(
            &[
                Row {
                    doc_id: 1,
                    path: "/v/a.md",
                    title: "甲",
                    heading: "",
                    body: "导出",
                },
                Row {
                    doc_id: 2,
                    path: "/v/b.md",
                    title: "乙",
                    heading: "",
                    body: "关联",
                },
            ],
            &[(1, "b")],
        );
        let ranked = fuse(&[1], &[]);
        let doc_of = docs_of_chunks(&conn, &[1]).expect("doc map");
        let extra = expand_by_graph(&conn, &ranked, &doc_of, 2).expect("expand");
        assert!(extra[0].score < ranked.last().expect("last").score);
    }

    #[tokio::test]
    async fn empty_query_returns_nothing() {
        let vault = temp_vault();
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(&vault, 4, &config, "   ", &SearchOptions::default())
            .await
            .expect("search");
        assert!(outcome.hits.is_empty());
        assert!(outcome.degraded.is_empty());
    }

    #[tokio::test]
    async fn searching_without_an_index_reports_missing_vectors() {
        // 没建过索引时不该报错 —— UI 要能据此提示「先建索引」。
        let vault = temp_vault();
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(&vault, 4, &config, "导出", &SearchOptions::default())
            .await
            .expect("search");
        assert!(outcome.hits.is_empty());
        assert!(outcome.vectors_missing);
    }

    #[tokio::test]
    async fn a_dead_provider_degrades_to_keyword_search() {
        // 本地 Ollama 没开着是最常见的情况。纯关键词检索仍然有用,但降级必须上报 ——
        // 静默降级的表现是「结果莫名变差」。
        let (vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "随手记的导出功能",
            }],
            &[],
        );
        // 塞一个向量,好让检索走到 embedding 那一步(而不是因为库里没向量就跳过)。
        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, ?1)",
            rusqlite::params![[0.5f32, 0.5, 0.5, 0.5]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>()],
        )
        .expect("insert vector");
        drop(conn);

        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            // 127.0.0.1:1 上不会有人监听。
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(&vault, 4, &config, "导出", &SearchOptions::default())
            .await
            .expect("search");
        assert_eq!(
            outcome
                .degraded
                .iter()
                .map(|d| d.stage.as_str())
                .collect::<Vec<_>>(),
            vec!["vector"],
            "降级原因必须上报"
        );
        assert!(!outcome.vectors_missing);
        assert_eq!(outcome.hits.len(), 1, "关键词那一路仍然要出结果");
        assert_eq!(outcome.hits[0].path, "/v/a.md");
    }

    #[tokio::test]
    async fn a_changed_embedding_dimension_degrades_instead_of_failing() {
        // 用户换了 embedding 模型还没重建索引。vec0 会拒收长度不对的向量,不拦的话
        // 那是一个从 `search` 里抛出来的错 —— 用户看到的是「搜索坏了」,而实际上
        // 关键词那一路完全正常,该提示的是「去重建索引」。
        let (vault, conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "随手记的导出功能",
            }],
            &[],
        );
        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (1, ?1)",
            rusqlite::params![[0.5f32, 0.5, 0.5, 0.5]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>()],
        )
        .expect("insert vector");
        drop(conn);

        // 库是 4 维,provider 回 3 维。
        let base_url = one_shot_server(r#"{"embeddings":[[1.0,0.0,0.0]]}"#).await;
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url,
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(&vault, 4, &config, "导出", &SearchOptions::default())
            .await
            .expect("维度不符必须降级,而不是让整次检索失败");
        assert_eq!(
            outcome
                .degraded
                .iter()
                .map(|d| d.stage.as_str())
                .collect::<Vec<_>>(),
            vec!["vector"]
        );
        assert!(
            outcome.degraded[0].detail.contains('3') && outcome.degraded[0].detail.contains('4'),
            "两个维度都要写出来,否则用户不知道该按哪个重建:{}",
            outcome.degraded[0].detail
        );
        assert_eq!(outcome.hits.len(), 1, "关键词那一路仍然要出结果");
        assert_eq!(outcome.hits[0].source, "fts");
    }

    #[tokio::test]
    async fn keyword_only_search_honours_the_limit() {
        let rows: Vec<Row<'_>> = (1..=5)
            .map(|i| Row {
                doc_id: i,
                path: match i {
                    1 => "/v/1.md",
                    2 => "/v/2.md",
                    3 => "/v/3.md",
                    4 => "/v/4.md",
                    _ => "/v/5.md",
                },
                title: "笔记",
                heading: "",
                body: "导出功能",
            })
            .collect();
        let (vault, _conn) = seeded(&rows, &[]);
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(
            &vault,
            4,
            &config,
            "导出",
            &SearchOptions {
                limit: 2,
                expand_links: false,
                per_doc: DEFAULT_PER_DOC,
                rerank: None,
            },
        )
        .await
        .expect("search");
        assert_eq!(outcome.hits.len(), 2);
        // 库里没有向量,不该报降级 —— 那不是「provider 挂了」。
        assert!(outcome.degraded.is_empty());
        assert!(outcome.vectors_missing);
    }

    #[tokio::test]
    async fn expansion_never_pushes_out_a_real_hit() {
        // limit 是硬上限:扩展占的是预留额度,不是额外名额。
        //
        // 关联笔记要给足三篇 —— 只给一篇的话扩展本来就填不满预留额度,「额度是预留
        // 的还是额外的」这两种实现跑出来的条数一样,断言就成了空的。
        let rows: Vec<Row<'_>> = vec![
            Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "导出功能",
            },
            Row {
                doc_id: 2,
                path: "/v/b.md",
                title: "乙",
                heading: "",
                body: "关联笔记",
            },
            Row {
                doc_id: 3,
                path: "/v/c.md",
                title: "丙",
                heading: "",
                body: "另一篇",
            },
            Row {
                doc_id: 4,
                path: "/v/d.md",
                title: "丁",
                heading: "",
                body: "还有一篇",
            },
        ];
        let (vault, _conn) = seeded(&rows, &[(1, "b"), (1, "c"), (1, "d")]);
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(
            &vault,
            4,
            &config,
            "导出",
            &SearchOptions {
                limit: 3,
                expand_links: true,
                per_doc: DEFAULT_PER_DOC,
                rerank: None,
            },
        )
        .await
        .expect("search");
        assert_eq!(outcome.hits.len(), 3, "limit 是硬上限");
        // 真正的命中在前,扩展来的在后。
        assert_eq!(outcome.hits[0].path, "/v/a.md");
        assert_eq!(
            outcome.hits.iter().filter(|h| h.source == "graph").count(),
            GRAPH_SLOTS,
            "预留给扩展的额度应当填满"
        );
    }

    /// 起一个只应答一次的 HTTP 服务,返回它的 base_url。
    ///
    /// 自己写而不是加 mock 依赖:整个测试要的就是「按固定 JSON 应答一次」,而
    /// `tokio` 的 `net` + `io-util` 已经在依赖里了。
    async fn one_shot_server(body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            // 把请求读完再应答:只读头就关会让客户端在写 body 时撞上 broken pipe,
            // 那时报的错是「连接断了」而不是「响应不对」,排查方向全歪。
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let Ok(read) = stream.read(&mut chunk).await else {
                    break;
                };
                if read == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&buf);
                let Some(head_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let expected: usize = text[..head_end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().ok())?
                    })
                    .unwrap_or(0);
                if buf.len() >= head_end + 4 + expected {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        });
        format!("http://{addr}")
    }

    fn dead_rerank() -> RerankConfig {
        RerankConfig {
            provider: "cohere".to_string(),
            model: "rerank-v3".to_string(),
            // 没有人在这个端口上监听。
            base_url: "http://127.0.0.1:1".to_string(),
            api_key: String::new(),
        }
    }

    #[tokio::test]
    async fn a_dead_reranker_still_returns_the_fused_order() {
        // 重排是可选增强,它挂掉不该让整次检索失败,也不该少给结果。
        let rows: Vec<Row<'_>> = (1..=4)
            .map(|i| Row {
                doc_id: i,
                path: match i {
                    1 => "/v/1.md",
                    2 => "/v/2.md",
                    3 => "/v/3.md",
                    _ => "/v/4.md",
                },
                title: "笔记",
                heading: "",
                body: "导出功能",
            })
            .collect();
        let (vault, _conn) = seeded(&rows, &[]);
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(
            &vault,
            4,
            &config,
            "导出",
            &SearchOptions {
                limit: 4,
                expand_links: false,
                per_doc: DEFAULT_PER_DOC,
                rerank: Some(dead_rerank()),
            },
        )
        .await
        .expect("search");
        assert_eq!(outcome.hits.len(), 4, "重排失败不该少给结果");
        assert_eq!(
            outcome
                .degraded
                .iter()
                .map(|d| d.stage.as_str())
                .collect::<Vec<_>>(),
            vec!["rerank"],
            "重排降级要单独上报"
        );
        assert!(
            outcome.hits.iter().all(|h| h.source == "fts"),
            "没排上就不该标 rerank"
        );
    }

    #[tokio::test]
    async fn a_pool_too_small_to_reorder_skips_the_reranker() {
        // 只有一个候选时没有可排的。仍然发一次请求就是纯浪费,而且会误报降级。
        let (vault, _conn) = seeded(
            &[Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "导出功能",
            }],
            &[],
        );
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(
            &vault,
            4,
            &config,
            "导出",
            &SearchOptions {
                limit: 4,
                expand_links: false,
                per_doc: DEFAULT_PER_DOC,
                rerank: Some(dead_rerank()),
            },
        )
        .await
        .expect("search");
        assert_eq!(outcome.hits.len(), 1);
        assert!(outcome.degraded.is_empty(), "没发请求就不该报降级");
    }

    /// 四篇都命中同一个词的笔记。融合名次因此只由 chunk_id 兜底,顺序是 1,2,3,4 ——
    /// 重排把它反过来就能看出来生效了。
    fn four_matching_notes() -> Vec<Row<'static>> {
        vec![
            Row {
                doc_id: 1,
                path: "/v/1.md",
                title: "一",
                heading: "",
                body: "导出功能",
            },
            Row {
                doc_id: 2,
                path: "/v/2.md",
                title: "二",
                heading: "",
                body: "导出功能",
            },
            Row {
                doc_id: 3,
                path: "/v/3.md",
                title: "三",
                heading: "",
                body: "导出功能",
            },
            Row {
                doc_id: 4,
                path: "/v/4.md",
                title: "四",
                heading: "",
                body: "导出功能",
            },
        ]
    }

    fn dead_embed() -> EmbedConfig {
        EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        }
    }

    #[tokio::test]
    async fn reranking_reorders_the_results() {
        let rows = four_matching_notes();
        let (vault, _conn) = seeded(&rows, &[]);
        // 服务端把顺序倒过来。
        let base_url = one_shot_server(
            r#"{"results":[
                {"index":3,"relevance_score":0.9},
                {"index":2,"relevance_score":0.8},
                {"index":1,"relevance_score":0.7},
                {"index":0,"relevance_score":0.6}
            ]}"#,
        )
        .await;
        let outcome = search(
            &vault,
            4,
            &dead_embed(),
            "导出",
            &SearchOptions {
                limit: 4,
                expand_links: false,
                per_doc: DEFAULT_PER_DOC,
                rerank: Some(RerankConfig {
                    provider: "cohere".to_string(),
                    model: "rerank-v3".to_string(),
                    base_url,
                    api_key: String::new(),
                }),
            },
        )
        .await
        .expect("search");
        assert!(outcome.degraded.is_empty(), "{:?}", outcome.degraded);
        assert_eq!(
            outcome
                .hits
                .iter()
                .map(|h| h.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/v/4.md", "/v/3.md", "/v/2.md", "/v/1.md"],
            "重排后的顺序要生效"
        );
        assert_eq!(outcome.hits[0].source, "fts+rerank");
        // 分数换成了重排给的相关度,且与数组顺序一致。
        assert!((outcome.hits[0].score - 0.9).abs() < 1e-6);
        assert!(outcome.hits[0].score > outcome.hits[1].score);
    }

    #[tokio::test]
    async fn candidates_the_reranker_omitted_are_kept() {
        // 服务端只提了两条。剩下的是「没进 top_n」,不是「不相关」 —— 丢掉会让结果
        // 凭空变少。
        let rows = four_matching_notes();
        let (vault, _conn) = seeded(&rows, &[]);
        let base_url = one_shot_server(
            r#"{"results":[
                {"index":2,"relevance_score":0.9},
                {"index":0,"relevance_score":0.5}
            ]}"#,
        )
        .await;
        let outcome = search(
            &vault,
            4,
            &dead_embed(),
            "导出",
            &SearchOptions {
                limit: 4,
                expand_links: false,
                per_doc: DEFAULT_PER_DOC,
                rerank: Some(RerankConfig {
                    provider: "cohere".to_string(),
                    model: "rerank-v3".to_string(),
                    base_url,
                    api_key: String::new(),
                }),
            },
        )
        .await
        .expect("search");
        assert_eq!(outcome.hits.len(), 4, "没被提到的候选要留下");
        assert_eq!(outcome.hits[0].path, "/v/3.md");
        assert_eq!(outcome.hits[1].path, "/v/1.md");
        // 被提到的两条标了 rerank,其余没标。
        assert_eq!(
            outcome
                .hits
                .iter()
                .filter(|h| h.source.contains("rerank"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn a_reranker_returning_a_bad_index_degrades_instead_of_misordering() {
        // 越界下标必须报降级,而不是静默取错块。
        let rows = four_matching_notes();
        let (vault, _conn) = seeded(&rows, &[]);
        let base_url = one_shot_server(r#"{"results":[{"index":99,"relevance_score":0.9}]}"#).await;
        let outcome = search(
            &vault,
            4,
            &dead_embed(),
            "导出",
            &SearchOptions {
                limit: 4,
                expand_links: false,
                per_doc: DEFAULT_PER_DOC,
                rerank: Some(RerankConfig {
                    provider: "cohere".to_string(),
                    model: "rerank-v3".to_string(),
                    base_url,
                    api_key: String::new(),
                }),
            },
        )
        .await
        .expect("search");
        assert_eq!(
            outcome
                .degraded
                .iter()
                .map(|d| d.stage.as_str())
                .collect::<Vec<_>>(),
            vec!["rerank"]
        );
        assert_eq!(outcome.hits.len(), 4);
        assert!(outcome.hits.iter().all(|h| h.source == "fts"));
    }

    #[test]
    fn rerank_inputs_join_heading_and_body() {
        let (_vault, conn) = seeded(
            &[
                Row {
                    doc_id: 1,
                    path: "/v/a.md",
                    title: "甲",
                    heading: "存储",
                    body: "正文",
                },
                Row {
                    doc_id: 2,
                    path: "/v/b.md",
                    title: "乙",
                    heading: "",
                    body: "无标题",
                },
            ],
            &[],
        );
        let inputs = rerank_inputs(&conn, &[1, 2]).expect("inputs");
        assert_eq!(inputs.get(&1).map(String::as_str), Some("存储\n\n正文"));
        // 没有标题时不该留下一串前导换行。
        assert_eq!(inputs.get(&2).map(String::as_str), Some("无标题"));
    }

    #[tokio::test]
    async fn an_unlinked_vault_still_fills_the_limit() {
        // 预留额度没被扩展用掉时要还给主检索 —— 一个没有任何链接的 vault 不该因为
        // 「预留了两个位置」就少给两条结果。
        let rows: Vec<Row<'_>> = (1..=5)
            .map(|i| Row {
                doc_id: i,
                path: match i {
                    1 => "/v/1.md",
                    2 => "/v/2.md",
                    3 => "/v/3.md",
                    4 => "/v/4.md",
                    _ => "/v/5.md",
                },
                title: "笔记",
                heading: "",
                body: "导出功能",
            })
            .collect();
        let (vault, _conn) = seeded(&rows, &[]);
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(
            &vault,
            4,
            &config,
            "导出",
            &SearchOptions {
                limit: 4,
                expand_links: true,
                per_doc: DEFAULT_PER_DOC,
                rerank: None,
            },
        )
        .await
        .expect("search");
        assert_eq!(outcome.hits.len(), 4);
        assert!(outcome.hits.iter().all(|h| h.source == "fts"));
    }

    #[tokio::test]
    async fn a_limit_of_one_gives_the_slot_to_the_real_hit() {
        // limit=1 时扩展必须让位,否则唯一那个名额被关联笔记占掉。
        let rows: Vec<Row<'_>> = vec![
            Row {
                doc_id: 1,
                path: "/v/a.md",
                title: "甲",
                heading: "",
                body: "导出功能",
            },
            Row {
                doc_id: 2,
                path: "/v/b.md",
                title: "乙",
                heading: "",
                body: "关联笔记",
            },
        ];
        let (vault, _conn) = seeded(&rows, &[(1, "b")]);
        let config = EmbedConfig {
            provider: embed::EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        let outcome = search(
            &vault,
            4,
            &config,
            "导出",
            &SearchOptions {
                limit: 1,
                expand_links: true,
                per_doc: DEFAULT_PER_DOC,
                rerank: None,
            },
        )
        .await
        .expect("search");
        assert_eq!(outcome.hits.len(), 1);
        assert_eq!(outcome.hits[0].path, "/v/a.md");
    }
}
