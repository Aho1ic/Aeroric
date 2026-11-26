# Agent Note: 目检 1126a：手机发帧乱序被 Mac 断开、顶栏停止后正文空白、Claude 停下的轮写“已完成”

Status: implemented

## Problem

目检报告在 `~/Documents/computer-use/2026-11-26-1140-aeroric-review-1126a/report.md`，测的是 `88aff625`。

**通过的：**
- R1 收到 SIGTERM 时 App 和宿主 0.5 秒内一起退出；
- 配对链接首地址是 `en0`、没有虚拟机网桥（[1124e](2026-11-25-review-1124e-findings.md) 的修复）；
- 手机插话后停止、实时审批、删除行、非 Git 项目、断线后自动连回与中文离线提示、DSH 附加的上下文。

**查出的问题：**

1. **手机在日常操作中短暂掉线：** 一共 6 次，打开任务、切换项目、点停止时都写“和 Mac 的连接断开了”，几秒后自己连回。1124c 记下的“隔几分钟断线一次”就是它。
   - 模拟器的系统日志里，每次都是 `read-side closed`、`Socket is not connected`（57），没有 WebSocket 关闭帧：是 Mac 直接断了 TCP；
   - 加密帧的序号要求严格递增，`server.rs` 收到解不开的帧（乱序、重放、篡改）就 `break`，不发关闭帧；
   - 手机的 `CompanionLink.sendControl` 在主线程加密（这时定下序号），再 `await socket.send(binary:)`。`URLSessionCompanionSocket.send` 不在主线程上，调用前先换一次执行器。两个请求同时发出时（任务页同时读会话和改动，切项目同时读文件和 Git 改动），先拿序号的不一定先到 socket，Mac 收到乱序的帧就断开；
   - [d4-phone-message-and-stop](../feature/2026-11-23-d4-phone-message-and-stop.md) 当时在测试替身的 Mac 一侧见过同样的乱序，结论写成“产品代码没有这个问题”。Mac 这一侧确实没有，但手机发往 Mac 的方向有。
2. **点顶栏的“停止”后，会话正文整片空白：** 85 秒后仍然空白。
   - 顶栏“停止”走 `task.cancel`，宿主关掉驱动，这个任务的事件日志随之消失；
   - 页面下一次 `session.updates` 拿到缺口，于是重新同步：先把进行中的行整块清掉，再读历史；
   - 页面在清掉的那一刻变矮很多，macOS 的滚动视图不把滚动位置收回来；历史读回来时条数没变，“条数变了就滚到底”也不触发。辅助功能树里正文的行全没了，统计栏却照样写“12 轮 12 步”。
3. **Claude 停下的轮，标题写“已完成”：** 顶栏写“已中断”，这一轮的标题却写“已完成，用时 1 分 55 秒”。
   - 标题按会话文件的统计（`transcript_stats`）写；
   - Codex 停下的轮有 `interrupted` 标记；Claude 的 `[Request interrupted by user]` 只被当作“不开新轮”跳过，从来没给那一轮打标记。
4. **小问题：**
   - 手机连 Tailscale 地址 `100.125.106.127` 报 `-1022`（App 传输安全不许明文连这个地址），顶栏写成“网络出错（-1022）”；
   - “配对与连接”页看不到当前连的是哪个地址；
   - Mac 恢复远程访问后，手机最多等 30 秒才重试（B8.2 卡在 30.2 秒）。

**不是 App 的问题：**
- B4b 的“按回车只换行”是清单写错了：Mac 输入框是 ⌘↩ 发送、⌥↩ 等这一轮结束再发；
- 默认配置的模型额度不足、`zzzcoding/gpt-6-astra` 不接受非官方 Codex 客户端，是账号问题。

## Decision

**手机的加密帧按序号顺序发出**（`CompanionLink.sendControl`）。
- 每一帧先等上一帧交给 socket，再加密、拿序号、发送。用一条 `Task` 链串起来，链头记在 `sending`，新连接开始时清掉。
- 等待期间换了连接，这一帧不发，按“没有连接”失败。

**Mac 解不开手机的帧时，先说原因再断开**（`server.rs`）。
- 发带原因 `frame-rejected`（`CloseCode::Protocol`）的关闭帧，再断开；
- 宿主日志记一行设备和原因；
- 手机把这个原因写成“Mac 拒收了这台手机发来的一条消息（顺序不对或已损坏），已断开”（`CompanionLink.closeReason`）。以后再出这种断开，不会和普通的断线混在一起。

**重新同步时，先读历史，再换掉进行中的行**（`LiveSessionStore.resync`）。
- 读完历史后在同一次更新里换掉模型，页面不会先变矮；
- 每次重新同步，`resyncs` 加一。页面看到它变，就滚到最新一条（`LiveSessionView`），和刚打开会话时一样。目检 1126b 证明只滚到底不够，页面仍空白；[1126b](2026-11-26-review-1126b-findings.md) 起改为 `resyncs` 一变就换一个新的滚动视图。

**Claude 的中断标记把它结束的那一轮记为“已中断”**（`transcript_stats.rs`）。`[Request interrupted by user`（含 `… for tool use]`）出现时，最后一轮 `interrupted = true`；它仍然不开新轮、不算一条消息。

**小项：**
- `-1022` 写成“iOS 不允许用明文连接这个地址（App 传输安全）”；
- 连上后，“配对与连接”页的状态写“已连上 <Mac>：<地址>”；
- 重连退避最多 15 秒一次（原来 30 秒），[d4-pairing-and-reconnect](../feature/2026-11-23-d4-pairing-and-reconnect.md) 已同步。

**手机的地址只列私有网段**（`identity::lan_endpoints`，用户 2026-11-26 定）。
- 选中的地址后面只跟私有网段：10/8、172.16/12、192.168/16，WireGuard 的 10.x 在内；
- Tailscale 的 100.64/10、公网地址、透明代理的 198.18/15 都不再列，iOS 不许明文 `ws://` 连这些地址；
- 用户在 Mac 上指定的首选地址，或者本机只有这一个地址时，照样列在第一个。

## Alternatives considered

**手机：加密和发送都放到 socket 自己的串行队列里。** 好处是顺序由 socket 保证，不靠调用方。没选：`E2eeSession` 的发送序号和接收序号在同一个对象里，由主线程上的 `CompanionLink` 管；放到 socket 里，要么把会话搬过去，要么跨线程共享，改动面比串一条 `Task` 链大得多。测试替身也要跟着改。

**Tailscale 地址：给手机 App 加 App 传输安全的例外，放行明文连接。** 好处是在外面也能经 Tailscale 连回 Mac，而且连接本身已经端到端加密。没选（用户定）：这等于放宽 App 的一项系统安全设置；在外面连回 Mac，可以用已有的 Relay 或 WireGuard。

**Mac：允许一个小窗口内的乱序，按序号重排。** 没选：协议定了“乱序一律断连”，这是防重放的边界。乱序本来就是手机的错，应该在手机上改；放宽 Mac 会让所有客户端的错都被藏起来。

**页面：内容变矮时自动收回滚动位置（`defaultScrollAnchor` 或监听内容高度）。** 没选：macOS 14 上 SwiftUI 的 `ScrollView` 没有可靠的“内容高度变化”信号。能修的地方在数据这一侧：别让内容先变矮。

**顶栏“停止”改成只打断这一轮。** 没选：顶栏“停止”的意思是结束这个任务（释放进程、记“已取消”），和输入框里的“停止这一轮”是两个动作。这次只修显示，不改它的含义。

## Consequences

- **收益：**
  - 手机同时发几个请求不再被 Mac 断开，日常操作中的短暂掉线应该消失；
  - 再有帧被拒，手机和宿主日志都写明原因，不会再查一个多星期；
  - 顶栏停止后，会话页落在最新一条，不再空白（1126b 复测仍空白，见上）；
  - Claude 停下的轮，标题和顶栏都写“已中断”；回合分级（R5-1）也随之把它算作停下的轮；
  - 配对链接和 `hello` 不再给手机一个连不上的 Tailscale 地址；万一连到这类地址，原因也说得清楚；连接页看得到当前地址；Mac 恢复后最多 15 秒连回。
- **局限：**
  - 手机的请求现在逐帧发出，一帧在 socket 上卡住，后面的都要等。局域网上每帧只是交给系统，可以忽略；
  - 页面重新同步后总是滚到最新一条，正在往上翻看的人会被带到底部。重新同步只在会话被关掉或日志缺口时发生；
  - 手机不再经 Tailscale 连 Mac；在外面要用 Relay 或 WireGuard。
- **验证：**
  - Swift：
    - `CompanionLinkTests.testRequestsSentAtOnceReachTheMacInOrder`：一个让每隔一帧晚一点到的 socket，同时发 12 个请求，全部回答、连接不断。把 `sendControl` 换回旧写法时，这条失败在“the phone sent a frame the Mac cannot open”；
    - `testAFrameTheMacRejectsSaysSo`；
    - `LiveSessionStoreTests.testAResyncDropsTheLiveRowsOnlyOnceHistoryIsIn`；
    - `CompanionPairingTests` 里的 `-1022` 说法、连接页地址、15 秒退避；
  - Rust：`remote/identity.rs` 的 `pairing_lists_every_reachable_lan_address_selected_first` 按 1124e 时的网卡，只列 `en0` 和 WireGuard，指定 Tailscale 为首选时才列它；`transcript_stats` 的 `claudes_interrupt_note_is_not_a_turn_or_a_message` 加了“标记那一轮已中断”；
  - 真机或模拟器上掉线是否消失，由下次目检覆盖。
