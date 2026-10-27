# docs/native：门禁与生成器读取的机读工件

本目录只放被代码或门禁读取的文件。人读的现状文档在 `docs/` 其它目录，决策在 `.agents/notes/`。

| 文件 | 读者 | 内容 |
|---|---|---|
| `command-map.json` | `export-schema`、`doctor.mjs`、`digest.mjs` | 冻结的命令清单与 `meta.rpc_resolution`，方法面的来源 |
| `command-map.summary.json` | `digest.mjs` | 命令清单摘要 |
| `event-topics.json` | `export-schema` | v1 事件 topic 映射表（规则见 [事件 topic 笔记](../../.agents/notes/implemented/architecture/2026-09-22-event-topic-namespace.md)） |
| `feature-matrix.json` | `doctor.mjs`、`digest.mjs`、`parity.mjs` | 产品能力 Fxx 与缺口 Gxx 台账 |
| `acceptance-matrix.json` | `parity.mjs` | 每条能力在各端的证据台账（唯一真源） |
| `acceptance-matrix.md` | 人 | 由台账派生的叙事版 |
| `acceptance-budgets.md` | `stress.mjs` 的对照 | 压测固定口径与预算 |
| `migration-rehearsal.md` | `migration-rehearsal.mjs` 的说明 | 迁移演练脚本回答什么 |
| `digest/` | `pnpm native:digest` 生成 | 供 agent 分片阅读的摘要，勿手改 |
