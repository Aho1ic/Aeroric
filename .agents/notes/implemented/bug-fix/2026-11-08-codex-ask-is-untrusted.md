# Agent Note: Codex 的"每次询问"改用 untrusted 加工作区沙盒，命令先问、同意后做得成

Status: implemented

## Problem

"每次询问"的界面文案是"执行命令和改文件前都先问你"，结构化 Codex 会话却把它映射成 `on-request` 加只读沙盒（`codex/map.rs::codex_policy`）。第二批的第一次目检就撞上了：子代理直接在沙盒里跑 `touch`，得到 `Operation not permitted`，用户从头到尾没被问过。

2026-11-08 用真 Codex 0.167 加假模型复现（模型跑 `touch probe-exec.txt` 或带它的复合命令，不申请提权；再用 `apply_patch` 加一个文件；客户端同意所有审批）。直接调用 `exec_command` 和 gpt-6.x 的代码模式（脚本里调 `tools.exec_command`）结果一样：

| 审批 / 沙盒 | 命令 | 改文件 |
|---|---|---|
| `on-request` / 只读（原"每次询问"） | 不问，沙盒里失败，也没有命令条目 | 先问，同意后写入 |
| `untrusted` / 只读 | 先问；同意后约一半仍在沙盒里跑、失败（同一请求反复跑，线上帧完全相同） | 先问，同意后写入 |
| `untrusted` / 工作区可写 | 先问，同意后执行成功（12/12） | 先问，同意后写入 |
| `on-request` / 工作区可写（"自动编辑"） | 不问，直接执行 | 不问，直接写入 |

C 批第一版选的是 `untrusted` 加只读，目检里批准后的 `touch` 就落在了失败的那一半。

## Decision

- `ask` 映射为 `untrusted` 加工作区可写沙盒：Codex 认为安全的只读命令直接跑，其余命令和改文件都先问；同意后的命令即使仍在沙盒里跑，也能写项目目录。实际上 Codex 认得的很少：复测里 gpt-6.1-sol 用 `/bin/zsh -lc ls` 跑 `ls`，也弹了审批。
- `plan` 单独保留 `on-request` 加只读：规划档不该写，只在 Agent 想出沙盒时问。只读委派一律用 `plan`，不受这次影响。
- 界面文案不变，现在和行为一致。

## Alternatives considered

- **保持映射，只改文案为"改文件前问你；命令只能只读"**：改动最小。没有采用：用户想要的是"先问"，不是"先失败"；失败的命令连条目都没有，用户不知道发生过。
- **`untrusted` 加只读**：沙盒最紧，C 批第一版用的就是它。没有采用（目检失败后用户选了工作区沙盒）：同意以后能不能做成看运气，用户批准了却得到 `Operation not permitted`。
- **`granular` 审批策略**：能逐类开关。没有采用：0.162 的契约里刚出现，字段还在变。

## Consequences

- **收益**：选"每次询问"时，写类命令会出审批卡，同意后做得成；和 Claude 的同名档一致。
- **代价**：
  - 几乎每条命令都会问，连 `ls` 也问（2026-11-08 复测），除非用户点"本会话都允许"。哪些命令算安全由 Codex 判定，Aeroric 改不了。
  - 沙盒从只读放宽到工作区可写。不经询问就跑的只有 Codex 认定的只读命令，所以不会出现"没问就写"；但只要 Codex 的安全命令判定出错，写入就不再有只读沙盒兜底。
- **影响面**：所有以"每次询问"开的结构化 Codex 会话，包括它们的子代理（继承父线程的策略）和以"每次询问"开出的可写委派子会话。PTY 启动不带参数，用的是 Codex 自己的默认，不在这次范围内。
- **需要重访的信号**：Codex 修好 `untrusted` 加只读下批准后仍在沙盒里跑的问题，或 `granular` 稳定下来。
- **目检**：第一次（`2026-11-08-1732-aeroric-batch-c-review`）用只读沙盒，批准后的 `touch` 失败；改映射后复测（`2026-11-08-1913-aeroric-batch-c-ask-retest`）三条命令都先问、批准后都成功，文件建出来了。
- **验证**：`codex/map_tests::policies_and_version_gate`、`executor_tests` 的 `thread/start` 参数、宿主 `live_codex` 的 `turn/start` 参数；第二批的 `codex/real_tests.rs` 用"每次询问"跑，改映射后在 0.167 上重跑通过。上表来自 Mac 上的一次性探针（真 `codex app-server` 加假 Responses 服务），没有留成测试。
- 第二批笔记 [Codex 原生子代理](2026-11-07-codex-subagent-threads.md) 里"留给 C 批"的那一条由本篇了结。
