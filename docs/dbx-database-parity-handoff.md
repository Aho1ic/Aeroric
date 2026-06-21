# DBX Database Parity Handoff

Last updated: 2026-06-19

## Overall Goal

Continue implementing `/Users/macbook/Downloads/同步空间/LYX/Aeroric/docs/dbx-database-parity-prompt.md`.

The requested end state is broad: make Aeroric's database module match the dbx database module in behavior, entry points, tree browsing, context menus, connection dialog, data grid, import/export, search, ER diagram, Redis/Mongo browsing, styles, i18n, and tests. This goal is not complete.

Important constraints:

- Use Aeroric React/hooks/Tauri patterns, not dbx Vue/Pinia architecture.
- Do not revert unrelated dirty worktree changes.
- High-risk database mutations must preview generated SQL and ask for confirmation.
- Expected test noise: `--localstorage-file` warnings and jsdom canvas `getContext()` warnings.
- Expected build warning: chunks larger than 500 kB; build can still be successful.

## Current Worktree State

Known dirty files at handoff:

- `src-tauri/src/database/connections.rs`
- `src-tauri/src/database/query.rs`
- `src-tauri/src/database/schema.rs`
- `src-tauri/src/database/types.rs`
- `src-tauri/Cargo.toml`
- `src-tauri/src/lib.rs`
- `src/components/database/DatabaseView.tsx`
- `src/components/database/MongoBrowser.tsx`
- `src/components/database/RedisBrowser.tsx`
- `src/components/database/DatabaseUserAdminPanel.tsx`
- `src/hooks/useMongoBrowser.ts`
- `src/hooks/useRedisBrowser.ts`
- `src/i18n.tsx`
- `src/lib/redisCommandSession.ts`
- `src/lib/redisCommandSafety.ts`
- `src/lib/redisKeyPattern.ts`
- `src/lib/redisKeyTree.ts`
- `src/lib/databaseApi.ts`
- `src/styles/database.ts`
- `src/test/database-api.test.ts`
- `src/test/database-mongo-browser.test.tsx`
- `src/test/database-redis-browser.test.tsx`
- `src/test/database-view.test.tsx`
- `src/types.ts`
- `src/types/database.ts`
- Untracked: `src/components/database/DatabaseSearchPanel.tsx`
- Untracked: `src/components/database/DatabaseSidebarTree.tsx`
- Untracked: `src/test/database-sidebar-tree.test.tsx`
- This handoff file: `docs/dbx-database-parity-handoff.md`
- Pause summary file: `docs/dbx-database-parity-pause-summary.md`

Do not assume the whole dirty tree is yours to revert. Treat it as active parity work.

## Completed Parity Increments

Major implemented areas already present in the current worktree:

- DBX-style management SQL wrappers in Tauri/API/types/tests for create/drop database, create/drop schema, drop table/object, empty/truncate table, rename object, duplicate structure, table child object drops, database search SQL, export/import helpers, and related APIs.
- DBX-style create database flow, including MySQL-compatible charset/collation and DuckDB attach-file behavior.
- Visible database selection dialog and default database behavior.
- Connection grouping, pin/unpin, group rename/delete, and connection metadata saving.
- Extracted sidebar tree component: `src/components/database/DatabaseSidebarTree.tsx`.
- Extracted database search panel: `src/components/database/DatabaseSearchPanel.tsx`.
- Sidebar tree renders DBX connection, database, schema, grouped table/view/procedure/function/sequence/package objects, columns, indexes, foreign keys, and triggers.
- Sidebar tree search with debounce and scope buttons.
- Sidebar tree keyboard shortcuts: F2 rename, F5 refresh, Delete/Backspace drop/delete, Mod+C copy name.
- Sidebar tree context-menu callback coverage for connection group, database, schema, object, column, and table child nodes.
- DBX table/view node drag-and-drop into the SQL editor. Drag payload publishes both `text/plain` and `application/x-aeroric-database-object`; dropping inserts the qualified object reference at the editor cursor.
- DBX tree local multi-select/range select:
  - Plain click selects one node and activates it.
  - Ctrl/Cmd-click toggles selection without activation.
  - Shift-click selects the visible range.
- Data grid parity increments: cell update SQL preview/confirm, insert/delete row preview/confirm, selected-row bulk delete, page size selector, JSON/large cell preview, export selector and export payload support.
- DBX table import/export and database export entries have substantial coverage.
- Redis/Mongo browser components and Tauri/API wrappers exist.

## Most Recent Increment

The latest work focused on NoSQL sidebar tree parity:

- Added Redis database nodes to `DatabaseSidebarTree`.
  - Nodes show `dbN` plus key count.
  - Selecting a Redis database calls `onSelectRedisDatabase`.
  - Redis nodes participate in search and local tree selection state.
- Added MongoDB database and collection nodes to `DatabaseSidebarTree`.
  - Database nodes expand/collapse.
  - Collection nodes are rendered below the database.
  - Selecting a database calls `onSelectMongoDatabase`.
  - Selecting a collection calls `onSelectMongoCollection`.
  - Mongo nodes participate in search and local tree selection state.
- Added parent state in `DatabaseView.tsx`:
  - `redisDatabasesByConnection`
  - `mongoDatabasesByConnection`
  - `mongoCollectionsByDatabase`
- Added loaders in `DatabaseView.tsx` using existing API wrappers:
  - `databaseApi.dbxRedisListDatabases`
  - `databaseApi.dbxMongoListDatabases`
  - `databaseApi.dbxMongoListCollections`
- Updated `loadDbxConnection` for Redis/Mongo to connect and load NoSQL sidebar metadata.
- Added sidebar tree tests for Redis database nodes and Mongo database/collection nodes.

Latest command run after this NoSQL sidebar work:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx
```

Result:

```text
Test Files  34 passed (34)
Tests       280 passed (280)
```

After the NoSQL sidebar changes, the broader focused tests, lint, diff check, and build were rerun and passed.

Subsequent completed increment:

- Added NoSQL sidebar node context menus in `DatabaseView.tsx`.
  - Redis database nodes now open a context menu with copy name, open workspace, and refresh.
  - MongoDB database nodes now open a context menu with copy name, open workspace, and refresh.
  - MongoDB collection nodes now open a context menu with copy name, open workspace, and refresh.
- Wired the already-added `DatabaseSidebarTree` NoSQL context-menu callbacks into `DatabaseView`.
- Added `database.openWorkspace` i18n labels in English and Chinese.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies Redis/Mongo node context-menu callbacks.
  - `src/test/database-view.test.tsx` verifies NoSQL context menu rendering, copy-name behavior, and Redis/Mongo refresh calls.

Latest completed increment:

- Improved MongoDB sidebar lazy loading.
  - Opening a MongoDB connection now loads the database list without eagerly loading the first database's collections.
  - Expanding a MongoDB database node through the tree disclosure control triggers collection loading without selecting the database.
  - Selecting a MongoDB database still opens the Mongo workspace and loads that database's collections.
  - MongoDB database keyboard refresh now reloads that database's collections instead of refreshing the whole connection.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies expansion-triggered MongoDB collection lazy loading without selection.
  - `src/test/database-view.test.tsx` verifies MongoDB collections are not requested immediately on connection open and are requested by node-level refresh.

Latest completed increment:

- Added NoSQL workspace context menus.
  - Redis key rows now support right-click actions for copy name, refresh, and delete key.
  - MongoDB collection rows now support right-click actions for copy name, refresh, insert document, and delete matching documents.
- Added confirmation gates for high-risk NoSQL mutations that previously executed immediately.
  - Redis key deletion now asks for confirmation before calling `dbx_redis_delete_key`.
  - MongoDB matching-document deletion now asks for confirmation before calling `dbx_mongo_delete_documents`.
- Added i18n labels for MongoDB matching-document deletion and NoSQL delete confirmations.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies Redis key context-menu actions and delete confirmation.
  - `src/test/database-mongo-browser.test.tsx` verifies Mongo collection context-menu actions and delete confirmation.

Latest completed increment:

- Added Redis sidebar key lazy loading.
  - Redis database nodes now have disclosure controls and lazily scan keys with `dbx_redis_scan_keys`.
  - Loaded Redis keys render under their `dbN` node with key type badges.
  - Selecting a Redis key from the sidebar opens the Redis workspace and passes the selected db/key to `RedisBrowser`.
  - `RedisBrowser` now accepts an initial db/key and loads the selected key value on mount or sidebar selection changes.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies Redis key child-node rendering, lazy-load callback, active state, and selection callback.
  - `src/test/database-view.test.tsx` verifies expanding Redis db nodes scans keys and selecting a key opens/loads it in the Redis workspace.

Latest completed increment:

- Added MongoDB sidebar collection document previews.
  - MongoDB collection nodes now have disclosure controls and lazily load preview documents with `dbx_mongo_find_documents` using `{ filter: "{}", sort: "{}", skip: 0, limit: 20 }`.
  - Loaded documents render as child nodes under their collection with compact `_id`/field previews.
  - Selecting a document from the sidebar opens the Mongo workspace, keeps the active database/collection/document highlighted, and passes the initial database/collection/document id to `MongoBrowser`.
  - `MongoBrowser` now accepts initial database, collection, and document id props and loads/selects the matching document in the JSON editor.
  - Opening a MongoDB connection still preserves the prior lazy-loading behavior: it loads the database list without eagerly loading collections. `DatabaseView` only passes initial Mongo workspace selection after an explicit database/collection/document sidebar action.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies MongoDB document preview child-node rendering, active state, and selection callback.
  - `src/test/database-view.test.tsx` verifies expanding a MongoDB collection loads document previews and selecting a document opens/loads it in the Mongo workspace.
  - `src/test/database-mongo-browser.test.tsx` verifies initial database/collection/document selection loads and selects the expected document.

Latest completed increment:

- Added MongoDB sidebar document context menus.
  - MongoDB document child nodes now emit a document-specific context-menu callback from `DatabaseSidebarTree`.
  - `DatabaseView` renders document context-menu actions for copy name/id, open workspace, refresh preview, and delete document.
  - Document deletion is gated by confirmation, disabled for read-only connections or documents without `_id`, and calls `dbx_mongo_delete_documents` with `many: false` and an `_id` filter before refreshing the collection preview.
  - Added `database.confirmDeleteMongoDocument` i18n labels in English and Chinese.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies MongoDB document context-menu callback payloads.
  - `src/test/database-view.test.tsx` verifies MongoDB document context-menu rendering, copy-name behavior, preview refresh, delete confirmation, and delete API payload.

Latest completed increment:

- Added MongoDB sidebar document preview pagination.
  - `DatabaseView` now stores MongoDB sidebar preview totals per collection in `mongoDocumentTotalsByCollection`.
  - Initial collection preview still loads lazily with the first 20 documents and replaces the cached preview on refresh.
  - `DatabaseSidebarTree` now shows a `Load more (loaded/total)` child button when a collection has more preview documents available.
  - Clicking Load more calls `dbx_mongo_find_documents` with `skip` equal to the currently loaded preview count and appends the next page.
  - Added `database.loadMore` i18n labels in English and Chinese.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies the MongoDB Load more child-node rendering and callback payload.
  - `src/test/database-view.test.tsx` verifies Load more appends the next sidebar preview page and calls `dbx_mongo_find_documents` with the expected `skip`/`limit`.

Latest completed increment:

- Added Redis sidebar key context menus.
  - Redis key child nodes now emit a key-specific context-menu callback from `DatabaseSidebarTree`.
  - `DatabaseView` renders Redis key context-menu actions for copy name, open workspace, refresh scanned keys, and delete key.
  - Redis key deletion is gated by confirmation, disabled for read-only connections, and calls `dbx_redis_delete_key` before refreshing the Redis db key preview.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies Redis key context-menu callback payloads.
  - `src/test/database-view.test.tsx` verifies Redis key context-menu rendering, copy-name behavior, key scan refresh, delete confirmation, and delete API payload.

Latest completed increment:

- Added Redis sidebar key pagination.
  - `DatabaseView` now stores Redis sidebar scan cursor/total state per connection database in `redisScanStateByDatabase`.
  - Initial Redis db expansion still scans from cursor `0` and replaces the cached key preview.
  - `DatabaseSidebarTree` now shows a `Load more (loaded/total)` child button when a Redis scan returns a non-zero cursor.
  - Clicking Load more calls `dbx_redis_scan_keys` with the cached cursor and appends the next page of keys.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies Redis Load more child-node rendering and callback payload.
  - `src/test/database-view.test.tsx` verifies Redis Load more appends the next sidebar key page and calls `dbx_redis_scan_keys` with the expected cursor.

Latest completed increment:

- Added DBX connection context-menu support for copying the final proxy port.
  - `DatabaseView` now detects enabled DBX `transport_layers` and an explicit final proxy port value from `dbx.final_proxy_port`/`dbx.finalProxyPort`.
  - DBX connection nodes show `Copy final proxy port` only when that port is available.
  - Clicking the menu item copies the final proxy port to the clipboard.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the connection menu item is shown for a proxied DBX connection and copies the expected port.

Latest completed increment:

- Added DBX local database file reveal support in the connection context menu.
  - `DatabaseView` now detects SQLite/DuckDB DBX connections backed by a local file path in `dbx.host`, excluding empty paths and `:memory:`.
  - DBX connection nodes show `Reveal in File Manager` only for those local-file connections.
  - Clicking the menu item calls the existing Tauri `open_in_system_file_manager` command with the database file path and the current `projectRoot` safety root, falling back to the file path itself when no project root is available.
  - Added English and Chinese i18n labels for the menu item.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies a SQLite DBX connection shows `Reveal in File Manager` and invokes `open_in_system_file_manager` with the expected path.

Latest completed increment:

- Added DBX SQLite backup support in the connection context menu.
  - Tauri now exposes `dbx_backup_sqlite_database`, which reads the saved DBX SQLite connection config, rejects non-SQLite or non-file-backed connections, and uses `rusqlite`'s backup API to write the selected destination file.
  - `src-tauri/Cargo.toml` enables the `rusqlite` `backup` feature.
  - `databaseApi.dbxBackupSqliteDatabase` wraps the new command.
  - `DatabaseView` shows `Backup SQLite Database` for file-backed DBX SQLite connections, opens a save dialog with a DBX-style default backup filename, and runs the backup command.
  - Added English and Chinese i18n labels for the menu item.
- Expanded tests:
  - `src/test/database-api.test.ts` verifies the new API wrapper payload.
  - `src/test/database-view.test.tsx` verifies a SQLite DBX connection shows `Backup SQLite Database`, uses the expected save dialog defaults, and invokes `dbx_backup_sqlite_database`.

Latest completed increment:

- Added DBX user management workspace support for supported SQL connections.
  - Added `src/components/database/DatabaseUserAdminPanel.tsx`.
  - The panel supports MySQL and PostgreSQL DBX connections.
  - It lists users, loads grants for the selected user, and exposes create user, change password, drop user, grant privileges, and revoke privileges actions.
  - High-risk user/privilege mutations preview the generated SQL and require confirmation before calling `dbx_execute_multi`.
  - The panel reuses the existing DBX query commands instead of adding new Tauri commands.
- Added entry points:
  - The top database toolbar now includes `Users and permissions`.
  - DBX connection context menus now show `Users and permissions` only for supported MySQL/PostgreSQL connections.
  - Opening the entry switches to a dedicated `user-admin` workspace.
- Added English and Chinese i18n labels for user management fields, empty states, confirmation prompts, and execution status.
- Expanded tests:
  - `src/test/database-view.test.tsx` now verifies the connection context-menu entry and a user-management flow that loads users/grants, confirms generated create-user SQL, and invokes `dbx_execute_multi`.

Latest completed increment:

- Improved DBX user management parity.
  - MySQL user listing now falls back to `information_schema.USER_PRIVILEGES` when direct `mysql.user` access fails, matching dbx behavior for restricted accounts.
  - The fallback parser extracts quoted MySQL `GRANTEE` identities such as `'user'@'host'`.
  - PostgreSQL grant loading now uses a dbx-style role summary query that includes role attributes, role membership, database privileges, schema privileges, and table privileges instead of only raw table grants.
  - Added English and Chinese i18n labels for the MySQL fallback state.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the MySQL fallback user-list flow and the PostgreSQL role summary grants query.

Latest completed increment:

- Deepened DBX user management workspace parity.
  - Added login/account state actions:
    - MySQL `Lock account` / `Unlock account` generate `ALTER USER ... ACCOUNT LOCK/UNLOCK`.
    - PostgreSQL `Disable login` / `Enable login` generate `ALTER ROLE ... NOLOGIN/LOGIN`.
  - Added privilege scope and option controls:
    - PostgreSQL now supports database, schema, table, and role membership grant targets.
    - PostgreSQL role grants can use `WITH ADMIN OPTION`.
    - SQL grants can use `WITH GRANT OPTION`.
    - MySQL grants can use `WITH GRANT OPTION`.
  - High-risk login/account and privilege changes still preview SQL and require confirmation before execution.
  - Added English and Chinese i18n labels for login/account actions, privilege scopes, member roles, grant option, admin option, and confirmation prompts.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies MySQL account locking, MySQL grant option SQL, and PostgreSQL role-scope grant SQL with admin option.

Latest completed increment:

- Added DBX-style user search to the user management workspace.
  - `DatabaseUserAdminPanel` now includes a user search field above the user list.
  - Search filters by displayed user label, username, host, and plugin/role detail.
  - Empty search results show a dedicated no-match state while preserving the loaded user set.
  - Added English and Chinese i18n labels for the search field, placeholder, and no-match state.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies user search filtering and the no-match state.

Latest completed increment:

- Added DBX-style PostgreSQL create-role login parity.
  - PostgreSQL create user now emits `CREATE ROLE ... LOGIN|NOLOGIN PASSWORD ...`.
  - The create-user action now requires both a username and password before it is enabled.
  - PostgreSQL create user includes a `Can login` checkbox, allowing `LOGIN` or `NOLOGIN` roles.
  - Added English and Chinese i18n labels for the `Can login` field.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies PostgreSQL role creation with `Can login` disabled, including `CREATE ROLE "batch_role" NOLOGIN PASSWORD 'secret';`.

Latest completed increment:

- Added DBX-style privilege picker parity to the user management workspace.
  - `DatabaseUserAdminPanel` now exposes dbx's built-in MySQL common privilege list.
  - PostgreSQL privilege options now change by scope: database (`CONNECT`, `CREATE`, `TEMPORARY`), schema (`USAGE`, `CREATE`), and table (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `REFERENCES`, `TRIGGER`).
  - Clicking privilege buttons updates the existing comma-separated privilege input, preserving the custom input escape hatch for privileges outside the default lists.
  - Role-scope grants still hide the privilege picker and use the member-role/admin-option flow.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies PostgreSQL table-scope privilege picking and previews `GRANT SELECT, UPDATE ON ALL TABLES IN SCHEMA "public" TO "app_role";`.

Latest completed increment:

- Added DBX-style Redis value header actions.
  - `RedisBrowser` now shows icon actions for refreshing the selected key, copying the selected value, and copying a Redis command script that can recreate the selected key.
  - The insert-statement generator supports string, list, set, zset, hash, stream, and JSON/ReJSON values from the current `RedisValue`.
  - Generated Redis command scripts preserve positive TTL values with an `EXPIRE` command.
  - Binary Redis values were initially gated from copy-insert generation; a later increment changed this to a clickable DBX-style feedback path.
  - Added English and Chinese i18n labels for copy value and copy insert statement.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies copying a selected string value and the generated `SET`/`EXPIRE` Redis script.

Latest completed increment:

- Added MongoDB workspace document-level context menu parity.
  - `MongoBrowser` document cards in document view and rows in table view now support right-click actions.
  - Document context menu actions include copy document JSON, refresh, and delete document.
  - Document deletion uses the same high-risk confirmation pattern as the sidebar, then calls `dbx_mongo_delete_documents` with `many: false` and an `_id` filter.
  - Delete is disabled for read-only connections or documents without `_id`.
  - Added English and Chinese i18n labels for copying document JSON.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies document context-menu rendering, copy-document behavior, delete confirmation, and the single-document delete API payload.

Latest completed increment:

- Improved MongoDB workspace table-view parity.
  - `MongoBrowser` table mode now renders MongoDB documents as field columns instead of a single JSON-string column.
  - Columns are derived from the currently loaded documents, with `_id` pinned first and remaining fields sorted by name.
  - Cell rendering keeps scalar values readable and stringifies nested objects/arrays.
  - Existing document selection and document context-menu behavior are preserved from table cells.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies table mode renders derived `_id`, `active`, `name`, and `role` field columns with scalar cell values.

Latest completed increment:

- Added MongoDB workspace document pagination parity.
  - `useMongoBrowser.findDocuments` now appends results when called with `skip > 0`, matching the existing lazy loading behavior used elsewhere.
  - `MongoBrowser` now shows a `Load more (loaded/total)` action when the loaded document count is below the collection total.
  - Clicking Load more calls `dbx_mongo_find_documents` with `skip` equal to the currently loaded count and appends the next page.
  - The action respects the active database, collection, filter JSON, and sort JSON.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies workspace Load more appends the next page and calls `dbx_mongo_find_documents` with the expected `skip`/`limit`.

Latest completed increment:

- Added Redis workspace key pagination parity.
  - `useRedisBrowser` now tracks `totalKeys` from Redis scan results.
  - `RedisBrowser` shows a `Load more (loaded/total)` action when the Redis scan cursor is non-zero.
  - Clicking Load more calls `dbx_redis_scan_keys` with the cached cursor and appends the next page of keys.
  - The action respects the active Redis database and current key pattern.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies `Load more (1/2)` appends `user:2` and calls `dbx_redis_scan_keys` with `cursor: 7` and `count: 100`.

Latest completed increment:

- Added DBX-style Redis workspace key hierarchy.
  - Added `src/lib/redisKeyTree.ts` to build Redis key group/leaf rows from scanned keys using the configured key separator.
  - `RedisBrowser` now renders expandable folder rows for grouped keys and leaf rows for actual Redis keys, preserving full key names for selection, copy, refresh, delete, and accessibility labels.
  - Expanded groups are preserved across appended scan pages when still available.
  - `DatabaseView` now passes each Redis connection's `redis_key_separator` into the Redis workspace, falling back to `:`.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` now expands the Redis `user` group before selecting `user:1`, verifies context-menu actions still target the full raw key, and verifies workspace Load more appends `user:2` inside the expanded group.

Latest completed increment:

- Added DBX-style Redis workspace key group context menus.
  - `src/lib/redisKeyTree.ts` now exposes `collectRedisGroupKeyRaws` for loaded group children.
  - Redis workspace group rows now support right-click actions for copy group prefix, refresh current key scan, and delete loaded keys in the group.
  - Group deletion is gated by a single high-risk confirmation and then deletes each loaded raw key in that group with `dbx_redis_delete_key`.
  - Opening a key menu and opening a group menu now clear the other menu state to avoid overlapping context menus.
  - Added English and Chinese i18n labels for deleting Redis key groups and the group deletion confirmation.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies Redis group context-menu rendering, copy-prefix behavior, refresh scanning, single confirmation, and deletion calls for only the loaded keys in the selected group.

Latest completed increment:

- Added DBX-style Redis workspace fuzzy key search.
  - Added `src/lib/redisKeyPattern.ts` with Redis glob escaping and `redisKeySearchPattern` behavior matching dbx key-mode search.
  - `RedisBrowser` now shows a compact `Fuzzy` toggle beside the key pattern input.
  - Normal key searches keep the entered Redis glob pattern unchanged.
  - Fuzzy key searches wrap the trimmed text in `*...*` and escape Redis glob special characters before calling `dbx_redis_scan_keys`.
  - The same effective pattern is used for refresh, database switching, and Load more pagination.
  - Added English and Chinese i18n labels/tooltips for the fuzzy search toggle.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies Enter search sends the raw pattern in normal mode and `*ser*` after enabling fuzzy mode.

Latest completed increment:

- Added DBX-style Redis workspace loaded-key multi-select and batch deletion.
  - Redis key leaf rows now show checkboxes for selecting loaded keys.
  - Selected key state is pruned when the loaded key list changes.
  - Selecting one or more loaded keys shows a compact `Delete selected` action.
  - Batch deletion uses a single high-risk confirmation and then deletes each selected raw key with `dbx_redis_delete_key`.
  - The batch action is disabled for read-only connections.
  - Added English and Chinese i18n labels for selected-key count and selected-key deletion confirmation.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies selecting `user:1` and `user:2` shows `Delete selected (2)`, confirms once, deletes only those selected raw keys, and leaves an unselected loaded key untouched.

Latest completed increment:

- Added DBX-style Redis workspace `Fetch all` key loading.
  - `RedisBrowser` now shows `Fetch all` next to `Load more` while a Redis scan cursor remains non-zero.
  - Clicking `Fetch all` loops through `dbx_redis_scan_keys` from the current cursor until Redis returns cursor `0`.
  - The loop preserves the active Redis database and the same effective key pattern used by normal search, fuzzy search, refresh, and Load more.
  - The action is disabled while Redis loading/fetch-all is already running, and it guards against repeated cursors.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies `Fetch all` requests cursor `5`, then cursor `8`, appends `user:2` and `user:3`, and keeps the expanded key group visible.

Latest completed increment:

- Added DBX-style Redis command safety confirmation in the workspace.
  - Added `src/lib/redisCommandSafety.ts`, matching dbx's first-token parsing and `allowed` / `confirm` / `blocked` classification.
  - `RedisBrowser` now blocks dangerous commands such as `KEYS`, `FLUSHALL`, `EVAL`, and `CONFIG` before calling Tauri.
  - Destructive commands such as `DEL`, `SET`, `HSET`, `LPUSH`, `ZADD`, and `FLUSHDB` now require a warning confirmation before execution.
  - Confirmed destructive commands call `dbx_redis_execute_command` with `skipSafetyCheck: true`, then refresh the key scan so the workspace reflects possible data changes.
  - Added English and Chinese i18n labels for blocked command output and Redis command confirmation.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies blocked commands do not invoke `dbx_redis_execute_command`, confirmed destructive commands invoke it with `skipSafetyCheck: true`, and cancelled confirmations do not execute.

Latest completed increment:

- Added DBX-style Redis command terminal behavior in the workspace.
  - Added `src/lib/redisCommandSession.ts` with dbx-compatible `clear`/`cls` detection and `SELECT n` prompt switching after an `OK` result.
  - `RedisBrowser` now keeps an in-memory command terminal history instead of replacing the last command result.
  - Command history rows show the `dbN>` prompt, submitted command, formatted output, and error output.
  - The command runner now uses a separate `commandDb`, initialized from the active Redis database and updated by successful `SELECT n` commands.
  - `clear`/`cls` clears the local terminal without invoking `dbx_redis_execute_command`.
  - Added English and Chinese i18n labels for command welcome text, clear history, and empty command output.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies `SELECT 2` updates the prompt and the next command executes against db2.
  - `src/test/database-redis-browser.test.tsx` verifies `clear` removes local terminal output without invoking Tauri.

Latest completed increment:

- Added DBX-style Redis command terminal history persistence.
  - `src/lib/redisCommandSession.ts` now stores validated command terminal history entries in connection-scoped localStorage, capped at the latest 200 entries.
  - `RedisBrowser` loads persisted command history when a Redis connection workspace mounts.
  - Successfully executed Redis commands and execution errors are appended to terminal history and persisted.
  - The Clear history toolbar action now clears both the in-memory terminal output and the connection-scoped persisted history.
  - Blocked commands, empty submissions, and `clear`/`cls` remain local terminal interactions and are not persisted.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies command output and command text are restored after remounting the same Redis connection.
  - `src/test/database-redis-browser.test.tsx` verifies Clear history removes persisted command terminal history.

Latest completed increment:

- Added DBX-style MongoDB workspace filter interactions.
  - MongoDB filter JSON and sort JSON inputs now apply the current query when Enter is pressed, matching dbx document browser behavior.
  - Added a `Clear filter` action that resets both filter and sort JSON to `{}` and immediately reloads the active collection with the reset query.
  - `MongoBrowser.loadDocuments` now accepts explicit filter/sort override values so reset actions do not depend on stale React state.
  - Added English and Chinese i18n labels for clearing filters.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies pressing Enter in the filter input calls `dbx_mongo_find_documents` with the current filter JSON.
  - `src/test/database-mongo-browser.test.tsx` verifies `Clear filter` resets filter/sort inputs and reloads documents with `{}` filter/sort payloads.

Latest completed increment:

- Added filter-aware MongoDB sidebar document previews.
  - `MongoBrowser` now reports the applied document query through `onDocumentsQueryApplied` whenever the workspace loads a collection with a filter/sort pair.
  - `DatabaseView` stores the latest MongoDB workspace filter/sort per `connection:database:collection`.
  - MongoDB sidebar preview refreshes and `Load more` requests now reuse the stored workspace query instead of always using `{}`.
  - The existing lazy-load MongoDB document preview test was adjusted to explicitly expand the collection before asserting sidebar pagination.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies applying a workspace filter refreshes the sidebar document preview with that filter.
  - `src/test/database-view.test.tsx` verifies MongoDB sidebar `Load more` keeps using the current workspace filter payload.

Latest completed increment:

- Added DBX-style MongoDB workspace page navigation.
  - `MongoBrowser` now keeps an explicit workspace page index and shows previous/next page controls using the existing `database.page` display.
  - Page navigation calls `dbx_mongo_find_documents` with `skip = page * 100` and replaces the current document page, matching dbx's paged document browser behavior.
  - Existing `Load more` behavior is preserved for the current Aeroric workflow by making `useMongoBrowser.findDocuments` append only when the caller passes `append: true`.
  - Applying filters, clearing filters, changing database, or opening a collection resets the Mongo workspace page to the first page.
  - Added English and Chinese labels for previous/next page controls.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies next/previous page navigation requests `skip: 100` and replaces rather than appends prior page results.
  - Existing workspace `Load more` coverage continues to verify appended Mongo document loading.

Latest completed increment:

- Added DBX-style MongoDB workspace page size control.
  - `MongoBrowser` now exposes the existing `database.gridRowsPerPage` selector in the Mongo workspace, with page size options `50`, `100`, `200`, `500`, and `1000`.
  - Changing page size resets the workspace to page 1 and reloads the active collection with the selected `limit`.
  - Previous/next page navigation now uses the selected page size for both `skip` and `limit`.
  - `Load more` now accounts for the current page offset when deciding whether more documents can be loaded and when computing the next `skip`, while preserving the existing append workflow.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies changing Mongo page size to `50` reloads with `limit: 50`.
  - `src/test/database-mongo-browser.test.tsx` verifies paging after the page-size change requests `skip: 50` and `limit: 50`.

Latest completed increment:

- Added DBX-style MongoDB table column sorting.
  - MongoDB table-mode column headers are now clickable sort controls.
  - Clicking a column cycles through ascending sort (`{"column":1}`), descending sort (`{"column":-1}`), and clearing the sort (`{}`).
  - Sorting updates the Sort JSON input, resets the workspace to page 1, and reloads the active collection with the current filter, selected page size, and new sort JSON.
  - Sorted columns expose `aria-sort` and show lucide sort direction icons while preserving existing table cell selection/context-menu behavior.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies clicking a table column header sends ascending and descending Mongo sort payloads.
  - `src/test/database-mongo-browser.test.tsx` verifies the Sort JSON input reflects the selected column sort.

Latest completed increment:

- Added DBX-style Redis workspace Clear current DB support.
  - `RedisBrowser` now exposes a destructive `Clear current DB` action in the Redis workspace toolbar.
  - The action requires warning confirmation before executing `FLUSHDB` through `dbx_redis_execute_command`.
  - Confirmed execution uses `skipSafetyCheck: true`, clears the selected Redis value and selected loaded keys, rescans the current Redis database with the active effective key pattern, and reloads Redis database metadata.
  - Added English and Chinese i18n labels for the action, confirmation message, and confirmation button.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies workspace confirmation, `FLUSHDB` execution payload, and current database rescan payload.

Latest completed increment:

- Added DBX-style Redis sidebar database Clear current DB support.
  - Redis database nodes in `DatabaseSidebarTree` now expose `Clear current DB` from the sidebar context menu.
  - The action is disabled for read-only connections.
  - Confirmed execution calls `dbx_redis_execute_command` with `FLUSHDB` and `skipSafetyCheck: true`, then rescans that Redis database's sidebar key preview and reloads Redis database metadata.
  - The sidebar action reuses the same Redis Flush DB confirmation text and labels as the workspace action.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies sidebar confirmation, `FLUSHDB` execution payload, and sidebar key rescan payload.

Latest completed increment:

- Added DBX-style Redis key TTL badge editing in the workspace.
  - The selected Redis key header now renders the key type and TTL as compact badges instead of plain `key_type · TTL n` text.
  - Clicking the TTL/no-expiry badge opens an inline TTL editor, matching dbx's Redis value detail behavior.
  - Saving the TTL validates the input, treats an empty value or `-1` as no expiry, calls `dbx_redis_set_ttl`, then reloads the selected key and refreshes the current key scan.
  - Read-only Redis workspaces keep the TTL badge non-editable.
  - Added English and Chinese i18n labels for no expiry, TTL seconds, TTL save, and TTL validation.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies editing the selected key TTL from the badge calls `dbx_redis_set_ttl` and refreshes the displayed TTL.

Latest completed increment:

- Added DBX-style default database actions to NoSQL database node context menus.
  - Redis database and MongoDB database sidebar nodes now show `Set as default database` or `Clear default database` based on the connection's current `dbx.database` value.
  - Redis default databases are saved using dbx-compatible bare database numbers such as `0`, while MongoDB default databases use the database name.
  - The actions reuse the existing `dbx_save_connection` path through `saveDbxDefaultDatabase`, then reload the DBX connection state.
  - Existing Redis `Clear current DB` and NoSQL open/refresh/copy actions remain unchanged.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies Redis database context-menu default saving and MongoDB database context-menu default clearing payloads.

Latest completed increment:

- Added DBX-style Redis JSON value draft formatting controls.
  - `RedisBrowser` now exposes `Format` and `Compress` actions for the selected non-binary Redis value draft.
  - `Format` parses the current draft as JSON and rewrites it with two-space indentation.
  - `Compress` parses the current draft as JSON and rewrites it as compact JSON.
  - Invalid JSON leaves the draft unchanged and shows the localized invalid JSON error.
  - The actions are disabled for read-only workspaces, no selected Redis value, and binary Redis values.
  - Added English and Chinese i18n labels for the two actions and invalid JSON state.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies formatting compact JSON to pretty JSON, compressing it back to compact JSON, and showing an invalid JSON error without saving.

Latest completed increment:

- Improved DBX-style Redis value save refresh behavior.
  - Saving a selected Redis value draft now calls `dbx_redis_set_value`, reloads the selected key with `dbx_redis_get_value`, and refreshes the current workspace key scan.
  - This keeps the selected value header, TTL badge, key size, and preview data in sync after edits.
  - The refresh uses the current Redis database and effective key pattern, matching the existing workspace refresh behavior.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies saving a value draft writes the new value/TTL, reloads the selected key, refreshes the key preview, and updates the displayed TTL badge.

Latest completed increment:

- Added Redis value-save TTL validation parity.
  - The main `Save value` action now validates the Redis TTL draft before calling `dbx_redis_set_value`.
  - Empty TTL and `-1` are treated as no expiry, matching the existing TTL badge editor behavior.
  - Invalid TTL values show the localized TTL validation error in the value editor area and do not call the backend.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies invalid TTL input blocks `dbx_redis_set_value` and shows the validation error.

Latest completed increment:

- Improved MongoDB workspace document refresh/selection parity.
  - Saving a selected MongoDB document now waits for the current query to refresh, then reselects the document with the same `_id` from the refreshed result set.
  - The MongoDB document context-menu `Refresh` action now refreshes the current query and keeps the same `_id` selected when it is still present.
  - Inserting a MongoDB document now uses the returned inserted `_id` to refresh the first page and select the inserted document.
  - These paths synchronize the JSON editor back to the refreshed pretty-printed document, avoiding stale or compact editor content after mutations/refreshes.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies save refresh/selection, context-menu refresh/selection, and insert refresh/selection behavior.

Latest completed increment:

- Improved MongoDB workspace editor clearing parity.
  - MongoDB selection reset now clears both the selected index and the JSON editor draft.
  - Refreshing a collection, changing pages, switching page size, clearing filters, deleting a selected document, and refreshing when the same `_id` is no longer present no longer leave stale document JSON in the editor.
  - The empty draft remains the existing `{\\n  \\n}` insert/edit template so insert workflows keep their current entry point.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies deleting the selected MongoDB document clears the JSON editor.

Latest completed increment:

- Improved MongoDB database switching state parity.
  - `useMongoBrowser` now exposes `clearDocuments()` to clear the current document page and total count without issuing a backend query.
  - Switching MongoDB databases now clears the active collection, selected document, JSON editor draft, current document page, and total count before loading the next database's collections.
  - This prevents stale documents from the previous database/collection from remaining visible while the new database has no active collection selected.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies switching databases removes stale document cards and clears the JSON editor while loading the new database's collections.

Latest completed increment:

- Improved MongoDB connection switching state parity.
  - `useMongoBrowser` now exposes `resetBrowserState()` to clear databases, collections, documents, totals, loading state, and errors when a workspace moves to another MongoDB connection.
  - MongoDB list/query requests are guarded by a request generation so late responses from the previous connection cannot repopulate the new connection's workspace state.
  - `MongoBrowser` now clears the active database, collection, filter, sort, selected document, JSON editor draft, current page, and context menus before loading the next connection's databases.
  - Initial database/collection/document selection still works after the reset, using the default `{}` filter/sort for the initial query.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies switching MongoDB connections removes stale database, collection, document, and editor state before showing the next connection's databases.

Latest completed increment:

- Improved Redis connection switching state parity.
  - `useRedisBrowser` now exposes `resetBrowserState()` to clear databases, scanned keys, cursor/total state, selected value, loading state, and errors when a workspace moves to another Redis connection.
  - Redis database/key/value requests are guarded by a request generation so late responses from the previous connection cannot repopulate the new connection's workspace state.
  - `RedisBrowser` now clears active database, command database, key pattern/fuzzy search, fetched-key state, expanded groups, selected keys, value/TTL drafts, create-key form, command input, and context menus before loading the next connection's databases.
  - Initial database/key selection still works after the reset.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies switching Redis connections removes stale database, key, selected value, and value editor state before showing the next connection's databases.

Latest completed increment:

- Improved Redis database switching state parity.
  - `useRedisBrowser` now exposes `clearKeyspaceState()` to clear scanned keys, cursor/total state, selected value, and errors while invalidating in-flight key/value requests for the previous Redis database.
  - Switching Redis databases in `RedisBrowser` now clears expanded key groups, selected key checkboxes, value/TTL drafts, TTL editing state, and key/group context menus before scanning the next database.
  - This prevents stale keys or selected values from one Redis db remaining visible after switching to another db in the same connection.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies switching Redis databases removes stale key, selected value, and value editor state before showing the next database's keyspace.

Latest completed increment:

- Improved MongoDB collection switching state parity.
  - Starting a new MongoDB document query from collection selection, filter clearing, sorting, page-size changes, or page navigation now clears the currently displayed document page and total count before the backend query returns.
  - This keeps the workspace from showing documents from the previous collection/page while a slower MongoDB query is still in flight.
  - The existing selected document and JSON editor draft clearing behavior remains in place.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies switching MongoDB collections removes stale document cards and clears the JSON editor immediately, even while the next collection query is pending.

Latest completed increment:

- Added DBX-style NoSQL tree-node pin/unpin parity.
  - Redis database sidebar context menus now include `Pin` / `Unpin`, matching dbx `redis-db` pinnable-node behavior.
  - MongoDB database sidebar context menus now include `Pin` / `Unpin`, matching dbx `mongo-db` pinnable-node behavior.
  - MongoDB collection sidebar context menus now include `Pin` / `Unpin`, matching dbx `mongo-collection` pinnable-node behavior.
  - Pinned NoSQL nodes are stored by tree-node id in localStorage, render a pin indicator in the sidebar tree, and are ordered before unpinned siblings.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies Redis database and MongoDB collection pin/unpin context-menu behavior and pinned-node indicators.

Latest completed increment:

- Improved MongoDB stale query suppression during collection switching.
  - Clearing MongoDB documents now also advances the browser request generation.
  - When a previous collection/page/filter query resolves after a newer query has started, the late result is ignored instead of replacing the current collection's documents.
  - This extends the existing connection-switch stale response guard to workspace-level document query changes.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies a late document result from the previous collection cannot overwrite the newer collection's result.

Latest completed increment:

- Improved MongoDB database switching collection-list parity.
  - `useMongoBrowser.clearCollections()` now advances the browser request generation before clearing the collection list.
  - Switching MongoDB databases in `MongoBrowser` now clears the previous database's collection list immediately before loading the next database's collections.
  - Late collection-list results from the previous database are ignored by the existing generation guard.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies stale collection-list results cannot overwrite the newer database's collections and that old collections disappear immediately while the next database's collections are loading.

Latest completed increment:

- Added DBX-style NoSQL database-node `New query` context-menu parity.
  - Redis database sidebar context menus now include `New query`, matching dbx `redis-db` node behavior.
  - MongoDB database sidebar context menus now include `New query`, matching dbx `mongo-db` node behavior.
  - Triggering the action switches Aeroric to the query workspace while preserving the selected NoSQL connection and database state.
- Stabilized Redis key scan stale-response handling.
  - `useRedisBrowser` now tracks key-scan request generations separately from connection/value request generations.
  - Starting a replacement key scan invalidates older scan responses without invalidating selected value/TTL flows.
  - Redis pattern search uses the input's current value on Enter, avoiding stale React state when a search is submitted immediately after typing.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies Redis/Mongo database-node `New query` menu entries and workspace switching.
  - `src/test/database-redis-browser.test.tsx` verifies late Redis scan results from an old pattern cannot overwrite the latest pattern's key tree and waits for value selection before invalid TTL save assertions.

Latest completed increment:

- Added DBX-style SQL schema-node utility workspace parity.
  - DBX schema sidebar context menus now include `Execute SQL file`, `Data transfer`, `Schema diff`, and `Data compare`.
  - `Execute SQL file` opened from a schema node preserves the selected DBX connection, database, and schema context.
  - SQL-file execution now receives the schema selected from the schema node via the existing `dbx_execute_sql_file` payload.
  - Advanced tool workspaces receive the active schema context before falling back to the selected table schema.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies schema context-menu utility entries and SQL-file execution payload context.

Latest completed increment:

- Added DBX-style SQL tree-node pin/unpin parity.
  - SQL database, schema, table, and view sidebar context menus now include `Pin` / `Unpin`, matching dbx pinnable-node behavior for these node types.
  - Pinned SQL database/schema/object nodes share the existing persisted tree-node pin set, render the pin indicator in the sidebar, and are ordered before unpinned siblings.
  - The persisted pin helpers were generalized from NoSQL-only naming while keeping the existing localStorage key compatible.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies pinned SQL database/schema/object ordering and indicators.
  - `src/test/database-view.test.tsx` verifies SQL database/schema/table context-menu pin/unpin behavior.

Latest completed increment:

- Added DBX-style SQL object-group context menu parity.
  - Sidebar object group rows such as `Tables`, `Views`, `Procedures`, `Functions`, `Sequences`, and `Packages` now emit context-menu callbacks.
  - `Tables` group context menus include `Create table` and `Refresh`.
  - `Views` group context menus include `Create view` and `Refresh`.
  - Other object groups expose node-level `Refresh`, matching dbx group-label behavior.
  - `Create table` and `Create view` open the query workspace with the selected DBX connection/database/schema preserved and a dbx-style SQL draft.
  - Added English and Chinese i18n labels for `Create view`.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies object-group context-menu callback payloads.
  - `src/test/database-view.test.tsx` verifies object-group `Refresh`, `Create table`, and `Create view` actions.

Latest completed increment:

- Added DBX-style connection-group context menu parity.
  - DBX connection group rows now expose `Copy name`, `New connection`, `New group`, `Rename group`, and `Delete group`.
  - `Copy name` writes the group name to the clipboard.
  - `New connection` opens the existing connection wizard with the selected group prefilled into the DBX connection metadata.
  - `New group` creates an empty subgroup using Aeroric's string group model, persisted in localStorage until a connection is assigned.
  - Group rename/delete keeps saved DBX connection metadata and persisted empty groups in sync.
  - Added English and Chinese i18n labels for new connection-group creation.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies connection-group copy, new subgroup creation, prefilled new connection, pin, move, rename, and delete behavior.

Latest completed increment:

- Added DBX-style view-node `Edit view` context-menu parity.
  - DBX view object context menus now expose a distinct `Edit view` item before `View source`, matching dbx's table/view node menu shape.
  - `Edit view` reuses the existing DBX object source loader and opens the query workspace with the fetched view source SQL.
  - Table-only structure editing remains hidden for view nodes.
  - Added English and Chinese i18n labels for `Edit view`.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the view-node `Edit view`, `View source`, and `View DDL` menu entries, guards against `Edit Structure` on views, and verifies the `dbx_get_object_source` payload.

Latest completed increment:

- Added DBX-style view-node SQL-template menu parity.
  - DBX view object context menus now expose `New query` for SELECT draft creation, matching dbx's view node behavior.
  - View nodes no longer show table-only `New SQL: INSERT` or `New SQL: UPDATE` template actions.
  - Table nodes keep the existing SQL template actions.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies view-node `New query`, verifies the generated SELECT draft, and guards against table-only INSERT/UPDATE entries on views.

Latest completed increment:

- Added DBX-style table-node duplicate-structure context-menu parity.
  - DBX table object context menus now expose `Duplicate structure`, matching dbx's table-only node action.
  - The action prompts for a target table name, defaults to a unique `{table}_copy` variant within the same schema, previews the generated SQL, asks for confirmation, executes it, and refreshes the DBX connection tree.
  - The duplicate action is hidden for read-only DBX connections and remains table-only.
  - DBX table object detection now uses the existing case-insensitive helper for uppercase backend object types such as `TABLE`.
  - Added English and Chinese i18n labels for duplicate-structure prompt and confirmation.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies table-node `Duplicate structure`, default target-name deconfliction, generated SQL confirmation, and execute payload.

Latest completed increment:

- Tightened DBX-style table/view context-menu shape parity.
  - `Data compare` is now table-only and no longer appears on view nodes, matching dbx's `isTableNotView` gate.
  - View-node `Drop view` now appears before `New query`, matching dbx's view-node action order.
  - View nodes still keep `Edit view`, `View source`, `View DDL`, `New query`, export, structure-copy, and refresh behavior.
  - DBX object menu items are assembled by a typed helper to keep the production TypeScript build from hitting oversized union inference.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies view-node `Drop view` visibility and guards against `Data compare` on views.

Latest completed increment:

- Tightened DBX-style table-node destructive context-menu order.
  - Table menus now list `Duplicate structure`, `Truncate Table`, `Empty Table`, and `Drop Table` in dbx order.
  - The existing high-risk SQL preview/confirmation behavior for truncate, empty, and drop is unchanged.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the relative table-node action order.

Latest completed increment:

- Tightened DBX-style routine-like object context-menu shape parity.
  - Procedure/function menus now match dbx by omitting the generic `Copy name` action and preserving `Execute procedure` / `View source` / `Drop procedure` order.
  - Sequence/package-style routine menus now show `View source` before `Copy name`, matching dbx's sequence/package menu shape.
  - Existing procedure execution, object source loading, and SQL preview/confirmation for procedure/function drops are unchanged.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies procedure menu ordering and absence of `Copy name`.
  - `src/test/database-view.test.tsx` verifies sequence menu ordering, copy-name payloads, and source loading payloads.

Latest completed increment:

- Added DBX-style truncate-table capability gating.
  - Table context menus now show `Truncate Table` only for DBX database types that support truncation, matching dbx's `supportsTableTruncate` behavior.
  - SQLite and DuckDB table menus hide `Truncate Table` while preserving `Empty Table` and `Drop Table`.
  - The truncate action handler now also guards unsupported DBX database types before generating or executing SQL.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies PostgreSQL table administration still exposes and executes truncate.
  - `src/test/database-view.test.tsx` verifies SQLite table menus hide `Truncate Table` but keep `Empty Table` and `Drop Table`.

Latest completed increment:

- Tightened DBX-style database/schema node context-menu parity.
  - Database and schema node menu items are now assembled through typed DBX capability helpers instead of unconditional inline arrays.
  - PostgreSQL database nodes hide `Open object browser`, while PostgreSQL schema nodes expose it, matching dbx's schema-aware object-browser gate.
  - Database-node actions now follow dbx order: default-database action before create actions, create table/schema before SQL-file execution, diagram/search before refresh, and export/close/drop at the end.
  - Schema-node menus no longer expose `Close database connection`, matching dbx's database-only close gate.
  - DuckDB database nodes hide `Open object browser`, `ER diagram`, and `Drop database`, while keeping `Create schema`.
  - The truncate support helper now carries dbx's full unsupported-type exclusion list for future DBX database types.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies PostgreSQL database/schema menu order and object-browser/close-action gates.
  - `src/test/database-view.test.tsx` verifies DuckDB database-node action gates.

Latest completed increment:

- Tightened DBX-style NoSQL sidebar context-menu shape parity.
  - Redis and MongoDB database node menu items are now assembled through typed NoSQL menu helpers.
  - Redis database node menus now match dbx order: `Pin`/`Unpin`, `New query`, default-database action, then `Clear current DB`.
  - MongoDB database node menus now match dbx order: `Pin`/`Unpin`, `New query`, then default-database action.
  - MongoDB collection node menus now only expose `Pin`/`Unpin`, matching dbx's collection node shape.
  - Existing Redis key and MongoDB document child-node context menus are unchanged.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies Redis database, MongoDB database, and MongoDB collection menu ordering.
  - `src/test/database-view.test.tsx` verifies NoSQL pin/unpin, default-database set/clear, Redis clear-DB confirmation, and MongoDB sidebar document preview flows against the updated menu shape.

Latest completed increment:

- Added DBX-style user-admin utility node parity in the sidebar tree.
  - `DatabaseSidebarTree` now renders a `Users and permissions` child node for DBX connections whose database type supports user management.
  - The node appears under the expanded active connection after regular database/object children, matching dbx's connection utility-node placement.
  - Clicking the node opens the existing Aeroric user-management workspace for that connection.
  - Right-clicking the node opens a single-item context menu, `Open Users & Privileges`, matching dbx's `user-admin` tree node menu shape.
  - Unsupported DBX connections do not render the utility node.
  - Added English and Chinese i18n labels for the node context-menu action.
- Expanded tests:
  - `src/test/database-sidebar-tree.test.tsx` verifies supported/unsupported user-admin node rendering, active selection styling, click behavior, and context-menu callback payload.
  - `src/test/database-view.test.tsx` verifies the sidebar utility node context menu opens the existing user-management panel and loads grants.

Latest completed increment:

- Added DBX-style connection context-menu open/close parity.
  - Connection context menus now show `Open connection` when the legacy or DBX connection is not the active connection.
  - Connection context menus now show `Close connection` only when that connection is active.
  - The `Open connection` action opens legacy connections via the existing inspect flow and DBX connections via `loadDbxConnection`.
  - Added English and Chinese i18n labels for `database.openConnection`.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies an inactive DBX connection shows `Open connection`, clicking it invokes `dbx_connect`, and the active connection menu switches to `Close connection`.

Latest completed increment:

- Tightened DBX-style connection context-menu order and duplicate parity.
  - DBX connection menus now put `Pin connection`/`Unpin connection` first, matching dbx's pin-first tree menu shape.
  - DBX connection menu items now follow dbx's relative order for Open/Close, New query, history, user admin, proxy port copy, SQL file execution, create database, move to group, refresh, visible databases, edit, local-file actions, duplicate, and delete.
  - The connection duplicate action now uses the dbx label `Duplicate Connection`.
  - Duplicated legacy and DBX connections now default to `Name (Copy)`, matching dbx's duplicate naming pattern.
  - Added English and Chinese i18n labels for `database.duplicateConnection`.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies DBX connection menu ordering, the duplicate label, and the `dbx_save_connection` payload name `DBX Source (Copy)`.

Latest completed increment:

- Added DBX-style connection group move entry labeling.
  - Ungrouped DBX connections with no available groups now show `Move to New Group`, matching dbx's direct move-to-new-group entry.
  - DBX connections that already belong to a group, or that have existing/extra groups available, continue to show `Move to group`.
  - The action still reuses Aeroric's existing prompt-based group move flow while aligning the visible entry point.
  - Added English and Chinese i18n labels for `database.moveToNewGroup`.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the ungrouped connection context menu shows `Move to New Group`.
  - The existing connection group flow verifies grouped connections still expose and execute `Move to group`.

Latest completed increment:

- Added DBX-style Redis binary insert-statement copy feedback.
  - `Copy insert statement` is now clickable for selected Redis binary values, matching dbx's action availability.
  - Clicking it for binary data shows `Cannot generate insert statement for binary data` instead of silently doing nothing behind a disabled button.
  - The binary path does not write to the clipboard.
  - Added English and Chinese i18n labels for `database.redisCopyInsertStatementBinary`.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies selected binary Redis values keep the copy-insert action enabled, show the binary-data feedback, and do not write to the clipboard.

Latest completed increment:

- Added DBX-style Redis workspace `Fetch all` progress and stop feedback.
  - `RedisBrowser` now shows loaded/total progress while `Fetch all` is scanning the remaining key pages.
  - The progress text uses DBX-style known-total and unknown-total labels: `{loaded} of {total} keys loaded` or `{loaded} keys loaded`.
  - While fetch-all is running, the normal `Load more` / `Fetch all` actions are hidden and replaced with a destructive `Stop` action.
  - Clicking `Stop` halts any additional Redis scan pages after the currently pending page resolves.
  - Added English and Chinese i18n labels for fetch-all progress and stop.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies fetch-all progress text, stop behavior, appending the currently resolving scan page, and preventing the next cursor request.

Latest completed increment:

- Added DBX-style Redis collection member viewing in the workspace.
  - `RedisBrowser` now renders DBX-style member/field rows for loaded Redis `list`, `set`, `hash`, `zset`, and `stream` values instead of relying only on the raw value textarea.
  - Collection rows show type-appropriate columns such as `#`/value, field/value, score/member, and stream entry/field/value.
  - The member section shows DBX-style loaded count labels, including loaded/total wording when Redis reports a larger collection total than the currently loaded value payload.
  - Selecting a member row opens an inline `Member detail` preview with raw/JSON format indication.
  - Member copy actions copy the selected member/field value without affecting the existing whole-key copy behavior.
  - Added English and Chinese i18n labels for member counts, columns, detail, view, copy, and raw-content labels.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies Redis hash member rows, loaded/total field counts, member detail selection, and member copy behavior.

Latest completed increment:

- Added DBX-style Redis collection member pagination in the workspace.
  - Tauri now exposes `dbx_redis_load_more`, a thin wrapper around dbx-core `redis_load_more_in_db_core`.
  - `databaseApi.dbxRedisLoadMore` and `useRedisBrowser.loadMoreValue` append returned collection members into the currently selected Redis value.
  - `RedisBrowser` now shows a `Load more (loaded/total)` action for selected Redis `list`, `set`, `hash`, and `zset` values when `scan_cursor` indicates additional members are available.
  - Loading more members preserves the current selected key/value detail and hides the action when the returned page has no further cursor.
  - Stream values remain read-only for the currently returned entries because dbx-core pagination support is limited to list/set/hash/zset collections.
- Expanded tests:
  - `src/test/database-api.test.ts` verifies the `dbx_redis_load_more` wrapper payload.
  - `src/test/database-redis-browser.test.tsx` verifies member `Load more`, request payload, appended hash fields, and action disappearance after the final page.

Latest completed increment:

- Added DBX-style Redis collection member deletion in the workspace.
  - Tauri now exposes thin DBX wrappers for `dbx_redis_hash_del`, `dbx_redis_list_remove`, `dbx_redis_set_remove`, and `dbx_redis_zrem`.
  - `databaseApi` and `useRedisBrowser` wrap those member-delete commands with typed request payloads.
  - `RedisBrowser` now shows a destructive row action for deletable `list`, `set`, `hash`, and `zset` members.
  - Member deletion is gated by a warning confirmation before calling the backend.
  - Confirmed deletion reloads the selected Redis value and refreshes the current key preview so member rows, counts, and key metadata stay in sync.
  - Added English and Chinese i18n labels for deleting members and member-specific confirmation text.
- Expanded tests:
  - `src/test/database-api.test.ts` verifies the Redis collection member delete command payloads.
  - `src/test/database-redis-browser.test.tsx` verifies hash field deletion from a member row, confirmation, backend payload, and refreshed member rows.

Latest completed increment:

- Added DBX-style Redis collection member editing in the workspace.
  - Tauri now exposes thin DBX wrappers for `dbx_redis_hash_set`, `dbx_redis_list_set`, `dbx_redis_set_add`, and `dbx_redis_zadd`.
  - `databaseApi` and `useRedisBrowser` wrap those member-edit commands with typed request payloads.
  - `RedisBrowser` now shows edit actions for editable `list`, `set`, `hash`, and `zset` members.
  - Member editing reuses the DBX behavior:
    - `list`: `LSET` by index.
    - `hash`: `HSET` by field.
    - `set`: remove the old member and `SADD` the edited member.
    - `zset`: remove the old member and `ZADD` the edited member with the previous score.
  - Saving an edit reloads the selected Redis value and refreshes the current key preview so member rows, counts, and metadata stay in sync.
  - Added English and Chinese i18n labels for editing, saving, canceling, and the member-value textarea.
- Expanded tests:
  - `src/test/database-api.test.ts` verifies the Redis collection member edit command payloads.
  - `src/test/database-redis-browser.test.tsx` verifies hash field editing from a member detail action, backend payload, reload, and refreshed member rows.

Latest completed increment:

- Added DBX-style Redis collection member creation in the selected value workspace.
  - Tauri now exposes `dbx_redis_list_push`, a thin DBX wrapper around dbx-core list push.
  - `databaseApi` and `useRedisBrowser` wrap `dbx_redis_list_push`; existing `hash_set`, `set_add`, and `zadd` wrappers now back the selected-value add controls.
  - `RedisBrowser` now shows DBX-style inline add controls in the selected value member header:
    - `list`: value input plus `Push`, calling `RPUSH`.
    - `hash`: field/value inputs plus `Set`, calling `HSET`.
    - `set`: member input plus `Add`, calling `SADD`.
    - `zset`: score/member inputs plus `Add`, calling `ZADD`.
  - Empty loaded collection values now still show the member section and add controls, because the section is keyed off the selected Redis value type rather than existing row count.
  - Saving a new member reloads the selected Redis value and refreshes the current key preview so rows, counts, and key metadata stay in sync.
  - Added English and Chinese i18n labels for the inline add controls and validation messages.
- Expanded tests:
  - `src/test/database-api.test.ts` verifies Redis collection member add command payloads, including default `ttl: null` for add wrappers.
  - `src/test/database-redis-browser.test.tsx` verifies adding a hash field and pushing a list item from selected-value controls, backend payloads, reloads, and refreshed member rows.

Latest completed increment:

- Added DBX-style Redis member detail JSON controls.
  - JSON-valued Redis member details now open in a JSON view by default, showing pretty-printed JSON instead of only the compact raw string.
  - Member detail now includes JSON/raw view toggles for JSON values.
  - In raw mode, editable JSON members expose `Format member JSON` and `Compress member JSON` actions that move the member into edit mode with the formatted/compressed draft, matching dbx's member detail behavior.
  - While already editing a JSON member, the edit toolbar also exposes format/compress actions against the current draft.
  - Read-only connections can still view JSON/raw content but do not expose actions that would enter edit mode.
  - Added English and Chinese i18n labels for member JSON view and member-specific format/compress actions.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies JSON member detail default pretty view, raw view toggle, and format/compress actions entering member edit mode with the expected draft.
  - `src/test/database-view.test.tsx` was tightened to target the SQLite table node by exact `users` text before checking the truncate-hidden context menu; this keeps the existing SQLite truncate capability test aligned with the current toolbar containing `Users and permissions`.

Latest completed increment:

- Added DBX-style Redis member detail word-wrap preference.
  - JSON-valued Redis member details now expose a `Word wrap` toggle in the member-detail toolbar, matching dbx's JSON detail controls.
  - The toggle controls wrapping for JSON member detail previews, using `pre-wrap`/`break-word` when enabled and `pre`/normal breaking when disabled.
  - The preference is persisted under dbx's `dbx-redis-json-word-wrap` localStorage key and defaults to enabled when unset.
  - Added English and Chinese i18n labels for the word-wrap control.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies the default checked state, persisted `dbx-redis-json-word-wrap` value, and preview wrapping style changes from the Redis member detail control.

Latest completed increment:

- Added DBX-style Redis member detail side panel behavior.
  - Redis collection member details now open only after an explicit row/detail action instead of implicitly selecting the first member when a value loads.
  - The member detail is rendered as a fixed right-side panel, matching dbx's `Sheet`-style detail surface instead of an inline section below the member table.
  - The panel includes a left resize handle with DBX-style column-resize behavior and clamps its width to the viewport.
  - Existing member detail JSON/raw view, word-wrap, format/compress, copy, edit, save, and cancel behavior is preserved inside the side panel.
  - Added English and Chinese i18n labels for the resize handle.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies the member detail no longer auto-opens, the side panel renders as a fixed dialog, the resize handle changes panel width, and the close action dismisses the detail panel.

Latest completed increment:

- Added DBX-style Redis member detail JSON tree view.
  - Added a React `RedisJsonTree` renderer for JSON-valued Redis member details, matching dbx's tree-style JSON view instead of showing only pretty-printed raw text.
  - The tree renders object/array summaries, quoted object keys, array indexes, scalar values, and per-node expand/collapse controls.
  - The existing member-detail word-wrap preference now controls the JSON tree's wrapping behavior as well as raw text.
  - The raw tab still shows the original member JSON text and keeps the existing format/compress-to-edit workflow.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies JSON tree rendering, root collapse/expand behavior, word-wrap style propagation to the tree, and raw-tab fallback text before format/compress editing.

Latest completed increment:

- Added DBX-style Redis collection member column resizing.
  - Hash member tables now allow resizing the `Field` column via a draggable header handle, matching dbx's hash member grid behavior.
  - Sorted-set member tables now allow resizing the `Score` column via a draggable header handle, matching dbx's zset member grid behavior.
  - The column widths are clamped to practical min/max values and update through the same pointer-move lifecycle used by the member-detail side panel.
  - Added English and Chinese i18n labels for the resize handles.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies hash `Field` column resizing and sorted-set `Score` column resizing from the member table headers.

Latest completed increment:

- Added DBX-style Redis string JSON value tree/raw viewing.
  - String Redis values that contain valid JSON now open in a JSON view by default, matching dbx's string JSON viewer behavior.
  - The main selected value area now exposes JSON/raw toggles for JSON string values.
  - The JSON view reuses the Redis JSON tree renderer and the existing word-wrap preference.
  - The raw view keeps the editable textarea and exposes format/compress actions against the value draft, preserving the existing save/TTL workflow.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies default JSON tree rendering for a string JSON value, word-wrap propagation, raw view switching, and format/compress behavior against the raw value editor.

Latest completed increment:

- Added DBX-style Redis binary string readonly handling.
  - Binary Redis string values now render the main value editor as read-only, matching dbx's protection against corrupting non-text data.
  - A localized readonly hint is shown below the value editor for binary string values.
  - The bottom `Save value` action is disabled for binary string values, and `saveValue` now guards against binary string saves even if called programmatically.
  - Existing copy insert-statement feedback remains clickable and still shows the DBX-style binary-data message.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies binary Redis string readonly editor state, localized hint text, disabled format/compress/save actions, binary insert-statement feedback, and no value-save API call.

Latest completed increment:

- Added DBX-style Redis value draft discard handling.
  - The main Redis value toolbar now shows a localized `Discard` action when the selected value draft differs from the loaded Redis value.
  - Discard resets only the value draft and clears JSON-format errors, leaving the TTL draft untouched so the existing TTL/save flow is preserved.
  - JSON format/compress actions now also guard against read-only, missing, or binary selected values at the handler level.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies changing a Redis value draft reveals `Discard`, clicking it restores the loaded value without saving, keeps the TTL draft intact, and still allows the subsequent save path to work.

Latest completed increment:

- Added DBX-style Redis selected-key size metadata.
  - The selected Redis value header now shows a localized `Size` pill beside the key type and TTL metadata, matching dbx's Redis key metadata row.
  - String keys are formatted as bytes or KB, while collection keys show the scanned `size` count directly.
  - The size pill uses the current scanned key metadata, so it updates after value saves refresh the key preview.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies the selected string key shows `Size: 3 B` and updates to `Size: 5 B` after saving a longer value and refreshing the key preview.

Latest completed increment:

- Added DBX-style Redis member detail panel copy action.
  - The fixed right-side Redis member detail panel now includes its own copy button when not editing, matching dbx's member detail sheet footer action.
  - The panel copy action reuses the selected member's normalized copy text, so JSON/string member values copy the same content as the row-level copy action.
- Expanded tests:
  - `src/test/database-redis-browser.test.tsx` verifies the member detail panel contains a `Copy member` action scoped inside the dialog and copies the selected JSON member value.

Latest completed increment:

- Added DBX-style Mongo workspace table view state and column visibility parity.
  - `MongoBrowser` now persists the document/table view mode in localStorage using a DBX-compatible workspace behavior, so reopening the Mongo workspace restores the last selected view mode.
  - Mongo table mode now exposes a `Columns` visibility menu, using the existing `database.gridColumnVisibility` i18n labels.
  - Hidden Mongo table columns are omitted from headers and cells, the menu shows visible/total counts, `Show all` restores hidden columns, and the UI prevents hiding the final visible column.
  - Column visibility state is pruned when the loaded document columns change and the menu closes when switching document view, databases, or collections.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies persisted Mongo view mode across remounts and verifies hiding/restoring a Mongo table column updates headers, cells, and visible-column counts.

Latest completed increment:

- Added DBX-style Mongo workspace table view options for hiding null columns.
  - Mongo table mode now exposes a `View options` wrench menu beside the column visibility menu.
  - The `Hide null columns` option detects columns whose currently loaded documents are all `null`, `undefined`, or missing and omits them from table headers and cells.
  - Hide-null behavior composes with explicit column visibility, preserves at least one visible column, and closes its menu when switching document view, databases, or collections.
  - Added English and Chinese i18n labels for the view-options menu and hide-null option.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies a null/missing-only Mongo column is hidden when `Hide null columns` is enabled, non-null columns remain visible, and toggling the option off restores the column.

Latest completed increment:

- Added DBX-style Mongo table column visibility search and invert controls.
  - The Mongo table `Columns` popover now includes a localized search field, visible/total counter, no-match state, and DBX-style hint text.
  - Column visibility options filter by the typed column query without changing the underlying table columns.
  - Added an `Invert` action that flips the current visible/hidden column set while preserving at least one visible column and composing with the existing hide-null behavior.
  - Added localized grid labels for search columns, no matches, column visibility hint, invert, and show all.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies column-search filtering, no-match messaging, invert behavior, visible-count updates, and `Show all` restoration.

Latest completed increment:

- Added DBX-style Mongo structured document filter builder.
  - `MongoBrowser` now has a `Filter` popover for table/document workspaces that creates Mongo filter rules using field, condition, value, and AND/OR conjunction controls.
  - Structured rules support equals, not equals, contains, does not contain, greater than, less than, is null, and is not null, matching dbx's document filter modes.
  - Applying structured rules combines them with the existing manual Filter JSON input using `$and` when both are present, then uses the effective filter for load, paging, sorting, load more, refresh, and matching-document deletion.
  - Clearing filters resets the manual filter, structured filter, sort, and builder state.
  - Added English and Chinese i18n labels for the filter builder controls and modes.
- Expanded tests:
  - `src/test/database-mongo-browser.test.tsx` verifies a manual filter JSON plus a structured `contains` rule produces the expected Mongo `$and` filter with case-insensitive `$regex` options.

Latest completed increment:

- Added DBX-style SQL grid column view options parity.
  - `DatabaseView` now exposes the DBX grid `Columns` panel as a searchable column list instead of a flat button strip.
  - The SQL table grid can filter column options by name, invert the current visible/hidden column set, restore all columns, and hide currently loaded columns whose values are all `null`/`undefined`.
  - Column visibility now preserves at least one visible data column, including after invert, hide-null, and column-list pruning when the active grid result changes.
  - The implementation reuses the existing localized `database.gridSearchColumns`, `database.gridInvertColumnVisibility`, `database.gridShowAllColumns`, `database.gridHideNullColumns`, `database.gridNoSearchResults`, and `database.gridColumnVisibilityHint` labels.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies SQL grid column search, null-only column hiding, show-all restoration, and the existing column-hide/export behavior.

Latest completed increment:

- Added DBX-style SQL grid column resizing parity.
  - SQL table result headers now expose a narrow resize handle on each data column, matching dbx's draggable column-edge behavior.
  - Dragging a handle updates the column width with DBX-like min/max clamps and expands the table `min-width` so horizontal scrolling can preserve the resized layout.
  - Column widths are reset when switching to a different DBX object and pruned when the active result set no longer contains a resized column.
  - Added localized resize labels for the handle.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies dragging the `email` column resize handle updates the rendered header width while preserving the existing grid filtering, sorting, search, and column visibility behavior.

Latest completed increment:

- Added DBX-style SQL grid column auto-fit parity.
  - Double-clicking a SQL grid column resize handle now auto-fits that column from the column label and currently loaded cell values, matching dbx's resize-handle double-click behavior.
  - Auto-fit uses the same min/max width clamps and column-width state as manual drag resizing.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies dragging the `email` column wider and then double-clicking the same resize handle auto-fits it back to the loaded content width.

Latest completed increment:

- Added DBX-style SQL grid initial column width parity.
  - New SQL grid result sets now initialize each data column width from the column label and currently loaded row values instead of using a fixed width for every column.
  - Initial widths reuse the same estimation and min/max clamp path as double-click auto-fit.
  - User-resized widths are preserved while paging, sorting, or filtering the same object; switching to another DBX object reinitializes widths from that object's result.
  - Resetting the active grid clears manual widths and reloads the DBX-estimated initial widths.
- Expanded tests:
  - `src/test/database-view.test.tsx` now verifies the `email` column starts at the DBX-estimated content width before manual resize and auto-fit interactions.

Latest completed increment:

- Added DBX-style SQL grid sortable-column parity.
  - `DatabaseView` now preserves `dbx_query_table_data.result.column_sortables` on `queryResult`.
  - SQL grid headers only render the sort button when the corresponding DBX result column is sortable, matching dbx `headerColumnSortable`.
  - Sort actions now no-op for columns explicitly marked non-sortable, while columns without sortable metadata still default to sortable like dbx.
- Expanded tests:
  - `src/test/database-view.test.tsx` marks the `status` column non-sortable and verifies its header keeps only the resize handle, not a sort control.

## Latest Verification

Latest verified commands after the DBX SQL grid sortable-column parity work:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

The broader database test command exercised the database test set and passed with 34 files / 376 tests.
`pnpm lint`, `git diff --check`, `pnpm build`, and `cargo check` passed.
`pnpm build` printed the expected chunks-larger-than-500-kB warning.
The expected `--localstorage-file` and jsdom canvas `getContext()` warnings were printed during tests.
No Rust files were changed in this increment, but `cargo check` was rerun because the current dirty worktree still contains existing Rust parity changes.

Previous checkpoint after the DBX Redis collection member column resizing work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "hash member rows|sorted set score column|member column"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis member-table command and the broader database test command both exercised the database test set and passed with 34 files / 370 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis member detail JSON tree work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "hash member rows|member detail|JSON tree"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis member-detail command and the broader database test command both exercised the database test set and passed with 34 files / 369 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis member detail side panel work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "hash member rows|member detail|word wrap"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis member-detail command and the broader database test command both exercised the database test set and passed with 34 files / 369 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis member detail word-wrap preference work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "hash member rows|word wrap|member detail"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis member-detail command and the broader database test command both exercised the database test set and passed with 34 files / 369 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis member detail JSON controls work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "hash member rows|member JSON|member detail"
pnpm test -- src/test/database-view.test.tsx -t "hides DBX truncate table"
pnpm test -- src/test/database-redis-browser.test.tsx src/test/database-view.test.tsx
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis/UI command, the focused DatabaseView rerun, and the combined RedisBrowser/DatabaseView test command exercised the database test set and passed with 34 files / 369 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis collection member creation work passed:

```bash
pnpm test -- src/test/database-api.test.ts -t "member add|list_push|collection member add"
pnpm test -- src/test/database-redis-browser.test.tsx -t "adds a Redis hash field|pushes a Redis list item|member controls"
pnpm test -- src/test/database-api.test.ts src/test/database-redis-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

At that checkpoint the focused Redis/UI/API commands and the combined API/Redis browser test command exercised the database test set and passed with 34 files / 369 tests. `pnpm lint`, `git diff --check`, `pnpm build`, and `cargo check` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis collection member editing work passed:

```bash
pnpm test -- src/test/database-api.test.ts -t "member edit|collection member edit|redis_hash_set|redis_list_set"
pnpm test -- src/test/database-redis-browser.test.tsx -t "edits a Redis hash field|deletes a Redis hash field|collection members|hash member"
pnpm test -- src/test/database-api.test.ts src/test/database-redis-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

At that checkpoint the focused Redis/UI/API commands and the combined API/Redis browser test command exercised the database test set and passed with 34 files / 366 tests. `pnpm lint`, `git diff --check`, `pnpm build`, and `cargo check` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis collection member deletion work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "hash field|member row action|collection members"
pnpm test -- src/test/database-api.test.ts -t "collection member delete|redis_load_more|member delete commands"
pnpm test -- src/test/database-api.test.ts src/test/database-redis-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

At that checkpoint the focused Redis/UI/API commands and the combined API/Redis browser test command exercised the database test set and passed with 34 files / 364 tests. `pnpm lint`, `git diff --check`, `pnpm build`, and `cargo check` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis collection member pagination work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "collection members|selected value cursor|hash member rows"
pnpm test -- src/test/database-api.test.ts -t "redis_load_more|collection pagination"
pnpm test -- src/test/database-view.test.tsx -t "hides DBX truncate table"
pnpm lint
git diff --check
pnpm build
cargo check
```

At that checkpoint the focused Redis/UI/API commands and the database-view rerun exercised the database test set and passed with 34 files / 362 tests. `pnpm lint`, `git diff --check`, `pnpm build`, and `cargo check` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis collection member viewing work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "hash member rows|Redis hash member"
pnpm test -- src/test/database-redis-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis member-row command and the full Redis browser test command both exercised the database test set and passed with 34 files / 360 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis workspace `Fetch all` progress/stop work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "Fetch all|fetch-all|remaining Redis key pages"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis fetch-all test command exercised the database test set and passed with 34 files / 359 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX Redis binary insert-statement copy feedback work passed:

```bash
pnpm test -- src/test/database-redis-browser.test.tsx -t "insert statement"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused Redis insert-statement test command exercised the database test set and passed with 34 files / 358 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX connection group move entry labeling work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "connection context menu|connection group"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused connection context-menu/group test command exercised the database test set and passed with 34 files / 357 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX connection context-menu order and duplicate parity work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "connection context menu|Duplicate Connection"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused connection context-menu order/duplicate test command exercised the database test set and passed with 34 files / 357 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX connection context-menu open/close parity work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "connection context menu|Open connection|Close connection"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused connection context-menu test command exercised the database test set and passed with 34 files / 357 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX user-admin sidebar utility node parity work passed:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx -t "user admin|Users and permissions|sidebar utility node"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused user-admin sidebar test command exercised the database test set and passed with 34 files / 357 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX NoSQL sidebar context-menu shape parity work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "NoSQL database and collection|pins and unpins NoSQL|sets and clears default databases|flushes a Redis database|MongoDB document previews"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused NoSQL menu test command exercised the database test set and passed with 34 files / 354 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX database/schema node context-menu parity work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "orders and gates|DuckDB database node"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused database/schema menu test command exercised the database test set and passed with 34 files / 354 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX truncate-table capability gating work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "DBX table administration|do not support truncate"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused truncate-capability test command exercised the database test set and passed with 34 files / 352 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX routine-like object menu-shape parity work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "DBX routine objects|DBX sequence object"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused routine-menu test command exercised the database test set and passed with 34 files / 351 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX table-node destructive menu-order parity work passed:

```bash
pnpm test -- src/test/database-view.test.tsx -t "duplicates DBX table structure"
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused table-menu test command exercised the database test set and passed with 34 files / 350 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous checkpoint after the DBX table/view context-menu shape parity work passed:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

At that checkpoint the focused database test command passed with 34 files / 350 tests. `pnpm lint`, `git diff --check`, and `pnpm build` passed. `pnpm build` printed the expected chunks-larger-than-500-kB warning.

Previous full checkpoint after the DBX PostgreSQL create-role parity work passed:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

At that checkpoint the focused database test command passed with 34 files / 300 tests. `pnpm lint`, `git diff --check`, and `cargo check` passed. `pnpm build` succeeded and printed the expected chunks-larger-than-500-kB warning.

Latest completed increment:

- Added DBX-style SQL grid column type metadata badges.
  - `DbQueryResult` now preserves `dbx_query_table_data.result.column_types` as `columnTypes`.
  - SQL grid headers render compact type badges such as `integer` and `text` next to the column label.
  - Type badges are visual metadata and keep sortable header accessible names as the raw column name.
  - Added English and Chinese i18n labels for the column type tooltip.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies SQL grid header type badges for DBX result column metadata while preserving the non-sortable header behavior.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Vitest passed with 34 files / 376 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

Latest completed increment:

- Added DBX-style SQL grid selected-row copy.
  - SQL grid selected rows can now be copied from the toolbar without requiring edit permissions.
  - The copied payload is TSV with the current visible column headers and visible column order.
  - Hidden columns are omitted from the copied payload, matching the current grid view.
  - TSV cells replace tab/newline characters with spaces and keep nulls consistent with the grid display as `NULL`.
  - Added English and Chinese i18n labels for the selected-row copy action.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies selecting all visible DBX grid rows and copying them writes the expected TSV payload to the clipboard before the existing selected-row delete flow.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Vitest passed with 34 files / 376 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

Latest completed increment:

- Added DBX-style SQL grid keyboard copy for selected rows.
  - The DBX SQL grid wrapper is now focusable and handles `Cmd/Ctrl+C` when rows are selected.
  - Keyboard copy reuses the same TSV payload as the toolbar `Copy selected` action.
  - Text inputs, textareas, selects, and content-editable targets keep their native copy behavior so cell editing is not interrupted.
  - Added an accessible grid wrapper label in English and Chinese.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies `Cmd+C` from the selected-row checkbox copies the expected TSV payload, then verifies the toolbar copy path still works.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Vitest passed with 34 files / 376 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

Latest completed increment:

- Added DBX-style SQL grid cell preview copy actions.
  - The DBX SQL grid cell preview dialog now includes copy actions for the formatted cell value and the column name.
  - JSON cell previews keep the existing pretty-printed formatting when copied.
  - Added English and Chinese i18n labels for copying the column name.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the JSON cell preview dialog copies the pretty-printed cell value and then copies the source column name.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Vitest passed with 34 files / 376 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

Latest completed increment:

- Added DBX-style SQL grid cell context menus.
  - Right-clicking a DBX SQL grid data cell now opens a context menu with copy value, copy column name, and preview value actions.
  - Copy value uses the same formatting as the cell preview dialog, so JSON values copy as pretty-printed JSON.
  - Preview value opens the existing cell preview dialog from the right-click menu.
  - Added English and Chinese i18n labels for the preview-value menu item.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the DBX JSON cell right-click menu copies the formatted value, copies the column name, and opens the preview dialog.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Vitest passed with 34 files / 376 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

Latest completed increment:

- Added DBX-style SQL grid row copy actions to the DBX grid cell context menu.
  - Right-clicking a DBX SQL grid data cell now also supports `Copy Row (JSON)`, `Copy as INSERT`, `Copy as INSERT without Primary Keys`, and `Copy as UPDATE`.
  - Row actions follow dbx selection behavior: if the right-clicked row is selected, actions apply to the selected row set; otherwise they apply to the clicked row.
  - Copy payloads use the visible grid columns and current visible column order.
  - INSERT/UPDATE SQL is generated through thin Tauri wrappers over `dbx_core::data_grid_sql`, not frontend SQL string building.
  - `Copy as INSERT without Primary Keys` is shown when primary keys are known; `Copy as UPDATE` is disabled when no primary key is available.
  - Added English and Chinese i18n labels for the row copy actions.
- Added Rust/API/type coverage:
  - `src-tauri/src/database/grid.rs` exposes `dbx_build_data_grid_copy_insert_statement` and `dbx_build_data_grid_copy_update_statements`.
  - `src-tauri/src/lib.rs` registers both Tauri commands.
  - `src/lib/databaseApi.ts`, `src/types/database.ts`, and `src/types.ts` expose the corresponding frontend wrappers/types.
- Expanded tests:
  - `src/test/database-api.test.ts` verifies the new invoke wrappers and payloads.
  - `src/test/database-view.test.tsx` verifies JSON row copy, INSERT copy, INSERT without primary keys, UPDATE copy, and the existing cell copy/preview actions.
  - Rust unit tests verify the copy INSERT/UPDATE builders call dbx core behavior.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Vitest passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed.

Latest completed increment:

- Added DBX-style SQL grid `Copy All (TSV)` to the DBX grid cell context menu.
  - This fills the remaining fixed item from dbx `DataGrid.vue`'s copy submenu after the cell/row/INSERT/UPDATE copy actions.
  - The action copies the current loaded grid page as TSV with visible columns in the current visible column order.
  - TSV formatting reuses the existing DBX grid TSV helper, including the header row and display text for cell values.
  - Added English and Chinese i18n labels for `Copy All (TSV)`.
- Expanded tests:
  - `src/test/database-view.test.tsx` now uses a two-row DBX JSON grid fixture and verifies `Copy All (TSV)` copies both visible rows while the current-row JSON/INSERT/UPDATE actions still target the right-clicked row.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed.

Latest completed increment:

- Added DBX-style multi-row copy labels to the SQL grid cell context menu.
  - When the right-clicked DBX grid row is part of the selected row set, row-copy menu items now switch to the dbx plural labels:
    - `Copy {count} Rows (JSON)`
    - `Copy {count} Rows as INSERT`
    - `Copy {count} Rows as INSERT without Primary Keys`
    - `Copy {count} Rows as UPDATE`
  - Single-row context menus keep the existing single-row labels.
  - The labels match the already-implemented behavior where row-copy actions apply to selected rows if the right-clicked row is selected.
  - Added English and Chinese i18n labels for the plural row-copy menu items.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the two-selected-row context menu shows the plural DBX labels and `Copy 2 Rows (JSON)` copies the selected row set.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "selected row deletes"
pnpm lint
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed.

Latest completed increment:

- Added a DBX-style SQL grid row details dialog.
  - The DBX grid cell context menu now includes `Open Row Details`, matching dbx `DataGrid.vue`'s row detail entry.
  - The dialog is titled `Row {row} Details`, shows the visible columns in the current visible column order, and displays each column's type and formatted value.
  - JSON-like cell values use the same formatted preview helper as the cell preview dialog.
  - The dialog includes `Copy Row (JSON)` and `Copy Row (TSV)` actions using the existing DBX grid row copy helpers.
  - Added English and Chinese i18n labels for row details, column count, field index, and row TSV copy.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies opening row details from the DBX JSON cell context menu, checks the displayed row metadata/value preview, and verifies row JSON/TSV copy actions.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm lint
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed.

Latest completed increment:

- Extended the DBX-style SQL grid row details dialog with dbx row-detail controls.
  - Added a row-detail search input that filters fields by column name, type, or displayed value.
  - Added the DBX-style no-match empty state for filtered row details.
  - Added per-field copy buttons with accessible labels such as `Copy profile value`.
  - Field copy uses the displayed/formatted value shown in the row details dialog.
  - Added English and Chinese i18n labels for row-detail search, no-match text, and per-field copy.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies searching row details filters out non-matching fields and that `Copy profile value` copies the formatted JSON value.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm lint
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed.

Latest completed increment:

- Added a DBX-style SQL grid column details dialog.
  - The DBX grid cell context menu now includes `Open Column Details`, matching dbx `DataGrid.vue`'s column detail entry.
  - The dialog is titled `{column} Column Details`, shows the selected column name, column type, and row count, and lists values from the currently loaded/visible grid rows.
  - Column details include a search input that filters by displayed value or row number and shows the existing DBX-style no-match empty state.
  - Each row value has a per-row copy button, using the same formatted display text as the grid preview helpers.
  - Footer actions copy the column as pretty JSON `[{ row, value }, ...]`, copy the column as TSV, copy the column name, or close the dialog.
  - Added English and Chinese i18n labels for opening column details, column details metadata, column JSON/TSV copy, and per-row value copy.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies opening column details from the DBX JSON cell context menu, searching the column values, per-row value copy, column JSON/TSV copy, and column-name copy.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm lint
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed.

Latest completed increment:

- Added DBX-style SQL grid header context menu parity.
  - Right-clicking a DBX SQL grid data-column header now opens a header-specific context menu, matching dbx `DataGrid.vue`'s `contextHeaderColumn` behavior.
  - Header menus expose `Copy column name` and `Open Column Details`.
  - Header menus intentionally omit cell/row copy entries such as `Copy value`, matching dbx's header-menu shape.
  - `Open Column Details` reuses the existing DBX-style column details dialog, including type metadata, row counts, search, per-row copy, JSON/TSV copy, and column-name copy.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies the profile column header context menu copies the column name, does not show cell-only copy entries, and opens the column details dialog.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

Latest completed increment:

- Added DBX-style SQL grid header sort menu parity.
  - DBX SQL grid data-column header context menus now include `Sort ascending` and `Sort descending`, matching dbx `DataGrid.vue`'s header menu sort actions.
  - When the grid currently has an `ORDER BY`, header menus also include `Clear sort`, matching dbx's clear-sort path.
  - Sort actions respect DBX `column_sortables`; non-sortable columns keep sort menu items disabled.
  - Sort menu actions update the `ORDER BY` input and reload the first page of the active DBX table grid.
  - Added English and Chinese i18n labels for `Clear sort`.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies header-menu ascending sort, descending sort, and clear sort actions by asserting the `dbx_query_table_data` `orderBy` payloads.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

Latest completed increment:

- Added DBX-style SQL grid cell context filter actions.
  - DBX SQL grid cell context menus now expose filter actions matching dbx `DataGrid.vue`'s context filter modes:
    - `Filter by This Value`
    - `Exclude This Value`
    - `Contains Value`
    - `Does Not Contain Value`
    - `Less Than Value`
    - `Greater Than Value`
    - `Show NULL Values`
    - `Show Non-NULL Values`
    - `Clear filter`
  - Filtering calls a new thin Tauri wrapper, `dbx_build_data_grid_context_filter_condition`, so DBX/dbx-core builds the SQL condition instead of the React layer hand-writing dialect-specific SQL.
  - Applying a context filter combines it with any existing `WHERE` input as `(existing) AND (condition)`, then reloads page 1 of the active DBX grid.
  - Clearing the filter resets the DBX grid `WHERE` input and reloads page 1 while preserving the current `ORDER BY`.
  - Added English and Chinese i18n labels for the DBX context filter actions.
- Added Rust/API/type coverage:
  - `src-tauri/src/database/grid.rs` exposes `dbx_build_data_grid_context_filter_condition`.
  - `src-tauri/src/lib.rs` registers the command.
  - `src/lib/databaseApi.ts`, `src/types/database.ts`, and `src/types.ts` expose the typed frontend wrapper.
- Expanded tests:
  - `src/test/database-api.test.ts` verifies the new invoke wrapper and payload.
  - `src/test/database-view.test.tsx` verifies filtering a DBX JSON cell by value calls the dbx-core wrapper and reloads `dbx_query_table_data` with the generated `whereInput`; it also verifies `Clear filter` reloads with `whereInput: null`.
  - Rust unit tests verify the context filter builder calls dbx-core behavior.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm test -- src/test/database-api.test.ts -t "row copy SQL builders"
cargo test builds_context_filter_condition_with_dbx_core
cargo fmt --check
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Targeted Vitest path passed with 34 files / 377 tests.
- API wrapper Vitest path passed with 34 files / 377 tests.
- Rust focused unit test passed.
- `cargo fmt --check` passed.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed.

Latest completed increment:

- Added DBX-style SQL grid cell context sort menu parity.
  - DBX SQL grid cell context menus now include `Sort ascending` and `Sort descending`, matching dbx `DataGrid.vue`'s cell context menu sort actions.
  - When the grid currently has an `ORDER BY`, cell context menus also include `Clear sort`, matching dbx's clear-sort path.
  - Sort actions respect DBX `column_sortables`; non-sortable columns keep sort menu items disabled.
  - Sort menu actions update the `ORDER BY` input and reload the first page of the active DBX table grid while preserving the current `WHERE` input.
- Expanded tests:
  - `src/test/database-view.test.tsx` verifies cell-menu ascending sort, descending sort, and clear sort actions by asserting the `dbx_query_table_data` `orderBy` payloads.

Verification that passed after this increment:

```bash
pnpm test -- src/test/database-view.test.tsx -t "previews DBX grid JSON cell values"
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

Result:

- Targeted DBX grid Vitest path passed with 34 files / 377 tests before this handoff update.
- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.
- `cargo check` passed. No Rust files were changed in this increment, but it was rerun because the current dirty worktree still contains existing Rust parity changes.

## Immediate Next Steps For The Next Agent

The latest checkpoint is verified. If resuming after more external changes, rerun:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
cargo check
```

If failures appear, likely areas to inspect:

- `DatabaseSidebarTree` new required props may need default wiring in any tests that render the component directly, including `redisScanStateByDatabase`, `onLoadMoreRedisKeys`, `onRedisKeyContextMenu`, `mongoDocumentTotalsByCollection`, `onLoadMoreMongoDocuments`, and `onMongoDocumentContextMenu`.
- `DatabaseView` Redis/Mongo connection flow now calls `dbxConnect` plus NoSQL list APIs; existing tests with Redis/Mongo mocked connections may need mocks for `dbx_redis_list_databases`, `dbx_mongo_list_databases`, `dbx_mongo_list_collections`, `dbx_redis_scan_keys`, or `dbx_mongo_find_documents`.
- Active NoSQL node highlighting uses `activeDbxDatabase` as `dbN` for Redis and database name for Mongo; Mongo collection active state uses `activeDbxSchema`, and Mongo document active state uses `activeMongoDocumentId`.
- `activeMongoWorkspaceDatabase` is intentionally separate from `activeDbxDatabase`: opening a Mongo connection may highlight the first database in the sidebar, but only explicit Mongo sidebar selections are passed into `MongoBrowser` to avoid eager collection loading.

After verification, continue implementing the prompt. Good next targets:

1. Continue full NoSQL context menu parity for remaining Redis/Mongo actions that dbx exposes beyond the current copy/refresh/open/insert/delete coverage.
2. Continue deeper NoSQL browsing parity such as richer Mongo sidebar child-node behavior beyond the current filter-aware preview pagination.
3. Continue tree refresh per NoSQL node beyond current safe cases if dbx exposes additional refresh targets not yet represented.
4. Continue right-click menu parity for SQL nodes: remaining dbx menu items and deeper user-management behavior beyond the initial MySQL/PostgreSQL workspace.
5. Continue connection dialog parity: complete transport layers and remaining profile-specific fields.
6. Continue data grid parity: column hide/show UX, filtering/search controls, SQL INSERT/UPDATE/XLSX export depth, CSV/XLSX import mapping polish.
7. Continue workspace parity: true multi-tab model for query/data/object browser/SQL file/Redis/Mongo/search/export/diagram.
8. Add or expand tests as each item is implemented.

## Current Completion Status

Do not mark the persistent goal complete.

The prompt remains broad and unfinished. Current work is meaningful progress, especially around backend SQL wrappers, sidebar tree extraction, SQL object browsing, grid editing/export/import, visible database selection, and initial NoSQL tree rendering, but the full dbx parity acceptance criteria are not yet satisfied.
