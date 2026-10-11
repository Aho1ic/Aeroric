//! P8-05 切片九十九（notebook 缺口三条之二）：rag_index／rag_context 的命令
//! 面业务主体。与 `src-tauri/src/notebook/rag/commands.rs` 同源 verbatim 上收；
//! 差异只有两处，都是出口/钥匙串的注入而不是行为变更：
//! ① 进度事件的出口是调用方给的 [`ProgressSink`]（宿主 append 进 EventLog 的
//!    v1 topic；Tauri 的 Embedded 出口仍是窗口双发 legacy 通道＋v1 topic）；
//! ② embedding key 的补读是调用方给的同步闭包（宿主走 SecretStore、Tauri 走
//!    钥匙串——都在 block_rag 自己的线程上同步跑，那是 on_own_thread 的本意）。
//! 并发互斥（JobGuard／native_jobs）留在调用方：两边各有一张 vault→CancelToken
//! 的表，core 不持全局状态。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::context::{self, ContextOptions, CurrentNote};
use super::db;
use super::embed::EmbedConfig;
use super::index::{self, IndexOutcome, IndexScope};
use super::search::{self, SearchOptions, SearchOutcome};
use crate::notebook::vault_index::split_frontmatter;

/// 确认一篇笔记落在给定 vault 里面（Tauri `ensure_note_in_vault` verbatim）。
///
/// `resolve_in_vaults` 只保证路径在**某个**已授权 vault 里,而这些命令的 `vault`
/// 参数决定了打开哪个索引库。两者不校验一致的话,前端(或者一个拼错的调用)可以
/// 拿 A vault 的库去装配 B vault 的笔记 —— 上下文里会混进另一个库的内容,而调用方
/// 完全看不出来。
pub fn ensure_note_in_vault(note: &Path, vault: &Path) -> Result<(), String> {
    if !note.starts_with(vault) {
        return Err("Notebook note is outside the given vault".to_string());
    }
    Ok(())
}

/// 上限类参数的取默认值口径:`None` 与 `Some(0)` 一样当成"没填"（Tauri
/// `positive_or` verbatim）。
///
/// 0 会让对应的那一步直接返回空,而那不像是用户的意图,更像是前端某个还没初始化
/// 完的状态(受控输入清空时就是 0)。
pub fn positive_or(value: Option<usize>, fallback: usize) -> usize {
    value.filter(|v| *v > 0).unwrap_or(fallback)
}

/// 检索参数的前端形状（Tauri `SearchOptionsDto` verbatim，补 Serialize 供宿主
/// 与壳两侧共用同一 serde）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptionsDto {
    pub limit: Option<usize>,
    pub expand_links: Option<bool>,
    pub per_doc: Option<usize>,
    pub rerank: Option<super::rerank::RerankConfig>,
}

impl SearchOptionsDto {
    pub fn into_options(self) -> SearchOptions {
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

/// 上下文预算的前端形状（Tauri `ContextOptionsDto` verbatim）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOptionsDto {
    pub max_tokens: Option<usize>,
    pub current_chars: Option<usize>,
}

impl ContextOptionsDto {
    pub fn into_options(self) -> ContextOptions {
        let defaults = ContextOptions::default();
        ContextOptions {
            max_tokens: positive_or(self.max_tokens, defaults.max_tokens),
            current_chars: positive_or(self.current_chars, defaults.current_chars),
        }
    }
}

/// 一次问答的上下文（Tauri `ContextBundle` verbatim）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBundle {
    #[serde(flatten)]
    pub assembled: context::Assembled,
    /// 检索时哪几路降级了。装配层不知道这件事,但 UI 必须一起显示 —— 否则
    /// 「上下文里没有想要的片段」看起来像是检索没找到,而实际是模型没连上。
    pub degraded: Vec<search::Degraded>,
    pub vectors_missing: bool,
}

/// 建索引（Tauri `notebook_rag_index` 的主体 verbatim；JobGuard 留在调用方）。
///
/// 调用方先占住 vault 的取消令牌（并发互斥），补 key 排在占位**之后**——同一个
/// 理由:钥匙串读在 macOS 上可能弹一个授权框,那期间的第二次点击必须撞在占位上。
pub async fn rag_index(
    resolved: &Path,
    config: EmbedConfig,
    scope: IndexScope,
    cancel: &super::index::CancelToken,
    sink: &dyn index::ProgressSink,
    resolve_key: &dyn Fn(EmbedConfig) -> Result<EmbedConfig, String>,
) -> Result<IndexOutcome, String> {
    let config = resolve_key(config)?;
    // 维度也在这条线程上探:它是索引的一部分,provider 连不上时这一轮本来
    // 就做不了,不该先占着位置再回去报错。
    let dim = super::embed::probe_dimension(&config)
        .await
        .map_err(|e| e.message().to_string())?;
    index::index_vault(resolved, dim, &config, scope, cancel, sink).await
}

/// 装配一次问答的上下文（Tauri `notebook_rag_context` 的主体 verbatim）。
///
/// `current` 由调用方解析出笔记真实路径并带上**编辑器里的当前内容**：用户问的
/// 往往正是刚写下还没保存的那几行,读盘会拿到一份旧的。路径仍然要过
/// [`ensure_note_in_vault`]（它会进返回值,也用于与命中去重）,并且允许尚不存在
/// —— 新建还没保存的笔记正是主场景。
pub async fn rag_context(
    resolved: &Path,
    current: Option<(PathBuf, String)>,
    query: &str,
    config: EmbedConfig,
    search_options: SearchOptionsDto,
    context_options: ContextOptionsDto,
    resolve_key: &dyn Fn(EmbedConfig) -> Result<EmbedConfig, String>,
) -> Result<ContextBundle, String> {
    let current = match current {
        Some((note_path, raw)) => {
            ensure_note_in_vault(&note_path, resolved)?;
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

    let outcome = match db::peek_dim(resolved) {
        Some(dim) => {
            let options = search_options.into_options();
            // 补 key 放在这个分支里而不是函数开头:没建过索引时下面根本不发 embedding
            // 请求,那就不该去碰钥匙串。
            let config = resolve_key(config)?;
            search::search(resolved, dim, &config, query, &options).await?
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
        &context_options.into_options(),
    );
    Ok(ContextBundle {
        assembled,
        degraded: outcome.degraded,
        vectors_missing: outcome.vectors_missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_context_upper_bound_goes_through_the_same_rule() {
        // 漏掉一处不会有任何症状,直到用户清空那个输入框。所以逐个 DTO 字段验一遍
        // 落到了默认值,而不是只验共用函数本身。默认值钉字面量,不读 Default ——
        // 否则把 DEFAULT_MAX_TOKENS 改成 0,RAG 上下文会变成空串而测试照绿。
        let zeroed = ContextOptionsDto {
            max_tokens: Some(0),
            current_chars: Some(0),
        }
        .into_options();
        assert_eq!(zeroed.max_tokens, 3000);
        assert_eq!(zeroed.current_chars, 1200);

        // Some(0) 与 None 必须走同一条路径;漏掉 None 会让「没填」和「清空」分叉。
        let omitted = ContextOptionsDto {
            max_tokens: None,
            current_chars: None,
        }
        .into_options();
        assert_eq!(omitted.max_tokens, 3000);
        assert_eq!(omitted.current_chars, 1200);
    }
}
