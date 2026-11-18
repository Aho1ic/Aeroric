# Agent Note: 通用 ACP 驱动与 DSH 方言

Status: implemented

## Problem

路线图 Q-R5 已定：做一个通用 ACP 驱动，每家一份方言，排在 R3 之后（见 [路线复核](../../proposed/architecture/2026-11-02-roadmap-recheck.md)）。core 的会话事件本来就对齐 ACP 形状（[结构化协议](../architecture/2026-10-25-structured-agent-protocols.md)）。但各家 ACP 实现的权限差别很大，最常见的坑是：用户自己的配置里开了自动批准（yolo），驱动不钉住就会继承下来，Aeroric 的"每次询问"形同虚设。

DSH 以前不走原生会话：新建会话在 PTY 里跑 `dsh`，界面只有终端和旁边的对话视图，没有审批卡、回执、交接上下文。

2026-11-13 在 Mac 上复现（`initialize` 加 `session/new`）：

| Agent | 结果 |
|---|---|
| DSH（`dsh acp`，0.2.2-rc.1） | 能建会话；声明 `sessionCapabilities` 的 resume、list、close；返回 DeepSeek 模型和思考强度两个配置项 |
| Grok（`grok agent stdio`，0.2.93） | 是 ACP，声明 `loadSession`；`session/new` 回 `Authentication required` |
| Gemini（`gemini --acp`，0.36.0） | 没登录，启动就进 Google 登录流程 |

用户当天定的做法，全部按推荐：

- 下一步做通用 ACP 驱动；
- 先接 DSH，Gemini 等登录后再说；Grok 原定第二批，2026-11-13 用户改为不接；
- 新建的 DSH 会话走 ACP，已经在 PTY 里跑的旧会话照旧；
- 权限按本笔记的钉法，给不出可接受模式就拒绝连接；
- 第一批包括对话、流式消息和工具卡、审批、中断、恢复、选模型和思考强度、Aeroric 上下文 MCP，以及按工具名认出 DSH 的工具；

## Decision

**一个驱动，每家一行方言**（`sessions/live/acp/`）

- `driver.rs` 的 `AcpSession` 和 omp 一样每个会话一个进程：第一条消息时启动，空闲或关闭时结束，下一条消息时透明地恢复。传输复用 Codex 的 JSON-RPC 客户端（`codex::rpc::RpcClient`）。
- `dialect.rs` 每家一行，写清协议留给各家的部分：启动参数、权限怎么钉、工具怎么认。现在只有 DSH 一行；`dialect::for_family` 查不到方言的家族，不会进 ACP 路由。
- `map.rs` 把 `session/update` 转成会话更新：
  - 消息块、思考块；
  - 工具调用和更新：ACP 每次重发整段 `content`，这里只追加新的部分；
  - 计划；
  - 上下文用量（`usage_update` 的 `used`、`size`）。
  - 只服务 Agent 自己界面的更新直接丢掉，认不出的保留为 `Unknown`。

**握手和会话**

- `initialize` 只接受协议版本 1。客户端声明不提供文件系统和终端；Agent 仍然来要，就回 `-32601` 并显示出来。
- 新会话走 `session/new`。已有会话只走 `session/resume`：
  - Agent 没声明 resume，就明确失败，绝不另开新会话；
  - 恢复出的会话 id 和原来的不一样，也明确失败；
  - 只有 `session/load` 的 Agent（Grok）暂不支持。`load` 会把整段历史重放成更新，第一批没有处理重放去重。
- 会话 id 经 `LiveStatusSink::session` 记到任务上（`dshSessionId`），重启后照常从任务恢复。
- transcript 路径：宿主给驱动一个 `TranscriptLocator`（`Routing::acp_transcript`），DSH 的是 `<DSH_HOME>/sessions/<project key>/<会话>/session.vN.jsonl`。每轮结束后文件已经写出，驱动把路径记到任务上（`dshSessionPath`）。页面的历史、轮数和重启后重开都读它。目检时没记路径，统计栏显示"0 轮"。
- 上下文 MCP：ACP 在 `session/new`、`session/resume` 里按会话接收 MCP 服务器，所以本机 ACP 会话也拿到 `aeroric_context`。环境变量用 ACP 的 `[{name, value}]` 形状，口令不进 argv。

**权限钉法**

- 方言把 Aeroric 的权限档换成启动环境（`Pin.env`）。宿主把它最后写入，盖过继承来的一切。
- 方言可以要求一个 ACP 模式（`Pin.mode`）：Agent 不提供就拒绝连接，提供了就 `session/set_mode`。DSH 不用模式。
- 方言给不出对应档位时，在启动进程之前就拒绝，消息回执记为拒绝，并写明原因。
- 权限在会话开始时固定，`permission_locked`；改档要新建会话。
- DSH 的钉法：

| Aeroric 档位 | `DSH_PERMISSION_MODE` |
|---|---|
| `ask`、`plan`（讨论成员、只读评审） | `read-only` |
| `auto_edit` | `workspace-write` |
| `full_access` | `danger-full-access` |

- DSH 用自己的沙盒守住这一档。录到的样子：只读档下写文件先被沙盒拒绝；模型带上 `sandbox_permissions` 和理由重试时，发来 `session/request_permission`，只有"允许一次"和"拒绝"两个选项。

**审批**

- 审批卡的种类按工具认：写文件类是 `fileChange`，命令类是 `command`，其他用工具名。
- 摘要是"工具名 + 路径或命令"，Agent 给了 `justification` 就加在括号里。
- 选项按 Agent 实际给的出：
  - 有 `allow_always` 才有"本会话一直允许"；
  - "拒绝"选 `reject_once`；
  - 拒绝后那次工具调用显示为"已拒绝"，不是"失败"。
- ACP 没有拒绝理由字段。用户写了理由，就在这一轮结束后作为下一条消息发出（"我拒绝了这次操作：…"），并当场提示一句；没写理由时，DSH 自己会告诉模型"用户拒绝了，停下说明"。审批卡上写着理由"会告诉 Agent"，这样才说到做到（2026-11-13 目检发现）。
- 中断时先把还开着的审批答成 `cancelled`（ACP 要求），再发 `session/cancel` 通知。
- 停止有兜底：5 秒内 Agent 没有结束这一轮，驱动就在本地把这一轮记为中断，并结束进程；下一条消息照常恢复这个会话。目检时 DSH 卡在一轮开头，`session/cancel` 没有回应，这一轮一直挂着。

**一轮的边界和回执**

- `session/prompt` 的回答经同一条有序通道送到驱动：转发任务用 `biased` 选择，先送完 Agent 在回答之前写出的所有消息，再送回答。目检时发现这一轮的结束跑到了最后一段消息和用量前面。
- `session/prompt` 的回答就是这一轮的结束：
  - `end_turn` 记为完成；
  - `cancelled` 记为中断；
  - `refusal` 记为失败；
  - `max_tokens`、`max_turn_requests` 记为失败（"<Agent> 到了单轮上限（…），这一轮没做完"），不再另给提示；
  - 规范之外的值和缺失的 `stopReason` 记为失败并带原值（`rawStatus`）。后三条是 2026-11-17 A3 改的，理由见 [A3](../bug-fix/2026-11-17-a3-unknown-turn-end-is-not-done.md)。
- 回执：Agent 发来第一条更新，或这一轮正常结束，就算送达。请求出错算拒绝。
- 一轮进行中发来的消息先收下（回执"已接收"），这一轮结束后作为下一轮发出。ACP 没有插话。
- 进程在一轮中退出，这一轮记为失败（"DSH 进程已退出"）。传输先断、迟到的失败回答不会再结束一次这一轮。

**模型和思考强度**

- 用 `session/set_config_option` 设置，从 Agent 自己的 `configOptions` 里选。
- 模型按三种方式匹配，任一命中即可：
  - 选项值本身；
  - 选项名；
  - 值里的模型部分：DSH 的值是 `["provider","model"]`，几家 provider 都有同名模型时，选当前在用的那家。
- 思考强度取最接近的档位，一样近时取高的：
  - DSH 只有 off、low、high、max；
  - 选 medium 给 high，选 xhigh 给 max。
- 找不到的模型或档位，提示一句，沿用 Agent 当前的设置。
- 页面上显示的模型和档位，以 Agent 回报的为准。

**宿主**

- `HostRouting::route`：本机的 DSH（内置的，或钉了 DSH Agent 配置）走 `Route::Acp`。
  - 自定义的 dsh 包装命令不一定接受 `acp`，留在 PTY。
  - SSH 工作区不走 ACP。
- `dsh_acp_launcher` 启动 `dsh acp`，`DSH_HOME` 始终是托管目录，从不用用户自己的：
  - 内置的 dsh：用 `agent-homes/dsh`，靠它自己的登录。
  - Agent 配置：用配置自己的 home，provider 只指向本机网关。
    - 这个任务的模型 patch 以 `--patch` 传入，和 PTY 路径同一套（`dsh_home::prepare_gateway_home`）。
    - 网关口令经 `DEEPSEEK_API_KEY` 注入，进程退出时吊销。
- 能力表里的 DSH 一列：

| 项 | 状态 |
|---|---|
| 权限档 | 由 DSH 沙盒把关，越权写入先问你 |
| 中途改权限 | 降级：下次启动生效 |
| 规划模式 | 降级：用只读沙盒代替，写入要你批准 |
| 插话 | 降级：作为下一轮发出 |
| 思考强度、回执 | 支持 |
| 压缩上下文、内置审查 | 不支持 |
| 当审查者、当子会话 | 还没加进候选（候选列表只有 Claude 和 Codex） |
| 远程 | 不支持 |

**旧会话**

- PTY 里还在跑的旧 DSH 任务没有驱动，停止和结束都落到 PTY 执行器，和以前一样。
- 界面按宿主的 `session.state.structured` 选视图：
  - 新的 ACP 会话用原生会话页；
  - 旧的 PTY 任务仍是对话加终端的分栏。

**其他 Agent 的方言（还没接）**

方言表初稿来自 papilot `src/server/acp.ts`，接入时逐家实测：

| Agent | 启动 | 权限钉法 | 现状 |
|---|---|---|---|
| Grok | `grok agent stdio` | `sessionMeta` 的 `yoloMode`、`autoMode` 都钉成 false；它有 `always-approve` 命令，不能放行 | 不接（用户 2026-11-13 定）；已确认是 ACP，需要 `authenticate`，只有 `session/load` |
| Copilot | 各家 ACP 入口 | `allow_all` 关掉，同时设 `COPILOT_ALLOW_ALL=false` | 本机没装 |
| Kimi | 同上 | 用 `default` 模式 | 本机没装 |
| Qwen | 同上 | `auto-edit` 或 `default` | 本机没装 |
| Gemini | `gemini --acp` | 待实测：确认不继承用户的自动批准设置 | 没登录 |

## Alternatives considered

- **每家写一个专用驱动**：方言差异最好处理。没有采用：差异集中在启动参数、权限和工具名上，一张表能装下，协议本身各家一样（Q-R5）。
- **继承用户自己的 ACP 配置**：最省事，用户在别处怎么用这里就怎么用。不采用：会把用户在别处开的 yolo 带进 Aeroric，"每次询问"失效。
- **DSH 的旧会话也改走 ACP**：一个路径更简单，`dsh acp` 也能恢复同一个 DSH home 里的会话。没有采用：用户选了新旧并存。已经在 PTY 里跑的进程没有 ACP 连接可接，硬切会把它们算成孤儿。
- **恢复不了就新开一个会话**：用户不会被一条错误卡住。不采用：用户会以为历史接上了，其实是一段新对话；和 `TaskError::NoSession` 同一个理由。
- **一轮中的消息直接拒绝**：实现最简单。没有采用：投递队列默认"立即"，进行中的一轮里发消息很常见；先收下、下一轮发出，和 Claude 的做法一致。

## Consequences

- 收益：DSH 新会话有了审批卡、回执、交接和上下文 MCP、用量、改动快照和检查，和 Codex、Claude、omp 用同一个会话页。以后接 Kimi、OpenCode 时只加一行方言。
- 代价：
  - DSH 不能中途改权限，不能压缩，不能插话；
  - 关联文件夹仍然到不了 DSH，ACP 没有额外工作目录；
  - "本会话一直允许"不跨进程记住。DSH 也不提供这个选项，以后有 Agent 提供时要像 omp 那样由驱动记住。
- 只接 `session/resume`。只支持 `session/load` 的 Agent（Grok）接入前，要先处理历史重放。
- 各家 ACP 实现还在快速变化，方言表要跟着版本走。DSH 的工具种类全是 `other`，靠工具名识别，改名就会漏判（漏判只影响卡片图标和审批种类）。
- 目检（2026-11-13，审阅版，Agent 配置"全部第三方模型"走网关，截图在 Mac `~/Documents/computer-use/2026-11-13-0950-aeroric-acp-dsh-claude/`）：
  - 新建 DSH 会话进了原生会话页；
  - 两次审批，允许的写入成功，拒绝的显示"已拒绝"；
  - 改动统计、待验收、权限"每次询问 · 本会话固定"都对；
  - 重启后从 transcript 读回历史和用量。
  - 之后改用宿主 RPC（`aeroric-cli rpc session.prompt / session.interrupt / session.updates`）测，不碰鼠标和前台：
    - 重启后续聊，约 10 秒恢复并回答；
    - 用 SIGSTOP 冻住 DSH 再停止，5 秒后这一轮记为中断，进程结束，下一条消息恢复同一会话；
    - 更新的顺序正确，这一轮的结束排在最后。
  - 重启后续聊卡住一次：DSH 开了一轮却没到 `step/start`，也没有请求网关。那时宿主有线程卡在读钥匙串（重新签名后的授权提示），上下文 MCP 在 `initialize` / `tools/list` 里问宿主"记忆、Wiki 开没开"，没有超时。DSH 要等 MCP 的工具列表才开始一轮。现在这两次询问 3 秒没回答就只给基础工具（`aeroric-cli` 的 `HOST_PROBE`），宿主忙不再挡住开轮。钥匙串提示没法稳定复现，这一条是按现场证据加的防护。
  - 另一次"很慢"是上游：网关两次 504（60 秒无输出），DSH 自己重试，第三次成功，共 84 秒。DSH 重试时不发 ACP 更新，页面只看到转圈。
- 登录状态没有 DSH 的条目（`login_key` 对 DSH 返回空）。DSH 内置的登录失效时，只在这一轮失败的原因里看得到。

## Testing

- `acp::dialect` 测试：四个档位的钉法和未知档位的拒绝，DSH 工具按名字识别。
- `acp::map` 测试：用 2026-11-13 录到的 DSH 帧，覆盖思考、用量、写文件、拒绝后显示"已拒绝"、消息完成；工具输出只追加新部分；计划；忽略和未知的更新。
- `acp::driver` 测试，用假的 ACP 对端：
  - 一整轮：钉住的环境、上下文 MCP 条目、选模型、两次审批（允许和拒绝）、回执；
  - 一轮结束后记下 transcript 路径；
  - 一轮中的消息作为下一轮，拒绝理由也作为下一条消息；
  - 中断时取消审批；Agent 不回应停止时本地结束这一轮并结束进程；
  - 恢复走 `session/resume`，不支持恢复时明确失败、不发 `session/new`；
  - 不能守住的档位在启动前拒绝；
  - 进程中途退出只结束一次这一轮，下一条消息恢复同一会话；
  - 模型和档位的挑选。
- 能力表测试补了 DSH 的插话、回执和远程。
- 真实环境：2026-11-13 用 `dsh acp` 录了一轮（只读档，一次允许、一次拒绝），恢复后模型和历史都在，没有重放旧消息。
