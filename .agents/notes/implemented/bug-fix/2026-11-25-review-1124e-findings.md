# Agent Note: 目检 1124e：停止后插话又跑一轮、手机连着虚拟机网桥的地址

Status: implemented

## Problem

目检报告在 `~/Documents/computer-use/2026-11-25-1456-aeroric-review-1124e/report.md`，测的是 `8a87b737`。桌面设置 A1–A5 全部通过；手机端两项没过。

**B4：手机插话后点“停止这一轮”，Mac 接着跑了一分多钟。**
- Claude 的会话记录（`agent-homes/…/c7c6ae71-….jsonl`）里的顺序：
  - 07:31:53，插话 `Also count the files at the end.` 进了 Claude 的输入队列（`queue-operation: enqueue`），没有并进正在跑的那一轮；
  - 07:32:19.094，`[Request interrupted by user]`：停止送到了，那一轮停了；
  - 136 毫秒后，Claude 自己把队列里的插话取出来（`dequeue`），开了新的一轮，调 `rg`，跑到 07:33:38。
- 驱动只要两轮之间来了 `system/init` 就开一轮（[1124c](2026-11-24-review-1124c-findings.md)）。插话写进去后还没等到 `result`，所以这一轮被当成“我们的消息开的”，照常放行。
- Mac 上的停止按钮走同一个 `interrupt`，问题一样。
- 手机在宿主一收下停止就写“Mac 已停止这一轮”，那时这一轮还在跑。

**B8：手机离线后再也连不回来。**
- 手机连的是 `ws://192.168.64.1:6790`。这是 `bridge100` 的地址：虚拟机（Docker、Virtualization.framework）开的网桥，随虚拟机起停出现或消失。目检后再看，`bridge100` 已经没了，这个地址被路由到透明代理的 `utun1024`，所以手机报的是超时（`-1001`），不是拒绝连接。
- 地址排序先比网段、再比网卡名。`bridge100` 和 `en0` 都是 192.168 段，名字 `bridge100` 在前；配对链接又只放排第一的那一个局域网地址。
- 手机只试配对时存下的地址。宿主的 `hello` 每次都回 `endpoints`，`rpc.rs` 的注释也说手机会用它刷新地址，但手机从来没读过。所以关开远程服务、切后台都救不回来。
- 离线提示原样显示 `Error Domain=NSURLErrorDomain Code=-1001 …` 的字典，占好几行。

**没测成的：**
- B5、C：清单写的模型 `codex/gpt-6.2-sol` 不在菜单里（只有 `gpt-6.2-sol`），Codex 登录也已失效；
- B3：辅助功能读取多次失败；
- B6、B7：被 B8 的断线挡住；
- B1 缩矮窗口、B2 的 30 条上限没核对到。

## Decision

**B4：停止也管住停止前排在 Claude 队列里的消息**（`claude/driver.rs`）。
- **记下排着的条数：** 用户停止时，记下 `stopped_queue`，即还没等到 `result` 的消息里、不属于正在跑的这一轮的条数：
  - 正在跑的是我们的一轮，它占一条；
  - 正在跑的是 Claude 自己开的，不占。
- **拦下自开的轮：** 之后两轮之间来的 `system/init` 如果是“我们的消息开的”，并且 `stopped_queue` 大于 0，这一轮照常开，然后：
  - 立刻发 `interrupt`，这一轮记为“已中断”，计数减一；
  - 会话里写一行警告：停止前发的补充说明已一并停止、没有执行，需要的话重新发送。
- **什么时候清零：**
  - 用户在停止后发新消息时清零。Claude 可能把新消息和排着的一起跑，拦下就会丢掉新消息；宁可让旧插话跑，也不吞新请求；
  - Claude 报空闲时清零，说明队列已经空了。

**手机：停止的回执改说实话**（`CompanionStore`）。宿主回答时停止只是送到了，提示改为“Mac 收到了停止；状态变成“已中断”才算停下”。

**B8：局域网地址不含虚拟网桥，配对链接和 `hello` 都列出全部可达地址**（`remote/identity.rs`、`remote/rpc.rs`）。
- **排除虚拟网桥：** `lan_addresses` 不收 `bridge*`、`vmnet*`、`vboxnet*`、`docker*`、`veth*`、`virbr*`、`br-*` 网卡上的地址。
- **`lan_endpoints` 统一列地址：**
  - 选中的（首选或排第一的）在前；
  - 其余手机可能连到的跟在后面（[1126a](2026-11-26-review-1126a-findings.md) 起只跟私有网段）；
  - 透明代理的 198.18/15 和链路本地地址只在被选中时才列出。
- **两处共用：** `pairing_endpoints` 和 `hello` 的 `endpoints` / `lanEndpoints` 都用它，两边列表一致。

**手机：每次连上就换成 Mac 现在给的地址**（`CompanionLink.onHostEndpoints`、`CompanionConnector.hostAnnounced`）。
- **什么时候换：** `hello` 的 `hostId` 和配对的 Mac 一致，并且至少有一个 `ws://` / `wss://` 地址时，用这份列表替换存下的地址：
  - 钥匙串里经 `CompanionCredentialStore.replace` 写入，令牌不变；
  - 别的 Mac 或没有可用地址时不动。
- **重连用最新地址：** 重连循环每一轮都读最新的地址，不再沿用启动时那份。
- **配对中不换：** 配对过程中的 `hello` 不更新地址。重新配对同一台 Mac 时，`credentials` 在配对完成前还是旧的，写回去会把新令牌换成旧令牌。

**手机：网络错误说成中文**（`CompanionLink.networkReason`）。
- 超时写“连接超时，Mac 的地址可能变了”；
- 连不上主机、网络断了、没联网、找不到地址各一句；
- 其他错误写错误码；
- 离线顶栏和连接页的失败原因都用它。

## Alternatives considered

**B4：不往 Claude 写插话，等这一轮结束再发。** 最干净：停止时队列在我们手里，直接作废。没选：Claude 会在一轮中间读排队的消息并据此调整（1124c 见过插话被并进当前一轮），等到轮末再发，插话就失去了意义。

**B4：停止时让 Claude 清空它的输入队列。** 没选：Claude 的控制协议里没有清空队列的请求，只有 `interrupt`。

**B4：拦下停止后 Claude 自己开的所有轮，不看计数。** 没选：后台任务回报时 Claude 也会自己开一轮（`own_turn`），那和用户的停止无关，不该拦。

**B8：保留网桥地址，只把它排到最后。** 没选：手机挨个试地址，一个连不上的地址要等满 URLSession 的超时（默认 60 秒）才轮到下一个。网桥对真机永远不可达，对模拟器也有 `en0` 可用。

**B8：手机连不上时向 Relay 或 Bonjour 重新发现 Mac。** 没选：没配 Relay 的用户用不上；Bonjour 要新加权限和发现流程，比让 `hello` 刷新地址大得多。这次的问题用已有的 `hello` 就能补上。

## Consequences

- **收益：**
  - 停止后不再冒出一轮没人要的执行；没执行的插话在会话里说清楚，可以重发；
  - 手机配对拿到的是 `en0` 等真正的局域网地址，并且有备选；
  - Mac 换了网络或某个地址消失后，手机下次连上就记住新地址；
  - 离线提示是一句中文。
- **局限：**
  - 被拦下的那一轮会先开起来，再被打断：Claude 已经发出一次模型请求，会产生少量用量；会话里会多一个“已中断”的轮；
  - 如果 Claude 取出排队消息的 `system/init` 早于被停那一轮的 `result`，驱动认不出这一轮，照旧会放行。B4 的真实记录里 `result` 在前；
  - 停止后马上发新消息时，排着的插话可能和新消息一起跑；
  - 手机只在连上时刷新地址：存下的地址全部失效后，只能重新配对（或配 Relay）；
  - 手机上的停止提示仍不跟着任务状态变，要看任务状态才知道是否已停下。
- **验证：**
  - Rust：
    - `claude/driver_tests.rs` 的 `a_message_queued_before_a_stop_does_not_run_after_it`（按 B4 的真实顺序）、`a_message_after_the_stop_runs`；
    - `remote/identity.rs` 的 `virtual_machine_bridges_are_not_lan_addresses`、`pairing_lists_every_reachable_lan_address_selected_first`，用的是 1124e 时这台 Mac 的网卡；
  - Swift：`CompanionPairingTests` 的四条：
    - `hello` 的地址存下、重连不回网桥；
    - 别的 Mac 或空列表不动；
    - 重新配对保留新令牌；
    - 网络错误的说法；
  - 真机和模拟器上的断线恢复，由下次目检覆盖（B8 的完整链路：在线 → 地址消失 → 恢复）。
