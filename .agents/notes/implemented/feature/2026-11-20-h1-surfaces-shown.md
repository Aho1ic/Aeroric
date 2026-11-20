# Agent Note: H1 第三步：会话信息里显示这次启动的入口

Status: implemented

## Problem

[H1 第一步](2026-11-20-h1-surfaces-per-launch.md) 让宿主在每次启动时记下各入口的实际情况：系统层、工具、技能、事件、子代理、命令行。记录由 `session.capabilities {taskId}` 的 `surfaces` 返回，但 App 不读它，用户看不到。

方案里"用户会看到"这一条写的是：会话信息里，除了工具清单，还有一行"这个 Harness 通过哪些入口用 Aeroric"，每项写"已接通""这家不支持"或"还没核对"。

动手前读了 App 一侧：

- 会话信息（`SessionInfoCard`）的"工具"一行由 `SessionToolsSection` 显示（[C8 第一步](2026-11-20-c8-effective-tool-catalog.md)）；
- 它通过 `SkillCatalogBook.loadTools` 调 `session.capabilities {family, taskId}`，回复里本来就带着 `surfaces`，只是没有解码。

## Decision

**协议**：Swift 新增 `LiveSurfaceLaunch`（`atMs`、`harness`、`place`、`surfaces`），每一项沿用已有的 `LiveCapabilityEntry`。

**读取**：`loadTools` 用同一次回复同时填 `tools` 和 `surfaces`，不多发请求。"入口"一行自己不加载，和"工具"一行放在同一个条件下显示，由"工具"那一行触发读取。

**界面**：会话信息里，"工具"上面加"入口"一行。

- 收起时写"接通 N/6：系统层、工具……"，只列已接通的入口。
- 点开逐行写"名字 · 状态 · 原因"。状态四种：已接通、没有、有差异、还没核对。没接通的用浅色字。说明可以折行，因为原因往往比一行长。
- 读取中写"读取中…"。会话在 H1 之前启动、之后没重启过的，写"未记录（这个会话在记录入口之前启动）"。

**待目检**：界面改动记进路线文档 §8 待目检清单。

## Alternatives considered

**把入口并进"工具"一行。** 最强的理由是会话信息少一行。没选：工具一行列的是一个个工具，入口是六个通道，"工具"只是其中之一，合在一起会让"工具"一行同时回答两个问题。

**"入口"一行单独调一次 `session.capabilities`。** 最强的理由是两个小节互不依赖。没选：两个小节总是一起出现，回复本来就同时带着两份记录。分开调就是同一个请求发两遍。

**收起时列出没接通的入口。** 没选：没接通的往往占多数（命令行、事件现在哪家都没有），列出来那一行会很长；接通的才是用户想确认"它能不能用"的那几项。没接通的在点开后逐项写原因。

## Consequences

- **收益**：用户能在会话信息里看到这个 Harness 通过哪些入口用到 Aeroric，没接通的写明原因。H1 的三步到此做完。
- **局限**：
  - 只显示最近一次启动，不比较前后两次启动的差别；
  - 没有界面能看整张能力表（各家、本机和远端对照），只在 `docs/subsystems/agent-capabilities.md` §6。
- **验证**：
  - 新增 1 个 Swift 单测，覆盖解码、同一次读取、汇总文字、逐项文字和"未记录"；
  - Swift 三个测试套件和 `native:build` 都要通过；
  - 界面未目检，已记入待目检清单。
