# ADR-029：原生续跑能力是第三个字段，不从任何一族推导

- 状态：已接受（Task P2-05 门禁，Q58）
- 日期：2026-09-23
- 相关：ADR-028（launchFamily / sessionFamily 拆分）、ADR-016（generation 权威与记录镜像）、ADR-022（队列只排队意图）、缺陷 G18
- 本 ADR 同时纠正一次我自己在 ADR-028 与 parity 表里留下的错误，见 §纠正

## 背景：一个字段被三个问题共用

实测（`src/taskSession.ts:58-83` + `src/agents.ts:167-178`，本机跑通）：

| 输入 | 客户端答案 |
| --- | --- |
| 已 stamped（`sessionFamily: "claude"`）的记录 | owner.family = **claude** |
| 未 stamped、但有 claude 会话字段 | owner.family = **claude** |
| 未 stamped、也没有任何会话字段 | owner.family = **claude** |
| `agentOption("claude_gpt55")` | family = **codex**、`codexLike = true` |

于是"claude 任务能不能被 `claude_gpt55` 原生续跑"这个判断，今天是拿**记录侧的 transcript 族**去比**目标 agent 的 `agentOption.family`（对 gpt55 是 codex）**——两个不同问题的答案被放在同一个 `ProtocolFamily` 里比，结论是"不能，走 handoff"。ADR-028 拆出 `launchFamily` / `sessionFamily` 之后，这个比较仍然没有归属：它既不是"用哪套参数起"，也不是"转录是什么格式"，而是"**哪个 CLI 的 `--resume` 读得懂这份转录**"。

## 决策

1. 新增第三个概念：**原生续跑族 `resumeFamily`**，落进任务记录，语义是"哪一族 CLI 的 `--resume` 能直接读这份转录"。
2. `AgentFamilies` 端口加 `native_resume_family(agent)`；`can_native_resume` / `can_adopt` / `resolve_config_switch_strategy` 一律改用它，**不得**再拿 `session_family` 或 `launch_family` 顶替。
3. 内置 agent 的取值以今天的**结论**为准，不是以某个字段为准：`claude→claude`、`codex→codex`、`dsh→dsh`、`omp→omp`、`claude_gpt55→codex`（所以 claude↔gpt55 今天仍是 handoff，本 ADR 不改产品行为，只是把理由写成显式字段而不是巧合）。
4. 缺失时（全部存量记录）按 `sessionFamily` 推导一次，并在下一次写该记录时补齐——与 ADR-028 决策 3 同一条规则，不做启动期批量重写。

## 后果

- "为什么 gpt55 要走 handoff"从一次字段巧合变成一条**可评审的记录**：`claude_gpt55` 三族齐发（launch=codex 或 content-derived、session=claude、resume=codex），P3-02 裁决 G18 时可以直接问"这三个里哪个是错的"，而不是从三处代码里反推。
- 契约再扩一个可选字段，仍然在 v1 内（ADR-009：兼容性由具名能力承载，不 bump 版本）。`legacy-task.json` 的 `familySplit` 段要长出第三个字段名，Rust/Swift/TS 三方各自断言。
- 风险要写明：字段一多，"某处仍拿错字段"的可能性也增加。因此门禁必须断言 `resumeFamily` 的读取点只有那三个消费者，且 `session_family` 不再被 `can_native_resume` 使用——否则这次拆分只是把一个巧合换成了三个。

## 纠正（同一次拷问里查出的两处我自己的错误）

1. **P2-04 的 parity 表里有一行是我编的**：`src/test/task-core-parity.test.ts` 的 `OPTIONS` 手写 `claude_gpt55` 为 `codexLike:false / family:"claude" / label:"Claude (GPT-5.5)"`，而客户端真实答案是 `codexLike:true / family:"codex" / label:"Claude Gpt55"`。45 条涉及它的用例是照着这行假数据算的。已改成用 `agentOption()` 派生（内置四个同理），重算后 **283 条里 2 条期望翻转**：`nativeResume:builtinsTrustEachOther` `true → false`、`strategy:builtinsTrustEachOther:native=true` `"resume" → "handoff"`。Rust 侧同步重放通过。
2. **我在 Q58 提问时说"今天 claude↔gpt55 不能原生续跑"，但给的理由不完整**：真正的机制不是记录的族算错，而是记录侧（claude）与目标侧 `agentOption`（codex）本来就不同名同义。上面表格是我犯错后重测的结果。
