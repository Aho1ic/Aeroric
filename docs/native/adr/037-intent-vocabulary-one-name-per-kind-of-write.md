# ADR-037：意图词汇表——每一类落盘写都有自己的名字

- 状态：已接受（Task P2-05 门禁，Q68～Q71 —— 用户拍板，四问全部采纳推荐项）
- 日期：2026-09-25
- 相关：ADR-022（队列只排意图、读-改-写归 core）、ADR-017（重试身份＝调用方的 operation key）、ADR-021（数据权威按平台）、ADR-026（元数据重放一次、状态变迁永不）、ADR-023（core 是 `task-status`/`task-session` 唯一发射者）、ADR-034/035（迟到观察丢弃、决策与落盘解耦）、ADR-028/029（`launchFamily`/`resumeFamily` 宿主装不下）

## 背景

ADR-022 定了"交的是意图不是快照"，但没有定**词汇表**。补记六十六把宿主入口装上之后，剩下的问题不是"要不要改前端"，而是"改成什么"——因为 ADR-022 决策 1 有一条硬推论：

> 只要还有**任何一处**发整表快照，"一份陈旧快照抹掉别人刚写进去的行"这个窗口就不关闭。所以词汇表必须覆盖全部写点，**不允许局部迁移**——按字段分叉"这半走 op、那半走快照"正是 ADR-022 第 28 行点名否掉的方案 ③。

于是先把 21 个持久化调用点逐点查清（`docs/native/reviews/tauri-adapter.md` §10）。查出来的规模远大于四个已有变体，并且暴露出四类结构完全不同的写：

| 类别 | 实例 | 为什么不是"再一条元数据意图"能装的 |
| --- | --- | --- |
| 非用户意图 | #11 启动修复（跨多项目、`attentionRequestedAt ?? interruptedAt`）、#16 完成后异步算出的 `additions`/`deletions`、#21 会话挂接（11 字段、四族互斥、`family` 由终端事件算出） | 载荷里没有"用户想怎样"，只有机器知道的事实 |
| 补偿写 | #4/#6/#8/#10/#13 | ADR-022 决策 4 说回滚留在宿主，但 #4/#6 删的是**已经真落盘**的行（远程路径 `persistBeforeLaunch` 还 await 过 flush） |
| 单行复合事务 | #9 resume（11 会话字段 + `agent` + `status`）、#12 switch_config（5 配置 + 11 会话 + `status` + 清 2 字段，await flush 后才启动） | 一次写横跨三个字段族，拆开就有中间态 |
| 抢同一字段的竞态 | #1 LLM 生成名字（3–5 s 后回来）vs #19 用户手输改名 | 今天靠宿主内存里 4 字段陈旧判定丢过期结果 |

## 决策

1. **每一类写都有自己的具名意图**，不引入通用 `PatchFields{fields}`＋字段白名单。规则住在 core：四族互斥"清三设一"、`?? interruptedAt` 的合并、`status === "todo"` 的前置，都由 core 在**权威快照**上执行。被否掉的方案是"一条通用 patch 意图覆盖三类"——它省一半实现量，但把白名单变成 ADR-022 已经否过的那种"这个字段谁能写"的长期逐字段记账，而且宿主仍然在表达"我以为这行长这样"，只是从整表缩到若干字段，漂移依旧是静默的。
2. **建行与删行都是意图**（`Create` / `Delete`）；**纯内存回滚不落盘**——意图失败时 core 什么都没写，宿主只改自己的视图。#4/#6 那种"删掉刚插的行"因此是一条真 `Delete` 意图，而不是快照回滚。明确否掉"把行的存在推迟到 spawn 成功之后"：那会拆掉远程路径今天用 `persistBeforeLaunch` + await flush（10 s 超时）钉死的崩溃窗口，把一个现有守卫当优化删掉。
3. **一次复合写＝一条原子意图**（`SwitchConfig` / `ResumeWithSession`），载荷与今天那一次写等价，原子性由 core 的同一轮 `load → 改 → commit` 保证。拆成多条小意图按序投递被否：它会留下"会话已挂但状态未改"的中间态，而 #12 的 await-flush-then-launch 要的语义正是"要么整套生效，要么不动"。
4. **`Rename` 带可选前置 `expect_name`**：core 在权威快照上判等，不等 ⇒ `changed: false` 且**不报错**。行为与今天"静默丢弃过期 LLM 结果"一字不变，变的只是"拿什么当权威"——从宿主内存换成磁盘。把生成器整体搬进 core（第三种方案）被推迟：那是 P3-02/P6 的范围，本轮不靠它解锁。
5. **状态变迁不进这条队列**（本轮查实，写下来防实现时手滑）：#15 是观察通道的既得结果——ADR-023 之后 core 已经是 `task-status` 的唯一发射者，ADR-035 已经把 observe 的决策与落盘解耦。队列里再加一条"设状态"意图，等于给同一个变迁两个入口，直接违反 ADR-026 的"状态变迁一律不自动重放"（两个入口意味着两个重放策略）。

## 词汇表（规模写死，别靠记忆）

既有 4 条：`Rename`（本轮起载荷多一个 `expect_name`）、`ToggleStar`、`Archive`、`Unarchive`。
新增 9 条：`Create`、`Delete`、`AttachSession`、`SetWorktree`、`DiscardWorktree`、`RecordDiffStats`、`NormalizeAfterBoot`、`SwitchConfig`、`ResumeWithSession`。
合计 **13 条**。按 ADR-026 分：**元数据类 9 条**（可自动重放一次）——`Rename`/`ToggleStar`/`Archive`/`Unarchive`/`AttachSession`/`SetWorktree`/`DiscardWorktree`/`RecordDiffStats`/`NormalizeAfterBoot`；**状态类 4 条**（永不自动重放）——`Create`/`Delete`/`SwitchConfig`/`ResumeWithSession`。这张分类表必须显式存在（代码里一处定义＋门禁），否则"按 op 分类的冲突处置"会退化成逐条 `if`。

## 后果

- **`Delete` 必须保留"删空 ⇒ 删文件"**。全仓没有 `delete_project` 之类的命令（`remove_dir_all` 只出现在无关代码里），所以 `App.tsx:788` 删项目是靠 #2 把该项目的任务清空、由 `save_project_tasks` 的空数组分支删掉 `projects/<id>/tasks.json`。这条隐性契约今天被 `write_task_records_raw`（`storage.rs:433`）逐字镜像着；意图化之后若丢了它，**删项目会静默留下 tasks.json**。同理不得为"确保目录存在"而把已删空的目录复活。
- **#16 的去重是真的变弱了，要写明而不是藏起来**：今天它靠 `pendingRef`（只在内存，刷新即失效）＋重新读盘上非 `undefined` 的值。防抖没了之后，幂等性只能靠 operation key（ADR-017），而"这一对数字是哪次完成产生的"必须由调用方给一个能表达该身份的值。实现时若给不出，就记为已知弱点，不能声称"意图队列天然去重"。
- **21 处调用点的删除是这一步的验收面**：改完之后 `persistProjectTasks` 应当没有生产调用方，`save_project_tasks` 退回它本来的角色（core 之外的兼容写路径，不再是宿主代表完整状态的手段）。门禁侧要有能数出"还剩几处快照写"的断言，否则"迁移完成"只能靠人读代码。
- **`launchFamily`/`resumeFamily` 的危险在此之后才真正消失**：它们今天靠补记六十补上的字段声明活着；意图不重写未点名的键，所以只要没有任何路径再发整表记录，这个键就不可能被宿主抹掉。

## 修正案（2026-09-26，Q72 —— 用户拍板）

逐点核对 21 处时发现 **#20（todo 编辑 `prompt`/`agent`/`permissionMode`，前置 `status === "todo"`）在 13 条里没有对应 op**：四问把它归为"单行编辑（带前置）"，却没有为它命名——`rename` 装不下三键，`create` 是建行不是编辑，保留快照则直接违反"不允许局部迁移"。用户批准**新增第 14 条具名意图 `EditTodo{task_id, prompt, agent, permission_mode}`**，分类为**元数据**（冲突可重放一次；前置判等在 core 的权威快照上，与 `Rename` 的 `expect` 同型）。词汇表因此为 **14 条：元数据 10 / 状态 4**；protocol 名单、`TASK_INTENT_VOCABULARY`、`TaskIntentOp`、契约产物四处同改——三侧门禁同红正是这条链该有的行为。被否的两项：把编辑并入 `create` 载荷（编辑发生在建行之后任意时刻）、#20 例外保留快照（=重开决策 1 否过的混合形态）。
