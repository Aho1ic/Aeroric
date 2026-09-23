# ADR-028：一个 agent 有两个家族——`launchFamily` 与 `sessionFamily` 分开，任务记录钉死当初怎么起的

- 状态：已接受（Task P2-05 门禁，Q55 + Q56 + Q57）
- 日期：2026-09-23
- 相关：ADR-009（v1 内部兼容性由具名能力承载，不由版本推断；升 v2 需另立 ADR）、ADR-013（无损归一）、ADR-016（generation 权威与记录镜像）、ADR-022（队列只排队意图）
- 新缺陷类别：**G18**（见"后果"）

## 背景：这不是设计出来的歧义，是产品里已经存在的行为

内置 agent `claude_gpt55` 在**今天的 shipping 客户端**里同时被当成两个家族，且两侧都有明确代码为证：

| 侧 | 位置 | 认成哪族 |
| --- | --- | --- |
| 启动参数 | `src/agents.ts:167-176`（`agentOption` 兜底：`codexLike: agent !== "claude"`、`family: "codex"`）→ `pty.rs:1625,1808` 按 `launch.codex_like` 选 `build_codex_cmd` | **codex** |
| 会话格式 | `src-tauri/src/session.rs:557`（"内建 claude/claude_gpt55 用 `~/.claude/projects/<encoded>`"） | **claude** |
| 配置包 | `app_settings/config_bundles.rs:100-108`（`bundle_agent_family`：`"codex" \| "claude_gpt55" => AgentFamily::Codex`） | codex |
| **启动解析（动态）** | `app_settings/launch_spec.rs:400-406`：**读用户那个 start 脚本的内容**推断（脚本里含 `export CLAUDE_CONFIG_DIR=` 且含 `CLAUDE_CODE_SESSION_ENV_DIR` 才算 claude-like）；`builtin.rs:196` 只提供路径 `~/.claude/start-gpt55.sh` | **取决于用户脚本，不是常量** |
| 新 core | `aeroric-protocol/src/task.rs`（`BuiltInAgent::ClaudeGpt55.family() => ProtocolFamily::Claude`）、`tasks/session_owner.rs:329` | **claude** |

`ProtocolFamily` 在 core 与 v1 契约里是**一个**字段，所以它无法复现今天的行为：无论报 claude 还是 codex，都有一侧与现状不符。而"哪一族"决定的是启动参数、会话文件格式与配置形状——这正是迁移必须逐字等价的三件事。

## 决策

1. **拆成语义明确的两个家族**：`launch_family`（用哪套 CLI 参数起进程）与 `session_family`（会话文件是什么格式、住在哪个 home）。
2. **落在任务记录**（Q56）：记录里存 `launchFamily` + `sessionFamily`，每个任务钉死自己当初是怎么起的。这样用户之后改自定义 profile 的 `codex_like`，不会追溯性地改变存量任务的启动行为。
3. **缺字段＝按现状推导、写时补齐**（Q57）：所有存量记录都没有 `launchFamily`，旧客户端也不会写它。缺失时 core 用**与 legacy 完全相同的纯函数**从 agent/profile 推导（这条推导进 parity 表逐条断言），并在下一次写该记录时把两个字段都落下。不做启动期批量重写用户数据。
4. **契约仍在 v1 内**：依 ADR-009，v1 内部的兼容性由**具名能力**承载，绝不由版本号推断，所以这是新增可选字段 + 一个能力位，**不 bump 主版本、不引入 v2**（引入 v2 需另立 ADR）。旧的 Swift/TS 二进制读不到新字段只会忽略，不会误判。

## 后果

- **G18（新缺陷类别）**：`ProtocolFamily` 单一字段在 `claude_gpt55` 上无法同时满足"启动参数"与"会话格式"两个既有事实。登记为缺陷而不是当场裁决谁对，是因为：判"启动该用 claude"要改真实用户的启动命令，判"会话该用 codex"会让 `~/.claude/projects` 下的历史读不到。本 ADR 的拆分让两者可以并存，真正的裁决归 **P3-02（四族命令构造抽取）**。
- 需要一并生成/更新的产物（逐项核实过，不是笼统猜测）：`aeroric-protocol` 的 task 类型、`core/crates/aeroric-core/src/tasks/{session_owner,service,launch_plan,reducer}.rs`（`AgentFamilies` 拆两个方法、创建/启动时落字段）、`knowledge/task-parity-cases.json` 与 `contracts/native/v1/fixtures/task-transitions.json`（283 条期望重算）。**记录字段本身不在 `envelope.schema.json` 里**（该 schema 只有 `taskStatuses`/`cancellation` 等，不枚举任务字段），任务记录的三方工件是 **`contracts/native/v1/fixtures/legacy-task.json`**（`{stored, expectations, resumeRule}`，由 `tests/storage_compat.rs`、`macos/.../ProtocolGoldenTests.swift`、`src/test/native-host-protocol.test.ts` 三方共读），新增字段要改的是它。
- `session_owner` 现在的会话归属优先级（`sessionFamily` > `sessionCodexLike` > 单族填充 > `agentFamily`）读的是**会话**那一半；`launch_plan` 起进程时必须改读 `launchFamily`，**不得**沿用同一个数。这条分开是本次拆分的唯一目的，若实现里两处仍共用一个字段，等于没做。
- 与 ADR-022 一致：补齐字段是"某次意图的副作用"，不是"宿主代表完整状态的一次快照"。
