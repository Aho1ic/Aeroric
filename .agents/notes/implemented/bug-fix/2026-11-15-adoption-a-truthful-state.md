# Agent Note: 调研落地批次 A：状态与回执说真话

Status: implemented

## Problem

[落地总纲](../../proposed/architecture/2026-11-15-research-adoption-roadmap.md) 把 207 项目调研里能在当前源码指到行号的缺口归成批次 A。它们是同一类问题：**界面显示的状态比实际发生的事情走得快，或者把"不知道"算成了"成功"。**

用户会遇到的情形：

- 委派结果卡写着"已发送"，主会话其实没收到。
- 点了批准，Agent 没收到回应，审批卡却消失了。
- Codex 回合异常结束，界面显示"完成"，自动检查照常跑。
- 讨论模式说成员只读，某一族的成员其实能写文件。
- 随手记的 RAG 搜索点了没反应。
- 长会话只显示最近一部分，看不到"还有更早的"。
- 撤销一个回合，没人动过的文件被报成"这一轮之后又改过"。

这些都不需要新的子系统，只需要让现有对象如实反映状态。本篇把它们拆成 11 片，各片互不依赖。

所有位置都是 `main@8d4391c3` 上的。开工前按总纲 §4 第 2 步复核。标"动手前复核"的片，调研给了位置，但这次没有逐行读过。

## Decision

### A1 委派结果卡跟随投递回执

**用户会看到：** 结果卡的发送状态分成"等待发送、已入队、对方已收下、已送达、没送到、结果不明"。没送到时卡上有原因和"再发一次"；结果不明时进"需要我处理"。

**现状：**

- `core/crates/aeroric-core/src/sessions/live/delegation/mod.rs:649–675`（`send`）和 `:1040–1065`（自动回传）先把 `card.sent = true` 写下并发布，再开线程调 `host.prompt`。失败时由 `unsend` 改回（C 批补的），但成功路径上"已发送"只表示入队调用没报错。
- `core/crates/aeroric-host/src/session_live_services/delegation.rs:383–391` 调 `delivery::enqueue_internal`。
- `core/crates/aeroric-host/src/session_live_services/delivery.rs:92–114`：`enqueue_internal` 把入队返回的 `Delivery` 丢掉（`.map(|_| ())`），`source_ref` 传 `None`。
- 投递记录本身已有完整状态和证据（`core/crates/aeroric-protocol/src/delivery.rs`：`DeliveryState`、`DeliveryEvidence`、`source_ref`）。

所以卡片和投递记录是两份状态，卡片那份不会随投递的后续变化更新。

**改法：**

1. `enqueue_internal` 返回 `Delivery`（或其 `id`），并接收 `source_ref`。委派传 `delegation:<parentTaskId>/<delegationId>`。
2. `DelegationCard` 加可选字段 `return_delivery_id`。`sent` 保留，含义改为"已交给投递队列"；界面不再直接读它。
3. 投递队列对同一 `source_ref` 且仍未终结的记录去重：重复入队返回已有记录，不产生第二条。先读 `core/crates/aeroric-core/src/sessions/live/delivery/{mod,store}.rs` 确认现有去重方式，没有就按 `source_ref` 加。
4. 委派服务订阅投递状态变化，按 `source_ref` 找到卡片并重新发布。卡片的发送状态由投递记录派生，不另存一份。
5. `Failed` 时走现有的 `mark_unsent`；`Unknown` 时卡片进"需要我处理"，给"去主会话核对"和"再发一次"（再发生成新的投递记录，旧记录保持 `Unknown`）。
6. Swift：委派卡视图读派生状态；`context.delegations` 的返回里带上发送状态。

**归属：** core 的 delegation 与 delivery；host 的 `delivery.rs`、`delegation.rs`；协议 `delegation.rs`；Swift 委派卡。

**验证输入：** 假 host 和假驱动。入队成功后驱动拒收；接收后断线；宿主在入队后、送达前重启，重启后卡片状态由投递库恢复；连点两次发送只有一条投递；过期事件（旧一代的送达回执不改新一代的卡）。

**完成条件：** 卡片显示"已送达"时，投递记录必有 `Delivered` 及其证据；任何失败路径都不会留下"已发送"的卡。

### A2 审批回应没送出去，不算已处理

**用户会看到：** 点了批准或拒绝后如果没送到，审批卡还在，写着"没送出去"和原因，可以重试。连接已断时写明"这次请求已随连接结束"。

**现状：** `core/crates/aeroric-core/src/sessions/live/claude/driver.rs:627–646`：`respond` 先把请求从 `pending` 里取走，`client.respond` 失败时推一条 `Error`（"审批未送达"），随后**照常**推 `PermissionResolved`。Codex（`codex/driver.rs` 的 `PermissionResolved` 推送点，约 `:685`、`:1024`、`:1122`）和 OMP（`omp/driver.rs` 约 `:781`、`:979`、`:1085`）是同形代码，逐处确认。检查点派生（`checkpoint/derive.rs:190`）和讨论模式（`discussion/mod.rs:924`）都把 `PermissionResolved` 当成事实。

**改法：**

1. 发送成功才推 `PermissionResolved`。
2. 发送失败且连接仍在：请求放回 `pending`，推一条新的更新（建议 `PermissionSendFailed { id, reason }`），界面保留卡片并显示原因。
3. 连接不在（`self.client()` 为空）：不推 `PermissionResolved`；请求随回合结束按现有逻辑清理，卡片显示"请求已随连接结束"。
4. 发送超时、结果说不清：记为不明，进"需要我处理"，不自动重发。
5. 手机端审批走同一条路：`ios/AeroricCompanionKit` 的 `CompanionStore` 目前在本地消费待审批（动手前复核），改成等宿主的结果再移除。

**归属：** 三个驱动；协议 `session_live.rs` 加更新种类；Swift 审批卡与"需要我处理"；iOS 伴随端。

**验证输入：** 故障注入假驱动让 `respond` 返回错误、超时、连接关闭三种情况（选了没发出）；Mac 和手机同时回答同一请求，先到的生效，后到的看到"已由另一端处理"。

**完成条件：** 没有任何路径能在回应未送达时产生 `PermissionResolved`；检查点里的已同意记录只来自送达成功的回应。

### A3 Codex 的未知结束状态不算完成

**用户会看到：** Codex 回合以不认识的状态结束时，回合标"没跑成"，原因里写着原始状态，不触发自动检查和验收。

**现状：** `core/crates/aeroric-core/src/sessions/live/codex/map.rs` 的 `turn/completed` 分支：`interrupted` → `Interrupted`，`failed` → `Failed`，其余（包括字段缺失）→ `Completed`。`TurnEndReason` 只有三个值（`core/crates/aeroric-protocol/src/session_live.rs:51`）。

**改法：**

1. 只有 `completed` 映射为 `Completed`。
2. 其他值和缺失值映射为 `Failed`，回合结束更新里带上原始状态字符串（新增可选字段，例如 `raw_status`），原因写"Codex 返回了没见过的结束状态：<原值>"。
3. 回合分级（R5-1，见 [回合分级](../../implemented/feature/2026-11-11-turn-outcomes.md)）把这种情况归"没跑成"。
4. 同时检查 Claude、OMP 的映射有没有同样的兜底分支，有就一并处理。

不给 `TurnEndReason` 加第四个值：它被很多地方穷举匹配，而"未知"在业务上的处理和"失败"一致（不检查、不验收、可重试）。

**验证输入：** 映射单测覆盖 `completed`、`failed`、`interrupted`、空、`cancelled`、任意新值。

**完成条件：** 只有明确的 `completed` 才能触发 `checks.start`。

### A4 讨论模式的只读要有依据

**用户会看到：** 选讨论成员时，不能保证只读的 Harness 或连接方式是灰的，旁边写着原因。已开始的讨论里每个成员都确实不能写文件。

**现状：** `core/crates/aeroric-host/src/session_live_services/discussion.rs:41` 定义 `READ_ONLY = "plan"`，`:379` 给每个成员任务写这个权限模式。各族实际效果不同：Claude 映射为 `plan`（`claude/map.rs:610`），Codex 映射为只读沙箱策略（`codex/map.rs:430`），OMP 的非测试代码里搜不到 `plan` 的映射（调研称落到"每次都问"），DSH 的权限通道不接受 `plan`（动手前复核）。

**改法：**

1. 能力表加一项 `Feature::StrictReadOnly`（`core/crates/aeroric-protocol/src/live_capability.rs`、`core/crates/aeroric-core/src/sessions/live/capabilities.rs`），按族、连接方式、本机或远端填 `Yes/No/Degraded/Unknown`。`Yes` 的条件是：该族在该路由上有 Harness 自身强制的只读模式，且 Aeroric 有测试证明映射正确。
2. `discussion.start` 对每个成员查这一项；不是 `Yes` 就拒绝该成员并返回原因。
3. OMP：先确认 `omp --mode rpc` 有没有真正的只读模式。有就补映射和测试；没有就填 `No`。
4. 委派的只读角色（`delegation/roles.rs`、`delegation/mod.rs:374–379`）用同一项能力判断，避免两处标准不同。
5. Swift：讨论的成员选择器和委派角色选择器按能力表置灰。

**验证输入：** 假族分别声明 `Yes`、`No`、`Unknown`；讨论里混入一个 `No` 成员时整体不启动、已建的任务被清理；能力表快照测试。

**完成条件：** 讨论和只读委派里不存在"写了 `plan` 但没人强制"的成员。

### A5 随手记 RAG 搜索接通

**用户会看到：** 随手记的语义搜索能出结果，统计和相邻片段可用。

**现状：** `macos/AeroricKit/Sources/AeroricClient/NotebookClient.swift:855–877` 调 `notebook.rag.stats`、`notebook.rag.search`、`notebook.rag.neighbors`；宿主方法表里是 `notebook.rag_stats`、`notebook.rag_search`、`notebook.rag_neighbors`。`macos/AeroricApp/Sources/TaskWorkspaceView.swift:1498` 写的是 `RAGSearchView()`，没有传搜索回调。

**改法：**

1. Swift 方法名改成宿主的名字（宿主是权威）。
2. 工作区创建 `RAGSearchView` 时接上回调。
3. 加一条静态检查：`AeroricClient` 里所有 `connection.request("…")` 的方法名字面量，都必须出现在 `contracts/native/v1/methods.schema.json` 或 `docs/native/command-map.json` 里。放进 `swift test`。这条检查能挡住同类的拼写漂移。

**验证输入：** 方法名检查本身；`NotebookClient` 的编解码单测用宿主测试 `core/crates/aeroric-host/tests/notebook_rag_services.rs` 里的真实返回形状。

**完成条件：** 方法名检查通过且覆盖全部客户端；搜索入口从视图到宿主的调用链完整。界面效果需目检。

### A6 历史能翻到更早，读不出的行有提示

**用户会看到：** 长会话顶部有"加载更早的消息"。历史文件里有读不出来的行时，会话信息里写"有 N 行读不出来（第一处在第 X 行）"。

**现状：** `macos/AeroricKit/Sources/AeroricClient/SessionsClient.swift:79–93` 的 `fullHistory` 最多取 24 页。两个调用点（`TaskWorkspaceView.swift:1705`、`LiveSessions.swift:38`）只用了消息和统计，没有用剩余的分页状态（动手前复核）。`core/crates/aeroric-core/src/sessions/claude.rs:31–34` 解析失败直接 `return`，没有计数。

**改法：**

1. 两个调用点保留 `hasMore` 和 `nextCursor`，视图在顶部提供继续加载。
2. 各族历史解析函数返回"跳过了几行、第一处在哪"；`SessionPage` 的统计里加可选的 `skipped_lines`、`first_skipped_line`。Codex、OMP 的历史解析同样处理。
3. 导入路径同样把计数带到导入结果里。

**验证输入：** 超过 24 页的假历史；中间夹坏行、截断的最后一行、空行（分页不全）。

**完成条件：** 任何会话都能翻到第一条消息；坏行不再静默消失。

### A7 回合撤销：已是目标内容的不算冲突，写回不留半截文件

**用户会看到：** 撤销预览里，内容已经和目标一致的文件归到"已是撤销后的样子"，不再报"这一轮之后又改过"。撤销中途被打断，文件要么是旧内容要么是新内容。

**现状：** `core/crates/aeroric-core/src/sessions/live/turn_ledger.rs:200–218`：当前内容等于预期内容才计划写回，否则一律 `SKIP_CHANGED`，包括"当前内容已经等于目标"这种情况。`:458` 用 `std::fs::write` 原地覆盖。同文件 `:343–344` 已有"写临时文件再改名"的做法可参照。

**改法：**

1. 计划阶段加第三类：当前内容等于目标内容时，记为"已到位"，不写，不算跳过。协议 `turn_ledger.rs` 的计划类型加对应列表。
2. 写回改为同目录临时文件加 `rename`，保留权限位；符号链接分支保持现有做法。
3. 多文件撤销中途失败时，结果里逐文件写明已写、未写和失败原因，回合状态记为部分撤销。先读现有 `undo_group`、`state_before_undo` 的语义再决定字段。
4. 批量撤销（[批量撤销](../../implemented/feature/2026-11-05-batch-undo-stale-conclusions.md)）共用同一分类。

**验证输入：** `turn_ledger_tests.rs` 加：文件已被用户手动改回；撤销做到一半再撤一次（幂等）；写回中途注入失败（落盘失败）；用户接管后改了其中一个文件。

**完成条件：** 重复撤销同一回合，第二次全部是"已到位"，没有跳过项。

### A8 缓存断点连同已有的一起不超过 4 个

**用户会看到：** 没有直接可见的变化；带着自定义缓存标记的请求不再被上游拒绝。

**现状：** `core/crates/aeroric-core/src/local_router/cache_injector.rs:21–73`：`written` 只统计本次新加的标记；`inject_cache_control` 对已有标记的块返回 `false`。请求里已有 2 个标记时最多还会加 4 个。

**改法：** 注入前先数请求体里已有的 `cache_control` 标记（tools、system、messages 的各内容块），可用额度为 `4 − 已有数`，用完即停。返回值改为"新加了几个"，调用方日志里同时记已有数。

**验证输入：** 已有 0、2、4、5 个标记的请求体；已有标记恰好在本来要加的位置上。

**完成条件：** 任意输入下，输出请求体里的标记总数不超过 `max(4, 输入已有数)`。

### A9 Wiki 解释页：来源被删或读不了，要分开说

**用户会看到：** 模块说明页的"待更新"原因分成"来源改了""来源被删了""来源这次读不到"。读不到时页面保留，不触发重写。

**现状：** `core/crates/aeroric-core/src/knowledge/explain.rs:58–64` 的 `changed` 只返回"当前仍存在且内容不同"的路径；被删的路径不在结果里，说明页不会变旧。调研还指出单个文件读取失败没有完整进入缺口状态（`core/crates/aeroric-host/src/session_live_services/wiki.rs`，动手前复核）。

**改法：**

1. `changed` 改为返回逐路径的状态：`Changed`、`Removed`、`Unreadable`。调用方传入的当前快照要能区分"确认不存在"和"这次没扫到"，为此扫描结果带一个完整性标记。
2. 扫描不完整时，只更新确认变化的部分；`Unreadable` 的来源保留原页，标"待核对"。
3. 第二份评审的 NR-06（`knowledge/notes.rs` 读不到的文件被当成不存在）如果仍在，同批修。
4. Swift：Wiki 页的状态条显示这三种原因。

**验证输入：** `explain_tests.rs` 加：来源删除、来源权限被收回、扫描中途出错（来源消失）。

**完成条件：** 读取失败不会导致任何页面或贡献被撤掉。

### A10 两个小项：字体注册失败、MCP 设置保存结果（动手前复核）

这两项调研给了位置，这次没有逐行读过。先读入口确认问题还在，不在就在笔记里写"已不成立"并跳过。

**字体。**

- 用户会看到：下载的对话字体没能注册时，设置里那一项显示"没装上"和原因，字体选择器不把它列为可用。
- 现状（调研给出）：`FontDownloads.swift:79` 注册失败只写日志；`ConversationFont.swift:45`、`ConversationFontSettings.swift:57` 仍按已安装处理。三个文件都在 `macos/AeroricKit/Sources/` 下。
- 改法：注册结果进入字体项的状态；失败时保留文件并提供重试和删除。纯 Swift，不涉及宿主。
- 完成条件：注册失败的字体不会被选中后静默回退到系统字体。

**MCP 设置。**

- 用户会看到：在设置里改了 MCP 服务器后，界面如实显示"已保存，重启会话后生效"或"没保存成功"和原因。
- 现状（调研给出）：保存的结果没有完整回到界面，"等待重启"的提示可能在保存失败时仍然出现。入口在设置页的 MCP 部分和 `core/crates/aeroric-core/src/extensions/mcp.rs`。
- 改法：保存方法返回逐项结果；"等待重启"只在确实写入成功后显示。
- 完成条件：保存失败时界面上没有任何"已保存"的表述。

### A11 Harness 升级不并发写同一个 npm 前缀（动手前复核）

**用户会看到：** 同时点两个 Harness 的升级，第二个显示"等前一个装完"。

**现状（调研给出，属条件风险）：** `core/crates/aeroric-core/src/…/upgrade.rs:496` 只检查同一个 Harness 是否在升级；几个 Harness 共用同一个 npm 全局前缀时，两个不同 Harness 的升级可以同时写。

**改法：** 按安装前缀（规范化路径）加一把进程内互斥；排队中的升级显示等待原因；宿主重启后不恢复排队，只恢复为"未开始"。

**验证输入：** 假安装器记录进入和退出时间，断言同一前缀下不重叠。

**完成条件：** 同一前缀任意时刻最多一个安装进程。

### 2026-11-17 锚点复核

A1、A2 已落地（见 [A1](./2026-11-16-a1-delegation-card-follows-delivery.md)、[A2](./2026-11-16-a2-approval-answer-must-arrive.md)）。2026-11-16 另一条工作线（ACP/DSH、讨论第二到四批、待办、定时任务等 11 个提交）合进了 main。2026-11-17 在 `main@9603b442` 加未提交的"交办选配置"改动上，按内容重新定位了 A3–A11 的锚点。问题都还在，变化如下：

| 片 | 现在的位置 | 和上面的说明相比 |
|---|---|---|
| A3 | `codex/map.rs:73`（`_ => TurnEndReason::Completed`） | 行号同。**新增**：合并进来的 ACP 驱动 `acp/driver.rs:1285–1302` 同样兜底，`stopReason` 缺失时默认 `end_turn`，`cancelled`、`refusal` 以外的值（含没见过的）都记 `Completed`；`max_tokens`、`max_turn_requests` 记完成并另发一条提醒。Claude 的 `result` 分支按 `is_error` 判断，没有这个兜底；OMP 的结束状态由驱动自己传入（`omp/driver.rs:1072–1078`），没有外部未知值，A3 会话再确认。**A3 再确认（2026-11-17）：不成立**，`prompt_result.status` 来自 omp 的帧，缺失默认 `completed`、未知值记完成，已随 A3 一并改，见 [A3](./2026-11-17-a3-unknown-turn-end-is-not-done.md) |
| A4 | `discussion.rs:44`（`READ_ONLY`）、`:314`、`:425`；`claude/map.rs:610`；`codex/map.rs:430` | 行号变了。**变化**：DSH 现在经 ACP 驱动接入，`acp/dialect.rs:68` 把 `ask` 和 `plan` 都映射成 DSH 的 `read-only` 环境；A4 要判断这是否算 Harness 自己强制的只读，原说明里"DSH 的权限通道不接受 `plan`"已不成立。**A4 落地（2026-11-17）**：DSH 的只读沙盒算 Harness 强制，本机"支持"；omp 的 `plan` 是 `always-ask`，"不支持"。委派子会话和审查者的候选只有 Claude、Codex（`review.rs` 的 `REVIEWERS`），所以委派那处检查今天不会触发，是给以后的保险；委派表单不置灰。见 [A4](./2026-11-17-a4-discussion-read-only-must-be-enforced.md) |
| A5 | `NotebookClient.swift:855`、`:867`、`:877`；`TaskWorkspaceView.swift:1542`（`RAGSearchView()`） | 视图行号变了；宿主方法名仍是 `notebook.rag_stats/rag_search/rag_neighbors`（`notebook_services.rs:187–191`）。**A5 落地（2026-11-17）**：漂移共 17 个（RAG、同步、存储连接、嵌入密钥），都已改；笔记页从 2026-10-30 起隐藏、App 没有 `NotebookClient`，"用户会看到"不成立，视图接线推迟到笔记页上线；检查改为对照 Rust 源码里的字符串，因为契约文件和 command-map 漏了 238 个现行方法。见 [A5](./2026-11-17-a5-client-method-names-follow-the-host.md) |
| A6 | `SessionsClient.swift:79`（`maxPages: Int = 24`）；调用点 `LiveSessions.swift:38`、`TaskWorkspaceView.swift:1777`；`sessions/claude.rs:32–33` | 调用点行号变了。**A6 落地（2026-11-17）**：读不出的行改在宿主读文件处统一计数、放在第一页回复的 `unreadable` 里，不分别改四个解析器；另外发现 DSH 整份被拒（日志版本太新）时宿主把它当成空历史，一并说出原因；提示放在会话顶部而不是会话信息里；导入会话和 DSH 终端会话页也接上了。见 [A6](./2026-11-17-a6-earlier-history-and-unreadable-lines.md) |
| A7 | `turn_ledger.rs:200–218`（`SKIP_CHANGED` 在 `:216`）、`:458`（`std::fs::write`）、`:341–344`（临时文件加改名） | 未变。**A7 落地（2026-11-17）**：计划加 `inPlace`；写回改成同目录临时文件加改名，符号链接不再被写穿；中途失败时把文件全部试一遍，结果里 `failed` 逐个写明，回合状态不变、再撤销一次接着做完。没有加"部分撤销"状态：`TurnState` 有多处在读，续做已经够用。见 [A7](./2026-11-17-a7-undo-in-place-and-whole-writes.md) |
| A8 | `local_router/cache_injector.rs:17–59`（`MAX_BREAKPOINTS`、`inject`） | 未变。**A8 落地（2026-11-17）**：按结构数已有标记（tools、system、消息内容块、`tool_result` 里那一层），工具 schema 和调用参数里的同名字段不算；没有额度时请求体不动；返回 `Injected { existing, added }`，请求自带标记时宿主日志记一行。见 [A8](./2026-11-17-a8-cache-breakpoints-count-existing.md) |
| A9 | `knowledge/explain.rs:58–64`（`changed`） | 未变。**A9 落地（2026-11-17）**：`changed` 换成 `sources_now`，分改过、被删、读不到；扫描逐文件记 `unreadable`、`too_large`，没加整体完整性标记；读不到只写"待核对"，不重写，模块文件全读不到时页面保留；原因写在页面引用块里，Swift 不用改；NR-06 已在 R1-1 修过。见 [A9](./2026-11-17-a9-wiki-sources-deleted-or-unreadable.md) |
| A10 | `FontDownloads.swift:79–81`（`register`，`CTFontManagerRegisterFontsForURL`） | 位置确认；MCP 保存仍要动手前复核。**A10 复核与落地（2026-11-17）**：字体成立但和调研说的不同——会话本来就按苹方回退，真正的问题是设置项把"文件在、没装上"显示成"未下载"，点了会重下；现在写明原因，给"重试"（只重新注册）和"删除"。MCP 保存已不成立：App 没有 MCP 设置页（扩展区是"即将推出"，`MCPSettingsView` 没有调用方，宿主没有保存方法），记为后续。见 [A10](./2026-11-17-a10-font-registration-and-mcp-save.md) |
| A11 | `agents/upgrade.rs:495`（开始升级）、`:359`（`jobs` 按 Harness） | 行号 `:496` → `:495`。**A11 复核与落地（2026-11-17）**：成立。锁的对象是步骤的安装程序（规范化后的 npm、brew 或官方二进制路径），没去求 `npm prefix -g`；一个升级一次拿齐所需的全部安装程序，否则整体等；等待时 `waitingFor` 写明在等谁，设置 › Agent 升级显示"等 X 装完"；排队只在内存里。见 [A11](./2026-11-17-a11-upgrades-share-an-installer-take-turns.md) |

## Alternatives considered

**把这 11 片合成一次"状态真实性第二轮"大改。** 它最强的理由是这些问题同源，一起改能统一术语和界面文字。没选它，因为各片落在不同的子系统（委派、驱动、网关、Wiki、随手记、撤销），合在一起既没法独立验收，也让一次会话的改动面过大。

**A1 不让卡片跟随投递，只把"已发送"改名为"已入队"。** 它最强的理由是改动最小，一行文字就诚实了。没选它，因为用户真正要知道的是主会话有没有收到；投递记录已经有这份信息，只差一个引用。

**A2 发送失败时自动重发审批回应。** 它最强的理由是用户不必再点一次。没选它，因为审批回应不是幂等的：第一次可能其实到了，重发会被 Harness 当成对下一个请求的回答。

**A3 给 `TurnEndReason` 加 `Unknown`。** 它最强的理由是语义最准确。没选它，原因写在 A3 里：穷举匹配点多，而业务处理与失败相同。

## Consequences

### 验收标准（提出时）

- 每片有对应的失败测试先行，改后通过。
- `cargo test --manifest-path core/Cargo.toml --workspace`、`cd core && cargo fmt --check`、`swift test --package-path macos/AeroricKit` 通过；A2 另跑 `swift test --package-path ios/AeroricCompanionKit`。
- 协议变化都是新增可选字段或新增枚举值，`export-schema` 后 `contracts/native/v1/**` 同步。
- 界面文字和状态变化逐片记入路线文档 §8 待目检清单。
- 每片落地时，对应的已落地笔记原地更新事实：A1 更新 [委派](../../implemented/feature/2026-10-31-delegation.md) 与 [投递队列](../../implemented/feature/2026-11-02-delivery-queue.md)，A4 更新 [讨论模式](../../implemented/feature/2026-11-12-discussion.md) 与 [能力表与登录](../../implemented/feature/2026-11-03-capabilities-and-login.md)，A7 更新 [回合撤销](../../implemented/feature/2026-11-04-turn-undo.md)，A9 更新 [Wiki 模块说明](../../implemented/feature/2026-11-10-wiki-module-explanations.md)。

### 风险（提出时）

- **A1 会改变自动回传的时序。** 现在是"标已发送后立即发"，改后是"入队后由队列决定何时发"。主会话忙时结果会排在回合之后，这是投递队列的既有行为，但委派卡上要说清楚"在排队"。
- **A2 可能让审批卡停留更久。** 连接已断但宿主还没察觉的窗口里，卡片会显示"没送出去"。要给这种卡一个明确的结束条件（回合结束或连接关闭事件）。
- **A3 可能把上游新增的正常状态判成失败。** Codex 以后若新增表示成功的状态值，会被记为"没跑成"。原始状态已保留在记录里，发现后加一行映射即可；相比之下，把异常算成完成的代价更高。
- **A4 可能让 OMP、DSH 成员暂时不能参加讨论。** 这是如实的结果；补上真正的只读模式后再放开。
- **A5 的方法名检查可能一次暴露多处不一致。** 发现的每一处都要判断是客户端写错还是宿主缺方法，不能只为了让检查通过去改名。
- **2026-11-18 全量审查**：A2–A4、A6–A9、A11 的边界遗漏已按审查交接单补上，逐条见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。本篇随之从 proposed 迁入 implemented。
