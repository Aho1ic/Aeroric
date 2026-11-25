# 远程、relay 与推送

| crate | 职责 |
|---|---|
| `remote-protocol/` | relay 撮合控制面协议。它与本地协议是两个独立版本空间，不是全量业务协议库 |
| `remote-relay/` | 用户自托管的盲转发 relay（注册、限流、拼接两端连接），不解密业务内容 |
| `remote-push/` | 产品方维护、也可由用户自托管的 APNs 发送端（暂无公网部署）：HMAC 认证宿主请求、白名单载荷、HTTP/2 到 APNs |

三者各有独立的 `Cargo.toml`/`Cargo.lock`，不依赖 core。

- 手机与 Mac 之间：配对后经 relay 走端到端加密通道；宿主侧在 `aeroric-host/src/remote_headless.rs`，core 侧纯逻辑在 `aeroric-core/src/remote/`。
- 配对设备是**完全可信的控制者**：能读项目文件、建/取消任务、控制 Agent 与终端。部署与安全要求见 [远程访问安全指南](../remote-public-access.md)。
- 写项目文件（`project.writeFile`）默认关：`remote-config.json` 的 `allow_file_writes`，在设置 › 远程与手机的“允许手机改文件”里开关（`remote.updateConfig` 的 `allowFileWrites`，`remote.status` 回 `allowFileWrites`）。关着时拒绝并记审计 `remote-file-write-refused`，`hello` 的能力表里没有 `files.write`；开关一次记审计 `remote-file-writes`。远程服务在运行时改配置会重启，连着的手机重连后拿到新的能力表。
- 手机的命令（会改东西的方法，见 `remote/commands.rs` 的 `COMMANDS`）可以带 `commandId` 和 `hostEpoch`：同一台设备的同一个 `commandId` 十分钟内只执行一次，重发拿到第一次的结果；`hostEpoch` 不是本次启动的（`hello` 里给出）就以 `STALE_HOST_EPOCH` 开头拒绝；撤销的设备的命令拒绝、结果清掉。不带 `commandId` 的照旧执行。见笔记 `2026-11-21-d4-command-identity`。
- 推送：宿主经 `push_host.rs` 签名请求发送端，发送端只转发 `kind/eventSeq/collapseKey`。见 [推送平面](../../.agents/notes/implemented/architecture/2026-10-13-push-plane.md)。
- relay RPC 金样在 `contracts/remote/rpc-golden.json`。
