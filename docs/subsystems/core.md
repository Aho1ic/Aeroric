# 业务内核 aeroric-core

`core/crates/aeroric-core` 承载全部业务逻辑与对数据目录的写入。它不含 UI 类型，也不直接做平台 FFI 或 HTTP：卷探测、钥匙串、进程监管、网络等待都经端口由宿主实现。

## 模块

| 模块 | 职责 |
|---|---|
| `storage`、`storage_lock`、`location*`、`paths` | 数据目录 `~/.aeroric` 的租约、原子写、内容指纹、`runtime.json`、写者资格 |
| `projects` | 项目记录、身份归一（`ProjectTarget::identity_key()`）、分组名单 `project-groups.json` |
| `tasks` | 任务生命周期：`TaskService`、意图 `TaskIntent`、`OperationLedger`（`task-operations.json`）、崩溃对账、观察归约 |
| `runtime` | 运行注册表（`RunKey`）、`LiveRuns` 端口、杀进程目标 `KillTarget` |
| `agents` | 四族与自定义 Agent 的家族判定、启动参数（`launch.rs`）、托管 home、版本检测与升级 |
| `sessions` | 会话文件解析、发现、分页读取、续跑与导出；`sessions/live/` 是 Claude/Codex/omp 结构化驱动与 SSH 远程会话 |
| `terminal` | 原始字节流、控制租约、有界回放与退出结算 |
| `events` | 事件序号分配与发布的单一原子边界 |
| `agent_connections`、`settings`、`providers` | Agent 连接、模型目录与配置；应用设置；网盘 OAuth（PKCE + 回环） |
| `files`、`git`、`ide` | 文件服务、Git 全流程、IDE（LSP/DAP 进程由宿主管理） |
| `connections`、`infrastructure`、`database`、`notebook`、`canvas` | SSH/SFTP 连接、Docker/端口/Conda、数据库、随手记、无限画布 |
| `extensions`、`adapters`、`operations` | skills/MCP/hooks/DSH 插件、外部 Agent 能力适配、Agent 安装/升级操作状态 |
| `metrics`、`maintenance` | 用量统计与索引；清理调度、周报、启动降级账 |
| `remote`、`local_router` | 手机远程连接的纯逻辑半边；本地路由 |
| `platform`、`storage_backends` | 平台特定实现（cfg 隔离）；18 种存储协议归一的文件操作 |

## 数据目录

| 文件 | 写者 | 说明 |
|---|---|---|
| `projects.json` | core | 项目记录 |
| `projects/<id>/tasks.json` | core | 任务记录；删空即删文件 |
| `project-groups.json` | core | 分组名单与顺序 |
| `task-operations.json` | core | 世代与未决认领的权威账本 |
| `runtime.json` | core | 写者 epoch/revision、内容指纹表、`volumeKind`；可丢弃 |
| `agent-homes/<family>/` | core/host | 托管 Agent home（omp 经 `PI_CODING_AGENT_DIR` 注入） |

写入规则见 [写入基线](../../.agents/notes/implemented/architecture/2026-09-22-content-fingerprint-write-cover.md) 与 [写者资格](../../.agents/notes/implemented/architecture/2026-09-22-data-dir-writer-eligibility.md)。

## 测试

`cargo test --manifest-path core/Cargo.toml --workspace`；集成测试在各 crate 的 `tests/`，共享夹具在 `core/crates/aeroric-core/tests/fixtures/`。测试使用隔离数据根与假 Agent，不碰真实 agent home。
