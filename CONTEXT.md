# CONTEXT — 术语表（原生迁移）

纯术语表。实现细节在 `MIGRATION_PLAN.md`，边界与决策在 `docs/native/adr/`，逐项映射在
`docs/native/command-map.json` / `docs/native/feature-matrix.json`。此文件只统一"同一个词只有一个意思"。

| 术语 | 定义（不是实现说明） | 不是什么 |
|---|---|---|
| 冻结清单 / frozen inventory | `docs/native/command-map.json` 里 660 条命令的逐项映射，是可比对的基线工件 | 不是随手更新的草稿；改动它等同于改协议 |
| 旧命令名 / legacy command name | `rust_path` 的尾段，即今天 Tauri `invoke("…")` 用的字符串 | 不是 RPC 线名（线名是 `rpc_method`） |
| RPC 线名 / `rpc_method` | 本地协议 v1 的方法名，口径 `<服务面>.<旧命令名>`（ADR-002） | 不是 relay 协议的方法名，两者版本互不借用 |
| 服务面 / service area | 22 个能力分组名（`database`、`notebook`、`git`、`dsh`、`ide` …），即 RPC 前缀的取值域 | 不是 Rust 模块名（56 个），也不等于源码目录 |
| `core_service` | 该命令由 `aeroric-core` 的哪个服务实现 | 不是 UI 归属，也不是 check 模块 |
| 类别 / `category` | `domain_rpc` / `platform_specific` / `host_local_os_ui` / `internal` 四选一，决定能否被 host 执行 | 不是"实现语言"；值必须由 `meta.category_resolution` 推导可复现 |
| 归属类字段 / ownership field | 决定"谁能执行、谁实现、走哪条路由"的字段（`category`、`rpc_method`、`core_service`、`check_module`） | 值不许停留在"当初手工定的"——必须有登记的推导 |
| check 模块 | `scripts/native/checks.json` 里一个验收入口的名字（如 `skeleton`） | 不是测试文件；不是 CI job |
| 门禁 / gate | `pnpm native:check <module>` 跑出的一组 case，全部通过才算该模块验收 | 绿灯不等于工件完好（见"正控"） |
| 正控 / positive control | 证明"扫描确实读到了内容"的断言（每个被扫路径至少一条） | 不是"没匹配上就算干净"——空语料与被忽略文件都长这样 |
| 地板 / floor | `runner.floors` 里冻结的最少用例数，与 `minCases` 同一次编辑才允许改，只升不降 | 不是可以顺手调低的阈值 |
| 反例 / counterexample | 能推翻当前实现的一个具体场景（含必须由真实测试覆盖的那类） | 不是"看起来能跑" |
| §5.1 展开表 / expansion | 一个目录级模块"一文件一条"的迁移行清单，可由 `scripts/native/extraction.mjs` 重导 | 不是授权改架构；不是手写后就准的清单 |
| 唯一写入者 / single writer | 一个数据目录同时只有一个业务写入进程（INV-03） | 不是"锁文件存在就代表它还活着" |
| PTY generation | 一次 PTY 生命周期的代号，用于丢弃迟到的退出/事件（INV-04） | 不是 taskId；不是 sessionId |
| 托管 home / managed agent home | `~/.aeroric/agent-homes/<family>` 下由 Aeroric 读写的目录 | 不是用户自己的 `~/.omp`、`~/.claude` 等外部历史 |
| 纯原生 / 高级(advanced) 模式 | 呈现方式偏好（`interactionMode`），切换不重启 Agent、不改协议 | 不是外观的 glass/classic，也不是权限档位 |
| 零证据 / zero-evidence | 在生产与测试源码里都找不到任何调用线索的命令 | 不是"确定死代码"；前缀派生与只被 mux 触达都另计 |
| 已退役 / RETIRED | `protocol_inventory.rs` 里显式标为退役的条目（48 项），不计入能力面 | 不是"待支持"，也不是 live 名字 |
| 能力门控 / capability gate | 方法在 schema 上声明 `requires_capability`；**能力由 host 声明，client 不得用自己所在 OS 推断 host 能力**（ADR-003） | 不是客户端本地开关；控件禁用之外服务端也必须拒绝 |
| 平台面 / platform surface | 62 条 `platform_specific` 命令（`wsl_git` 32、`wsl` 19、`wsl_fs` 9、`platform` 2）及其方法 | 不是"暂不迁移"的委婉说法；已进入协议 v1 |
| 方法面 / method surface | 协议 v1 冻结时包含的全部方法（ADR-003 后为 641） | 不是 660：其中 19 条 host_local_os_ui 有意无线名 |
| 权威来源 / authoritative source | 一个事实唯一的答案所在处（如事件通道以源码常量与调用点为准） | 文档转述、ADR 表格、记忆里的数字都不是 |
| 回滚点 / rollback point | 基线 commit `ca69a4b` + 可整块丢弃的新增目录 | 不是 `git reset --hard`；不是把旧全量内存快照写回去 |
| 未决 / open、deferred | 需要用户拍板或需单独审定才可关闭的事项（Q1、Q5、Q6、Q7） | 不许写成"已通过"；关闭需带实测复验 |
