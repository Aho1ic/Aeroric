/**
 * 「导入文件到表」对话框的全部状态与动作。
 *
 * 从 `DatabaseView.tsx` 抽出:那个文件是单个约 8,800 行的函数体,所有 state 都在
 * 同一个闭包里,读局部无法判断改动影响面。抽取手法沿用同目录已有的成品
 * (`databaseGridState.ts`、`databaseSidebarTreeState.ts`、`redisBrowserState.ts` 等):
 * 先把状态连同它的动作搬进独立 hook,再把对应 JSX 拆成纯展示组件。
 *
 * 这一簇的外部依赖被收窄成显式参数(见 `TableImportDialogDeps`),因此它与
 * DatabaseView 里其余上百个 state 之间不再有隐式耦合。
 */

import { useCallback, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import type {
  AeroricDbConnectionConfig,
  DbxColumnInfo,
  DbxObjectInfo,
  TableImportMode,
  TableImportPreview,
} from "../../types";
import { databaseApi } from "../../lib/databaseApi";
import { autoMapImportColumns, isDbxTableObject, isSqlDbxConnection } from "./databaseViewModel";
import { confirmDbxProductionOperation } from "./databaseProductionSafety";

export interface TableImportTarget {
  connectionId: string;
  database: string | null;
  object: DbxObjectInfo;
}

export interface TableImportDialogDeps {
  /** 用于把 `tableImportTarget.connectionId` 解析回连接配置。 */
  dbxConnections: AeroricDbConnectionConfig[];
  /** 打开对话框时收起右键菜单。 */
  closeContextMenu: () => void;
  /** DatabaseView 的全局忙碌态 / 错误条,导入期间与其保持一致。 */
  setGlobalLoading: (loading: boolean) => void;
  setGlobalError: (error: string | null) => void;
  /** 导入成功后重新拉一次目标表,让网格显示新数据。 */
  reloadImportedObject: (
    object: DbxObjectInfo,
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  t: (key: string, vars?: Record<string, string | number>) => string;
}

export interface TableImportDialogState {
  target: TableImportTarget | null;
  connection: AeroricDbConnectionConfig | null;
  preview: TableImportPreview | null;
  mappings: Record<string, string>;
  /** 目标表的列名,供映射下拉框和自动映射使用。 */
  targetColumnNames: string[];
  /** 已建立映射的列对,也是"能不能导"的判据之一。 */
  mappedColumns: Array<{ sourceColumn: string; targetColumn: string }>;
  mode: TableImportMode;
  batchSize: string;
  loading: boolean;
  error: string;
  canRun: boolean;
  open: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
  ) => Promise<void>;
  close: () => void;
  chooseFile: () => Promise<void>;
  setMapping: (sourceColumn: string, targetColumn: string) => void;
  autoMap: () => void;
  setMode: (mode: TableImportMode) => void;
  setBatchSize: (batchSize: string) => void;
  submit: () => Promise<void>;
}

const DEFAULT_BATCH_SIZE = "500";

export function useTableImportDialog({
  dbxConnections,
  closeContextMenu,
  setGlobalLoading,
  setGlobalError,
  reloadImportedObject,
  t,
}: TableImportDialogDeps): TableImportDialogState {
  const [target, setTarget] = useState<TableImportTarget | null>(null);
  const [columns, setColumns] = useState<DbxColumnInfo[]>([]);
  const [preview, setPreview] = useState<TableImportPreview | null>(null);
  const [mappings, setMappings] = useState<Record<string, string>>({});
  const [mode, setMode] = useState<TableImportMode>("append");
  const [batchSize, setBatchSize] = useState(DEFAULT_BATCH_SIZE);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const connection = useMemo(
    () => dbxConnections.find((item) => item.id === target?.connectionId) ?? null,
    [dbxConnections, target?.connectionId],
  );
  const targetColumnNames = useMemo(() => columns.map((column) => column.name), [columns]);
  const mappedColumns = useMemo(
    () =>
      preview
        ? preview.columns
            .map((sourceColumn) => ({
              sourceColumn,
              targetColumn: mappings[sourceColumn] ?? "",
            }))
            .filter((mapping) => mapping.targetColumn)
        : [],
    [mappings, preview],
  );
  const canRun = Boolean(connection && target && preview && mappedColumns.length > 0);

  const close = useCallback(() => {
    setTarget(null);
    setColumns([]);
    setPreview(null);
    setMappings({});
    setMode("append");
    setBatchSize(DEFAULT_BATCH_SIZE);
    setError("");
  }, []);

  const open = useCallback(
    async (
      nextConnection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(nextConnection) || !isDbxTableObject(object)) return;
      closeContextMenu();
      setTarget({ connectionId: nextConnection.id, database, object });
      setColumns([]);
      setPreview(null);
      setMappings({});
      setMode("append");
      setBatchSize(DEFAULT_BATCH_SIZE);
      setLoading(true);
      setError("");
      try {
        const nextColumns = await databaseApi.dbxGetColumns(
          nextConnection.id,
          object.name,
          database,
          object.schema ?? null,
        );
        setColumns(nextColumns);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [closeContextMenu],
  );

  const chooseFile = useCallback(async () => {
    if (!target) return;
    const selected = await openDialog({
      multiple: false,
      filters: [
        { name: "Data files", extensions: ["csv", "tsv", "json", "xlsx", "xlsm", "xls"] },
        { name: "CSV", extensions: ["csv", "tsv"] },
        { name: "JSON", extensions: ["json"] },
        { name: "Excel", extensions: ["xlsx", "xlsm", "xls"] },
      ],
    });
    if (typeof selected !== "string" || !selected.trim()) return;

    setLoading(true);
    setError("");
    try {
      const nextPreview = await databaseApi.dbxPreviewTableImportFile(selected.trim());
      setPreview(nextPreview);
      setMappings(autoMapImportColumns(nextPreview.columns, targetColumnNames));
    } catch (err) {
      setPreview(null);
      setMappings({});
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [target, targetColumnNames]);

  const setMapping = useCallback((sourceColumn: string, targetColumn: string) => {
    setMappings((current) => ({ ...current, [sourceColumn]: targetColumn }));
  }, []);

  const autoMap = useCallback(() => {
    if (!preview) return;
    setMappings(autoMapImportColumns(preview.columns, targetColumnNames));
  }, [preview, targetColumnNames]);

  const submit = useCallback(async () => {
    if (!connection || !target || !preview || mappedColumns.length === 0) return;
    setGlobalLoading(true);
    setLoading(true);
    setGlobalError(null);
    setError("");
    try {
      const tableName = target.object.schema
        ? `${target.object.schema}.${target.object.name}`
        : target.object.name;
      // 生产库闸门:写入前必须先拿到确认,取消就直接返回(finally 仍会清忙碌态)。
      const approved = await confirmDbxProductionOperation({
        connection,
        database: target.database,
        operation: t("database.productionTableImportOperation", {
          table: tableName,
          mode:
            mode === "truncate"
              ? t("database.tableImportTruncate")
              : t("database.tableImportAppend"),
        }),
        okLabel: t("database.tableImport"),
        t,
      });
      if (!approved) return;
      await databaseApi.dbxImportTableFile({
        importId: `import:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        connectionId: connection.id,
        database: target.database || "",
        schema: target.object.schema || "",
        table: target.object.name,
        filePath: preview.filePath,
        mappings: mappedColumns,
        mode,
        batchSize: Math.max(1, Number(batchSize) || 500),
      });
      // close() 会清掉 target,所以先取出刷新需要的两个值。
      const importedObject = target.object;
      const importedDatabase = target.database;
      close();
      await reloadImportedObject(importedObject, connection, importedDatabase);
    } catch (err) {
      setGlobalError(String(err));
      setError(String(err));
    } finally {
      setGlobalLoading(false);
      setLoading(false);
    }
  }, [
    batchSize,
    close,
    connection,
    mappedColumns,
    mode,
    preview,
    reloadImportedObject,
    setGlobalError,
    setGlobalLoading,
    t,
    target,
  ]);

  return {
    target,
    connection,
    preview,
    mappings,
    targetColumnNames,
    mappedColumns,
    mode,
    batchSize,
    loading,
    error,
    canRun,
    open,
    close,
    chooseFile,
    setMapping,
    autoMap,
    setMode,
    setBatchSize,
    submit,
  };
}
