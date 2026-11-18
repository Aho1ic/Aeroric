# Agent Note: A8 缓存断点连同已有的一起不超过 4 个

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A8。2026-11-17 复核 `core/crates/aeroric-core/src/local_router/cache_injector.rs`（未改过）：

- 本机路由开了"缓存注入"时，`inject` 会给 Claude 请求加 `cache_control: {"type": "ephemeral"}` 标记，最多加 4 个：
  - tools 的最后一个；
  - system 的最后一块；
  - 最后一条消息的最后一个非思考块；
  - 倒数第二条用户消息的最后一块。
- `MAX_BREAKPOINTS` 只和本次新加的个数比，没有算请求里本来就有的标记。Claude Code 自己会打标记，请求里已有 2 个时，最多还会加 4 个，一共 6 个。Anthropic 一个请求最多接受 4 个缓存断点，超过就拒绝整个请求。
- 预算是 0 时也会改请求体：字符串形式的 `system` 和消息 `content` 会被改写成数组，虽然最后没加标记。

## Decision

**注入前先按结构数已有的标记，额度 = 4 − 已有数，用完即停；一个都不能加时不碰请求体。**

- `existing_markers(body)` 只数 Anthropic 允许放标记的那几层，不往下递归：
  - `tools[*]`；
  - `system`（数组的各块，或对象形式的单块）；
  - `messages[*].content[*]`；
  - `tool_result` 块里 `content[*]` 那一层。

  工具的 `input_schema`、`tool_use.input` 里可能有名叫 `cache_control` 的字段，那不是标记，不往里数。
- 位置和先后不变（tools → system → 最后一条消息 → 倒数第二条用户消息），每加一个扣一个额度，扣完就返回。
- 已有标记正好在要加的位置上时，照旧不加。它已经算在已有数里，不会重复扣额度。
- 返回值从"新加了几个"改成 `Injected { existing, added }`。路由在请求里本来就有标记时，往宿主日志写一行 `[router] 缓存断点：请求里已有 N 个，新加 M 个`；没有已有标记时不写，免得每个请求都记一行。
- 已有数超过 4 时什么都不加，也不删调用方自己的标记：那是调用方发来的原样，上游拒绝的原因不在路由。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 原地补 A8 的落地情况；A8 状态回写路线文档 §8 |
| [调研落地路线](../../proposed/architecture/2026-11-15-research-adoption-roadmap.md) | 只提到这一片 | 不动 |

## Alternatives considered

**递归数请求体里所有叫 `cache_control` 的键。** 最强的理由是不用记 Anthropic 允许放在哪几层。没选：工具定义的 JSON Schema 和工具调用参数里完全可能有同名字段，会多数；多数虽然只是少加标记，但会让正常请求少了缓存。按结构数，和上游数的是同一批。

**已有超过 4 个时删掉多出来的。** 最强的理由是请求能发出去。没选：标记是调用方自己打的，删哪个都在替它决定缓存策略；路由只对自己加的负责。

**额度不够时改优先级（先保最后一条消息）。** 最强的理由是对多轮对话，最后一条消息的断点最值钱。没选：顺序沿用 CC Switch 的做法，这片只修"超出上限"，不改策略。

## Consequences

- **收益**：带着自定义缓存标记的请求，不再因为路由多加标记被上游拒绝；不能加时请求体原样转发。
- **代价**：数的位置是写死的；Anthropic 以后允许在别处放标记时，这里要跟着加。
- **验证**：
  - `cache_injector` 单测：
    - 已有 0、2、4、5 个标记：新加 4、2、0、0 个，输出的总数不超过 `max(4, 已有数)`；
    - 已有 4 个以上时请求体原样不动，连字符串 `system` 也不改写；
    - 已有标记正好在要加的位置（tools、最后一条消息）上，不重复加，剩下的额度给 system 和倒数第二条用户消息；
    - `tool_result` 里那一层的标记算数，工具 schema 和 `tool_use.input` 里的同名字段不算；
    - 原有 5 个测试照常通过。
  - 2026-11-17 在 Mac 上：
    - 只格式化本次改动的文件，`cargo clippy -p aeroric-core --all-targets -D warnings` 通过；
    - `local_router` 测试 178 个通过；
    - `cargo test --workspace --no-fail-fast` 3752 通过、0 失败；
    - `pnpm verify-notes` 通过。
  - 没有对真实上游发请求；日志那一行记在路线文档 §8 待目检清单。
- **2026-11-18 补强**：空文本不打标记、请求顶层的 `cache_control` 计入名额（B1-6），见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。
