# Agent Note: E5 第二步：已经对外动手的回合，重启后不自动重跑

Status: implemented

## Problem

[1118 调研](../../proposed/architecture/2026-11-19-new-addition-1118-research.md) 的 E5 第 2 项。[宿主重启后续跑](2026-11-14-resume-after-restart.md) 会在自动继续打开时，把被重启打断的无人值守会话（定时任务开的、委派的子会话、交办的待办）自动续一次。续跑时只发一句固定的话："先看看工作区现在的状态，再从中断处接着做，做完的不要重做"。

工作区能看出本地文件改到了哪里，看不出对外做过什么。被打断的那一轮如果已经执行过这些操作，续跑的 Agent 很可能再做一遍：

- `git push`；
- `gh pr create`；
- `npm publish`；
- 通过 MCP 发出一条消息。

重复的推送、重复的 PR、重复的消息，都比停下来更糟。

OMB 运行时契约第 5 节的规则是：宿主说这一轮已经对外动过手时，不重跑，直接以"中断"结束并告诉用户。

## Decision

- **从转写判断，不听模型说**（core `sessions/outward.rs`）：
  - 用会话自己的转写（`transcript_stats` 解析出的每轮工具调用：名字、命令的第一行、有没有结果、有没有失败），找到重启时正在跑的那一轮（最后一次重启之前最后开始的一轮）；
  - 逐个工具调用判断它是不是对外动作。
- **哪些算对外动作**：
  - **命令**：在行首，或在 `&&`、`;`、`|`、`sudo`、`npx` 之后出现的 `git push`，以及：
    - `gh pr` 的 create、merge、comment、review、close、edit，`gh issue` 的 create、comment、close、edit，`gh release create`、`gh workflow run`；
    - 各种 publish：npm、pnpm、yarn、cargo、twine、gem、pod trunk、docker、helm；
    - 部署：kubectl apply、delete、rollout，terraform apply、destroy，vercel、netlify、fly、firebase、wrangler；
    - sendmail。
  - **写请求**：`curl` 和 `gh api` 带写方法（`-X POST` 等）或请求体（`-d`、`-F`、`-f`）。
  - **MCP 工具**：名字里有 send、post、publish、merge、comment、reply、push、submit、invite 这几个词，或者是 create_issue、create_pull_request、create_release 这类调用。Aeroric 自己的上下文工具不算。
  - **不算**：本地的 `git commit`；读操作（`gh pr view`、不带写方法的 `curl`）；失败了的调用。
  - **还在执行的**：被打断时还没有结果的调用记作"被打断时还在执行"，因为它可能已经生效。
- **宿主重启后的自动处理**（宿主 `restart::resume_settled`）：这一次收尾的每个被打断会话都先判断一遍；有对外动作时：
  - 在记录上写 `outwardBeforeCut`（例如 `["git push"]`）；
  - 不自动继续，开关打开、无人值守也一样；
  - 定时任务的运行记录写"被 Aeroric 重启打断；这一轮已经执行过 git push，没有自动继续，等你决定"。
- **用户手动继续**：仍然允许，但续跑消息改为：
  - 开头加宿主信封：`[Aeroric 重启续跑] 来自：Aeroric · 重启后继续上一轮 · 代用户执行：…`；
  - 有对外动作时，追加"被打断前，这一轮已经执行过：git push。不要再执行一遍；先核对它们的结果，再决定还要做什么。"
- **字段的生命周期**：`outwardBeforeCut` 和 `interruptedBy` 一起，任何不是 `interrupted` 的状态切换都会清掉它（core `reducer`）。
- **App**：
  - "需要我处理"里那一行写"Aeroric 重启打断 · 已执行过 git push"；
  - 会话页顶部的提示写"……被打断前它已经执行过 git push，所以没有自动继续；继续前先核对，避免做两遍。"

## Alternatives considered

**从检查点的副作用记录（`effects`）判断。** 最强的理由是检查点已经把命令、MCP 调用记成副作用了。没选：检查点要到回合结束才写盘，被重启打断的那一轮恰好没有结束，它的副作用多半还在内存里，跟着宿主进程一起没了。转写是 Agent 进程自己写的，宿主重启后还在。

**让模型自己判断做过什么。** 没选：OMB 的规矩是"从宿主看到的活动里读，从不取模型写的文字"。模型可能记错，也可能根本没机会说就被打断了。

**有对外动作时直接把会话标成失败。** 最强的理由是和 OMB 一样干脆。没选：Aeroric 原来的"继续"按钮已经交给用户决定；只要不替用户自动重跑，再把要核对的事写进续跑消息就够了，没必要另造一个状态。

## Consequences

- **收益**：无人值守的会话在推送、发布、发消息之后被重启打断时，不会被自动续跑做第二遍；用户点继续时，Agent 也会被提醒哪些事不要重做。
- **代价**：每次宿主启动，对每个被打断的会话读一遍它的转写。
- **局限**：
  - 只认得列出来的命令和 MCP 动词，别的对外动作（自定义脚本里的推送、`ssh` 到远程执行）认不出来；
  - 命令只看第一行，最多 80 个字（`ToolSpan.detail`），写在多行脚本后面的推送认不出来；
  - 转写里根本没来得及写下的调用，也无从知道；
  - [无人值守自动重试](2026-11-14-unattended-auto-retry.md)对"没跑成"的回合仍按原规则重试，没有做这项判断。没跑成的回合通常没走到对外动作，但并不保证。
  - E5 其余两项（续跑卡住守卫、不变量测试）还没做。
- **验证**：
  - `outward.rs` 新增 2 个测试：
    - 各类命令、写请求和 MCP 工具的判断，包括 `echo "git push"`、`git commit`、`git pushd`、读请求、Aeroric 自己的工具都不算；
    - 只看被打断的那一轮；失败的调用不算；同一动作只记一次；没有结果的标"被打断时还在执行"。
  - core `reducer` 测试补充：切到运行中后 `outwardBeforeCut` 被清掉。
  - 宿主新增：
    - 集成测试 `a_cut_turn_that_already_pushed_is_not_continued_on_its_own`：委派子会话的转写里有一次成功的 `git push`，宿主重启后打开自动继续，记录写上 `["git push"]`，会话仍是 `interrupted`，没有自动继续；
    - 单元测试 `the_continue_message_names_what_not_to_repeat`。
  - Swift 新增 `testACutTurnThatActedOutwardSaysWhat`。
  - 没有用真实 Agent 和真实重启跑过（NOT RUN）。界面待目检，记在路线文档 §8。
