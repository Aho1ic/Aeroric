# ADR-016：世代计数以 `task-operations.json` 为唯一权威，任务记录里只放一份会被改写的镜像

- 状态：已接受（Task P2-04 门禁 Q37）
- 日期：2026-09-22
- 相关：ADR-013（项目身份无损）、INV-03（一个数据目录一个业务写入者）、`docs/native/adr/015-*`（写入基线）

## 背景

"旧 generation 的退出不覆盖新 generation" 和 "同一 ID 重试只 spawn 一次" 都需要一个**每次运行单调前进**的计数器。它放在哪里决定它会不会被别的东西抹掉：

| 位置 | 事实 |
| --- | --- |
| 任务记录（`projects/<id>/tasks.json`） | 旧客户端整表覆盖会丢未知字段（`src-tauri/src/storage.rs:90` 起的 `Task` 没有 `deny_unknown_fields`，读时忽略、写时丢弃；`storage/compat.rs` 记着它曾经咬过一口）。计数器被抹回 0 的后果不是丢数据，是**丢顺序**：一个早就该作废的退出事件会看起来像当前这一代 |
| core 独占的 `task-operations.json` | 与认领同一文件、同一份进程崩溃后仍在；`legacy-client-ignores-core-exclusive-files` 门禁（P2-04 把 `task-operations.json` 加进正则）证明旧客户端根本不读它。代价：项目导出/同步只搬 `projects.json` + `tasks.json`，计数不跟着走 |
| 两处都写 | 副本随导出走，权威仍独占；风险是两份数字不一致时谁说了算 |

## 决策

**两处都写，`task-operations.json` 是唯一权威。**

- 权威：`OperationLedger` 的 `generations` 表。判定"这一代还在不在飞""事件属于哪一代"只看它。
- 镜像：`TaskService::commit` 在每次写 `tasks.json` 前，把每一行的 `generation` 改写成权威值——**包括改小，包括 0**。规则刻意不做"只升不降"：一个从别的机器导入、而本机账本并不认识的高计数会*看起来*权威而*其实不是*，那比丢掉这段历史更糟。
- 权威不受镜像影响：账本不认识的任务从 0 起算；携带 `generation: "7"` 的记录发来第 7 代事件，得到 `FutureGeneration` 报错（不是静默丢弃），因为"本机从未发放过这一代"属于接线故障，需要被看见。

## 后果

- 导出/导入后能看到任务曾经跑到第几轮（副本随文件走），但**导入那一刻轮次重新从 0 计**，且这是可测行为：`the_record_copy_is_a_mirror_and_never_the_authority` 钉住"副本不参与决策"和"谎言副本被改写回权威值"。
- 副本会被旧客户端抹掉，下一次 core 写入时再补回来。因此"最后一次由谁写"在副本上可见，但它不承担任何判定职责。
- 认领记录（未决的 start/resume）**不镜像**。副本解释"这是第几轮"是展示；"这一轮结束了没有"必须是独占文件里的事实，否则旧客户端一次保存就能让 spawn-once 失去凭据。
- 若将来要求"导出带走完整世代"，需要的不是把镜像变成权威，而是把 `task-operations.json` 纳入导出格式——那属于 Phase 6 的工作区/会话文件话题，本 ADR 不预告其形状。
