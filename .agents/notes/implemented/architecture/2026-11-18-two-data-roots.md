# Agent Note: 两个数据根——omp 托管 home 跟宿主走，全量统一另立项

Status: implemented

## Problem

2026-11-17 全量审查交接单 B5-2 说 `omp_managed_home()` 固定写 `~/.aeroric`，越过了 `--data-dir`。2026-11-18 在 Mac 上核实，实际情况比交接单说的大：**宿主进程里同时在用两个数据根。**

- **宿主数据根**（`--data-dir`，审阅版是 `~/Library/Application Support/com.aeroric.native.dev`）：
  - 经 `CoreContext::paths` 读写，放着会话、讨论、委派、记忆、`agent-connections.json`，以及这个数据根自己的 `settings.json`；
  - `agent-homes/` 下是按配置生成的 `config-*` 和 `dsh`。
- **旧数据根 `~/.aeroric`**（Tauri 时期留下的）：
  - 经 `settings::aeroric_dir()` 读写。宿主启动时从不调用 `settings::configure_data_dir`，所以它总是落到 `AERORIC_HOME`，或者 `~/.aeroric`；
  - 走这条路的有：`settings::load_save` 的 `settings.json`、`agent-credentials/`、`agents/`、自定义 Agent home（本机 100 多个）、`remote-config.json`、`remote-devices.json`、`remote-keypair.json`，以及本地路由的兜底目录；
  - `omp_managed_home()` 更直接，固定拼 `HOME/.aeroric/agent-homes/omp`。

对 omp 来说，结果是：

| 谁 | 用哪个 omp home |
|---|---|
| 宿主启动 omp 原生会话（`omp_agent_env_for`） | `<data-dir>/agent-homes/omp` |
| 设置面板的配置路径（`config_bundles` → `default_builtin_agent_config_path("omp")`） | `~/.aeroric/agent-homes/omp` |
| 模型探测（`list_builtin_omp_models`） | `~/.aeroric/agent-homes/omp` |
| 升级时的环境（`upgrade.rs`） | `~/.aeroric/agent-homes/omp` |

在设置里改的 omp 配置、探测到的模型，都不是会话实际用的那一份。本机的 `~/.aeroric/agent-homes/omp` 有 `agent.db`、`models.db`、3 个旧的终端会话，最后写入是 11 月 12 日；宿主数据根下还没有 `agent-homes/omp`（第一次启动内建 omp 会话时才会建）。

宿主测试 `legacy_global_helpers_keep_their_original_roots_and_missing_home_errors` 明确钉住"旧的全局辅助函数从 `HOME/.aeroric` 解析"。2026-11-18 试改 `omp_managed_home()` 时被它拦下，已撤回。

## Decision

**方案 B（用户 2026-11-18 选定）：只让 omp 托管 home 跟宿主走，旧辅助函数的默认行为不变；两个数据根的全量统一另立项。**

- `settings::configure_omp_home` 设一个进程级覆盖（与 `configure_data_dir`、`configure_secret_store` 同一种注入方式）。宿主只在生产启动（`run_startup_with_secrets_and_legacy_agent_migration`，在迁移之前）把它设为 `<data-dir>/agent-homes/omp`；`omp_managed_home()` 有覆盖就用覆盖，否则仍是 `HOME/.aeroric/agent-homes/omp`；
- 测试 harness 走另一条启动路径，不设这个全局覆盖。一个测试进程里有多个临时数据根，设了会互相串；
- 宿主进程里设置面板、模型探测、升级三处于是都和会话用同一个 home。没有经宿主启动的调用（那条钉住的测试、CLI 的离线命令）行为不变；
- 不搬旧目录：`~/.aeroric/agent-homes/omp` 里是旧终端模式的会话和 omp 自己的缓存（`models.db`），原生会话从来没用过它。新 home 第一次启动时由 omp 自己生成；`config.yml` 由 Aeroric 按版本标记写入。旧目录原样留着，不删。

## Alternatives considered

**方案 A：宿主启动时调用 `configure_data_dir(<data-dir>)`，一次性把 `~/.aeroric` 里仍在用的旧数据迁过去。** 最强的理由是彻底只剩一个数据根，以后不会再出现"设置改了不生效"。没选（先不做）：

- 影响面是所有经 `aeroric_dir()` 的数据：凭据文件、100 多个自定义 Agent home、远程密钥与已配对设备、`ssh-passwords.json`、旧的 `settings.json`；
- 要先逐项盘点"谁还在读、迁过去后旧路径谁还会写"，并设计可回退的迁移；
- 迁错一项，就可能丢凭据或让已配对的手机失效。

这值得单独立项，先出盘点清单再动手。

**方案 C：不改，只在文档里写明两个根。** 最强的理由是零风险。没选：设置面板改 omp 配置不生效，是用户能直接碰到的错误。

**方案 B 的变体：直接把 `omp_managed_home()` 改成 `aeroric_dir()`（2026-11-18 试过）。** 最强的理由是一行改动。没选：生产环境里 `aeroric_dir()` 仍是 `~/.aeroric`，没解决问题，还破坏了那条钉住的契约测试。

## Consequences

### 验收

- 宿主进程内，设置面板（内建配置路径）、模型探测和升级用的 `PI_CODING_AGENT_DIR` 都来自 `omp_managed_home()`，注入后都等于 `<data-dir>/agent-homes/omp`。
- 没有注入时，`legacy_global_helpers_keep_their_original_roots_and_missing_home_errors` 原样通过。
- `~/.aeroric/agent-homes/omp` 不被读写、不被删除。测试只用临时 HOME 和临时数据根。
- `cargo test --workspace` 与 `pnpm native:check storage`（含"不碰真实家目录"扫描）通过。

### 代价与风险

- 换到新 home 后，omp 的模型缓存和 `agent.db` 从空开始，第一次探测模型会慢一点（冷启动最多 20 秒的预算之内）。
- 用户在旧 home 里手写过的 omp 配置（`config.yml` 以外的文件）不会自动带过来。本机旧 home 里没有这类文件；其他机器若有，要在发布说明里提一句。
- 进程级覆盖要在任何 omp 相关调用之前设好，所以放在宿主启动的最前面，和 `configure_secret_store` 同一处。

### 后续

- 方案 A（两个数据根全量统一）仍是待办：先出"谁还在读写 `~/.aeroric`"的盘点清单，再设计可回退的迁移。
- `docs/subsystems/agents.md` 已写明两个数据根的现状。
- 验证：
  - 新增 `aeroric-core/tests/omp_home_override.rs`：在单独的测试进程里，用临时 HOME 先断言旧根，注入后断言 `managed_omp_home()` 和设置面板的内建 omp 配置路径都换成注入的目录，且不在磁盘上建任何东西；
  - 宿主测试 `legacy_global_helpers_keep_their_original_roots_and_missing_home_errors` 不注入，照旧通过。
