# Agent Note: H5 第一步：角色库作为各家的原生子代理，delegate 讲清分工

Status: implemented

## Problem

[各 Harness 原生融合](../../proposed/architecture/2026-11-16-harness-native-capabilities.md) 的 H5 要让 Harness 自带的子代理也用上 Aeroric 的东西，不和它们抢，也不强迫模型改用 `delegate`。H5 有四条：

1. 原生子代理和委派列在一起；
2. 子代理写黑板时标明是谁；
3. 把角色库投影成各家的原生子代理定义；
4. `delegate` 的说明讲清用途。

动手前（`main@52d5c991`）核对的事实：

- **角色库**（`delegation/roles.rs`）：六个内置角色。
  - 调研员：同族，只读；
  - 审查员、构想者：另一族，只读；
  - 实现者、测试员：同族，隔离副本，可写；
  - 专家：只读，供 `ask_expert` 用更强的模型。

  另有用户自己的 `roles.json`，存在 `<数据目录>/delegations`。
- **各家的原生子代理定义**：
  - Claude：插件的 `agents/*.md`，插件里的显示为 `插件名:名字`。直连 Claude 已经有 Aeroric 的会话插件，插件名是 `aeroric`。
  - OMP：agent 目录下的 `agents/*.md`，frontmatter 的 `tools` 是内置工具白名单，MCP 工具另外附加（上游 `task-agent-discovery.md`）。OMP 自带 `scout`、`reviewer`、`security-reviewer`、`task`、`sonic`（同一份文档）。OMP 18.12.0 的只读内置工具叫 `read`、`grep`、`glob`、`ast_grep`、`think`（`tools/*.ts`）。
  - Codex：角色能覆盖的只有开场说明、功能开关和技能（`core/src/agent/role.rs` 的 `AgentRoleOverrides`），沙盒沿用父会话。[H1 第二步](2026-11-20-h1-pending-checks-settled.md)查过，按次下发角色在源码上可行，但实际效果没跑过。
- **`delegate` 的说明**只说“适合可以独立完成、能验收的检索、审查、实现或测试”，没提什么时候不该用它。

## Decision

本片分两步。这是第一步：第 3、4 条，只改核心层。第 1、2 条要改委派列表的界面和黑板的记录，留给第二步。

**投影规则**（`extensions/native_roles.rs`）：

- 只投影在原生子代理里说得通的角色。以下几种不投影：
  - 要换 Harness 或模型的（`agent` 不是“同族”）；
  - 要隔离可写副本的；
  - 带验收或预算的；
  - 专家角色：它的意义是另一个更强的模型，同会话的原生子代理给不了。

  不投影的仍然只能经 `delegate` 用。内置角色里现在只有调研员可以投影；用户自己定义的同族角色、只读或原地可写的，也会投影。
- 权限只在 Harness 能守住时才写：
  - 只读角色写工具白名单。Claude 是 `Read, Grep, Glob, WebFetch, WebSearch` 加 Aeroric 的只读工具（`mcp__aeroric_context__…`）；OMP 是那五个只读内置工具，MCP 工具由 OMP 附加（Aeroric 的工具只动 Aeroric 自己的记录，与 H2 的放行理由相同）。
  - 可写角色不写白名单，沿用父会话的工具，原生子代理本来就不会比父会话多。
- 定义正文是角色的指令；描述写成“名字（Aeroric 角色库）：描述”，模型按描述选择。

**写到哪里**：挂技能时一起写，挂载代码已经知道这次的去处。

- Claude 直连：写进会话插件的 `agents/`，名字是角色 id，模型看到 `aeroric:<id>`。
- Claude 走 Agent 配置：写进受管目录的 `agents/`，名字是 `aeroric-<id>`。
- OMP：写进受管目录的 `agents/`，名字是 `aeroric-<id>`，避开自带的 `reviewer` 等。
- Codex、DSH：不写。Codex 的理由见 Alternatives。

定义文件带标记 `<!-- aeroric-role -->`。只替换、删除带标记的文件；同名但不带标记的（用户自己的）不动。投影失败只记日志，技能照常挂载。

**`delegate` 的说明**写明分工：

- 用它的场合：要换一种 Harness 或模型、要用户能随时接手、要跑很久、要一份隔离的可写工作区、要按验收条款检查结果。
- 同一会话里快速分头检索、查资料，用自己的子代理就够了；在 Claude 和 OMP 里，角色库中适合的角色也已经是子代理，名字带 aeroric。
- [委派](2026-10-31-delegation.md)原有的规则照旧写在说明里。

## Alternatives considered

**Codex 也投影，写进项目的 `.codex/agents/` 或经 `thread/start` 的 `config` 下发。** 最强的理由是三家一致。没选：Codex 的自定义代理不能收窄父会话的沙盒，只读角色到了 Codex 会变成可写的子代理，这正是方案说的“表达不了的不放宽”。

**投影所有角色，不能守住的在正文里写“请只读”。** 没选：靠提示约束权限，模型可以不听。另换 Harness、隔离副本、预算、验收这些，原生子代理根本做不到，写进定义只会让模型以为有。

**角色定义每次启动按会话临时生成，不写进共用目录。** 最强的理由是不留文件。没选：Claude 的会话插件目录本来就按内容共用；OMP 只从 agent 目录读，没有按次传入代理定义的参数。用标记管理自己的文件，能做到可替换、可清理。

## Consequences

- **收益**：
  - Claude、OMP 会话里，模型用自家的派发工具点名 `aeroric:researcher` 或 `aeroric-researcher`，拿到的就是角色库里的调研员，而且是只读的；
  - `delegate` 的说明讲清了何时用它、何时用原生子代理。
- **局限**：
  - 第 1、2 条（与委派列在一起、黑板写入标明子代理）没做；
  - 真实 Harness 里子代理能否按名字派出、只读白名单是否生效，NOT RUN（Claude 插件代理在无界面模式下是否生效本来就在 H1 第二步记为 NOT RUN；这台 Mac 没装 omp）；
  - C8 的触发评测里，关键词假 Harness 的基线数字会因为 `delegate` 说明变化而变；测试只锁评测集的结构，不锁这组数字。
- **验证**（在 Mac mini 上跑）：核心层新增 4 个测试。
  - 内置角色里只有调研员被投影，其余的不投影原因正确；带预算的不投影；原地可写的投影。
  - 定义里 Claude、OMP 各自的只读工具白名单正确，写工具不在其中；可写角色没有白名单；OMP 用前缀避开自带名字；描述里的引号和换行正确转义。
  - 目录只留当前可投影的带标记文件，过期的删掉，用户自己的不动；没有可投影的角色时不新建目录。
  - 挂技能时 Claude 插件、OMP 受管目录都有角色定义，Codex 没有。
