# Agent Note: B4 检查结果保留原始判定

Status: implemented

## Problem

[调研落地批次 B](../../proposed/feature/2026-11-15-adoption-b-acceptance-and-artifacts.md) 的 B4。自动检查跑完后，留下来的信息很少：

- 回合账本的 `TurnCheck` 只有 `passed` 和一句汇总（"测试 3 项失败"）；
- 每项检查（`AutoCheckItem`）有状态、摘要、失败列表和输出尾部，但没有退出码、问题数、耗时；
- 输出超过 4000 字、失败超过 20 条时被截掉，没有任何标记，看不出结论是不是只基于一部分输出；
- 审查门禁读到的检查文字只有"全部通过 / 有失败"加每项摘要；
- "通过"的规则（`overall`）没写明：所有项都 `passed` 才算通过，没有"必须项"和"可选项"之分。

## Decision

- **每项检查留下原始判定**（`AutoCheckItem` 加可选字段）：
  - `exitCode`：跑完时的退出码，超时、取消、启动失败为空；
  - `issues`：失败的测试数或诊断错误数，输出里读得出时才有；
  - `durationMs`：耗时；
  - `optional`：可选项；
  - `truncated`：输出或失败列表被截断。
- **通过规则**写进 `overall` 的文档并测试：
  - 所有必须项 `passed` 才算通过；
  - 可选项失败照样显示，但不拖垮整轮；
  - 超时、出错、取消的一项不算通过。
  - 目前没有哪类检查是可选的，`CheckSpec.optional` 都是 `false`，规则先就位。
- **回合账本**：`TurnCheck` 加 `items`（每项的名称、是否必须、判定、退出码、问题数、耗时、是否截断）和 `incomplete`。有项被截断、出错或超时时，`incomplete` 为真，整轮记录由新的 `auto_check::turn_check` 统一生成。
- **审查门禁**：交给审查者的检查文字逐项写出原始判定（可选、退出码、超时、没跑完、输出被截断），不完整时开头标"结果可能不完整"。
- **App**：
  - 检查行每项下面一行原始判定，例如"退出码 101 · 3 个问题 · 2.4 秒 · 输出被截断"；
  - 不完整时标题加"（结果可能不完整）"。

## Alternatives considered

**把截断的输出全部留下，不再截。** 最强的理由是不会不完整。没选：检查输出会进会话更新和回合账本，几 MB 的测试日志会把它们撑大；截断本身没问题，问题在于没告诉人。

**按检查种类定必须与可选（诊断可选、测试必须）。** 最强的理由是马上有用。没选：项目对诊断的要求各不相同，定错了会让本该失败的回合显示通过；先把规则和字段放好，等有明确需求时由项目设置决定。

## Consequences

- **收益**：
  - 检查失败时能看到退出码、问题数和耗时；
  - 截断和没跑完不再被当成完整的结论；
  - 审查者拿到的是逐项判定。
- **代价**：每项检查的更新多几个字段，可选字段为空时不写出。
- **验证**：
  - `auto_check_tests` 新增两个测试：
    - `a_check_keeps_its_exit_code_issues_duration_and_cuts`：退出码、耗时、截断、超时无退出码；
    - `only_required_checks_decide_and_a_cut_run_is_incomplete`：可选项失败仍通过，必须项超时失败且不完整，被截断的通过也标不完整；
  - Swift 新增 `testACheckShowsItsRawVerdictAndWhenItMayBeIncomplete`；
  - 界面待目检，记在路线文档 §8。
