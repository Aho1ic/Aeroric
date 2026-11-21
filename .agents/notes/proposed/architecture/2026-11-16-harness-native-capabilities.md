# Agent Note: Aeroric 的能力怎样在每个 Harness 里像原生功能一样被自动用起来

Status: proposed

## Problem

Aeroric 已经有长期记忆、项目 Wiki、委派和共享黑板、内置与沉淀技能、插件和 MCP 管理。owner 的要求是：不管用 Claude Code、Codex、DSH、OMP，还是以后接的 ZCode、Antigravity、Kimi，Agent 都应该自己想到去用这些能力，用法和它在自家 Harness 里用原生功能一样，包括派子代理；用户不该感觉到"这是外挂上去的"。

对照 `8d4391c3` 的代码，现在离这个要求有五处具体的缝：

1. **工具只到了两家。** Context MCP 的 15 个工具只注入本机 Claude 和 Codex（[`claude/driver.rs:219`](../../../../core/crates/aeroric-core/src/sessions/live/claude/driver.rs)、[`codex/driver.rs:227`](../../../../core/crates/aeroric-core/src/sessions/live/codex/driver.rs)）。`SessionDriver::set_context_server` 默认返回 false（[`live/mod.rs:120`](../../../../core/crates/aeroric-core/src/sessions/live/mod.rs)）；`Route::runs_locally()` 对 OMP 和所有远端路由也返回 false（[`executor.rs:85`](../../../../core/crates/aeroric-core/src/sessions/live/executor.rs)）。OMP、DSH、远端会话都拿不到。
2. **OMP 会话读不到任何 Aeroric 记忆，但不是 OMP 不支持。** 记忆档案只走系统层，`set_instructions` 只有 Claude 和 Codex 实现；没有系统层时交接简报会拼到第一条消息前，记忆档案直接丢掉（[`executor.rs:302–345`](../../../../core/crates/aeroric-core/src/sessions/live/executor.rs)）。OMP 上游有 `--append-system-prompt`，也读 agent 目录下的 `mcp.json`，Aeroric 启动 OMP 时两样都没传（[`launch.rs:157–179`](../../../../core/crates/aeroric-core/src/agents/launch.rs)）。
3. **什么时候用，全靠模型自己想起来。** 给模型的线索只有一句服务器说明（[`mcp_context.rs:212`](../../../../core/crates/aeroric-cli/src/mcp_context.rs)）和各工具的描述。第一次 Wiki 评测里 16 个会话全部跳过了 Wiki，原因就是说明里没提它（同文件 `:209` 的注释）。目前没有"该触发时触发了没有"的回归数据。
4. **原生子代理看得见，但用不上 Aeroric 的东西。** 三族的原生子代理已经映射成 `Subagent` 更新并显示在父回合里（依据见 §2）。缺的是另外三件：没有验证过它们能不能调到 Context 工具；Aeroric 的角色库（[`delegation/roles.rs`](../../../../core/crates/aeroric-core/src/sessions/live/delegation/roles.rs)）只有 `delegate` 用得上，模型用自家派发工具时拿不到；原生子代理和 `delegate` 开的子会话在记录上是两回事，黑板上分不清是谁写的。
5. **每种能力各有各的挂载办法。** 技能有三种挂法（受管 home 符号链接、Claude 的 `--plugin-dir`、Codex 的项目副本，见 [内置技能](../../implemented/feature/2026-10-30-builtin-skills.md)），MCP 是另一种，系统层又是一种。新接一家 Harness 要逐项重新想一遍，漏一项就是一条缝。

"智能伙伴"在仓库里没有同名功能（只有 iOS 伴随端），本文按"跨会话、跨 Harness 认识用户、会主动找用户的那个角色"来理解，见 §8 第 6 问。

**2026-11-16 核对修订。** 本文第一版有三处判断是错的或不完整的，逐格核对后已改：

- 第一版说原生子代理"没映射"。实际上 Claude、Codex、OMP 三族都已映射。第 4 条和 H5 相应改写，H5 的工作量变小。
- 第一版把 OMP 的系统层和 MCP 记为"没有 / 待实测"。上游两样都有，是 Aeroric 没接。H2 因此从"给 OMP 做命令行兜底"改成"给 OMP 接原生入口"，命令行兜底只留给真的两样都没有的 Harness。
- 第一版说 Hook "没用上"。仓库里曾有一套写进用户 `~/.claude`、`~/.codex` 的 Hook 管线，2026-10-27 已停用并在启动时清理（[删除旧 Hook 管线](../../implemented/simplification/2026-10-27-remove-legacy-hook-pipeline.md)）。H4 不恢复那套做法。

两篇外部文章给了方向，结论先放这里，细节在对应小节里引用：

- **Anthropic《The Components of Agent Memory》**：记忆有两条到达模型的路，一条是自动注入的很小的顶层，一条是按需读和搜；顶层必须小，注错一条会污染每个会话。告诉模型记忆是干什么用的，比规定它怎么用更有效（给强模型加"只记失败"的硬规则，收益反而掉了 0.234）。会话外的整理（"做梦"）对照原始记录纠正错误笔记、把资料编成 Wiki（多文档判断题 87% 对 73%），并决定什么升到顶层、什么降下去。
- **doubleAI《Can LLMs redesign themselves as they train?》**（只读了前 10 万字符，共约 17.8 万）：讲的是训练期改模型结构，和 Aeroric 只有类比关系。可借的是它的验证纪律：新增的东西先挂在初始为零的门后面；用"关掉"和"同样大小但内容打乱"两个对照证明收益是真的；介入时机有讲究（查表层第 1,200 步再加，召回 72%，从第 0 步就加只有 55%）。

## Proposal

### 1. 原则：一份能力，多个原生入口

能力只在 core/host 里实现一次（现状已如此：`context.*` 宿主方法）。每个 Harness 通过它**本来就会用**的入口拿到同一份能力。入口一共七种：

| 入口 | 模型怎么感知它 | 适合承载 |
|---|---|---|
| 系统层开场白 | 不用知道它存在，开局就在上下文里 | 记忆档案、"Aeroric 能帮你做什么"、Wiki 索引 |
| MCP 工具 | 工具列表 | 检索、读写黑板、委派、记忆提议 |
| 技能（`SKILL.md`） | 描述常驻，正文按需加载 | 做事的方法；也是没有 MCP 时教它用命令行的地方 |
| Hook 或宿主侧事件 | 不经过模型决定，到点就发生 | 开场注入、按需召回、收尾写回 |
| 原生子代理定义 | 它自己的派发工具 | Aeroric 角色库 |
| 斜杠命令 | 用户主动敲 | 交接、整理记忆这类用户发起的动作 |
| 命令行（`aeroric-cli ctx …`） | Shell 工具 | 兜底：系统层和 MCP 都没有的 Harness |

同一个能力在不同 Harness 里名字、参数、返回含义都一样，只是入口不同。这是"没有割裂感"的可检查定义。

### 2. 现状对照表（2026-11-16 逐格核对）

四种状态：**已接通**（Aeroric 现在就在用，有代码行号）；**可接未接**（上游有这个入口，Aeroric 没传）；**待实测**（静态阅读定不了，条件见本节末尾）；**无**。

| Harness | 系统层 | Context 工具 | 技能 | Harness 自己的 Hook | 子代理：看见 | 子代理：能用 Context 工具 | 子代理：Aeroric 角色 |
|---|---|---|---|---|---|---|---|
| Claude Code（本机） | 已接通 | 已接通 | 已接通 | 可接未接 | 已接通 | 上游说继承，待实测 | 可接未接 |
| Codex（本机） | 已接通 | 已接通 | 已接通 | 待实测 | 已接通 | 上游说继承，待实测 | 待实测 |
| OMP（本机） | 可接未接 | 可接未接 | 已接通 | 可接未接 | 已接通 | 接通工具后才谈得上 | 可接未接 |
| DSH | 待实测 | 待实测 | 已接通 | 待实测 | 无 | 待实测 | 待实测 |
| 远端 Claude、远端 OMP | 同本机，缺断言 | 无（留给 D 批 D4） | 待实测 | 无 | 同本机 | 无 | 无 |
| 通用 ACP（驱动未建） | 规范有可选字段 | 规范要求必备 | 各家不同 | 各家不同 | 各家不同 | 各家不同 | 各家不同 |

逐族依据。Aeroric 一侧是 `8d4391c3` 的代码；上游一侧写明来源，凡是"经摘要读取"的，开工时对原文再读一遍。

**Claude Code（本机）**

- 系统层：`--append-system-prompt`（[`live.rs:965`](../../../../core/crates/aeroric-host/src/live.rs)）。
- 工具：`--mcp-config` 加 `--allowedTools mcp__aeroric_context`（[`live.rs:952`](../../../../core/crates/aeroric-host/src/live.rs)）。
- 技能：直连 Claude 用会话插件 `--plugin-dir`（[`live.rs:848`](../../../../core/crates/aeroric-host/src/live.rs)，插件由 [`builtin_skills.rs:366`](../../../../core/crates/aeroric-core/src/extensions/builtin_skills.rs) 的 `claude_plugin_offers` 生成，插件名 `aeroric`）；走配置的 Claude 用受管 home。
- Hook：没接。上游的插件标准布局里有 `hooks/hooks.json`，和 `skills/`、`agents/`、`commands/`、`.mcp.json` 并列（官方 plugins-reference，2026-11-16 读了全文）。所以现有的会话插件目录就能带 Hook，不用碰用户的 `settings.json`。
- 子代理看见：`Task`、`Agent` 工具映射成 `ToolKind::Subagent`（[`claude/map.rs:121`](../../../../core/crates/aeroric-core/src/sessions/live/claude/map.rs)）；带 `parent_tool_use_id` 的帧留在卡片里（`:214–219`）；`task_started`、`task_progress`、`task_notification`、`task_updated` 映射成 `Subagent` 更新。
- 子代理能用工具：官方 sub-agents 页说子代理默认继承主会话的 MCP 工具，不继承技能但能通过 Skill 工具发现插件技能（经摘要读取，只读了前 10 万字符）。`--allowedTools` 对子代理是否同样生效没有验证。
- 角色：插件的 `agents/*.md` 会成为 `aeroric:<名字>` 子代理；另有 `--agents '<JSON>'` 只在本次会话生效。插件子代理的 `hooks`、`mcpServers`、`permissionMode` 字段会被忽略（同上页）。

**Codex（本机）**

- 系统层：`thread/start` 的 `developerInstructions`（[`codex/driver.rs:547`](../../../../core/crates/aeroric-core/src/sessions/live/codex/driver.rs)）。
- 工具：`thread/start` 的 `config.mcp_servers`，标了 `required` 和 20 秒启动超时（[`codex/driver.rs:227–244`](../../../../core/crates/aeroric-core/src/sessions/live/codex/driver.rs)）。
- 技能：项目 `.agents/skills` 下带标记的副本。
- Hook：没接。上游从 `~/.codex/hooks.json`、`~/.codex/config.toml`、受信任项目的 `.codex/` 和插件的 `hooks/hooks.json` 读；`SessionStart`、`SubagentStart`、`UserPromptSubmit`、`PreToolUse`、`PostToolUse` 能加上下文（官方 hooks 页，经摘要读取）。Aeroric 不写用户的 `~/.codex`，所以只剩两条路：`thread/start` 的 `config` 里带，或写进项目目录。前者文档没提，待实测。
- 子代理看见：`subAgentActivity`、`collabAgentToolCall`（[`codex/map.rs:301`、`:317`](../../../../core/crates/aeroric-core/src/sessions/live/codex/map.rs)）。
- 子代理能用工具：官方 subagents 页说子代理继承父会话的权限、MCP 服务器和技能，自定义代理文件不能覆盖这些（经摘要读取）。
- 角色：自定义代理是 `~/.codex/agents/*.toml` 或项目 `.codex/agents/*.toml`，必填 `name`、`description`、`developer_instructions`（同上页）。能否经 `config` 按次下发待实测；不能的话只能像技能一样写项目目录的带标记副本。

**OMP（本机）**。上游依据是本地克隆 `0838d2ab4407`（2026-10-27，18.13.1）的 `docs/`；Aeroric 要求的最低版本是 18.12.0（[`upgrade.rs:77`](../../../../core/crates/aeroric-core/src/agents/upgrade.rs)），那个版本有没有下面这些没有核对。

- 系统层：没接。上游有 `--append-system-prompt <文字或文件>`（`cli-reference.md:173`）。
- 工具：没接。上游读 agent 目录（Aeroric 用 `PI_CODING_AGENT_DIR` 指到受管 home，[`live.rs:598–658`](../../../../core/crates/aeroric-host/src/live.rs)）下的 `mcp.json`，`command`、`args`、`env` 里支持 `${VAR}` 和 `${VAR:-默认}`（`mcp-config.md:402`）；RPC 模式照常加载 MCP（`rpc.md:544`）。MCP 的认证质询在 RPC 模式下没有界面处理（`rpc.md:1236`），Context 服务器不需要认证，不受影响。
- 技能：受管 home。
- Hook：没接。上游可以按次传 `--hook <路径>`、`--extension <路径>`（`cli-reference.md:157–158`）；`before_agent_start`、`context`、`tool_call`、`tool_result` 能往上下文里加东西（`hooks.md:112–150`）。Aeroric 一侧只有扩展名守卫。
- 子代理看见：启动时发 `set_subagent_subscription`（[`omp/driver.rs:521`](../../../../core/crates/aeroric-core/src/sessions/live/omp/driver.rs)）；`subagent_lifecycle`、`subagent_progress` 映射成 `Subagent` 更新（[`omp/map.rs:206–240`](../../../../core/crates/aeroric-core/src/sessions/live/omp/map.rs)）。
- 子代理能用工具：上游说子代理的 MCP 经父会话的连接代理（`mcp-runtime-lifecycle.md:231`、`:265`），MCP 工具对子代理是"附加的"（`task-agent-discovery.md:38`）。Aeroric 先得把工具接给父会话。
- 角色：`<agent 目录>/agents/*.md`，frontmatter 有 `name`、`description`、`tools`、`spawns`、`model`、`autoloadSkills`（`task-agent-discovery.md`）。
- 另一个事实：OMP 会自己导入用户 `~/.claude.json`、`~/.codex/config.toml` 里的 MCP 服务器。C8 的清单要把这些也列出来，否则用户会看到清单外的工具。

**DSH**。不走 `SessionDriver`，走自己的结构化宿主（[`agents/dsh_host/`](../../../../core/crates/aeroric-core/src/agents/dsh_host/mod.rs)），能力表整行是 No。上游依据是本地克隆 `fcc0ad4621`（2026-11-04）。

- 系统层：上游的系统提示由同进程插件分段拼装（`docs/subsystems/system-prompt.md`），没找到从外部追加一段的参数。待实测。
- 工具：上游的 MCP 按 `@deepseek-ai/dsh-mcp-client` 条目配置（`docs/subsystems/mcp.md`）；`dsh acp` 的会话可以按 ACP 标准挂 MCP 服务器（`packages/acp/README.md`）。Aeroric 的受管配置能不能加一个客户端条目，待实测。
- 子代理：上游有 `subagents.*`，Aeroric 没接。

**远端 Claude、远端 OMP**。复用本机的会话类型，只换了启动器（[`live.rs:292`、`:329`](../../../../core/crates/aeroric-host/src/live.rs)），所以系统层和子代理事件应与本机一致，但没有测试断言。Context 服务器是本机进程，远端连不回来，`executor_tests.rs:646` 断言跳过。

**通用 ACP 和几家候选**

- ACP 规范（session-setup 页，经摘要读取）：`session/new` 带 `mcpServers`，"所有 Agent 必须支持 stdio"；另有可选的 `systemPrompt`，Agent 在 `sessionCapabilities.systemPrompt` 里声明了才能传，`append` 模式是追加。
- ZCode（本地克隆 `a9fe401e`，2026-11-03）：技能读 `.zcode/skills` 和 `.agents/skills`（`docs/external-agent-skills-import.md:61–62`）；Hook 在 `~/.zcode/cli/config.json`，`SessionStart`、`UserPromptSubmit` 能加上下文（`docs/hooks-runtime-contract.md:61`、`:186–187`）；子代理定义是 `~/.zcode/agents/*.md`。没找到无界面或 ACP 的启动方式，怎么驱动它待实测。
- Kimi 读 `.agents/skills`，Antigravity 读 `.agent/skills`（本地 Trellis 的平台表）。

**还剩的待实测**（H1 的输入；每条都要假进程或真实 Harness，本文没有跑）：

1. OMP 18.12.0 是否已有 `--append-system-prompt`、`mcp.json` 的 `${VAR}` 展开和 `agents/*.md`。没有就抬高最低版本。
2. Codex `thread/start` 的 `config` 能否带 Hook 和自定义代理。
3. Claude 会话插件里的 `hooks/hooks.json`、`agents/` 在无界面 stream-json 模式下是否生效，依赖的最低 Claude Code 版本是多少。
4. Claude、Codex 的原生子代理是否真能调到 `aeroric_context` 的工具，调用时用的是不是父任务的 token。
5. DSH：受管配置能否加 MCP 客户端条目，系统提示能否从外部追加，`subagents.*` 事件长什么样。
6. 远端：系统层补断言；技能是否挂上。
7. ZCode 的无界面驱动方式；ACP `systemPrompt` 字段对原文复核。

另有一处语义要补：能力表里 `DelegateChild` 对 Claude、Codex、OMP 本机都是 Yes（[`capabilities.rs:56–59`](../../../../core/crates/aeroric-core/src/sessions/live/capabilities.rs)），但这个 Feature 没有文档注释，Swift 侧也没有使用方。OMP 会话没有 `delegate` 工具；如果它的意思是"能当父会话发起委派"，OMP 这一格现在不实。

### 3. 分片

编号用 H。H1–H3 把缝补上，H4–H6 让能力被自动用起来，H7–H8 让系统越用越好。格式沿用批次笔记：用户会看到、现状、改法、验证输入、完成条件。

**与批次 C 的关系**（[C 批](../feature/2026-11-15-adoption-c-memory-gateway-skills.md)）：C7 技能目录、C8 有效能力清单原样保留，H1 给 C8 的清单提供数据来源。C9 第 1 步"事实核对"里 OMP 的部分本文 §2 已做完，C9 第 2 步里 OMP 的注入和 H2 是同一件事，只在 H2 做一次；C9 留下 DSH、远端的说明和如实置灰。实现时 C8、C9 先读本文再动手，不要做出第二套清单。

#### H1 入口表：每个 Harness 有哪些入口，凭什么说有

2026-11-20 第一步已实现，见 [H1 第一步笔记](../../implemented/feature/2026-11-20-h1-surfaces-per-launch.md)：能力表加了系统层、技能、事件、子代理、命令行五个入口项，工具沿用 C9 的 `contextTools`，没有另加 `SurfaceTools`；每次启动按实际情况记下系统层（驱动是否收下开场内容、多少字）、工具、技能（Aeroric 自己的技能挂了几个），其余三项取能力表，`session.capabilities {taskId}` 返回 `surfaces`；假 Codex、假 Claude 的启动测试核对记录与启动参数一致，远端 Claude 的系统层有了断言；`delegateChild` 写明含义。第 4 条（七条待实测）留给第二步，会话信息里的显示留给第三步。第二步也已实现，见 [H1 第二步笔记](../../implemented/feature/2026-11-20-h1-pending-checks-settled.md)：七条逐条定论，写进 `docs/subsystems/agent-capabilities.md` §6；OMP 18.12.0 三样都有，不抬最低版本（没设的 `${VAR}` 原样保留，H2 要用 `${VAR:-}`）；Codex 的 Hook 和角色可经 `config` 按次下发，子代理继承 Aeroric 服务器和 token；DSH 的 ACP 没有 `systemPrompt`、不发子代理事件；Claude 插件的 Hook 和代理、三家子代理的真实工具调用为 NOT RUN。能力表里 Codex、DSH 的事件改为"没有"并写明原因。第三步也已实现、待目检，见 [H1 第三步笔记](../../implemented/feature/2026-11-20-h1-surfaces-shown.md)：会话信息在"工具"上面加"入口"一行，写接通几个、是哪几个，点开逐项写状态和原因；和工具共用同一次 `session.capabilities` 读取。H1 完成。

**用户会看到：** 会话信息里除了工具清单（C8），还有一行"这个 Harness 通过哪些入口用 Aeroric"：系统层、工具、技能、事件、子代理、命令行，各自是"已接通""这家不支持""还没核对"。

**现状：** 能力表的 `Feature` 和 `SupportState` 在 [`live_capability.rs:25`、`:64`](../../../../core/crates/aeroric-protocol/src/live_capability.rs)，按家族和本机/远端取值（[`capabilities.rs`](../../../../core/crates/aeroric-core/src/sessions/live/capabilities.rs)）。`set_instructions`、`set_context_server` 返回的 bool 是"这次启动真的接上了没有"的唯一证据，但现在没有存下来：没有交接简报时 `set_instructions` 的返回值直接丢弃（[`executor.rs:321–323`](../../../../core/crates/aeroric-core/src/sessions/live/executor.rs)），`give_context` 只用它决定要不要撤销 token（`:358–372`）。

**改法：**

1. `Feature` 增加六个入口项（暂名 `SurfaceInstructions`、`SurfaceTools`、`SurfaceSkills`、`SurfaceEvents`、`SurfaceSubagents`、`SurfaceCli`），沿用 `SupportState`。不新建类型，[能力表](../../implemented/feature/2026-11-03-capabilities-and-login.md) 的协议、UI 和拒绝逻辑直接复用。
2. 每一项为 Yes 的依据是启动时真实发生的事（参数里带了、握手回来了），不是家族名。把两个 `set_*` 的返回值存进启动记录，入口项从启动记录读。
3. 给 `DelegateChild` 写清含义并按含义改 OMP 那一格。
4. 把 §2 末尾的七条待实测逐条做掉。能用假进程或固定版本的上游源码定的就定；必须跑真实 Harness 的记 NOT RUN 并写明条件。结论写进 [`docs/subsystems/agent-capabilities.md`](../../../../docs/subsystems/agent-capabilities.md) §4（那张表现在停在 2026-10-13）。

**验证输入：** 四族假进程的启动记录；能力表快照。

**完成条件：** 七条待实测每条有结论或带条件的 NOT RUN；任何一格是 Yes 都能指到一条测试或启动记录。

#### H2 OMP 接上系统层和工具；命令行兜底留给两样都没有的 Harness

2026-11-20 第一步已实现，见 [H2 第一步笔记](../../implemented/feature/2026-11-20-h2-omp-opening-and-tools.md)：本机 OMP 的开场内容写进会话自己的 0600 临时文件，用 `--append-system-prompt` 传入，进程退出即删；工具服务器声明在受管目录的 `mcp.json`（token 写成 `${AERORIC_CONTEXT_TOKEN:-}`，只在进程环境里），`runs_locally()` 对本机 OMP 为真；OMP 的 `always-ask` 会拦 MCP 工具，用 `--config` 叠加层按工具名只放行这 21 个，不动用户的 `config.yml`；远端 OMP 两样都不收。真实 OMP 会话为 NOT RUN（这台 Mac 没装 omp）。第 5、6 条（`aeroric-cli ctx` 与内置技能 `aeroric`）留给第二步。第二步也已实现，见 [H2 第二步笔记](../../implemented/feature/2026-11-20-h2-ctx-command-line.md)：`ctx` 直接调 MCP 服务的 `tools/call`，结果逐字段相同（对照测试用假宿主），`ctx --help` 由工具表生成；内置技能 `aeroric` 只挂给能力表里没有 Aeroric 工具的 Harness，挂载与 C7 记录走同一个筛选，现在四家都有工具所以不挂给任何一家，用只有 Shell 的假 Harness 验证；给这种 Harness 的环境变量由 `ContextServer::shell_env` 得出。H2 完成。

**用户会看到：** OMP 会话和 Claude、Codex 会话一样，知道用户的偏好，能查记忆、读 Wiki、看交接清单、发委派。以后接入一个既不能配 MCP 也没有系统层的 Agent，它也能通过命令行用到同样的东西。

**现状：** OMP 的启动参数由 `omp_rpc_args` 生成，只有 `--mode rpc`、审批模式、`--cwd`、模型、思考档、恢复和 `--add-dir`（[`launch.rs:157–179`](../../../../core/crates/aeroric-core/src/agents/launch.rs)）。受管 home 和技能挂载在 [`live.rs:598–658`](../../../../core/crates/aeroric-host/src/live.rs)。Context 服务器的命令、参数和 token 环境变量在 [`context_server.rs`](../../../../core/crates/aeroric-core/src/sessions/live/context_server.rs)（`TOKEN_ENV` 在 `:14`）。工具表只有一份，在 [`mcp_context.rs:16`](../../../../core/crates/aeroric-cli/src/mcp_context.rs) 的 `tools()`。

**改法：**

1. **OMP 系统层。** `OmpSession` 实现 `set_instructions`，启动参数加 `--append-system-prompt`。上游接受文件路径，用会话自己的临时文件（权限 0600，关会话删除），记忆内容不进进程参数。
2. **OMP 工具。** 受管 home 里写一份静态的 `mcp.json`，声明 `aeroric_context` 服务器，`env` 里写 `${AERORIC_CONTEXT_TOKEN}`；token 放进这次会话进程的环境。受管 home 是多个会话共用的，所以文件里不能出现具体 token。`OmpSession` 实现 `set_context_server`；`Route::runs_locally()` 把本机 OMP 改成 true，远端 OMP 仍是 false。改之前先读清它当初为什么是 false。
3. 没有 token 的启动（Context 被关掉时）要验证 `${VAR}` 展开失败时 OMP 的行为，必要时用 `${AERORIC_CONTEXT_TOKEN:-}` 并让服务器对空 token 直接退出。
4. Codex 上出过"每次询问"模式下 MCP 调用被当成用户拒绝的问题，当时的修法是只给这一个服务器设 `default_tools_approval_mode = "approve"`，理由是这些工具只动 Aeroric 自己的记忆、黑板和委派（`codex/driver.rs:238–243` 的注释）。OMP 的各审批模式下 Context 工具会不会被拦，逐档验证。被拦时，OMP 有同样的按服务器设置就照 Codex 的做法；没有就走现有审批路径给用户看，不放宽整个会话的审批模式。
5. **命令行兜底。** `aeroric-cli ctx <工具名> [--json 参数]`：和 MCP 走同一张工具表、同一个宿主方法、同一个 token。`ctx --help` 的文字从 `tools()` 生成。
6. 新增内置技能 `aeroric`：描述里写清什么时候该用（"需要以前的讨论、项目约定、设计决定，或者想把一块工作交给别人时"），正文列命令。它只挂给 H1 表里 `SurfaceTools` 不是 Yes 的 Harness；有 MCP 的不挂，避免同一能力两个入口抢着被调用。这一片里它没有真实的挂载对象（DSH 要等待实测第 5 条），用一个"只有 Shell"的假 Harness 验证。

**验证输入：** 假 OMP 进程的启动参数和环境快照；受管 home 里 `mcp.json` 的内容；`ctx` 与 MCP 对同一输入的输出。

**完成条件：** 假 OMP 的启动快照里有系统层文件参数和 token 环境变量，`mcp.json` 里没有 token 明文；关会话后 token 失效（一条测试）；`ctx` 与 MCP 对同一输入返回同样的结构化结果（一条对照测试）；待实测第 1 条有结论。

#### H3 统一的开场白

2026-11-20 第一步已实现，见 [H3 第一步笔记](../../implemented/feature/2026-11-20-h3-unified-opening.md)：`sessions/live/opening.rs` 拼用途说明（480 字）、记忆档案（1200 字）、指路（Wiki 页标题与角色名，600 字），合计上限 2280 字，超出先去掉指路；每次启动（含恢复）交给系统层，Claude、Codex、本机 OMP 都收下；用途说明里的工具清单那一句与 MCP 服务说明同一个函数（`opening::tool_uses`），原文未改。第 5 条（账单按段计入）留给第二步，没有系统层的 DSH 留给第三步。第二步也已实现，见 [H3 第二步笔记](../../implemented/feature/2026-11-20-h3-opening-in-the-bill.md)：账单按段计入开场白，用途说明与指路算"说明"、记忆档案算"记忆"、交接简报算"其他"；Codex 读开发者消息里那一段，Claude 从系统提示里用途说明的标题起拆出。第三步也已实现、待目检，见 [H3 第三步笔记](../../implemented/feature/2026-11-20-h3-attached-context.md)：没有系统层的 DSH 和远端 OMP，把整份开场白（连同交接简报）装进"Aeroric 附加的上下文"信封附在第一条消息前，App 折叠显示、不当作用户的话；没按本节写的"用途说明和指路放进技能 `aeroric`"，因为 DSH 有 MCP 工具、不挂那个技能，指路又是每个项目不同的；账单那一项没做，因为 DSH 的会话文件不记基础部分，账单本来就不拆。H3 完成。

**用户会看到：** 换一个 Harness 开会话，Agent 一样知道用户的偏好和项目约定，一样知道这里有 Wiki 和以前的会话可查。

**现状：** 开场内容由 `hand_handoff` 拼（[`executor.rs:302–345`](../../../../core/crates/aeroric-core/src/sessions/live/executor.rs)）：记忆档案（[`memory/profile.rs`](../../../../core/crates/aeroric-core/src/memory/profile.rs)，上限 `PROFILE_BUDGET_CHARS` 1200 字）加交接简报，中间用分隔线。"Aeroric 能做什么"只出现在 MCP 服务器说明那一句里（[`mcp_context.rs:212`](../../../../core/crates/aeroric-cli/src/mcp_context.rs)），没接工具的 Harness 完全看不到。

**改法：**

1. core 里一个函数拼出开场白，三部分，各有字数上限，合计有总上限：
   - 记忆档案（现有）；
   - 用途说明：三四句话讲 Aeroric 提供了什么、**是干什么用的**，不写"每次必须先查"之类的硬规则（依据见 Problem 里的记忆文章）；
   - 指路：Wiki 有哪些页的标题、角色库有哪些角色。只给标题，不给正文。
2. 各族的投递位置：Claude 用 `--append-system-prompt`，Codex 用 `developerInstructions`，OMP 用 H2 接上的系统层，ACP 在 Agent 声明了 `systemPrompt` 能力时用它。都没有时，用途说明和指路放进技能 `aeroric` 的描述；记忆档案按 §8 第 2 问，带明显标记拼到第一条消息前，界面上显示为"Aeroric 附加的上下文"。
3. MCP 服务器说明那一句和开场白的用途说明用同一份文字来源，避免两处说法不一致。
4. 开场白里每条记忆的 id 照旧计入"已注入"（B5-3a），这样 H8 才能比较注入和没注入的差别。
5. 开场白的内容和长度进 [回合上下文账单](../../implemented/feature/2026-11-09-turn-context-and-retry.md) 的 `memory`、`instructions` 两类，用户看得到它占了多少。

**验证输入：** 四族假进程收到的开场内容；账单里的分类和字数。

**完成条件：** 四族各一条启动测试，断言开场白到达的入口和内容摘要一致；总长不超过上限；没有系统层的假 Harness 上，第一条消息前的附加段带标记且账单里单独记一项。

#### H4 到点就发生的事，不靠模型想起来

Aeroric 是各 Harness 的上级进程，结构化事件流本来就经过宿主。所以大部分"Hook"不需要 Harness 自己的 Hook 机制，宿主看到事件就能做，对所有家族一视同仁。

2026-11-21 第一步已实现，见 [H4 第一步笔记](../../implemented/feature/2026-11-21-h4-changed-files-mark-wiki.md)：第 3 条，回合结束时用改过的文件给引用它们的 Wiki 页标“可能过时”。第 1、4 条已由 H3、H5 做完；2026-11-21 第二步已实现，见 [H4 第二步笔记](../../implemented/feature/2026-11-21-h4-per-message-recall.md)：第 2 条，默认只记录（H8 的“候选”档），打开后才附加。第 5 条目前没有需要在回合中途改写上下文的事，不做。

**用户会看到：** 提的问题和某条记忆、某页 Wiki 明显相关时，Agent 开口前就已经拿到了；回合结束后，值得记的东西自己进了待确认列表；刚改过的模块，对应的 Wiki 页当场标上"可能过期"。

**现状：**

| 时机 | 宿主现在做什么 |
|---|---|
| 会话开始 | Claude、Codex 投递记忆档案和交接简报 |
| 用户发出一条消息 | 什么都不做 |
| 回合结束、压缩前 | 更新检查点；抽取记忆提议（[`memory/extract.rs`](../../../../core/crates/aeroric-core/src/memory/extract.rs)） |
| 原生子代理启动、结束 | 显示 `Subagent` 更新，不做别的 |
| 改了某个模块的文件 | 不做；Wiki 页是在下次编译时才标"可能过期"（[`knowledge/compile.rs:487–525`](../../../../core/crates/aeroric-core/src/knowledge/compile.rs)） |

旧的 Hook 管线靠往用户的 `~/.claude/settings.json` 和 `~/.codex/config.toml` 里写条目、每次事件拉起一个 Node 脚本来报状态，2026-10-27 因为状态已改由协议事件提供而停用。`managed_home/` 里的构造函数还在，等清理逻辑发布一个周期后删除。

**改法：**

1. 会话开始：投递开场白（H3）。
2. 用户发出一条消息：本地检索记忆和 Wiki（不调模型），分数过线才附上，最多几条、带来源和 id；没有就什么都不加。**默认关闭**，等 H8 证明它确实有用再开。理由有两条：记忆文章指出注入的东西错了会污染每个会话；doubleAI 那篇的类比是"加得早不如加得准"。
3. 改了文件：从工具事件里拿到被改的路径，和 Wiki 页记录的来源文件比对，命中就立刻标"可能过期"。用的是编译时同一套判断，只是提前。
4. 原生子代理启动、结束：见 H5。
5. 只有需要在回合中途改写模型上下文的事才用 Harness 自己的 Hook，且只走按会话生效的位置：Claude 的会话插件 `hooks/hooks.json`，OMP 的 `--hook`。不写用户的配置文件，不恢复旧管线，Hook 脚本不依赖 Node。Codex 等待实测第 2 条。

**验证输入：** 同一条用户消息在四族假进程上的召回决定；一次文件修改事件之后 Wiki 页的状态。

**完成条件：** 同一条用户消息在四族上触发同样的召回决定（同样的命中、同样的"不附加"）；开关关闭时没有任何附加；改文件后对应页在编译前就带"可能过期"。

#### H5 让原生子代理用上 Aeroric 的东西

2026-11-20 第一步已实现，见 [H5 第一步笔记](../../implemented/feature/2026-11-20-h5-roles-as-native-subagents.md)：第 3 条（给角色）和第 4 条（`delegate` 说明）。角色库里适合原生子代理的角色写成 Claude（会话插件 `aeroric:<id>`、配置的受管目录 `aeroric-<id>`）和 OMP（受管目录 `aeroric-<id>`）的子代理定义，只读角色用工具白名单限住；Codex 不投影，因为它的自定义代理不能收窄父会话的沙盒；内置角色里现在只有调研员可投影。2026-11-21 第二步已实现，见 [H5 第二步笔记](../../implemented/feature/2026-11-21-h5-native-subagents-listed.md)：第 1 条，进度页的委派一节同时列出原生子代理（由 Harness 管理，没有打断按钮）和委派子会话，Codex 子线程的开始、结束也发 `Subagent` 更新；第 2 条，Context 服务器分不出调用来自主会话还是子代理，按退路由子代理在 `board_write` 里自报名字，投影的角色定义写明该填的名字。H5 完成。

不和原生子代理抢，也不强迫模型改用 `delegate`。"看见"已经有了，剩三件事。

**用户会看到：** Agent 用自家的派发工具点名"审查员"时，拿到的就是 Aeroric 角色库里的那个角色；子代理写到黑板上的内容标着是哪个子代理写的；原生子代理和 `delegate` 开的子会话在同一个地方列出来，各自标明"Harness 自己管理"和"Aeroric 管理"。

**现状：** 协议里有 `SessionUpdateKind::Subagent { id, title, status, detail }` 和 `ToolKind::Subagent`（[`session_live.rs:183`、`:64`](../../../../core/crates/aeroric-protocol/src/session_live.rs)），三族的映射位置见 §2。角色库是 `Role { id, name, description, instruction, agent, context_mode, permission, workspace, acceptance, budget, … }`，四个内置角色加用户的 `roles.json`（[`delegation/roles.rs`](../../../../core/crates/aeroric-core/src/sessions/live/delegation/roles.rs)）。`delegate` 的工具描述在 [`mcp_context.rs:42`](../../../../core/crates/aeroric-cli/src/mcp_context.rs)。

**改法：**

1. **记在一起。** 委派列表里把本回合的 `Subagent` 更新也列出来，如实标"由 Harness 自己管理，Aeroric 只能看"。不给打断按钮，除非该族有结构化的打断方法。
2. **装备。** 先做待实测第 4 条。能继承 MCP 的（上游文档说 Claude、Codex 是；OMP 经父连接代理）不用额外做事，只要确认 token 是父任务的。黑板写入时带上子代理标识，需要 Context 服务器能区分调用来自父还是子；区分不了就在角色定义里要求子代理写入时自报名字。
3. **给角色。** 把角色库投影成各家的原生定义：
   - Claude：会话插件的 `agents/<id>.md`，模型看到的名字是 `aeroric:<id>`。
   - OMP：受管 home 的 `agents/<id>.md`。
   - Codex：按待实测第 2 条的结论，走 `config` 或项目 `.codex/agents/` 的带标记副本。
   - `instruction` 进正文；`permission` 能用该家的工具白名单表达时才表达，表达不了的不放宽。
   - 只投影在原生子代理里说得通的角色。要换 Harness 或模型（`agent` 不是当前这家）、要隔离的可写工作区、要预算和验收的角色不投影，仍然只能经 `delegate` 用。
4. `delegate` 的工具描述改成讲清用途：要换一种 Harness 或模型、要用户随时能接手、要跑很久、要一个可写且隔离的工作区时用它；同一会话里的快速分头检索用原生子代理。[委派](../../implemented/feature/2026-10-31-delegation.md) 的规则不变：子会话权限不高于父、同时只有一个可写子会话、不能再委派。

**验证输入：** 假事件流里的原生子代理；各族启动快照里的角色定义文件；一个带可写工作区的角色。

**完成条件：** 原生子代理和委派子会话在同一列表里出现并各自正确收束；内置角色里可投影的出现在各族的定义位置，不可投影的没有出现；没有打断能力的家族界面上没有打断按钮。

#### H6 能力包：插件和 MCP 的统一投影

2026-11-21 第一步已实现，见 [H6 第一步笔记](../../implemented/feature/2026-11-21-h6-capability-packs-mcp.md)：能力包清单、信任判定和投影计划；包里的 MCP 服务器按会话投到四族（第 1、3 条的服务器部分，第 5 条的“未注入及原因”）。2026-11-21 第二步已实现，见 [H6 第二步笔记](../../implemented/feature/2026-11-21-h6-pack-skills-roles-hooks.md)：技能和命令（写成技能）随内置技能挂载，角色并进原生子代理的角色库，事件处理只投给直连 Claude 的会话插件；表里 Codex、OMP 的事件处理、Claude 的命令目录与下表不同，原因见该笔记。2026-11-21 第三步已实现，见 [H6 第三步笔记](../../implemented/feature/2026-11-21-h6-packs-page.md)：Agent 配置 → 能力包页和 `packs.*` 接口；第 4 条核对后不用另做去重，OMP 按来源优先级让 Aeroric 受管目录里的服务器胜出，清单写明。H6 完成。

**用户会看到：** 在 Aeroric 里装一个插件或加一个 MCP 服务器，所有 Harness 的新会话都能用，不需要到每家的配置里再配一遍；不支持的那家写明原因。

**现状：** `extensions/` 里是分散的守卫和校验：来源信任（[`trust.rs`](../../../../core/crates/aeroric-core/src/extensions/trust.rs)）、MCP 配置校验和掩码（`mcp.rs`）、DSH 插件版本映射（`plugins.rs`）、技能路径守卫（`skills.rs`）、OMP Hook 扩展名守卫（`hooks.rs`）。唯一成形的投影是内置技能的三种挂载（`builtin_skills.rs:707` 的 `Mount`）。

**改法：**

1. 一份 Aeroric 清单描述一个能力包：技能、MCP 服务器、角色、命令、事件处理。来源信任沿用 `trust.rs`。
2. 每个 Harness 一个投影函数，把清单变成那家的原生形式：

   | 内容 | Claude | Codex | OMP | ACP |
   |---|---|---|---|---|
   | 技能 | 会话插件 `skills/` | 项目带标记副本 | 受管 home | 各家的项目技能目录 |
   | MCP 服务器 | `--mcp-config` 或插件 `.mcp.json` | `config.mcp_servers` | 受管 home `mcp.json` | `session/new` 的 `mcpServers` |
   | 角色 | 插件 `agents/` | 待实测第 2 条 | 受管 home `agents/` | 各家不同 |
   | 命令 | 插件 `commands/` | 技能代替 | 技能代替 | 技能代替 |
   | 事件处理 | 插件 `hooks/hooks.json` | 待实测第 2 条 | `--hook` | 无 |

   Claude 现成的会话插件目录从"只装技能"扩成"装整个包"。**仍然不写用户自己的 `~/.claude`、`~/.codex`**，这条现有规则不动。
3. 用户在 MCP 管理里加的服务器按会话启动投影。工具多的时候不在 Aeroric 里自己做网关，先依赖各家已有的延迟加载；Codex 延迟加载时只给模型看服务器说明，所以每个被投影的服务器都要有一句说清用途的说明（Wiki 评测的教训）。
4. OMP 会自己导入用户其他 Harness 的 MCP 配置，同名服务器可能出现两次。投影时按名字去重，清单里写明哪个生效。
5. 清单里某一项在某家投影不了，C8 的清单里逐项写原因。

**验证输入：** 一个含五类内容的假能力包；四族启动快照。

**完成条件：** 假能力包的每一项在各族的启动快照里出现在正确位置，或出现在"未注入及原因"里；未受信任的包里的事件处理和 MCP 服务器没有被投影。

#### H7 会话外的整理，跨 Harness

2026-11-21 第一步已实现，见 [H7 第一步笔记](../../implemented/feature/2026-11-21-h7-native-memory-as-evidence.md)：第 1 条，原生记忆文件当证据读，`native.rs` 文件头和路线文档 §6.4 已原地更新。2026-11-21 第二步已实现，见 [H7 第二步笔记](../../implemented/feature/2026-11-21-h7-opening-placement.md)：第 2 条，事实按使用移出或移回开场白，有记录。第 3 条（对照原文纠错）是第三步。

**用户会看到：** 在 Claude 里让它"记住"的事，换到 Codex 也能用上（先经用户确认）；长期没用上的记忆自己退出开场白；和会话原文对不上的记忆会收到一条带原文位置的改正提议。

**现状：** 后台整理和 Wiki 编译都在宿主侧，和 Harness 无关。各家自己的记忆文件只展示：[`memory/native.rs:1–3`](../../../../core/crates/aeroric-core/src/memory/native.rs) 写明"不索引、不注入、不写"。

**改法：**

1. **原生记忆当证据读。** 整理时把各家记忆文件当候选来源读：从中提炼的内容照常走提议和用户确认，带来源路径。Aeroric 仍然不写它们，权威仍只有一处（§8 第 3 问已定）。落地时原地更新 `native.rs` 的文件头和路线文档 §6.4。
2. **升降。** 按被注入后的反馈和被检索的次数，把条目在"开场白"和"按需检索"之间移动；开场白满了先降最久没用上的。整理频率按访问量定，不按固定周期。
3. **对照原文纠错。** 整理时发现记忆和会话原文矛盾，提议替换并附原文位置（C1 做了定位，这里用它）。

**验证输入：** 一份假的原生记忆文件；一条和录好的会话原文矛盾的记忆；一条注入多次但从未被标"有用"的开场白条目。

**完成条件：** 原生记忆文件产生带来源路径的提议，文件本身字节不变；矛盾的记忆产生一条带原文定位的替换提议；没用上的条目被降级并有记录。

#### H8 证明它被用了，而且有用

**用户会看到：** 设置里能看到每种 Aeroric 能力在每个 Harness 上"该用的时候用了几成"；新的自动行为先以"候选"出现，能一键退回。

**现状：** 没有触发评测。唯一的数据是那次 Wiki 评测（16 个会话全部跳过），之后靠改服务器说明修复（`mcp_context.rs:209` 的注释，R4-3）。

**改法：**

1. **触发评测**（扩展 C8 第三步）：每个能力三到五条"该用"和"不该用"的提示，按 Harness 分别记录触发率、第一次成功调用的时间、无关调用数。改工具描述、技能描述、开场白用途说明之前和之后各跑一次。
2. **收益对照**：同一批录好的任务跑三种条件：关掉、内容打乱但长度相同、正常开启。只有"正常开启"明显好于另外两种，才算这个注入有用。自动召回（H4）、新沉淀的技能（[记忆技能](../../implemented/feature/2026-10-31-memory-skills.md)）、开场白里新增的段落都过这一关。
3. **分三档上线**：新沉淀的技能和新的自动注入先是"候选"（只记录本来会不会触发），再是"只挂描述"，证据够了才"默认生效"。任何一档都能一键退回。
4. 流程用假 Harness 跑通。真实 Harness 的评测按 §8 第 7 问：H2、H3 落地后做一次小规模的（Claude、Codex、OMP 各一轮），此前记 NOT RUN。

**验证输入：** 仓库里的评测集；一次描述修改前后的两次运行记录。

**完成条件：** 评测集在仓库里；一次描述修改能产出前后对比表；三档之间的切换各有一条测试。

### 4. 智能伙伴

owner 已确认按这个理解设计（§8 第 6 问）：伙伴不是又一个 Harness，而是宿主侧常驻的一个使用者，和各 Harness 用的是同一份能力。它在三个地方露面：每个会话的开场白（所以每个 Agent 都"认识"用户）、需要用户处理的事项队列、iOS 伴随端。它自己要调模型时走现有网关，要动手时走委派，不另开通道。本文不为它列分片。

### 5. 对其余批次的一条共同要求

总纲（[调研采纳路线](./2026-11-15-research-adoption-roadmap.md)）里 A–D 批每个会新增 Agent 可用能力的切片，完成条件加四行：

1. Agent 通过哪个入口用到它（工具名、技能名或开场白里的哪一句）；
2. 什么情况下它会被自动用上，靠模型判断还是靠事件；
3. 它在 §2 表里每个 Harness 的那一格是什么；
4. 至少三条"该用"、三条"不该用"的提示进 H8 的评测集。

只做了界面入口、Agent 自己用不到的能力，不算完成。

### 6. 顺序和工作量

H1 → H2、H3（补缝，OMP 先受益）→ H5 → H6 → H4、H7。H8 从 H2 起就开始积累评测集。

粗估：H1 1–2 天，H2 4–6 天，H3 2–3 天，H4 4–6 天，H5 4–6 天，H6 6–10 天，H7 4–6 天，H8 4–6 天，合计 29–45 个工程日。和第一版相比，H1 少了（大部分核对已做），H2 多了（OMP 真接线比只做命令行多），H5 少了（"看见"已有）。估算只基于静态阅读；待实测第 2、5 条的结论可能让 H5、H6 明显变化。

和总纲的关系（§8 第 5 问已定）：H1–H3 插在 C 批的 C8 之前；H4–H8 在 C 批之后，与 D 批并行。

### 7. 明确不做

- 不把某一家的记忆文件、配置文件当成 Aeroric 的存储，也不往里写。
- 不恢复写进用户 `~/.claude`、`~/.codex` 的 Hook 管线。
- 不解析终端界面文字来判断能力或子代理状态。
- 不为了统一而替换 Harness 自己的子代理、压缩、权限机制。
- 不在 H8 给出证据之前默认开启任何"每条消息都注入"的行为。
- 不自动批准：能力包里的事件处理和 MCP 服务器照常过信任判定和权限。

### 8. 需要 owner 拍板的事

**2026-11-16 owner 答复：七项全部按推荐选择。** 下面的 ➡️ 就是定下来的做法，实现模型不用再问。其中两项改动了已有决定，落地那一片时要原地更新持有该决定的笔记和 `docs/`：第 2 问放宽 B5-1 的"记忆档案不进用户消息"，第 3 问放宽路线文档 §6.4 的"原生记忆不索引"。第 7 问的授权只在 H2、H3 落地后生效，范围是一次小规模评测。这些答复定的是做法，不是开工授权；每片仍按总纲 §4 的流程单独写笔记。

核对之后，第 1 问的答复仍然成立，但适用范围变了：OMP 不再需要命令行兜底，它留给系统层和 MCP 都没有的 Harness（H2）。其余六项不受影响。

1. 接不上 MCP 的 Harness 用命令行加技能兜底（H2），会话进程环境里会有本任务的 token。➡️ 接受；token 范围不变（单任务、关会话即撤销）。
2. 既没有系统层也没有开场事件的 Harness，记忆档案能否带明显标记拼到第一条消息前（改动 B5-1 的决定）？➡️ 可以，但只作最后手段，界面上显示为"Aeroric 附加的上下文"而不是用户的话；有技能入口的优先用技能。
3. 整理时把各家原生记忆文件当候选证据读（H7 第 1 条，放宽 §6.4 的"不索引"）？➡️ 读，但产出一律走提议和确认，不直接生效，不回写。
4. 原生子代理只"看见、装备、给角色"，不接管（H5）？➡️ 是。
5. H 批排在哪？➡️ H1–H3 插到 C8 之前，其余在 C 批之后与 D 批并行。
6. "智能伙伴"具体指什么：§4 的理解对不对？➡️ 按 §4 理解，先不单独立项；如果指的是别的（比如一个有形象、能闲聊的常驻助手），请说明后另写笔记。
7. H8 的真实 Harness 评测何时授权？➡️ H2、H3 落地后授权一次小规模评测（Claude、Codex、OMP 各一轮），其余继续用假 Harness。

## Alternatives considered

- **所有 Harness 只走 MCP 一个入口。** 最强的理由是只维护一套协议，MCP 也是目前跨厂商最通用的；核对后发现连 OMP 和 ACP 都支持它。没有采用：MCP 只解决"能调用"，不解决"想得起来调用"；延迟加载工具的 Harness 上，模型只看得到一句服务器说明；开场白和到点发生的事也不是工具能承载的。
- **Aeroric 自己做一个 Agent 循环，把各家模型当纯模型用。** 最强的理由是完全控制上下文、工具和子代理，不再有适配问题。没有采用：等于放弃各家 Harness 的原生能力和它们的持续改进，也违背 Aeroric "各家 Harness 的原生工作台"的定位；调研里 pi 的报告已经指出嵌入第二套运行时会多出第二套持久化、审批和运行身份。
- **把 Aeroric 的东西写进各家的用户配置（`~/.claude`、`~/.codex`、`CLAUDE.md`）。** 最强的理由是最省事，而且用户在 Aeroric 之外直接用这些 Harness 时也能受益。没有采用：现有规则是不碰用户自己的配置；写进去就有了第二个权威，卸载和版本升级都说不清。旧 Hook 管线就是这么做的，后来专门写了清理逻辑。
- **用硬规则强制触发（"每个任务开始必须先查记忆和 Wiki"）。** 最强的理由是触发率立刻上去，评测好看。没有采用：记忆文章的数据显示固定规则会拖累强模型；简单任务被迫走一遍检索，用户会感到变慢，这本身就是一种割裂。
- **强迫模型用 `delegate`，禁用原生子代理。** 最强的理由是所有子工作都在侧栏可见、可接手，单写者规则好保证。没有采用：模型对自家派发工具更熟，禁掉会降低质量和速度；多数 Harness 也没有结构化的办法禁用它。
- **OMP 也只做命令行兜底，不接它的原生 MCP 和系统层。** 最强的理由是一套兜底覆盖所有"非 Claude、非 Codex"的 Harness，少写一个投影。没有采用：OMP 上游两样都有，接上之后它的子代理也能经父连接用到工具；命令行入口要靠模型读技能再敲命令，触发率天然比工具列表低。

## Acceptance criteria

- owner 对 §8 的七个问题给出答复（2026-11-16 已答复，全部按推荐）。
- §2 末尾的七条待实测每条有结论或带条件的 NOT RUN，结论写入 `docs/subsystems/agent-capabilities.md`。
- OMP 会话能读到记忆档案、能查记忆和 Wiki、能读交接清单（H2、H3）。
- 四族启动快照测试覆盖开场白、工具、技能、角色的投递位置。
- 评测集在仓库里，至少覆盖记忆、Wiki、委派三类能力的"该用"和"不该用"。
- 界面上不存在"这个 Harness 实际用不到、入口却亮着"的 Aeroric 能力。
- 真实 Harness 评测、真实窗口检查在未授权前记 NOT RUN。

## Risks

- **入口多了，同一能力被重复提供。** 模型可能在 MCP 工具和命令行之间来回试。H2 规定命令行技能只挂给没有工具入口的 Harness；H1 的表是唯一依据。
- **token 进了会话进程的环境。** Claude、Codex 现在只把 token 给 Context 服务器进程；OMP 的 `mcp.json` 展开和命令行兜底都要求它在会话进程的环境里，Agent 启动的任何子进程都读得到。范围限于单个任务，写操作仍过宿主的权限判断；H2 的测试要覆盖"会话关闭后 token 立即失效"。
- **开场白变长。** 每加一段都在吃每个会话的上下文，文章里也指出指令越长越不被遵守。总上限写死在 core，超了先砍指路部分；进账单让用户看得见。
- **自动召回注入了错的或过时的内容。** 默认关闭，过 H8 才开；每次注入带 id，用户能从回合上下文里看到并标"没用"。
- **原生子代理的事件各家不全。** 有的家族可能只有开始没有结束。按"结果未知"显示，不猜。
- **角色投影后权限说不清。** 原生子代理的权限由 Harness 决定（Codex 明说自定义代理不能覆盖父会话的权限）。投影只能收窄不能放宽，角色里的权限要求表达不了时宁可不投影。
- **上游变动和版本差。** Harness 的插件格式、Hook 事件、ACP 字段都可能改；OMP 的事实来自 18.13.1，Aeroric 的最低版本是 18.12.0。每个投影函数钉住上游版本，探测不到就退回 No，不沿用旧结论。
- **评测本身的成本和授权。** 三条件对照要跑三倍的量；真实评测要 owner 授权。先用小任务集，假 Harness 只能证明流程通，证明不了模型真的会触发。
- **本文的上游事实都来自阅读，没有运行。** Claude 的 sub-agents 页、Codex 的两页、ACP 的一页是经摘要读取的，可能漏掉限定条件；§2 的七条待实测没有做；doubleAI 一文只读了约 56%，引用仅限已读部分。
