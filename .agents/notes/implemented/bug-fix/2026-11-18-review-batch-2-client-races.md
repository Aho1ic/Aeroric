# Agent Note: 全量审查 Batch 2——会话页与新会话页的读写竞态

Status: implemented

## Problem

2026-11-17 全量审查交接单（`.local_docs/2026-11-17-project-review-handoff.md`，本地）的 Batch 2 列出 macOS 客户端的并发与协议对齐问题。本篇记已经改完的几条，复核都成立：

- **B2-1**：
  - `loadSessionConfigurations` 在读取进行中时直接返回。新会话页的 `.task(id:)` 换 id 时，SwiftUI 取消旧任务、立刻起新任务，新任务因此被丢掉；
  - 被取消的旧任务在 `catch` 里把状态写成 `.failed`，新会话页就被锁住。
- **B2-2**：`LiveSessionStore` 有四处问题：
  - `answering` 只在失败时移除。同一会话里问题 id 复用（`q1`）时，新卡片永远点不了；
  - 菜单"重新读取"调 `open()`，后台每 50 ms 调一次 `poll()`。`poll()` 挂起前拷贝的旧 model 会在 `open()` 之后写回去；
  - "加载更早的消息"读失败时，页数已经加上去不退回；历史读取也没有先后保护；
  - `refreshReruns()` 每调一次就多一条每 5 秒的轮询链。
- **B2-3**：`SessionRouteView.load()` 和 `ImportedTranscript.load()` 各自起一个不受管的 Task。慢的旧响应会覆盖新的，旧任务的 `defer` 还会把 `loading` 提前置成 false。
- **B2-6**：
  - `LiveDelegateAgent.named` 不解码 `agentConfigId`，编辑委派角色再保存时会把绑定的配置丢掉；
  - 回合记录不解码 `check` 和 `stateBeforeUndo`，投递回执丢掉 `reason`；
  - 更新里的问题卡没有 `askedAtMs` 时记成 0，倒计时从 1970 年算起；
  - 会话页的升级胶囊在排队时也写"正在升级"。
- **B2-4**：App 只拉 `remote: false` 的能力表，SSH 项目也用它，DSH、只读讨论成员、委派和审查在远程项目上被误放行。
- **B2-5**：
  - 讨论没有"接力"格式的文案，`round` 是发言人次、`maxRounds` 是圈数，于是显示成"第 6 / 2 轮"；
  - 讨论成员按 `agent` 而不是 `harness` 分组，`claude_gpt55` 这类衍生配置和自定义配置找不到自己的 Harness。
- **B2-7**：
  - 待办的保存、删除没有防重入，连点"保存"会建出两条；
  - 升级簿换宿主时不停旧的轮询，也不清排队待发的消息；同一会话连点"发送"会排两条；
  - 字体下载的 `onCancel` 和建任务之间有数据竞争，取消早于建任务时会丢，取消后的下载仍可能装上。

## Decision

- **B2-1**：
  - 去掉"进行中就返回"，每次读取都发新的 `read`，旧的回复由 `configurationReads[projectID] == read` 丢弃；
  - 错误怎么落成状态，抽到 `WorkspaceReadState.after(_:hasEarlierRead:)`：取消（`CancellationError`、`ConnectionError.cancelled`、`RequestResolution.cancelled`，或者任务本身已取消）不算失败——读到过就是 `.loaded`，没读到过就是 `.notLoaded`；其余错误照旧 `.failed`。
- **B2-2**：
  - `poll()` 结算时，把 `answering` 裁成宿主还列着的问题。只在成功后移除会让卡片在宿主处理完之前就能再点，所以按宿主的问题列表裁；
  - 加 `syncGeneration`，`open()`、`resync()` 各加一。`poll()` 等到回复后，代数变了就丢掉这次读到的内容；
  - `reloadHistory()` 改为返回是否落地，每次读取取一个序号，只有最新的那次写回；`loadEarlierHistory()` 失败时页数退回；
  - 重跑只留一个轮询任务 `rerunPollTask`，已有就不再起；任务弱引用 store，store 释放后自行结束；另有 `stopFollowingReruns()`。
- **B2-3**：新增 `HistoryReads`。每次读取先取消上一个任务、取一个新序号，只有最新序号的回复或错误会写回，被取消的不报错，只有最新那次读完才把 `loading` 置回 false。"加载更早的消息"失败时页数退回到点之前。两个视图共用这一份逻辑。
- **B2-6**：
  - `LiveDelegateAgent.named(agent:agentConfigId:model:)` 双向编解码，写法与 `LiveReviewerChoice` 一致；
  - `LiveTurnRecord` 加 `stateBeforeUndo`、`check`（`LiveTurnCheck`）；`deliveryReceipt` 带 `reason`；
  - `LiveSessionUpdate.decode` 给没有 `askedAtMs` 的问题补上更新的 `atMs`；
  - 升级胶囊新增 `.waiting` 状态，沙漏加"排队升级：等 X 装完"。
- **B2-4**：
  - 新增 `CapabilityPlace`（`ssh://` 开头的项目路径就是远程），App 同时缓存本机和远程两套表；
  - 新会话页按项目所在位置取表；
  - 会话页通过 `capabilities(taskID:family:)` 按任务所属项目带上 `remote`。协议里给这个方法加了默认实现，其余实现不用改。
- **B2-5**：
  - "接力"的标题、状态写成"第 2 / 2 圈 · 第 2 位"，轮次标题加"开题 / 接着说"；
  - `RerunTarget` 加 `harness`，分组、能力查询、"没有能启动的配置"都按 `harnessKey`（没有 harness 时退回 agent）。
- **B2-7**：
  - `TodoStore.writing` 按待办 id 防重入，编辑框保存中按钮置灰并显示"保存中…"；
  - `AgentUpgradeBook.attach` 停掉旧轮询、清空排队；`whenUpgraded` 按会话（key）去重，后一次替换前一次，并带上 `afterTurn`；
  - `DownloadWatcher` 用锁保护任务、continuation 和取消标志：取消早于开始时直接以取消结束，不再启动下载；
  - 每次开始下载发一个令牌，取消或被替换后旧任务不再写状态；下载完成后若已取消，就删掉压缩包、不安装。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [A6 历史分页](./2026-11-17-a6-earlier-history-and-unreadable-lines.md) | 被本篇补强 | 结论不变；"加载更早"失败退回页数在本篇 |

## Alternatives considered

**B2-1：在视图里用 `.task` 的取消，不在 AppServices 里处理。** 最强的理由是改动只在一处。没选：取消在 SwiftUI 侧发生，错误却在服务里落成状态，只有服务知道这次读取是不是最新的那次。

**B2-2：`answer()` 成功后立刻移除 `answering`。** 最强的理由是最简单，交接单也这样建议。没选：成功只表示请求送到了宿主，问题要等宿主那边处理完才算结束；中间这段时间卡片应该保持不可点。按宿主列出的问题来裁，两件事都能做到。

**B2-3：给每个视图各写一套序号。** 最强的理由是不用多一个类型。没选：两处逻辑完全一样，抽成 `HistoryReads` 后也能单独测试。

## Consequences

- **收益**：
  - 新会话页不会因为切项目时的取消而锁住；
  - 会话页不会被旧快照、旧历史倒回去；
  - 同一会话里复用的问题 id 能再回答；
  - 重跑的轮询不会越叠越多。
- **代价**：远程能力表多拉一轮，宿主启动后读一次。
- **验证**：
  - Swift：`WorkspaceReadStateTests`（两条）；
  - `LiveSessionStoreTests` 新增三条：问题 id 复用、`open` 期间的慢 `poll`、"加载更早"失败退回页数；
  - `HistoryReadsTests`、`LiveDelegateAgentTests`（含回执原因、问题时间、回合记录、能力表按位置选取）；
  - `DiscussionTests` 两条（接力文案、按 Harness 分组）；
  - `AgentUpgradeTests` 两条（排队文案、按会话去重与换宿主清空）；
  - `TodoTests` 一条（连点保存只建一条），`FontDownloadsTests` 一条（开始前取消不丢）；
  - 契约测试里问题卡的期望改为带更新时间。
  - 2026-11-18 在 Mac 上：
    - `swift test --package-path macos/AeroricKit`：974 个（跳过 1 个）、36 个、110 个，0 失败。第一次跑时问题 id 复用的测试失败：同一批里先结算、再出现同 id 的新问题，只按问题列表裁剪会漏掉。改成同时去掉这一批里已结算的 id 后通过；
    - App 在临时 DerivedData 里构建成功。
  - 界面没有目检，记在路线文档 §8 待目检清单。
