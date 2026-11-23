# Agent Note: D4 第四步：手机端的连接循环

Status: implemented

## Problem

D4 前三步把手机端命令的规则写进了 `CompanionStore`：
- 命令带 `commandId` 和 `hostEpoch`，发出后留在 `outbox`；
- 重连到同一次启动时原样重发；
- 事件序号跳号、缓冲溢出或被拒为 `STALE_HOST_EPOCH` 时，要求重新同步；
- 审批回执按 Mac 的结论改卡片。

但手机端没有任何东西真的连到 Mac。`CompanionStore` 只是内存里的状态，`E2eeTransport` 只有握手和加解密，`RemoteRpcCodec` 只有编解码：
- 没有谁去发握手、认证；
- 没有谁把 `outbox` 发出去；
- 没有谁把 Mac 的回答喂给 `applyCommandReply` / `applyApprovalReply`，把推送喂给 `ingestHostEvent`。

前三步的笔记都把这件事记作“真实连接上的收发还没接”。

Mac 一侧的协议是现成的（`core/.../remote/server.rs`、`protocol.rs`、`rpc.rs`），手机端照着它说话：

1. **握手：** 第一条明文 text 帧是 `{"v":2,"type":"hello","pub":…}`，Mac 回 `hello_ack`。之后所有帧都是加密的二进制帧 `[kind][seq][密文]`，控制帧 kind 为 1，终端帧 kind 为 2。
2. **认证：** 第一条加密控制帧必须是 v2 的 `auth`：`{"v":2,"id":1,"method":"auth","params":{"deviceToken":…,"supportedRpcVersions":[3]}}`。成功后回 `deviceId`、`rpcVersion`；设备不认识时回失败 `Unknown device token`。
3. **之后的请求：** 用 v3 信封 `{"v":3,"type":"request","id":"…","method":…}`，回答 `{"type":"response","id":…,"ok":…}`。推送是 `{"type":"push","event":…,"seq":…,"data":…}`。
4. **几个要用的方法：**
   - `hello` 回 `hostEpoch`；
   - `events.since {after}` 回 `latestSeq`，缓冲已经溢出时带 `reset`；
   - `projects.list`；
   - `tasks.list {projectId}` 回任务记录，有待审批的任务带 `approval {requestId, kind, toolName}`。
5. **关闭：** Mac 正常退出远程时，关闭原因是 `signed-out`。

## Decision

**新增 `CompanionLink`（`AeroricMobileTransport/CompanionLink.swift`）**：
- 一条连接从握手到断开都由它管，状态只写进 `CompanionStore`，不另存一份；
- 网络藏在 `CompanionSocket` 协议后面，只有发文本、发二进制、收一帧、关闭四个操作；
- 真机用 `URLSessionCompanionSocket`（`URLSessionWebSocketTask`），测试用假连接。

**`connect(socket:deviceToken:mode:)` 的顺序：**

1. **握手：** 用配对时存下的 Mac 静态公钥（`pairedStaticPubB64`）发 `hello`，收 `hello_ack`。公钥对不上就断开，不认证。
2. **认证：** 发 v2 的 `auth`，带上设备令牌，声明支持 v3。失败原因是 `Unknown device token` 时，按设备被撤销处理（`handleDeviceRevoked`）；其他失败只算离线。
3. **读当前状态，下面各步都用 v3：**
   - `hello` 取 `hostEpoch`；
   - `events.since {after: lastEventSeq}` 只取 `latestSeq`。回了 `reset` 也不用补事件，下面的快照本来就是全量；
   - `projects.list`，再对每个项目调 `tasks.list`，组成快照：任务取 `id`、`projectId`、`name`、`agent`、`status`、`permissionMode`；有 `approval` 的任务出一张审批卡，摘要取 `toolName`，没有就取 `kind`，归到这次的 `hostEpoch`。
4. **交给 store：** `handleReconnect` 用这份快照。Mac 换了启动，store 会把 `outbox` 移到 `droppedCommands`。
5. **重发：** 发 `commandsToResend()`，编号不变，Mac 只执行一次。

这一段里到的推送先攒着，`handleReconnect` 之后再按序号交给 `ingestHostEvent`。否则 `handleReconnect` 会把水位线拨回 `latestSeq`，后面的推送就会被当成跳号。

**连上以后：**

- **发命令：** `flush()` 把 `outbox` 里这条连接还没发过的命令发出去。App 在 `submitApproval`、`createTask` 之后调它，store 本身不碰网络。
- **收回答：** 按请求 id 对回去。
  - 回答的是 `task.respond`：成功时交 `applyApprovalReply`；失败时交 `applyCommandReply(failure:)`，卡片记为“没送出去”。
  - 回答的是别的命令：交 `applyCommandReply`。
  - 回答的是 `connect` 里的读取：交给等它的那一步。
- **收推送：** 带序号的交 `ingestHostEvent`。
- **收终端帧：** 交 `ingestTerminalFrame`。

**断开：**

- **store 的状态：** 改成离线。关闭原因是 `signed-out` 时写“电脑端已退出远程连接”。
- **在等读取的步骤：** 一律失败。
- **在等回答的审批：** 卡片记 `timedOut`（“没等到 Mac 的回复，不知道送到没有”），不自动重发。
- **命令本身：** 留在 `outbox`，回到同一次启动再发时，Mac 会给第一次的结果。

## Alternatives considered

**让 `CompanionStore` 自己持有连接，`submitApproval` 里直接发。** 没选：store 现在是纯状态，前三步的测试全靠这一点，不用网络就能跑。连接放在外面，store 的规则和网络的时序就能分开测。

**用推送通知触发重连和同步。** 没选：推送只是提示（AP6），不能成为状态来源。连接在线时，状态只来自 E2EE 通道里的快照和推送。

**不做快照，只靠 `events.since` 补事件。** 没选：缓冲会溢出，事件只是增量，`handleReconnect` 本来就要一份权威快照。`events.since` 只用来拿水位线和溢出标记。

**这一步就做邀请配对（`invite` 加 `pairingConfirmationVersion`）。** 没选：配对要把临时令牌先落盘再确认，和钥匙串存储一起做更合适。这一步只用已配对设备的令牌连上。

## Consequences

- **收益：**
  - 手机端第一次有了能真正连上 Mac 的代码路径，前三步的命令编号、重发、判旧、审批回执都接上了真实的收发；
  - 协议细节集中在一个类里，和 Mac 端的 `server.rs` 一一对应。
- **局限：**
  - 邀请配对、令牌存进钥匙串、App 入口里建立连接和自动重连都还没做，App 还不会自己连；
  - 快照只取任务和审批，会话消息、文件、Git 照旧不同步；
  - 结构化会话的待审批不在 `tasks.list` 的 `approval` 里，这一步的快照拿不到；
  - 按 `generation` 和修订号判旧仍没做；
  - 没有请求超时，只在断开时把等待的读取判为失败；
  - 真机、真实 Relay、真实 Mac 上的往返都是 NOT RUN。
- **验证：** `AeroricCompanionKit` 新增 8 个假连接测试，在 MacBook 上通过，连跑 5 次都过，包里一共 40 个测试都过。测试里的“Mac”用 `ServerHandshakeResponder` 和 store 自己的加解密，按 `server.rs` 的帧格式回话，覆盖：
  - 握手、认证、取 `hostEpoch`、快照、重发 `outbox`；
  - 连接中的推送；
  - 同一次启动重连只重发、换了启动就丢掉；
  - 令牌不认识时撤销设备；
  - 审批在断开前没等到回答时卡片记为不确定。
