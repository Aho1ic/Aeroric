/**
 * dbx 对象层的动作:看 DDL / 看源码、写 SQL 草稿、导出表与结构、以及 drop 表 / 对象 / 列 /
 * 表子对象 / 库 / schema。
 *
 * 从 `DatabaseView.tsx` 抽出:这一批原本在文件里就是连续的一段(只有 `dropDbxDatabase` 与
 * `dropDbxSchema` 隔在几个菜单派生量后面,依赖上并不需要它们,所以一并收进来)。
 * 它们共享同一套骨架 —— 「先让后端拼 SQL → confirm 里连 SQL 一起给用户看 → 执行 →
 * 重新拉一层树」,而重新拉哪一层各不相同(drop 表/对象拉整条连接,drop 表子对象拉库再补列
 * 元数据),所以那段收尾没有折成公共 helper。
 *
 * 分支顺序、i18n key 与每一处 setState 都与原来逐字一致 —— `dropDbxObject` 会先把表类对象转交
 * `dropDbxTableObject`,顺序换了非表对象会走错分支。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 这些 setter 的身份本来就不变,行为不受影响。
 */

import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback } from "react";

import { useI18n } from "../../i18n";
import { confirm } from "../../lib/appDialog";
import { databaseApi } from "../../lib/databaseApi";
import type {
  AeroricDbConnectionConfig,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbxColumnInfo,
  DbxObjectInfo,
  TableChildObjectType,
  TableExportRequest,
} from "../../types";
import type { DatabaseExportProgress } from "./DatabaseExportProgressOverlay";
import type { TableExportFormat } from "./databaseGridState";
import {
  dbxObjectDropConfirmLabelKey,
  dbxObjectDropLabelKey,
  dbxObjectKey,
  dbxObjectSourceKind,
  dbxQualifiedSqlName,
  dbxQueryToExecuteResult,
  dbxTableChildObjectType,
  isDbxTableObject,
  isSqlDbxConnection,
  type DbWorkspaceMode,
} from "./databaseViewModel";
import type { DatabaseExportDialogState } from "./useDatabaseExportDialog";
import type { DbxDataGridController } from "./useDbxDataGrid";

/** 六支「对某个对象动手」的动作签名一致,统一成「连接 + 库 + 对象」。 */
type DbxObjectAction = (
  connection: AeroricDbConnectionConfig,
  database: string | null,
  object: DbxObjectInfo,
) => Promise<void>;

export interface DbxObjectOperationsDeps {
  grid: DbxDataGridController;
  activeObject: DbObject | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxObject: DbxObjectInfo | null;
  queryResult: DbQueryResult | null;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setExportProgress: (progress: DatabaseExportProgress | null) => void;
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  setActiveDbxSchema: (schema: string | null) => void;
  setActiveDbxObject: (object: DbxObjectInfo | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setSql: (sql: string) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  loadDbxDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<unknown>;
  loadDbxColumnsForTables: (
    objects: DbxObjectInfo[],
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
  ) => Promise<void>;
  /** 复制结构那两支把文本交给它写剪贴板。 */
  copyNodeName: (name: string) => void;
  databaseExport: Pick<DatabaseExportDialogState, "open">;
}

export interface DbxObjectOperations {
  dropDbxTableObject: DbxObjectAction;
  showDbxObjectDdl: DbxObjectAction;
  showDbxObjectSource: DbxObjectAction;
  writeDbxProcedureExecutionDraft: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
  ) => void;
  writeDbxObjectSqlDraft: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    mode: "select" | "insert" | "update",
  ) => void;
  exportDbxTableObject: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    format: TableExportFormat,
    requestOverrides?: Partial<
      Pick<TableExportRequest, "columns" | "columnTypes" | "primaryKeys" | "whereInput" | "orderBy">
    >,
  ) => Promise<void>;
  exportActiveDbxGrid: (format?: TableExportFormat) => Promise<void>;
  copyDbxObjectStructure: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    format: "markdown" | "tsv",
  ) => Promise<void>;
  exportDbxObjectStructure: DbxObjectAction;
  copyDbxObjectStructureDdl: DbxObjectAction;
  dropDbxObject: DbxObjectAction;
  dropDbxTableChildObjectByName: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    childObjectType: TableChildObjectType,
    childObjectName: string,
  ) => Promise<void>;
  dropDbxColumn: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    column: DbxColumnInfo,
  ) => Promise<void>;
  dropDbxTableChildObject: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    childObject: DbxObjectInfo,
  ) => Promise<void>;
  dropDbxDatabase: (connection: AeroricDbConnectionConfig, database: string) => Promise<void>;
  dropDbxSchema: (
    connection: AeroricDbConnectionConfig,
    database: string,
    schemaName: string,
  ) => Promise<void>;
}

export function useDbxObjectOperations(deps: DbxObjectOperationsDeps): DbxObjectOperations {
  const { t } = useI18n();
  const {
    grid,
    activeObject,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    queryResult,
    setLoading,
    setError,
    setExportProgress,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setActiveDbxObject,
    setWorkspaceMode,
    setSql,
    setSqlResult,
    setQueryResult,
    loadDbxConnection,
    loadDbxDatabase,
    loadDbxColumnsForTables,
    copyNodeName,
    databaseExport,
  } = deps;
  const { dbxGridExportFormat, dbxGridOrderByInput } = grid.state;
  const { visibleTableColumns, dbxGridEffectiveWhereInput } = grid.derived;

  const dropDbxTableObject = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection) || !isDbxTableObject(object)) return;
      const tableOptions = {
        databaseType: connection.dbType,
        schema: object.schema ?? null,
        tableName: object.name,
      };
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropTableSql(tableOptions);
        const ok = await confirm(
          `${t("database.confirmDropTable", { name: dbxObjectKey(object) })}\n\n${sql}`,
          {
            title: t("database.dropTable"),
            kind: "warning",
            okLabel: t("database.dropTable"),
            cancelLabel: t("common.cancel"),
          },
        );
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxConnection, setError, setLoading, t],
  );

  const showDbxObjectDdl = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(object.schema ?? null);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      setLoading(true);
      setError(null);
      try {
        const ddl = await databaseApi.dbxGetTableDdl(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
        setSql(ddl);
        setSqlResult(
          dbxQueryToExecuteResult({
            columns: ["ddl"],
            column_types: ["text"],
            column_sortables: [false],
            rows: [[ddl]],
            affected_rows: 0,
            execution_time_ms: 0,
            truncated: false,
            has_more: false,
          }),
        );
        setQueryResult(null);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveDbxSchema,
      setError,
      setLoading,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const showDbxObjectSource = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      const objectType = dbxObjectSourceKind(object);
      if (!objectType) return;
      const schema = object.schema ?? database ?? "";
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      setLoading(true);
      setError(null);
      try {
        const source = await databaseApi.dbxGetObjectSource(
          connection.id,
          database,
          schema,
          object.name,
          objectType,
          object.signature ?? null,
        );
        setSql(source.source);
        setSqlResult(
          dbxQueryToExecuteResult({
            columns: ["source"],
            column_types: ["text"],
            column_sortables: [false],
            rows: [[source.source]],
            affected_rows: 0,
            execution_time_ms: 0,
            truncated: false,
            has_more: false,
          }),
        );
        setQueryResult(null);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setError,
      setLoading,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const writeDbxProcedureExecutionDraft = useCallback(
    (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      setSql(`CALL ${dbxQualifiedSqlName(object)}();`);
      setSqlResult(null);
      setQueryResult(null);
      setError(null);
    },
    [
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setError,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const writeDbxObjectSqlDraft = useCallback(
    (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      mode: "select" | "insert" | "update",
    ) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      const name = dbxQualifiedSqlName(object);
      const draft =
        mode === "select"
          ? `SELECT * FROM ${name}\nLIMIT 100;`
          : mode === "insert"
            ? `INSERT INTO ${name} (\n  column_name\n) VALUES (\n  value\n);`
            : `UPDATE ${name}\nSET column_name = value\nWHERE condition;`;
      setSql(draft);
      setSqlResult(null);
      setQueryResult(null);
      setError(null);
    },
    [
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setError,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const exportDbxTableObject = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      format: TableExportFormat,
      requestOverrides: Partial<
        Pick<
          TableExportRequest,
          "columns" | "columnTypes" | "primaryKeys" | "whereInput" | "orderBy"
        >
      > = {},
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      const extensionByFormat: Record<TableExportFormat, string> = {
        csv: "csv",
        json: "json",
        markdown: "md",
        insertSql: "sql",
        updateSql: "sql",
        xlsx: "xlsx",
      };
      const selectedPath = await saveDialog({
        defaultPath: `${object.name}.${extensionByFormat[format]}`,
        filters: [{ name: format.toUpperCase(), extensions: [extensionByFormat[format]] }],
      });
      if (typeof selectedPath !== "string" || !selectedPath.trim()) return;
      const request = {
        exportId: `export:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        connectionId: connection.id,
        database: database ?? "",
        schema: object.schema ?? null,
        tableName: object.name,
        filePath: selectedPath.trim(),
        format,
        batchSize: 1000,
        ...requestOverrides,
      };
      setLoading(true);
      setExportProgress({ active: true, format, filePath: selectedPath.trim() });
      setError(null);
      try {
        if (format === "csv") await databaseApi.dbxExportTableCsv(request);
        else if (format === "json") await databaseApi.dbxExportTableJson(request);
        else if (format === "markdown") await databaseApi.dbxExportTableMarkdown(request);
        else if (format === "insertSql") await databaseApi.dbxExportTableInsertSql(request);
        else if (format === "updateSql") await databaseApi.dbxExportTableUpdateSql(request);
        else await databaseApi.dbxExportTableXlsx(request);
      } catch (err) {
        setError(String(err));
      } finally {
        setExportProgress(null);
        setLoading(false);
      }
    },
    [setError, setExportProgress, setLoading],
  );

  const exportActiveDbxGrid = useCallback(
    async (format: TableExportFormat = dbxGridExportFormat) => {
      if (
        !activeDbxConnection ||
        !activeDbxObject ||
        !queryResult ||
        visibleTableColumns.length === 0
      )
        return;
      const columns = visibleTableColumns.map(({ column }) => column);
      const columnTypes = columns.map((column) => {
        const metadata = activeObject?.columns.find(
          (item) => item.name.toLowerCase() === column.toLowerCase(),
        );
        return metadata?.dataType ?? null;
      });
      await exportDbxTableObject(activeDbxConnection, activeDbxDatabase, activeDbxObject, format, {
        columns,
        columnTypes,
        primaryKeys: activeObject?.primaryKeys ?? [],
        whereInput: dbxGridEffectiveWhereInput.trim() || null,
        orderBy: dbxGridOrderByInput.trim() || null,
      });
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      activeObject?.columns,
      activeObject?.primaryKeys,
      dbxGridExportFormat,
      dbxGridOrderByInput,
      dbxGridEffectiveWhereInput,
      exportDbxTableObject,
      queryResult,
      visibleTableColumns,
    ],
  );

  const copyDbxObjectStructure = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      format: "markdown" | "tsv",
    ) => {
      setLoading(true);
      setError(null);
      try {
        const columns = await databaseApi.dbxGetColumns(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
        const text =
          format === "markdown"
            ? [
                "| Column | Type | Nullable | Primary key |",
                "| --- | --- | --- | --- |",
                ...columns.map(
                  (column) =>
                    `| ${column.name} | ${column.data_type ?? ""} | ${column.is_nullable ? "yes" : "no"} | ${column.is_primary_key ? "yes" : "no"} |`,
                ),
              ].join("\n")
            : [
                "Column\tType\tNullable\tPrimary key",
                ...columns.map(
                  (column) =>
                    `${column.name}\t${column.data_type ?? ""}\t${column.is_nullable ? "yes" : "no"}\t${column.is_primary_key ? "yes" : "no"}`,
                ),
              ].join("\n");
        copyNodeName(text);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [copyNodeName, setError, setLoading],
  );

  const exportDbxObjectStructure = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      const exportDatabase = database || activeDbxDatabase;
      if (!exportDatabase) return;
      await databaseExport.open(connection, exportDatabase, object.schema ?? null, [object.name]);
    },
    [activeDbxDatabase, databaseExport],
  );

  const copyDbxObjectStructureDdl = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      setLoading(true);
      setError(null);
      try {
        const ddl = await databaseApi.dbxGetTableDdl(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
        copyNodeName(ddl);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [copyNodeName, setError, setLoading],
  );

  const dropDbxObject = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      if (isDbxTableObject(object)) {
        await dropDbxTableObject(connection, database, object);
        return;
      }
      setLoading(true);
      setError(null);
      try {
        const title = t(dbxObjectDropLabelKey(object));
        const sql = await databaseApi.dbxBuildDropObjectSql({
          databaseType: connection.dbType,
          objectType: object.object_type.toUpperCase(),
          schema: object.schema ?? null,
          name: object.name,
        });
        const ok = await confirm(
          `${t(dbxObjectDropConfirmLabelKey(object), { name: dbxObjectKey(object) })}\n\n${sql}`,
          {
            title,
            kind: "warning",
            okLabel: title,
            cancelLabel: t("common.cancel"),
          },
        );
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [dropDbxTableObject, loadDbxConnection, setError, setLoading, t],
  );

  const dropDbxTableChildObjectByName = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      childObjectType: TableChildObjectType,
      childObjectName: string,
    ) => {
      if (!isSqlDbxConnection(connection) || !isDbxTableObject(object)) return;
      const actionConfig: Record<
        TableChildObjectType,
        { title: string; message: string; okLabel: string }
      > = {
        COLUMN: {
          title: t("database.dropColumn"),
          message: t("database.confirmDropColumn", { name: childObjectName }),
          okLabel: t("database.dropColumn"),
        },
        INDEX: {
          title: t("database.dropIndex"),
          message: t("database.confirmDropIndex", { name: childObjectName }),
          okLabel: t("database.dropIndex"),
        },
        FOREIGN_KEY: {
          title: t("database.dropForeignKey"),
          message: t("database.confirmDropForeignKey", { name: childObjectName }),
          okLabel: t("database.dropForeignKey"),
        },
        TRIGGER: {
          title: t("database.dropTrigger"),
          message: t("database.confirmDropTrigger", { name: childObjectName }),
          okLabel: t("database.dropTrigger"),
        },
      };
      const config = actionConfig[childObjectType];
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropTableChildObjectSql({
          databaseType: connection.dbType,
          objectType: childObjectType,
          schema: object.schema ?? null,
          tableName: object.name,
          name: childObjectName,
        });
        const ok = await confirm(`${config.message}\n\n${sql}`, {
          title: config.title,
          kind: "warning",
          okLabel: config.okLabel,
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxDatabase(connection, database);
        await loadDbxColumnsForTables([object], connection, database);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxColumnsForTables, loadDbxDatabase, setError, setLoading, t],
  );

  const dropDbxColumn = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      column: DbxColumnInfo,
    ) => {
      await dropDbxTableChildObjectByName(connection, database, object, "COLUMN", column.name);
    },
    [dropDbxTableChildObjectByName],
  );

  const dropDbxTableChildObject = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      childObject: DbxObjectInfo,
    ) => {
      const childObjectType = dbxTableChildObjectType(childObject);
      if (!childObjectType) return;
      await dropDbxTableChildObjectByName(
        connection,
        database,
        object,
        childObjectType,
        childObject.name,
      );
    },
    [dropDbxTableChildObjectByName],
  );

  const dropDbxDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string) => {
      if (!isSqlDbxConnection(connection)) return;
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropDatabaseSql({
          databaseType: connection.dbType,
          name: database,
        });
        const ok = await confirm(
          `${t("database.confirmDropDatabase", { name: database })}\n\n${sql}`,
          {
            title: t("database.dropDatabase"),
            kind: "warning",
            okLabel: t("database.dropDatabase"),
            cancelLabel: t("common.cancel"),
          },
        );
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database: "",
          sql,
        });
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxConnection, setError, setLoading, t],
  );

  const dropDbxSchema = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string, schemaName: string) => {
      if (!isSqlDbxConnection(connection)) return;
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropSchemaSql({
          databaseType: connection.dbType,
          name: schemaName,
        });
        const ok = await confirm(
          `${t("database.confirmDropSchema", { name: schemaName })}\n\n${sql}`,
          {
            title: t("database.dropSchema"),
            kind: "warning",
            okLabel: t("database.dropSchema"),
            cancelLabel: t("common.cancel"),
          },
        );
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxDatabase(connection, database);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxDatabase, setError, setLoading, t],
  );

  return {
    dropDbxTableObject,
    showDbxObjectDdl,
    showDbxObjectSource,
    writeDbxProcedureExecutionDraft,
    writeDbxObjectSqlDraft,
    exportDbxTableObject,
    exportActiveDbxGrid,
    copyDbxObjectStructure,
    exportDbxObjectStructure,
    copyDbxObjectStructureDdl,
    dropDbxObject,
    dropDbxTableChildObjectByName,
    dropDbxColumn,
    dropDbxTableChildObject,
    dropDbxDatabase,
    dropDbxSchema,
  };
}
