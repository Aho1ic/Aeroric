# core/ 规则

Rust workspace：`aeroric-protocol`（线协议）、`aeroric-core`（业务）、`aeroric-host`（宿主进程）、`aeroric-cli`（`serve` 入口与 RPC 客户端）；`vendor/` 是打过补丁的第三方 crate。现状见 [docs/subsystems/core.md](../docs/subsystems/core.md)、[host-and-cli.md](../docs/subsystems/host-and-cli.md)。

- 业务判断与对数据目录的写只在 `aeroric-core`；宿主只做适配。core 不引 UI 类型、不做平台 FFI 或 HTTP，需要时定义端口由宿主实现。端口实现不得回调 core。
- 写入走 `load → 改 → commit`；不符基线即 `REVISION_CONFLICT`，绝不静默覆盖、绝不自动恢复。→ [写入基线](../.agents/notes/implemented/architecture/2026-09-22-content-fingerprint-write-cover.md)
- 任务写入是具名 `TaskIntent`，失败无副作用。→ [意图](../.agents/notes/implemented/architecture/2026-09-23-task-writes-are-intents.md)
- 运行身份是 `RunKey = (task_id, generation)`；注册表锁序见 `aeroric-core/src/runtime/registry.rs`，`kill_targets` 是叶子锁。
- `RpcError.detail` 只能经 `ErrorDetail` 构造。→ [白名单](../.agents/notes/implemented/architecture/2026-09-22-error-detail-allowlist.md)
- 改线协议后跑 `export-schema` 重新生成 `contracts/native/v1/**`，再同步 Swift `AeroricProtocol`。
- 改四族分支同步全部穷尽点（protocol、core match、Swift 枚举、设置与统计）。
- 测试用隔离数据根与假 agent；在 VM 内 cargo 会 OOM，到 Mac 上跑。只 `rustfmt --edition 2021 <改过的文件>`，不要全 workspace `cargo fmt` 改别人的文件。
- 依赖：先问"它是否已在依赖图里"；在就不算新依赖。
