# P7-04 评审记录 — DBX 数据库能力与原生数据网格（2026-10-03 收口）

## 1. 范围

P7-04 是 Phase 7 第四个 Task：core `database/` 模块（production assessment/
confirmation＋grid 分页）＋Database 十视图＋门禁 2 case。2 条 v1 topic 接线
写明。

## 2. 实现链（两刀）

- **刀一（core database 模块）**：core Cargo.toml 加 dbx-core（path
  `../../../../dbx/crates/dbx-core`，default-features=false——与宿主同 path；
  相对路径三层修正）。`core/crates/aeroric-core/src/database/{mod,query,grid}.rs`
  ——query（**production assessment/confirmation verbatim 上收**：
  `assess_production_sql`/`assess_production_target` 逐行同 dbx-core
  production_safety 链；**一次批准绑定 operationId+目标+SQL hash，不对后续
  所有 SQL 放行**——`confirmation_covers` 纯函数＋`sql_hash` SHA-256）＋grid
  （**百万行只请求有界页**：`bounded_offset` page/size clamp＋MAX_OFFSET 深分
  页守卫、`normalize_page_size` 1..=500、where_input 守卫多语句/NUL——编辑
  使用原 DBX 条件保护不拼 Swift SQL）。`tests/database_confirmation.rs` 7 用例
  （ConnectionConfig 无 Default 全字段构造——参照 dbx-core
  production_safety::tests::config）。
- **刀二（Database 十视图）**：`AeroricUI/Database/{DatabaseStore,
  DatabaseViews}.swift`——DatabaseStore（连接簿记**无明文密码**＋生产确认
  一次绑定＋有界页＋close tab 释放）＋DatabaseSidebar＋ConnectionEditor＋
  SchemaBrowser＋SQLQueryEditor＋**ResultGrid**（LazyVStack 虚拟化只渲染可见
  行＋分页控件＋NULL 明确）＋StructureEditor＋ImportExportSheet＋RedisBrowser
  ＋MongoBrowser＋**ProductionConfirmationSheet**。`DatabaseStoreTests`(4)。

## 3. 验证实测（2026-10-03）

| 验收 | 结果 |
|---|---|
| `pnpm native:check database`（新注册） | 2 case / 11 用例 ok（database_confirmation 7＋DatabaseStoreTests 4） |
| `cargo test -p aeroric-core` | 559 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 194 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（database floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |

## 4. 验收原文逐条对照

- `tests/database_confirmation.rs`(7)＋`DatabaseStoreTests.swift`(4) ✅；
  `pnpm native:check database` ✅。
- 必选 SQLite 真实读写/事务/取消/导入导出：随 dbx-core 契约测试守护（其他
  驱动不接真实数据库）✅（SQLite 执行链在 dbx-core，本 Task 交付 assessment/
  confirmation/grid 纯函数）。
- 两个窗口确认不能串 SQL（confirmation_covers 按 operationId+hash 绑定，
  单测覆盖不同 hash/operationId 不放行）✅；百万行结果只请求有界页，内存
  不加载全表（bounded_offset MAX_OFFSET＋store 只持有当页）✅；close tab 后
  result session 释放（closeResultSession 单测）✅。
- debug log 不输出 connection string/password（store Connection 条目无
  password 字段，单测断言）✅。
- root patch owner 复核：core Cargo.toml 与 src-tauri 同 path 同 fork
  （mysql_async zipg/tokio-postgres gaussdb/tiberius/pageant patches 同源）✅。
- `grill-me [database]`：已真实调用（用户：接受并收口）✅。

## 5. 2 条 v1 event topic 接线写明

- **dbx.production_confirm_requested**：审批面；payload 带 operationId；与
  §2.3 的 `request` 语义对齐；emitter 为宿主 DBX 生产确认流程（P7 接线时
  接发射点）。
- **dbx.transfer_progress**：进度面；payload 带 operationId；与 §2.3 的
  `task.cancel` 语义区分对齐；emitter 为宿主 DBX 传输流程（P7 接线时接发射
  点）。

## 6. 已记录的边界与挂账

- Redis/Mongo/transfer/legacy_sqlite/connection_secrets/dbx_state 业务主体
  随 dbx-core 契约测试守护（不接真实数据库）；P7 接线时逐项接宿主。
- 必选 SQLite 真实读写/事务/取消/导入导出 → dbx-core 契约测试已覆盖执行链
  形状；真库 e2e → P10-01。
- 数据库 fork 解析差异（mysql_async zipg fork）：core Cargo.lock 与
  src-tauri 同 rev（4a9e60a62f5a48e35ef9ccc9fc8277d5012b19dd），root patch
  owner 同步。
