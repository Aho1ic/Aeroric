# Agent Note: H2 第一步：本机 OMP 接上系统层和 Aeroric 工具

Status: implemented

## Problem

[各 Harness 原生融合](../../proposed/architecture/2026-11-16-harness-native-capabilities.md) 的 H2 要让 OMP 会话和 Claude、Codex 一样：知道用户的偏好，能查记忆、读 Wiki、发委派。到 [H1](2026-11-20-h1-surfaces-per-launch.md) 为止，OMP 这两项都没有：

- **系统层**：没有。交接简报拼到第一条消息前，记忆档案不给；
- **工具**：没有。`Route::runs_locally()` 对 OMP 为假，驱动也不接受工具服务器；
- **能力表**：OMP 作为子会话和审查者只能写"有差异"。

动手前（`main@9cc364dc`）核对的事实：

- OMP 18.12.0 有 `--append-system-prompt`，参数是路径时读文件。规则在 `system-prompt.ts` 的 `resolvePromptInput`：不含换行就先当路径试读。
- agent 目录下的 `mcp.json` 支持 `${VAR}` 展开（H1 第二步已核对）。没设的 `${VAR}` 会原样保留，`${VAR:-}` 才变成空串。
- 本机 OMP 每次启动都用 Aeroric 自己的受管目录（`PI_CODING_AGENT_DIR`），有两种：
  - 自有登录：`agent-homes/omp` 或自定义档案的目录；
  - Agent 配置：该配置的网关目录。

  受管目录里的 `mcp.json` 只写了一个空骨架，从没填过。
- 工具服务器拿不到 token（或拿到空串）时直接退出（`mcp_context.rs` 的 `serve`）。方案第 3 条担心的情况，已有代码处理了。
- OMP 的审批：
  - MCP 工具一律算"会写"，`always-ask`（Aeroric 的"每次询问"和只读规划都映射到它）下每次都要问；
  - `mcp.json` 里没有按服务器放行的字段；
  - 有按工具名的 `tools.approval.<工具名>: allow`，工具名是 `mcp__aeroric_context_<工具>`；
  - 有 `--config <文件>` 叠加层，只对这一个进程生效，记录型设置按条目合并。
- 远端 OMP 也用同一个驱动，但开场文件和工具服务器都在这台 Mac 上。

## Decision

**系统层。**

- OMP 驱动实现 `set_instructions`，内容随启动参数 `OmpStart.append_system` 交给宿主。
- 宿主把内容写进 `<data>/tmp/omp-openings/<随机名>.md`（目录 0700，文件 0600），用 `--append-system-prompt <路径>` 传入。记忆不进进程参数。
- 进程退出时由进程守卫删除文件；启动失败也删除。

**工具。**

- OMP 驱动实现 `set_context_server`；`Route::runs_locally()` 对本机 OMP 改为真，远端仍为假。
- 宿主在受管目录的 `mcp.json` 里声明 `aeroric_context`：
  - 只改这一项，用户自己加的服务器和其他键都保留；
  - 内容没变就不重写；
  - 文件不是有效 JSON 时不动它，启动报错并写明原因。
- 文件里 token 写成 `${AERORIC_CONTEXT_TOKEN:-}`，真正的 token 只放进这次进程的环境变量。受管目录被多个会话共用，所以文件里不能有某个任务的 token。
- 关会话时 token 照旧撤销。

**审批。**

- 受管目录里另写一份 `aeroric-context-tools.yml`，列出 21 个工具名，每个都是 `allow`；有工具服务器的启动用 `--config` 传入。
- 放行范围只有这 21 个工具，和 Codex 只给这一个服务器设 `approve` 的理由相同：它们只动 Aeroric 自己的记录。
- 不改整个会话的审批模式，也不改用户的 `config.yml`。

**远端。**

- `OmpSessionSpec` 加 `remote`，远端 OMP 的驱动对开场内容和工具服务器都返回"不收"。
- 交接简报照旧放进第一条消息，入口记录如实写"没有"。

**能力表。** 本机 OMP 的工具、系统层改为"有"。子会话和审查者的"有差异"去掉，因为它现在有黑板和记忆了。远端 OMP 不变。C9 的一致性测试（路由与能力表）照旧成立。

本片不做方案第 5、6 条（`aeroric-cli ctx` 命令行和内置技能 `aeroric`），留给第二步。

## Alternatives considered

**开场内容直接放进参数（像 Claude 那样）。** 最强的理由是不用管临时文件的生命周期。没选：方案要求记忆不进进程参数，`ps` 能看到别的进程的参数。

**把放行规则写进受管 `config.yml`（提升它的版本标记）。** 最强的理由是不多一个文件、不多一个参数。没选：

- 版本标记一提升，会整体重写 `config.yml`，用户在设置里改的默认思考档会丢；
- Agent 配置的网关目录只在缺文件时才写 `config.yml`，老目录拿不到新规则。

叠加层只对这一个进程生效，按条目合并，两个问题都没有。

**不放行，走现有审批路径让用户逐次批准。** 最强的理由是最保守。没选：

- "每次询问"下 Agent 每查一次记忆都要弹一次批准，等于没接；
- Codex 上已经因为同样的原因只对这个服务器放行，两家应该一致。

**远端 OMP 把开场文件传到远端再用。** 没选：远端启动要多一套文件分发和清理，而远端会话的工具本来也要等批次 D。本片如实写"没有"。

## Consequences

- **收益**：
  - 本机 OMP 会话有了记忆档案和交接简报（系统层），能用 21 个 Aeroric 工具；
  - 当子会话和审查者时能读写黑板；
  - 能力表、入口记录、会话信息都显示"已接通"。
- **局限**：
  - 真实 OMP 会话没跑（NOT RUN）：这台 Mac 上没装 omp。待装上后，要确认 `--config` 叠加层确实让工具调用不再弹批准，确认 `--append-system-prompt` 读到了文件；
  - 远端 OMP 仍然没有系统层和工具；
  - 命令行兜底和内置技能 `aeroric` 留给第二步；
  - OMP 会自己导入用户 `~/.claude.json`、`~/.codex/config.toml` 里的 MCP 服务器，工具清单还没列出这些。
- **验证**：
  - 核心层新增 3 个测试：
    - 驱动把开场内容和工具服务器带进启动参数，远端不收；
    - 受管 `mcp.json` 的声明不含 token、保留其他服务器、没变不重写、坏文件不动，叠加层正好放行 21 个工具；
    - 执行器给本机 OMP 交出工具服务器和开场内容，入口记录写"有"，关会话后 token 失效。
  - 宿主新增 1 个测试：开场文件 0600、参数里没有记忆和 token、`mcp.json` 里没有 token、环境变量里有 token、清理后文件消失。
  - 能力表和入口记录的已有测试按新事实改了期望值。
