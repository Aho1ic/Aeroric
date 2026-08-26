# Orca kernel compatibility

更新时间：2026-08-24

已对照 `stablyai/orca` 的 `upstream/main` 提交
`afd76a4df939e336180bd53611c5be8fa53d930b`（上次对照为
`a7ed5a45c2d7a9c6af777c9e1a3928d877e845ea`）。Aeroric 本身是 Tauri/Rust
应用，仓库中没有可升级的 Orca 包或共享 Git 历史；因此这里保留 Aeroric
内核，并实现 Orca 最新移动 E2EE v2 的双栈兼容。

本次对照结论：**线格式契约未变。** Orca 侧
`mobile-e2ee-v2-key-schedule.ts`、`mobile-e2ee-v2-physical-channel.ts`、
`mobile-e2ee-v2-client-session.ts`、`rpc/e2ee-crypto.ts`、`e2ee-keypair.ts`
自上次对照以来全部零变更，下面宣称的兼容面依然成立。区间内 E2EE 相关的两处
改动分别是遥测收敛（`e2ee-channel.ts`，关闭行为与错误码不变）与
`onopen` 内 `send` 的失败兜底，均不影响协议。

## 已兼容

- Orca `e2ee_hello`/`e2ee_ready` v2 精确字段、上下文绑定、HKDF transcript。
- XSalsa20-Poly1305 secretbox 的文本/二进制帧、方向与单调计数器。
- Orca `e2ee_auth`/`e2ee_authenticated` v2 设备令牌认证。
- Orca RPC 的字符串 ID、`_meta.runtimeId` 响应信封，以及 `status.get`、`hello`、`ping`。
- Aeroric 旧远程协议继续可用，默认不切换协议；移动主机记录可显式设置 `protocol: "orca"`。

## 明确未宣称兼容

Orca 的完整 runtime method registry、流式 RPC、Orca Relay 身份/控制面和
Electron daemon 不属于 Aeroric 的现有内核。未实现的 Orca 方法返回标准
`method_not_found`，不会被静默映射成可能误操作任务或 worktree 的 Aeroric
操作。要完整替换为 Orca 内核，需要另行决定是否迁移到 Electron/runtime
架构。
