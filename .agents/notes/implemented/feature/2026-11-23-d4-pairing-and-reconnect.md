# Agent Note: D4 第五步：手机端配对、存凭据、自动重连

Status: implemented

## Problem

D4 第四步有了 `CompanionLink`，但 App 还不会自己连 Mac：
- 没有配对：拿不到设备令牌，也拿不到 Mac 的公钥和地址；
- 令牌没处存，App 重开就忘了；
- App 入口没有建立连接，断了也不会重连；
- 界面上“配对与连接”一页只有两个不起作用的输入框；
- 手机提交审批或新建任务后，没有谁去调 `flush()`。

Mac 一侧的配对协议是现成的（`core/.../remote/control.rs`、`server.rs`、`auth.rs`）：

1. **配对链接：** Mac 生成的配对链接是 `aeroric://pair?code=<base64url(JSON)>`。JSON 里有：
   - `v`（2）、`endpoints`（局域网的 `ws://ip:port`、用户配的公网地址、Relay 的 `<base>/connect/<hostId>`）；
   - `invite`（10 分钟有效）、`hostName`、`hostId`；
   - `publicKey`（Mac 的静态公钥，base64url）、`pairingConfirmationVersion`（1）。
2. **用邀请认证：** 握手之后，第一条加密控制帧是 v2 的 `auth`，带 `invite`、`deviceName`、`pairingConfirmationVersion: 1`。Mac 回临时的 `deviceId`、`deviceToken`、`pairingId`，这时只存在内存里。
3. **确认：** 手机**先把凭据落盘**，再在 30 秒内发 v2 的 `pairing.confirm {pairingId, pairingConfirmationVersion: 1}`。Mac 这时才把设备写盘，回 `{"confirmed": true}`。没等到确认，Mac 就撤掉这次配对。
4. **旧版：** 不带确认版本的旧式配对直接回 `deviceToken`，没有 `pairingId`。

## Decision

**配对链接：** `CompanionPairingOffer.parse` 认整段链接，也认光秃秃的 code。
- 校验：`v` 为 2、公钥是 32 字节、地址至少一个且是 `ws://` 或 `wss://`、邀请码不为空；
- 每个地址的连接方式：路径是 `/connect/<hostId>` 的算 Relay，其余算直连。

**凭据：** `CompanionCredentials` 包括 Mac 名称、`hostId`、公钥、地址列表、`deviceId`、`deviceToken`。存取走 `CompanionCredentialStore` 协议，凭据分两份：
- “待确认”：`stagePending` / `commitPending` / `discardPending`；
- “已确认”：`load` / `clear`。

App 用 `KeychainCredentialStore`：通用密码项，只在本机解锁后可读，不随 iCloud 同步。测试用 `InMemoryCredentialStore`。

**配对：** `CompanionLink.pair(socket:offer:deviceName:credentials:mode:)`：
1. 用链接里的公钥握手，发带邀请的 `auth`；
2. 回答里有 `pairingId`：先 `stagePending`，再发 `pairing.confirm`。回 `confirmed` 后 `commitPending`，确认被拒就 `discardPending`；
3. 没有 `pairingId`（旧式）：直接存成已确认；
4. 之后和 `connect` 走同一段：读 `hello`、`events.since`、快照，交给 store。

发出确认后连接断了，就不知道 Mac 有没有写盘，这时凭据**留在待确认**。下次启动先拿它去连：认证过了就转正；Mac 说不认识这个令牌，就丢掉。

**连接器：** `CompanionConnector` 是 App 持有的一个对象。
- **`start()`**：App 进前台时调。
  - 读出已确认的凭据（没有就试待确认的），让 store 记下配对的 Mac，状态写“正在连接 Mac”；
  - 按顺序试每个地址，局域网在前、Relay 在后，哪个连上就用哪个；
  - 都连不上就退避重试：1、2、4……秒，最多 30 秒一次；
  - 连上以后等断开，断了马上重来。
- **`stop()`**：进后台时调，断开并停止重试。
- **令牌被拒：** Mac 说不认识这个令牌时，清掉凭据、停止重试，store 显示“需要重新配对”。
- **`pair(code:deviceName:)`**：配对页调。先停掉现有连接，配对成功后接着按上面的方式保持连接。
- **`forget()`**：清掉凭据，store 回到未配对。

**命令自动发出：** `CompanionStore` 加一个 `onCommandIssued` 钩子，在 `issue()` 里调。连接器把它接到 `link.flush()`，所以界面上的提交审批、新建任务不用改。

**推送登记：** `CompanionLink.call(_:_:)` 能在已认证的通道上发一个 v3 请求。连接器用它实现 `push.register`，替掉 App 里原来那个一律报“没接”的闭包。

**界面：** “配对与连接”一页改成：
- 已配对：显示 Mac 名称、当前连接状态、“取消配对”；
- 未配对：粘贴配对链接、设备名称、“配对”按钮，失败时写明原因。

扫码以后再做。

## Alternatives considered

**确认前不落盘，确认成功后再存。** 没选：Mac 的协议明确要求先落盘再确认（`server.rs` 里的注释）。确认已经到了 Mac、回答却丢了的时候，Mac 已经认了这台设备，手机却没有令牌，只能重新配对。

**令牌存 `UserDefaults`。** 没选：设备令牌等同于这台手机能操控 Mac 的凭证，要放钥匙串。

**连接器直接观察 `store.outbox` 的变化去发。** 没选：`@Observable` 的观察在非界面代码里要自己反复登记。在 `issue()` 里调一个钩子更直接，也好测。

**加 URL scheme，点开配对链接就跳进 App。** 没选：要改 `project.yml`、重新生成 Xcode 工程，还要真机验证。先做粘贴，扫码和链接跳转放到下一步。

## Consequences

- **收益：** 手机端第一次能完整走完配对、存凭据、连上、断线重连。审批和新建任务提交后会自动发给 Mac，推送登记也走真实通道。
- **局限：**
  - 没有扫码，也不能点链接跳进 App，只能粘贴；
  - 钥匙串那一层没有自动测试，测试只用内存版；
  - 快照仍只有任务和挂在任务上的审批；
  - 按 `generation` 和修订号判旧仍没做；
  - 真机、真实 Relay、真实 Mac 上的配对和重连都是 NOT RUN；
  - 配对页没有目检。
- **验证：**
  - `AeroricCompanionKit` 新增 12 个测试（`CompanionPairingTests`），共用的假 Mac 挪到 `FakeMac.swift`，并支持邀请配对和三种确认结局。覆盖：
    - 配对链接的读取和各种坏链接；
    - 先落盘再确认、确认被拒丢掉、没等到确认留着、旧式直接存；
    - 连接器先试局域网再试 Relay、断了重连、命令自动发出、退避 1/2/4/8/16/30/30 秒、令牌被拒清凭据、待确认的凭据连上转正或被拒丢掉、取消配对。
  - MacBook 上包里 52 个测试都过，新测试连跑 5 次都过。
  - iOS App 目标在模拟器上编过（`xcodebuild … -destination "generic/platform=iOS Simulator" CODE_SIGNING_ALLOWED=NO`）。
