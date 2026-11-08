# Agent Note: Codex 原生子代理的线程随父会话路由，审批在父会话里问

Status: implemented

## Problem

Codex 的模型可以在回合中派生子代理，每个子代理跑在自己的子线程里。子线程的通知和审批请求与父线程共用同一条 app-server 连接，只是 `threadId` 是子线程自己的。

Aeroric 的共享 server 只按已注册的线程分发消息（[结构化协议](../architecture/2026-10-25-structured-agent-protocols.md)：一个进程、多个会话）。子线程的通知被丢弃；子线程的审批请求被回 `-32601 "no Aeroric session owns this thread"`。

后果有三条：
- 在"询问"或"自动编辑"权限档下，子代理申请提权的命令会当场失败，用户看不到这次审批。
- 子代理做了什么、改了哪些文件，父会话都看不到。
- 回合结束后，界面用落盘历史替换实时内容。历史里子代理只剩 `spawn_agent` / `wait_agent` 两张扳手卡和一串加密参数，子代理的回报也不显示。

在用户 Mac 上统计 Aeroric 托管 home 里的记录：共有 291 个原生子代理会话。它们的回合都在完全放开的权限档下运行（`approval_policy=never`），所以"审批被拒"这条路径还没有在实际使用中触发过，但子代理不可见的问题每次都存在。

### 线上的实际形态（2026-11-08 抓包）

第一版只按协议定义实现，Computer Use 目检没有通过：
- 子代理继承了"每次询问"档（`on-request` 加只读沙盒），直接在沙盒里跑 `touch`，得到 `Operation not permitted`，没有申请提权，所以没出现审批。
- 父会话里的派生是 `subAgentActivity`，第一版不认，也就不会认领子线程。

之后用按脚本应答的假模型驱动真的 `codex app-server`（0.163 和 0.166 都试过，结果一致），记录连接上的每一帧：

| | 多代理 v1 | 多代理 v2（gpt-6.x 实际用这个） |
|---|---|---|
| 子线程的 `thread/started` | 不发 | 不发 |
| 父线程怎么报告派生了子代理 | `collabAgentToolCall`（`spawnAgent`），完成时带 `receiverThreadIds` | 只有 `subAgentActivity`（`kind=started`、`agentThreadId`、`agentPath`），条目 id 就是 spawn 调用的 id |
| 子线程的回合、条目、审批请求 | 走同一条连接，`threadId` 是子线程 | 同左 |
| 审批请求发在哪个线程 | 只在子线程 | 只在子线程 |
| 向子线程 `turn/steer` | 可以 | 被拒：`-32600 "direct app-server input is not allowed for multi-agent v2 sub-agents"` |
| 落盘记录（父线程） | `spawn_agent` / `wait_agent`（命名空间 `multi_agent_v1`），spawn 输出带 `agent_id` 和 `nickname` | 同名调用（命名空间 `collaboration`），spawn 的 `message` 加密；`SubAgentActivity` 事件记开始和结束；子代理的最终回报是一条 `agent_message`（`author` 是它的路径，正文在 `Payload:` 之后） |

## Decision

### 归属（`codex/server.rs`）

- 每条路由（`Route`）都记下它属于哪个会话（`owner`，即会话自己的线程）。
- 路由每处理一条消息，先调用一次 `adopt`。一个线程满足下列任一条件，就挂到父线程的路由上：
  - 某个已注册线程的 `subAgentActivity` 在 `agentThreadId` 里写了它（v2 唯一的信号）；
  - 某个已注册线程的 `collabAgentToolCall` 在 `receiverThreadIds` 里列出了它（v1）；
  - `thread/started` 里的 `thread.parentThreadId` 或 spawn source 的 `parent_thread_id` 指向已注册的线程。实测两个版本都不发，保留只为协议定义里有。
- 孙线程按同一规则逐级认领；只有世代（epoch）一致才认领。
- **never**：认领不了的线程照旧拒绝，不能为了"能看见"就把陌生线程归到当前会话。
- `unregister(会话线程)` 会连带删除它认领的所有子线程。server 退出时只通知 owner 路由，每个会话只收到一次 `SERVER_EXITED`。

### 驱动（`codex/driver.rs`、`codex/map.rs`）

- 子线程的消息按 `threadId` 与会话自己的线程区分（`child_of`），不经过父线程的回合映射。
- 进度挂在派生它的那张卡上：
  - v2：`subAgentActivity`（started）这张卡，标题"子代理 X"，X 取 `agentPath` 的最后一段。这类条目的其他种类（completed 等）不另起卡片，结束由子线程自己的 `turn/completed` 写成一行。
  - v1：`spawnAgent` 的 collab 卡。`wait`、`sendInput` 也会把同一个子线程列为接收方，但不能抢走这张卡（第一版就是这样把"完成"写到了等待卡上）。
- collab 卡的标题：spawn 用它的任务（v2 加密时不用），其他调用按做什么命名（"等待子代理""给子代理发消息"……），和历史用同一张表（`map::multi_agent_action`）。第二次目检时 v2 的等待卡还写着"子 Agent"。
- 子代理的名字：v2 用路径末段；v1 不给名字，统一叫"子代理"，行文写"子代理 开始 / 完成"，审批胶囊只写"子代理"，不写成"子代理 · 子代理"。
- 子线程 `turn/started` 时追加"子代理 X 开始"，卡片设为运行中；回合中的 `fileChange` 收集路径；`turn/completed` 时追加"子代理 X 完成/失败/中断 · 改了 …"。同一张卡上如果还有其他子代理在跑，卡片保持运行中。
- 子线程的审批：
  - 照常生成审批卡，`PermissionRequest.subagent` 和 `PendingPermission.subagent` 写上名字；
  - 拒绝理由先作为插话发给子线程当前的回合。Codex 拒收（v2 一律拒收，v1 在子回合已结束时也会拒收）时，改发给会话自己正在进行的回合，写成"用户拒绝了子代理 X 的这一步：理由"，由父代理转告或换做法。会话这一轮也已结束时，在会话里提示理由没能送到。
  - 子线程那一轮结束时，它还没处理的审批随之取消。
- 父任务状态：
  - 父线程的回合以父线程自己的 `turn/completed` 为准；
  - 回合结束时只取消父线程自己的审批，并记下这一轮结束的状态（`settled`）；
  - 子代理的审批处理完、没有其他等待时，若父回合仍在跑就回到运行中，否则回到 `settled`。
- 空闲：还有子线程在跑时，父会话不算空闲（与 [空闲回收与自启回合](2026-11-07-idle-release-and-own-turns.md) 的后台工作否决一起生效）。server 换代、会话 `unsubscribe` 时清空子线程记录。

### 历史（`sessions/codex.rs`）

回合结束后界面用落盘历史替换实时内容（N4 D3），所以子代理在历史里也要认得出来：
- 多代理调用（`spawn_agent`、`wait_agent`、`send_input`、`followup_task`、`close_agent` 等；`wait` 和 `send_message` 只在多代理命名空间下算）显示成名为 `Agent` 的工具卡（和 Claude 的子代理工具同名），界面据此用人形图标。参数里加 `description`（"子代理 X"、"等待子代理"……），加密的 `message` 去掉，明文的保留。
- spawn 卡的结果由三部分拼成：
  - 输出给出名字时写"子代理 X 开始"（v2 取 `task_name` 的末段，v1 取 `nickname`）；
  - `SubAgentActivity` 的 completed / interrupted 事件追加"完成 / 中断"；
  - 子代理发给父代理的 `agent_message` 追加"回报：…"（`FINAL_ANSWER`）或"消息：…"。
- 代理之间的消息不当作对话显示；对不上任何 spawn 卡的就不显示。
- 有输出记录就算调用完成，输出为空也一样：送达的 `send_message` 记的就是空输出，第二次目检时它被标成了"未完成"。这条对所有 Codex 工具生效；界面展开时不显示空的"输出"段。
- 不读子线程自己的落盘记录，所以历史里没有"改了 …"，这一行只在回合进行中看得到。

### 协议与界面

- `PermissionRequest` 和 `PendingPermission` 新增可选字段 `subagent`，缺省时不序列化；手写的 session-live schema 同步更新，契约已重新生成（`contracts/native/v1/session-live.schema.json`）。
- 审批卡在标题旁显示"子代理 · 名字"胶囊（`LiveApprovalCard`）。

## Alternatives considered

- **启动时关掉 Codex 原生多代理（`-c features.multi_agent=false`），只用 Aeroric 自己的委派**：最强的理由是改动最小，所有写入都归 Aeroric 的单写者规则管。没有采用：用户选了"接住"。另外，关掉以后 Codex 的能力会和用户在别处使用时不一致，291 个真实子代理会话也说明这是常用功能。
- **把同一连接上所有陌生线程都当作当前会话的子线程（antumi 的做法）**：理由是不依赖父子关系字段，简单。在 antumi 里成立，因为它一个会话一条连接；Aeroric 的 app-server 由多个会话共用，这样做会把别的会话的线程误收进来，所以只认协议里写明的父子关系。
- **在父会话里展开子线程的完整对话**：理由是信息最全。没有采用：会把两个上下文混在一条对话里。用户确认只汇总开始、结束和改动文件。
- **v2 子代理被拒后只拒绝、不带理由**：理由是不打扰父代理。没有采用（2026-11-08 用户选了回退到父回合）：理由送不到，子代理和父代理都不知道用户要什么。
- **历史里再读子线程的落盘记录，列出它跑过的命令和改过的文件**：信息更全，但要按线程 id 找文件，改动更大。用户选了只读父会话自己的记录。
- **历史卡主标题直接写"子代理 X"**：理由是和实时卡一致。没有采用（2026-11-08 用户选了 `Agent`）：历史里所有工具都是"工具名 + 摘要"，名字放进摘要就和别的工具一个样子，也和 Claude 会话的 Agent 卡一致。
- **回合结束后不再用历史替换子代理卡**：会破坏 N4 D3（实时内容和历史不共享 id，只能整体替换），没有采用。

## Consequences

- **收益**：
  - 子代理的审批能交到用户手里，不再当场失败；
  - 子代理做了什么、改了哪些文件能在父会话里看到，回合结束后历史里也有它的开始、结束和回报；
  - 子代理还在跑时会话不会被回收。
- **代价与已知上限**：
  - v2 的拒绝理由经父代理转告，送不送到由父代理决定；父代理的 `wait_agent` 会被这次插话打断（"Wait interrupted by new input."），需要它自己再等。
  - 如果子线程的第一条请求早于派生它的条目到达，它仍会被拒绝。实测两个版本都是先发条目再起子回合，但协议不保证这个顺序。
  - 子线程的 `acceptForSession` 由 Codex 按线程处理，Aeroric 不额外沿用父会话的授权。
  - "每次询问"档对应 `on-request` 加只读沙盒：模型可以不申请提权就让写操作失败（目检就是这样），和界面文案"执行命令和改文件前都先问你"不符。会影响所有 Codex 会话，留给 C 批单独决定。
- **需要重访的信号**：
  - Codex 改了 `subAgentActivity`、`receiverThreadIds` 或落盘记录里多代理调用的名字和命名空间；
  - 子代理换成独立连接；
  - Codex 开始发子线程的 `thread/started`，或开放向 v2 子线程插话；
  - 用户希望在父会话里看到子代理的完整对话。

## Verification

- `codex/server_tests.rs`：`subagent_threads_go_with_their_parent_and_leave_with_it`。覆盖：
  - `parentThreadId`、spawn source、`receiverThreadIds`、`subAgentActivity` 四种认领；
  - 孙线程；陌生线程仍被拒；
  - 连带注销；每个会话只收到一次退出通知。
- `codex/driver_tests.rs`：
  - `a_subagent_s_approval_is_asked_here_and_its_denial_reason_goes_to_it`
  - `a_subagent_s_turn_is_a_line_on_its_card_and_leaves_the_session_s_status`（含 v1 的 `wait` 也列出同一子线程的情形）
  - `a_running_subagent_keeps_the_session_up_and_its_late_approval_returns_to_done`
  - `a_thread_nobody_spawned_is_still_refused`
  - `a_v2_subagent_is_named_by_its_activity_and_a_refused_reason_goes_to_the_session`（帧形状取自抓包）
- `codex/map_tests.rs`：`collab_cards_are_named_by_what_their_tool_does`。
- `tests/session_parsers.rs`：`codex_multi_agent_calls_become_subagent_cards_with_their_child_s_report`，用 0.163 真实记录的形状覆盖 v1 和 v2，含输出为空的 `send_message`。
- `codex/real_tests.rs`（默认跳过）：用真 Codex 加假模型跑 v1、v2 两轮，覆盖子代理审批进到本会话、拒绝理由送到能听见的一方、卡上的开始和完成两行、文件没被创建，以及历史里的 spawn 卡。2026-11-08 在 0.163 和 0.166 上各跑 3 次，全部通过：

  ```text
  AERORIC_REAL_CODEX=<codex 二进制> cargo test -p aeroric-core --lib real_codex -- --ignored
  ```

- `macos/AeroricKit/Tests/AeroricProtocolTests/SubagentApprovalTests.swift`：更新与快照两处都能解码 `subagent`。
- 第一版的 Computer Use 目检报告：`~/Documents/computer-use/2026-11-08-0039-aeroric-codex-subagent-review/report.md`（未通过，原因见 Problem）。
- 返工版的目检报告：`~/Documents/computer-use/2026-11-08-0210-aeroric-codex-subagent-review2/report.md`。审批、拒绝理由、开始/完成/回报都通过；它发现的"未完成"标记和"子 Agent"标题按上面两条修了。
