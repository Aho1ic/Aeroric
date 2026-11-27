# Agent Note: 目检 1126c：顶栏停止的轮写“已完成”、MCP 启动失败的提示一闪就没、全局记忆页没有加载用量、命令行技能的开关不说明

Status: implemented

## Problem

目检报告在 `~/Documents/computer-use/2026-11-27-0008-aeroric-review-1126c/report.md`，测的是 `d1573132`（[1126b](2026-11-26-review-1126b-findings.md) 的修复）。

**通过的：**
- 顶栏“停止”后正文不再空白：5 秒、30 秒、上下滚动、切走再回来都在（1126b 的修复）；
- 手机插话后停止：Mac 上这一轮写“已中断，用时 47.6 秒”，并出现“停止之前发的补充说明……已一并停止”的提示；
- 手机点“断开”写“你点了“断开””；
- 会话信息的技能、入口、工具三行，输入框停止后写“已中断”且不进“需要我处理”，自动行为页，结果结构填 `[1]` 当场拦下。

**查出的问题：**

1. **顶栏“停止”的那一轮，标题写“已完成，用时 1 分 15 秒”**，顶栏写“已取消”。
   - 顶栏“停止”走 `task.cancel`，宿主直接结束 Claude 进程。Claude 来不及写 `[Request interrupted by user]`，[1126a](2026-11-26-review-1126a-findings.md) 靠这句标“已中断”，这里用不上。
   - 这一轮的会话文件停在一条工具结果上：`assistant`（`stop_reason: tool_use`）→ `user`（`tool_result`）→ 一次 `api-request`，后面没有回答。正常结束的一轮，最后一条是 `stop_reason: end_turn` 的回答。
2. **能力包里命令不存在的 MCP 服务器，Claude 会话里看不到“没连上”的红色提示。**
   - 本次启动的记录（`tool-catalogs/<task>.json`）写着 `missing-probe` 已写进 `--mcp-config`；
   - 用同样的配置单独跑 Claude Code 2.1.330，`system/init` 里是 `{"name":"missing-probe","status":"failed"}`，驱动据此发出提示；
   - 但提示是“进行中的行”：这一轮结束后，页面用会话文件的历史换掉进行中的行（`clearItems`），而 Claude 的会话文件里没有 Aeroric 的提示。一轮只有 3 秒，提示一闪就没了。
3. **全局记忆页没有“新会话会带入 N / 1200 字”。**
   - 宿主 `memory.profile` 回得好好的（1211 / 1200 字，11 条，另有 9 条没带上）；
   - `MemoryService.load(projectID:)` 只写在协议扩展里，不在协议本体。经协议类型调用时，Swift 走的是扩展里的默认实现（回 `nil`），App 自己的 `HostMemoryService.load` 从没被调到。这一行从 E8 起就没显示过。
   - 扫了 macOS 端全部协议，只有这一处。
4. **Agent 配置 → 技能 里 `aeroric` 开关是开着的，没说它其实没挂给任何 Harness。** 技能目录（C7）写了原因，这一页的列表来自另一个宿主方法 `builtinSkills.list`，没带这句。

**不是 App 的问题（清单写错或操作偏了）：**
- F2“旧会话写未记录”：挑的会话是 11-24 建的，那时已经在记录，清单应挑 11-20 以前的；
- F3 `@任务`：在新建会话页的输入框里试的，`@` 引用只在会话页的输入框里有；
- F8“结果不合格”：要求 `ticket` 字段，模型给了 `"ticket": ""`，按结构就是合格的，清单没设计出 Agent 给不出的字段；
- R3 退到后台的提示、F4 悬停“正在停止”：转瞬即逝，没截到。

## Decision

**Claude 的一轮停在半路时，按会话文件认出来**（`transcript_stats.rs`）。
- 一轮里 Claude 已经说过话或调过工具（`worked`），并且停在等 Claude 的状态（发出了工具调用，或者收到了工具结果，之后没有回答），就是停在半路；
- 后面又开了新的一轮：前一轮记 `interrupted`，这一轮确实被停了；
- 是最后一轮：记新字段 `unfinished`，因为它也可能还在跑；
- Claude 写了中断说明、或者这一轮是 API 错误，照旧按 `interrupted` / `failed`。
- 页面：`unfinished` 的标题写“已中断”；会话页上最后一轮还在跑（回合进行中或还有进行中的行）时不算（`LiveSessionView.markingFailure`）。

**Aeroric 的警告留到下一轮结束**（`LiveSessionModel.clearItems`）。
- 这一轮里 Aeroric 自己发的 `warning` / `error` 提示（MCP 没连上、停止前的补充说明没执行……）不在任何会话文件里，历史换掉进行中的行时保留；
- 下一轮结束时清掉，不会越积越多；普通的 `info` 照旧随这一轮清掉。

**`load` 写进 `MemoryService` 协议本体**，扩展里的默认实现留给测试替身。

**内置技能列表带上 `withheld`**（`builtinSkills.list`）：命令行技能在每家都有 Aeroric 工具时，写“只给没有 Aeroric 工具的 Harness；现在这几家都有工具，没有挂给任何一家”，技能页那一行显示它。

## Alternatives considered

**按 `stop_reason` 判断一轮有没有结束。** 最强的理由是直接：正常结束的一轮最后是 `end_turn`。没选：Claude Code 写会话文件时，同一条回答拆成好几行，中间的行 `stop_reason` 常是空的，不同版本写法也不一样；“最后是工具调用或工具结果、后面没有回答”只看内容块，更稳。

**顶栏“停止”先发 `interrupt`，等 Claude 写完中断说明再结束进程。** 最强的理由是会话文件自己就说清楚了。没选：顶栏停止要立即释放进程，进程卡住时也要能停；而宿主重启、崩溃同样会留下停在半路的轮，只能从会话文件认。

**把 Aeroric 的提示写进会话的历史（单独存一份，打开会话时合进来）。** 最强的理由是重开会话也看得到。没选：要新的存储和合并规则；这类提示主要在当下有用，留到下一轮结束已经够看。重开后看不到，是局限。

**不改协议，在 `HostMemoryService` 外面包一层具体类型。** 没选：根子是“方法只在扩展里”，加进协议本体一行就修好，以后别的实现也不会再踩。

## Consequences

- **收益：**
  - 顶栏停止、宿主重启打断的 Claude 轮，标题和顶栏一致；
  - 短的一轮里 MCP 没连上之类的提示看得见；
  - 记忆页有了加载用量这一行（E8 原本就该有）；
  - 技能页说清命令行技能为什么开着却不起作用。
- **局限：**
  - 一轮在 Claude 说出第一句话之前就被停，认不出来，照旧写“已完成”；
  - 最后一轮的 `unfinished` 靠页面知道它是否还在跑；只读的历史页（导入的会话）一律写“已中断”；
  - 保留的提示重开会话后就没了（[1127a](2026-11-27-review-1127a-findings.md) 起 `session.state` 带最近 5 条，页面打开时补上；它也查出新会话的页面打开得比提示晚，这次的改法单独管不到）；
  - Codex 被结束进程的轮早已按 `task_started` 认出，这次没改。
- **验证：**
  - Rust：`transcript_stats` 的 `a_claude_turn_cut_mid_work_is_not_completed`（按 1126c 的真实顺序：调用、结果、没有回答；后面还有一轮、在调用中被停、正常答完、有中断说明四种）；
  - Swift：`LiveSessionModelTests.testAWarningOutlivesItsTurnsLiveRows`、`MemoryTests.testThePageReadsTheServicesLoad`、`SessionPolishTests` 的“已中断，用时 1 分 15 秒”和解码、`BuiltinSkillsTests.testStatusLines`；
  - 页面效果留给下一轮目检；
  - 全量验证时 `delegation::tests::a_delegation_keeps_its_invariants_from_start_to_a_late_proven_delivery` 偶发失败一次：测试复制数据目录时，后台写入把一个临时文件改名挪走，`copy_tree` 读到 `NotFound`。单独跑 5 次都过。改为跳过复制途中消失的条目，它本来就不是写完的文件。
