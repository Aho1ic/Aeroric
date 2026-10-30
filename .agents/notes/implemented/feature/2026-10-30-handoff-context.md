# Agent Note: 交接与上下文（B4′）：检查点、交接清单、开工前澄清、Context MCP、侧支

Status: implemented

## Problem

会话常常要"接力"：换模型、换来源、`/compact` 之后续接、委派、第二天再打开。这时新会话拿到的，要么是整段对话的转写，要么什么都没有。前沿调研（`docs/frontier-roadmap.md` §1 J2、§3）的结论是：多 Agent 和长任务的失败大多发生在交接处——摘要丢掉了已确认的决定、已发生的副作用、"结果未知"的操作，以及"仅供谁用"这类边界。owner 2026-10-30 确认了 Q-F1–Q-F19，并批准 B4′ 先于协作实施（Trellis 任务 `10-30-handoff-context`）。

## Decision

**检查点**（core `sessions/live/checkpoint`）

- `CheckpointSink` 包在宿主 sink 外面、回合改动追踪器里面，所以也能看到回合结束后才追加的 `turnChanges` / `autoCheck`。
- 内容完全从结构化事件推导，不调用模型（`derive.rs` 是纯函数）：

  | 事件 | 检查点里的结果 |
  |---|---|
  | 首条用户消息 | 目标 |
  | 已回答的问题 | 决定 |
  | Command / FileChange / MCP 完成 | 改动 |
  | 回合结束时仍未返回的调用 | 结果未知 |
  | 审批请求 | 立即存一条"拟执行" |
  | 审批被拒 | 已拒绝 |
  | 批准了但没有结果 | 结果未知 |
  | Plan | Plan 类义务 |
  | 带 `acceptance:<id>` 档名的自动检查结果 | 核销对应义务 |

- 每条带认知状态（已验证 / 推断 / 未知）。状态由来源决定，模型无法自报。
- 存储：`<data>/checkpoints/<task>.json`，先写临时文件、fsync，再 rename。

**交接清单**（`brief.rs`）

- 固定结构的中文 Markdown。
- 换来源走 handoff 模式时武装 `SourceMove`，下一次启动会话时注入：
  - Claude：`--append-system-prompt`；
  - Codex：`developerInstructions`；
  - omp/DSH：没有系统层，放在首条消息前。
- 同时向会话日志插入 `handoff` 更新（交接卡，带当时的检查点快照）。
- 恢复会话：距上次回合超过 1 小时、且有未完成项时才给。
- 原地换模型、`/compact`：会话没换，只在下一条消息前加一段状态提醒，并且只在有未知或未完成项时加。

**开工前澄清**

- `SubmissionRequest.clarify` 是可选字段。只有为真时才往指纹材料末尾加一个字节，所以旧请求的指纹不变。
- 宿主收到后：
  1. 把请求包进澄清模板；
  2. 用 `plan` 权限档跑第一轮（Claude 的 plan 模式；Codex 落到只读；omp 落到 always-ask）；
  3. 把原权限档记在任务记录的 `clarifyRestorePermission` 字段里。
- 第一轮结束后，core 解析 JSON 代码块，追加一条 `clarification` 更新，App 显示为澄清卡。
- `clarify.accept`：
  - 勾选的条款变成义务（`a1…`），约束写进检查点；
  - 恢复原权限档，发出开工消息；
  - 带命令的条款在之后每个回合由自动检查器加跑。

**Context MCP**

- `aeroric-cli mcp-context` 是 stdio MCP 服务，提供 `search_sessions` / `read_session` / `read_handoff`。
- 令牌按任务签发：经 `AERORIC_CONTEXT_TOKEN` 传入，会话关闭即作废，再次签发时旧令牌失效。
- 每次启动随参数注入（Claude `--mcp-config`，Codex `thread/start` 的 `config`），不写全局配置。只给本机的 Claude、Codex 会话，远程会话和 omp/DSH 都不给。
- 检索只查令牌所属项目。`allProjects` 一律拒绝（Q-F18 要求用户授权，目前还没有开关）。
- 只有进程名是 `aeroric-cli` 时宿主才启用，测试宿主不受影响。

**侧支**

- `branch.start` 复用 `task.fork`（抽成 `fork_task`），给新任务打上 `branchOf` 标记和"（侧支）"后缀。
- `branch.return` 取侧支最后一条助手回答：写进父任务检查点的 `branches`，并在父会话日志插入 `branchReturned`（结果卡）。父会话不注入过程，是否把结论发给主会话由用户点按钮决定。

**App**

- 右侧栏新增"进度"页（详情 / 进度 / 文件 / Git），可见时每 3 秒读一次 `checkpoint.get`。
- 时间线上的交接卡、澄清卡、侧支结果卡共用胶囊行组件。
- 新建会话的加号菜单里加"开工前澄清"开关（不放进"即将支持"的组合能力里，也不计入 `enabledNames`，否则会被当成未接通而拦截提交）。

## Alternatives considered

- **让模型自己写检查点**（网友方案原样照搬）：模型会漏写，也会把推测写成已验证。core 已经掌握结构化事件。
- **检查点写进任务记录 JSON**：任务记录频繁改写、受写入基线约束，而且大小敏感。
- **原地换模型或压缩后改系统层**：会话没有重开，改不了，所以只加状态提醒。
- **Context MCP 写进全局托管配置**：令牌无法限定到任务，多个任务会互相读到。
- **从自由文本里猜约束**：误报代价高，只认显式来源。
- **用 `checkpointChanged` 事件推送**（design 原稿）：App 拉会话更新本来就靠轮询，进度页可见时轮询更简单，监听接口留着没用。

## Consequences

- 与 design.md 的偏差：
  - 删除任务时**不删**检查点文件，沿用 `TaskService::delete` 的原则：删列表里的一行，不等于有权删它的工作记录。
  - 没有"Aeroric 上下文工具"设置开关，Context MCP 对本机 Claude/Codex 常开。
  - 配方的 `clarify_first` 默认值没做。
  - 消息菜单里的"从这里开侧支"没做，入口在进度页。
  - omp/DSH 的侧支（用交接清单起新会话）没做，宿主会如实报"只支持 Claude Code 与 Codex"。
- HANDOFF-001（同日补测发现）：换来源走 handoff 时，宿主原先用 `attach_session(None, None)` 想清掉旧会话，但这在契约上是"两个都空就不动"，旧 `codexSessionId` 留着，下一条消息会 `thread/resume` 旧会话，交接清单也跟着发到旧会话里。这个问题 10-29 换来源时就有，B4′ 让它更显眼。改为 core 新增 `detach_session`：清掉四族和无前缀的会话 id/路径，保留会话归属。没有改 attach 的兼容契约。回归测试 `a_handoff_move_starts_a_new_session_instead_of_resuming`。
- 宿主测试装配里，在"刚写入会话、还没有任何回合"时 fork，会撞上写入基线冲突（普通 `task.fork` 也一样），所以侧支测试先跑完一个回合。生产环境的会话回调走同一个 `TaskService`，没有观察到这个问题，但也没有专门验证。
- `cargo test` 里 4 个 LSP 宿主测试，在非交互 ssh shell 下因为 PATH 里没有 node 而失败（"language server header: early eof"），与本次改动无关。
- 测试：
  - core：推导 16 个、存储与 sink 4 个、交接清单 2 个、澄清 4 个、令牌 2 个；
  - 宿主：检查点、`clarify.accept`、上下文令牌、侧支各 1 个，`--append-system-prompt` 参数 1 个，Codex `developerInstructions` 1 个；
  - CLI MCP：2 个；协议：指纹 1 个、检查点 2 个；
  - Swift：契约 1 个、文字 4 个。
- 真实界面 NOT RUN，等 owner 目检。
