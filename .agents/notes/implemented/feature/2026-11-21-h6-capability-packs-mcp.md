# Agent Note: H6 第一步：能力包清单和信任，MCP 服务器按会话投到四族

Status: implemented

## Problem

[各 Harness 原生融合](../../proposed/architecture/2026-11-16-harness-native-capabilities.md) 的 H6 要的是：在 Aeroric 里装一个插件或加一个 MCP 服务器，所有 Harness 的新会话都能用，不用到每家的配置里再配一遍；某一家用不上，写明原因。

动手前（`main@15282fa3`）核对的事实：

- **Aeroric 没有用户 MCP 服务器的存储。** `extensions/mcp.rs` 只有校验和掩码两个纯函数。界面上的 `MCPSettingsView`、`ExtensionStore` 是 P8-01 留下的，应用里创建了这个 store 但没有页面用它，也没有接到核心层。
- **`extensions/trust.rs` 的判定只适用于技能**：写入范围必须在 `skills/` 下，否则拒绝。它不能直接用来判断一个包能不能运行命令。
- **四族都已经按会话接受一个 MCP 服务器**（Aeroric 自己的 `aeroric_context`），入口在 `SessionDriver::set_context_server`：
  - Claude：`--mcp-config` 的 JSON 直接放在命令行参数里，里面带每个任务自己的 token；
  - Codex：`thread/start` 的 `config.mcp_servers`；
  - OMP：宿主写进受管目录的 `mcp.json`，token 在进程环境里；
  - ACP：`session/new` 的 `mcpServers`。
- **每次启动的工具清单**（C8 的 `ToolLaunch`）记 Aeroric 工具给了没有，以及原因。

## Decision

H6 分三步：

1. 清单、信任、投影计划，加上 MCP 服务器的投影（本笔记）。
2. 技能、角色、命令、事件处理的投影。
3. 管理界面和接口；处理 OMP 自己导入的同名服务器。

**清单**：协议里的 `packs::Pack`，存在 `<数据目录>/packs/<id>/pack.json`。

- 字段：id、名字、说明、来源（类型、出处、版本）、`confirmed`、`enabled`，以及五类内容：
  - 技能：包里 `skills/` 下的文件夹名；
  - MCP 服务器：名字、用途说明、命令、参数、环境变量、是否启用；
  - 角色：沿用角色库的 `Role`；
  - 命令：名字、说明、正文；
  - 事件处理：事件名、命令。
- id 必须等于文件夹名。读不了的包跳过，原因写进日志，不影响其他包。

**信任**（`extensions/packs.rs` 的 `trusted`）：

- 内置包或官方市场的包，在已知版本上可信；
- 其余的要用户确认来源（`confirmed`）；
- 不可信的包，MCP 服务器和事件处理都不投影，因为它们会运行命令。技能、角色、命令只是文字，不受这条限制。

**投影计划**是纯函数 `plan(packs, target)`，每一项都给出结论，或者给了，写明给到哪里；或者没给，写明原因。MCP 服务器不给的情形，按判断顺序：

1. 包已停用；
2. 服务器已停用；
3. 包不可信；
4. 远端会话（命令在这台 Mac 上）；
5. 名字不合规，或和 `aeroric_context` 重名；
6. `validate_server` 不通过（命令为空等）；
7. 名字已被前面的包占用：按包 id 排序，先到先得。

结果放进 `ToolLaunch.packs`，随 `session.capabilities` 返回。技能、角色、命令、事件处理这一步如实写“Aeroric 还没有把能力包里的…投影给 X”；不可信包里的事件处理，写的是不可信这个原因。

**下发**：驱动新增 `set_pack_servers`，默认返回 false。执行器在给完 Aeroric 工具服务器之后调用它；驱动不接受时，计划里已给的服务器全部改为“这次启动不接受”。

- **Claude**：与 `aeroric_context` 合进同一份 `--mcp-config`，两者谁先给都一样。宿主把这份 JSON 写进本进程的私有文件（0600），参数里只放路径，进程结束时删掉。包服务器的环境变量里常有密钥，放在命令行参数里，同一台机器上的其他进程都能看到；顺带，Aeroric 工具的 token 也离开了命令行参数。包服务器的工具不加进 `--allowedTools`，照常过权限。
- **Codex**：合进同一份会话 `config.mcp_servers`。不设 `required`，也不预先批准，照 Codex 对其他服务器的规矩来问。
- **OMP**：宿主写进受管目录的 `mcp.json`，旁边用 `aeroric-pack-servers.json` 记下 Aeroric 写了哪些：
  - 只替换和删除记在里面的；
  - 同名但不是 Aeroric 写的，原样保留并写日志。

  受管目录是 Aeroric 自己的目录，不是用户的 `~/.omp`。
- **ACP**：和 `aeroric_context` 一起放进 `session/new`、`session/resume` 的 `mcpServers`。
- **远端会话**：不下发。

## Alternatives considered

**沿用 `trust.rs` 的 `verdict` 判定包。** 最强的理由是一套信任规则。没选：它把写入范围不在 `skills/` 下的一律拒绝，而能力包本来就有 MCP 服务器和事件处理；硬套进去，要么全部拒绝，要么改坏技能安装的守卫。来源的判断沿用同一口径：内置或官方市场、版本已知，或用户确认。

**清单只放 MCP 服务器，其余四类第二步再加字段。** 最强的理由是这一步更小。没选：清单格式要一次定好，用户和测试用的包才不用改；而且五类内容都进计划之后，“未注入及原因”从第一步起就覆盖完整，不会出现某一类悄悄消失的情况。

**Claude 包服务器的密钥用 `${VAR}` 展开，值放进进程环境。** 最强的理由是配置里一个密钥都没有。没选：Claude Code 不开源，`--mcp-config` 是否展开环境变量静态读不出来；私有文件的做法不依赖这一点。

**在 Aeroric 里做一个 MCP 网关，把所有包服务器聚成一个。** 方案第 3 条已经否决：先依赖各家已有的延迟加载。

## Consequences

- **收益**：
  - 一个受信任的包里的 MCP 服务器，四族的本机会话都能用上，不碰用户自己的配置文件；
  - 每次启动给了哪些、没给哪些、为什么，都能查到；
  - Claude 的 MCP 配置离开了命令行参数。
- **局限**：
  - 还没有管理界面和接口，要手写 `pack.json`；
  - 技能、角色、命令、事件处理还没有投影；
  - 服务器的用途说明还没有送到模型面前（Codex 延迟加载时只给模型看服务器自己的说明，打算第二步放进开场白的指路里）；
  - OMP 会自己导入用户其他 Harness 的 MCP 配置，同名服务器可能出现两次，这一步没有处理；
  - 只支持本地命令（stdio）的服务器，HTTP 服务器没有接；
  - 真实 Harness 里包服务器能否启动、工具能否调用，NOT RUN；Claude 是否接受文件路径形式的 `--mcp-config`，静态读到的帮助文本说接受文件或字符串，真实进程 NOT RUN。
- **验证**（在 Mac mini 上跑）：
  - 协议：最小清单的默认值。
  - 计划：
    - 可信包的五类内容，在四族上各有结论，服务器写明各自的去处；
    - 不可信包的服务器和事件处理不投影，官方包要有版本才可信；
    - 停用、远端、重名、名字不合规、命令为空、被前面的包占用，各给出原因；
    - 驱动不接受时，全部记为没给；
    - 按文件夹读包，坏的单独列出。
  - Claude、Codex：两种服务器合在同一份配置里。
  - OMP：只删 Aeroric 自己写的服务器，用户的同名服务器留下，没事可做时不建目录。
  - 宿主：Claude 的配置写进 0600 文件，参数里没有密钥。
  - 执行器：Codex 的 `thread/start` 带上可信包的服务器、不带不可信包的；工具清单两项都有记录。
