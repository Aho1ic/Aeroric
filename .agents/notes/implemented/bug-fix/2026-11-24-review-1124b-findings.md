# Agent Note: 目检 1124b 查出的五处

Status: implemented

## Problem

目检报告在 `~/Documents/computer-use/2026-11-24-1126-aeroric-review-1124b/report.md`，测的是 `238724d3`。

**通过的：**
- 设置的分组、搜索、跳转、高亮；
- 委派“只读（默认）”；
- 思考和命令收成一行；
- 新建会话记住选择，重启后也还在；
- 打开项目后进新建会话；
- 远程访问的开关和状态行。

**没测成的：**
- **手机端：** 配对没成，手机上的任务页、改动、文件与 Git、发消息和停止都没测成；
- **拖侧栏：** 截图在拖完以后才取到，看不出拖动中是否卡顿；
- **中文输入法打字：** Codex 打不了中文，要用户自己试。

**查出的五处：**

1. **配对二维码一直在生成。** 点“生成配对二维码”只是打开弹窗，没有任何地方去向宿主要配对码。审计日志里两次 `invite-created` 都是 CLI 发的。
2. **手机配对报 `CompanionCredentialError error 0`。**
   - **原因：** 目检用 `CODE_SIGNING_ALLOWED=NO` 编的模拟器 App 只有链接器签名，没有 `application-identifier`，钥匙串拒绝存配对信息（`errSecMissingEntitlement`）。
   - **为什么没进确认：** 手机在发 `pairing.confirm` 之前存配对信息，存不进就停了，Mac 等不到确认，没有留下设备，所以设备数一直是 0。
   - **错误码看不懂：** Swift 给枚举的错误编号时，带值的情况排在前面，所以 `keychain(-34018)` 显示成 “error 0”。页面还把它写成“连不上”，并且对每个地址都试一遍。
3. **Claude 会话跑着时 token、上下文是“—”，中断后才有数。**
   - **原因：** 经网关（Magpie）时，网关在调用开始的 `message_start` 里写的用量是 0/0，真实用量只在调用结束的 `message_delta` 里。
   - **漏掉的地方：** Claude Code 在调用结束前就把每个内容块作为 `assistant` 消息发出来，带的都是那个 0。映射只读 `assistant` 消息里的用量，不读流事件里的，所以整轮都没有数，直到 `result` 才有。
4. **切 Harness 往返丢了思考强度。** Codex 下选了“超高”，切到 Claude（这个配置没有思考强度）再切回 Codex，变成配置默认的“低”。切走时没有地方记住目标配置不支持的选择。
5. **“远程与手机”页还是带底色的圆角分组。** 这一页用的是 `Form` 加 `.formStyle(.grouped)`，没有换成设置里统一的平铺卡片和行。

## Decision

**配对页打开时自己去要配对码（`RemoteStore.preparePairing`）。**
- `PairingView` 出现时清掉上次的配对码和错误，再调 `remote.createInvite`；
- 失败时弹窗里写原因，比如没连上宿主时写“未连接宿主”。

**手机说清楚存不下配对信息，并且停下，别的地址也不试。**
- `CompanionCredentialError` 实现 `LocalizedError`：
  - 缺钥匙串权限时写“这台手机存不下配对信息：App 缺少钥匙串权限（-34018）。模拟器里要用带签名的构建”；
  - 其他钥匙串错误写状态码。
- `CompanionConnector.pair` 遇到这个错误就停，换地址也存不下。
- **模拟器构建命令：** 带临时签名，`CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM=`。编出来的 App 的权限段里有 `application-identifier`，已在 MacBook 上核对过。这条命令写进了 `docs/subsystems/ios-companion.md`。只检查能不能编译时，`CODE_SIGNING_ALLOWED=NO` 照样可以用。

**Claude 映射也从流事件读用量（`ClaudeMapper::observe_call`）。**
- `message_start` 里的 `message.usage` 和 `message_delta` 里的 `usage`，都按当前流的消息 id 记进这一轮的调用；
- 和 `assistant` 消息用同一个函数：每项计数取最大值，有变化才发一次实时用量；
- 上下文按合并后的输入加输出算。只带输出数的 `message_delta`（官方 API 的写法）不会把上下文压成只剩输出。

**切 Harness 时记住目标不支持的模型和思考强度。**
- `NewSessionDraft` 加 `heldModel`、`heldEffort`；
- 切过去时，目标配置不提供的选择记下来；再切到提供它的配置时，用它补上，然后清掉。

**“远程与手机”页改成平铺。**
- 服务、Relay、已配对设备三组都用 `SettingsCard` 和 `SettingsRow`；
- 页面本身不滚动，设置窗口和其他页一样把它放进滚动区，`SettingsPane.scrollsItself` 删掉；
- 主窗口里的远程页由主窗口包一层滚动。

## Alternatives considered

**手机用钥匙串以外的地方存配对信息（模拟器上退到文件）。** 没选：设备令牌就该在钥匙串里。模拟器的问题是构建方式，改构建命令就够了。

**网关在 `message_start` 里估一个输入数。** 没选：网关在调用结束前不知道真实用量，估出来的数会和结束时的对不上。读 `message_delta` 是 Anthropic 流格式本来就有的位置，直连官方 API 时也一样。

**切回原 Harness 时恢复上一次在这个 Harness 的整套选择。** 没选：用户在 Claude 下改过模型或权限时，应该带回去。只补目标配置当时不支持的那几项。

## Consequences

- **收益：**
  - 设置里能直接生成配对二维码；
  - 手机端出错时能看懂原因，用带签名的构建可以在模拟器里配对；
  - 经网关的 Claude 会话，每次模型调用结束统计栏就跟着动，不用等这一轮结束；
  - Harness 往返不丢思考强度；
  - 设置各页样式一致。
- **局限：**
  - 一次模型调用之内用量还是不动，要等这次调用结束；
  - `heldModel`、`heldEffort` 不存盘，重启以后切 Harness 就补不回来；
  - 手机端、拖侧栏、中文输入法这几项仍待下一次目检和用户自测。
- **验证：**
  - Rust：`counts_from_the_stream_when_the_messages_carry_none`，原来的 `counts_move_while_a_turn_runs` 照样过；
  - 手机：`testAPhoneThatCannotStoreThePairingSaysWhyAndStops`；
  - Mac：`testOpeningThePairingSheetAsksForAnInviteAndSaysWhyNoneCame`，以及 `testSwitchingHarnessCarriesTheChoicesOver` 里加的 Harness 往返断言。
