# Agent Note: H2 第二步：命令行入口 aeroric-cli ctx 与内置技能 aeroric

Status: implemented

## Problem

[各 Harness 原生融合](../../proposed/architecture/2026-11-16-harness-native-capabilities.md) H2 的第 5、6 条，是给“既不能配 MCP、也没有系统层”的 Harness 留一条路：

- **第 5 条**：`aeroric-cli ctx <工具名> [--json 参数]`。和 MCP 走同一张工具表、同一个宿主方法、同一个令牌；`ctx --help` 的文字从工具表生成。
- **第 6 条**：内置技能 `aeroric`，描述写清什么时候该用，正文列命令。只挂给工具这一项不是“有”的 Harness。这一片没有真实的挂载对象，用“只有 Shell”的假 Harness 验证。

完成条件是：`ctx` 与 MCP 对同一输入返回同样的结构化结果，有一条对照测试。

动手前（`main@7f18cecb`）读代码：

- MCP 服务（`aeroric-cli mcp-context`）的工具调用在 `mcp_context.rs` 的 `call_tool`。它包含：
  - 工具名到宿主方法的映射；
  - 带副作用工具的 `idempotency_key`；
  - 结果的 `structuredContent`（C8）。
- 工具说明在同一文件的五组列表里，`all_tools()` 只在测试里编译。
- 内置技能打包自 `core/skills/builtin`，挂载有两处，用的是同一套筛选：
  - `builtin_skills::mount`：启动时真正挂载；
  - `catalog::plan`：C7 记下这次给了什么。

  两处都没有按 Harness 过滤内置技能。
- [H2 第一步](2026-11-20-h2-omp-opening-and-tools.md) 之后，本机四家（Claude、Codex、OMP、DSH）都有 Aeroric 工具。

## Decision

**`ctx` 就是 MCP 的 `tools/call`**

- 新模块 `aeroric-cli/src/ctx.rs`。它把工具名和参数包成 `{"name", "arguments"}`，直接调 `mcp_context::call_tool`，输出一行结果 JSON。
- 因为是同一个函数，这几样都和 MCP 一致：
  - 工具名到宿主方法的映射；
  - 令牌注入；
  - `idempotency_key` 和 `operationId`；
  - `structuredContent.status`。
- 参数：
  - `--json '<对象>'` 直接传，`--json -` 从标准输入读；
  - 必须是 JSON 对象，否则退出码 2。
- 令牌：从 `AERORIC_CONTEXT_TOKEN` 读，没有就退出码 2，不从别处找。
- 退出码沿用 `args::exit`：
  - ok、partial 为 0；
  - retry 为 3（连接类，可以重试）；
  - rejected、unclear 为 4。
- `ctx --help` 关掉 clap 自带的帮助，由 `ctx::help()` 从 `all_tools()` 生成。`all_tools()` 改为非测试也编译。生成的内容有：
  - 每个工具的名字、参数（必填的标出）和说明；
  - 带副作用的工具额外写上 `idempotency_key`。

**内置技能 `aeroric`**

- 位置：`core/skills/builtin/develop/aeroric/SKILL.md`，目录里排在开发组末尾。
- 描述按方案写清什么时候用：要以前的讨论、项目约定、设计决定，或者想把一块工作交给别人时。
- 正文写这些：
  - 调用方式：`"$AERORIC_CLI" --data-dir "$AERORIC_DATA_DIR" ctx …`；
  - 输出、状态和退出码的含义；
  - 哪类事用哪个工具；
  - 提醒不必每一轮都先查。

**只挂给没有工具的 Harness**

- `builtin_skills::has_context_tools(harness)` 读能力表本机的“Aeroric 工具”一格。C9 的测试已把这一格和执行器的路由绑在一起。不认识的 Harness 算没有。
- `for_harness` 只在这一项上过滤。`mount` 和 `plan` 都经过它，所以真正挂载的和 C7 记下的不会分叉。
- C7 技能目录（`skills.catalog`）里，这个技能的 Harness 列表也按同一规则算。现在一家都没有，标成不生效，说明写“只给没有 Aeroric 工具的 Harness；现在这几家都有工具，没有挂给任何一家”。

**给将来的启动器准备环境**

- `context_server.rs` 新增 `CLI_ENV`（`AERORIC_CLI`）、`DATA_DIR_ENV`（`AERORIC_DATA_DIR`）。
- `ContextServer::shell_env()` 从工具服务器的定义得出这三个环境变量，令牌也在其中，都不进参数。
- 技能正文写的就是这三个变量名，有测试核对。

**能力表“命令行”一项**

- 本机写“不挂：这一家已有 Aeroric 工具，命令行只给没有工具的 Harness”。
- 远端写“远端没有这台 Mac 的 aeroric-cli 和数据目录”。

## Alternatives considered

**`ctx` 另写一份调用逻辑。** 最强的理由是命令行可以有更友好的输出，比如只打印文字。没选：方案要求两个入口返回同样的结构化结果。另写一份，映射、令牌、幂等键迟早会和 MCP 分叉。现在的输出就是一行 JSON，文字在 `content` 里。

**`--data-dir` 允许从环境变量读，技能里就不用写它。** 最强的理由是命令更短。没选：`aeroric-cli` 的 `--data-dir` 按约定只能由调用方显式给出（args.rs 顶部的说明）。技能里用 `"$AERORIC_DATA_DIR"` 显式传，不破坏这条约定。

**`aeroric` 技能挂给所有 Harness，让模型自己选 MCP 还是命令行。** 没选：方案明确要求有 MCP 的不挂，同一能力两个入口会互相抢调用，也会让 H8 的评测分不清是哪个入口起的作用。

## Consequences

- **收益**：
  - 以后接入一个只有 Shell 的 Agent，给它三个环境变量就能用全部 Aeroric 工具，结果和 MCP 一致；
  - 帮助文字和工具表不会分叉。
- **局限**：
  - 现在没有哪个真实 Harness 会挂这个技能，也没有启动器真的给出 `AERORIC_CLI` 和 `AERORIC_DATA_DIR`；
  - 第一个只有 Shell 的 Harness 接入时，它的启动器要调用 `shell_env()`；
  - 技能目录和“Agent 配置 → 技能”里多了一项，显示为不生效并写明原因。
- **验证**：
  - aeroric-cli 新增 4 个测试：
    - 帮助文字覆盖工具表、标出必填和幂等键；
    - 参数必须是 JSON 对象；
    - 退出码与状态对应；
    - 对照测试：用假宿主让 `ctx` 和 MCP 处理同一输入，宿主收到的方法和参数相同，返回结果逐字段相同；没有宿主、工具不存在时两边也一致。
  - 核心层新增 2 个测试：
    - 四家本机都不挂命令行技能，只有 Shell 的假 Harness 会挂，技能正文写的环境变量和工具名都真实存在；
    - `shell_env` 给出命令、数据目录和令牌。
  - 内置技能数量相关的旧测试按 28 个改了期望值。
