/**
 * 「表属性」面板的全部状态与派生值。
 *
 * 从 `DatabaseView.tsx` 抽出,手法与同目录的 `useTableImportDialog.ts` 等一致。
 *
 * 与那几个对话框的区别:这一簇的**数据源在外面** —— 列 / 索引 / 外键 / 触发器都由
 * DatabaseView 从 `dbxObjects`、`dbxColumnsByTable` 派生后传进来(见
 * `TableInfoPanelDeps`),hook 只负责"看哪个 tab、搜什么词、DDL 拉到了没有"。
 * 因此它把传入的四份列表原样透出,让面板同时拿到未过滤的计数与过滤后的行。
 *
 * 头部那支 `refreshMetadata` 同理:真正落地靠 `deps.reloadMetadata`(DatabaseView 传进来的
 * 加载器),这里只管「什么时候调、按钮什么时候置灰、过期的响应别落地」。它有自己的一条请求号,
 * 与 DDL 那条各管一段;换表时两条都作废。
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
  /**
   * 重新拉当前表的列与它所在模式的对象列表。四份列表的真相在外面(`dbxColumnsByTable` /
   * `dbxObjects`),所以刷新只能由外面那支加载器落地,本 hook 只管什么时候调、以及别让
   * 过期的响应盖住新表 —— `shouldApply` 就是这个用途。
   */
  reloadMetadata: (
    object: DbxObjectInfo,
    connection: AeroricDbConnectionConfig,
    database: string | null,
    shouldApply: () => boolean,
  ) => Promise<void>;
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
  /** 头部「刷新」:重拉列 / 索引 / 外键 / 触发器,拉过 DDL 的话连 DDL 一起。 */
  refreshMetadata: () => Promise<void>;
  /** 上面那支正在飞,按钮据此置灰。 */
  metadataRefreshing: boolean;
  /** 刷新元数据失败时的提示,成功一次就清掉。 */
  metadataError: string;
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
  reloadMetadata,
}: TableInfoPanelDeps): TableInfoPanelState {
  const [activeTab, setActiveTab] = useState<TableInfoTab>("columns");
  const [search, setSearch] = useState("");
  const ddlRequestSequenceRef = useRef(createRequestSequence());
  const metadataRequestSequenceRef = useRef(createRequestSequence());
  const [metadataState, setMetadataState] = useState({
    scopeKey: "",
    refreshing: false,
    error: "",
  });
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
    if (scopeChanged) {
      // 换表时上一张表的刷新如果还在飞,作废它:它的响应只该落在自己那张表上。
      metadataRequestSequenceRef.current.invalidate();
      setMetadataState({ scopeKey: currentScopeKey, refreshing: false, error: "" });
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

  const metadataRefreshing = metadataState.scopeKey === currentScopeKey && metadataState.refreshing;

  const refreshMetadata = useCallback(async () => {
    if (!connection || !object || metadataRefreshing) return;
    const scopeKey = currentScopeKey;
    const sequence = metadataRequestSequenceRef.current.next();
    const isCurrent = () => metadataRequestSequenceRef.current.isCurrent(sequence);
    setMetadataState({ scopeKey, refreshing: true, error: "" });
    try {
      await reloadMetadata(object, connection, database, isCurrent);
      // DDL 只在已经拉到过(或正停在那个 tab)时才跟着刷:没看过就没有会变旧的东西,
      // 白拉一次 DDL 是实打实的一次服务端查询。
      if (isCurrent() && (activeTab === "ddl" || ddlState.ddl)) {
        await loadDdlForObject(connection, database, object);
      }
      if (!isCurrent()) return;
      setMetadataState({ scopeKey, refreshing: false, error: "" });
    } catch (err) {
      if (!isCurrent()) return;
      setMetadataState({ scopeKey, refreshing: false, error: String(err) });
    }
  }, [
    activeTab,
    connection,
    currentScopeKey,
    database,
    ddlState.ddl,
    loadDdlForObject,
    metadataRefreshing,
    object,
    reloadMetadata,
  ]);

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
    refreshMetadata,
    metadataRefreshing,
    metadataError: metadataState.scopeKey === currentScopeKey ? metadataState.error : "",
    loadDdl,
    loadDdlForObject,
  };
}
