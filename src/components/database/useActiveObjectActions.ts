/**
 * 「当前这张表」这一层的两支动作:按页重拉当前对象的数据,以及工具栏上的「表属性」。
 *
 * 从 `DatabaseView.tsx` 抽出。两支原本紧挨在 JSX 之前,共同点是都以「当前打开的那个对象」
 * 为主语 —— 前者刷新它的某一页,后者切到 table-info 面板并把那张表的列与 DDL 预热好。
 *
 * 两支都保留成普通函数而不是 `useCallback`:原文就是每次渲染重建,它们只作为 JSX 上的
 * 事件回调用一次,不参与任何依赖比较。
 *
 * 逐字保留的几处:
 * - `loadActiveObjectPage` 的分支顺序:dbx 连接 + dbx 对象都在才走 `loadDbxObject`,并把
 *   网格上「输入框里的」`dbxGridWhereInput` / `dbxGridOrderByInput` 一起带过去;否则退回
 *   legacy 的 `loadTable`。两条都不在时什么也不做。
 * - `openActiveTableProperties` 里 `tableInfo.setActiveTab("columns")` 是每次都重置到列这一 tab;
 *   标签 id 固定是 `table-info:${表名}`,已存在就不再追加(`prev.some` 那一段),标签是可关的;
 *   最后两句预热(列与 DDL)都用 `void` 丢掉 promise,不等它们完成就返回。
 */

import { useI18n } from "../../i18n";
import type { AeroricDbConnectionConfig, DbObject, DbxObjectInfo } from "../../types";
import type { DbWorkspaceMode } from "./databaseViewModel";
import type { WorkspaceTab } from "./databaseWorkspaceStore";
import type { TableInfoPanelState } from "./useTableInfoPanel";

export interface ActiveObjectActionsDeps {
  activeObject: DbObject | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxObject: DbxObjectInfo | null;
  dbxGridWhereInput: string;
  dbxGridOrderByInput: string;
  tableInfo: TableInfoPanelState;
  loadDbxObject: (
    object: DbxObjectInfo,
    nextPage: number,
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
    whereInput?: string | null,
    orderBy?: string | null,
  ) => Promise<void>;
  loadDbxColumnsForTables: (
    objects: DbxObjectInfo[],
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
  ) => Promise<void>;
  loadTable: (object: DbObject, nextPage: number) => Promise<void>;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  /** 「表属性」要用 updater 形式读旧标签去重,所以拿的是完整的 setState 签名。 */
  setWorkspaceTabs: (updater: (prev: WorkspaceTab[]) => WorkspaceTab[]) => void;
  setActiveTabId: (id: string) => void;
}

export interface ActiveObjectActions {
  loadActiveObjectPage: (targetPage: number) => void;
  openActiveTableProperties: () => void;
}

export function useActiveObjectActions(deps: ActiveObjectActionsDeps): ActiveObjectActions {
  const { t } = useI18n();
  const {
    activeObject,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    dbxGridWhereInput,
    dbxGridOrderByInput,
    tableInfo,
    loadDbxObject,
    loadDbxColumnsForTables,
    loadTable,
    setWorkspaceMode,
    setWorkspaceTabs,
    setActiveTabId,
  } = deps;

  /** 刷新和翻页共用的一段:dbx 连接走 loadDbxObject,legacy 连接走 loadTable。 */
  const loadActiveObjectPage = (targetPage: number) => {
    if (activeDbxConnection && activeDbxObject) {
      loadDbxObject(
        activeDbxObject,
        targetPage,
        activeDbxConnection,
        activeDbxDatabase,
        dbxGridWhereInput,
        dbxGridOrderByInput,
      );
    } else if (activeObject) {
      loadTable(activeObject, targetPage);
    }
  };

  /** 工具栏的「表属性」:切到 table-info 工作区,补一个可关的标签,再把列与 DDL 拉齐。 */
  const openActiveTableProperties = () => {
    if (!activeDbxConnection || !activeDbxObject) return;
    const target = activeDbxObject;
    setWorkspaceMode("table-info");
    tableInfo.setActiveTab("columns");
    const tabId = `table-info:${target.name}`;
    setWorkspaceTabs((prev) =>
      prev.some((tab) => tab.id === tabId)
        ? prev
        : [
            ...prev,
            {
              id: tabId,
              mode: "table-info",
              label: `${t("database.tableProperties")}: ${target.name}`,
              closable: true,
            },
          ],
    );
    setActiveTabId(tabId);
    void loadDbxColumnsForTables([target], activeDbxConnection, activeDbxDatabase);
    void tableInfo.loadDdlForObject(activeDbxConnection, activeDbxDatabase, target);
  };

  return { loadActiveObjectPage, openActiveTableProperties };
}
