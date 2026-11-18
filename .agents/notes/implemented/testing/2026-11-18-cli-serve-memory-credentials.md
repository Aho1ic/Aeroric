# Agent Note: CLI 集成测试拉起的 serve 用内存凭据

Status: implemented

## Problem

2026-11-17 的全量审查（交接单 B0-4）实测：`cargo test --workspace` 里 `aeroric-cli --test agent_config_roots` 每次都报 `serve ready timeout`。

- `aeroric-cli serve` 无条件装上 `KeyringSecrets`（经 `/usr/bin/security` 读写登录钥匙串），再跑 `migrate_legacy_agent_settings`。
- `agent_config_roots` 预先写了 `settings.json`。启动时在绑定 `host.sock` 之前，要串行调用几十次 `security` 查找和迁移凭据字段，稳定超过 5 秒的看门狗。
- 更要紧的是隔离：`agent_config_roots` 和 `cli_headless` 拉起的 serve 都直接碰开发者真实的钥匙串。`startup.rs` 自己的注释就写着改 `HOME` 隔离不了系统凭据服务，违反"测试不碰真实凭据"的规则。

## Decision

**`serve` 认一个测试专用变量 `AERORIC_TEST_MEMORY_SECRETS=1`，设了就用 `MemorySecrets`，但只在数据目录位于临时目录下时才接受。**

- 数据目录（不存在时取它的父目录）规范化后，必须在 `std::env::temp_dir()`、`/tmp` 或 `/private/tmp` 之下。
- 不在时直接报错退出（退出码同连接失败），并说明这个变量只给测试用。不静默忽略，也不照做：在真实数据目录上用内存凭据，第一次写 settings 就会把明文凭据迁进内存，退出即丢。
- 没设这个变量时行为不变，生产仍用钥匙串并做旧设置迁移。迁移在测试里照跑，只是写进内存。
- 测试侧：
  - `agent_config_roots` 的 `cli()` 和 `cli_headless` 的四处 `serve` 都带上这个变量；
  - `agent_config_roots` 的冷启动看门狗从 5 秒放宽到 10 秒，和同文件另一处一致；
  - 新增 `memory_credentials_are_refused_outside_a_temporary_folder`：数据目录放在 `CARGO_TARGET_TMPDIR`（在 `target/` 下，不是临时目录），serve 必须失败、报出变量名，且没有写出 `host.token`。
- `conda_cli`、`database_*_cli` 用进程内 harness 起宿主，本来就是 `MemorySecrets`，CLI 只当 RPC 客户端用，不受影响。

## Alternatives considered

**文件后端 `AERORIC_TEST_SECRETS_FILE=<scratch>`。** 最强的理由是凭据能跨两次 serve 留存，可以测"重启后还读得到"。没选：现有测试没有这种需要，多一个文件格式就多一份要维护的落盘格式；需要时再加。

**只在 debug 构建里认这个变量。** 最强的理由是发布版完全没有这条路径。没选：审阅版 App 本身就是 Debug 构建，挡不住误设；按数据目录位置把关，对所有构建都成立。

**只把看门狗放宽到 30 秒。** 最强的理由是改动最小。没选：慢只是表象，测试碰真实钥匙串才是问题。

## Consequences

- **收益**：
  - CLI 集成测试不再碰真实钥匙串；
  - `agent_config_roots` 4 个用例 2.5 秒跑完，`aeroric-cli` 全部测试通过。
- **代价**：生产入口多了一个测试变量，要靠"只认临时目录、否则报错"的门把住。
- **验证**：2026-11-18 在 Mac 上：
  - `cargo test -p aeroric-cli` 全部通过（含新增的拒绝用例）；
  - `cargo clippy -p aeroric-protocol -p aeroric-core -p aeroric-cli --all-targets -D warnings` 通过。
