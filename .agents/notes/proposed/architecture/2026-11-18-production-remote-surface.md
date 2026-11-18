# Agent Note: 生产宿主的远程面——接线、Relay 出站循环与如实诊断

Status: proposed

## Problem

2026-11-17 全量审查交接单 B4-3（Critical）指出，远程访问在生产宿主上走不通：

- `aeroric-cli serve` 构造 `HostWiring` 时没有填 `remote`，`remote.status`、`remote.start`、`remote.stop`、`remote.createInvite` 等 12 个方法在生产宿主上不可用；只有测试 harness 装了它们。
- 宿主没有连往 Relay 的出站循环：没有向 `/host` 注册的控制连接，也没有在收到 `ClientConnected` 后拨 `/host-data/<conn_id>`、把字节交给 E2EE 会话处理的代码。手机经 Relay 连不上桌面。
- `probe_relay` 在出站循环不存在时仍会回答 "phones can reach this host through it"，是假话。
- `orca_crypto.rs` 校验 hostId 的长度与 `StaticKeys::host_id()`（16 字节 SHA-256 前缀的 base64url，固定 22 字符）不一致，Relay 模式的握手会被拒。

这四件事跨 `aeroric-cli`、`aeroric-host`、`aeroric-core::remote` 三层，还牵涉"一个宿主只能有一份 `RemoteState`"。按用户 2026-11-17 的决定，先写方案、批了再做。

## Proposal

分三步，每步单独可验收：

1. **先改诊断，不改能力**：
   - `probe_relay` 在出站循环未运行时如实回答"Relay 可达，但这个宿主还没有连上它，手机暂时无法经它连接"；
   - `orca_crypto` 的 hostId 校验改成"无填充 base64url，解码后 16 字节"，补一条 Relay 握手单测。
   - 这一步不接任何新连接，风险最低，可以先做。
2. **接线**：
   - `aeroric-cli serve` 构造唯一一份 `RemoteState`，同一个 `Arc` 交给 `RemoteServiceWiring` 和推送平面；
   - 宿主启动时，按 `remote-config.json` 是否启用远程来决定是否装上 12 个方法；
   - 不复制第二份状态。
3. **出站循环**（在 `aeroric-core::remote` 里，宿主只提供运行时）：
   - 控制连接：注册、心跳、断线后指数退避重连。实时状态写进 `RemoteState::relay_state`，界面和 `probe_relay` 都从这里读；
   - 收到 `ClientConnected { conn_id, peer }` 时：先过本地按 `peer` 的限流，再拨 `/host-data/<conn_id>`，交给已有的 E2EE 会话处理器；
   - 关闭远程或撤销所有设备时，主动断开。

## Alternatives considered

**把出站循环放在 App（Swift）里。** 最强的理由是 App 已经有网络栈和前台生命周期。没选：违反"业务与写只在 core/host"；而且 App 不在前台时，手机就连不上。

**宿主内嵌一个 Relay，手机直连桌面。** 最强的理由是少一跳。没选：用户的 Mac 通常在 NAT 后面，这正是要 Relay 的原因；内嵌也解决不了可达性。

**先只做第 1 步，长期不接 Relay。** 最强的理由是没有新攻击面。没选：这样伴随端的远程功能永远不可用。第 1 步可以先做，作为如实的过渡状态。

## Acceptance criteria

- 第 1 步：Relay 握手单测通过；出站循环没在运行时，`probe_relay` 不说"手机能经它连到"。
- 第 2、3 步：集成测试起本地内存 `remote-relay`、`aeroric-cli serve`，再用一个模拟手机的 E2EE 客户端经 Relay 完成握手，并调用 `remote.status`。
- `cargo test -p aeroric-core -p aeroric-host` 通过。

## Risks

- 出站循环是新的长连接和攻击面：
  - 断线重连要有上限和退避；
  - 来自 Relay 的 `peer` 不可信，只能当限流的键用；
  - E2EE 握手失败不能泄露宿主信息。
- 两份 `RemoteState` 一旦并存，设备列表和撤销会对不上：接线时只能有一个构造点。

## Consequences

- 需要用户决定是否先做第 1 步，以及第 2、3 步什么时候排期。
- 伴随端客户端的方法名对齐（交接单 B4-4 第 3 小项）依赖第 2 步装上的方法表，一并排在后面。
