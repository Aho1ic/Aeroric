# Aeroric 原生功能接线交接（2026-10-23）

> 交接目标：让下一位 agent 在**不丢失产品决策、不覆盖现有未提交改动、不误触真实数据或 UI/VM**的前提下，继续 `10-23-native-feature-wiring-01`。
>
> 本文是执行交接摘要。产品与范围真源仍为 [`harness-feature-candidates.md`](./harness-feature-candidates.md)；逐项证据以该文引用的调研底稿为准。若本文与真源冲突，以真源及更新日期更晚、由用户明确确认的决定为准。

## 1. 接手后先做什么

1. 保持在 `main`，不要创建 branch/worktree；先运行 `git status --short`，把现有 dirty tree 当作其他协作者共享中的工作区，**不要 reset、checkout、clean、stash 或回滚陌生改动**。
2. 阅读：
   - `AGENTS.md`
   - `CLAUDE.md`
   - `docs/native/harness-feature-candidates.md`
   - `.trellis/tasks/10-23-native-feature-wiring-01/prd.md`
   - 本文
3. 先检查本轮迁移切片的未提交文件（见 §8），不要重新实现；只有发现回归或需求冲突才做最小修正。
4. 接着从 §10 的待办中选择一个边界清晰的后端切片。不要同时展开 UI、官方账号入池和真实数据迁移。
5. 每次只跑与改动直接相关的非 UI 检查，并如实记录结果。未经用户明确要求，不 commit、push、tag、发布、签名交付或操作真实数据。

## 2. 当前项目与不可突破的边界

- 产品仅支持 **macOS 原生 SwiftUI App + iOS/iPadOS 伴随端**；旧 React/Tauri/RN 客户端已经删除，不再引用 `src/`、`src-tauri/`、`mobile/`。
- macOS 走 Developer ID + 公证官网分发；iOS 上 App Store。host 保持独立进程。
- 业务真相及写操作在 Rust core/host；每个数据根只有一个 writer。SwiftUI 只读 read model、发送 typed action，不拼 Agent argv、不解析原始会话 JSONL、不直接写共享数据。
- 四族 Agent：Claude Code、Codex、oh-my-pi（omp）、DeepSeek Harness（DSH）。只经 PTY、hook、会话文件和托管配置施加影响，不改 Agent 内核、不重绘 TUI。
- omp 必须使用真实 PTY；绝不传 `--mode`；flags 在 `--` 前、prompt 在后；托管 home 用 `PI_CODING_AGENT_DIR`，启动前清除 `PI_PROFILE`/`OMP_PROFILE`；不得扫描用户 `~/.omp`。

> **已被 ADR-043 修订（2026-10-25）：** Claude Code、Codex 改为经结构化协议驱动（`claude` stream-json 控制协议、`codex app-server`），不再跑在 PTY；omp 的 PTY 与“绝不传 `--mode`”规则在二期迁移前仍有效。
- 测试使用隔离数据根、假 agent 和 `MemorySecrets`，不得触碰真实 agent home、Keychain 凭据或生产数据。
- **UI/VM 全面暂停**：不得启动 App、运行 XCUITest、`pnpm native:ui-test`、accessibility check、手工窗口自动化，也不得创建/启动/恢复测试 VM。此类检查必须记为 `NOT RUN / DEFERRED`，不能写成通过。
- 新界面直接做 SwiftUI，由用户审签名 App；不做浏览器 mockup。但“直接做 SwiftUI”不代表 agent 获准自行启动 UI。
- 专有许可；CometixCode 为 AGPL-3.0，只能借机制，不能复制实现。其他来源的配方正文也应自行编写或逐项核许可。
- 不执行破坏性 Git 操作；不自动 kill lock holder；不改生产 CI、远程服务、数据库或发布渠道。

## 3. 产品真源摘要：20 个能力族

以下是 `harness-feature-candidates.md` §3 的自包含压缩版。优先级仍以真源为准。

| ID | 能力族 | 目标形态 | 建议阶段 |
|---|---|---|---|
| A01 | 工作配方库 | 五组目录、manifest、依赖探测、诚实启用状态、按族注入；导入先预览 | 目录/注入 P0，导入导出 P2 |
| A02 | 主计划与工作包 | 计划审批、按修订指纹精确一次拆解、parent/blocks 分离、单负责人、依赖晋升 | P1 |
| A03 | 任务团队与委派 | 恰好一个 lead、角色绑定、原子领取、Aeroric MCP 委派、树级暂停/取消、预算护栏 | P1 |
| A04 | 反馈/消息/人工问题 | 工作包消息及已存/已投递/已读/已采纳状态、PTY 就绪闸、忙时排队、待回应卡、结构化反馈 | 卡片/排队 P0，其余 P1 |
| A05 | 质量与交付证据 | 回合 git diff、自动检查及过时态、只读 verifier、审查政策、Office/PDF/图像产物预览 | diff/检查 P0，其余 P1 |
| A06 | 任务进化 | Signal→Proposal→Evaluation→Revision；反思/治理分离、审批、版本与回滚 | P1，评测 P2 |
| A07 | 任务上下文与历史 | 托管 home 内发现、标题、导出、搜索、历史检索 MCP、按能力开放 fork | 标题/导出 P0，搜索/MCP P1 |
| A08 | 项目知识与记忆 | 候选记忆、置信度和原文引用、配额、审计、禁止自动写开关、外发密钥扫描 | P1 |
| A09 | 工具与扩展资源 | 技能/MCP 统一库、按四族投影、四态链接、OAuth、深链导入预览 | P0 |
| A10 | 角色配置与环境方案 | 内建 Harness 真实配置目录；角色绑定族/模型/权限；三层提示词投影 | 配置目录 P0，角色库 P1 |
| A11 | 连接弹性与协议适配 | 能力三元组、协议协商、连接/模型路由/号池/Agent 配置及本机网关 | 能力三元组 P0 |
| A12 | 工作安排与 typed 流程 | 一次/间隔/日/周/cron，错过只补一次，忙时排队，可选 EventKit；不执行模型生成 JS | P2 |
| A13 | 统计与资源护栏 | 用量/估算费用、预算、并发/无进展护栏、重复/振荡提醒、脱敏日志 | 日志 P0，护栏/预算 P1 |
| A14 | 连续执行/活性/恢复 | 非终态活性契约、静默提示、卡死检测、防休眠、单实例、Stop/Archive、Ralph | 活性/防休眠/单实例 P0 |
| A15 | 项目检查与安全变更 | 可选 worktree、两段合并、Git 工具页、Forge | Git/worktree P1，Forge P2 |
| A16 | 原生工作台与事实同步 | revision 栅栏、幂等回执、IME 安全、菜单栏、深链、终端尺寸单 owner、语音 | 菜单栏/深链 P1 |
| A17 | 权限/审计/外发 | 远程默认观察、显式接管、有限 Auto review、高危恒拒、外发密钥扫描 | 接管 P0，Auto review P2 |
| A18 | 远端 Host 与移动控制 | E2EE 配对、局域网/自建中继、游标恢复、观察终端、接管、文件预览 | P0 |
| A19 | 待回应与态势通知 | Mac/iOS 共用处理队列、白名单 APNs、深链、OSC 通知、Live Activity 后续 | P0 |
| A20 | 配置迁移与备份恢复 | 分域导出、加密/系统备份、回滚快照、last-known-good | P2 |

## 4. 四条主线设计摘要

### 4.1 配方库

- 首批目录：代码开发（专家＋例行）和轻量问答；办公、科研随 Python/uv 依赖进入后续批次；能力沉淀包含 retro/refiner/creator。
- manifest 归 core，记录 id、分类、步骤、注入模板、技能依赖、环境/key 依赖、推荐能力和产物类型。
- 模板只预填输入框，不自动发送；配方不改变模型和权限；冲突必须显式提示。
- 技能先进中央库，再按四族投影到托管 home；不可把“文件存在”误报为“可用”。

### 4.2 多 Agent 协作

- 团队恰好一个 lead；用户和 iOS 默认只与 lead/待回应卡交互；成员不能再创建团队（深度 1）。
- 工作包是唯一协作单元且单负责人。领取由 core 单写者做 CAS；冲突即停，不自动重试。
- host 暴露 delegate/status/cancel/complete/progress 等 Aeroric MCP 工具；子任务仍是普通 PTY 任务。
- 消息挂工作包并独立记录投递状态；注入必须经过空闲/无人输入的就绪闸。
- 审查退修超过 3 轮升级给用户；多 Agent 上线时必须同时具备预算、并发、无进展和派发熔断护栏。

### 4.3 自进化

- 只修改托管物：托管技能、按族/模型版本化指令、角色、托管记忆和 MCP 配置；绝不改四族内核或用户自有目录。
- 统一 Signal 来自 hook、会话、用户反馈、verifier、检查、卡死/失控检测；同指纹冷却。
- 会话结束反思默认关闭、按项目开启；反思者只看有界证据，输出固定结构；治理者独立且监督不可关闭。
- 所有提案经用户审批并形成 revision；事务式应用，评估不过不生效，失败回到 last-known-good。
- 进化代码与运行状态分离，每次在任务专属副本中工作；不允许凭据回退进入子进程。

### 4.4 iOS 远程控制

- 执行永远在用户自己的 Mac；iOS 只发送 typed 意图，不运行 shell、不下载执行代码。
- 两阶段 E2EE 配对：二维码时效 ≤120s，iPhone 握手后必须由 Mac 确认设备名和指纹。
- 首发局域网 Bonjour＋用户自建中继入口；产品方无状态中继留作后续订阅。
- 每任务单调 seq 日志；前台恢复用 cursor，缺口时重拉快照；按钮只看能力三元组。
- 远程默认观察，显式接管后才能输入；APNs 载荷不含内容，只做唤醒和深链。

## 5. Agent 配置、模型路由与号池的已定模型

这是当前 B1 后端线的核心，不应擅自改型：

1. **连接**：官方 OAuth、本机识别关联、或 API key＋base URL；协议为 Responses / Messages / Chat Completions。凭据只进 Keychain，文档只存引用。
2. **模型目录**：挂在连接上且带版本；上游 ID、映射名、显示名、上下文、输出、输入类型、推理档位分开。
3. **模型路由**：按映射名汇集多个渠道；只在首个上游输出前故障转移；400/405/406/413/414/415/422/501 等请求错误不切；含熔断、半开探测和首包前缓冲。
4. **号池**：独立对象，全局共享冷却；支持 priority bucket、round-robin、weighted、fill-first、least-used、pace、free-first；会话粘滞且子代理继承；按错误类型冷却，401 先刷新 token。
5. **Agent 配置**：Harness ＋ connection/router/pool ＋推理档映射。连接不属于某个 Harness；不兼容组合要给稳定原因，能转换的标“经网关转换”。
6. **本机网关**：复用 `local_router`，host 按需启动；只绑 `127.0.0.1`，每任务独立 token；支持三协议互转；可直连时不绕网关。
7. **任务快照**：任务创建时固定配置；普通修改只影响新任务，但撤销 key/删除账号等安全动作立即生效。
8. **首发族**：Claude Code、Codex；omp、DSH 下一批，期间只支持直连配置。
9. **官方账号入池**：用户已知晓条款/封号风险并选择允许；功能必须默认关闭、按账号显式启用并在 UI 标风险。该项排在本批最后，不能把第三方实现直接复制进来。

## 6. 已确认决定（Q8–Q37 摘要）

- 未接通能力：禁用并标“即将支持”，但不阻断其他可用发送路径。
- 原生端和 core/protocol 使用专有许可；旧 Tauri/移动端已清理。
- Agent 消息挂工作包，投递状态为已存/已投递/已读/已采纳。
- 办公引擎采用 python-docx/openpyxl/python-pptx，并与科研共用 Python/uv。
- 批次顺序固定为 B1→B2→B3→B4→B5→B6。
- 首批配方为代码开发和轻量问答；会话结束反思默认关闭。
- 网关为本机按需服务；连接与 Harness 解耦；号池独立且冷却全局共享；故障转移按映射名聚合并允许跨协议。
- 凭据目标状态是 Keychain 且清除明文；旧 custom agent 迁移成功并验证后，才进入删除旧脚本/key 文件的清理阶段。
- `core/.../git/service.rs` 的既有未提交改动不要碰。
- 开工载体为 Trellis 任务 `10-23-native-feature-wiring-01`；界面不做浏览器稿，由用户审实际签名 App。

## 7. 实施批次

| 批次 | 内容 |
|---|---|
| B1 闭环可用 | Harness 真实配置、能力三元组、未接通状态、回合 diff/自动检查、标题/导出、活性/防休眠/单实例、诊断日志、Git/文件/运维接线 |
| B2 配方库 | 技能/MCP 统一库、按族投影、配方 manifest/目录、状态与依赖探测、加号菜单 |
| B3 iOS 远程 MVP | 配对和 Mac 确认、局域网、游标恢复、任务观察、待回应、APNs、接管 |
| B4 协作 | 工作包/计划、团队/lead、原子领取、委派 MCP、消息闸、审查、预算/并发护栏 |
| B5 自进化与记忆 | Signal、反思提案、审批/版本/回滚、记忆候选、U01 三角色 |
| B6+ | 外网中继、iOS 终端输入、调度、评测、Ralph/Goal、worktree、备份同步、Forge、数据库/笔记 |

## 8. 本终端刚完成的迁移切片（未提交）

### 8.1 已实现

- 新增 `core/crates/aeroric-core/src/agent_connections/migration.rs`：
  - 从 legacy `settings.json`、Aeroric-generated custom wrapper、credential sidecar 或旧 SecretStore marker 复制 Claude/Codex 的可表示配置。
  - 支持 deterministic id、冲突跳过、同名配置保护、模型去重、generated-wrapper/path/symlink 安全判据。
  - legacy 输入只读；不覆盖新文档中的冲突对象或不同值凭据。
  - 新凭据写入 `SecretStore`，不进入 `agent-connections.json` 或报告。
  - warning 汇入 `MigrationReport`；报告原子写到私有文件 `agent-connections-migration-report.json`。
  - 首次迁移与第二次 `already_present` 幂等路径都有测试；OMP/DSH legacy profile 当前明确跳过。
- `core/crates/aeroric-core/src/settings/agent_scripts.rs`：将 generated-wrapper 判据可见性扩大到 `pub(crate)`，供迁移模块复用。
- `core/crates/aeroric-host/src/startup.rs`：
  - `run_startup` / `run_startup_with_secrets` 默认**不迁移**。
  - 新增 `run_startup_with_secrets_and_legacy_agent_migration`，仅生产组合显式选择。
  - 迁移位于数据兼容性检查之后、token/listener/ready 之前；失败映射为 startup failure，不能宣告 ready。
- `core/crates/aeroric-cli/src/main.rs`：生产 `serve` 在首次读取 settings 前安装 `KeyringSecrets`，并调用生产迁移入口。
- `core/crates/aeroric-host/tests/host_lifecycle.rs`：新增真实文件副作用测试，证明库入口不迁移、生产入口才迁移。
- `core/crates/aeroric-host/tests/database_dbx_services.rs`：组合断言更新到生产显式入口。

### 8.2 本轮实际验证

- migration 定向测试：3 passed，0 failed。
- host lifecycle：6 passed，0 failed。
- DBX 启动组合断言：1 passed，0 failed。
- `cargo check`：`aeroric-core`、`aeroric-host`、`aeroric-cli` 通过。
- `cd core && cargo fmt --check`：通过。
- 未运行全 workspace 测试；未运行 UI/VM；未 commit/push/publish。

### 8.3 已审查的失败语义

- 必须先写凭据再写引用它的文档，避免文档指向不存在的 secret。
- 若凭据成功而文档原子写失败，可能留下“同值、无引用”的新 secret；下次迁移会识别并复用，再补写文档。没有做删除补偿，避免误删并发更新或增加 Keychain 删除失败面。
- 若文档成功而报告写失败，startup 失败且不会 ready；下次迁移按 `already_present` 重写报告。
- 报告和 connection JSON 均不包含凭据。

### 8.4 重要：复制阶段不等于 Q28/Q37 清理完成

当前实现有意是**非破坏性复制阶段**：不会清除 legacy settings 明文，不会删除旧 wrapper、sidecar 或旧 Keychain 项。它完成了安全复制、冲突报告和生产启动前执行，但没有完成真源中以下后续动作：

- Q28：“迁入 Keychain 后清除明文”。
- Q37：“迁移并验证成功后再删启动脚本与 key 文件；失败项原样保留并标原因”。

下一位 agent 不得把这些删除混进当前复制函数，也不得直接对真实用户目录执行。应先设计**可证明成功、逐条确认、可重试、失败保留原件**的清理阶段，为隔离夹具补测试，并在任何真实数据动作前重新取得用户明确授权。

## 9. 当前工作区责任边界

- 工作区存在大量未提交新增、修改和删除，横跨旧端清理、CI、native/core/remote/backup 等多条工作线。它们不是本轮迁移切片可以随意整理的垃圾。
- 本轮迁移直接相关文件只有：
  - `core/crates/aeroric-core/src/agent_connections/migration.rs`（untracked）
  - `core/crates/aeroric-core/src/settings/agent_scripts.rs`
  - `core/crates/aeroric-host/src/startup.rs`
  - `core/crates/aeroric-host/tests/host_lifecycle.rs`
  - `core/crates/aeroric-host/tests/database_dbx_services.rs`
  - `core/crates/aeroric-cli/src/main.rs` 中的 Keyring/启动入口接线
- `main.rs` 同时含 backup 与 remote config 等其他协作者改动；不要整文件回退、覆盖或把全部 diff 归因于迁移。
- 用户未授权 commit。即使后续获准，也应按文件/逻辑切片精确暂存，先检查 mixed hunks；不得 `git add .`。
- `docs/native/task-status.json` 显示旧路线的 `P8-05` 仍为 `in_progress`，P9/P10 尚 pending；这与新的 B1 Trellis 任务不是同一个状态轴，不要擅自改成 accepted。
- `.trellis/tasks/10-23-native-feature-wiring-01/task.json` 仍为 `planning`，PRD 已写但没有 commit/PR；不要仅因迁移切片通过就把整个 B1 标为完成。

## 10. 下一位 agent 的未完成任务

### 10.1 立即可做：迁移切片收尾

1. 重新读取 §8 所列文件与最新 `git diff`，确认没有后来协作者改动与本文冲突。
2. 运行与当前 HEAD 对应的定向 migration、host lifecycle、DBX composition 和三 crate check；若未改代码，可引用已有结果但在最终交付前应使用新鲜输出。
3. 评估并单独设计 Q28/Q37 清理阶段：
   - 成功判据必须包含新文档对象、新 secret 可读、报告落盘及源→目标对应关系。
   - 只清理成功条目；冲突/跳过/失败条目保持原样。
   - 区分 settings 内明文、sidecar、wrapper、旧 Keychain 项；不能用一个全局“迁移成功”布尔值批量删除。
   - 先做隔离测试和 dry-run/report，真实数据需用户再次明确授权。
4. 不要为“整洁”删除无引用 secret；当前幂等恢复语义是有意选择。

### 10.2 B1 后端仍未因本切片而自动完成

按 PRD 逐项核对实际代码与测试，不能以文件/方法存在代替行为验收：

- Connection / ModelCatalog 的完整版本化模型和 host CRUD。
- 独立号池、全部已定策略、会话粘滞与按错误类型冷却。
- 按映射名的多渠道路由、首输出前故障转移、请求错误不切换、熔断/半开/首包缓冲。
- `local_router` 三协议互转、loopback-only、每任务 token、按需启动与直连绕过。
- 任务配置快照及安全撤销即时生效。
- `session_catalog.rs` 输出真实模型/权限/思考档，解除内建 Harness 发送阻断。
- aeroric-protocol 能力三元组（supported/available/allowed、稳定原因码、revision）及 Mac/iOS 消费。
- `connections.*` / `pools.*` / `router.*` / `models.detect` 等 typed host 方法。
- 官方账号 OAuth/入池必须最后做、默认关闭、风险可见；先核法律/条款和实现边界，不要发送代码或凭据到第三方。

### 10.3 B1 其他闭环项

- 回合改动＋自动检查及“后续编辑后已过时”状态。
- 会话标题与 Markdown/JSON 导出。
- 活性提示、防休眠、单实例租约。
- 脱敏诊断日志导出。
- Git/文件/运维工具页与现有 host 方法接线。
- UI 线只有在后端真实可用后再接：连接页、号池页、Agent 配置页、新会话 Harness→配置选择。不可运行 UI 自动化；最终由用户审签名 App。

## 11. 明确不做或暂缓

- 不做 Computer Use、任意模型生成 JS 工作流、JSX Studio、桌面宠物、推测执行、自定义 CSS 主题。
- 冻结独立 IDE 的 LSP/DAP/补全/大纲；诊断和测试归 A05，运行配置/Web 预览归任务运行区。
- iOS 不执行 shell，远程写文件默认关闭。
- 不做账号体系、公开分享链接服务、Codex 私有 IPC follower。
- 不干预四族内核中的工具参数修复、多级压缩或工具结果裁剪。
- macOS App Store 版未来也不做自更新。
- 当前不接 omp/DSH 网关；不启动 UI/VM；不做真实数据 cutover。

## 12. 验证命令与报告纪律

常用命令（按改动范围选择，不要机械全跑）：

```bash
cd core && cargo fmt --check
cargo test --manifest-path core/Cargo.toml --workspace
cargo check --manifest-path core/Cargo.toml -p aeroric-core -p aeroric-host -p aeroric-cli
swift test --package-path macos/AeroricKit
swift test --package-path ios/AeroricCompanionKit
pnpm native:doctor
pnpm native:check <module>
pnpm native:build
```

纪律：

- 只根据本次实际输出写“通过”；没跑就写 NOT RUN。
- `pnpm native:build`、签名、zip、sha256 属于每批交付动作；若会产生对外/签名副作用，先确认当前授权。
- UI/VM 检查仍是 DEFERRED，不能通过其他 runner 绕开暂停。
- 真实数据迁移参照 `docs/native/migration-rehearsal.md` §4；该文明确需要用户同意。不得仅凭合成夹具成功就操作 `~/.aeroric`。

## 13. 关键资料索引

- 产品与 backlog 真源：`docs/native/harness-feature-candidates.md`
- 旧版完整决策流水：`docs/native/archive/harness-feature-candidates-2026-10-22.md`
- 当前 B1 PRD：`.trellis/tasks/10-23-native-feature-wiring-01/prd.md`
- 项目总规则：`AGENTS.md`、`CLAUDE.md`
- 迁移演练与真实数据边界：`docs/native/migration-rehearsal.md`
- UI 测试暂停：`docs/native/ui-testing-policy.md`
- Agent 配置调研：
  - `docs/research/agent-config-review-2026-10-23.md`
  - `docs/research/agent-config-aeroric-state-2026-10-23.md`
- 20 个来源的调研底稿：
  - `docs/research/harness-full-review-2026-10-23-g1.md`
  - `docs/research/harness-full-review-2026-10-23-g2.md`
  - `docs/research/harness-full-review-2026-10-23-g3.md`
  - `docs/research/harness-full-review-2026-10-23-g4.md`
- 连接 UI/模型设计：
  - `docs/native/add-connection-design-review-v5.md`
  - `docs/native/connection-model-catalog-design-v5.md`
  - `docs/native/agent-connections-design-checkpoint-01.md`

## 14. 接手完成判据

下一位 agent 在声称某一切片完成前，应同时给出：

1. 修改范围与用户既有 dirty changes 的边界。
2. 行为证据，而不只是源码字符串或方法计数。
3. 凭据不落 JSON/报告、只使用隔离 SecretStore 的安全证据。
4. 默认入口与生产入口的副作用差异测试。
5. 实际执行的 fmt/check/test 输出；未执行项明确列出。
6. 未运行 UI/VM、未操作真实数据、未 commit/push/publish 的说明（除非用户后来明确授权）。
