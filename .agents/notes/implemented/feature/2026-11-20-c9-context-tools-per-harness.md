# Agent Note: C9：各 Harness 的 Aeroric 工具接入，接了的确认、没接的如实写明

Status: implemented

## Problem

[批次 C 方案](../../proposed/feature/2026-11-15-adoption-c-memory-gateway-skills.md) C9 要求：OMP、DSH 的会话里，Aeroric 工具要么能用，要么写明"这个 Harness 还不能接入"和原因；依赖工具的功能，在不能用的 Harness 上入口是灰的。完成条件是不存在"界面上有入口、但这个 Harness 实际调不到工具"的功能。

方案写于 2026-11-15，之后 [各 Harness 原生融合](../../proposed/architecture/2026-11-16-harness-native-capabilities.md)（11-16）查清了 OMP 上游支持 MCP，并把 OMP 的注入划给它的 H2。C9 只剩三件事：DSH、远端，以及如实置灰。

动手前（`main@f1c9de82`）逐处读代码：

- **DSH 其实已经接上了。**
  - 本机 DSH 走 ACP（`Route::Acp`，方言 DSH）。
  - ACP 驱动的 `set_context_server` 接受服务器，并在 `session/new` 的 `mcpServers` 里带上它（`acp/driver.rs` 的 `mcp_entry`）。
  - `Route::runs_locally()` 对 ACP 为真。
  - `mcp_context.rs` 里"DSH 等 `tools/list` 才开始一回合"的注释（2026-11-13）也说明 DSH 在用这些工具。
  - 方案和 11-16 那篇说"注入只覆盖本机 Claude 和 Codex"，写于 ACP 接入之前，已经过时。
- **OMP 没接。** 驱动用默认实现，不接受服务器；`runs_locally()` 对 OMP 为假。属于 H2，本片不做。
- **远端都没接。** 远端的 Codex、Claude、OMP 都不给；DSH 不支持远端。
- **能力表没有这一项。** `session.capabilities` 能说一个 Harness 能不能做子会话、能不能审查，但说不出它有没有 Aeroric 工具。OMP 作为子会话和审查者被写成"支持"，没有提它读写不了黑板、查不到记忆。
- **App 里哪些入口依赖工具。** 逐个看过：
  - 会话里的"新委派"和"角色库"（`DelegationPanelSection`）调的是宿主的委派方法；
  - `#角色` 开头的输入，由 App 解析；
  - 黑板面板、记忆页、Wiki 页、专家设置，都由 App 直接调宿主；
  - 团队协作开关在提交时就被宿主拒绝（"尚未接通"）。

  它们都不依赖会话里的 Agent 有没有工具。真正依赖工具的，是 Agent 自己发起的委派、检索和记忆提议，这在界面上没有入口可以置灰。

## Decision

**能力表新增"Aeroric 工具"**（`Feature::ContextTools`，线上名 `contextTools`）：

| | 本机 | 远端 |
|---|---|---|
| Codex | 有：启动时在 `thread/start` 里接入 | 没有 |
| Claude | 有：启动时用 `--mcp-config` 接入 | 没有 |
| DSH | 有：经 ACP 的 `session/new` 接入 | 没有（DSH 只在本机） |
| OMP | 没有：查不到记忆和 Wiki，也不能自己发起委派、读写黑板 | 没有 |

远端写明原因：工具服务器要能连回这台 Mac 的宿主，由批次 D 处理。

**OMP 作为子会话和审查者改为"有差异"**，而不是方案说的"不支持"：

- 委派的交接清单写在子会话的提示里，结论随回合结束送回，审查结论也一样，这些都不经 Aeroric 工具，OMP 照常能做。
- 少的是黑板和记忆，说明里写明。
- 写成"不支持"反而会让用户以为 OMP 不能接委派。

**一致性测试**（`executor_tests.rs` 的 `the_capability_table_matches_who_gets_the_context_server`）：

- 对每个 Harness，本机路由会不会交出 Context 服务器（`runs_locally()`），要与能力表"Aeroric 工具"是否为"有"一致；
- 所有远端路由都不交出，能力表都写"没有"。

以后有人改了路由（比如 H2 接上 OMP）而没改能力表，或者反过来，这个测试会失败。

**不置灰，写明原因**：App 里没有依赖 Agent 工具的入口（见上），不用改界面。没有工具的会话：

- 会话信息的"工具"一行（[C8 第一步](2026-11-20-c8-effective-tool-catalog.md)）用警示色写"没有 Aeroric 工具：原因"；
- 能力表的说明也写清楚。

**事实写进 `docs/subsystems/agent-capabilities.md` §5**：每家怎么接、远端为什么没有、能力表与测试的对应、App 入口为什么不受影响。另外记下：记忆档案只进系统层，Claude、Codex 有；DSH、OMP 的驱动没有系统层接口，拿不到档案。

## Alternatives considered

**本片顺手把 OMP 接上。** 最强的理由是 OMP 用户马上就能用工具。没选：11-16 那篇已经把 OMP 的注入定为 H2，还要连带写受管 home 的 `mcp.json`、逐档验证审批模式会不会拦工具调用、补系统层。在这里做一半，就会做出第二套方案，那篇明确要求"不要做出第二套"。

**OMP 的子会话和审查者照方案写成"不支持"。** 最强的理由是保守。没选：这两件事 OMP 实际做得到，写成"不支持"是另一种不实，会让用户错过可用的组合。"有差异"加说明更准确。

**把会话里的"新委派"按钮在 OMP 会话上置灰。** 没选：那个按钮由 App 直接调宿主，在 OMP 会话上也能用，置灰反而造出了一个"能用却不让用"的入口。

## Consequences

- **收益**：
  - 能力表能回答"这个 Harness 有没有 Aeroric 工具"，原因写在每一项里；
  - 路由和能力表不会悄悄分叉；
  - DSH 已接入这件事被确认并写下，之前的文档说它没接。
- **局限**：
  - OMP 和远端仍然没有工具，分别等 H2 和批次 D；
  - DSH 和 OMP 拿不到记忆档案（系统层），本片只记录，不处理；
  - App 没有显示整张能力表的地方，新项目只在 `session.capabilities` 的回复里，以及会话信息的工具一行里看得到。
- **验证**：
  - 核心层新增 1 个测试（路由与能力表一致），并在能力表测试里加了 4 条断言：OMP 没有工具、OMP 子会话有差异、DSH 有工具、远端 Claude 没有；
  - 已有的"每个 Harness 和位置都回答每一项、非'有'的都附原因"的测试覆盖到新项；
  - 宿主的能力表集成测试照常通过；
  - 没有界面改动。
