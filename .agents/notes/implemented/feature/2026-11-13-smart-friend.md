# Agent Note: Smart Friend：主会话卡住时请教更强的模型（ask_expert）

Status: implemented

## Problem

路线图 §5.2 的协作形态 B 叫 Smart Friend：主会话用便宜的模型干活，遇到难点时把一个更强的模型当工具来问；强模型缺信息时应该说"先去读某个文件再来问"，而不是编造。路线复核把它放在 B6（Q-F11：不首发，和网关的模型路由一起做）。

Aeroric 已经有[委派](2026-10-31-delegation.md)（B4-2）：子会话、只读角色、结果卡、预算、审计，以及上下文 MCP 的 `delegate` 工具。但它不能直接用来求助，原因有两点：

- 委派是异步的：结果要等主会话结束这一回合、空闲下来之后才合并送回；
- 结果卡只保留前 2000 字。

Smart Friend 要的是同一回合内就拿到完整回答。

用户 2026-11-13 说"全部按推荐"，第一批定为：

- 上下文 MCP 加一个 `ask_expert {question}` 工具，等专家答完后，把回答作为工具结果直接返回。Codex 默认 60 秒超时，要调长到 10 分钟以上。超过 10 分钟，答案改走委派结果，在主会话结束这一回合后送回。
- 在设置 → Agents 里全局选一个配置加模型作为专家，可以跨 Harness；不设默认值，没设置就不提供这个工具。
- 专家能看到问题、交接清单，以及项目目录（只读）。不用 fork，因为 fork 只能同一种 Harness。
- 第一批只提供工具，由主会话的模型自己决定什么时候问；每回合最多问 2 次。强制求助点（合并冲突、连续失败、改安全相关文件）放第二批。
- 前后对照评测放第二批。

## Decision

**求助就是一项只读委派**（`aeroric-core` `sessions/live/delegation`）

- 内置角色多一个"专家"（`EXPERT_ROLE = "expert"`）：交接清单模式，`plan` 权限，只读工作区，指令写明"只回答、不改文件、缺信息时说明先读什么"。
- 发给专家的消息由 `prompt::expert_question` 生成：把问题包在 `<question>` 里，要求先看交接清单，可以只读地核对文件，不要编造。
- 专家是谁（`ExpertChoice`）：配置的目录 id、agent、模型和显示名。存放在 `<data>/delegations/expert.json`，由 `Delegations::expert` / `set_expert` 读写；清除就是删掉这个文件。
- 每回合最多问 2 次（`expert_asks_this_turn`）：Delegations 在更新链上记下每个会话最近一次 `TurnStarted` 的时间，统计这之后由会话发起、角色是专家的委派。
- 取回答（`take_expert_answer`）：
  - 还在进行，返回"等待中"；
  - 已回报，取子会话最后一回合的完整文字，不受结果卡 2000 字的截断；
  - 失败、超预算、被停止，返回原因。子会话的回合以报错结束时，原因里带上这条报错的前 300 字（`TurnSettled.error`），比如网关拒绝了专家的模型；以前只写"子会话的回合没有正常结束"。这一点对所有委派都生效。
  - 取到回答或原因的同时，这张卡记为已发送。主会话结束这一回合时，委派结果的合并发送就会跳过它，不会把答案再发一遍。
- 专家的时长预算是 10 分钟（`EXPERT_MINUTES`）。超时由委派原有的看守线程打断，记为超预算。

**宿主**（`session_live_services/expert.rs`）

- `delegation.expert {set?: {configurationId, model?}, clear?}` 返回 `{expert, choices}`。
  - `choices` 取自不绑定项目的配置目录，只保留能启动的 Claude Code 和 Codex 配置，因为委派只会启动这两种。
  - 设置时会检查配置和模型；显示名写成"配置名 · 模型"。
- `context.expertAvailable {token}`：设置过专家时为真。
- `context.askExpert {token, question}`：
  - 没设置专家，回答"用户还没有设置专家模型（设置 → Agents → 求助专家）"；
  - 这一回合已经问满 2 次，也拒绝；
  - 否则按专家的 agent、配置和模型起一项只读委派（`AgentChoice::Named`），返回委派 id。
- `context.expertAnswer {token, delegationId}`：返回 `{state: pending | answered | failed, text, label}`。
- 超时要两边都放宽：
  - Codex：注入上下文 MCP 时加 `tool_timeout_sec = EXPERT_MINUTES × 60 + 60`；
  - Claude Code：带上下文 MCP 启动时设环境变量 `MCP_TOOL_TIMEOUT`，值相同，单位毫秒。

**上下文 MCP**（`aeroric-cli/src/mcp_context.rs`）

- 设置过专家时，`tools/list` 才列出 `ask_expert`，`initialize` 的说明里也加一句"遇到难点时请教一个更强的专家模型（ask_expert）"。Codex 只看这句说明决定要不要加载工具（见 [Wiki 评测](../testing/2026-11-10-wiki-agent-eval.md)）。
- `ask_expert` 不是简单地映射到一个宿主方法：
  1. 先调用 `context.askExpert`；
  2. 之后每 3 秒查一次 `context.expertAnswer`；
  3. 答完了，返回"专家（标签）的回答：……"；失败了，返回错误；
  4. 等满 10 分钟，返回"专家还在想"：答案会作为委派结果在这一回合结束后送回，主会话可以先按已知信息继续。

**界面**

- 设置 → Agents 多了一张"求助专家"卡：上面一行是当前专家（或"未设置"）和"不用专家"按钮；下面先选配置胶囊，再选模型胶囊，点模型就生效。宿主没有 `delegation.expert` 时，这张卡不显示。
- 专家的回答在主会话里照常显示为一张委派卡（"委派·专家"），带用时和 token。卡上会标"已发送"，因为答案已经通过工具结果给了主会话。

## Alternatives considered

- **直接用 `delegate` 加专家角色**：不用新工具。没有采用：`delegate` 立即返回，结果要等主会话空闲后才合并送回，主会话在这一回合里拿不到答案；而且结果只保留 2000 字。
- **fork 主会话给专家**：专家能看到完整对话。没有采用：fork 只能同一种 Harness，而 Smart Friend 的价值在于跨模型，比如主会话用 DeepSeek、专家用 Claude。交接清单加上问题里写的背景，已经够专家起步。
- **宿主一直挂着这个请求，等答完再回复**：MCP 那边少一个轮询循环。没有采用：宿主的阻塞线程会被占住整整 10 分钟；改由 MCP 进程轮询，宿主每次调用都立刻返回。
- **默认用最强的已登录模型当专家**：装好就能用。没有采用：专家很可能是最贵的模型，不该在用户不知情时花钱；等第二批评测证明有用之后，再考虑给推荐值。
- **第一批就加强制求助点**：路线图里写了。没有采用：要接"连续失败"和"改了安全相关文件"的检测，而且强制问会花钱；先看主会话的模型自己会不会用这个工具。

## Consequences

- 收益：
  - 便宜模型卡住时，可以在同一回合内拿到强模型的完整回答，而且能跨 Harness；
  - 复用了委派的预算、审计、结果卡和侧栏子会话：专家的会话可以打开来看，用量照常计入统计。
- 代价：
  - 主会话在等专家的这段时间里什么也做不了，最长 10 分钟；
  - 专家会话出现在侧栏，挂在主会话下面，问得多了侧栏会变长；
  - 只有 Claude Code 和 Codex 能当专家；
  - DSH（ACP）会话的上下文 MCP 没有放宽超时，暂时只在 Claude Code 和 Codex 主会话里可靠。
- 第二批的前后对照见 [Smart Friend 评测](../testing/2026-11-13-smart-friend-eval.md)：没有净收益。便宜模型三组都是满分，18 个会话里一次都没有主动问专家，所以专家保持默认不设置，强制求助点不做。
- 待目检（已记进路线图的待目检清单）：
  - 设置 → Agents → 求助专家的选择与清除；
  - 真实会话里调用 `ask_expert` 的等待、回答和委派卡；
  - 超过 10 分钟后答案改走委派结果送回。

## Testing

- core 委派测试（4 个新增）：
  - 工具取走的回答是完整的（超过 2000 字），取走后不会再作为委派结果发给主会话；
  - 专家没能启动时，工具拿到原因，也不再重复发送；
  - 每回合只统计本回合的专家提问，普通委派不算在内；
  - 专家的设置能保存、能清除，没有存放位置时报错；专家角色只读、`plan` 权限、交接清单模式。
- core 测试 `an_expert_whose_turn_failed_tells_the_tool_why`：专家的回合以报错结束，工具拿到带报错的原因。
- 内置角色从 5 个变成 6 个：角色库测试和宿主测试 `delegation.roles` 的计数随之改动。
- 宿主集成测试 `the_expert_is_set_from_the_choices_before_an_agent_can_ask`：没设置时不提供工具，提问被拒并提示去哪里设置；`delegation.expert` 返回可选的配置；不能用的配置被拒；无效令牌被拒。
- CLI 测试：说明里提到 `ask_expert`；工具清单只有它一个，而且不映射到单个宿主方法。
- Swift `ExpertStoreTests`：解码；设置、被拒时保留原专家、清除；宿主不支持时隐藏这张卡。
