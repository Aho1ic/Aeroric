/**
 * 工作区这一层的三个派生值(当前网格的主键、能不能插行、要不要藏顶栏),外加把 `workspaceMode`
 * 翻成标题文案的那支 `databaseWorkspaceTitle`。
 *
 * 从 `DatabaseView.tsx` 抽出:原文里这五支就是紧挨着的一段,共同点是都只回答「当前这一屏
 * 该长什么样」,不碰任何加载器,也不改状态。
 *
 * 四个派生值保留成普通 `const`(原文就是每次渲染重算),`databaseWorkspaceTitle` 保留成普通函数
 * 而不是 `useCallback` —— 它只在 JSX 里被立即调用一次,包一层反而多一次依赖比较。
 *
 * 逐字保留的几处:
 * - `activeDbxGridPrimaryKeys` 的回退顺序是 `activeObject` → `queryResult` → 空数组。
 * - `canInsertActiveTable`:先要 `workspaceMode === "table"` 且确有 `queryResult`,再分两条 ——
 *   dbx 那条要求当前对象是表、legacy 侧也确有 `activeObject`、且连接不是只读;legacy 那条
 *   只看 `activeObject.objectType === "table"` 与连接的只读位。两条的只读位取自不同的连接对象。
 * - `hasActiveDatabaseWorkspace`:四个「有东西」之一,或者 mode 既不是 table 也不是 query。
 *   它在原文里只被下面那支 `hideDatabaseWorkspaceTopbar` 读,别处没有消费者,所以留在内部不外传。
 * - `hideDatabaseWorkspaceTopbar` 列的四种 mode(drivers / transfer / schema-diff / data-compare)
 *   是白名单,不能改成按别的条件推。
 * - `databaseWorkspaceTitle` 的 `default` 分支回退到 `activeObject?.name`,再回退到
 *   `database.noSelection`;redis / mongo 两支是写死的字面量,不走 i18n。
 */

import { useI18n } from "../../i18n";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbxObjectInfo,
} from "../../types";
import { isDbxTableObject, type DbWorkspaceMode } from "./databaseViewModel";

export interface DatabaseWorkspaceDerivedDeps {
  workspaceMode: DbWorkspaceMode;
  activeObject: DbObject | null;
  activeConnection: DbConnectionConfig | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxObject: DbxObjectInfo | null;
  queryResult: DbQueryResult | null;
  sqlResult: DbExecuteResult | null;
}

export interface DatabaseWorkspaceDerived {
  activeDbxGridPrimaryKeys: string[];
  canInsertActiveTable: boolean;
  hideDatabaseWorkspaceTopbar: boolean;
  databaseWorkspaceTitle: (mode: DbWorkspaceMode) => string;
}

export function useDatabaseWorkspaceDerived(
  deps: DatabaseWorkspaceDerivedDeps,
): DatabaseWorkspaceDerived {
  const { t } = useI18n();
  const {
    workspaceMode,
    activeObject,
    activeConnection,
    activeDbxConnection,
    activeDbxObject,
    queryResult,
    sqlResult,
  } = deps;

  const activeDbxGridPrimaryKeys = activeObject?.primaryKeys ?? queryResult?.primaryKeys ?? [];
  const canInsertActiveTable =
    workspaceMode === "table" &&
    Boolean(queryResult) &&
    (activeDbxConnection
      ? Boolean(
          activeDbxObject &&
          isDbxTableObject(activeDbxObject) &&
          activeObject &&
          !activeDbxConnection.readOnly,
        )
      : Boolean(activeObject?.objectType === "table" && !activeConnection?.readOnly));
  const hasActiveDatabaseWorkspace =
    Boolean(activeObject || activeDbxObject || queryResult || sqlResult) ||
    (workspaceMode !== "table" && workspaceMode !== "query");
  const hideDatabaseWorkspaceTopbar =
    !hasActiveDatabaseWorkspace ||
    workspaceMode === "drivers" ||
    workspaceMode === "transfer" ||
    workspaceMode === "schema-diff" ||
    workspaceMode === "data-compare";
  const databaseWorkspaceTitle = (mode: DbWorkspaceMode) => {
    switch (mode) {
      case "query":
        return t("database.newQuery");
      case "sql-file":
        return t("database.executeSqlFile");
      case "query-history":
        return t("database.queryHistory");
      case "drivers":
        return t("database.driverManager");
      case "redis":
        return "Redis";
      case "mongo":
        return "MongoDB";
      case "transfer":
        return t("database.dataTransfer");
      case "schema-diff":
        return t("database.schemaDiff");
      case "data-compare":
        return t("database.dataCompare");
      case "user-admin":
        return t("database.userAdmin");
      case "er-diagram":
        return t("database.erDiagram");
      case "database-search":
        return t("database.databaseSearch");
      case "table-structure":
        return t("database.tableStructure");
      case "table-info":
        return t("database.tableInfo");
      default:
        return activeObject?.name ?? t("database.noSelection");
    }
  };

  return {
    activeDbxGridPrimaryKeys,
    canInsertActiveTable,
    hideDatabaseWorkspaceTopbar,
    databaseWorkspaceTitle,
  };
}
