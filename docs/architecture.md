# Aeroric 架构全景

Aeroric 是面向 AI coding agent 的原生任务管理器：在 Mac 上管理项目与任务，驱动 Claude Code、Codex、omp、DeepSeek Harness（DSH）与自定义 Agent，并提供 iOS/iPadOS 伴随端做远程查看与控制。所有执行都留在 Mac 上。

## 进程与组件

```
┌──────────────────────┐   UDS (本地协议 v1)   ┌──────────────────────────┐
│ macOS App (SwiftUI)  │ ────────────────────▶ │ 宿主 aeroric-cli serve    │
│ AeroricApp+AeroricKit│ ◀──── 事件 topic ──── │ aeroric-host + -core      │
└──────────────────────┘                       │  ├─ 数据目录 ~/.aeroric   │
                                               │  ├─ Agent 驱动 (结构化)   │
┌──────────────────────┐   E2EE via relay      │  └─ SSH 远程会话          │
│ iOS 伴随端 (SwiftUI)  │ ◀──────────────────▶ └──────────────────────────┘
└──────────────────────┘          ▲                       │ HMAC
           ▲ APNs 提示             │                       ▼
           └──────────── remote-push (APNs 发送端) ◀───────┘
                          remote-relay (用户自托管盲转发)
```

| 组件 | 路径 | 职责 |
|---|---|---|
| 协议 | `core/crates/aeroric-protocol` | 线协议类型、能力、错误；`bin/export-schema` 生成 `contracts/native/v1/**` |
| 业务内核 | `core/crates/aeroric-core` | 存储、项目、任务、Agent 驱动、会话、终端、git、文件、数据库、笔记等全部业务逻辑 |
| 宿主 | `core/crates/aeroric-host` | 进程监管、RPC 分发、平台适配（卷探测、钥匙串、SSH）、把 core 服务暴露为方法 |
| CLI | `core/crates/aeroric-cli` | `serve` 启动宿主；也是 RPC 客户端与 SSH askpass 入口 |
| macOS App | `macos/AeroricApp`、`macos/AeroricKit` | `AeroricProtocol`（Swift 线类型）、`AeroricClient`（UDS 客户端与各服务客户端）、`AeroricUI`（界面） |
| iOS 伴随端 | `ios/AeroricCompanionApp`、`ios/AeroricCompanionKit` | `AeroricMobileTransport`（E2EE 传输、`CompanionStore`、推送桥）、`AeroricMobileUI` |
| 契约 | `contracts/` | `native/v1` 本地协议 schema 与 golden fixtures、`codex/` app-server Schema 快照、`remote/` relay RPC 金样 |
| 远程 | `remote-protocol/`、`remote-relay/`、`remote-push/` | relay 撮合控制面协议、自托管 relay、APNs 发送端 |
| 工具 | `scripts/native/` | 构建、签名、门禁 runner、契约检查（Node 内置模块，无 npm 依赖） |

`core` 依赖仓库外的 `../dbx`（版本钉在 `scripts/dbx-ref.txt`，补丁 `patches/dbx-security.patch` 由 `scripts/prepare-dbx.sh` 打）；`core/vendor/` 是打过补丁的第三方 crate（`portable-pty`、`ssfmt`、`notify-types`）。

## 核心不变量

- 业务逻辑与写操作只在 core/host；Swift 端只读 read model、发 typed 动作，不拼 Agent argv、不解析会话 JSONL、不直写共享数据。→ [原生边界](../.agents/notes/implemented/architecture/2026-09-21-native-boundary-and-single-writer.md)
- 一个数据目录同时只有一个写者，由数据目录租约保证；拿不到租约的实例只读。→ [写者资格](../.agents/notes/implemented/architecture/2026-09-22-data-dir-writer-eligibility.md)
- 每次写只覆盖 core 上一次发布的字节；不符即 `REVISION_CONFLICT`，绝不静默覆盖。→ [写入基线](../.agents/notes/implemented/architecture/2026-09-22-content-fingerprint-write-cover.md)
- Agent 经结构化协议驱动；会话结构只来自协议消息，未知内容带来源保留。→ [结构化协议](../.agents/notes/implemented/architecture/2026-10-25-structured-agent-protocols.md)
- 任务事件只由 core 发射；运行身份是 `(taskId, generation)`。→ [事件权威](../.agents/notes/implemented/architecture/2026-09-24-core-is-task-event-authority.md)、[运行世代](../.agents/notes/implemented/architecture/2026-09-22-task-run-generations.md)
- 本地协议主版本严格相等，兼容性由宿主声明的具名能力承载。→ [版本边界](../.agents/notes/implemented/architecture/2026-09-22-protocol-version-boundary.md)

## 术语

| 术语 | 含义 |
|---|---|
| 服务面 | RPC 方法名的前缀（`git`、`terminal`、`dsh`…），表示由哪个 core 服务实现 |
| `twinAliases` | 旧 `remote_*`/`wsl_*` 命令名到"父方法 + target"的别名表，不属于方法面 |
| 能力门控 | 方法声明 `requiresCapability`，由宿主声明能力，客户端不得自行推断 |
| generation | 一个任务的某一次运行的序号，权威在 `task-operations.json` |
| operation key | 调用方给的幂等键；同键重复投递是同一次尝试 |
| 托管 home | `~/.aeroric/agent-homes/<family>`，Aeroric 读写的 Agent 配置目录；不是用户自己的 `~/.claude`、`~/.omp` |
| 门禁 / 地板 | `pnpm native:check <module>` 跑的一组 case；地板是冻结的最少用例数，只升不降 |
| 正控 | 证明静态扫描确实读到内容的断言 |
| NOT RUN / DEFERRED | 被暂停或未执行的检查，永不记为通过 |

## 文档地图

- `docs/subsystems/`：各子系统现状（[core](subsystems/core.md)、[宿主与 CLI](subsystems/host-and-cli.md)、[Agent 驱动](subsystems/agents.md)、[Agent 能力契约](subsystems/agent-capabilities.md)、[macOS App](subsystems/macos-app.md)、[iOS 伴随端](subsystems/ios-companion.md)、[远程与推送](subsystems/remote.md)）
- `docs/cookbook/`：[构建与验证](cookbook/build-and-verify.md)、[签名与分发](cookbook/release-and-signing.md)、[参照项目许可登记](cookbook/reference-provenance.md)
- [`docs/remote-public-access.md`](remote-public-access.md)：远程访问安全指南（relay 部署者必读）
- [`docs/backlog.md`](backlog.md)：产品能力候选库
- `docs/native/`：门禁与生成器读取的机读工件，见 [docs/native/README.md](native/README.md)
- `.agents/notes/`：决策与取舍笔记
