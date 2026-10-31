# Agent Note: 黑板、探针与 Needs You（B4-3）：共享黑板、失效探针、决定待确认

Status: implemented

## Problem

B4-2 之后，一个主会话可以同时有几个子会话。它们之间只能靠"结果卡"传话，没有共享的目标、假设、决定和待解问题；而 qunica 的经验是"一个成员的建议不等于团队决定"。另外，多 Agent 的典型失效（MAST：步骤重复、不知何时停、过早宣告完成、无验证）需要被看见，但不能替用户自动终止（`docs/frontier-roadmap.md` §5.4–§5.6）。

owner 2026-10-31 睡前授权"尽可能多做"，当时没有人可以问。Q-B17–Q-B23 都按推荐自主采纳，记在 Trellis 任务 `10-31-board-probes` 的 prd.md 和路线文档 §10，标为"待 owner 醒后确认"。

## Decision

**黑板**（协议 `board.rs`，core `sessions/live/board.rs`）

- 每个主会话一块黑板，它委派出的子会话共用。宿主按 `delegateOf` 找到黑板的主人。存为 `<data>/boards/<owner>.json`。
- 条目有六种类型，各自只能用对应的状态：

  | 类型 | 可用状态 |
  |---|---|
  | 目标、证据 | open |
  | 假设 | open / confirmed / refuted |
  | 认领 | open / done，可写认领人 |
  | 决定 | proposed / accepted / rejected |
  | 问题 | open / answered |

- 决定的规则：
  - Agent 写的决定一律是 proposed，`board.write` 也不能直接改决定的状态；
  - 采纳或否决只能走 `decide`：用户在 App 里做，或者黑板主人（主会话）通过 `board_decide` 做；
  - 子会话调用 `board_decide` 一律被拒。
- 每次写入都记进黑板的 `changes`（谁、何时、做了什么），最多保留 500 条。
- 读游标按"黑板主人 × 读者"分别记录，`board_read` 只返回这个读者上次读取之后有变化的条目（只注入变化）。子会话的首条消息里会提示它可以用黑板。
- `attention()` 返回两类条目：提议中的决定，和超过 30 分钟没有更新的认领。

**探针**（`delegation/mod.rs`；协议里 `DelegationCard.probes`、`delegation.ackProbes`）

- 只在委派出的子会话上跑，只做标记，不打断任何东西。
- 五种探针：

  | 探针 | 触发条件 |
  |---|---|
  | `repeat` | 一个回合里，同一个工具调用（标题加输入）出现 3 次 |
  | `oscillation` | 最近四次调用是 A B A B |
  | `nearBudget` | 时长或 token 用过 80%，子会话检查点里还没有通过的验收 |
  | `unverifiedDone` | 回报时有验收条款，但没有全部通过 |
  | `noEvidence` | 回报时没有证据（没有改动，也没有核对过的验收） |

- 同一种探针在用户确认之前只记一次。
- 有未确认探针的委派算作"需要用户"，进"委派要你定"；用户点"知道了"清除。

**宿主与 App**

- 宿主新增 `board.get/write/decide/attention`、`context.boardRead/boardWrite/boardDecide`；Context MCP 新增 `board_read`、`board_write`、`board_decide` 三个工具。
- 进度页的"黑板"小节：主会话有委派、黑板已有条目，或者当前是子会话（显示主会话的黑板）时才出现。内容依次是：
  - 目标、提议中的决定、待解问题排在前面；
  - 决定行有"采纳""否决"；
  - 假设、认领、问题的状态徽标可以点开改状态；
  - 最后一行用来添加条目。
- 委派卡上有一条探针提示，旁边是"知道了"。
- "需要我处理"新增"黑板要你定"，详情写"决定待确认：…"或"认领久未更新：…"。

## Alternatives considered

- **黑板作为笔记流**：路线文档 §5.4 明确要结构化、字段级写入。笔记流里看不出哪条是决定，也就没法单独确认决定。
- **子会话也能采纳决定**：违背"成员的建议不等于团队决定"。
- **每次把整块黑板注入给子会话**：浪费上下文；按读者游标只给变化。
- **探针命中时自动打断**：§5.5 明确"不自动终止"；会打断的只有预算（B4-2 的 overBudget）。
- **"认领久未更新"放在委派卡上**：认领是黑板上的东西，未必对应一项委派，所以放在黑板的 attention 里。

## Consequences

- 与路线文档的偏差：
  - Needs You 的"任务提案"要等 §3.6 行动卡做完才能接，没做。
  - "推理-行动不一致"探针（P2，需要 judge 抽样）没做。
  - 黑板的决定和经验沉淀进长期记忆，留到 B5。
- 黑板对所有 Agent 都可写，但只有本机 Claude/Codex 会话有 Context MCP。omp/DSH 和远程会话读写不了黑板。
- 测试：
  - core：黑板 4 个，覆盖决定只能提议、状态校验、读游标、过期认领与落盘；探针 3 个，覆盖重复与 ABAB、八成预算、完成未核销与无证据；
  - 协议：1 个；
  - 宿主端到端 1 个：用户写黑板，决定进入 attention，采纳后消失，坏令牌被拒；
  - CLI：工具列表；
  - Swift：文字与解码 1 个。
- 没有发过真实模型请求。真实界面没有运行过（NOT RUN），等 owner 目检。
- 2026-10-31 目检报告（computer-use `2026-10-31-1303-aeroric-b5-experience-feedback`）抓到一个死锁：
  - `Boards::attention` 里写的是 `if let Some(dir) = self.dir.lock().clone() { … }`，锁要到这段代码结束才释放；中间调用的 `ensure_loaded` 又去拿同一把锁，线程把自己锁死了；
  - 之后每次 attention 轮询都排在它后面，直到连接的 64 个请求名额全部占满。记忆读取、发送消息因此都超时，侧栏却还显示"已连接"；
  - 现在先把目录取到局部变量、放开锁，再去加载；
  - 回归测试：磁盘上有一块还没加载的黑板，在线程里调用 attention，5 秒内必须返回，并且确实加载了那块黑板；
  - 其余几处"锁住期间做事"的写法也检查过，它们都不会再拿同一把锁。
