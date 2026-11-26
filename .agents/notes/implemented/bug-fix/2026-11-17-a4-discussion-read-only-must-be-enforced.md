# Agent Note: A4 讨论的只读要有 Harness 自己强制

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A4。2026-11-17 在 `main@1e682f44` 加未提交的 A3 等改动上复核：

- 讨论成员一律用权限档 `plan`（`core/crates/aeroric-host/src/session_live_services/discussion.rs:44` 的 `READ_ONLY`，建讨论记录和成员任务时写入）。[讨论模式](../feature/2026-11-12-discussion.md)的 Consequences 写的是"只读依赖各 Harness 自己的规划 / 只读模式，Aeroric 不额外拦截"。
- 各族拿到 `plan` 后的实际情况：
  - **Codex**（`codex/map.rs` 的 `codex_policy`）：只读沙盒（`sandbox: read-only`，`approval: on-request`）。Agent 要越出沙盒时发审批，讨论把它留给用户（`discussion/mod.rs` 只自动拒绝退出规划的请求）。
  - **Claude**（`claude/map.rs` 的 `claude_permission_mode`）：`--permission-mode plan`，由 Claude 自己的权限系统挡写入，没有 OS 沙盒；退出规划的请求被讨论自动拒绝。
  - **DSH**（`acp/dialect.rs` 的 `dsh_pin`）：`DSH_PERMISSION_MODE=read-only`，由 DSH 自己的沙盒挡写入，越权写入走 `session/request_permission` 问用户。批次笔记里"DSH 的权限通道不接受 `plan`"在 DSH 改走 ACP 后已不成立。
  - **omp**（`agents/launch.rs` 的 `omp_permission_flag`）：`plan` 映射成 `--approval-mode always-ask`。这是"每个工具调用都问"，不是只读：读文件也要问，写文件只要用户点了允许就会写。2026-11-17 在 Mac 上看了已安装的 omp 18.19.0 的 `--help`：审批档只有 `always-ask | always-ask-cached | write | yolo`；有 plan 模式，但能在启动时进入它的只有 `--plan-yolo`，它会自动批准计划并切去实现，不能用。另有 `--tools` 白名单，可以只开读的工具，但没在 rpc 模式下验证过。
- 委派：只读工作区强制 `plan`（`delegation/mod.rs` 的 `resolve`、`delegation/roles.rs` 的 `save`）。子会话的候选只有 Claude 和 Codex（宿主 `review.rs` 的 `REVIEWERS`，委派的 `pick` 也用它），所以今天不会出现 omp 或 DSH 的只读子会话。
- 能力表（[能力表与登录](../feature/2026-11-03-capabilities-and-login.md)）里没有一项说"这个族在 `plan` 下能不能保证只读"。

后果：omp 成员进了讨论，名义上只读，实际上每一步都弹审批，用户点了允许它就能改文件。

## Decision

**能力表加一项"严格只读"。讨论成员和只读委派的子会话，只有这一项是"支持"的族才能用；其余在开始前拒绝，并说明原因。**

### 能力表

- 协议 `Feature` 新增 `StrictReadOnly`（线上 `strictReadOnly`），追加在 `Feature::ALL` 末尾。只加不改；能力表不在 `contracts/native/v1` 里，Swift 按字符串解码，旧客户端忽略新项。
- 填法（`core/crates/aeroric-core/src/sessions/live/capabilities.rs`）。"支持"的条件是：这个族在 `plan` 下拿到的是 Harness 自己强制的只读模式，并且有测试证明 Aeroric 的映射确实是这样：

| 族 | 状态 | 说明（界面上显示） |
|---|---|---|
| Codex | 支持 | Codex 只读沙盒强制 · 越权写入先问你 |
| Claude | 支持 | Claude 的 plan 模式把关 · 无沙盒 |
| DSH（本机） | 支持 | DSH 只读沙盒强制 · 越权写入先问你 |
| DSH（远端） | 不支持 | 沿用已有的"DSH 只在本机工作区上运行原生会话" |
| omp | 不支持 | omp 没有能在启动时固定的只读档：只读会话会落到"每次都问"，写入只能靠你拒绝 |

- Codex、Claude 在远端也是"支持"：远端启动用的是同一套参数，沙盒和 plan 模式在远端主机上由 Harness 自己执行。
- 测试 `strict_read_only_rests_on_the_plan_mappings` 把表和映射钉在一起：`claude_permission_mode("plan") == "plan"`、`codex_policy("plan")` 是只读沙盒、`DSH.pin("plan")` 是 `read-only`、`omp_permission_flag("plan") == "always-ask"`。映射一改，这个测试就会提醒回来改表。
- core 新增两个函数：`place_of(ProjectTarget)`（只有 SSH 项目算远端）、`read_only_refusal(family, place)`（不是"支持"时返回原因）。宿主两处都调用它们，判断只有一份。

### 讨论

- `discussion.start` 在逐个校验成员时（建任何记录之前）按成员配置的启动族（`AgentFamilies::launch_family`）和项目位置查表。不是"支持"就整体拒绝，`INVALID_ARGUMENT`："<配置名>：不能保证只读，不能参加讨论（<原因>）"。因为在建记录之前拒绝，没有要清理的任务。
- macOS：新建页的讨论设置里，"成员 Harness"滑块按能力表把不能保证只读的 Harness 灰掉，悬停写"<Harness>：不能参加讨论（<原因>）"；默认打开的 Harness 也跳过它。宿主没给这张表（旧宿主）时不灰，交给宿主判断。App 读能力表时补上了 `dsh`（以前只读 codex、claude、omp）。

### 委派

- 宿主 `start_child` 在选定子会话的族之后，权限是 `plan` 时同样查表；不是"支持"就返回"<族> 不能保证只读，不能接只读委派（<原因>）"，委派卡记失败并显示这句话。
- 今天候选只有 Claude、Codex，这一处不会触发。它是给以后把 omp 或 DSH 加进候选时的保险，和讨论用同一个判断。
- 委派表单不做置灰：子会话的族由宿主按"同族 / 另一族 / 角色默认"选出，表单不知道最后会是哪一族，在表单里再算一遍就是第二份业务逻辑。

### 不在这次范围

- 审查会话（`review.rs` 的 `start_reviewer`）也用 `plan`，候选同样只有 Claude、Codex，暂不加检查。
- 已经存在的、带 omp 成员的讨论不追溯：再来一轮、插话照旧发给原成员。
- omp 用 `--tools` 白名单做真正的只读，要先在 rpc 模式下实测，属于以后的事。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 原地补 A4 的落地情况和"委派候选只有两族"的事实；A4 状态和待目检项回写路线文档 §8 |
| [讨论模式](../feature/2026-11-12-discussion.md) | 被本篇修改 | 原地改"只读依赖各 Harness、Aeroric 不额外拦截"那一条 |
| [能力表与登录](../feature/2026-11-03-capabilities-and-login.md) | 被本篇补充 | 原地补"严格只读"一项 |
| [A3](./2026-11-17-a3-unknown-turn-end-is-not-done.md) | 无关 | 不动 |

## Alternatives considered

**omp 改用 `--tools` 只开读的工具（read、grep、glob、web_search），表里填"支持"。** 最强的理由是 omp 成员不用被挡在讨论外面，而且工具不存在比审批更彻底。没选：没在 rpc 模式下验证 `--tools` 是否生效、子代理（`task`）是否继承；读的工具里 `lsp` 带格式化，要逐个确认。按"有测试证明映射正确才算支持"的规矩，现在只能填"不支持"。验证之后加一行映射、改一格表就能放开。

**omp 填"未知"。** 最强的理由是还没在 rpc 模式下把 omp 的所有入口都核对完。没选：问题不在 omp 能不能，而在 Aeroric 给它的就是"每次都问"；这是 Aeroric 自己的映射，已经确定。

**讨论里遇到 omp 成员时降级（照常开始，界面写"靠你拒绝写入"）。** 最强的理由是用户自己决定要不要冒这个险。没选：讨论会自动推进好几轮，成员的审批会在用户不盯着时堆起来；"只读"是讨论对用户的承诺，降级就等于不承诺。

**只在 Swift 里灰掉，不在宿主拒绝。** 最强的理由是改动小。没选：讨论还能从"围绕这份稿子讨论"带着原成员发起，也能经 RPC 发起；判断必须在宿主。

## Consequences

- **收益**：讨论和只读委派里不再有"写了 `plan` 但没人强制"的成员；不能参加的 Harness 在选成员时就灰掉并写明原因。
- **代价**：
  - omp 暂时不能参加讨论。
  - Claude 的"支持"靠它自己的 plan 模式，没有 OS 沙盒；这和权限选项旁边已有的"由 Claude 把关 · 无沙盒"一致。
  - Codex 和 DSH 的只读沙盒允许越权请求，讨论会把这类请求留给用户；用户点了允许，成员仍然能写。
- **验证**：
  - core：`capabilities` 的 `strict_read_only_rests_on_the_plan_mappings`（四族映射和表一致，DSH 远端、omp 被拒的原因文字）、`only_an_ssh_project_is_remote`；原有的"每个族、每个位置都回答每一项"测试覆盖新项。
  - host 端到端 `a_discussion_takes_only_members_that_hold_themselves_read_only`：`session.capabilities` 里 omp 的 `strictReadOnly` 是 `no`、Codex 是 `yes`；Codex 加 omp 的讨论返回 `INVALID_ARGUMENT`，原因带"不能保证只读""每次都问"，项目里没有多出任何任务。
  - Swift `DiscussionTests.testAHarnessThatCannotStayReadOnlyIsGreyedOut`：有表时 omp 灰掉、默认 Harness 跳过它、悬停文字；没有表或表里是"支持"时不灰；"未知"也灰。
  - 委派那一处今天触发不了（候选只有 Claude、Codex），没有端到端测试，靠和讨论共用的 `read_only_refusal` 的单测。
  - 宿主测试里假的 `AgentFamilies` 原来把 claude 以外都当 Codex；这次让 `omp` 解析成 omp 族，其他测试都不启动 omp 任务，不受影响。
  - 2026-11-17 在 Mac 上：`cargo test --workspace --no-fail-fast` 3736 个通过、0 失败；`cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings` 通过；`swift test --package-path macos/AeroricKit` 960 + 32 + 109 个用例 0 失败（1 个跳过）；App 用单独的 DerivedData 编译通过；`pnpm verify-notes` 通过。能力表不在 `contracts/native/v1` 里，没有跑 `export-schema`。
  - 界面没有目检，记在路线文档 §8 待目检清单。
- **2026-11-18 补强**：只读的执行补到宿主：`plan` 会话的审批一律自动拒绝、`session.configure` 不许改只读角色的权限、Claude 在 `plan` 下不带 bypass 标志（B1-1），见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。
