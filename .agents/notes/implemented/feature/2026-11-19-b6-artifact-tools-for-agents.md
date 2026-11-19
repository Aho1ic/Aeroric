# Agent Note: B6 第二步：Agent 读交付物，委派结果按版本引用

Status: implemented

## Problem

[B6 第一步](2026-11-19-b6-artifact-registry.md)让每个任务的交付物有了身份和版本，但只有 App 能看：

- Agent 没有工具读取上一个任务、或它委派出去的子任务交付了什么。上级只能靠子任务在结论里描述文件，或者自己去工作目录里翻。工作目录里的文件之后可能又被改过，读到的不一定是子任务交付时的那一版。
- [委派](2026-10-31-delegation.md)的结果消息只写"改动：N 个文件 +a −d"，不说是哪些文件、哪一版。

## Decision

**两个 Context MCP 工具**（`core/crates/aeroric-cli/src/mcp_context.rs`，基础工具，总是提供）：

- `artifact_list {taskId?, cursor?}` → `context.artifactList`：
  - 列出一个任务的交付物，默认是当前任务；
  - 返回 `readable`：调用方还能读哪些任务，每个带标题和关系。
- `artifact_read {artifact, taskId?, offset?, limit?}` → `context.artifactRead`：
  - `artifact` 写 `art-…@3`，不带版本就读当前版；
  - 不填 `taskId` 时，在调用方能读的任务里找这个 id；
  - 和 App 的 `artifacts.read` 走同一个读取函数，分片、编码、"这个版本的内容已不可用"都一样。
- 工具说明（Codex 只看 `instructions`）加了一句"读取任务交付的文件及其各版本"。

**能读哪些任务**（`core/crates/aeroric-host/src/session_live_services/artifacts.rs` 的 `relation`）：

- 只在令牌所属的项目内；
- 当前任务自己；
- 它委派或分出侧支的任务：记录的 `delegateOf` 或 `branchOf` 是它；
- 它的上级；
- 同一上级下的其他任务，例如测试子任务读实现子任务的交付物。
- 其他任务一律拒绝，返回"只能读当前任务、以及和它有委派或侧支关系的任务的交付物"。错误码是 `NOT_FOUND`，和 `read_session` 一样不透露那个任务存不存在。孙辈任务不算。

**委派结果按版本引用**：

- 子任务回报时，委派卡加上 `artifacts`：它当前成立的交付物，每个是 `{artifactId, version, path}`，最近改过的在前，最多 20 个；不算交付物的、已删除的不列。
- 发给上级的结果消息在"改动"之后列出"交付物（artifact_read 按 id@版本 读取）"，每行写路径和 `art-…@n`。
- App 的委派卡脚注写"交付 report.md（第 2 版）"，多个文件时写"交付 3 个文件：a、b 等"。

## Alternatives considered

**让 Agent 读项目里任何任务的交付物。** 最强的理由是简单，`read_session` 也是整个项目可读。没选：方案要求读取范围受任务关系约束；交付物会成为下一步工作的输入，读到不相干任务的产物，比读到不相干的对话更容易被误用。

**交接说明（`read_handoff`）里也改成按版本引用。** 方案原文提到了。没做：交接清单由检查点生成，里面的"改动"是回合级的事实，没有交付物登记处的入口；Agent 需要时可以直接调 `artifact_list`。等有具体需要再接。

**结果消息里直接附上交付物内容。** 最强的理由是上级不用再调一次工具。没选：结果消息会进上级的上下文，几个文件就可能占掉大量 token；按版本引用，需要时再读。

## Consequences

- **收益**：
  - 上级拿到的是子任务交付时的那一版，之后工作目录里再怎么改，读到的都不变；
  - Agent 读到的内容和用户在 App 里打开的同一版逐字节一致。
- **代价**：委派卡多一个字段，旧卡没有它，读出来为空。
- **没做**：
  - 交接说明里的版本引用；
  - 跨项目读取；
  - 孙辈任务的读取。
- **验证**：
  - 宿主单测 `an_agent_reads_its_own_its_children_its_parent_and_its_siblings_only`：自己、子任务、侧支、上级、同级可读，无关任务、别人的子任务、孙辈不可读。
  - 宿主集成测试 `context_artifacts_read_the_tokens_task_and_refuse_strangers`：
    - 无效令牌被拒；
    - 按 `id@1` 每次 100 字节分片读，拼起来等于原文件，也等于 App 方法读到的内容；
    - 别的任务被拒，不存在的 id 返回 `NOT_FOUND`。
  - 委派测试 `a_childs_result_names_its_deliverables_by_version`：卡上只有 `report.md@1`（`tmp/` 下的文件不算），结果消息写出 `- report.md：art-…@1`，按这个引用读回的就是交付时的内容。
  - CLI 测试：基础工具清单和说明包含两个新工具，映射到对应的宿主方法。
  - Swift `testADelegationCardSaysWhatTheChildDelivered`：解码卡上的交付物，脚注文字正确。
  - 真实 Agent 调用这两个工具没有跑过（NOT RUN），委派卡脚注待目检，记在路线文档 §8。
