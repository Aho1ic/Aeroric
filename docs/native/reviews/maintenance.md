# P8-04 评审记录 — 统计、通知、清理、周报、权限与更新（2026-10-07 收口）

## 1. 范围与实现（六刀）

- **刀一（core `maintenance`）**：`maintenance/{mod,cleanup,reports,notifications,updates,diagnostics}.rs`
  ＋`tests/maintenance.rs` 24 用例。清理**执行半边**（规则半边 P2-04 已在
  `tasks/cleanup.rs`）：`run_cleanup_once(TaskService, CleanupSchedule, LocalCalendar,
  now_ms)` 逐项目 `expired_task_ids` → `TaskService::delete`（单写者），候选/实删/逐项目
  失败分开记账；`CLEANUP_TICK_MS = 30min`。周报：`week_window`/`tasks_in_window`/
  `week_label`（取区间中点算 ISO 周）/`render_report`/`generate_weekly_report`
  （串行、上限 40、逐条降级），摘要走 `ReportSummarizer` 端口，落盘 0600。通知：抓取
  节流收 `now`、过期收 `today_utc`、可装性收 `os/arch/linux_kind`。更新：
  `compare_versions`/资产选择/路径守卫/哈希实读。降级账：`StartupDegradationLog` 实例。
- **刀二（core `metrics` 族）**：`analytics`(1876)/`index`(1913)/`agent_usage`(321)/
  `usage`(660) 四文件 `git mv` 上收，**55 条自带单测随迁**；`MetricsPaths` 携带
  `home`（只读扫外部会话）与 `data_dir`（可写落库）。**G03 半边闭合**：
  `UsageIndexWatcher{stop, join}`（幂等、drop 即停、250ms 分片）取代裸
  `thread::spawn`；**G21**：族内不再有 `aeroric_dir()`/`env::var("HOME")` 自寻路径。
  `IndexEventSink` 取代 `AppHandle::emit`。
- **刀三（宿主接线）**：core `maintenance/service.rs`＝`MaintenanceHost` 端口＋
  `MaintenanceService`（旧 `get_notifications`/`mark_*`/`prepare_release_update`/
  `generate_weekly_report` 命令体全部上收）；aeroric-host
  `maintenance_services.rs`＝`usage.*` 六方法＋`diagnostics.*` **总在**、`maintenance.*`
  九方法**只在注入端口时**注册；`HostWiring` 注入；GUI 薄壳化
  （`notification.rs` 1671→891、`report.rs` 528→65）。两条 v1 topic 双发。
- **刀四（Swift 客户端与七视图）**：`AeroricClient/MaintenanceClient.swift`（13 方法，
  缺键降级、`UsageSource` 三分支、`SessionMetrics` 保持 snake_case）＋
  `AeroricUI/Operations/{MaintenanceStore,OperationsViews}.swift` 七视图＋13 用例。
- **刀五（原生权限面）**：`PermissionPresentation`（纯策略：ad-hoc 不读成永久授权、
  `notBundled` 优先且无 Re-authorize、`unknown` 不算已授权、`ReauthorizeSequence`
  未确认即空表且不越界清服务）＋`SystemPermissionProbe` 端口与 `MacOSPermissionProbe`
  真实现（native 自己的身份；reset 三守卫）＋16 用例。
- **刀六（门禁与收口）**：`native:check maintenance` 5 case／69 用例注册；本文件；
  台账与进度同步；G03/G06 收口。

## 2. 验证实测（当日命令）

| 命令 | 结果 |
|---|---|
| `cargo test -p aeroric-core --lib` | **1639 / 0** |
| `cargo test -p aeroric-core --test maintenance` | **24 / 0** |
| `cargo test -p aeroric-core --test usage_index` | **11 / 0** |
| `cargo test -p aeroric-host --test maintenance_services` | **5 / 0** |
| `swift test --package-path macos/AeroricKit` | **全绿**：UIUnit **190/0**、Client 42/0、Protocol 24/0 |
| `pnpm native:check maintenance` | **5 case / 69 用例**（floors 双写） |
| `pnpm native:check skeleton` | **24 case / 188 用例** 全绿 |
| `pnpm native:check tauri-adapter` | **12 case / 1268 用例**（宿主 lib 地板 1277→1222→1215→1207） |
| `pnpm native:check cli` | **5 case / 10 用例** |
| `cargo clippy -D warnings`（core/host/cli/protocol ＋ src-tauri） | 干净（含 `--all-targets`） |
| `cargo fmt --check`（core ＋ src-tauri） | 干净 |
| `node scripts/native/inventory.mjs --check` | consistent（660/26/40/41 events/51 taskStates） |
| `node scripts/native/contracts.mjs --check` | 12 份逐字节 current |
| `node scripts/native/digest.mjs --check` | verified 15 片，最大 21 798 B |
| `pnpm test` | **375 文件 / 5456 用例全通过** |

## 3. 真发现与根修

- **guard 跨 await**（刀三）：`MaintenanceService::get_notifications` 首版把
  `parking_lot` guard 持到 `fetch_releases().await` 之后——clippy
  `await_holding_lock` 当场逮住。改成同步临界区 `with_store`（core 规约
  "guard 不得跨 await"）。
- **`export-schema` 的自承接判据**（刀三）：校验器把**以 v1 主题名发出的字面量**
  当成"旧通道"，要求它被某个 v1 topic 承接＝要求它承接自己，判据永不成立。
  **该红灯在 HEAD 工件上同样复现**（当场用 HEAD 版 command-map/event-topics
  跑出同一错误）⇒ P8-03 留下的既有缺陷。修法：`channels` 过滤掉已声明的主题名；
  同一口径同步到 TS 与 Swift 两处覆盖判据。
- **`task-core-parity` 的环境依赖**（刀三）：夹具把"周日 20:00"换算成 epoch 存表，
  被测的 `shouldRunCleanup` 又按**进程时区**把 epoch 读回本地分量 ⇒ 表只在那台生成
  它的机器上成立（提交表 UTC−7，本机 UTC+8 重算 → 11 行 input＋1 行 expected 全红）。
  修法：模块顶层钉 `process.env.TZ = "Etc/GMT+7"`（POSIX 反号＝UTC−7，即提交表的
  历史口径）。**判据是"表不用改就能绿"**——修完 3/3 通过且夹具零 diff。
- **门禁逮住的夹具 bug**（刀六）：`read_usage_statistics_reports_the_seeded_window`
  把本地墙钟文本硬贴 `+00:00` 当作 UTC 时间戳，在 UTC+8 机器上把记录算到"明天"，
  整窗落空——`native:check maintenance` 首次运行即红。改 `to_rfc3339()`（带真实偏移）。
- **dead re-export**：`line_is_dsh_header`/`validate_export_output_path`/
  `format_timestamp_ms`/`sanitize_md_inline` 四个 re-export 在生产消费者随迁后
  变成 lib 目标 dead code ⇒ 改为 `#[cfg(test)]` 组（与同文件既有先例一致）。

## 4. 验收原文对照（§Task P8-04）

- **Action 1**（core `maintenance/{mod,cleanup,reports,notifications,updates,diagnostics}.rs`；
  timer/policy 移出 React，保留用户 opt-in 与时区/频率）✅——六个文件全部落地；
  `CLEANUP_TICK_MS`＋`run_cleanup_once` 是宿主 tick 的入口，策略与时间窗都在
  core；opt-in（`enabled`/`NoPreviousRun`）与时区（`LocalCalendar` 端口）逐条保留。
- **Action 2**（`AeroricUI/Operations/` 七视图）✅——UsageDashboard /
  NotificationCenterView / CleanupSettingsView / WeeklyReportView /
  PermissionSettingsView / ReleaseView / DiagnosticsView，全部带
  `accessibilityIdentifier` 且在 `MaintenanceStoreTests`＋`PermissionPresentationTests`
  的判据下。
- **Action 3**（任务后台通知由 host 发抽象 event；NSUserNotification/打开系统设置/
  请求 TCC 由拥有当前 app 身份的 GUI adapter 执行）✅——core 出口是注入的
  `IndexEventSink`/`ReportProgressSink`；`MacOSPermissionProbe` 的
  `openSettings`/`reset`/`request` 只被显式动作调用。
- **Action 4**（native permission report 用实际 `com.aeroric.native.dev` 身份；
  保留 unstableSignature/notBundled 解释；Re-authorize 前说明 reset 的服务、
  bundle ID、后续重新请求，让用户确认）✅——`PermissionPresentation.plan` 全部覆盖；
  `PermissionSettingsView` 的 sheet 逐条列出三样东西才给确认按钮。
- **Action 5**（更新准备/下载/校验与实际替换分开；清理只处理满足策略的任务，
  不删运行中/等待输入任务或无归属目录）✅——`prepare_release_update` 与
  `restart_and_install_release_update` 是两条路；`run_cleanup_once` 只删
  `expired_task_ids` 的候选，`TaskService::delete` 再拒运行中的，无 id 的项目直接跳过。
- **Acceptance**：`tests/maintenance.rs`（24）＋`PermissionPresentationTests.swift`（16）
  ＋`MaintenanceStoreTests.swift`（13）✅；`pnpm native:check maintenance` ✅（5/69）。
  fake clock 验证 cleanup/report 时区与幂等：core 侧用例用固定日历夹具（`Utc8`/
  `Offset`）＋显式 `now_ms`，重启幂等由"重算候选"（删除幂等）覆盖 ✅。
  ad-hoc 重签后不虚假显示永久授权：16 条判据用例 ✅。
  **没有在后台静默 `tccutil reset`、提权或改系统权限** ✅——`reset` 三守卫＋
  `ReauthorizeSequence` 未确认即空表；真机实跑未执行（见 §7）。

## 5. 两条具名移交项与 gap 收口

- **`weekly.report_progress`（v1 event topic，旧常量 `report.rs:28`）**：
  **mapped**（宿主 `report.rs` 的进度 sink 双发旧通道＋v1 topic；payload verbatim
  ＝`{scope, completed, total, currentTitle}`）。同批 `usage.statistics_updated`
  也由 `pending: P8-04` → **mapped**。event-topics mapped 9→11、pending 29→27。
- **`G06`（后端行为仍由 React 决定）**：**closed**——判据：「native 路径的计时器
  与策略已归 core（`maintenance/cleanup.rs` 的 `CLEANUP_TICK_MS`＋`run_cleanup_once`、
  `metrics/index.rs` 的扫描循环），legacy React 客户端在 cutover 前仍跑自己的
  `setInterval`，其拆除属 P8-05 的具名工作」。证据：core 两处新落点＋
  `native:check maintenance` 的清理/周报用例。
- **`G21`（宿主测试写真实使用统计库）**：台账已 closed（2026-09-30）；本轮补上
  它点名的 P8-04 半边——**落点可注入**（`MetricsPaths`）。结构性判据：
  `grep -rn 'aeroric_dir\|env::var' core/crates/aeroric-core/src/metrics/` → 0 命中；
  新套件断言落点在注入目录内且 scratch 根内没有第三个目录。**诚实记账**：真实
  `~/.aeroric/usage-statistics.sqlite3` 在普查窗口内仍有变化，但空转对照（89s 零测试
  活动）同样三次改哈希 ⇒ 写者是**在跑的旧 app** 自身的索引线程，不是本轮代码。
- **`G03`（持有线程须可取消并 join）**：**closed**——第三半（`usage_index.rs:1048`）
  由 `UsageIndexWatcher` 关闭；前两半是 P3-04 的 `WatcherSupervisor` 与 P7-06 的
  `SyncDaemonHandle`。判据：「core 侧不再有裸 `thread::spawn` 承载长驻循环」。
  `stop()` 幂等、`Drop` 即停、250ms 分片保证一轮内退出（有用例）。

## 6. grill-me [maintenance]（§4.2 门禁，真实调用）

入口：`grilling` 技能（`skill://grilling`，本会话真实读取并按其"设计树 + frontier
单轮问完"的方式执行）。frontier 七问经 `ask` 工具一次性提交（2026-10-07），
**七问全按推荐项获答**：

- **Q1（`maintenance.*` 无头面）**：保持现状——无头宿主不注册这九条，
  method-not-found 是明确的能力边界，不注册"会失败的空壳"。
- **Q2（`usage.read_usage_snapshot` 归属）**：由 P8-04 认领，台账不动
  （`core_service: metrics::usage` 与实况一致）。
- **Q3（`install_release_update` 零调用方）**：保留并写明"两步是正路"，
  一步口留给 CLI/脚本。
- **Q4（真机 TCC 手测）**：记为「未运行·待用户手测」，挂 P10-02。
- **Q5（G03/G06 收口口径）**：两条都 **closed**，判据写明"native 路径已归 core，
  legacy 随 cutover 拆除"。
- **Q6（task-parity 时区钉法）**：留（钉 TZ、表不动）。
- **Q7（七视图挂载）**：不挂，保持 P6/P7/P8 先例（挂载属 P8-05 cutover）。

**G31 三件套（当场记）**：HEAD＝`8a9b5fc2adeed4c78bf651434bbf601e1105f168`
（刀五提交；刀六的门禁注册在此之后、本文件落盘之前，随收口提交一并落）；
porcelain＝收口批尚未提交的 3 项（本文件、checks.json、tests/usage_index.rs 的
夹具修正）；diff hash 以收口提交为准（见 §8）。

**五轴自查**：①唯一 owner——清理只有 `TaskService::delete` 一条写路径，用量库只有
`MetricsPaths.data_dir` 一处落点，通知账本只有 `MaintenanceService::with_store`
一个临界区；②反例——`native:check maintenance` 五条 case 均含反例（空任务/坏路径/
坏 JSON/越界服务/未确认清授权/codex 窗口为 0）；③边界——路径守卫（`safe_tag`、
`validate_pending_installer_path`）、URL 消毒、`sanitize_text` 截断、`tccutil` 三守卫、
身份不符则不显示已授权；④证据——本文件 §2/§3 全部当日实测，红灯与绿跑都留痕；
⑤悬置——无产品决策悬置（七问全答）。

## 7. 挂账（如实）

- **真机 TCC 手测未运行**：系统设置里的读数与 `tccutil reset` 实跑需要用户在场
  且会改本机授权状态 ⇒ 按 §5.4 处置，挂 **P10-02**（用户已确认）。
- **`maintenance.*` 在无头宿主不可达**（Q1 裁定）：能力边界的如实记账，
  不是缺口；将来若要 CLI 查更新需先有 HTTP 出口决策。
- **`install_release_update` 无 UI 调用方**（Q3 裁定）：保留并按
  `docs/native/reachability-triage.md` 记"有调用路径、无 UI 调用方"。
- **七视图未挂进 app shell**（Q7 裁定）：属 P8-05 的 cutover 工作。
- **`G21` 的行级读数未采集**：本轮判据改按结构（注入根＋落点断言），
  行级对账（表计数/关键聚合值前后不变）仍归 P10-05 的完整矩阵。

## 8. 收口提交

见紧随本文件之后的 `docs(maintenance): P8-04 收口` 提交：进度总表行、Task 标题、
`docs/native/task-status.json`、`feature-matrix.json`（G03/G06 closed）、
`inventory --freeze` 与 digest 重写同批。
