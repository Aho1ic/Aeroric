# Agent Note: A5 客户端的方法名以宿主为准

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A5 原题是"随手记 RAG 搜索接通"。批次笔记的判断是：Swift 调 `notebook.rag.stats/search/neighbors`，宿主注册的是 `notebook.rag_stats/rag_search/rag_neighbors`；工作区建 `RAGSearchView()` 时没传搜索回调。要做的是：改方法名、接上回调、加一条方法名检查。

2026-11-17 在 `main@1e682f44` 加未提交的 A3、A4 等改动上复核，情况和批次笔记的设想不同：

- **漂移不止 RAG 三个。** 把 `macos/AeroricKit/Sources` 和 `ios/AeroricCompanionKit/Sources` 里所有 `request("…")`、`call("…")` 的方法名字面量，和 `core/crates/*/src` 里出现过的带点字符串对照，有 17 个对不上，全在 `AeroricClient/NotebookClient.swift`。Swift 写的是 `notebook.<模块>.<动作>`，宿主注册的是 `notebook.<模块>_<动作>`：
  - RAG：`rag.stats`、`rag.search`、`rag.neighbors`
  - 同步：`sync.bind`、`sync.unbind`、`sync.remotes`、`sync.run`、`sync.set_auto`、`sync.resolve`、`sync.resolutions`、`sync.status`，以及 `git.sync`
  - 存储连接：`storage.save_connection`、`storage.delete_connection`
  - 嵌入密钥：`embedding_key.set`、`embedding_key.status`、`embedding_key.clear`
- **这些方法从来没有被调用过。** App 里没有任何地方创建 `NotebookClient`。工作区的"笔记"一节（`TaskWorkspaceView.swift` 的 `.notebook`）从 2026-10-30 起就是故意隐藏的：`listed` 为假，状态是 `comingSoon("项目笔记、同步与知识检索。")`，注释写着"建好之前不显示"。`NotebookStore` 的 `ingestVaults` 也没有人调用。所以批次笔记里"用户会看到：随手记的语义搜索能出结果"不成立：用户根本进不到这一页。
- **批次笔记提出的检查依据不够。** 它要求方法名必须出现在 `contracts/native/v1/methods.schema.json` 或 `docs/native/command-map.json` 里。实测 Swift 客户端用到的方法有 238 个不在这两个文件里（会话、委派、讨论、记忆、导入等现行方法都在其中），这两个文件只覆盖了一部分方法，拿它们做依据会全面误报。
- **宿主运行时的方法表也不适合做依据。** `ServiceSet::is_registered` 能查，但维护、远程等方法只在特定装配下注册，测试装配和生产装配不同，会误报。

## Decision

**改名，加一条静态检查；页面接线等笔记页真正要做时再做。**

### 改名

- `NotebookClient.swift` 里 17 个方法名改成宿主的写法（第一个点之后的点换成下划线）。宿主是权威，不改宿主。
- 逐个核对了参数和返回：
  - 参数键名一致：`vault`、`remoteId`、`connectionId`、`root`、`strategy`、`enabled`、`path`、`localHash`、`remoteHash`、`resolution`、`remoteIds`、`key`、`connection`。
  - 存储连接对象带了宿主要求的 `createdAt`（传 0，宿主更新时沿用旧值）。
  - 返回：`remotes`、`status`、`resolutions`、`neighbors` 的字段和宿主的 camelCase 序列化一致（`SyncRemote`、`RemoteStatus`）。
  - RAG 的 `rag_search` 仍要求调用方传 `config`（嵌入配置），返回 `hits`、`degraded`、`vectorsMissing`。

### 检查（`AeroricClientTests/ClientMethodNamesTests.swift`，进 `swift test`）

- 读 `macos/AeroricKit/Sources` 和 `ios/AeroricCompanionKit/Sources` 下所有 `.swift`，取 `request(`、`call(`、`requestRaw(`、`stream(`、`subscribe(` 后面第一个字符串字面量里带点的方法名。插值的名字（如 `"discussion.\(action)"`）跳过。
- 读 `core/crates/*/src` 下所有 `.rs` 里带点的字符串字面量（`register("…")`、`service_ctx!("…")`、`*_METHOD` 常量都在其中）。
- 每个 Swift 方法名都必须出现在 Rust 那一侧；对不上的列出"方法名（文件名）"。
- 防止目录挪动后静默通过：Rust 侧少于 500 个、Swift 侧少于 250 个就失败（现在分别是 1231 和 291），并且必须找得到 `notebook.rag_search`。
- 它只查"宿主有没有这样拼的名字"，不查参数形状、也不查这个名字在当前装配下是否注册。

### 不做：搜索视图接线

- 批次笔记的第 2 步（`RAGSearchView` 接上回调）这次不做。笔记页隐藏、App 没有 `NotebookClient`、没有加载库，接上回调既用不到也测不到。
- 等笔记页要上线时，接线要一起做：在 App 里建 `NotebookClient`、加载库、`RAGSearchView` 的回调调 `ragSearch`、嵌入配置用宿主保存的随手记嵌入设置（记忆的向量已经用 `settings::notebook_embed_config()` 读同一份设置）。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 原地补 A5 的复核结论和范围变化；A5 状态回写路线文档 §8 |
| [落地总纲](../../proposed/architecture/2026-11-15-research-adoption-roadmap.md) | 上级方案 | 原地在 §2 那一行补一句：笔记页本来隐藏，漂移共 17 个，已改 |
| [A4](./2026-11-17-a4-discussion-read-only-must-be-enforced.md) | 无关 | 不动 |

## Alternatives considered

**照批次笔记，用 `methods.schema.json` 和 `command-map.json` 做依据。** 最强的理由是契约文件是正式的接口说明，检查依据清楚。没选：这两个文件只覆盖一部分方法，现在就有 238 个现行方法不在里面；要用它们，得先把契约补全，那是另一件事。

**在宿主测试里组装完整的宿主，对每个 Swift 方法名查 `is_registered`。** 最强的理由是查的是真正会被分发的方法，比"源码里出现过这个字符串"更严格。没选：部分方法按装配有条件注册（维护面、远程、终端重放），测试装配和生产不同，会误报，要维护一张例外表。静态对照已经能抓住这次这类拼写漂移。

**顺带把笔记页做出来，让 RAG 搜索真能用。** 最强的理由是 A5 的原意是"用户能搜"。没选：笔记页隐藏是 2026-10-30 定的，做出来要接库列表、编辑、同步、嵌入设置，是一个功能批次，不是 A 批这种"把假状态改成真状态"的小修。

**改宿主的方法名去迁就 Swift。** 最强的理由是 Swift 的点分写法和会话、委派等方法风格更一致。没选：宿主是权威；宿主测试 `notebook_rag_services.rs` 已经按下划线写法验过这些方法，改宿主要连带改它们，而 Swift 这一侧的调用从来没有成功过，改它没有兼容代价。

## Consequences

- **收益**：Swift 客户端调用的每个方法名，宿主那边都有同样拼写的字符串，以后再出现这类漂移，`swift test` 会直接列出来。随手记的同步、存储连接、嵌入密钥、RAG 调用不再必然报"方法不存在"。
- **代价**：
  - 检查是"出现过这个字符串"，宿主里恰好有同名的普通字符串也会让它通过；参数形状不在检查范围内。
  - 随手记这一页仍然隐藏，RAG 搜索在界面上仍然用不到。
- **验证**：
  - 先用脚本对照，改名前抓到 17 个，改名后 0 个。
  - 2026-11-17 在 Mac 上：`swift test --package-path macos/AeroricKit` 960 + 32 + 110 个用例 0 失败（1 个跳过），新检查通过。
  - 反向验证：临时把 `notebook.rag_stats` 改回 `notebook.rag.stats`，检查失败并列出"notebook.rag.stats（NotebookClient.swift）"；随后改回。
  - 这次没改 Rust，没有跑 cargo。
  - 没有界面可目检（笔记页隐藏）；路线文档 §8 待目检清单记了一行说明。
