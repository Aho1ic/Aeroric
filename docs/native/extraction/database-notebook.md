# §5.1 extraction expansion — `database/` and `notebook/`

> Generated from the source files themselves, one row per file, as `MIGRATION_PLAN.md` §5.1 requires
> before either module may be extracted. Kept in `docs/native/` rather than only inside the Trellis
> child task because `.trellis/` is gitignored here — a plan artifact that exists on one machine is
> not a checklist. Companion: `docs/native/command-map.json`, `docs/native/reachability-triage.md`.
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

| # | source | destination | commands | moved symbols (top-level public) | per-file hazards to verify |
|---|---|---|---|---|---|
| 1 | `src-tauri/src/database/connection_secrets.rs` | `core/crates/aeroric-core/database/connection_secrets.rs` | 0 | Mod test_support, Struct KeyringDbxConnectionSecretStore, Trait DbxConnectionSecretStore, apply_secrets_to_dbx(), blank_secrets_in_dbx(), delete_connection_secrets(), extract_secrets_from_dbx(), fill_blank_secrets_in_dbx(), hydrate_blank_secrets(), hydrate_connection_on_load(), hydrate_connections_on_load(), is_sensitive_key(), is_sensitive_url_parameter(), prepare_connection_for_disk(), prepare_connections_for_disk() +2 more (10 test items excluded) | include_str! path rewrite; credential owner |
| 2 | `src-tauri/src/database/connections.rs` | `core/crates/aeroric-core/database/connections.rs` | 7 | dbx_backup_sqlite_database(), dbx_connect(), dbx_delete_connection(), dbx_disconnect(), dbx_list_connections(), dbx_save_connection(), dbx_test_connection(), ensure_connected(), ensure_loaded(), ensure_writable(), parse_core_config() (14 test items excluded) | command-layer; AppHandle/State; spawns task/thread (needs a cancellation token); credential owner |
| 3 | `src-tauri/src/database/dbx_state.rs` | `core/crates/aeroric-core/database/dbx_state.rs` | 0 | Struct DbxState (0 test items excluded) | — |
| 4 | `src-tauri/src/database/drivers.rs` | `core/crates/aeroric-core/database/drivers.rs` | 1 | dbx_driver_manifest(), driver_manifest_value() (1 test items excluded) | command-layer; include_str! path rewrite |
| 5 | `src-tauri/src/database/grid.rs` | `core/crates/aeroric-core/database/grid.rs` | 8 | Struct GridSaveRequest, Struct SqlPreviewResponse, Struct TableDataRequest, Struct TableDataResponse, dbx_build_data_grid_context_filter_condition(), dbx_build_data_grid_copy_insert_statement(), dbx_build_data_grid_copy_update_statements(), dbx_delete_rows(), dbx_insert_row(), dbx_preview_grid_sql(), dbx_query_table_data(), dbx_update_cell() (5 test items excluded) | command-layer; AppHandle/State |
| 6 | `src-tauri/src/database/import_export.rs` | `core/crates/aeroric-core/database/import_export.rs` | 10 | Struct ExecuteSqlFileRequest, dbx_execute_sql_file(), dbx_export_database(), dbx_export_table_csv(), dbx_export_table_insert_sql(), dbx_export_table_json(), dbx_export_table_markdown(), dbx_export_table_update_sql(), dbx_export_table_xlsx(), dbx_import_table_file(), dbx_preview_table_import_file(), error_progress() (7 test items excluded) | command-layer; AppHandle/State; credential owner |
| 7 | `src-tauri/src/database/legacy_sqlite.rs` | `core/crates/aeroric-core/database/legacy_sqlite.rs` | 9 | Enum DbEndpoint, Struct DbCellValue, Struct DbColumn, Struct DbConnectionConfig, Struct DbExecuteResult, Struct DbForeignKey, Struct DbIndex, Struct DbKeyValue, Struct DbObject, Struct DbQueryResult, Struct DbRow, Struct DbRowKey, Struct DbSchema, Struct DbTrigger, db_delete_row() +9 more (14 test items excluded) | command-layer; cfg(unix) branch; spawns task/thread (needs a cancellation token); credential owner |
| 8 | `src-tauri/src/database/mod.rs` | `core/crates/aeroric-core/database/mod.rs` | 0 | Mod connection_secrets, Mod connections, Mod dbx_state, Mod drivers, Mod grid, Mod import_export, Mod legacy_sqlite, Mod mongo, Mod query, Mod redis, Mod schema, Mod transfer, Mod types (0 test items excluded) | credential owner |
| 9 | `src-tauri/src/database/mongo.rs` | `core/crates/aeroric-core/database/mongo.rs` | 6 | dbx_mongo_delete_documents(), dbx_mongo_find_documents(), dbx_mongo_insert_document(), dbx_mongo_list_collections(), dbx_mongo_list_databases(), dbx_mongo_update_document() (1 test items excluded) | command-layer; AppHandle/State; credential owner |
| 10 | `src-tauri/src/database/query.rs` | `core/crates/aeroric-core/database/query.rs` | 23 | Struct AssessProductionSqlRequest, Struct AssessProductionTargetRequest, Struct DuckDbAttachDatabaseSqlOptions, Struct ExecuteMultiRequest, Struct ExecuteQueryRequest, Struct ProductionSqlAssessment, Struct ProductionTargetAssessment, dbx_assess_production_sql(), dbx_assess_production_target(), dbx_build_create_database_sql(), dbx_build_create_schema_sql(), dbx_build_database_search_sql(), dbx_build_drop_database_sql(), dbx_build_drop_object_sql(), dbx_build_drop_schema_sql() +17 more (18 test items excluded) | command-layer; AppHandle/State; emits events; credential owner |
| 11 | `src-tauri/src/database/redis.rs` | `core/crates/aeroric-core/database/redis.rs` | 18 | Enum AeroricRedisCollectionPage, Struct AeroricRedisValue, Struct RedisCreateKeyRequest, dbx_redis_create_key(), dbx_redis_delete_key(), dbx_redis_execute_command(), dbx_redis_get_value(), dbx_redis_hash_del(), dbx_redis_hash_set(), dbx_redis_list_databases(), dbx_redis_list_push(), dbx_redis_list_remove(), dbx_redis_list_set(), dbx_redis_load_more(), dbx_redis_scan_keys() +6 more (8 test items excluded) | command-layer; AppHandle/State; credential owner |
| 12 | `src-tauri/src/database/schema.rs` | `core/crates/aeroric-core/database/schema.rs` | 6 | dbx_get_columns(), dbx_get_object_source(), dbx_get_table_ddl(), dbx_list_databases(), dbx_list_objects(), dbx_list_schemas() (0 test items excluded) | command-layer; AppHandle/State |
| 13 | `src-tauri/src/database/transfer.rs` | `core/crates/aeroric-core/database/transfer.rs` | 7 | dbx_build_data_compare_sync_plan(), dbx_cancel_transfer(), dbx_generate_schema_sync_sql(), dbx_prepare_data_compare(), dbx_prepare_data_compare_from_tables(), dbx_prepare_schema_diff(), dbx_start_transfer() (9 test items excluded) | command-layer; AppHandle/State; emits events; spawns task/thread (needs a cancellation token) |
| 14 | `src-tauri/src/database/types.rs` | `core/crates/aeroric-core/database/types.rs` | 0 | Enum DbxDatabaseType, Struct AeroricDbConnectionConfig, Struct ProjectScope (0 test items excluded) | — |
| 15 | `src-tauri/src/notebook/attachments.rs` | `core/crates/aeroric-core/notebook/attachments.rs` | 0 | Const ATTACHMENT_DIR, Const DEFAULT_LIST_LIMIT, Struct Attachment, Struct SavedAttachment, dir(), kind_of(), list(), read(), save_bytes(), save_from_path(), write_claimed() (0 test items excluded) | — |
| 16 | `src-tauri/src/notebook/export.rs` | `core/crates/aeroric-core/notebook/export.rs` | 0 | export_roots(), validate_export_dest(), validate_site_dest(), write_export(), write_site_page() (16 test items excluded) | cfg(unix) branch |
| 17 | `src-tauri/src/notebook/fields.rs` | `core/crates/aeroric-core/notebook/fields.rs` | 0 | Struct NoteField, Struct NoteFieldSource, scan_vault_fields() (17 test items excluded) | — |
| 18 | `src-tauri/src/notebook/fs_ops.rs` | `core/crates/aeroric-core/notebook/fs_ops.rs` | 0 | Const SYNC_PRIVATE_DIR, Const VAULT_PRIVATE_DIR, Const fn, Enum SaveOutcome, Struct NoteEntry, Struct OpenedNote, atomic_write(), atomic_write_bytes(), create_folder(), create_note(), is_note_file(), is_scan_skip_dir(), private_dir(), read_icons(), read_note() +8 more (0 test items excluded) | — |
| 19 | `src-tauri/src/notebook/html2md.rs` | `core/crates/aeroric-core/notebook/html2md.rs` | 0 | html_to_markdown() (25 test items excluded) | credential owner |
| 20 | `src-tauri/src/notebook/import/apple_notes.rs` | `core/crates/aeroric-core/notebook/import/apple_notes.rs` | 0 | Const FIELD, Const FIELD_ESCAPE, Const PROVIDER, Const SENTINEL, Const SENTINEL_ESCAPE, apple_script(), import(), import_from_output() (0 test items excluded) | — |
| 21 | `src-tauri/src/notebook/import/apple_tests.rs` | `core/crates/aeroric-core/notebook/import/apple_tests.rs` | 0 | — (0 test items excluded) | — |
| 22 | `src-tauri/src/notebook/import/bear.rs` | `core/crates/aeroric-core/notebook/import/bear.rs` | 0 | Const PROVIDER, import() (0 test items excluded) | — |
| 23 | `src-tauri/src/notebook/import/dir_tests.rs` | `core/crates/aeroric-core/notebook/import/dir_tests.rs` | 0 | — (0 test items excluded) | cfg(unix) branch |
| 24 | `src-tauri/src/notebook/import/enex_tests.rs` | `core/crates/aeroric-core/notebook/import/enex_tests.rs` | 0 | — (0 test items excluded) | — |
| 25 | `src-tauri/src/notebook/import/evernote.rs` | `core/crates/aeroric-core/notebook/import/evernote.rs` | 0 | Const PROVIDER, import() (0 test items excluded) | credential owner |
| 26 | `src-tauri/src/notebook/import/guards.rs` | `core/crates/aeroric-core/notebook/import/guards.rs` | 0 | Const MAX_DEPTH, Const MAX_ENTRIES, Const MAX_ENTRY_BYTES, Const MAX_TOTAL_BYTES, Enum LimitHit, Struct Budget, entry_too_large(), read_zip_entry_limited() (9 test items excluded) | — |
| 27 | `src-tauri/src/notebook/import/landing.rs` | `core/crates/aeroric-core/notebook/import/landing.rs` | 0 | Const IMPORT_DIR, private_dir_name(), provider_dir(), report_name(), resolve_in_vault(), sanitize_name(), sanitize_relative(), unique_path() (18 test items excluded) | — |
| 28 | `src-tauri/src/notebook/import/logseq.rs` | `core/crates/aeroric-core/notebook/import/logseq.rs` | 0 | Const PROVIDER, import() (0 test items excluded) | — |
| 29 | `src-tauri/src/notebook/import/manifest.rs` | `core/crates/aeroric-core/notebook/import/manifest.rs` | 0 | Struct Session, fingerprint() (9 test items excluded) | — |
| 30 | `src-tauri/src/notebook/import/mod.rs` | `core/crates/aeroric-core/notebook/import/mod.rs` | 0 | Mod apple_notes, Mod bear, Mod evernote, Mod guards, Mod landing, Mod logseq, Mod manifest, Mod notion, Mod obsidian, Mod report, Mod roam, Mod run, Mod walk, Mod zip_common, Mod zip_src (0 test items excluded) | — |
| 31 | `src-tauri/src/notebook/import/notion.rs` | `core/crates/aeroric-core/notebook/import/notion.rs` | 0 | Const PROVIDER, import() (0 test items excluded) | — |
| 32 | `src-tauri/src/notebook/import/obsidian.rs` | `core/crates/aeroric-core/notebook/import/obsidian.rs` | 0 | Const PROVIDER, import() (0 test items excluded) | — |
| 33 | `src-tauri/src/notebook/import/report.rs` | `core/crates/aeroric-core/notebook/import/report.rs` | 0 | Enum ItemIssue, Enum ItemStatus, Enum SkipReason, Struct ImportItem, Struct ImportReport (13 test items excluded) | — |
| 34 | `src-tauri/src/notebook/import/roam.rs` | `core/crates/aeroric-core/notebook/import/roam.rs` | 0 | Const PROVIDER, import() (0 test items excluded) | — |
| 35 | `src-tauri/src/notebook/import/run.rs` | `core/crates/aeroric-core/notebook/import/run.rs` | 0 | Type LandFn, run() (6 test items excluded) | — |
| 36 | `src-tauri/src/notebook/import/walk.rs` | `core/crates/aeroric-core/notebook/import/walk.rs` | 0 | Enum Take, Struct WalkCtx, copy_dir() (0 test items excluded) | — |
| 37 | `src-tauri/src/notebook/import/zip_common.rs` | `core/crates/aeroric-core/notebook/import/zip_common.rs` | 0 | dest_relative(), land_bytes(), land_verbatim() (0 test items excluded) | — |
| 38 | `src-tauri/src/notebook/import/zip_src.rs` | `core/crates/aeroric-core/notebook/import/zip_src.rs` | 0 | Enum Handled, Type HandleFn, walk_archive() (0 test items excluded) | — |
| 39 | `src-tauri/src/notebook/import/zip_tests.rs` | `core/crates/aeroric-core/notebook/import/zip_tests.rs` | 0 | — (0 test items excluded) | — |
| 40 | `src-tauri/src/notebook/links.rs` | `core/crates/aeroric-core/notebook/links.rs` | 0 | Struct LineHit, Struct NoteLinkRef, Struct NoteLinkSource, scan_line(), scan_vault_links() (4 test items excluded) | — |
| 41 | `src-tauri/src/notebook/mentions.rs` | `core/crates/aeroric-core/notebook/mentions.rs` | 0 | Enum MentionConfidence, Enum MentionSkipReason, Struct MentionHit, Struct MentionLinkChange, Struct MentionLinkFailure, Struct MentionLinkReport, Struct MentionLinkSkip, Struct MentionSource, Struct MentionTarget, link_mentions(), scan_vault_mentions() (33 test items excluded) | — |
| 42 | `src-tauri/src/notebook/migrate.rs` | `core/crates/aeroric-core/notebook/migrate.rs` | 0 | Struct ConvertedNote, Struct LegacyNote, Struct MigratedNote, Struct MigrationReport, Struct RichtextConversionReport, allocate_note_path(), convert_richtext_notes(), default_vault_path(), ensure_default_vault(), ensure_project_vault(), migrate_legacy_notes(), project_vault_path(), slugify() (0 test items excluded) | — |
| 43 | `src-tauri/src/notebook/mod.rs` | `core/crates/aeroric-core/notebook/mod.rs` | 62 | Mod attachments, Mod export, Mod fields, Mod fs_ops, Mod html2md, Mod import, Mod links, Mod mentions, Mod migrate, Mod rag, Mod snapshots, Mod state, Mod sync, Mod tag_rename, Mod tags +70 more (0 test items excluded) | command-layer; AppHandle/State; spawns task/thread (needs a cancellation token) |
| 44 | `src-tauri/src/notebook/rag/chunk.rs` | `core/crates/aeroric-core/notebook/rag/chunk.rs` | 0 | Const MAX_CHARS, Const OVERLAP_CHARS, Struct Chunk, estimate_tokens(), split() (23 test items excluded) | credential owner |
| 45 | `src-tauri/src/notebook/rag/cjk.rs` | `core/crates/aeroric-core/notebook/rag/cjk.rs` | 0 | Const FTS_TOKENIZER, is_cjk(), match_expression(), segment() (12 test items excluded) | credential owner |
| 46 | `src-tauri/src/notebook/rag/commands.rs` | `core/crates/aeroric-core/notebook/rag/commands.rs` | 11 | Struct ContextBundle, Struct ContextOptionsDto, Struct SearchOptionsDto, notebook_embedding_key_clear(), notebook_embedding_key_set(), notebook_embedding_key_status(), notebook_rag_cancel(), notebook_rag_clear(), notebook_rag_context(), notebook_rag_index(), notebook_rag_neighbors(), notebook_rag_probe(), notebook_rag_search(), notebook_rag_stats() (23 test items excluded) | command-layer; AppHandle/State; emits events; spawns task/thread (needs a cancellation token); credential owner |
| 47 | `src-tauri/src/notebook/rag/context.rs` | `core/crates/aeroric-core/notebook/rag/context.rs` | 0 | Const DEFAULT_CURRENT_CHARS, Const DEFAULT_MAX_TOKENS, Struct Assembled, Struct Citation, Struct ContextOptions, Struct CurrentNote, assemble() (20 test items excluded) | credential owner |
| 48 | `src-tauri/src/notebook/rag/db.rs` | `core/crates/aeroric-core/notebook/rag/db.rs` | 0 | Enum DocStatus, db_path(), delete_doc_payload(), embed_model(), mark_all_stale(), open(), open_existing(), peek_dim(), read_meta(), rebuild_vec_table(), set_embed_model(), write_meta() (23 test items excluded) | credential owner |
| 49 | `src-tauri/src/notebook/rag/embed.rs` | `core/crates/aeroric-core/notebook/rag/embed.rs` | 0 | Enum EmbedError, Enum EmbedProvider, Struct EmbedConfig, batches(), classify_status(), client(), embed_batch(), endpoint_for(), network_error_hint(), normalize(), parse_response(), probe_dimension(), request_body(), truncate_detail(), validate_base_url() (38 test items excluded) | credential owner |
| 50 | `src-tauri/src/notebook/rag/graph.rs` | `core/crates/aeroric-core/notebook/rag/graph.rs` | 0 | Enum Via, Struct DocRef, Struct LinkIndex, Struct Resolved, backlinks(), forward_targets(), load_docs(), neighbors(), normalize_target(), stem_of() (22 test items excluded) | — |
| 51 | `src-tauri/src/notebook/rag/index.rs` | `core/crates/aeroric-core/notebook/rag/index.rs` | 0 | Enum IndexPhase, Enum IndexScope, Struct CancelToken, Struct IndexFailure, Struct IndexOutcome, Struct IndexProgress, Struct IndexStats, Struct NoProgress, Trait ProgressSink, clear(), index_vault(), stats(), title_of() (28 test items excluded) | AppHandle/State; credential owner |
| 52 | `src-tauri/src/notebook/rag/mod.rs` | `core/crates/aeroric-core/notebook/rag/mod.rs` | 0 | Mod chunk, Mod cjk, Mod commands, Mod context, Mod db, Mod embed, Mod graph, Mod index, Mod rerank, Mod search (0 test items excluded) | credential owner |
| 53 | `src-tauri/src/notebook/rag/rerank.rs` | `core/crates/aeroric-core/notebook/rag/rerank.rs` | 0 | Const MAX_DOCUMENTS, Struct Ranked, Struct RerankConfig, endpoint_for(), parse_response(), rerank() (16 test items excluded) | — |
| 54 | `src-tauri/src/notebook/rag/search.rs` | `core/crates/aeroric-core/notebook/rag/search.rs` | 0 | Struct Candidate, Struct Degraded, Struct SearchHit, Struct SearchOptions, Struct SearchOutcome, Struct Sources, Struct Span, cap_per_doc(), find_spans(), fuse(), query_terms(), search(), to_source_spans() (45 test items excluded) | spawns task/thread (needs a cancellation token) |
| 55 | `src-tauri/src/notebook/snapshots.rs` | `core/crates/aeroric-core/notebook/snapshots.rs` | 0 | Const NOTE_LAYOUT, Struct RestoredNote, discard(), list(), read(), record_before_save(), restore() (0 test items excluded) | — |
| 56 | `src-tauri/src/notebook/state.rs` | `core/crates/aeroric-core/notebook/state.rs` | 0 | Struct FileSig, Struct Inner, Struct NotebookState, hash64(), resolve_within(), signature_for(), signature_for_bytes() (0 test items excluded) | — |
| 57 | `src-tauri/src/notebook/sync/daemon.rs` | `core/crates/aeroric-core/notebook/sync/daemon.rs` | 0 | Const SYNC_EVENT, Struct RemoteStatus, classify(), event_is_in_scope(), forget_pending(), note_local_change(), start(), status_for() (18 test items excluded) | AppHandle/State; emits events; cfg(windows) branch; spawns task/thread (needs a cancellation token) |
| 58 | `src-tauri/src/notebook/sync/diff.rs` | `core/crates/aeroric-core/notebook/sync/diff.rs` | 0 | Enum Action, Enum ConflictStrategy, Enum Resolution, Struct DiffOpts, Struct PlanSummary, Struct PlannedAction, Struct RemoteEntry, Struct SyncPlan, pending_tombstones(), plan() (35 test items excluded) | — |
| 59 | `src-tauri/src/notebook/sync/engine.rs` | `core/crates/aeroric-core/notebook/sync/engine.rs` | 0 | Enum OutcomeStatus, Struct ActionOutcome, Struct SyncReport, Trait LocalFs, Trait RemoteFs, run() (40 test items excluded) | cfg(unix) branch |
| 60 | `src-tauri/src/notebook/sync/git.rs` | `core/crates/aeroric-core/notebook/sync/git.rs` | 0 | Enum GitSyncStage, Struct GitSyncReport, run() (20 test items excluded) | — |
| 61 | `src-tauri/src/notebook/sync/local.rs` | `core/crates/aeroric-core/notebook/sync/local.rs` | 0 | Struct VaultLocalFs (11 test items excluded) | cfg(unix) branch |
| 62 | `src-tauri/src/notebook/sync/manifest.rs` | `core/crates/aeroric-core/notebook/sync/manifest.rs` | 0 | Const FORMAT_VERSION, Const MANIFEST_DIR, Const MANIFEST_NAME, Const MANIFEST_TMP_NAME, Struct Manifest, Struct ManifestEntry, is_manifest_path() (9 test items excluded) | — |
| 63 | `src-tauri/src/notebook/sync/mod.rs` | `core/crates/aeroric-core/notebook/sync/mod.rs` | 0 | Mod daemon, Mod diff, Mod engine, Mod git, Mod local, Mod manifest, Mod remote, Mod scan, Mod schedule, Mod store (0 test items excluded) | — |
| 64 | `src-tauri/src/notebook/sync/remote.rs` | `core/crates/aeroric-core/notebook/sync/remote.rs` | 0 | Struct StorageRemote (37 test items excluded) | credential owner |
| 65 | `src-tauri/src/notebook/sync/scan.rs` | `core/crates/aeroric-core/notebook/sync/scan.rs` | 0 | Const MAX_DEPTH, Const MAX_FILES, Const MAX_HASH_BYTES, Const OVERSIZE_PREFIX, Struct FileSig, by_path(), is_out_of_scope(), resolve_rel(), scan_vault(), signature_at() (21 test items excluded) | cfg(unix) branch |
| 66 | `src-tauri/src/notebook/sync/schedule.rs` | `core/crates/aeroric-core/notebook/sync/schedule.rs` | 0 | Const BACKOFF_BASE, Const BACKOFF_MAX, Const DEBOUNCE, Const FALLBACK, Const MAX_DEBOUNCE_WAIT, Enum Decision, Enum RunResult, Struct PendingChanges, Struct RemoteRuntime, backoff_delay(), decide() (22 test items excluded) | — |
| 67 | `src-tauri/src/notebook/sync/store.rs` | `core/crates/aeroric-core/notebook/sync/store.rs` | 0 | Const TOMBSTONE_TTL_MS, Struct Baseline, Struct RemoteTarget, Struct StoredResolution, Struct Tombstone, add_tombstone(), baselines(), bump_seq(), clear_baseline(), clear_resolution(), db_path(), device_id(), get_remote(), list_remotes(), live_tombstones() +9 more (38 test items excluded) | — |
| 68 | `src-tauri/src/notebook/tag_rename.rs` | `core/crates/aeroric-core/notebook/tag_rename.rs` | 0 | Enum TagSkipReason, Struct TagRenameChange, Struct TagRenameFailure, Struct TagRenameReport, Struct TagRenameSkip, rename_vault_tag(), write_rewritten() (9 test items excluded) | — |
| 69 | `src-tauri/src/notebook/tags.rs` | `core/crates/aeroric-core/notebook/tags.rs` | 0 | Struct NoteTagRef, Struct NoteTagSource, Struct TagHit, normalize_key(), scan_vault_tags(), tag_hits(), validate_tag() (11 test items excluded) | — |
| 70 | `src-tauri/src/notebook/tasks.rs` | `core/crates/aeroric-core/notebook/tasks.rs` | 0 | Struct NoteTaskRef, Struct NoteTaskSource, scan_vault_tasks() (15 test items excluded) | credential owner |
| 71 | `src-tauri/src/notebook/tests.rs` | `core/crates/aeroric-core/notebook/tests.rs` | 0 | — (0 test items excluded) | cfg(unix) branch; credential owner |
| 72 | `src-tauri/src/notebook/trash.rs` | `core/crates/aeroric-core/notebook/trash.rs` | 0 | Struct RestoredItem, Struct TrashItem, list(), purge(), purge_all(), restore(), trash(), trash_at(), trash_dir() (0 test items excluded) | — |
| 73 | `src-tauri/src/notebook/user_templates.rs` | `core/crates/aeroric-core/notebook/user_templates.rs` | 0 | Struct UserTemplate, list_user_templates(), parse_template() (8 test items excluded) | — |
| 74 | `src-tauri/src/notebook/vault_index.rs` | `core/crates/aeroric-core/notebook/vault_index.rs` | 0 | Struct VaultIndexEntry, scan_vault_titles(), split_frontmatter(), unquote_scalar() (0 test items excluded) | — |
| 75 | `src-tauri/src/notebook/vault_walk.rs` | `core/crates/aeroric-core/notebook/vault_walk.rs` | 0 | Const MAX_DEPTH, Const MAX_FILES, Const MAX_FILE_BYTES, Enum WalkNext, fence_marker(), frontmatter_lines(), inline_code_spans(), line_spans(), preview_line(), walk_notes() (1 test items excluded) | — |

### Aggregate risk register for this expansion

```json
{
  "files": 75,
  "commands": 168,
  "testOnlyItemsExcluded": 782,
  "pubSymbolsTotal": 650,
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
