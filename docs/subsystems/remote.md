# 远程、relay 与推送

| crate | 职责 |
|---|---|
| `remote-protocol/` | relay 撮合控制面协议。它与本地协议是两个独立版本空间，不是全量业务协议库 |
| `remote-relay/` | 用户自托管的盲转发 relay（注册、限流、拼接两端连接），不解密业务内容 |
| `remote-push/` | 产品方维护的 APNs 发送端：HMAC 认证宿主请求、白名单载荷、HTTP/2 到 APNs |

三者各有独立的 `Cargo.toml`/`Cargo.lock`，不依赖 core。

- 手机与 Mac 之间：配对后经 relay 走端到端加密通道；宿主侧在 `aeroric-host/src/remote_headless.rs`，core 侧纯逻辑在 `aeroric-core/src/remote/`。
- 配对设备是**完全可信的控制者**：能读写项目文件、建/取消任务、控制 Agent 与终端。部署与安全要求见 [远程访问安全指南](../remote-public-access.md)。
- 推送：宿主经 `push_host.rs` 签名请求发送端，发送端只转发 `kind/eventSeq/collapseKey`。见 [推送平面](../../.agents/notes/implemented/architecture/2026-10-13-push-plane.md)。
- relay RPC 金样在 `contracts/remote/rpc-golden.json`。
