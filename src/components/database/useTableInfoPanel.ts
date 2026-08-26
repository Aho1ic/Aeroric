/**
 * 「表属性」面板的全部状态与派生值。
 *
 * 从 `DatabaseView.tsx` 抽出,手法与同目录的 `useTableImportDialog.ts` 等一致。
 *
 * 与那几个对话框的区别:这一簇的**数据源在外面** —— 列 / 索引 / 外键 / 触发器都由
 * DatabaseView 从 `dbxObjects`、`dbxColumnsByTable` 派生后传进来(见
 * `TableInfoPanelDeps`),hook 只负责"看哪个 tab、搜什么词、DDL 拉到了没有"。
 * 因此它把传入的四份列表原样透出,让面板同时拿到未过滤的计数与过滤后的行。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { AeroricDbConnectionConfig, DbxColumnInfo, DbxObjectInfo } from "../../types";
import { databaseApi } from "../../lib/databaseApi";
import type { TableInfoTab } from "./databaseViewModel";
import { createRequestSequence } from "./requestSequence";

export interface TableInfoPanelDeps {
  /** 当前连接与库,拉 DDL 时要用。 */
  connection: AeroricDbConnectionConfig | null;
  database: string | null;
  /** 面板正在描述的表 / 视图。 */
  object: DbxObjectInfo | null;
  /** 表标识;换表时重置 tab、搜索词与已拉到的 DDL。 */
  objectKey: string;
  columns: DbxColumnInfo[];
  indexes: DbxObjectInfo[];
  foreignKeys: DbxObjectInfo[];
  triggers: DbxObjectInfo[];
}

export interface TableInfoPanelState {
  activeTab: TableInfoTab;
  setActiveTab: (tab: TableInfoTab) => void;
  search: string;
  setSearch: (search: string) => void;
  ddl: string;
  ddlLoading: boolean;
  ddlError: string;
  /** 原样透出的四份列表,tab 上的计数用它们(不受搜索词影响)。 */
  columns: DbxColumnInfo[];
  indexes: DbxObjectInfo[];
  foreignKeys: DbxObjectInfo[];
  triggers: DbxObjectInfo[];
  /** 经搜索词过滤后的四份列表,表格正文渲染的是它们。 */
  filteredColumns: DbxColumnInfo[];
  filteredIndexes: DbxObjectInfo[];
  filteredForeignKeys: DbxObjectInfo[];
  filteredTriggers: DbxObjectInfo[];
  /** 刷新当前表的 DDL。 */
  loadDdl: () => Promise<void>;
  /** 给指定表拉 DDL —— 工具栏上的「表属性」按钮会在切面板时顺手预热。 */
  loadDdlForObject: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
  ) => Promise<void>;
}

export function useTableInfoPanel({
  connection,
  database,
  object,
  objectKey,
  columns,
  indexes,
  foreignKeys,
  triggers,
}: TableInfoPanelDeps): TableInfoPanelState {
  const [activeTab, setActiveTab] = useState<TableInfoTab>("columns");
  const [search, setSearch] = useState("");
  const ddlRequestSequenceRef = useRef(createRequestSequence());
  const currentScopeKey = `${connection?.id ?? ""}\0${database ?? ""}\0${object?.schema ?? ""}\0${object?.name ?? ""}`;
  const requestedScopeRef = useRef<string | null>(null);
  const renderedScopeRef = useRef(currentScopeKey);
  const [ddlState, setDdlState] = useState({
    scopeKey: currentScopeKey,
    ddl: "",
    error: "",
    loading: false,
  });

  const query = search.trim().toLowerCase();
  const filteredColumns = useMemo(() => {
    if (!query) return columns;
    return columns.filter((column) =>
      [column.name, column.data_type, column.column_default ?? ""].some((value) =>
        value.toLowerCase().includes(query),
      ),
    );
  }, [columns, query]);
  const filterObjects = useCallback(
    (objects: DbxObjectInfo[]) => {
      if (!query) return objects;
      return objects.filter((item) =>
        [item.name, item.schema ?? "", item.object_type].some((value) =>
          value.toLowerCase().includes(query),
        ),
      );
    },
    [query],
  );
  const filteredIndexes = useMemo(() => filterObjects(indexes), [filterObjects, indexes]);
  const filteredForeignKeys = useMemo(
    () => filterObjects(foreignKeys),
    [filterObjects, foreignKeys],
  );
  const filteredTriggers = useMemo(() => filterObjects(triggers), [filterObjects, triggers]);

  // 换表就回到默认视图:留着上一张表的 DDL 会显示成新表的内容。
  useEffect(() => {
    const scopeChanged = renderedScopeRef.current !== currentScopeKey;
    renderedScopeRef.current = currentScopeKey;
    // `openActiveTableProperties` can start the request before React renders the
    // new object. Preserve that request when its scope already matches the new
    // render; otherwise invalidate the old object's request.
    if (scopeChanged && requestedScopeRef.current !== currentScopeKey) {
      ddlRequestSequenceRef.current.invalidate();
      requestedScopeRef.current = null;
      setDdlState({ scopeKey: currentScopeKey, ddl: "", error: "", loading: false });
    }
    setActiveTab("columns");
    setSearch("");
  }, [currentScopeKey, objectKey]);

  const loadDdlForObject = useCallback(
    async (
      nextConnection: AeroricDbConnectionConfig,
      nextDatabase: string | null,
      nextObject: DbxObjectInfo,
    ) => {
      const scopeKey = `${nextConnection.id}\0${nextDatabase ?? ""}\0${nextObject.schema ?? ""}\0${nextObject.name}`;
      const sequence = ddlRequestSequenceRef.current.next();
      requestedScopeRef.current = scopeKey;
      setDdlState({ scopeKey, ddl: "", error: "", loading: true });
      try {
        const nextDdl = await databaseApi.dbxGetTableDdl(
          nextConnection.id,
          nextObject.name,
          nextDatabase,
          nextObject.schema ?? null,
        );
        if (
          ddlRequestSequenceRef.current.isCurrent(sequence) &&
          requestedScopeRef.current === scopeKey
        ) {
          setDdlState({ scopeKey, ddl: nextDdl, error: "", loading: false });
        }
      } catch (err) {
        if (
          ddlRequestSequenceRef.current.isCurrent(sequence) &&
          requestedScopeRef.current === scopeKey
        ) {
          setDdlState({ scopeKey, ddl: "", error: String(err), loading: false });
        }
      }
    },
    [],
  );

  const loadDdl = useCallback(async () => {
    if (!connection || !object || ddlState.loading) return;
    await loadDdlForObject(connection, database, object);
  }, [connection, database, ddlState.loading, loadDdlForObject, object]);

  // DDL 是唯一需要单独请求的 tab,所以进这个 tab 才拉,且失败后不自动重试。
  useEffect(() => {
    if (
      activeTab === "ddl" &&
      ddlState.scopeKey === currentScopeKey &&
      !ddlState.ddl &&
      !ddlState.loading &&
      !ddlState.error
    ) {
      void loadDdl();
    }
  }, [activeTab, currentScopeKey, ddlState, loadDdl]);

  return {
    activeTab,
    setActiveTab,
    search,
    setSearch,
    ddl: ddlState.scopeKey === currentScopeKey ? ddlState.ddl : "",
    ddlLoading: ddlState.scopeKey === currentScopeKey && ddlState.loading,
    ddlError: ddlState.scopeKey === currentScopeKey ? ddlState.error : "",
    columns,
    indexes,
    foreignKeys,
    triggers,
    filteredColumns,
    filteredIndexes,
    filteredForeignKeys,
    filteredTriggers,
    loadDdl,
    loadDdlForObject,
  };
}
