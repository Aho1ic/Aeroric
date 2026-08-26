/**
 * SQL 编辑器上的三支动作:执行当前语句,以及往编辑器里拖对象时的 dragover / drop。
 *
 * 从 `DatabaseView.tsx` 抽出:这三支原本在文件里就是连续的一段,共同点是都只碰
 * 「编辑器里的那段 SQL」——`runSql` 把它送去执行并记一条历史,两支拖拽回调把拖进来的
 * 对象引用插到光标处。
 *
 * `executeSqlFileFromPanel`(「执行 SQL 文件」面板上的执行键)原文里离这三支有一段距离,
 * 但它和 `runSql` 是同一个形状 —— 同一套「先 dbx、再退回 legacy」的二选一分支、同一批依赖,
 * 只是那段 SQL 来自磁盘上的文件而不是编辑器,所以收进同一层。两处与 `runSql` 不同、
 * 需要逐字保留:它不写查询历史,也不切 `workspaceMode`;legacy 那条路读完文件会把内容
 * 回填进编辑器(`setSql(fileSql)`),dbx 那条路不回填。
 *
 * `runSql` 的分支顺序与原来逐字一致:先看 dbx 那条路(`activeDbxConnection` 且
 * `dbxHasSqlObjectBrowser`),再退回 legacy 的 `activeEndpoint`;最后那句
 * 「只要有 legacy 连接就重新 inspect 一次」在两条路之外,不能挪进任一分支。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 这些 setter 的身份本来就不变,行为不受影响。
 */

import { useCallback, type DragEvent } from "react";

import { databaseApi } from "../../lib/databaseApi";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbQueryResult,
  DbSchema,
} from "../../types";
import {
  PAGE_SIZE,
  dbxQueryToExecuteResult,
  dbxSqlFileResultsToExecuteResult,
  endpointLabel,
  type DbWorkspaceMode,
  type QueryHistoryEntry,
} from "./databaseViewModel";
import type { SqlFilePanelState } from "./useSqlFilePanel";

export interface SqlEditorActionsDeps {
  sql: string;
  setSql: (updater: string | ((current: string) => string)) => void;
  activeConnection: DbConnectionConfig | null;
  activeEndpoint: DbEndpoint | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxSchema: string | null;
  dbxHasSqlObjectBrowser: boolean;
  projectRoot: string | undefined;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  setSchema: (schema: DbSchema | null) => void;
  addQueryHistoryEntry: (entry: Omit<QueryHistoryEntry, "id" | "executedAt">) => void;
  /** 「执行 SQL 文件」面板的那份表单状态,来自 `useSqlFilePanel`。 */
  sqlFile: SqlFilePanelState;
}

export interface SqlEditorActions {
  runSql: () => Promise<void>;
  handleSqlDragOver: (event: DragEvent<HTMLTextAreaElement>) => void;
  handleSqlDrop: (event: DragEvent<HTMLTextAreaElement>) => void;
  executeSqlFileFromPanel: () => Promise<void>;
}

export function useSqlEditorActions(deps: SqlEditorActionsDeps): SqlEditorActions {
  const {
    sql,
    setSql,
    activeConnection,
    activeEndpoint,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxSchema,
    dbxHasSqlObjectBrowser,
    projectRoot,
    setLoading,
    setError,
    setWorkspaceMode,
    setSqlResult,
    setQueryResult,
    setSchema,
    addQueryHistoryEntry,
    sqlFile,
  } = deps;

  const runSql = useCallback(async () => {
    if (!activeEndpoint && !activeDbxConnection) return;
    setLoading(true);
    setError(null);
    try {
      setWorkspaceMode("query");
      if (activeDbxConnection && dbxHasSqlObjectBrowser) {
        const result = await databaseApi.dbxExecuteQuery({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxSchema,
          sql,
          pageSize: PAGE_SIZE,
        });
        setSqlResult(dbxQueryToExecuteResult(result));
        setQueryResult(null);
        addQueryHistoryEntry({
          sql,
          connectionName: activeDbxConnection.name,
          database: activeDbxDatabase,
          schema: activeDbxSchema,
          rowsAffected: result.affected_rows,
          executionTimeMs: result.execution_time_ms,
        });
      } else if (activeEndpoint) {
        const result = await databaseApi.executeSql({
          endpoint: activeEndpoint,
          sql,
          page: 1,
          pageSize: PAGE_SIZE,
          readOnly: activeConnection?.readOnly ?? false,
          connectionId: activeConnection?.id,
          projectRoot,
        });
        setSqlResult(result);
        setQueryResult(null);
        addQueryHistoryEntry({
          sql,
          connectionName: activeConnection?.name ?? endpointLabel(activeEndpoint),
          database: null,
          schema: null,
          rowsAffected: result.rowsAffected,
          executionTimeMs: null,
        });
      }
      if (activeConnection) {
        const nextSchema = await databaseApi.inspect(activeConnection.endpoint, projectRoot);
        setSchema(nextSchema);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [
    activeConnection,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxSchema,
    activeEndpoint,
    addQueryHistoryEntry,
    dbxHasSqlObjectBrowser,
    projectRoot,
    setError,
    setLoading,
    setQueryResult,
    setSchema,
    setSqlResult,
    setWorkspaceMode,
    sql,
  ]);

  const handleSqlDragOver = useCallback((event: DragEvent<HTMLTextAreaElement>) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  }, []);

  const handleSqlDrop = useCallback(
    (event: DragEvent<HTMLTextAreaElement>) => {
      const structured = event.dataTransfer.getData("application/x-aeroric-database-object");
      let droppedText = "";
      if (structured) {
        try {
          const payload = JSON.parse(structured) as { reference?: unknown };
          if (typeof payload.reference === "string") droppedText = payload.reference;
        } catch {
          droppedText = "";
        }
      }
      if (!droppedText) droppedText = event.dataTransfer.getData("text/plain");
      if (!droppedText.trim()) return;

      event.preventDefault();
      const textarea = event.currentTarget;
      const selectionStart = textarea.selectionStart ?? textarea.value.length;
      const selectionEnd = textarea.selectionEnd ?? selectionStart;
      setSql((current) => {
        const start = Math.max(0, Math.min(selectionStart, current.length));
        const end = Math.max(start, Math.min(selectionEnd, current.length));
        return `${current.slice(0, start)}${droppedText}${current.slice(end)}`;
      });
      requestAnimationFrame(() => {
        const cursor = selectionStart + droppedText.length;
        textarea.focus();
        textarea.setSelectionRange(cursor, cursor);
      });
    },
    [setSql],
  );

  const executeSqlFileFromPanel = useCallback(async () => {
    if (!sqlFile.path.trim()) return;
    setLoading(true);
    setError(null);
    try {
      if (activeDbxConnection && dbxHasSqlObjectBrowser) {
        const timeoutSecs = Number.parseInt(sqlFile.timeoutSecs, 10);
        const results = await databaseApi.dbxExecuteSqlFile({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxSchema,
          path: sqlFile.path.trim(),
          timeoutSecs: Number.isFinite(timeoutSecs) && timeoutSecs > 0 ? timeoutSecs : undefined,
        });
        setSqlResult(dbxSqlFileResultsToExecuteResult(results));
        setQueryResult(null);
      } else if (activeEndpoint) {
        const fileSql = await databaseApi.readSqlFile(sqlFile.path.trim());
        const result = await databaseApi.executeSql({
          endpoint: activeEndpoint,
          sql: fileSql,
          page: 1,
          pageSize: PAGE_SIZE,
          readOnly: activeConnection?.readOnly ?? false,
          connectionId: activeConnection?.id,
          projectRoot,
        });
        setSql(fileSql);
        setSqlResult(result);
        setQueryResult(null);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [
    activeConnection?.id,
    activeConnection?.readOnly,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxSchema,
    activeEndpoint,
    dbxHasSqlObjectBrowser,
    projectRoot,
    setError,
    setLoading,
    setQueryResult,
    setSql,
    setSqlResult,
    sqlFile,
  ]);

  return { runSql, handleSqlDragOver, handleSqlDrop, executeSqlFileFromPanel };
}
