# Agent Note: 委派与契约（B4-2）：子会话、权限衰减、审计、结果卡与角色库

Status: implemented

## Problem

主会话就是 manager（R-1）。它经常需要把一块能独立完成的工作，比如检索、审查、实现、补测试，交给另一个会话做，再把结论带回来。前沿调研（`docs/frontier-roadmap.md` §5.2 形态 C、§5.3）给出了几条要求：

- 委派是契约，不是只拆任务；
- 子会话的权限只能是父会话的子集；
- 委派链要有不可改的审计；
- 子会话只回传结果卡；
- 深度为 1。

B4-1 审查回环已经有"另一个原生会话 + 卡片回到主会话"的骨架，B4-2 在它上面做通用委派。

owner 2026-10-31 睡前授权"尽可能多做"，当时没有人可以问，所以 Q-B8–Q-B16 都按推荐自主采纳，记在 Trellis 任务 `10-31-delegation` 的 prd.md 和路线文档 §10，标为"待 owner 醒后确认"。

## Decision

**协议**（`aeroric-protocol/src/delegation.rs`）

- `Contract` 是请求方写的契约，留空的字段取角色默认值。`Resolved` 是实际生效的契约。
- `DelegationCard`：每一步都往父会话日志追加一条 `delegation` 更新，界面按 `delegationId` 原地替换。
- `Role`、`AuditEntry`、`ReputationRow`。
- `AgentChoice` 有三种写法："same"、"other"，或 `{agent, model?}`。

**core**（`sessions/live/delegation/`）

- `Delegations` 在 sink 链里的位置：宿主 sink ← Delegations ← ReviewLoop ← CheckpointSink ← tracker。tracker 的 turn hook 同时通知 Delegations 和 ReviewLoop。
- `resolve` 规则：
  - 角色要求的权限高于父会话时，降到父会话的权限；
  - 契约里显式写的权限高于父会话时，直接拒绝；
  - 只读工作区只能用 plan。
- 其余拒绝：
  - 父会话是子会话、审查会话或侧支（深度 1）；
  - 已经有一个可写的子会话在运行（单写者）；
  - 指令为空，或角色不存在。
  - 每次拒绝都写进审计。
- 子会话的 task id 由 core 先分配，并在宿主建会话之前登记。否则子会话的第一句话会在登记之前到达，丢失。宿主建任务、分叉（新增 `fork_task_as`）都用这个 id。
- 子会话的验收条款写进它的检查点，作为义务，自动检查器在之后的回合跑它们的命令。
- 子会话回合结束（tracker 的 settle）时：记下改动行数；如果有检查在跑，就等 AutoCheck 结束；然后从子会话检查点读出验收结果，判断有没有证据（验收被核对过，或者有改动），状态记为 reported，写审计。
- settle 可能先于 TurnEnded 到达 sink：它在 tracker 自己的线程上执行。这时结论取当前回合正在收集的文字（测试 `a_settle_that_overtakes_the_turns_end_still_reads_the_text`）。
- 回传主会话：
  - 由会话发起的委派，全部结束、并且主会话空闲时，合并成一条 `[Aeroric 委派结果]` 消息发给主会话；主会话忙时，等它的下一个 TurnEnded。
  - 由用户发起的委派只出卡片，用户点"发给主会话"才发。
- 预算：每个运行中的委派配一个看守线程，每 5 秒看一次时长（默认 30 分钟）和 token。超了就打断子会话，记为 overBudget，进"需要我处理"。"退回重做"会起一个新的看守线程，旧的随之退出。
- 存储都在 `<data>/delegations/` 下：
  - `tasks/<parent>.json`：每个父会话的委派卡；重启后，还在运行的读回为 interrupted。
  - `audit.jsonl`：只追加。
  - `roles.json`：用户自建的角色，以及对内置角色的修改。
- 内置角色四个：调研员、审查员、实现者、测试员。内置角色可以改，删掉修改即恢复默认；不能删。
- 信誉统计：扫描全部委派卡，按 harness × 模型 × 角色聚合。只在界面上显示，不参与自动选择（Q-F10）。

**宿主**（`session_live_services/delegation.rs`）

- 起子会话：
  - fork 模式：`fork_task_as` 分叉，然后 reopen 并发出首条消息；只能交给与主会话同一种 Harness。
  - brief / isolated 模式：`create` 加 `start`。
  - 两种都写 `delegateOf` 和契约里的权限，复制 `worktree*` 字段，改名为"<标题>（委派·角色）"。
- 候选 Harness 与审查共用 `REVIEWERS`，即本机的 Claude Code 和 Codex。"same" 沿用父会话的模型和来源。
- Context MCP 新增三个工具：`delegate`、`delegations`、`list_roles`，分别对应 `context.delegate`、`context.delegations`、`context.roles`。令牌所属的任务就是主会话。`delegate` 立即返回；没写验收条款时，回执里会提示"建议继续拆分"。

**App**

- 时间线新增委派卡（胶囊行）：结论可以展开、逐条显示验收结果，按钮有"发给主会话""退回重做"（可以写补充说明）"停止""打开子会话"。
- 进度页新增"委派"小节：本会话的委派列表、新委派表单（CapsuleField 加 CapsulePicker，并显示信誉统计和"缺验收条款"提示）、审计折叠、"角色库"弹窗。
- 输入框里以 `#` 开头时，弹出角色和 harness 的建议；发出 `#名字 指令` 就改走 `delegation.start`。名字不是角色也不是 harness 时，原样当普通消息发出。
- 侧栏：子会话缩进挂在主会话下，用 `person.2` 图标。"需要我处理"新增"委派要你定"。

## Alternatives considered

- **`delegate` 工具阻塞到子会话结束才返回**：MCP 工具调用会超时，而且主会话这段时间什么都做不了。改为立即返回，结果全部回来后合并成一条消息推给主会话（对应 map-reduce 里的 reduce）。
- **每个子会话结束就单独回传一次**：会打断主会话的推理，几条结论也无法放在一起比较。
- **子会话的 task id 由宿主在建会话时生成**：子会话的第一句话会在 core 登记之前到达，结论为空（宿主集成测试复现过）。
- **子会话在独立 worktree 里写**：新建会话还没有接通 worktree（提交回执里 worktree 一直是 false）。改为"共享目录、同一时刻只有一个写者"。
- **把角色库放进设置页**：要新开设置导航。改为放在进度页的弹窗里，委派要用的东西都在同一处。
- **用信誉统计自动选 harness**：Q-F10 已经定为只显示、不自动选。

## Consequences

- 与路线文档的偏差：
  - 独立 worktree 没做。
  - 预算只有时长和 token，没有轮数。
  - 子会话越权时没有专门的待回应卡，沿用现有的审批卡和"需要我处理"。
  - 委派由会话发起时，主会话侧没有交接卡，因为子会话首条消息里已经带了交接清单。
  - 远程会话、omp、DSH 不能当子会话。
- 已知限制：
  - 自动检查只在回合有改动时才跑，所以只读子会话的带命令验收条款不会被执行，结果卡上显示"未核对"。
  - 子会话回报之后，如果用户直接在子会话里继续聊，结果卡不会更新，要用"退回重做"。
- 测试：
  - core：状态机 13 个，角色库 1 个；
  - 协议：2 个；
  - 宿主端到端 1 个：用户从 Codex 委派给另一族的只读审查员；Claude 子会话只拿到契约、拿不到对话；回报后结论进卡；子会话不能再委派；发出后 Codex 收到一个新回合；审计依次为拒绝、发起、回报、发出；
  - CLI MCP：工具列表；
  - Swift：契约 1 处、文字和表单 4 个、输入框 `#` 1 个。
- 2026-10-31 目检报告（computer-use `2026-10-31-0755-aeroric-delegation-board`）之后的修复：
  - **F1 经网关的模型看不到 Aeroric 工具**。两个原因：
    - 网关 `chat_bridge::response_tools_to_chat` 只转 function、custom、tool_search 三种工具，而 Codex 0.163 把 MCP 工具打包成 `{"type":"namespace","tools":[…]}`，被整组丢掉了。
      - 现在把命名空间里的工具展开成 `命名空间__工具名` 的 function；历史里带 `namespace` 的调用按同样规则展开；
      - 模型回调时，`restore_tool_namespaces` 和 `ChatSseTransformer::with_namespaces` 再拆回 `namespace` 加工具名；
      - Codex 转 Messages 的 pivot 路径同样适用。
    - Codex 日志记有 "omitting pending optional MCP server aeroric_context"：可选的服务还在启动，新会话的第一回合就没有它的工具。
      - 现在 Codex 的配置给 `aeroric_context` 加上 `required = true`、`startup_timeout_sec = 20`；
      - Claude 的 `mcpServers` 不加，以免它的配置校验拒收。
  - **F2 父会话下一回合之后，历史委派卡不见了**。原因：`LiveSessionModel.clearItems` 在历史追上之后清空全部实时行，委派卡不在会话历史里；重新打开会话时，模型也只从最新进度往后读。
    - 现在只要卡片还需要用户（运行中、未发出，或者有未确认的探针，`wantsUser`），清理时就保留；
    - 打开会话和断档重新同步时，用 `delegation.list` 把这类卡补回来。
  - **F3 表单没有 token 上限**：新委派表单加了"token 上限"（如 200k、1.5m），写进 `budget.tokens`。
  - **探针太吵**：
    - "完成未核销"区分"还没有自动核对"和"没有通过"；
    - "没有证据"只在回报完全没有依据时触发：没有改动、没有核对过的验收，最后一个回合也没有读文件或跑命令。
- 没有发过真实模型请求。真实界面没有运行过（NOT RUN），等 owner 目检。
