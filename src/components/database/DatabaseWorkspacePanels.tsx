/**
 * `<main>` 里那条「按 `workspaceMode` 挑一个面板」的三元链:查询历史 / SQL 文件 / 驱动管理 /
 * Redis / Mongo / 数据传输 / 结构对比 / 数据对比 / 用户管理 / ER 图 / 库内搜索 / 表结构 / 表信息,
 * 以及每一档「连接不合格」时对应的那块 `GuidancePanel`。
 *
 * 从 `DatabaseView.tsx` 抽出:原文里这一整条链就是连续的一段(在工作区顶栏之后、网格那一叠之前),
 * 纯展示 —— 不持有状态、不含任何 hook,只把上层算好的值分发给对应面板。
 *
 * 链尾那条 `: (` 分支(网格那一叠)通过 `fallback` 传进来,而不是在这里内联或者在
 * `DatabaseView` 里重算一遍条件:整条链的判定顺序、短路行为都必须逐字保留一份,复制成两处
 * 早晚会漂。`fallback` 里的元素在调用方是提前构造好的,这不会有副作用 —— JSX 里没有 hook,
 * 没挂载的元素也不跑 effect。
 *
 * 逐字保留的几处:
 * - 整条链的**顺序**就是优先级。同一个 mode 往往连着两条(先「连接不合格 → GuidancePanel」,
 *   再「合格 → 真面板」),前一条的否定条件不能省。
 * - `table-structure` 有三档:连接不合格、没选中表、选中了表。中间那档单独存在,不能和第一档合并
 *   (两者文案相同但条件不同)。
 * - `TableInfoPanel` 那条的 `activeDbxConnection!` 是原文里的非空断言 —— 前一条已经把
 *   `!activeDbxConnection` 的情况接走了,这里靠链的顺序保证非空。
 * - Redis 的 `initialDb` 只在 `activeDbxDatabase` 形如 `db<N>` 时才解析,否则 `undefined`。
 * - Mongo 的 `initialCollection` / `initialDocumentId` 都以 `activeMongoWorkspaceDatabase` 存在为前提。
 * - `onDocumentsQueryApplied` 里的 key 是 `连接 id:库:集合` 三段拼的,先写查询再拉文档,顺序不能反。
 */

import type { ReactNode } from "react";

import { useI18n } from "../../i18n";
import type {
  AeroricDbConnectionConfig,
  DatabaseDriverManifest,
  DbxColumnInfo,
  DbxObjectInfo,
} from "../../types";
import { DatabaseAdvancedTools } from "./DatabaseAdvancedTools";
import { DatabaseSearchPanel } from "./DatabaseSearchPanel";
import { DatabaseUserAdminPanel, supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";
import { GuidancePanel } from "./DatabaseViewPrimitives";
import { dbxConfigRecord, dbxString } from "./databaseConnectionDraft";
import type {
  DbWorkspaceMode,
  MongoSidebarDocumentQuery,
  QueryHistoryEntry,
} from "./databaseViewModel";
import { DriverManagerPanel } from "./DriverManagerPanel";
import { ErDiagramPanel } from "./ErDiagramPanel";
import { MongoBrowser } from "./MongoBrowser";
import { QueryHistoryPanel } from "./QueryHistoryPanel";
import { RedisBrowser } from "./RedisBrowser";
import { SqlFilePanel } from "./SqlFilePanel";
import { TableInfoPanel } from "./TableInfoPanel";
import { TableStructurePanel } from "./TableStructurePanel";
import type { SqlFilePanelState } from "./useSqlFilePanel";
import type { TableInfoPanelState } from "./useTableInfoPanel";

export interface DatabaseWorkspacePanelsProps {
  workspaceMode: DbWorkspaceMode;
  loading: boolean;
  queryHistory: QueryHistoryEntry[];
  setQueryHistory: (entries: QueryHistoryEntry[]) => void;
  restoreQueryHistoryEntry: (entry: QueryHistoryEntry) => void;
  sqlFile: SqlFilePanelState;
  activeSqlCapable: boolean;
  executeSqlFileFromPanel: () => Promise<void>;
  driverManifest: DatabaseDriverManifest | null;
  openDriverManager: () => Promise<void>;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxSchema: string | null;
  activeMongoWorkspaceDatabase: string | null;
  activeMongoDocumentId: string | null;
  setMongoDocumentQueriesByCollection: (
    updater: (
      current: Record<string, MongoSidebarDocumentQuery>,
    ) => Record<string, MongoSidebarDocumentQuery>,
  ) => void;
  loadMongoSidebarDocuments: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
    append?: boolean,
    queryOverride?: MongoSidebarDocumentQuery,
  ) => Promise<unknown[]>;
  dbxHasSqlObjectBrowser: boolean;
  selectedDbxTable: DbxObjectInfo | null;
  sqlDbxConnections: AeroricDbConnectionConfig[];
  dbxTableObjects: DbxObjectInfo[];
  dbxColumnsByTable: Record<string, DbxColumnInfo[]>;
  dbxObjects: DbxObjectInfo[];
  loadDbxObject: (
    object: DbxObjectInfo,
    nextPage: number,
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
    whereInput?: string | null,
    orderBy?: string | null,
  ) => Promise<void>;
  selectedDbxInfoObject: DbxObjectInfo | null;
  tableInfo: TableInfoPanelState;
  showDbxObjectDdl: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
  ) => Promise<void>;
  /** 链尾那条 `: (` 分支的内容(网格那一叠),由调用方构造好传进来。 */
  fallback: ReactNode;
}

export function DatabaseWorkspacePanels({
  workspaceMode,
  loading,
  queryHistory,
  setQueryHistory,
  restoreQueryHistoryEntry,
  sqlFile,
  activeSqlCapable,
  executeSqlFileFromPanel,
  driverManifest,
  openDriverManager,
  activeDbxConnection,
  activeDbxDatabase,
  activeDbxSchema,
  activeMongoWorkspaceDatabase,
  activeMongoDocumentId,
  setMongoDocumentQueriesByCollection,
  loadMongoSidebarDocuments,
  dbxHasSqlObjectBrowser,
  selectedDbxTable,
  sqlDbxConnections,
  dbxTableObjects,
  dbxColumnsByTable,
  dbxObjects,
  loadDbxObject,
  selectedDbxInfoObject,
  tableInfo,
  showDbxObjectDdl,
  fallback,
}: DatabaseWorkspacePanelsProps): ReactNode {
  const { t } = useI18n();

  return workspaceMode === "query-history" ? (
    <QueryHistoryPanel
      entries={queryHistory}
      onClear={() => setQueryHistory([])}
      onRestore={restoreQueryHistoryEntry}
    />
  ) : workspaceMode === "sql-file" ? (
    <SqlFilePanel
      state={sqlFile}
      busy={loading}
      canExecute={activeSqlCapable}
      onExecute={() => void executeSqlFileFromPanel()}
    />
  ) : workspaceMode === "drivers" ? (
    <DriverManagerPanel manifest={driverManifest} loading={loading} onRefresh={openDriverManager} />
  ) : workspaceMode === "redis" && activeDbxConnection ? (
    <RedisBrowser
      connectionId={activeDbxConnection.id}
      connection={activeDbxConnection}
      readOnly={activeDbxConnection.readOnly}
      initialDb={
        activeDbxDatabase?.startsWith("db") ? Number(activeDbxDatabase.slice(2)) : undefined
      }
      initialKey={activeDbxSchema ?? undefined}
      keySeparator={dbxString(dbxConfigRecord(activeDbxConnection), "redis_key_separator", ":")}
    />
  ) : workspaceMode === "mongo" && activeDbxConnection ? (
    <MongoBrowser
      connectionId={activeDbxConnection.id}
      connection={activeDbxConnection}
      readOnly={activeDbxConnection.readOnly}
      initialDatabase={activeMongoWorkspaceDatabase ?? undefined}
      initialCollection={activeMongoWorkspaceDatabase ? (activeDbxSchema ?? undefined) : undefined}
      initialDocumentId={
        activeMongoWorkspaceDatabase ? (activeMongoDocumentId ?? undefined) : undefined
      }
      onDocumentsQueryApplied={(database, collection, filter, sort, projection) => {
        const query = { filter, sort, projection };
        const key = `${activeDbxConnection.id}:${database}:${collection}`;
        setMongoDocumentQueriesByCollection((current) => ({ ...current, [key]: query }));
        void loadMongoSidebarDocuments(activeDbxConnection, database, collection, false, query);
      }}
    />
  ) : (workspaceMode === "transfer" ||
      workspaceMode === "schema-diff" ||
      workspaceMode === "data-compare") &&
    (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
    <GuidancePanel
      title={
        workspaceMode === "transfer"
          ? t("database.dataTransfer")
          : workspaceMode === "schema-diff"
            ? t("database.schemaDiff")
            : t("database.dataCompare")
      }
      message={t("database.selectDbxSqlConnection")}
    />
  ) : (workspaceMode === "transfer" ||
      workspaceMode === "schema-diff" ||
      workspaceMode === "data-compare") &&
    activeDbxConnection ? (
    <DatabaseAdvancedTools
      connectionId={activeDbxConnection.id}
      mode={workspaceMode}
      database={activeDbxDatabase}
      schema={activeDbxSchema ?? selectedDbxTable?.schema ?? null}
      table={selectedDbxTable?.name ?? null}
      availableConnections={sqlDbxConnections}
      sourceObjects={dbxTableObjects}
    />
  ) : workspaceMode === "user-admin" &&
    (!activeDbxConnection || !supportsDbxUserAdmin(activeDbxConnection.dbType)) ? (
    <GuidancePanel
      title={t("database.userAdmin")}
      message={t("database.selectUserAdminConnection")}
    />
  ) : workspaceMode === "user-admin" && activeDbxConnection ? (
    <DatabaseUserAdminPanel
      connection={activeDbxConnection}
      database={activeDbxDatabase}
      schema={activeDbxSchema}
    />
  ) : workspaceMode === "er-diagram" && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
    <GuidancePanel title={t("database.erDiagram")} message={t("database.selectDbxSqlConnection")} />
  ) : workspaceMode === "er-diagram" && activeDbxConnection ? (
    <ErDiagramPanel tables={dbxTableObjects} columnsByTable={dbxColumnsByTable} />
  ) : workspaceMode === "database-search" &&
    (!activeDbxConnection || !dbxHasSqlObjectBrowser || !activeDbxDatabase) ? (
    <GuidancePanel
      title={t("database.databaseSearch")}
      message={t("database.selectDbxSqlConnection")}
    />
  ) : workspaceMode === "database-search" && activeDbxConnection ? (
    <DatabaseSearchPanel
      connection={activeDbxConnection}
      database={activeDbxDatabase}
      schema={activeDbxSchema}
      objects={dbxObjects}
      onOpenResult={(object, whereInput) => {
        void loadDbxObject(object, 1, activeDbxConnection, activeDbxDatabase, whereInput);
      }}
    />
  ) : workspaceMode === "table-structure" && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
    <GuidancePanel title={t("database.tableStructure")} message={t("database.selectDbxTable")} />
  ) : workspaceMode === "table-structure" && !selectedDbxTable ? (
    <GuidancePanel title={t("database.tableStructure")} message={t("database.selectDbxTable")} />
  ) : workspaceMode === "table-structure" && selectedDbxTable ? (
    <TableStructurePanel
      connectionId={activeDbxConnection?.id}
      database={activeDbxDatabase}
      schema={selectedDbxTable.schema ?? null}
      databaseType={activeDbxConnection?.dbType ?? null}
      tableName={selectedDbxTable.name}
      columns={
        dbxColumnsByTable[
          selectedDbxTable.schema
            ? `${selectedDbxTable.schema}.${selectedDbxTable.name}`
            : selectedDbxTable.name
        ] ?? []
      }
      readOnly={activeDbxConnection?.readOnly ?? true}
    />
  ) : workspaceMode === "table-info" &&
    (!activeDbxConnection || !dbxHasSqlObjectBrowser || !selectedDbxInfoObject) ? (
    <GuidancePanel title={t("database.tableInfo")} message={t("database.selectDbxTable")} />
  ) : workspaceMode === "table-info" && selectedDbxInfoObject ? (
    <TableInfoPanel
      state={tableInfo}
      onViewDdl={() =>
        void showDbxObjectDdl(activeDbxConnection!, activeDbxDatabase, selectedDbxInfoObject)
      }
    />
  ) : (
    fallback
  );
}
