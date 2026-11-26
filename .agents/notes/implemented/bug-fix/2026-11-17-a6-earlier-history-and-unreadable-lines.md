# Agent Note: A6 历史能翻到更早，读不出的行要说出来

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A6。2026-11-17 在 `main@1e682f44` 加未提交的 A3–A5 等改动上复核：

- **历史只读前 24 页。** 宿主 `session.history`（`aeroric-host/src/services.rs` 的 `session_history`）每次整份读会话文件、全部解析，再按回复大小切页（一帧 1 MiB，页预算 640 KiB），游标是"还剩多少条更早的消息"。App 的 `SessionsClient.fullHistory` 最多翻 24 页，两个调用方（`LiveSessions.swift` 的 `historyWithStats`、`TaskWorkspaceView.swift` 的 `HostSessionRouteActions.loadPage`）只取消息和统计，`hasMore`、`nextCursor` 被丢掉。工具输出很长的会话，24 页之前的消息看不到，界面也不说。导入会话（`ImportBook.messages`，同样 24 页）一样。
- **读不出的行静默消失。** 四族解析器遇到不是 JSON 的行都直接跳过：Claude、Codex（`sessions/claude.rs` 的 `parse_claude_session_line`、`parser.rs`）、omp（`omp.rs` 的 `parse_omp_session_lines`）、DSH（`dsh.rs` 的 `parse_dsh_session_lines`）。导入路径（`import.rs` 的 `messages`）同样。
- **DSH 整份被拒也静默。** `parse_dsh_session_lines` 遇到比 Aeroric 认识的更新的日志格式版本时返回错误，宿主 `parse_messages` 用 `unwrap_or_default()` 把它变成空历史，会话页显示"发一条消息开始"。
- 远程会话从 SSH 读文件末尾最多 32 MiB（`tail -c`），第一行可能是半截。

## Decision

**宿主在第一页说出"读不出什么"；App 在会话顶部提供"加载更早的消息"，翻过的深度在每轮结束重载时保持。**

### 宿主与 core

- core `sessions/history.rs` 新增 `Unreadable { count, firstLine?, reason? }` 和纯函数 `unreadable_lines(lines, Window)`：数不是 JSON 的非空行，记第一处的行号。
- `Window` 说明这些行是文件的哪一段：
  - `numbered`：从文件第一行开始，下标就是行号；
  - `cut_head`：从文件中间开始读（字节截尾），第一行可能是半截，不判；
  - `open_tail`：文件不以换行结尾，最后一行可能正在写，不判。
- 计数放在宿主这一层统一做，不分别改四个解析器：`session_history` 本来就把整份文件读成行，四族都是一行一条 JSON，这里的行号是准的。
- `parse_messages_checked` 把 DSH、omp 解析器的整份拒绝交出来；原来的 `parse_messages` 包装它，其他调用方行为不变。拒绝的原因放进 `Unreadable.reason`。
- `session.history` 只在第一页（没有 `cursor`）的回复里加 `unreadable`，和 `stats` 一样；什么都读得出时不出现。远程读满 32 MiB 时按截尾处理，只给条数、不给行号。
- 导入：`ImportStore::messages_with_unreadable` 多返回一个 `Unreadable`，`messages` 包装它（十来个调用方不用改）。整份读取用 `Window::whole`；超大文件按行读末尾，不是半截，但行号不是文件的，不给行号。从数据库读的来源（OpenCode、Cursor）没有"行"，不给。`imports.history` 第一页带 `unreadable`。
- `session.history` 的回复不在 `contracts/native/v1` 里，新字段只加不改。

### App

- 协议 `SessionPage.unreadable`（`SessionUnreadable`），文字：
  - "历史文件里有 N 行读不出来（第一处在第 X 行）"；
  - 只读了末尾时写"（在读到的末尾部分里）"；
  - 整份被拒写"历史读不出来：<原因>"。
- `fullHistory(maxPages:)` 带回第一页的 `unreadable`，最后一页的 `hasMore`/`nextCursor` 说明还有更早的。
- `LiveSessionActions`、`SessionRouteActions` 各加 `history(taskID:pages:) -> LiveHistory`（消息、统计、`hasEarlier`、`unreadable`），默认实现包装旧方法，其他实现不用改。
- 三处会话页的顶部加同一个 `EarlierHistoryBar`：
  - 还有更早的消息时显示"加载更早的消息"按钮（载入中写"正在载入…"）；
  - 有读不出的行时显示一行黄色提示，悬停说明"这些行不是完整的 JSON，没有显示出来；通常是 Agent 写文件时被打断。文件本身没有改动"。
- 每点一次多读 24 页（`LiveHistory.pageStep`）：
  - 实时会话页（`LiveSessionStore.loadEarlierHistory`）记住页数，之后每轮结束重载时按这个深度读；
  - 导入会话页（`ImportedTranscript`）和 DSH 终端会话页（`SessionRouteView`）也记住页数。
- 每次重载都从最新一页往前翻，宿主每页都整份解析文件，所以翻得越深越慢；这是现有分页方式的代价，没有改宿主的游标。

### 和批次笔记的出入

- 批次笔记说"各族历史解析函数返回跳过了几行"、"`SessionPage` 的统计里加 `skipped_lines`"。改成在宿主读文件的地方统一计数、放在回复的 `unreadable` 里：一处代码管四族，还能把 DSH 的整份拒绝一起说出来。
- 批次笔记说提示写在"会话信息里"。改为写在会话顶部：读不出的行影响的是看到的消息，放在消息上方最容易被看到；会话信息面板没有动。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 原地补 A6 的落地情况和出入；A6 状态和待目检项回写路线文档 §8 |
| [A5](./2026-11-17-a5-client-method-names-follow-the-host.md) | 无关 | 不动 |

## Alternatives considered

**`fullHistory` 不设上限，一直翻到第一条。** 最强的理由是不需要按钮，打开就是全部。没选：宿主每页都整份读、解析文件，翻 N 页要 N 次整份解析，几百 MB 的会话打开会很慢；24 页上限本来就是为此设的。按需加载只在用户要看时付这个代价。

**宿主改成字节游标的反向分页（`read_session_page_lines` 已有）。** 最强的理由是每页只读一段，翻再深也快。没选：统计（`transcript_stats`）、回合分级、Codex 的格式探测都要整份文件；改游标会动到统计和分级，超出 A 批的范围。

**每个解析器自己数跳过的行。** 最强的理由是"解析器最清楚哪些行没用上"。没选：四个解析器还会有意跳过认识但不显示的行（系统消息、空内容），那些不是"读不出"；"不是 JSON"是对四族都成立、和格式无关的标准，在一处数就够。

**读不出的行在会话里原样显示。** 最强的理由是用户能直接看到内容。没选：这些行通常是写到一半的 JSON，原样显示是一段乱码；行号足够用户去文件里找。

## Consequences

- **收益**：任何会话都能一直往前翻到第一条消息；文件里读不出的行、整份读不出的文件都会在会话顶部说出来，不再看起来像"没有内容"。
- **代价**：
  - 翻得越深，每次重载越慢（每页整份解析）；翻过的深度会保持到页面关闭。
  - 远程会话超过 32 MiB 时，只读末尾，"加载更早的消息"也翻不到被截掉的部分（这部分从来没读到）。
  - "不是 JSON"的标准抓不到"是 JSON 但字段不对"的行。
- **验证**：
  - core：`unreadable_tests`（中间坏行带行号；正在写的最后一行不判，写完后判；截尾不给行号、跳过半截；干净文件没有结果）；`import_tests::an_unreadable_line_of_an_imported_file_is_counted`。
  - host 单测 `unreadable_history_tests`：DSH 日志版本 99 被拒时原因和坏行一起带上，旧的 `parse_messages` 仍返回空；干净的 Codex 文件没有结果。
  - host 端到端 `a_transcript_line_that_does_not_read_is_reported`：假 Codex 的会话文件中间夹一行坏行，第一页 `unreadable` 是 `{count: 1, firstLine: 2}`，两条好消息照常显示；带 `cursor` 的后续页不重复。
  - Swift：`SessionUnreadableTests`（解码、三种文字、没有内容时为空）；`LiveSessionStoreTests.testEarlierHistoryIsOfferedAndTheDepthIsKept`（第一次读 24 页、有更早的；加载后读 48 页、没有更早的就不再请求；重试保持 48 页；提示文字）。
  - 2026-11-17 在 Mac 上：`cargo fmt --check`（只格式化本次改动的文件）、`cargo clippy` 通过；`cargo test --workspace --no-fail-fast` 3744 通过、0 失败；`swift test`（AeroricKit 961 个，跳过 1 个；iOS 35 + 110 个）0 失败；App 在临时 DerivedData 里构建成功；`pnpm verify-notes` 通过。
  - 没有造超过 24 页的真实会话做端到端；界面没有目检，记在路线文档 §8 待目检清单。
- **2026-11-18 补强**：导入的 omp/DSH 会话计坏行、尾部读不判最后一行、远程截断按退出码、去掉 BOM（B1-8），见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。
