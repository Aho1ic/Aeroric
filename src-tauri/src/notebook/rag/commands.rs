//! RAG 的 `#[tauri::command]` 层。
//!
//! 这一层只做四件事:把 vault 路径过 allowlist、解析出向量维度、把索引任务按
//! vault 互斥、把进度转成事件。检索与索引的逻辑在同级模块里,那些都不知道 Tauri
//! 存在(也因此可测)。
//!
//! ## 维度是从哪来的
//!
//! `dim` 对 [`db::open`] 是权威的:传错会重建向量表并把全库标 `Stale`。而探测维度
//! 要发网络请求,所以只有**建索引**那条路探(它本来就要连 provider);「看现状」
//! 「搜一下」两条路读库里记着的([`db::peek_dim`]),读不出来就当没建过索引。
//!
//! ## 为什么索引任务是 await 而不是 spawn
//!
//! 返回 [`index::IndexOutcome`] 让前端拿到确定的结果(索引了几篇、失败几篇、是不是
//! 被取消),而不必从事件流里拼。代价是 `index_vault` 里的 SQLite 写会占用一个
//! async worker —— 单篇一个短事务,而 provider 请求那一段是正常 await,可以接受。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter, State};

use super::context::{self, ContextOptions, CurrentNote};
use super::embed::{self, EmbedConfig};
use super::index::{self, CancelToken, IndexProgress, IndexScope, ProgressSink};
use super::rerank::RerankConfig;
use super::search::{self, SearchOptions, SearchOutcome};
use super::{db, graph};
use crate::notebook::state::NotebookState;
use crate::notebook::vault_index::split_frontmatter;

/// 进度事件名。前端 `listen` 这个 topic。
const PROGRESS_EVENT: &str = "notebook-rag-index-progress";

/// 邻居列表不传 limit 时取几个。够填满一个引用面板,又不至于把长文的反链全拉出来。
const DEFAULT_NEIGHBOR_LIMIT: usize = 8;

/// 确认一篇笔记落在给定 vault 里面。
///
/// `resolve_in_vaults` 只保证路径在**某个**已授权 vault 里,而这些命令的 `vault`
/// 参数决定了打开哪个索引库。两者不校验一致的话,前端(或者一个拼错的调用)可以
/// 拿 A vault 的库去装配 B vault 的笔记 —— 上下文里会混进另一个库的内容,而调用方
/// 完全看不出来。
///
/// 单独抽出来是因为它在两条命令里各写了一遍,而这种校验漏掉一处不会有任何症状。
fn ensure_note_in_vault(note: &Path, vault: &Path) -> Result<(), String> {
    if !note.starts_with(vault) {
        return Err("Notebook note is outside the given vault".to_string());
    }
    Ok(())
}

/// 正在跑的索引任务:vault → 取消标志。
///
/// 同一个 vault 上两个并发的索引任务会互相覆盖 `docs.status`、重复花 embedding
/// 请求,并且都以为自己那份是最终状态。所以按 vault 互斥,而不是全局互斥 ——
/// 两个不同 vault 各自建索引是合理操作。
fn jobs() -> &'static Mutex<HashMap<PathBuf, CancelToken>> {
    static JOBS: OnceLock<Mutex<HashMap<PathBuf, CancelToken>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 登记一个任务,离开作用域时自动注销。
///
/// 用 guard 而不是在函数末尾手工移除:`index_vault` 有多条 `return`(取消、失败),
/// 漏一条就会把这个 vault 永久锁死 —— 表现是「重启前再也建不了索引」。
struct JobGuard {
    vault: PathBuf,
    cancel: CancelToken,
}

impl JobGuard {
    fn begin(vault: &Path) -> Result<Self, String> {
        let mut map = jobs().lock().map_err(|e| e.to_string())?;
        if map.contains_key(vault) {
            return Err("Notebook index is already running for this vault".to_string());
        }
        let cancel = CancelToken::new();
        map.insert(vault.to_path_buf(), cancel.clone());
        Ok(Self {
            vault: vault.to_path_buf(),
            cancel,
        })
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = jobs().lock() {
            map.remove(&self.vault);
        }
    }
}

/// 带上 vault 的进度事件。
///
/// `IndexProgress` 自己不带 vault:那一层不知道有几个 vault。可是前端可能同时开着
/// 两个 vault 的面板,不带 vault 的进度会画到错的那个进度条上。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    vault: String,
    #[serde(flatten)]
    progress: IndexProgress,
}

/// 把进度转成 Tauri 事件。
///
/// 持有 `AppHandle` 而不是借用:索引跑在自己的线程上(见 [`on_own_thread`]),
/// 借用活不过命令返回。`AppHandle` 是 `Clone + Send + Sync`,克隆是廉价的。
struct EventSink {
    app: AppHandle,
    vault: String,
}

impl ProgressSink for EventSink {
    fn report(&self, progress: &IndexProgress) {
        // 发不出去也不该让索引停下 —— 窗口可能已经关了,而索引本身是有价值的。
        let _ = self.app.emit(
            PROGRESS_EVENT,
            ProgressEvent {
                vault: self.vault.clone(),
                progress: progress.clone(),
            },
        );
    }
}

/// 在自己的线程 + 自己的 current-thread runtime 上跑一个 future,await 它的结果。
///
/// 两个理由,缺一不可:
///
/// 1. `rusqlite::Connection` 是 `Send` 但**不是** `Sync`,而 `index_vault` 与
///    `search` 都要在 await 之间攥着 `&Connection`。`&T` 只有在 `T: Sync` 时才是
///    `Send`,于是它们的 future 不是 `Send` —— 而 `#[tauri::command]` 要求 `Send`。
/// 2. 这两条路里的 SQLite 读写是阻塞的。跑在 Tauri 的 async worker 上会占着 IPC
///    线程池里的一格,而建索引可能要几分钟。
///
/// 这不是「runtime 套 runtime」:线程是新开的,上面本来没有 runtime。(套用会
/// panic,那也是当初没有照抄 Markio 的 `rerank_blocking` 的原因。)
async fn on_own_thread<T, F, Fut>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("notebook-rag".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(Err(format!("Cannot start notebook RAG runtime: {error}")));
                    return;
                }
            };
            // 接收端可能已经没了(窗口关了)。工作照做完 —— 索引的成果留在库里
            // 仍然有用,半途放弃只会让下次从头再来。
            let _ = tx.send(runtime.block_on(work()));
        })
        .map_err(|e| format!("Cannot start notebook RAG worker: {e}"))?;
    rx.await
        .map_err(|_| "The notebook RAG worker stopped unexpectedly".to_string())?
}

/// 检索参数的前端形状。全部可省,省了就用 [`SearchOptions::default`] 的值。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptionsDto {
    pub limit: Option<usize>,
    pub expand_links: Option<bool>,
    pub per_doc: Option<usize>,
    pub rerank: Option<RerankConfig>,
}

/// 上限类参数的取默认值口径:`None` 与 `Some(0)` 一样当成"没填"。
///
/// 0 会让对应的那一步直接返回空,而那不像是用户的意图,更像是前端某个还没初始化
/// 完的状态(受控输入清空时就是 0)。四处上限都走这里,是因为这条口径散在各处的
/// 时候,漏掉一处不会有任何症状 —— 直到用户清空一个输入框,面板就空了。
///
/// 注意**不是**所有 0 都该被吃掉:`per_doc` 的 0 表示"每篇不限",是有意义的取值,
/// 所以它不走这里。
fn positive_or(value: Option<usize>, fallback: usize) -> usize {
    value.filter(|v| *v > 0).unwrap_or(fallback)
}

impl SearchOptionsDto {
    fn into_options(self) -> SearchOptions {
        let defaults = SearchOptions::default();
        SearchOptions {
            limit: positive_or(self.limit, defaults.limit),
            expand_links: self.expand_links.unwrap_or(defaults.expand_links),
            // per_doc 的 0 是有意义的(不限),所以不走 `positive_or`。
            per_doc: self.per_doc.unwrap_or(defaults.per_doc),
            rerank: self.rerank,
        }
    }
}

/// 上下文预算的前端形状。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOptionsDto {
    pub max_tokens: Option<usize>,
    pub current_chars: Option<usize>,
}

impl ContextOptionsDto {
    fn into_options(self) -> ContextOptions {
        let defaults = ContextOptions::default();
        ContextOptions {
            max_tokens: positive_or(self.max_tokens, defaults.max_tokens),
            current_chars: positive_or(self.current_chars, defaults.current_chars),
        }
    }
}

/// 一次问答的上下文。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBundle {
    #[serde(flatten)]
    pub assembled: context::Assembled,
    /// 检索时哪几路降级了。装配层不知道这件事,但 UI 必须一起显示 —— 否则
    /// 「上下文里没有想要的片段」看起来像是检索没找到,而实际是模型没连上。
    pub degraded: Vec<search::Degraded>,
    pub vectors_missing: bool,
}

/// 探一次 embedding 服务:通不通、维度多少。
///
/// 设置页的「测试连接」用它。建索引前先探一次也有意义 —— provider 没开着的话
/// 不该白起一轮会整批失败的索引。
#[tauri::command]
pub async fn notebook_rag_probe(config: EmbedConfig) -> Result<usize, String> {
    embed::probe_dimension(&config)
        .await
        .map_err(|e| e.message().to_string())
}

/// 读索引现状。不发网络请求 —— 面板一打开就调它。
#[tauri::command]
pub async fn notebook_rag_stats(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<index::IndexStats, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    crate::notebook::blocking(move || match db::peek_dim(&resolved) {
        Some(dim) => index::stats(&resolved, dim),
        // 还没建过索引。全零而不是报错 —— 那是正常的初始状态。
        None => Ok(index::IndexStats::default()),
    })
    .await
}

/// 建索引。进度走 `notebook-rag-index-progress` 事件,可用
/// [`notebook_rag_cancel`] 取消。
#[tauri::command]
pub async fn notebook_rag_index(
    app: AppHandle,
    state: State<'_, NotebookState>,
    vault: String,
    config: EmbedConfig,
    scope: Option<IndexScope>,
) -> Result<index::IndexOutcome, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    // 先占住这个 vault 再做别的:反过来的话探测维度那几百毫秒里第二次点击会溜
    // 进来,于是两个任务并发写同一个库。
    let guard = JobGuard::begin(&resolved)?;
    let cancel = guard.cancel.clone();
    let scope = scope.unwrap_or_default();
    let vault_label = resolved.to_string_lossy().to_string();
    on_own_thread(move || async move {
        // 任务登记随线程结束而注销 —— `index_vault` 有多条 return,交给 Drop 比
        // 每条路径手工注销可靠。
        let _guard = guard;
        // 维度也在这条线程上探:它是索引的一部分,provider 连不上时这一轮本来
        // 就做不了,不该先占着位置再回去报错。
        let dim = embed::probe_dimension(&config)
            .await
            .map_err(|e| e.message().to_string())?;
        let sink = EventSink {
            app,
            vault: vault_label,
        };
        index::index_vault(&resolved, dim, &config, scope, &cancel, &sink).await
    })
    .await
}

/// 请求取消这个 vault 上正在跑的索引。返回是否真有任务被通知到。
///
/// 取消是**协作式**的:标志置位后 `index_vault` 在下一个检查点收工并把已经算完的
/// 部分留在库里。所以取消不会丢已完成的工作,重新建索引会从那儿接着走。
#[tauri::command]
pub async fn notebook_rag_cancel(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<bool, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    let map = jobs().lock().map_err(|e| e.to_string())?;
    let Some(cancel) = map.get(&resolved) else {
        return Ok(false);
    };
    cancel.cancel();
    Ok(true)
}

/// 丢掉整个索引库。
///
/// 索引是派生数据,删掉只会让检索暂时失效。但仍然要拦住「索引正在跑的时候删库」:
/// 那会让还在写的任务对着一个已经不存在的文件继续写,把库重新建出来一半。
#[tauri::command]
pub async fn notebook_rag_clear(
    state: State<'_, NotebookState>,
    vault: String,
) -> Result<(), String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    let guard = JobGuard::begin(&resolved)?;
    let result = crate::notebook::blocking(move || index::clear(&resolved)).await;
    drop(guard);
    result
}

/// 混合检索。
///
/// 维度取库里记着的那个,不探测:探测要发请求,而**索引里的**维度才是能查的那个。
/// provider 换了模型时查询向量长度会对不上,那由 [`search::search`] 降级处理。
#[tauri::command]
pub async fn notebook_rag_search(
    state: State<'_, NotebookState>,
    vault: String,
    query: String,
    config: EmbedConfig,
    options: Option<SearchOptionsDto>,
) -> Result<SearchOutcome, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    let Some(dim) = db::peek_dim(&resolved) else {
        // 还没建过索引。`vectors_missing` 让 UI 提示「先建索引」,而不是「检查模型配置」。
        return Ok(SearchOutcome {
            hits: Vec::new(),
            degraded: Vec::new(),
            vectors_missing: true,
        });
    };
    let options = options.unwrap_or_default().into_options();
    on_own_thread(
        move || async move { search::search(&resolved, dim, &config, &query, &options).await },
    )
    .await
}

/// 装配一次问答的上下文:当前笔记 + 检索片段,按 token 预算裁。
///
/// `currentBody` 由前端传**编辑器里的当前内容**而不是这里读盘:用户问的往往正是
/// 刚写下还没保存的那几行,读盘会拿到一份旧的。路径仍然要过 allowlist(它会进
/// 返回值,也用于与命中去重),并且允许尚不存在 —— 新建还没保存的笔记正是主场景。
#[tauri::command]
pub async fn notebook_rag_context(
    state: State<'_, NotebookState>,
    vault: String,
    query: String,
    config: EmbedConfig,
    current_path: Option<String>,
    current_body: Option<String>,
    search_options: Option<SearchOptionsDto>,
    context_options: Option<ContextOptionsDto>,
) -> Result<ContextBundle, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    let current = match current_path.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(path) => {
            let note_path = state.resolve_in_vaults(path, true)?;
            ensure_note_in_vault(&note_path, &resolved)?;
            let raw = current_body.unwrap_or_default();
            // 与建索引同一套口径:归一化换行、剥掉 frontmatter。不一致的话
            // `char_start` 与这里的偏移不在同一个坐标系里,去重会错位。
            let normalized = raw.replace("\r\n", "\n");
            let (_front, body) = split_frontmatter(&normalized);
            let path_string = note_path.to_string_lossy().to_string();
            Some(CurrentNote {
                title: index::title_of(&raw, &path_string),
                path: path_string,
                body: body.to_string(),
            })
        }
        None => None,
    };

    let outcome = match db::peek_dim(&resolved) {
        Some(dim) => {
            let options = search_options.unwrap_or_default().into_options();
            let vault_path = resolved.clone();
            on_own_thread(move || async move {
                search::search(&vault_path, dim, &config, &query, &options).await
            })
            .await?
        }
        // 没索引也要出上下文:当前笔记本身仍然是有用的上下文,而这正是「刚装好、
        // 还没建索引」时用户的第一次提问。
        None => SearchOutcome {
            hits: Vec::new(),
            degraded: Vec::new(),
            vectors_missing: true,
        },
    };

    let assembled = context::assemble(
        current.as_ref(),
        &outcome.hits,
        &context_options.unwrap_or_default().into_options(),
    );
    Ok(ContextBundle {
        assembled,
        degraded: outcome.degraded,
        vectors_missing: outcome.vectors_missing,
    })
}

/// 一篇笔记的邻居(它引用的 + 引用它的),给引用面板与「相关笔记」用。
///
/// 走索引里的 `links` 表,所以只有建过索引的 vault 有结果。解析规则与前端
/// `noteLinks.ts` 对齐(stem → 标题 → 路径),见 [`graph`] 的模块注释。
#[tauri::command]
pub async fn notebook_rag_neighbors(
    state: State<'_, NotebookState>,
    vault: String,
    path: String,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    let resolved = state.resolve_in_vaults(&vault, false)?;
    let note = state.resolve_in_vaults(&path, false)?;
    ensure_note_in_vault(&note, &resolved)?;
    let limit = positive_or(limit, DEFAULT_NEIGHBOR_LIMIT);
    crate::notebook::blocking(move || {
        let Some(dim) = db::peek_dim(&resolved) else {
            return Ok(Vec::new());
        };
        let Some(conn) = db::open_existing(&resolved, dim)? else {
            return Ok(Vec::new());
        };
        let docs = graph::load_docs(&conn)?;
        let wanted = note.to_string_lossy().to_string();
        let Some(doc) = docs.iter().find(|d| d.path == wanted) else {
            // 这篇还没进索引。空列表而不是报错 —— 新建的笔记本来就还没索引。
            return Ok(Vec::new());
        };
        let index = graph::LinkIndex::build(&docs);
        let ids = graph::neighbors(&conn, &index, doc, limit)?;
        let by_id: HashMap<i64, &graph::DocRef> = docs.iter().map(|d| (d.id, d)).collect();
        Ok(ids
            .into_iter()
            .filter_map(|id| by_id.get(&id).map(|d| d.path.clone()))
            .collect())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notebook::rag::index::IndexPhase;

    /// 每个测试一个独立的 vault 路径。任务注册表是进程全局的,共用路径会让并行跑的
    /// 测试互相抢锁。路径不必真的存在 —— [`JobGuard`] 只拿它当键。
    fn fake_vault(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        PathBuf::from(format!(
            "/nonexistent/aeroric-rag-jobs-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn a_second_index_run_on_the_same_vault_is_refused() {
        // 两个并发任务会互相覆盖 `docs.status`、重复花 embedding 请求,而且都以为
        // 自己那份是最终状态。
        let vault = fake_vault("busy");
        let first = JobGuard::begin(&vault).expect("first");
        let second = JobGuard::begin(&vault);
        assert!(second.is_err(), "同一个 vault 上不该并发跑两个索引");
        drop(first);
        // 上一个结束之后必须能再开 —— 漏注销的表现是「重启前再也建不了索引」。
        JobGuard::begin(&vault).expect("释放之后要能重新开始");
    }

    #[test]
    fn two_different_vaults_can_index_at_once() {
        let a = fake_vault("a");
        let b = fake_vault("b");
        let _first = JobGuard::begin(&a).expect("a");
        JobGuard::begin(&b).expect("不同 vault 各自建索引是合理操作");
    }

    #[test]
    fn cancelling_flips_the_registered_token() {
        // `notebook_rag_cancel` 拿到的是注册表里那一份,必须与任务手里的是同一个
        // flag —— 克隆错了会让取消按钮变成装饰。
        let vault = fake_vault("cancel");
        let guard = JobGuard::begin(&vault).expect("begin");
        assert!(!guard.cancel.is_cancelled());
        jobs()
            .lock()
            .expect("lock")
            .get(&vault)
            .expect("registered")
            .cancel();
        assert!(guard.cancel.is_cancelled());
    }

    #[test]
    fn the_registry_is_empty_after_the_guard_drops() {
        let vault = fake_vault("drop");
        {
            let _guard = JobGuard::begin(&vault).expect("begin");
            assert!(jobs().lock().expect("lock").contains_key(&vault));
        }
        assert!(!jobs().lock().expect("lock").contains_key(&vault));
    }

    #[test]
    fn omitted_search_options_fall_back_to_the_defaults() {
        let options = SearchOptionsDto::default().into_options();
        let defaults = SearchOptions::default();
        assert_eq!(options.limit, defaults.limit);
        assert_eq!(options.expand_links, defaults.expand_links);
        assert_eq!(options.per_doc, defaults.per_doc);
        assert!(options.rerank.is_none());
    }

    #[test]
    fn a_note_in_another_vault_is_refused() {
        // 两个 vault 都可能是已授权的,所以 allowlist 放行它 —— 拦住它的是这一道。
        // 放过去的后果是 A 的索引库拿去装配 B 的笔记,而调用方看不出任何异常。
        let a = Path::new("/vaults/a");
        let b = Path::new("/vaults/b");
        assert!(ensure_note_in_vault(&a.join("note.md"), a).is_ok());
        let err = ensure_note_in_vault(&b.join("note.md"), a).expect_err("跨 vault 必须拒绝");
        assert!(err.contains("outside the given vault"), "错误信息:{err}");
    }

    #[test]
    fn a_sibling_directory_with_the_same_prefix_is_refused() {
        // `starts_with` 是按路径段比的,不是按字符串前缀 —— 否则 `/vaults/ab` 会被
        // 当成 `/vaults/a` 的里面。这里盯住那个区别。
        let vault = Path::new("/vaults/a");
        assert!(ensure_note_in_vault(Path::new("/vaults/ab/note.md"), vault).is_err());
        assert!(ensure_note_in_vault(Path::new("/vaults/a/sub/note.md"), vault).is_ok());
    }

    #[test]
    fn a_zero_upper_bound_is_treated_as_omitted() {
        // 0 会让对应那一步直接返回空。那不像用户的意图,更像前端某个还没初始化的
        // 状态 —— 而「搜索永远没有结果」是最难查的一种 bug。四处上限共用这一条
        // 口径,所以这里盯的是那个共用函数。
        assert_eq!(positive_or(Some(0), 8), 8, "0 当成没填");
        assert_eq!(positive_or(None, 8), 8);
        assert_eq!(positive_or(Some(3), 8), 3, "填了就用填的");
    }

    #[test]
    fn every_upper_bound_goes_through_the_same_rule() {
        // 漏掉一处不会有任何症状,直到用户清空那个输入框。所以逐个 DTO 字段验一遍
        // 落到了默认值,而不是只验共用函数本身。
        let search = SearchOptionsDto {
            limit: Some(0),
            ..Default::default()
        }
        .into_options();
        assert_eq!(search.limit, SearchOptions::default().limit);

        let ctx = ContextOptionsDto {
            max_tokens: Some(0),
            current_chars: Some(0),
        }
        .into_options();
        let defaults = ContextOptions::default();
        assert_eq!(ctx.max_tokens, defaults.max_tokens);
        assert_eq!(ctx.current_chars, defaults.current_chars);
    }

    #[test]
    fn a_zero_per_doc_is_honoured() {
        // per_doc 的 0 是有意义的:不限制同一篇贡献几块。不能和「没填」混为一谈。
        let options = SearchOptionsDto {
            per_doc: Some(0),
            ..Default::default()
        }
        .into_options();
        assert_eq!(options.per_doc, 0);
    }

    #[test]
    fn search_options_deserialize_from_camel_case() {
        let dto: SearchOptionsDto =
            serde_json::from_str(r#"{"limit":5,"expandLinks":false,"perDoc":1}"#).expect("parse");
        let options = dto.into_options();
        assert_eq!(options.limit, 5);
        assert!(!options.expand_links);
        assert_eq!(options.per_doc, 1);
    }

    #[test]
    fn context_options_fall_back_to_the_defaults() {
        let options = ContextOptionsDto {
            max_tokens: Some(0),
            current_chars: None,
        }
        .into_options();
        let defaults = ContextOptions::default();
        assert_eq!(options.max_tokens, defaults.max_tokens);
        assert_eq!(options.current_chars, defaults.current_chars);
    }

    #[test]
    fn the_progress_event_carries_the_vault() {
        // 前端可能同时开着两个 vault 的面板。不带 vault 的进度会画到错的进度条上。
        let event = ProgressEvent {
            vault: "/v".to_string(),
            progress: IndexProgress {
                phase: IndexPhase::Embedding,
                total: 10,
                done: 3,
                failed: 1,
                current: Some("/v/a.md".to_string()),
                error: None,
            },
        };
        let json = serde_json::to_value(&event).expect("serialize");
        assert_eq!(json["vault"], "/v");
        assert_eq!(json["phase"], "embedding");
        assert_eq!(json["done"], 3);
        assert_eq!(json["current"], "/v/a.md");
    }

    #[test]
    fn the_context_bundle_is_flat() {
        let bundle = ContextBundle {
            assembled: context::assemble(None, &[], &ContextOptions::default()),
            degraded: vec![search::Degraded {
                stage: "vector".to_string(),
                detail: "boom".to_string(),
            }],
            vectors_missing: true,
        };
        let json = serde_json::to_value(&bundle).expect("serialize");
        assert_eq!(json["tokens"], 0);
        assert!(json["citations"].is_array());
        assert_eq!(json["degraded"][0]["stage"], "vector");
        assert_eq!(json["vectorsMissing"], true);
    }

    #[test]
    fn the_index_scope_parses_the_camel_case_name() {
        assert_eq!(
            serde_json::from_str::<IndexScope>(r#""failedOnly""#).expect("parse"),
            IndexScope::FailedOnly
        );
        assert_eq!(
            serde_json::from_str::<IndexScope>(r#""all""#).expect("parse"),
            IndexScope::All
        );
        assert_eq!(IndexScope::default(), IndexScope::All);
    }

    #[tokio::test]
    async fn the_worker_thread_returns_its_result() {
        let value = on_own_thread(|| async { Ok::<_, String>(7usize) })
            .await
            .expect("result");
        assert_eq!(value, 7);
    }

    #[tokio::test]
    async fn the_worker_thread_propagates_errors() {
        let error = on_own_thread(|| async { Err::<usize, String>("boom".to_string()) })
            .await
            .expect_err("error");
        assert_eq!(error, "boom");
    }

    #[tokio::test]
    async fn the_worker_thread_can_hold_a_connection_across_an_await() {
        // 这条测试钉住 `on_own_thread` 存在的理由:`&Connection` 跨 await 会让
        // future 不是 `Send`,而这个包装让它无所谓。改成 spawn / spawn_blocking
        // 的话这里编译不过。
        let vault = std::env::temp_dir().join(format!(
            "aeroric-rag-cmd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&vault).expect("create vault");
        let probe = vault.clone();
        let count = on_own_thread(move || async move {
            let conn = db::open(&probe, 4)?;
            tokio::task::yield_now().await;
            conn.query_row("SELECT count(*) FROM docs", [], |row| row.get::<_, i64>(0))
                .map_err(|e| e.to_string())
        })
        .await
        .expect("query");
        assert_eq!(count, 0);
    }
}
