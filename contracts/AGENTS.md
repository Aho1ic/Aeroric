# contracts/ 规则

Rust 与 Swift 共用的线协议契约与金样。

- `native/v1/**` 是生成物：由 `core/crates/aeroric-protocol/src/bin/export-schema.rs` 写出，**不手改**；`pnpm native:contracts --check`（或 `native:check protocol`）逐字节比对。要改就改 Rust 类型或 `docs/native/command-map.json` / `event-topics.json` 后重新生成。
- 方法改名、删方法、删 topic 都是协议变更；别名表与映射表内的改动不是。→ [方法面](../.agents/notes/implemented/architecture/2026-09-22-rpc-method-surface.md)
- `codex/app-server-<版本>/` 是 `codex app-server generate-json-schema` 的快照；升级 Codex 门槛时新建目录而不是覆盖旧版。升级时：
  1. 新建目录；
  2. `pnpm native:codex-diff` 看方法表和驱动在用方法的参数变化；
  3. 跑 Codex 驱动测试。测试里的假 Codex 会按每份快照核对驱动发出的每条消息，新目录自动纳入。→ [E9 笔记](../.agents/notes/implemented/testing/2026-11-23-e9-codex-contract-check.md)
- `remote/rpc-golden.json` 是 relay RPC 金样，与本地协议是独立版本空间。
