# ADR-024：项目分组名单的唯一真相源是 core 文件，localStorage 只做一次性幂等导入

- 状态：已接受（Task P2-05 门禁，Q51；P2-03 Q34 移交的第一条）
- 日期：2026-09-23
- 相关：ADR-013（项目身份无损归一）、ADR-012（`runtime.json` 可丢弃、内容指纹才是真相）、ADR-022（保存队列只排队意图）、INV-03

## 背景

分组名单今天住在 webview 的 `localStorage["aeroric:projectGroups"]`（`src/projectGroups.ts:3,41-46`），是一个**有序字符串数组**——顺序就是左侧 rail 的显示顺序，属于用户自己安排出来的东西。读取时 `mergeProjectGroupNames`（`projectGroups.ts:48-59`）会再把它和"项目记录里出现的 group"取并集，所以显示名单从来不是纯 localStorage 的产物。

P2-03 已经在 core 侧新建独占的 `project-groups.json`，并且门禁断言旧客户端会**无声忽略**它（`checks.json` 的 `legacy-client-ignores-core-exclusive-files`）。于是"两个客户端读两份名单"成为既成事实，只是还没人规定谁说了算。

关键事实：webview 的 localStorage **可以被平台清掉而用户无责**（Tauri 的 WebKit 数据目录会被系统清理、重装、隐私策略影响）。把用户安排的顺序放在那里，等于把一份不可再生成的数据放在一个可被回收的容器里。

## 决策

1. **`project-groups.json`（core）是分组名单的唯一真相源**，包括顺序。
2. **localStorage 只在迁移时读一次**：core 文件不存在、且 localStorage 里有归一化后的非空名单时，把那份**带顺序**的名单一次性导入 core。导入必须幂等——以"core 文件是否存在"为闸门，导入后 localStorage 不再参与读取，也不参与合并。
3. **归一化沿用现有函数**（`normalizeProjectGroupNames`：去空白、去空项、按首次出现去重）。core 与旧客户端对"什么是一个合法分组名"的判断必须逐字节一致，否则导入会制造一次无谓的顺序变化。
4. `mergeProjectGroupNames` 那半边（"项目引用了但名单里没有的 group 也要显示"）**保留**：它解决的是"名单落后于项目数据"，与"谁是真相源"是两件事，不要因为本决策顺手删掉。

## 后果

- 被否的两个方案：并集（core 顺序在前、localStorage 追加在后）——名字不丢但顺序仍会分叉，且"谁先进 core"取决于导入时机，日后无人能解释这个顺序；各管各自——同一台机器上两个客户端的分组顺序永久分叉，正是 P2-03 把名单搬进 core 时要避开的事。
- 旧客户端（Windows/Linux 产品）在 P2-05 之后要从"读 localStorage"改为"读 core 提供的名单"，这是一条真实的读取路径变更；macOS 上随 writer 切换一起生效，其余平台按 ADR-021 的口径推迟，因此过渡期非 macOS 平台仍以 localStorage 为准——这条差异必须写进 `tauri-adapter` 门禁，不能靠记忆。
- 导入是**一次性**动作，因此它需要一个可断言的边界：导入前后名单顺序不变、重复执行不追加。这属于 ADR-022 的"队列只排队意图"同一类问题——不是"覆盖一份快照"，而是"提交一次迁移"。
