# 外部 Agent 适配器契约(AP7 / P9-01)

外部 Agent 适配的基本要求是「每个 advertised 能力绑定具体 adapter/版本/边界,
抓 TUI 文本伪造协议被明确禁止」。本文是那条契约的落点。

## 1. 能力从哪来:探测,不是猜测

`aeroric_protocol::capability` 的设计前提是 **`v == 1` 从不蕴含能力**。所以:

- 来源只能是**结构化事实**:`adapters.capabilities` 里那份由
  `CapabilityProbe` 收集、`probe_agent_capabilities()` 翻译的 `AgentCapabilities`;
- 事实本身取自 core 内**钉在源码 commit 上**的方法表
  (`aeroric_core::agents::dsh_host::client::REMOTE_METHODS`,连同 `PACKAGE_VERSION`)——
  不是运行时截屏、不是终端文本匹配;
- 探不到就是 `false`,并附 `extra.unsupported` 列出**具体**缺哪个方法。
  宿主握手里的 `session-controls` 能力带 `note`,写清适配器名、版本与已广告的方法名。

**禁止**:解析 TUI 输出推断出「这个族大概支持 fork」。任何这类实现都违背本契约,
因为它把一个显示细节当成了协议事实。

## 2. 未广告 = 服务端拒绝

三道门,顺序不可颠倒:

| 层 | 行为 | 证据 |
|---|---|---|
| UI | 未广告的控件**不渲染** | `macos/.../AgentTools/AgentToolsViews.swift`(`store.offeredControls`) |
| 客户端 | 未广告的请求**构造不出** | `AgentToolsStore.makeRequest` 返回 `nil`;iOS `CompanionControlRequest.refusal` |
| 宿主 | 未广告的调用**被拒绝** | `SessionControls::authorize` → `ControlError::Unsupported` → `UnsupportedCapability` |

UI 隐藏只是体验层。**安全边界在宿主**:
`node scripts/native/check.mjs session-controls` 的 host 用例直接绕过 UI 调
`session.control` 并断言拿到 `UnsupportedCapability`。

方法与 `session.*` 字面量在 Swift/iOS/Rust 三处必须一致:宿主
`SessionCapability::method()`,iOS `CompanionSessionControl.rawValue`,macOS
`AdapterClient.CompanionControlName.rawValue`。

## 3. 端口画在哪

沿用项目既有的 app-owned port 模板(与 AP6 的 `PushTransport`、P3-05 的
`DshHostPorts` 同形):

| 关注点 | 归属 | 理由 |
|---|---|---|
| 能力探测与策略 | core(`aeroric_core::adapters`) | 纯逻辑,可单测,不碰 I/O |
| HTTP / 进程 / 麦克风 | 宿主端口 | core 不阻塞、不持有平台资源 |
| 展示与触发 | SwiftUI | 只读 store;`AgentToolsStore` 不做裁决 |

## 4. 各家能力现状(如实记账,2026-10-13)

| 能力 | DSH 上游 | 本仓库暴露 |
|---|---|---|
| `plan` | 无 `session.plan` 方法 | 未广告(UI 不显示) |
| `compact` | 无 | 未广告 |
| `fork` | `session.fork` | 已接(`session.control` → 宿主) |
| `revert` | 无方法;策略层已实现(会话/工作区分流 + 预览门槛) | 策略可用,上游调用待适配器 |
| `share` | 无方法;脱敏导出已实现 | 本地脱敏可用,公开上传需 endpoint+确认 |
| 子代理 | `subagents.list` / `subagents.prompt` / `subagents.interruptByParent` | core API 已存在;宿主面逐步接 |
| goal | `goals.create|edit|pause|resume|complete|clear|get` | 同上 |

未广告的项**不从清单里删除、也不放占位按钮**:它们在
`extra.unsupported` 与本节表里具名存在,等适配器补齐后自动变为可用。

## 5. Aeroric 工具接到了哪些 Harness（C9，2026-11-20）

Aeroric 的 Context 工具（记忆、Wiki、黑板、委派、交付物、参考库、专家，共 21 个，表在 `aeroric_protocol::tools::AERORIC_TOOLS`）由执行器在每次启动会话时交给 Harness。逐家事实：

| Harness | 本机 | 怎么接 | 远端 |
|---|---|---|---|
| Claude Code | 有 | `--mcp-config` 加 `--allowedTools mcp__aeroric_context` | 没有 |
| Codex | 有 | `thread/start` 的 `config.mcp_servers`，标 `required` | 没有 |
| DSH | 有 | ACP `session/new` 的 `mcpServers`（stdio） | 不支持远端 |
| OMP | 有 | 受管目录（`PI_CODING_AGENT_DIR`）的 `mcp.json` 声明服务器，token 写成 `${AERORIC_CONTEXT_TOKEN:-}`、只放进这次进程的环境；工具的放行规则在受管目录的 `aeroric-context-tools.yml`，用 `--config` 传入（H2） | 没有 |

- 远端会话都没有：工具服务器要能连回这台 Mac 的宿主，跨设备的身份和回执由批次 D 处理。
- 能力表（`session.capabilities`）的"Aeroric 工具"（`contextTools`）按上表给出；`executor_tests.rs` 的 `the_capability_table_matches_who_gets_the_context_server` 核对它与执行器的路由一致。
- 每次启动实际给了哪些工具、没给的原因，记在任务的工具清单里（C8），会话信息的"工具"一行显示。
- OMP 的审批模式会拦 MCP 工具（它们一律算"会写"，`always-ask` 下要问）。Aeroric 用 `--config` 叠加层按工具名给出 `tools.approval.mcp__aeroric_context_<工具>: allow`，只放行这 21 个，和 Codex 只给这一个服务器设 `approve` 一样，不改整个会话的审批模式，也不动用户的 `config.yml`。
- App 里的"新委派"、角色库、黑板面板、记忆页都由 App 直接调宿主，不依赖会话里的 Agent 有没有工具。依赖工具的是 Agent 自己发起的委派、检索和记忆提议，这部分在没有工具的会话里不会发生，会话信息里写明原因。
- 记忆档案随开场白交出：Claude、Codex、本机 OMP 放在系统层；DSH 和远端 OMP 没有系统层，放在第一条消息前的“Aeroric 附加的上下文”里（见 §6）。

## 6. 每个 Harness 用哪些入口接 Aeroric（H1，2026-11-20）

能力表（`session.capabilities`）里有六个入口项，顺序见 `Feature::SURFACES`：系统层（`surfaceInstructions`）、工具（`contextTools`，即 §5）、技能（`surfaceSkills`）、事件（`surfaceEvents`）、子代理（`surfaceSubagents`）、命令行（`surfaceCli`）。

本机各家：

| Harness | 系统层 | 工具 | 技能 | 事件 | 子代理 | 命令行 |
|---|---|---|---|---|---|---|
| Claude Code | 有：`--append-system-prompt` | 有 | 有：会话插件或受管 home | 没有：会话插件可带 `hooks/hooks.json`，未放（H4） | 有：`Task`、`Agent` 工具与 `task_*` 事件 | 不挂：已有工具 |
| Codex | 有：`thread/start` 的 `developerInstructions` | 有 | 有：项目 `.agents/skills` 下带标记的副本 | 没有：可经 `thread/start` 的 `config` 按次下发，未接（H4） | 有：`subAgentActivity`、`collabAgentToolCall` | 不挂：已有工具 |
| OMP | 有：`--append-system-prompt`，内容在会话自己的 0600 临时文件里，进程退出时删除 | 有 | 有：受管 home | 没有：上游可按次传 `--hook`，未传（H4） | 有：启动时订阅子代理事件 | 不挂：已有工具 |
| DSH | 没有：DSH 的 ACP 没有声明 `systemPrompt` | 有 | 有：受管配置目录 | 没有：没有能从外部配置的 Hook | 没有：ACP 不发子代理事件 | 不挂：已有工具 |

远端：子代理同本机；系统层 Claude、Codex 同本机（有测试断言），OMP 没有（开场文件在这台 Mac 上，远端读不到）；工具、技能、事件都没有；DSH 不支持远端。

**依据从哪来。**

- 会话每次启动（含恢复）记下这次各入口实际怎样，存在 `<data>/surfaces/<任务>.json`（每任务保留最近 20 次），`session.capabilities {taskId}` 的 `surfaces` 返回最近一次；会话信息的"入口"一行显示它（接通几个、是哪几个，点开逐项写状态和原因）。
- 系统层、工具、技能三项按这次启动实际发生的事填：驱动收下了开场内容（记字数）、收下了工具服务器、挂了几个 Aeroric 自己的技能（内置和记忆沉淀的，不算用户自己的）。这次没有开场内容要给时，沿用能力表那一格并写明"这次没有开场内容要给"。
- 事件、子代理、命令行三项启动时不涉及，取能力表那一格。表里每个"有"都有对应的驱动测试：Claude 的 `Task` 映射（`claude/map_tests.rs`）、Codex 的 `collabAgentToolCall`（`codex/driver_tests.rs`）、OMP 的订阅与生命周期映射（`omp/driver_tests.rs`、`omp/map_tests.rs`）。
- `executor_tests.rs` 的 `the_kept_surfaces_match_what_the_launch_was_given` 与 `a_claude_launch_hands_what_its_surfaces_say` 用假 Codex、假 Claude 进程核对：记下的"有"与启动参数里实际带的 `developerInstructions`、`--append-system-prompt`、MCP 配置一致，字数一致。

**开场白（H3，2026-11-20）。** 每次启动（含恢复）交给 Harness 系统层的内容由 `sessions/live/opening.rs` 拼成，三段依次是：

- 用途说明（`# Aeroric 能做什么`，上限 480 字）：Aeroric 提供什么、什么时候用，不写"每次必须先查"。接了工具的会话附上工具清单那一句，和 MCP 服务说明是同一份文字（`opening::tool_uses`）；没接工具的写明没有。
- 记忆档案（`# Aeroric 记忆`，上限 1200 字）：原有的那份，其中每条照旧计入"已注入"。
- 指路（`# Aeroric 里能查的`，上限 600 字）：项目 Wiki 页标题和委派角色名，只给标题，放不下的写"等 N 个"；只给接了工具的会话。

合计上限 2280 字，超出时先去掉指路。有交接简报时，简报接在开场白后面，中间用 `---` 分隔。

回合账单的"带入的上下文"按段计入开场白（`opening::bill_parts`）：用途说明和指路算"说明"，记忆档案算"记忆"，交接简报算"其他"，来源都写"Aeroric 开场白"。Codex 的开场白是开发者消息里单独的一段；Claude 的在系统提示末尾，从用途说明的标题起拆出来，前面的仍算系统提示。

**没有系统层时开场内容去哪（H3 第三步）。** DSH 和远端 OMP 的驱动不收系统层。开场白（有交接简报时连同简报）装进一个信封，附在第一条消息前（`opening::attach`）：首行是 `[Aeroric 附加的上下文] 来自：Aeroric · … · 背景信息：…`，正文按来源信封的规则转义，末行是 `[Aeroric 附加的上下文结束]`，后面才是用户的话。App 认出这个信封，在用户消息上方折叠显示为“Aeroric 附加的上下文”，不当作用户的话；同意记录也只看信封之后的字。恢复会话时没有消息可附，开场白就没给出，入口记录里写明。这两家的会话文件不记基础部分，回合账单本来就不拆“带入的上下文”，所以这一段也不在账单里。

**命令行入口（H2 第二步，2026-11-20）。** `aeroric-cli --data-dir <数据目录> ctx <工具名> [--json '<参数>' | --json -]` 调用 Aeroric 上下文工具。它就是 MCP 服务的 `tools/call`：同一张工具表、同一个宿主方法、同一个令牌（`AERORIC_CONTEXT_TOKEN`），输出同样的结果（`structuredContent.status`），退出码 0 成功、2 参数不对或没有令牌、3 连不上、4 被拒绝或结果不明；`ctx --help` 由工具表生成。配套的内置技能 `aeroric` 教 Agent 用这个命令，只挂给没有 Aeroric 工具的 Harness（`builtin_skills::for_harness`），有工具的不挂，免得同一能力两个入口抢着被调用。现在本机四家都有工具，所以这个技能不挂给任何一家，技能目录里写明原因。给这种 Harness 的启动器要在环境里放 `AERORIC_CLI`、`AERORIC_DATA_DIR` 和令牌（`ContextServer::shell_env`），目前还没有这样的 Harness。

**角色库作为原生子代理（H5 第一步，2026-11-20）。** 挂技能时，把角色库里适合当原生子代理的角色写成各家自己的子代理定义（`extensions/native_roles.rs`）：

| 去处 | 位置 | 模型看到的名字 |
|---|---|---|
| Claude 直连 | 会话插件的 `agents/<id>.md` | `aeroric:<id>` |
| Claude 走 Agent 配置 | 受管目录的 `agents/` | `aeroric-<id>` |
| OMP | 受管目录（`PI_CODING_AGENT_DIR`）的 `agents/` | `aeroric-<id>`（OMP 自带 `reviewer` 等，避免重名） |
| Codex、DSH | 不投影 | — |

- 只投影在原生子代理里说得通的角色。要换 Harness 或模型、要隔离的可写副本、带验收或预算的角色不投影，仍然只能经 `delegate`；专家角色（要另一个更强的模型）也不投影。内置角色里现在只有“调研员”。
- 只读角色（权限 `plan`）用工具白名单限住：Claude 是 `Read, Grep, Glob, WebFetch, WebSearch` 加 Aeroric 的只读工具和 `board_write`（黑板是 Aeroric 自己的记录，委派出去的只读子会话也能写）；OMP 是 `read, grep, glob, ast_grep, think`，MCP 工具由 OMP 自己附加。可写角色不写白名单，沿用父会话的工具，原生子代理本来就不会比父会话多。
- Codex 的自定义代理不能收窄父会话的沙盒，只读角色投影过去会变成可写，所以不投影。
- 定义文件带标记 `<!-- aeroric-role -->`，只替换和删除带标记的；同名但不带标记的文件（用户自己的）不动。
- `delegate` 的说明写明分工：要换 Harness 或模型、要用户能接手、要跑很久、要隔离的可写工作区、要按验收检查时用它；同一会话里快速分头检索用原生子代理。

**原生子代理和委派列在一起（H5 第二步，2026-11-21）。**

- 进度页的“委派”一节先列这个会话的原生子代理，标明“由 Harness 管理，Aeroric 只能看”，后面是经 Aeroric 委派的子会话，标明“由 Aeroric 管理”。没有原生子代理时，这一节和原来一样。
- 原生子代理来自会话的 `Subagent` 更新：Claude 的任务事件、OMP 的子代理事件、Codex 子线程的开始和结束（会话卡片上的进度行照旧）。Codex 进程退出时，还在跑的子线程记为已取消。后台命令不算子代理，仍只在会话信息里。
- 原生子代理行没有打断按钮：三家都没有按子代理打断的结构化方法。

**黑板写入标明子代理（H5 第二步）。** Context 服务器分不出一次调用来自主会话还是它的子代理：子代理和主会话共用同一个 MCP 连接和 token。所以按方案的退路，由子代理自报名字：

- `board_write` 多一个可选参数 `subagent`，说明里写“你是被派出来的子代理时填自己的名字”。
- 投影出去的角色定义正文末尾写明该填的名字（`aeroric:researcher` 或 `aeroric-researcher`）。
- 条目的作者仍是会话；条目和变更记录另记 `subagent`，名字最多 60 个字符。用户的写入不带它。
- 黑板上这一行显示成“证据 · 子代理 aeroric-researcher：……”，悬停说明是“会话 X 的子代理 Y 写的”。
- 这是自报，不是认证，只用来显示，不影响任何权限。

**"子会话"（`delegateChild`）的含义。** Aeroric 能把这一家的会话作为委派的子会话启动，并把结果送回上级。它不表示这一家能自己发起委派，那要看工具这一项。

### 七条待实测的结论（H1 第二步，2026-11-20）

[各 Harness 原生融合](../../.agents/notes/proposed/architecture/2026-11-16-harness-native-capabilities.md) §2 末尾列了七条静态阅读定不了的事。下面逐条给出结论。依据是本地克隆的上游源码：OMP `v18.12.0` 标签、Codex `fe8e540a`（2026-11-11）、DSH `fcc0ad4621`（0.2.2-rc.1）、ZCode `a9fe401e`（3.17.2）；另外读了 ACP 规范原文（session-setup 页），看了本机 Claude Code 2.1.327 的 `--help`。凡是要跑真实 Harness 才能定的，记为 NOT RUN 并写明条件。

1. **OMP 18.12.0 有没有这三样。** 都有，不用抬高最低版本：
   - `--append-system-prompt`：`cli/flag-tables.ts`，接受文字或文件；
   - agent 目录下 `mcp.json` 的 `${VAR}`、`${VAR:-默认}` 展开：`discovery/builtin.ts` 加载时调 `expandEnvVarsDeep`，规则在 `discovery/helpers.ts`。没设的 `${VAR}` 原样保留，`${VAR:-}` 才会变成空串；
   - `agents/*.md`：`task/discovery.ts` 读 agent 目录下的 `agents`。
2. **Codex 能否经 `thread/start` 的 `config` 下发 Hook 和自定义代理。** 源码上都能：
   - 请求里的 `config` 落在会话层（`SessionFlags`）；
   - Hook 能从会话层发现，但会话层的 Hook 不算受管 Hook，要有信任哈希才运行。信任状态会从用户层和会话层读（`hooks/src/config_rules.rs`），所以同一份 `config` 里可以连同 `hooks.state.<键>.trusted_hash` 一起给；
   - 自定义代理用 `agents.<角色名>` 声明，带 `description` 和 `config_file`（绝对路径），文件可以放在 Aeroric 自己的目录里，不碰 `~/.codex`；
   - 实际是否生效：NOT RUN。条件是用真实 Codex 开一个会话，下发一个 `SessionStart` Hook 和一个角色，看 Hook 是否运行、子代理能否按角色生成。
3. **Claude 会话插件里的 `hooks/hooks.json`、`agents/` 在无界面模式下是否生效。** NOT RUN：Claude Code 不开源，要跑真实会话才能确认。
   - 本机 2.1.327 的 `--help` 里有三个按次生效的参数：`--plugin-dir`（只对这次会话加载插件）、`--agents`（JSON，`--print` 模式下也可以给文件路径）、`--settings`（再加载一份设置文件或 JSON）；
   - 所以除了插件目录，还有两条不碰用户 `~/.claude` 的路：经 `--settings` 按次给 Hook（设置里本来就能写 Hook，按次传入是否生效未实测），经 `--agents` 按次给角色。帮助里没有说插件里的 Hook 和代理在 stream-json 模式下是否生效；
   - Aeroric 没有设 Claude Code 的最低版本。
4. **原生子代理能不能调到 `aeroric_context`，用的是不是父任务的 token。**
   - Codex：能，用的是父任务的 token。子代理的配置是父回合配置的副本（`core/src/agent/child_config.rs`），`mcp_servers` 里的 Aeroric 服务器连同环境变量里的 token 一起带过去，开场内容（`developer_instructions`）也一样；
   - OMP：子代理借用父会话的 MCP 连接（上游 `mcp-runtime-lifecycle.md`）。H2 第一步已给本机父会话接上工具，子代理的真实调用 NOT RUN；
   - Claude：NOT RUN。官方文档说子代理继承主会话的 MCP 工具，要用真实会话让子代理调一次 `aeroric_context` 才能确认；
   - 三家的真实调用都没跑。
5. **DSH。**
   - 工具：不需要改受管配置，ACP 的 `session/new` 已经带上（§5）；
   - 系统层：DSH 0.2.2 的 ACP 只声明了 `mcpCapabilities`、`promptCapabilities` 和 `sessionCapabilities` 的 close、list、resume，没有 `systemPrompt`；它用的 ACP SDK 0.25.1 也没有这个字段。所以没有入口；
   - 子代理：ACP 只发消息、思考、工具调用、用量和配置更新，不发子代理事件；子代理在 DSH 进程内部管理（`subagents` 服务）；
   - Hook：没找到能从外部配置的 Hook，DSH 的扩展点是同进程插件。
6. **远端。**
   - 系统层：远端 Claude 用的是同一个会话驱动，执行器测试 `a_claude_launch_hands_what_its_surfaces_say` 已断言它带 `--append-system-prompt`；远端 Codex 同理；
   - 技能：Aeroric 不给远端会话挂技能（`before_codex_session` 对远端直接返回，技能挂载记录也不记）。
7. **ZCode 怎么驱动；ACP 的 `systemPrompt`。**
   - ZCode 没有 ACP。无界面方式是每轮一次进程（`--prompt`、`--output-format stream-json`，用 `--resume` 接上一轮），另有桌面端用的 `--stdio` 协议。驱动没建；
   - ACP 规范原文：`session/new`、`session/load`、`session/resume` 可以带可选的 `systemPrompt`，前提是 Agent 声明了 `sessionCapabilities.systemPrompt`；`mode` 必填，有 `append`（追加）和 `override`（替换，需另外声明 `mode.override`）两种；
   - 规范要求所有 Agent 都支持 stdio 的 MCP 服务器。

## 7. 能力包（H6，2026-11-21）

一个能力包在 Aeroric 里描述一次，每次启动按各家的原生形式投影过去。包放在 `<数据目录>/packs/<id>/pack.json`，技能文件夹在它旁边；清单格式是协议里的 `packs::Pack`，有五类内容：技能、MCP 服务器、角色、命令、事件处理。

**信任。** 内置包或官方市场的包在已知版本上可信，其余的要用户确认来源（清单里的 `confirmed`）。没有确认的包，MCP 服务器和事件处理都不投影，因为它们会运行命令。

**第一步（2026-11-21）投 MCP 服务器**，四族都接：

| Harness | 写到哪里 |
|---|---|
| Claude | 本次启动的 `--mcp-config`，和 Aeroric 工具服务器在同一份配置里。配置写进本进程的私有文件（0600），进程结束时删掉，不放命令行参数，因为里面可能有服务器的密钥 |
| Codex | `thread/start` 的 `config.mcp_servers` |
| OMP | 受管目录的 `mcp.json`。Aeroric 在旁边的 `aeroric-pack-servers.json` 里记下自己写了哪些，只替换和删除这些；同名但不是 Aeroric 写的服务器原样保留 |
| ACP（DSH 等） | `session/new`、`session/resume` 的 `mcpServers` |

以下几种服务器不投影，每一项都记下原因：

- 包或服务器已停用；
- 包未受信任；
- 远端会话：命令在这台 Mac 上，远端跑不了；
- 名字不合规，或和 Aeroric 自己的 `aeroric_context` 重名；
- 命令为空；
- 名字已被排在前面的包占用（按包 id 排序，先到先得）。

包里的工具和其他服务器的工具一样，要过权限；只有 Aeroric 自己的工具事先放行。

**记在哪里。** 每次启动的工具清单（`session.capabilities` 的 `tools`）多一个 `packs`，逐项写给了没有、给到哪里，或为什么没给。读不了的包（清单坏了、id 和文件夹名不一致）跳过，并写进日志。

**第二步（2026-11-21）投其余四类**，和内置技能在同一次挂载里放好：

| 内容 | Claude 直连 | Claude 走配置 | Codex 直连 | Codex 走配置 | OMP | DSH |
|---|---|---|---|---|---|---|
| 技能 | 会话插件 `skills/` | 受管目录 `skills/` | 项目 `.agents/skills` 的带标记副本 | 受管目录 `skills/` | 受管目录 `skills/` | 受管目录 `skills/` |
| 命令 | 作为技能，同上（Claude 里用 `/aeroric:<名字>`） | 同左 | 同左 | 同左 | 同左 | 同左 |
| 角色 | 插件 `agents/`（`aeroric:<id>`） | 受管目录 `agents/`（`aeroric-<id>`） | 不投影 | 不投影 | 受管目录 `agents/`（`aeroric-<id>`） | 不投影 |
| 事件处理 | 插件 `hooks/hooks.json` | 不投影 | 不投影 | 不投影 | 不投影 | 不投影 |

- 技能先按内容复制进 Aeroric 的技能仓库（`builtin-skills/packs/<包>/<名字>-<内容摘要>/`），再像内置技能一样链接或复制过去。这样旧的链接能认出是 Aeroric 的并清掉，内容改了就换一个文件夹。命令写成一个 `SKILL.md`，同样处理。
- 同名时的先后：你自己的技能优先；内置技能的名字包不能占；两个包重名，先到先得。
- 角色并进原生子代理用的角色库，规则同 H5：id 和角色库重名的不进，要换 Harness、要隔离副本、带验收或预算的不投影。包里的角色只作为原生子代理，不进 `delegate` 的角色库。
- 事件处理只投给直连的 Claude，写进会话插件，只放 Claude 有的事件（`sessionStart`、`preToolUse` 等），工具事件匹配所有工具。
  - Claude 走配置：没有会话插件。
  - Codex：按会话下发 Hook 没在真实会话里验证过（见下面待实测第 2 条）。
  - OMP：事件处理是 JS/TS 模块，不接受命令。
  - DSH：没有入口。

  以上几种都在清单里写明原因。
- 远端会话什么都不投。

**OMP 里同名的服务器谁生效。** OMP 会自己读用户其他 Harness 的 MCP 配置（Claude、Cursor、Windsurf、Gemini 等），按来源优先级去重：名字相同，或命令和参数相同，都只留优先级高的那个（上游 `capability/index.ts`；`discovery/*.ts` 的 `PRIORITY`，v18.12.0 起未变）。Aeroric 写进的受管目录 `mcp.json` 属于 OMP 自己的配置，优先级 100，最高。所以包里的服务器会盖过从其他 Harness 导入的同名服务器，只有项目里 `.omp/mcp.json` 的同名服务器能盖过它。工具清单里 OMP 一栏写的就是这句。

**管理（第三步，2026-11-21）。** Agent 配置 → 能力包：

- 列出每个包和它在六种去处（Claude、Claude 走预设、Codex、Codex 走预设、OMP、DSH）各给了什么，没给的写明原因；
- 添加或编辑一个 MCP 服务器：它自成一个包，你写的，算已确认；环境变量里的密钥回显为掩码，原样保存就保留原值；
- 导入能力包文件夹：来源一律记成那个文件夹，不管清单里写的是什么；先是“待确认来源”，确认后 MCP 服务器和事件处理才会随会话启动；
- 停用、撤销确认、删除（文件夹移到废纸篓）；
- 读不了的文件夹单独列出原因。

接口是 `packs.list`、`packs.saveServer`、`packs.import`、`packs.setEnabled`、`packs.setConfirmed`、`packs.remove`。

## 8. 到点就发生的事（H4，2026-11-21）

宿主是各 Harness 的上级进程，结构化事件都经过它，所以这些事不靠 Harness 自己的 Hook，四族一视同仁。

| 时机 | 宿主做什么 |
|---|---|
| 会话开始 | 投开场白（H3） |
| 用户发出一条消息 | 按消息在本地找相关的记忆和 Wiki；默认只记录，不附加 |
| 回合结束 | 更新检查点、抽取记忆提议；改过文件的回合，立刻给引用这些文件的 Wiki 页标“可能过时” |
| 原生子代理启动、结束 | 和委派列在一起（H5） |

**改了文件，Wiki 页当场标“可能过时”（第一步）。**

- 回合结束、改动快照做完时，拿这个回合改过的文件（相对仓库顶层），和每页引用的代码文件（相对项目文件夹）比对。路径相同，或改过的路径以“/引用路径”结尾，就算命中；命中的页记下文件、会话和时间，存在项目 Wiki 库旁的 `changed-since-compile.json`。
- 判断和编译时一样（引用的代码和生成时那一版不同），只是提前到回合结束，不用等下次编译。下次编译会重新读所有文件，并清掉这些标记。
- `wiki.page` 多返回 `changed`，Wiki 页标题旁显示“可能过时”，悬停写明是哪些文件。Agent 用 `read_wiki` 读到这一页时，正文前多一行 `> 可能过时：这页生成后，… 在会话里改过，还没重新生成。`

**按消息召回（第二步，2026-11-21）。**

- 用户的消息经宿主送进各家之前（`send_user`），执行器在本地找（不调模型），四族的决定一样：
  - 记忆：从消息取关键词（三个字符以上的英文或标识符词，相邻两个汉字成一个词，去掉“怎么”“一下”这类常用词），用全文检索找候选；和消息至少共有两个关键词的才算命中（六个字符以上的标识符算两个），按共有多少排，最多 3 条；
  - Wiki：页面标题出现在消息里的，最多 1 页；只看 Agent 能读 Wiki 的项目；
  - 少于 6 个字的消息（“继续”“好的”）不找。
- 四种模式（`recall.mode`），即下面“三档上线”里 `recall` 的档位：
  - `off`：什么都不做；
  - `candidate`（默认，候选）：照常判断并记录，什么都不附加；
  - `described`（只挂描述）：命中时只附出处，即记忆 id 和前 16 个字、Wiki 页标题，写明用得上再用 `memory_recall` 或 `read_wiki` 取；
  - `on`（默认生效）：命中时把指路装进“Aeroric 附加的上下文”信封，放在消息前。说明写“Aeroric 按这条消息自动找到的相关记忆和 Wiki”，正文列记忆 id 和原文、Wiki 页标题和 slug，并写明不一定相关。
- 默认不附加，原因是注错的东西会污染每个回合；H8 证明有用之前，只积累 `candidate` 的记录。
- 每条消息的决定记在 `<数据目录>/recall/<任务>.jsonl`，`recall.decisions {taskId}` 可读。
- 这种查看不算记忆的“被检索”，不改记忆的使用统计。

**三档上线（H8 第二步，2026-11-21）。**

- 自动行为分三档，能关的另有“关”：
  - 候选：照常判断，记下本来会不会触发，什么都不改；
  - 只挂描述：告诉 Agent 有这个东西，用不用由它；
  - 默认生效：直接起作用。
- 现在有档位的两类：
  - 按消息召回（`recall`），默认候选，四档的含义见上；
  - 每个学到的技能（`skill:<名字>`），默认只挂描述，也就是用户采纳后照常挂载。候选时不挂载；默认生效时照常挂载，开场白的指路里多一行“学到的技能（遇到相关问题先用）”。学到的技能最低到候选，要关掉，用技能本身的停用。
- 每次换档都记下换之前的档位和时间（最多 20 次）。“退回”回到上一次换档之前的档位；没有记录时降一档。
- 每条用户消息（少于 6 个字的除外）都给召回和候选档的技能各记一次判断：召回记有没有命中；技能记它的描述和消息有没有至少两个共同关键词，召回关着也照记。
- 档位和计数存在 `<数据目录>/rollout/state.json`。接口：`rollout.list {}` 列出召回、每个学到的技能和其他动过的；`rollout.set {id, tier}` 换档，`tier` 取 `off`、`candidate`、`described`、`default`；`rollout.revert {id}` 退回。

## 9. 具名阻塞

- `GAP-AP7-EXTERNAL`:ACP 对端、OAuth 真实账号、媒体 provider 的线上验证需要用户
  授权与凭据。代码侧已实现 fail-closed 语义(版本交集、越界目录拒绝、不支持音频即
  拒绝整条消息、沙箱未就绪不执行),但**未经真实对端验证**。
- macOS 麦克风/屏幕录制/辅助功能的**实际授权**由用户在系统设置完成;
  原生端只提供指路与签名不稳定时的重新授权提示。
