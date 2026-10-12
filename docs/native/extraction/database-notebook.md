# §5.1 extraction expansion — `database/` and `notebook/`

> Generated from the source files themselves, one row per file, as `MIGRATION_PLAN.md` §5.1 requires
> before either module may be extracted. Kept in `docs/native/` rather than only inside the Trellis
> child task because `.trellis/` is gitignored here — a plan artifact that exists on one machine is
> not a checklist. Companion: `docs/native/command-map.json`, `docs/native/reachability-triage.md`.
>
> **Corrected 2026-10-11 (P8-05 切片一百零三).** transfer／差异比较面 7 条＋连接生命周期 2 条＋legacy db_read_sql_file 1 条改宿主转发壳：主体上收 core `database::transfer_face`（8 条既有单测随迁 `transfer_face_tests`，test items 8→0）与 `dbx_connections::disconnect/backup_sqlite_database`、`legacy_sqlite::db_read_sql_file`；transfer.rs 剩 7 条壳＋窗口 sink（"emits events" 标签保留——进度仍经窗口 legacy 通道，v1 topic 半边被 Tauri 事件名校验拒绝，见 EventSink 注释）；transfer_command_tests.rs 新增入列（files 21→22）；register 标量按生成值（testOnlyItemsExcluded 37→29、pubSymbolsTotal 204→200）。
>
> **Corrected 2026-10-11 (P8-05 切片一百零二).** 导入导出面 10 条改宿主转发壳：主体＋Excel ZIP 炸弹校验层＋SQL 文件执行（BOM/编码/生产确认闸出口注入）上收 core `database::ie_face`；import_export.rs 剩 10 条薄壳（本地 Struct ExecuteSqlFileRequest/error_progress 等随主体迁走，"emits events" 标签移除——确认事件改经 ConfirmEventSink 注入出口，权威记录在 command-map.json event_dynamic_emit_sites）；7 条既有命令壳测试迁 core ie_face_tests（14→7 条本侧 exclude 减少）＋新增 import_export_command_tests.rs（files 20→21）；register 标量按生成值（testOnlyItemsExcluded 44→37、pubSymbolsTotal 204→202）。
>
> **Corrected 2026-10-11 (P8-05 切片一百零一).** mongo 面 6 条主体＋run_cancellable 上收 core `database::mongo_face`：mongo.rs 1 条单测随迁（test items 1→0），mongo.rs 只剩转发壳；register 标量按生成值（testOnlyItemsExcluded 45→44）。
>
> **Corrected 2026-10-11 (P8-05 切片一百).** redis 面 18 条主体＋适配器/归一化/补充安全闸上收 core `database::redis_face`：redis.rs 8 条单测随迁（test items 8→0），redis.rs 只剩转发壳（moved 相应减少）；register 标量按生成值（testOnlyItemsExcluded 53→45、pubSymbolsTotal 207→204）。
>
> **Corrected 2026-10-11 (P8-05 切片九十九).** rag_index／rag_context 主体上收 core `notebook::rag::face`：commands.rs 两符号迁走（moved 14→12；本地 ContextOptionsDto/ContextBundle 死代码删除，TestOptionsDto 测试半边随迁）；register 标量按生成值（pubSymbolsTotal 209→207）。commands.rs 剩 search/probe/cancel/clear/stats 等本地命令与转发壳。
>
> **Corrected 2026-10-11 (P8-05 切片九十八).** schema／驱动面 7 条改宿主转发壳：drivers.rs 的静态清单测试随实现迁 core `database::schema_face`（schema.rs 6 条本无测试）；register 标量按生成值（testOnlyItemsExcluded 54→53、pubSymbolsTotal 210→209）。schema.rs/drivers.rs 全部成为转发壳。
>
> **Corrected 2026-10-11 (P8-05 切片九十七).** 执行面 11 条改宿主转发壳＋确认回路主体上收 core `database::execution`：query.rs 全部 13 条测试随迁（test items 13→0）、grid.rs 2 条随迁（2→0），本地 assess/confirm/options/DTO 符号随主体迁走（moved 31→23／11→8）；register 标量按生成值（testOnlyItemsExcluded 69→54、pubSymbolsTotal 221→210）。执行/保存/评估的业务主体留在 src-tauri 的只剩转发壳与本地路径/密钥适配。
>
> **Corrected 2026-10-11 (P8-05 切片九十六).** SQL builders／grid 预览 20 条改宿主转发壳：query.rs 5 条 builder 测试随实现迁 core（test items 18→13）、grid.rs 3 条随迁（5→2），符号随业务留 Tauri 的减一格（moved 32→31／12→11）；register 标量按生成值（testOnlyItemsExcluded 77→69、pubSymbolsTotal 223→221）。
>
> **Corrected 2026-10-11 (P8-05 切片九十五).** dbx 五命令（list/save/delete/test/connect）改宿主转发壳、connection_command_tests.rs 新增入列（files 19→20）；connection_secrets.rs 主体（密钥读写）与 connections.rs 部分符号迁 core `database::dbx_*`，旧侧测试随之缩减（testOnlyItemsExcluded 100→77、pubSymbolsTotal 244→223，按 `extraction.mjs --check` 生成值）。
>
> **Corrected 2026-10-04 (P8-02 刀六).** P7-05 把 56 个 notebook 文件搬进 core 后本表只剩 src-tauri 侧 19 行/168 wrappers（database 7 文件未动）；上表四个机器核对标量按 `extraction.mjs --check` 生成值同步刷新（testOnlyItemsExcluded 782→110→100、pubSymbolsTotal 650→249→244（切片九十四：legacy_sqlite 业务体与测试迁 core）——搬走的 56 文件的测试与符号随业务进 core 门禁面）。数组字段（withAppHandle 等）仍是 2026-09-21 快照，见下方 2026-09-29 注记，per-row `hazards` 列仍为权威。
>
> **Corrected 2026-09-21 (round 11).** These rows were first produced by throwaway code that no longer existed, so nothing could re-derive them. They now come from `scripts/native/extraction.mjs`, and `--check` fails if they drift. Re-deriving exposed two wrong columns in this document: `testOnlyItemsExcluded` was 914 and `pubSymbolsTotal` 586, while the `#[test]` attributes actually present in these files total 782 (verified per file against `grep -c '#\[[a-z:]*test'`, e.g. connection_secrets.rs 18→10, sync/engine.rs 64→40) and column-0 `pub` items total 650 (the old count omitted `pub(crate)` items and every `mod.rs` surface). Rows/destinations/wrapper counts were already right — 168 still equals command-map.json's 168.
>
> Cross-checked: the 168 command wrappers counted here equal the 168 `database`/`notebook` entries in
> `command-map.json`, and the generator **aborts rather than writes** if those two ever disagree.
> `moved symbols` lists only `pub` items outside a `#[cfg(test)] mod …` block (skipped by brace depth,
> because test modules are not always at end of file); the parenthesised count is what was excluded.
>
> Planning data only. It authorises no code movement; P7-04 / P7-05 execute it row by row.

## §5.1 one-file-per-row extraction expansion

Generated 2026-09-21 from the files themselves (75 rows). Order of work is fixed by
`MIGRATION_PLAN.md` §5.1 step 4: pure DTO/helper → service → command wrapper → delete the duplicate body,
`cargo check` on **core and src-tauri** after every row. `commands` = number of `#[tauri::command]` items
whose body moves out while a same-named wrapper stays behind. Destination mirrors the relative path.

<!-- generated:start — `scripts/native/extraction.mjs` owns everything between the two markers; everything outside them is hand-written and survives `--write` -->
> **落点以这一段为准（2026-09-29 表态项 14 定为 ②）**：下表 `destination` 列是生成器按 `core/crates/aeroric-core/<模块>/<文件>` 机械推导的**镜像**——它只保证"每个源文件都有一条"，不承诺落点（全列还缺 `src/` 那一层）。一个宿主文件被拆成多个职责文件时"同名 1:1"本来就不存在，所以 `Action 给定的落点` 列只在 basename 相同才算命名，否则写 `unmapped`，**不做猜测**。真正授权搬迁的是所属 Task 的 Action 原文，逐字抄在下面（`--check` 连这几行一起比对）。
>
> **Task P7-04 — Action 原文（机械抽取）**
> 1. 在 core 新建 `database/` 对应同名模块，逐个搬移业务；原 command 同名保留为 adapter。DBX 初始化不是 AppHandle 的职责；降级内存库要有显式 diagnostics，不能悄悄当持久连接库。
> 2. 新建 `AeroricUI/Database/{DatabaseSidebar,ConnectionEditor,SchemaBrowser,SQLQueryEditor,ResultGrid,StructureEditor,ImportExportSheet,RedisBrowser,MongoBrowser,ProductionConfirmationSheet}.swift`。
> 3. SQL 执行沿用 production assessment/confirmation；一次批准绑定 operationId、目标与 SQL hash，不对后续所有 SQL 放行。
> 4. 结果分页/虚拟化；列类型/NULL/binary/大值明确，编辑使用原 DBX 条件保护，不拼 Swift SQL；query cancel/close result session 释放资源。
> 5. 连接 secret 走既有专用服务；debug log 不输出 connection string/password；导入导出使用用户选定路径、预览与写入确认。
>
> **Task P7-05 — Action 原文（机械抽取）**
> 1. 新建 core `notebook/` 并保留对应业务文件结构；提取的纯 Rust parser/tests 原样移动，去除 command 宏和 UI state 查找，不重写已经稳定的导入格式。
> 2. 新建 `AeroricUI/Notebook/{NotebookSidebar,NoteDocumentView,NoteMetadataEditor,BacklinksPanel,TagBrowser,NoteTaskInbox,TemplatePicker,AttachmentView,TrashView,ImportExportWizard}.swift`。
> 3. 与文件编辑器复用 document/Markdown 基础组件；noteId/path/attachment identity 保留，不依赖 display title 当主键。
> 4. 支持现有导入格式集合；zip 防 traversal/bomb、HTML sanitize、附件白名单、冲突重命名/中断报告沿用，不能调用系统打开未受信任脚本。
> 5. note delete/restore/template/task checkbox 等调用 core command；多窗口 expectedRevision 冲突提示，不静默覆盖笔记。
>

| # | source | destination（机械镜像，非落点） | commands | moved symbols (top-level public) | Action 给定的落点 | per-file hazards to verify |
|---|---|---|---|---|---|---|
| 1 | `src-tauri/src/database/connection_command_tests.rs` | `core/crates/aeroric-core/database/connection_command_tests.rs` | 0 | — (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | credential owner |
| 2 | `src-tauri/src/database/connection_secrets.rs` | `core/crates/aeroric-core/database/connection_secrets.rs` | 0 | Struct KeyringDbxConnectionSecretStore (1 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | include_str! path rewrite; credential owner |
| 3 | `src-tauri/src/database/connections.rs` | `core/crates/aeroric-core/database/connections.rs` | 7 | dbx_backup_sqlite_database(), dbx_connect(), dbx_delete_connection(), dbx_disconnect(), dbx_list_connections(), dbx_save_connection(), dbx_test_connection() (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; spawns task/thread (needs a cancellation token); credential owner |
| 4 | `src-tauri/src/database/dbx_state.rs` | `core/crates/aeroric-core/database/dbx_state.rs` | 0 | Struct DbxState (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | — |
| 5 | `src-tauri/src/database/drivers.rs` | `core/crates/aeroric-core/database/drivers.rs` | 1 | dbx_driver_manifest() (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State |
| 6 | `src-tauri/src/database/grid.rs` | `core/crates/aeroric-core/database/grid.rs` | 8 | dbx_build_data_grid_context_filter_condition(), dbx_build_data_grid_copy_insert_statement(), dbx_build_data_grid_copy_update_statements(), dbx_delete_rows(), dbx_insert_row(), dbx_preview_grid_sql(), dbx_query_table_data(), dbx_update_cell() (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; credential owner |
| 7 | `src-tauri/src/database/import_export.rs` | `core/crates/aeroric-core/database/import_export.rs` | 10 | dbx_execute_sql_file(), dbx_export_database(), dbx_export_table_csv(), dbx_export_table_insert_sql(), dbx_export_table_json(), dbx_export_table_markdown(), dbx_export_table_update_sql(), dbx_export_table_xlsx(), dbx_import_table_file(), dbx_preview_table_import_file() (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; credential owner |
| 8 | `src-tauri/src/database/import_export_command_tests.rs` | `core/crates/aeroric-core/database/import_export_command_tests.rs` | 0 | — (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | spawns task/thread (needs a cancellation token); credential owner |
| 9 | `src-tauri/src/database/legacy_sqlite.rs` | `core/crates/aeroric-core/database/legacy_sqlite.rs` | 9 | Enum DbEndpoint, Struct DbConnectionConfig, db_delete_row(), db_execute_sql(), db_insert_row(), db_inspect(), db_load_connections(), db_query_table(), db_read_sql_file(), db_save_connections(), db_update_cell(), local_db_delete_row(), local_db_execute_sql(), local_db_insert_row(), local_db_inspect() +4 more (4 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; cfg(unix) branch; spawns task/thread (needs a cancellation token); credential owner |
| 10 | `src-tauri/src/database/mod.rs` | `core/crates/aeroric-core/database/mod.rs` | 0 | Mod connection_secrets, Mod connections, Mod dbx_state, Mod drivers, Mod grid, Mod import_export, Mod legacy_sqlite, Mod mongo, Mod query, Mod redis, Mod schema, Mod transfer, Mod types (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | credential owner |
| 11 | `src-tauri/src/database/mongo.rs` | `core/crates/aeroric-core/database/mongo.rs` | 6 | dbx_mongo_delete_documents(), dbx_mongo_find_documents(), dbx_mongo_insert_document(), dbx_mongo_list_collections(), dbx_mongo_list_databases(), dbx_mongo_update_document() (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; credential owner |
| 12 | `src-tauri/src/database/query.rs` | `core/crates/aeroric-core/database/query.rs` | 23 | dbx_assess_production_sql(), dbx_assess_production_target(), dbx_build_create_database_sql(), dbx_build_create_schema_sql(), dbx_build_database_search_sql(), dbx_build_drop_database_sql(), dbx_build_drop_object_sql(), dbx_build_drop_schema_sql(), dbx_build_drop_table_child_object_sql(), dbx_build_drop_table_sql(), dbx_build_duckdb_attach_database_sql(), dbx_build_duplicate_table_structure_sql(), dbx_build_empty_table_sql(), dbx_build_rename_object_sql(), dbx_build_search_result_where() +8 more (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; emits events; credential owner |
| 13 | `src-tauri/src/database/redis.rs` | `core/crates/aeroric-core/database/redis.rs` | 18 | dbx_redis_create_key(), dbx_redis_delete_key(), dbx_redis_execute_command(), dbx_redis_get_value(), dbx_redis_hash_del(), dbx_redis_hash_set(), dbx_redis_list_databases(), dbx_redis_list_push(), dbx_redis_list_remove(), dbx_redis_list_set(), dbx_redis_load_more(), dbx_redis_scan_keys(), dbx_redis_set_add(), dbx_redis_set_remove(), dbx_redis_set_ttl() +3 more (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; credential owner |
| 14 | `src-tauri/src/database/schema.rs` | `core/crates/aeroric-core/database/schema.rs` | 6 | dbx_get_columns(), dbx_get_object_source(), dbx_get_table_ddl(), dbx_list_databases(), dbx_list_objects(), dbx_list_schemas() (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; credential owner |
| 15 | `src-tauri/src/database/transfer.rs` | `core/crates/aeroric-core/database/transfer.rs` | 7 | dbx_build_data_compare_sync_plan(), dbx_cancel_transfer(), dbx_generate_schema_sync_sql(), dbx_prepare_data_compare(), dbx_prepare_data_compare_from_tables(), dbx_prepare_schema_diff(), dbx_start_transfer() (1 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; credential owner |
| 16 | `src-tauri/src/database/transfer_command_tests.rs` | `core/crates/aeroric-core/database/transfer_command_tests.rs` | 0 | — (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | spawns task/thread (needs a cancellation token); credential owner |
| 17 | `src-tauri/src/database/types.rs` | `core/crates/aeroric-core/database/types.rs` | 0 | — (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | — |
| 18 | `src-tauri/src/notebook/mod.rs` | `core/crates/aeroric-core/notebook/mod.rs` | 62 | Mod rag, Mod sync, notebook_attachment_list(), notebook_attachment_read(), notebook_attachment_save(), notebook_attachment_save_from_path(), notebook_close_note(), notebook_convert_richtext(), notebook_create_folder(), notebook_create_note(), notebook_create_note_in_vault(), notebook_delete_note(), notebook_ensure_default_vault(), notebook_ensure_project_vault(), notebook_export_site_write() +49 more (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; spawns task/thread (needs a cancellation token) |
| 19 | `src-tauri/src/notebook/rag/commands.rs` | `core/crates/aeroric-core/notebook/rag/commands.rs` | 11 | Struct SearchOptionsDto, notebook_embedding_key_clear(), notebook_embedding_key_set(), notebook_embedding_key_status(), notebook_rag_cancel(), notebook_rag_clear(), notebook_rag_context(), notebook_rag_index(), notebook_rag_neighbors(), notebook_rag_probe(), notebook_rag_search(), notebook_rag_stats() (23 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | command-layer; AppHandle/State; emits events; spawns task/thread (needs a cancellation token); credential owner |
| 20 | `src-tauri/src/notebook/rag/mod.rs` | `core/crates/aeroric-core/notebook/rag/mod.rs` | 0 | Mod commands (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | credential owner |
| 21 | `src-tauri/src/notebook/sync/daemon.rs` | `core/crates/aeroric-core/notebook/sync/daemon.rs` | 0 | Struct SyncDaemonGuard, start() (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | AppHandle/State; emits events; spawns task/thread (needs a cancellation token) |
| 22 | `src-tauri/src/notebook/sync/mod.rs` | `core/crates/aeroric-core/notebook/sync/mod.rs` | 0 | Mod daemon (0 test items excluded) | unmapped（Action 未给同名文件，见上方 Action 原文） | — |
<!-- generated:end -->

### Aggregate risk register for this expansion

```json
{
  "files": 22,
  "commands": 168,
  "testOnlyItemsExcluded": 29,
  "pubSymbolsTotal": 200,
  "withAppHandle": [
    "src-tauri/src/database/connections.rs",
    "src-tauri/src/database/grid.rs",
    "src-tauri/src/database/import_export.rs",
    "src-tauri/src/database/mongo.rs",
    "src-tauri/src/database/query.rs",
    "src-tauri/src/database/redis.rs",
    "src-tauri/src/database/schema.rs",
    "src-tauri/src/database/transfer.rs",
    "src-tauri/src/notebook/mod.rs",
    "src-tauri/src/notebook/rag/commands.rs",
    "src-tauri/src/notebook/rag/index.rs",
    "src-tauri/src/notebook/sync/daemon.rs"
  ],
  "withIncludeStr": [
    "src-tauri/src/database/connection_secrets.rs",
    "src-tauri/src/database/drivers.rs"
  ],
  "withCfgWindows": [
    "src-tauri/src/notebook/sync/daemon.rs"
  ],
  "withSpawn": [
    "src-tauri/src/database/transfer.rs",
    "src-tauri/src/notebook/rag/search.rs",
    "src-tauri/src/notebook/sync/daemon.rs"
  ],
  "withSecrets": [
    "src-tauri/src/database/connection_secrets.rs",
    "src-tauri/src/database/connections.rs",
    "src-tauri/src/database/legacy_sqlite.rs",
    "src-tauri/src/notebook/rag/commands.rs"
  ],
  "withInFileTests": [
    "src-tauri/src/database/connection_secrets.rs",
    "src-tauri/src/database/connections.rs",
    "src-tauri/src/database/drivers.rs",
    "src-tauri/src/database/grid.rs",
    "src-tauri/src/database/import_export.rs",
    "src-tauri/src/database/legacy_sqlite.rs",
    "src-tauri/src/database/mongo.rs",
    "src-tauri/src/database/query.rs",
    "src-tauri/src/database/redis.rs",
    "src-tauri/src/database/transfer.rs",
    "src-tauri/src/notebook/export.rs",
    "src-tauri/src/notebook/fields.rs",
    "src-tauri/src/notebook/html2md.rs",
    "src-tauri/src/notebook/import/guards.rs",
    "src-tauri/src/notebook/import/landing.rs",
    "src-tauri/src/notebook/import/manifest.rs",
    "src-tauri/src/notebook/import/report.rs",
    "src-tauri/src/notebook/import/run.rs",
    "src-tauri/src/notebook/links.rs",
    "src-tauri/src/notebook/mentions.rs",
    "src-tauri/src/notebook/rag/chunk.rs",
    "src-tauri/src/notebook/rag/cjk.rs",
    "src-tauri/src/notebook/rag/commands.rs",
    "src-tauri/src/notebook/rag/context.rs",
    "src-tauri/src/notebook/rag/db.rs",
    "src-tauri/src/notebook/rag/embed.rs",
    "src-tauri/src/notebook/rag/graph.rs",
    "src-tauri/src/notebook/rag/index.rs",
    "src-tauri/src/notebook/rag/rerank.rs",
    "src-tauri/src/notebook/rag/search.rs",
    "src-tauri/src/notebook/sync/daemon.rs",
    "src-tauri/src/notebook/sync/diff.rs",
    "src-tauri/src/notebook/sync/engine.rs",
    "src-tauri/src/notebook/sync/git.rs",
    "src-tauri/src/notebook/sync/local.rs",
    "src-tauri/src/notebook/sync/manifest.rs",
    "src-tauri/src/notebook/sync/remote.rs",
    "src-tauri/src/notebook/sync/scan.rs",
    "src-tauri/src/notebook/sync/schedule.rs",
    "src-tauri/src/notebook/sync/store.rs",
    "src-tauri/src/notebook/tag_rename.rs",
    "src-tauri/src/notebook/tags.rs",
    "src-tauri/src/notebook/tasks.rs",
    "src-tauri/src/notebook/user_templates.rs",
    "src-tauri/src/notebook/vault_walk.rs"
  ],
  "zeroPublic": [
    "src-tauri/src/database/mod.rs",
    "src-tauri/src/database/types.rs",
    "src-tauri/src/notebook/import/apple_tests.rs",
    "src-tauri/src/notebook/import/dir_tests.rs",
    "src-tauri/src/notebook/import/enex_tests.rs",
    "src-tauri/src/notebook/import/mod.rs",
    "src-tauri/src/notebook/import/zip_tests.rs",
    "src-tauri/src/notebook/rag/mod.rs",
    "src-tauri/src/notebook/sync/mod.rs",
    "src-tauri/src/notebook/tests.rs"
  ]
}
```

Notes that change how the work must be sequenced:

- `notebook/mod.rs` is the command layer (130 coupling hits): its body must be split by sub-concern before
  the wrapper-retention step, otherwise one file becomes the destination for everything and §5.1 is void.
- `database/*` concentrate `AppHandle`/`State` in `redis.rs`, `query.rs`, `import_export.rs`, `transfer.rs`,
  `connections.rs`, `schema.rs`, `mongo.rs`, `grid.rs`, `legacy_sqlite.rs` — the DBX state owner must become a
  core-side resource with explicit init/drop, not a Tauri-managed `State<...>` (P7-04 Action 1).
- Any `cfg(windows)` and `cfg(unix)` row must keep compiling for Windows/Linux after the move (INV-10), so
  P7-02's added CI matrix is a **precondition** for closing these rows, not an optional extra.
- Rows marked `credential owner` must keep the existing secret service as the only reader; nothing may
  serialise a connection string into a snapshot, log or review evidence (P7-04 Action 5).
- `spawns task/thread` rows need a cancellation token in core and must not be restarted by a UI lifecycle event.
- **Which parts of the JSON register above are machine-checked (measured 2026-09-29).** `extraction.mjs --check` verifies exactly four scalars of that block — `files`, `commands`, `testOnlyItemsExcluded`, `pubSymbolsTotal` — and nothing else, because the arrays were cut by a 2026-09-21 hand rule the generator does not reproduce. Comparing them against today's hazard classifier shows three are already stale: `withSpawn` lists **3** files where the table now marks **7**, `withSecrets` lists **4** where the classifier finds **21**, `zeroPublic` lists **10** where the splitter now yields **5** (the other keys match). So read the per-row `hazards` column as the authority and that block as a 2026-09-21 snapshot whose array fields nobody re-derived. Two ways out, neither taken here: teach the generator to emit and verify the array form (needs the original definitions, which are not recorded), or delete the arrays and keep only the verified scalars. Same shape as `G26`'s "five of the nine §5.1 artifacts have no freshness gate", except here the guarded document carries an unguarded half.
