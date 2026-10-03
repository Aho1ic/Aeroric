//! 随手记的 RAG 索引与检索（P7-06 自 `src-tauri/src/notebook/rag/` 上收；
//! 9 个业务模块原样移动，`#[tauri::command]` 层留在宿主）。
//!
//! 分层:
//!
//! ```text
//! cjk      FTS5 的中日韩分词垫片。SQLite 自带的分词器不切中文,不垫这一层
//!          关键词检索对中文恒零命中(而且不报错)。
//! chunk    markdown 切块。按标题分段、段内按段落聚合、代码围栏整块保留。
//! commands `#[tauri::command]` 层。allowlist、维度解析、任务互斥、进度事件。
//!          （宿主侧；经 re-export 使用本模块）
//! context  AI 对话的上下文装配。当前笔记 + 检索片段,按 token 预算拼。
//! db       索引库(`<vault>/.notebook/index.db`)的连接与 schema。
//! embed    取向量。Ollama 原生 + OpenAI 兼容两路,入库前归一化。
//! graph    引用图。把 `[[字面目标]]` 解析成 doc_id,据此取邻居。
//! index    建索引流水线。带进度、可取消、单篇失败隔离且可重试。
//! rerank   可选的 cross-encoder 精排。cohere 兼容协议一路。
//! search   混合检索。向量 + 关键词按 RRF 融合,再按引用图补关联笔记。
//! ```
//!
//! 索引是**派生数据**:删掉 `index.db` 只会让检索暂时失效,重建一次即可,不丢
//! 任何用户内容。这是它敢用 `synchronous = NORMAL`、也敢在维度变化时整表重建的
//! 前提。
//!
//! ## topic 常量表（v1 接线证据面）
//!
//! 宿主 `commands.rs` 的 emit wrapper 发**两条**：旧桌面通道（verbatim）与
//! v1 topic。⚠️ 历史教训（第十一补记的符号表）：旧通道名与 `import/report.rs`
//! 的 `PROGRESS_EVENT` 同名不同值——两处常量各自文件内解析，不得跨文件合并。

/// v1 事件 topic：RAG 索引进度（payload verbatim＝`IndexProgress` camelCase）。
pub const RAG_INDEX_PROGRESS_TOPIC: &str = "notebook.rag_index_progress";
/// 旧桌面通道名（React 客户端监听；原 `commands.rs:35` 的字面值）。
pub const RAG_INDEX_PROGRESS_LEGACY: &str = "notebook-rag-index-progress";

pub mod chunk;
pub mod cjk;
pub mod context;
pub mod db;
pub mod embed;
pub mod graph;
pub mod index;
pub mod rerank;
pub mod search;
