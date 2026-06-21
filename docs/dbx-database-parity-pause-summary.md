# DBX Database Parity Pause Summary

Last updated: 2026-06-19

## Total Task

Continue implementing `docs/dbx-database-parity-prompt.md` and `docs/dbx-database-parity-handoff.md`.

The overall goal is to make Aeroric's database module match dbx's database module in behavior, entry points, tree browsing, context menus, connection dialog, data grid, import/export, search, ER diagram, Redis/Mongo browsing, styles, i18n, and tests.

Do not mark the overall parity task complete. The prompt remains broad and unfinished.

## Last Fully Verified Checkpoint

The latest completed and fully verified checkpoint is:

- DBX-style connection dialog TLS mode parity and data grid enhancements.

What was completed:

- **Connection dialog TLS mode dropdowns**:
  - Added PostgreSQL SSL mode dropdown with options: Disable, Prefer, Require, Verify CA, Verify Full.
  - Added MySQL TLS mode dropdown with options: Preferred, Disabled, Required, Verify CA, Verify Identity.
  - TLS mode is saved in `url_params` as `sslmode` (PostgreSQL) or `ssl-mode` (MySQL).
  - TLS mode is loaded from `url_params` when editing existing connections.

- **Data grid Refresh toolbar button**:
  - Added a dedicated `Refresh` button to the SQL data grid toolbar.
  - The button reloads the current page for both DBX and legacy connections.

- **SQL preview panel**:
  - Added a `Preview SQL` button to the grid toolbar that opens a side panel showing the last generated SQL statements.
  - The panel displays the SQL statements and rollback SQL for cell updates, insert rows, and delete rows.
  - Cell edits, insert row, and delete row operations now store their SQL in the preview panel state.

- **Data grid custom page size**:
  - Added a `Custom...` option to the page size dropdown that prompts for a custom value.
  - Custom page size values are validated (1-10000) and applied immediately.
  - Non-standard page sizes are displayed in the dropdown when active.

- **Import preview full rows**:
  - Table import preview now shows all rows instead of limiting to 5 rows.
  - Users can see the complete preview data before importing.

- **Export selected rows**:
  - Added `Export selected (N)` button that appears when rows are selected in the grid.
  - Builds a WHERE clause from selected rows' primary key values.
  - Supports null primary keys with `IS NULL` conditions.

- **Export progress dialog**:
  - Added a progress dialog that shows during export operations.
  - Displays the export format and target file path with loading spinner.

- **Data grid export selected rows**:
  - Added `Export selected (N)` button that appears when rows are selected in the grid.
  - Builds a WHERE clause from selected rows' primary key values.
  - Supports null primary keys with `IS NULL` conditions.
  - Uses OR-combined conditions for multiple selected rows.

- **Data grid export progress dialog**:
  - Added a progress dialog that shows during export operations.
  - Displays the export format and target file path.
  - Shows a loading spinner while the export is in progress.

- **Connection test result in dialog footer**:
  - Connection test results now display inline in the dialog footer instead of the global error banner.
  - Success results show in green, error results show in red.
  - Results persist until the next test or dialog close.

- **Default database indicator in sidebar**:
  - MongoDB database nodes now show a "Default" badge when they are the configured default database.
  - Redis database nodes now show a "Default" badge when they are the configured default database.
  - The badge uses the accent color and appears after the database name.

- **Oracle connection dialog parity**:
  - Added Oracle connection type toggle (Service Name vs SID) in the connection dialog.
  - Added SYSDBA checkbox for Oracle connections.
  - Oracle-specific fields are saved in the connection config and loaded when editing.
  - Added i18n labels for Oracle connection type and SYSDBA in English and Chinese.

- **Locate active tab in sidebar**:
  - Added a "Locate active tab" button (crosshair icon) in the sidebar header next to the search box.
  - Clicking the button selects and highlights the active tab's corresponding node in the sidebar tree.
  - Supports locating connection, database, schema, and object nodes.

- **i18n additions**:
  - Added `connection.postgresSslMode*` and `connection.mysqlTlsMode*` labels in English and Chinese.
  - Added `database.gridPreviewStatements` label in English and Chinese.

- **Previous increment (context menu parity)**:
  - Table/View: Export structure, Copy structure DDL, Refresh.
  - Database/Schema: Open object browser, Create table, Close database connection.
  - Column: Open field lineage.
  - All table/view menu items now match dbx order and coverage.

- **Database/Schema context menu parity**:
  - Added `Open object browser` action for databases/schemas, opening the object browser workspace.
  - Added `Create table` action for databases/schemas, opening the query workspace with a CREATE TABLE draft.
  - Added `Close database connection` action for databases, disconnecting the DBX connection.
  - Database and schema menus now match dbx's full action set.

- **Column context menu parity**:
  - Added `Open field lineage` action for columns, opening the field lineage workspace.
  - Column menus now match dbx's column-specific actions.

- **i18n additions**:
  - Added `database.exportStructure`, `database.copyStructureDdl`, `database.openObjectBrowser`, `database.createTable`, `database.closeDatabaseConnection`, `database.openFieldLineage` labels in English and Chinese.

- **TypeScript type updates**:
  - Added `object-browser` and `field-lineage` to `DbWorkspaceMode` type.

Verification that passed after this checkpoint:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

Result:

- Focused database Vitest command passed with 34 files / 377 tests.
- `pnpm lint` passed with 0 errors and 0 warnings.
- `git diff --check` passed.
- `pnpm build` passed with the expected chunks-larger-than-500-kB warning.

## Previous Verified Checkpoint

The previous completed checkpoint was:

- DBX-style SQL grid cell context sort menu parity.

- DBX-style SQL grid cell context filter actions.

What was completed there:

- DBX SQL grid cell context menus expose DBX filter actions:
  - `Filter by This Value`
  - `Exclude This Value`
  - `Contains Value`
  - `Does Not Contain Value`
  - `Less Than Value`
  - `Greater Than Value`
  - `Show NULL Values`
  - `Show Non-NULL Values`
  - `Clear filter`
- Filtering calls the Tauri wrapper `dbx_build_data_grid_context_filter_condition`, so dbx-core builds the SQL condition.
- Applying a context filter combines it with any existing `WHERE` input as `(existing) AND (condition)` and reloads page 1.
- Clearing the filter resets the DBX grid `WHERE` input and reloads page 1 while preserving the current `ORDER BY`.
- English and Chinese i18n labels were added for the context filter actions.
- Tests cover the frontend API wrapper payload, DBX grid filter application, clear-filter reload, and Rust dbx-core filter-builder behavior.

## Current In-Progress Work

No new coding increment is recorded after the verified cell context sort checkpoint.

## Immediate Next Steps For Next Agent

1. Continue implementing the broader parity prompt from `docs/dbx-database-parity-prompt.md` and the next targets listed in `docs/dbx-database-parity-handoff.md`.
2. Preserve the dirty worktree and do not revert unrelated changes.
3. Before marking a new frontend increment complete, rerun:

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

4. If Rust files are touched, or if broad validation is needed while Rust parity work remains dirty, also run:

```bash
cargo check
```

## Good Follow-Up Targets

Continue with the remaining parity work from the prompt and handoff:

1. Continue full NoSQL context menu parity for remaining Redis/Mongo actions beyond current copy/refresh/open/insert/delete coverage.
2. Continue richer Mongo sidebar/document behavior beyond current filter-aware preview pagination.
3. Continue tree refresh per NoSQL node where dbx exposes additional refresh targets.
4. Continue SQL node right-click menu parity for remaining dbx menu items.
5. Continue user-management behavior beyond the initial MySQL/PostgreSQL workspace.
6. Continue connection dialog parity, especially transport layers and remaining profile-specific fields.
7. Continue data grid parity: column hide/show UX, filtering/search controls, SQL INSERT/UPDATE/XLSX export depth, CSV/XLSX import mapping polish.
8. Continue workspace parity: true multi-tab model for query/data/object browser/SQL file/Redis/Mongo/search/export/diagram.

## Important Constraints

- Preserve the dirty worktree. Many modified/untracked files are ongoing DBX parity work.
- Do not revert unrelated changes.
- Use Aeroric React/hooks/Tauri patterns, not dbx Vue/Pinia architecture.
- Prefer Tauri/dbx-core wrappers for SQL builders already implemented in dbx-core.
- High-risk database mutations must preview generated SQL and ask for confirmation where applicable.
- Expected Vitest warnings: `--localstorage-file` and jsdom canvas `getContext()`.
- Expected build warning: chunks larger than 500 kB.
- Run `cargo check` when Rust files are touched. Current dirty Rust parity work exists, so rerunning `cargo check` during validation is still useful.

## Current Dirty Worktree Notes

Known ongoing dirty/untracked files include:

- `src-tauri/Cargo.toml`
- `src-tauri/src/database/connections.rs`
- `src-tauri/src/database/grid.rs`
- `src-tauri/src/database/query.rs`
- `src-tauri/src/database/redis.rs`
- `src-tauri/src/database/schema.rs`
- `src-tauri/src/database/types.rs`
- `src-tauri/src/lib.rs`
- `src/components/database/DatabaseView.tsx`
- `src/components/database/MongoBrowser.tsx`
- `src/components/database/RedisBrowser.tsx`
- `src/components/database/DatabaseSearchPanel.tsx`
- `src/components/database/DatabaseSidebarTree.tsx`
- `src/components/database/DatabaseUserAdminPanel.tsx`
- `src/hooks/useMongoBrowser.ts`
- `src/hooks/useRedisBrowser.ts`
- `src/i18n.tsx`
- `src/lib/databaseApi.ts`
- `src/lib/redisCommandSafety.ts`
- `src/lib/redisCommandSession.ts`
- `src/lib/redisKeyPattern.ts`
- `src/lib/redisKeyTree.ts`
- `src/styles/database.ts`
- `src/test/database-api.test.ts`
- `src/test/database-mongo-browser.test.tsx`
- `src/test/database-redis-browser.test.tsx`
- `src/test/database-sidebar-tree.test.tsx`
- `src/test/database-view.test.tsx`
- `src/types.ts`
- `src/types/database.ts`
- `docs/dbx-database-parity-handoff.md`
- `docs/dbx-database-parity-pause-summary.md`

Treat the dirty worktree as active parity work. Do not clean, reset, or revert it unless the user explicitly asks.
