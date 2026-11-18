# Agent Note: 目检发现的两处——大导入会话读不出来、撤回撤销后回合仍写"已撤销"

Status: implemented

## Problem

2026-11-17 晚 A4–A11 目检报告（Codex Computer Use，`~/Documents/computer-use/2026-11-17-2154-aeroric-a4-a11-review/report.md`）列出两处问题。

1. **导入会话一直显示"正在读取会话历史…"。** 打开 Codex 导入会话 `imp_f8198b…` 等了 85 秒，正文始终没出来。2026-11-18 在 Mac 上复现：
   - 源文件是 121 MB 的 Codex rollout，`imports.history` 每读一页要 6.5 秒；
   - 宿主每读一页都把整份文件重新读一遍、重新解析，再切出那一页；
   - App 的 `ImportBook.history` 要先连读最多 24 页（`maxHistoryPages`）才显示，于是几分钟都是空页面。
2. **撤回撤销后，回合底部仍写"产品失败 · 已撤销"。**
   - 这行字由宿主读历史时按回合账本打分（`turn_outcome::grade`：账本状态是 `undone` 就记产品失败）；
   - 会话页撤销或撤回后只刷新了回合列表（`refreshTurns`），没有重读历史，统计里的打分还是旧的。标题已经回到"待验收"，底部还停在撤销时的结论。

## Decision

1. **解析过的导入文件留一份**：
   - `ImportStore::messages_with_unreadable` 先查会话、检查授权，再按"文件路径、大小、修改时间"查最近一次的解析结果，对得上就直接用；
   - 只留一份（进程级 `LAST_PARSED`），换文件或文件变了就重新解析；
   - 数据库来源（Cursor、OpenCode）不缓存；
   - 授权每次都查，撤销后立刻读不到，不会从缓存里漏出来。
2. **撤销和撤回成功后重读历史**：`LiveSessionStore.applyTurn` 成功后调用 `reloadHistory()`，回合底部的结论跟着账本走。

## Alternatives considered

**App 先显示第一页，后面的页在后台补。** 最强的理由是第一屏立刻出来。没选：宿主每页都重新解析，后台补 24 页仍要几分钟、占着宿主的阻塞线程；缓存之后后续页几乎不要时间，体验已经够。以后若首页本身太慢，再考虑渐进显示。

**缓存按会话 id 存、只在重新扫描时失效。** 最强的理由是不用查文件元数据。没选：Codex 会话还在写时，文件一直在长，按 id 缓存会显示旧内容；按大小和修改时间判断更可靠，代价只是一次 `stat`。

**撤回后只把那一回合的打分在本地改掉。** 最强的理由是不用重读历史。没选：打分规则在宿主（还要看检查结果），客户端照抄一遍会两边不一致；重读一次历史就是唯一的事实来源。

## Consequences

- **收益**：大导入会话从"几分钟没内容"变成首页读完就能看；撤销、撤回后，回合底部的结论和标题一致。
- **代价**：进程里多留一份最近解析的消息，121 MB 的文件大约几十 MB；只留一份，换会话就释放。
- **验证**：见交接单第 5 节"第三轮"与路线文档 §8。
  - 新增 core 测试 `pages_of_one_file_reuse_its_parse_until_it_changes_or_is_revoked`：同一文件两次读到相同内容，文件变了就读到新内容，撤销授权后立刻拒绝；
  - Swift `testUndoAndRestoreReadHistoryAgain`；
  - 在 Mac 上用重建后的宿主，逐页计时读取那份 121 MB 会话（结果写在交接单）。
