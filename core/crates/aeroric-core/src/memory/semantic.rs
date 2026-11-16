//! B6 (2026-11-14): 可选 embedding — "按意思找相近的记忆". A derived index
//! only (frontier-roadmap J5): the vectors find candidates, the items with
//! their evidence stay the authority, and dropping the table loses nothing.
//!
//! - Vectors live in memory.db's `vectors` table, one per active item, all
//!   from one model: when the model changes, the table is emptied and
//!   rebuilt. Similarity is a dot product of unit vectors, in memory — a few
//!   thousand items need no vector index.
//! - Core never calls the network: the host passes an `embed` function built
//!   from 随手记's embedding settings, and fills the table in its sweep.
//! - `memory_recall` fuses the FTS hits with the nearest vectors (RRF); a
//!   dream also groups items whose vectors are close (换了说法的重复).

use std::collections::HashMap;

use aeroric_protocol::memory::{MemoryItem, SemanticStatus, SEMANTIC_EACH, SEMANTIC_RECALL_MIN};

use super::{Memory, MemoryError};

/// RRF's constant: a hit's weight is `1 / (RRF_K + place)`.
const RRF_K: f64 = 60.0;

/// The kv key holding the model the table was built with, and the last error.
const MODEL_KEY: &str = "vectors:model";
const ERROR_KEY: &str = "vectors:error";

/// What an item is embedded as: its text, and an experience's problem and fix.
pub fn text_of(item: &MemoryItem) -> String {
    match &item.experience {
        Some(detail) => format!("{}\n{}\n{}", item.text, detail.problem, detail.solution),
        None => item.text.clone(),
    }
}

/// nomic-embed-text is trained with task prefixes and ranks noticeably worse
/// without them; other models take the text as it is.
pub fn for_document(model: &str, text: &str) -> String {
    if model.contains("nomic-embed") {
        format!("search_document: {text}")
    } else {
        text.to_string()
    }
}

pub fn for_query(model: &str, text: &str) -> String {
    if model.contains("nomic-embed") {
        format!("search_query: {text}")
    } else {
        text.to_string()
    }
}

/// Unit length, so a dot product is the cosine.
pub fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Reciprocal rank fusion of ranked id lists: higher is better.
pub fn fuse(lists: &[Vec<String>]) -> HashMap<String, f64> {
    let mut score: HashMap<String, f64> = HashMap::new();
    for list in lists {
        for (place, id) in list.iter().enumerate() {
            *score.entry(id.clone()).or_default() += 1.0 / (RRF_K + place as f64 + 1.0);
        }
    }
    score
}

/// Active items of the project and global nearest to `query` (a unit
/// vector), best first, at most `SEMANTIC_EACH`, none under
/// `SEMANTIC_RECALL_MIN`.
pub(super) fn nearest(
    store: &super::store::Store,
    project_id: &str,
    query: &[f32],
) -> Result<Vec<String>, MemoryError> {
    let mut scored: Vec<(f32, String)> = store
        .vectors(Some(project_id))?
        .into_iter()
        .chain(store.vectors(None)?)
        .map(|(id, v)| (cosine(query, &v), id))
        .filter(|(score, _)| *score >= SEMANTIC_RECALL_MIN)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(SEMANTIC_EACH);
    Ok(scored.into_iter().map(|(_, id)| id).collect())
}

impl Memory {
    /// Embed up to `limit` active items that have no vector from `model`
    /// yet (all of them after a model change). Returns how many were added;
    /// an error is also kept for [`Memory::semantic_status`].
    pub fn embed_missing(
        &self,
        model: &str,
        embed: impl Fn(&[String]) -> Result<Vec<Vec<f32>>, String>,
        limit: usize,
    ) -> Result<usize, String> {
        let todo = self
            .with(|store| {
                if store.kv(MODEL_KEY)?.as_deref() != Some(model) {
                    store.clear_vectors()?;
                    store.set_kv(MODEL_KEY, model)?;
                }
                store.unembedded(limit)
            })
            .map_err(|e| e.to_string())?;
        if todo.is_empty() {
            return Ok(0);
        }
        let inputs: Vec<String> = todo
            .iter()
            .map(|item| for_document(model, &text_of(item)))
            .collect();
        let vectors = match embed(&inputs) {
            Ok(vectors) if vectors.len() == todo.len() => vectors,
            Ok(vectors) => {
                let error = format!(
                    "返回了 {} 个向量，送去的是 {} 段",
                    vectors.len(),
                    todo.len()
                );
                let _ = self.with(|store| store.set_kv(ERROR_KEY, &error));
                return Err(error);
            }
            Err(error) => {
                let _ = self.with(|store| store.set_kv(ERROR_KEY, &error));
                return Err(error);
            }
        };
        self.with(|store| {
            for (item, vector) in todo.iter().zip(vectors) {
                store.put_vector(&item.id, &unit(vector))?;
            }
            store.set_kv(ERROR_KEY, "")
        })
        .map_err(|e| e.to_string())?;
        Ok(todo.len())
    }

    /// How far the table is (设置里那一行).
    pub fn semantic_status(&self) -> Result<SemanticStatus, MemoryError> {
        self.with(|store| {
            let (embedded, missing) = store.vector_counts()?;
            Ok(SemanticStatus {
                model: store.kv(MODEL_KEY)?.unwrap_or_default(),
                embedded,
                missing,
                error: store.kv(ERROR_KEY)?.filter(|e| !e.is_empty()),
            })
        })
    }

    /// The vectors of a scope's active items (`None`: global), by id.
    pub fn vectors(&self, project_id: Option<&str>) -> HashMap<String, Vec<f32>> {
        self.with(|store| store.vectors(project_id))
            .unwrap_or_default()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use aeroric_protocol::memory::{MemoryDraft, MemoryKind};

    use super::*;
    use crate::memory::evidence;

    fn rig() -> Memory {
        let dir = std::env::temp_dir().join(format!("aeroric-semantic-{}", uuid::Uuid::new_v4()));
        let memory = Memory::default();
        memory.set_dir(dir).unwrap();
        memory.set_project_enabled("p1", true).unwrap();
        memory
    }

    fn fact(memory: &Memory, text: &str) -> String {
        memory
            .remember(
                MemoryDraft {
                    project_id: Some("p1".into()),
                    kind: MemoryKind::Fact,
                    category: None,
                    text: text.into(),
                    supersedes: None,
                },
                evidence(None, None, None, text, 1),
                1,
            )
            .unwrap()
            .id
    }

    /// A toy model: one axis per topic word, so "related" is checkable.
    fn toy(inputs: &[String]) -> Result<Vec<Vec<f32>>, String> {
        Ok(inputs
            .iter()
            .map(|t| {
                vec![
                    f32::from(u8::from(
                        t.contains("缩进") || t.contains("空格") || t.contains("排版"),
                    )),
                    f32::from(u8::from(t.contains("发布") || t.contains("上线"))),
                    0.1,
                ]
            })
            .collect())
    }

    #[test]
    fn fusion_rewards_agreement_and_vectors_find_what_words_miss() {
        let score = fuse(&[vec!["a".into(), "b".into()], vec!["b".into(), "c".into()]]);
        assert!(score["b"] > score["a"] && score["a"] > score["c"]);

        let memory = rig();
        let spaces = fact(&memory, "Python 代码缩进用 4 个空格");
        let ship = fact(&memory, "发布前先跑 native:check");
        assert_eq!(memory.embed_missing("toy", toy, 100).unwrap(), 2);
        assert_eq!(
            memory.embed_missing("toy", toy, 100).unwrap(),
            0,
            "done once"
        );
        let status = memory.semantic_status().unwrap();
        assert_eq!(
            (status.model.as_str(), status.embedded, status.missing),
            ("toy", 2, 0)
        );

        // No shared word with the item: FTS finds nothing, the vector does.
        assert!(memory.recall("p1", "排版规则").unwrap().is_empty());
        let query = unit(toy(&["排版规则".into()]).unwrap().remove(0));
        let found = memory.recall_with("p1", "排版规则", Some(&query)).unwrap();
        assert_eq!(
            found.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            vec![spaces]
        );
        let query = unit(toy(&["上线 流程".into()]).unwrap().remove(0));
        let found = memory.recall_with("p1", "上线 流程", Some(&query)).unwrap();
        assert_eq!(found[0].id, ship);

        // Another model: the table starts over.
        assert_eq!(memory.embed_missing("toy-2", toy, 1).unwrap(), 1);
        assert_eq!(memory.semantic_status().unwrap().missing, 1);
        let failed = memory.embed_missing("toy-2", |_| Err("连不上".into()), 5);
        assert!(failed.is_err());
        assert_eq!(
            memory.semantic_status().unwrap().error.as_deref(),
            Some("连不上")
        );
    }

    #[test]
    fn nomic_gets_its_task_prefixes() {
        assert_eq!(for_query("nomic-embed-text", "x"), "search_query: x");
        assert_eq!(
            for_document("nomic-embed-text:v1.5", "x"),
            "search_document: x"
        );
        assert_eq!(for_query("bge-m3", "x"), "x");
        let v = unit(vec![3.0, 4.0]);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&v, &[1.0]), 0.0, "different models never match");
    }

    /// The before/after eval (B6, 2026-11-14), against a real model:
    /// `AERORIC_EMBED_EVAL=http://127.0.0.1:11434 cargo test -p aeroric-core
    /// --lib memory::semantic -- --ignored --nocapture`. 24 memories, each
    /// asked for in other words; 6 questions about nothing stored; 6 pairs
    /// of the same rule said twice for a dream.
    #[test]
    #[ignore]
    fn eval_with_a_real_model() {
        use crate::notebook::rag::embed::{self, EmbedConfig, EmbedProvider};
        let Ok(base) = std::env::var("AERORIC_EMBED_EVAL") else {
            return;
        };
        let model = std::env::var("AERORIC_EMBED_MODEL").unwrap_or("nomic-embed-text".into());
        let config = EmbedConfig {
            provider: EmbedProvider::Ollama,
            base_url: base,
            model: model.clone(),
            api_key: String::new(),
            proxy: Default::default(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let embed = |inputs: &[String]| -> Result<Vec<Vec<f32>>, String> {
            let mut out = Vec::new();
            for range in embed::batches(config.provider, inputs.len()) {
                out.extend(
                    runtime
                        .block_on(embed::embed_batch(&config, &inputs[range], &|| false))
                        .map_err(|e| e.to_string())?,
                );
            }
            Ok(out)
        };
        let pairs: [(&str, &str); 24] = [
            (
                "Python 代码缩进用 4 个空格，不用 tab",
                "排版时每一级往里推几格",
            ),
            ("发布前先跑 native:check sessions", "上线之前要做哪些检查"),
            (
                "训练 YOLO 时 batch size 设成 16，显存 24G 刚好",
                "一次喂给检测网络多少张图合适",
            ),
            (
                "数据集放在 /data/coco 下，标签是 YOLO TXT 格式",
                "图片和标注文件存在哪个目录",
            ),
            ("回复一律用简体中文", "回答问题时用什么语言"),
            (
                "SSH 连接超时改用 ConnectTimeout=10",
                "远程登录老是卡住等很久怎么办",
            ),
            ("不要自动提交代码，提交前先问我", "可以直接 commit 吗"),
            (
                "推理部署在 RK3588 上，用 RKNN 转换模型",
                "边缘板子上怎么跑模型",
            ),
            ("导出 ONNX 时 opset 用 12", "转中间格式选哪个算子版本"),
            (
                "验证集按 8:2 随机划分，固定随机种子 42",
                "怎么切分训练和评估的样本",
            ),
            (
                "图片增强只用水平翻转和 Mosaic，不用旋转",
                "做数据扩充时哪些变换可以开",
            ),
            ("日志统一写到 logs/ 目录，按天轮转", "运行记录输出到哪里"),
            ("Swift 界面按钮一律用胶囊样式", "按键做成什么外形"),
            (
                "单元测试用 pytest，跑之前先激活 conda 环境 cv",
                "怎么执行测试用例",
            ),
            (
                "LoRA 微调 Qwen 时 rank 设 16，学习率 2e-4",
                "大模型低秩适配的超参数",
            ),
            (
                "git 推送要走代理 127.0.0.1:7890",
                "往远端仓库上传代码连不上",
            ),
            ("不要删除数据集和权重文件", "清理磁盘时哪些东西不能动"),
            (
                "macOS 上编译 Rust 前先 ulimit -n 10240",
                "构建时报打开文件太多",
            ),
            (
                "评测指标看 mAP50-95，不只看 mAP50",
                "检测模型好坏主要看哪个分数",
            ),
            (
                "TensorRT 推理用 FP16，INT8 精度掉太多",
                "GPU 加速时用哪种精度",
            ),
            ("代码注释只写不明显的业务规则", "什么时候需要在代码里加说明"),
            (
                "关键点任务的标签格式是 cls cx cy w h 加每个点 x y v",
                "姿态估计的标注每行怎么写",
            ),
            ("视频抽帧每秒取 2 帧", "从录像里截图的频率"),
            (
                "改动前先读子项目的 AGENTS.md",
                "动手修改一个子工程之前要看什么",
            ),
        ];
        let nothing = [
            "今天晚饭吃什么",
            "推荐一部科幻电影",
            "怎么给猫剪指甲",
            "周末去哪里爬山",
            "如何学弹吉他",
            "明天会下雨吗",
        ];
        let memory = rig();
        let ids: Vec<String> = pairs.iter().map(|(text, _)| fact(&memory, text)).collect();
        memory.embed_missing(&model, embed, 500).unwrap();
        let ask = |q: &str| -> Vec<String> {
            let v = unit(embed(&[for_query(&model, q)]).unwrap().remove(0));
            memory
                .recall_with("p1", q, Some(&v))
                .unwrap()
                .into_iter()
                .map(|i| i.id)
                .collect()
        };
        let words = |q: &str| -> Vec<String> {
            memory
                .recall("p1", q)
                .unwrap()
                .into_iter()
                .map(|i| i.id)
                .collect()
        };
        let (mut fts1, mut fts3, mut hyb1, mut hyb3, mut fts_n, mut hyb_n) = (0, 0, 0, 0, 0, 0);
        for ((_, query), id) in pairs.iter().zip(&ids) {
            let a = words(query);
            let b = ask(query);
            fts1 += usize::from(a.first() == Some(id));
            fts3 += usize::from(a.iter().take(3).any(|x| x == id));
            hyb1 += usize::from(b.first() == Some(id));
            hyb3 += usize::from(b.iter().take(3).any(|x| x == id));
            fts_n += a.len();
            hyb_n += b.len();
            eprintln!(
                "{query}: words {} hybrid {}",
                a.iter()
                    .position(|x| x == id)
                    .map_or("-".into(), |p| (p + 1).to_string()),
                b.iter()
                    .position(|x| x == id)
                    .map_or("-".into(), |p| (p + 1).to_string())
            );
        }
        let vectors = memory.vectors(Some("p1"));
        let qv = |q: &str| unit(embed(&[for_query(&model, q)]).unwrap().remove(0));
        let mut hits: Vec<f32> = pairs
            .iter()
            .zip(&ids)
            .map(|((_, q), id)| cosine(&qv(q), &vectors[id]))
            .collect();
        hits.sort_by(|a, b| b.total_cmp(a));
        let mut misses: Vec<f32> = nothing
            .iter()
            .map(|q| {
                let v = qv(q);
                ids.iter()
                    .map(|id| cosine(&v, &vectors[id]))
                    .fold(f32::MIN, f32::max)
            })
            .collect();
        misses.sort_by(|a, b| b.total_cmp(a));
        eprintln!("COS query→its memory {hits:.2?}");
        eprintln!("COS off-topic→best memory {misses:.2?}");
        let noise_fts: usize = nothing.iter().map(|q| words(q).len()).sum();
        let noise_hyb: usize = nothing.iter().map(|q| ask(q).len()).sum();
        eprintln!("RECALL model={model} n=24 words top1={fts1} top3={fts3} avg_hits={:.1} | hybrid top1={hyb1} top3={hyb3} avg_hits={:.1}", fts_n as f64 / 24.0, hyb_n as f64 / 24.0);
        eprintln!("NOISE 6 off-topic questions: words {noise_fts} hits, hybrid {noise_hyb} hits");

        let twins = [
            (0, "缩进统一四个空格"),
            (4, "所有回答都要中文简体"),
            (6, "commit 之前必须征得我同意"),
            (12, "界面上的按钮做成圆角胶囊形"),
            (15, "push 到远程仓库时设置 http 代理端口 7890"),
            (16, "权重和数据集文件不许删"),
        ];
        let twin_ids: Vec<(String, String)> = twins
            .iter()
            .map(|(i, text)| (ids[*i].clone(), fact(&memory, text)))
            .collect();
        memory.embed_missing(&model, embed, 500).unwrap();
        let items = memory.list(Some("p1"), None).unwrap();
        let score = |groups: Vec<Vec<MemoryItem>>| {
            let found = twin_ids
                .iter()
                .filter(|(a, b)| {
                    groups
                        .iter()
                        .any(|g| g.iter().any(|i| &i.id == a) && g.iter().any(|i| &i.id == b))
                })
                .count();
            let members: usize = groups.iter().map(Vec::len).sum();
            (found, groups.len(), members)
        };
        let plain = score(crate::memory::dream::groups(&items, &Default::default()));
        let near = score(crate::memory::dream::groups_with(
            &items,
            &Default::default(),
            &memory.vectors(Some("p1")),
        ));
        eprintln!("DREAM 6 twins: words found {} in {} groups ({} items) | vectors found {} in {} groups ({} items)", plain.0, plain.1, plain.2, near.0, near.1, near.2);
        let mut cos: Vec<f32> = Vec::new();
        let vectors = memory.vectors(Some("p1"));
        for (a, b) in &twin_ids {
            cos.push(cosine(&vectors[a], &vectors[b]));
        }
        eprintln!("DREAM twin cosines {cos:?}");
        let mut unrelated: Vec<f32> = Vec::new();
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                unrelated.push(cosine(&vectors[&ids[i]], &vectors[&ids[j]]));
            }
        }
        unrelated.sort_by(|a, b| b.total_cmp(a));
        eprintln!(
            "DREAM highest cosines between different rules {:?}",
            &unrelated[..5]
        );
    }
}
