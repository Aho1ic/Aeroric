# Agent Note: H1 第一步：每次启动记下各入口实际怎样，能力表补齐入口项

Status: implemented

## Problem

[各 Harness 原生融合](../../proposed/architecture/2026-11-16-harness-native-capabilities.md) 的 H1 要回答"这个 Harness 通过哪些入口用 Aeroric"。入口共六个：系统层、工具、技能、事件、子代理、命令行。每一格说"有"，都要能指到启动时真实发生的事，或者指到一条测试，不能只凭家族名。

按总纲，H1–H3 应排在 C8 之前，实际顺序是 C 批先做完了。C8 已经有每次启动的工具清单，C9 已经有能力表的"Aeroric 工具"一项。

动手前（`main@5b66207c`）读代码：

- **开场内容。** `set_instructions` 的返回值是"这次启动有没有接上系统层"的唯一证据，但没有存下来：
  - 没有交接简报时直接丢弃（`executor.rs` 的 `hand_handoff`）；
  - 有简报时只用它决定拼不拼到消息前。
- **各驱动谁接受开场内容。** Claude、Codex 的驱动收下；ACP（DSH）和 OMP 用默认实现，返回"不收"。
  - 远端 Claude 用的是同一个 `ClaudeSession`，远端 Codex 用同一个 `CodexSession`，所以远端也有系统层。这一点之前没有断言。
- **恢复会话时的一处丢失。** Harness 没有系统层、又没有消息可附时，交接简报直接没了（`prompt.map(...)`），之前没有任何记录。
- **工具。** C8 的 `Injection` 已经区分接上、远端、不支持、宿主没准备。
- **技能。** C7 的挂载记录列出了这次给的技能，但用户自己的技能也混在里面。
- **子代理。** 三家都有映射和测试：
  - Claude 的 `Task`/`Agent` 工具；
  - Codex 的 `collabAgentToolCall`；
  - OMP 启动时订阅子代理事件。

  DSH 没有接。
- **事件（Harness 自己的 Hook）和命令行。** 哪家都没接。
- **`DelegateChild` 没有文档注释。** 方案担心 OMP 那一格不实。C9 已把它改成"有差异"并写明原因，含义还没写下来。

## Decision

本片分步做。这是第一步：协议、记录、能力表和测试。第二步逐条核对方案 §2 末尾的七条待实测，第三步在会话信息里显示。

**能力表加五个入口项，工具复用 C9 的 `contextTools`。**

- 五个新项是 `surfaceInstructions`、`surfaceSkills`、`surfaceEvents`、`surfaceSubagents`、`surfaceCli`。
- 方案原本列了 `SurfaceTools`，但它和 C9 的 `ContextTools` 是同一件事。方案自己要求"不要做出第二套清单"，所以不另加。
- `Feature::SURFACES` 定下六项的顺序。

各家取值见 `docs/subsystems/agent-capabilities.md` §6：

- Claude、Codex 有系统层；OMP 没有（H2），DSH 没有。
- 技能本机都有，远端没有。
- 事件：Claude、OMP 没有，未接（H4）；Codex、DSH 还没核对。
- 子代理：Claude、Codex、OMP 有，DSH 没有。
- 命令行：都没有（H2）。

**每次启动记下入口记录 `SurfaceLaunch`**，存在 `<data>/surfaces/`，与工具清单、技能挂载同一套按任务保存的方式。`session.capabilities {taskId}` 返回 `surfaces`。

- **系统层**按这次实际情况填。`hand_handoff` 返回 `Opening`，有四种情况：
  - 没有要给的；
  - 收下了，记字数；
  - 不收，简报放进了第一条消息；
  - 不收，又没有消息可附，简报没给出。
- **工具**按 `Injection` 填。
- **技能**按这次挂了几个 Aeroric 自己的技能（内置和记忆沉淀的）填，用户自己的不算。
  - `MountLog::note` 改为返回这个数。
  - 能力表那格是"有"、但这次一个都没挂时，写"没有"并说明原因。
- **事件、子代理、命令行**启动时不涉及，取能力表那一格。
- 这次没有开场内容要给时，系统层也取能力表那一格，并在说明前加"这次没有开场内容要给"。

**"有"的依据**：

- 系统层和工具：两条执行器测试，分别用假 Codex 和假 Claude 进程，核对记录与启动参数一致：
  - `developerInstructions` 和 `--append-system-prompt` 在不在、字数是否相同；
  - MCP 配置在不在；
  - 远端 Claude 有系统层、没有工具和技能。
- 子代理：表里三个"有"指向已有的驱动测试，`surfaces_tests.rs` 里写明对应关系，并锁定这几格的取值。

**`DelegateChild` 写清含义**：Aeroric 能把这一家的会话作为委派的子会话启动，并把结果送回上级。它不表示这一家能自己发起委派，那要看工具这一项。C9 给 OMP 定的"有差异"符合这个含义，不用改。

## Alternatives considered

**按方案另加 `SurfaceTools`。** 最强的理由是六个入口项名字整齐。没选：它和 `ContextTools` 永远同值，两项并存会让人问"这两个有什么区别"，C9 的一致性测试也得测两遍。

**入口记录合进 C8 的工具清单 `ToolLaunch`。** 最强的理由是每次启动只写一个文件。没选：工具清单记的是一项项工具，入口记录记的是六个入口，两者的读取方不同，合在一起会让工具清单背上与工具无关的字段。分开存，按任务保存的方式是共用的。

**系统层只看驱动类型，不看这次是否真给了。** 最强的理由是简单，不用改 `hand_handoff` 的返回值。没选：方案明确要求依据是启动时真实发生的事；而且只有记下这次的结果，才看得到"恢复时简报没给出"这种情况。

**没有开场内容时调用一次 `set_instructions("")` 试探。** 没选：Claude 会因此带一个空的 `--append-system-prompt`，为了记账改变了启动参数。

## Consequences

- **收益**：
  - 能说清每个 Harness 通过哪些入口用 Aeroric，"有"的都能指到启动记录或测试；
  - 恢复会话时交接简报没给出的情况第一次被记下；
  - 远端 Claude 的系统层有了断言；
  - `delegateChild` 的含义写清了。
- **局限**：
  - 方案 §2 末尾的七条待实测还没做，留给第二步；事件这一项里 Codex、DSH 仍是"还没核对"；
  - 会话信息里还看不到入口记录，留给第三步；
  - OMP 的系统层和工具、命令行入口都等 H2，事件等 H4。
- **验证**：
  - 核心层新增 7 个测试：入口记录 5 个，执行器 2 个（假 Codex、假 Claude，含远端）；
  - 已有的"每个 Harness 和位置都回答每一项、非'有'的都附原因"的测试覆盖到新项；
  - 没有界面改动。
