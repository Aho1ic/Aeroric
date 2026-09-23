# ADR-015：写入基线是"core 上一次发布的字节"，首次遇到未知文件时一次性采纳

- 状态：已接受（Task P2-04 门禁 Q36）
- 日期：2026-09-22
- 相关：ADR-012（指纹是真相、`runtime.json` 可丢弃）、ADR-014（摘要实现）、INV-03（一个数据目录一个业务写入者）

## 背景

`runtime.json` 的 `revision` 只对**读过它**的写入者有意义，而发货中的旧客户端整个数据模型里没有 revision。迁移期必然出现"旧客户端写了一次、core 也准备写"的窗口。内容指纹要在这个窗口里做判断，就必须先回答：**一次写入凭什么算"覆盖的是自己的东西"？**

三个可辩护的基线：

| 基线 | 后果（实测） |
| --- | --- |
| 本次写之前刚读到的字节 | **检查永久失效**：服务层每个写路径都先 load，于是"我上次看到的"恒等于"现在盘上的"。反例 M7 只把 `commit` 的实参换成这个，`task_lifecycle` 立刻 3 条红，其中两条正是 ADR-012 要求的内容指纹反例 |
| 没有任何基线时一律拒绝 | core 面对从旧客户端继承来的目录，第一次写就被自己挡住，迁移无法开始 |
| `runtime.json` 里 core 上一次发布的字节（**选定**） | 首次遇到某文件时一次性采纳现状，从那次写入起任何旁路写都会被拒 |

## 决策

1. **每次写只允许覆盖"core 上一次发布的字节"**。基线存在 `runtime.json` 的 `contentFingerprints` 表里（schema v2），与 `revision` 同一次写发布——计数器前进一步和"这一步覆盖了什么"必须是同一个原子决定，否则表会指向改动之前的字节。
2. **core 从未发布过的文件，第一次写直接采纳磁盘现状并记账**。这是一次性的交接口，不是永久的盲点：采纳之后该文件就进入受保护状态。`a_file_core_has_never_published_is_adopted_rather_than_refused_forever` 同时钉住"能采纳"和"采纳后再被旁路写就会拒"两侧。
3. 字节不一致时返回 `StorageError::ContentDiverged`，按 ADR-011 归为 `staleWrite → REVISION_CONFLICT`（可重试性为 false：客户端要做的是重新读，不是再写一次）。
4. 写入顺序固定为 **先数据文件、后 `runtime.json`**。中间崩溃会让表落后磁盘一代，表现为"下一次写被拒"，靠重新 load 消除；反序会让表承诺一份从未落盘的字节，那才是会丢数据的方向。

## 后果

- 两条必需反例都成立：旧客户端先写过（`a_persist_that_refuses_starts_no_process_at_all`）、人工把文件换回旧内容（`content_hand_back_by_hand_is_a_conflict_rather_than_a_replay`），且两者都**一个进程都不起**——持久化失败不得启动进程。
- "拒绝"是保护而不是修复：core 永远不自动恢复、不猜 revision、不合并对方的改动（沿用 ADR-012 的"一代 .bak + core 永不自动恢复"）。
- P2-05 的旧客户端反例（"旧客户端仍在写 → core 必须因指纹冲突而拒写"）不需要新增机制，这条 ADR 就是它的落点；它同时要求 Tauri adapter 先接上 `StorageLocationProbe`，否则连写资格都没有（Q16/Q21/Q24 的旧账）。
- 项目导出/导入只搬 `projects.json` 与各 `tasks.json`，不搬 `runtime.json`：导入回来的目录基线为空，第一次写仍是一次性采纳。这是本决策已知且接受的行为，记录在此以免将来被当成 bug。
