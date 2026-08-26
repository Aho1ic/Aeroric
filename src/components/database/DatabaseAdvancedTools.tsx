import { useEffect, useMemo, useState } from "react";
import { Play, RefreshCcw, Square } from "lucide-react";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import s from "../../styles";
import type {
  AeroricDbConnectionConfig,
  DbxColumnInfo,
  DbxObjectInfo,
  DbxTransferProgress,
} from "../../types";
import { Button as DbxButton } from "../ui/Button";
import { confirmDbxProductionOperation } from "./databaseProductionSafety";
import { listAllDbxObjects } from "./databaseViewModel";

export type DatabaseAdvancedToolMode = "transfer" | "schema-diff" | "data-compare";

interface Props {
  connectionId: string;
  mode: DatabaseAdvancedToolMode;
  database?: string | null;
  schema?: string | null;
  table?: string | null;
  availableConnections?: AeroricDbConnectionConfig[];
  sourceObjects?: DbxObjectInfo[];
}

function isSqlDbxConnection(connection: AeroricDbConnectionConfig) {
  return !["redis", "mongodb"].includes(connection.dbType);
}

function tableKey(schema: string | null | undefined, tableName: string) {
  return schema ? `${schema}.${tableName}` : tableName;
}

function tableNamesFromText(value: string) {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function objectToTableInfo(object: DbxObjectInfo) {
  const objectType = object.object_type.toLowerCase();
  return {
    name: object.name,
    table_type: objectType.includes("view") ? "VIEW" : "TABLE",
    comment: object.comment ?? null,
    parent_schema: object.parent_schema ?? object.schema ?? null,
    parent_name: object.parent_name ?? null,
  };
}

function columnsForDetail(columns: DbxColumnInfo[]) {
  return columns.map((column) => ({
    name: column.name,
    data_type: column.data_type,
    is_nullable: column.is_nullable,
    column_default: column.column_default ?? null,
    is_primary_key: column.is_primary_key,
    extra: column.extra ?? null,
    comment: column.comment ?? null,
    numeric_precision: column.numeric_precision ?? null,
    numeric_scale: column.numeric_scale ?? null,
    character_maximum_length: column.character_maximum_length ?? null,
  }));
}

type LoadedToolMetadata = {
  objects: DbxObjectInfo[];
  columnsByTable: Record<string, DbxColumnInfo[]>;
};

function metadataKey(object: DbxObjectInfo, fallbackSchema: string) {
  return tableKey(object.schema ?? fallbackSchema, object.name);
}

async function loadToolMetadata(
  connection: AeroricDbConnectionConfig,
  database: string,
  schema: string,
  tableNames: string[],
): Promise<LoadedToolMetadata> {
  const objects = await listAllDbxObjects(connection.id, database || null, schema || null, {
    objectTypes: ["TABLE", "VIEW", "MATERIALIZED_VIEW"],
  });
  const wanted = new Set(tableNames);
  const selectedObjects = objects.filter((object) => wanted.has(object.name));
  const columnsEntries = await Promise.all(
    selectedObjects.map(async (object) => {
      const columns = await databaseApi.dbxGetColumns(
        connection.id,
        object.name,
        database || null,
        object.schema ?? (schema || null),
      );
      return [metadataKey(object, schema), columns] as const;
    }),
  );
  return { objects: selectedObjects, columnsByTable: Object.fromEntries(columnsEntries) };
}

function detailsForMetadata(metadata: LoadedToolMetadata, fallbackSchema: string) {
  return metadata.objects.map((object) => ({
    name: object.name,
    columns: columnsForDetail(metadata.columnsByTable[metadataKey(object, fallbackSchema)] ?? []),
    indexes: [],
    foreign_keys: [],
    triggers: [],
    ddl: null,
  }));
}

function transferProgressText(progress: DbxTransferProgress, t: ReturnType<typeof useI18n>["t"]) {
  if (progress.status === "done") return t("database.transferCompleted");
  if (progress.status === "cancelled") return t("database.transferCancelled");
  if (progress.status === "error") {
    return t("database.transferFailed", { error: progress.error ?? "Unknown error" });
  }
  return t("database.transferProgress", {
    table: progress.table,
    rows: progress.rowsTransferred,
  });
}

export function DatabaseAdvancedTools({
  connectionId,
  mode,
  database,
  schema,
  table,
  availableConnections = [],
  sourceObjects = [],
}: Props) {
  const { t } = useI18n();
  const sqlConnections = useMemo(
    () => availableConnections.filter(isSqlDbxConnection),
    [availableConnections],
  );
  const defaultTargetConnectionId = useMemo(
    () =>
      sqlConnections.find((connection) => connection.id !== connectionId)?.id ??
      sqlConnections.find((connection) => connection.id === connectionId)?.id ??
      "",
    [connectionId, sqlConnections],
  );
  const [targetConnectionId, setTargetConnectionId] = useState(defaultTargetConnectionId);
  const sourceConnection = useMemo(
    () => sqlConnections.find((connection) => connection.id === connectionId) ?? null,
    [connectionId, sqlConnections],
  );
  const [sourceDatabase, setSourceDatabase] = useState(database ?? "");
  const [sourceSchema, setSourceSchema] = useState(schema ?? "");
  const [targetDatabase, setTargetDatabase] = useState(database ?? "");
  const [targetSchema, setTargetSchema] = useState(schema ?? "");
  const [tablesText, setTablesText] = useState(
    table ??
      sourceObjects.find((object) => object.object_type.toLowerCase() === "table")?.name ??
      "",
  );
  const [resultText, setResultText] = useState("");
  const [loading, setLoading] = useState(false);
  const [activeTransferId, setActiveTransferId] = useState<string | null>(null);
  const [cancelRequested, setCancelRequested] = useState(false);
  const targetConnection = useMemo(
    () => sqlConnections.find((connection) => connection.id === targetConnectionId) ?? null,
    [sqlConnections, targetConnectionId],
  );

  useEffect(() => {
    setTargetConnectionId((current) => current || defaultTargetConnectionId);
  }, [defaultTargetConnectionId]);

  useEffect(() => {
    setSourceDatabase(database ?? "");
    setTargetDatabase(database ?? "");
  }, [database]);

  useEffect(() => {
    setSourceSchema(schema ?? "");
    setTargetSchema(schema ?? "");
  }, [schema]);

  useEffect(() => {
    if (!tablesText.trim()) {
      setTablesText(
        table ??
          sourceObjects.find((object) => object.object_type.toLowerCase() === "table")?.name ??
          "",
      );
    }
  }, [sourceObjects, table, tablesText]);

  const selectedTables = useMemo(() => tableNamesFromText(tablesText), [tablesText]);
  const missingReason = !connectionId
    ? t("database.selectDbxSqlConnection")
    : !sourceConnection
      ? t("database.sourceConnectionUnavailable")
      : !targetConnection
        ? t("database.selectTargetConnection")
        : selectedTables.length === 0
          ? t("database.selectDbxTable")
          : "";

  const sameTransferTarget =
    mode === "transfer" &&
    connectionId.trim() === targetConnectionId.trim() &&
    sourceDatabase.trim() === targetDatabase.trim() &&
    sourceSchema.trim() === targetSchema.trim();

  const transferTargetsSelf =
    Boolean(sourceConnection && targetConnection) &&
    sameTransferTarget &&
    selectedTables.length > 0;

  const effectiveMissingReason = transferTargetsSelf
    ? t("database.transferSameTarget")
    : missingReason;

  async function run() {
    if (effectiveMissingReason) {
      setResultText(effectiveMissingReason);
      return;
    }
    setLoading(true);
    setResultText("");
    try {
      if (mode === "transfer") {
        if (!targetConnection) return;
        const approved = await confirmDbxProductionOperation({
          connection: targetConnection,
          database: targetDatabase,
          operation: t("database.productionTransferOperation", {
            count: selectedTables.length,
            database: targetDatabase.trim() || t("database.defaultDatabase"),
          }),
          okLabel: t("database.startTransfer"),
          t,
        });
        if (!approved) return;
        const request = {
          transferId: `transfer:${Date.now()}`,
          sourceConnectionId: connectionId,
          sourceDatabase,
          sourceSchema,
          targetConnectionId,
          targetDatabase,
          targetSchema,
          tables: selectedTables,
          createTable: true,
          mode: "append" as const,
          batchSize: 500,
        };
        setActiveTransferId(request.transferId);
        setCancelRequested(false);
        setResultText(t("database.transferStarted"));
        const terminalProgress = await databaseApi.dbxStartTransfer(request, (progress) => {
          setResultText(transferProgressText(progress, t));
        });
        if (terminalProgress) setResultText(transferProgressText(terminalProgress, t));
      } else if (mode === "schema-diff") {
        if (!targetConnection) return;
        if (!sourceConnection) return;
        const [sourceMetadata, targetMetadata] = await Promise.all([
          loadToolMetadata(sourceConnection, sourceDatabase, sourceSchema, selectedTables),
          loadToolMetadata(targetConnection, targetDatabase, targetSchema, selectedTables),
        ]);
        const result = await databaseApi.dbxPrepareSchemaDiff({
          sourceTables: sourceMetadata.objects.map(objectToTableInfo),
          targetTables: targetMetadata.objects.map(objectToTableInfo),
          sourceDetails: detailsForMetadata(sourceMetadata, sourceSchema),
          targetDetails: detailsForMetadata(targetMetadata, targetSchema),
          sourceFunctions: [],
          targetFunctions: [],
          sourceSequences: [],
          targetSequences: [],
          sourceRules: [],
          targetRules: [],
          sourceOwners: [],
          targetOwners: [],
          databaseType: targetConnection.dbType,
          targetSchema,
          ignoreComments: false,
          cascadeDelete: false,
        });
        setResultText(JSON.stringify(result, null, 2));
      } else {
        if (!targetConnection) return;
        if (!sourceConnection) return;
        const sourceTable = selectedTables[0] ?? table ?? "";
        const [sourceColumns, targetColumns] = await Promise.all([
          databaseApi.dbxGetColumns(
            sourceConnection.id,
            sourceTable,
            sourceDatabase || null,
            sourceSchema || null,
          ),
          databaseApi.dbxGetColumns(
            targetConnection.id,
            sourceTable,
            targetDatabase || null,
            targetSchema || null,
          ),
        ]);
        const targetByName = new Map(targetColumns.map((column) => [column.name, column]));
        const sourcePrimaryKeys = sourceColumns
          .filter((column) => column.is_primary_key)
          .map((column) => column.name);
        const targetPrimaryKeys = targetColumns
          .filter((column) => column.is_primary_key)
          .map((column) => column.name);
        const targetPrimaryKeySet = new Set(targetPrimaryKeys);
        const samePrimaryKeys =
          sourcePrimaryKeys.length === targetPrimaryKeys.length &&
          sourcePrimaryKeys.every((name) => targetPrimaryKeySet.has(name));
        const columns = sourceColumns
          .filter((column) => targetByName.has(column.name))
          .map((column) => column.name);
        const keyColumns = sourceColumns
          .filter(
            (column) =>
              column.is_primary_key && targetByName.get(column.name)?.is_primary_key === true,
          )
          .map((column) => column.name);
        if (keyColumns.length === 0 || !samePrimaryKeys) {
          setResultText(t("database.dataCompareNoCommonPrimaryKey"));
          return;
        }
        const result = await databaseApi.dbxPrepareDataCompareFromTables({
          sourceConnectionId: connectionId,
          sourceDatabase,
          sourceSchema,
          sourceTable,
          targetConnectionId,
          targetDatabase,
          targetSchema,
          targetTable: sourceTable,
          columns,
          keyColumns,
          fetchBatchSize: 1000,
        });
        setResultText(JSON.stringify(result, null, 2));
      }
    } catch (err) {
      setResultText(String(err));
    } finally {
      setLoading(false);
      setActiveTransferId(null);
      setCancelRequested(false);
    }
  }

  async function cancelTransfer() {
    if (!activeTransferId || cancelRequested) return;
    setCancelRequested(true);
    try {
      await databaseApi.dbxCancelTransfer(activeTransferId);
    } catch (err) {
      setCancelRequested(false);
      setResultText(String(err));
    }
  }

  const title =
    mode === "transfer"
      ? t("database.dataTransfer")
      : mode === "schema-diff"
        ? t("database.schemaDiff")
        : t("database.dataCompare");

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{title}</div>
          <div style={s.databaseDialogHint}>{t("database.advancedToolsHint")}</div>
        </div>
        {mode === "transfer" && loading ? (
          <DbxButton
            variant="destructive"
            size="sm"
            icon={Square}
            onClick={() => void cancelTransfer()}
            disabled={!activeTransferId || cancelRequested}
          >
            {cancelRequested ? t("database.transferCancelling") : t("database.cancelTransfer")}
          </DbxButton>
        ) : (
          <DbxButton
            variant="default"
            size="sm"
            icon={loading ? RefreshCcw : Play}
            onClick={() => void run()}
            disabled={loading || Boolean(effectiveMissingReason)}
          >
            {mode === "transfer" ? t("database.startTransfer") : t("database.compare")}
          </DbxButton>
        )}
      </div>
      <div style={s.databaseDialogFormGrid}>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.sourceConnection")}</span>
          <input
            aria-label="Source connection"
            style={s.databaseDialogInput}
            value={connectionId}
            readOnly
          />
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.targetConnection")}</span>
          <select
            aria-label="Target connection"
            style={s.databaseDialogInput}
            value={targetConnectionId}
            onChange={(event) => setTargetConnectionId(event.target.value)}
          >
            <option value="">{t("database.chooseConnection")}</option>
            {sqlConnections.map((connection) => (
              <option key={connection.id} value={connection.id}>
                {connection.name}
              </option>
            ))}
          </select>
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.sourceDatabase")}</span>
          <input
            style={s.databaseDialogInput}
            value={sourceDatabase}
            onChange={(event) => setSourceDatabase(event.target.value)}
          />
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.targetDatabase")}</span>
          <input
            style={s.databaseDialogInput}
            value={targetDatabase}
            onChange={(event) => setTargetDatabase(event.target.value)}
          />
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.sourceSchema")}</span>
          <input
            style={s.databaseDialogInput}
            value={sourceSchema}
            onChange={(event) => setSourceSchema(event.target.value)}
          />
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.targetSchema")}</span>
          <input
            style={s.databaseDialogInput}
            value={targetSchema}
            onChange={(event) => setTargetSchema(event.target.value)}
          />
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.tables")}</span>
          <input
            aria-label="Tables"
            style={s.databaseDialogInput}
            value={tablesText}
            onChange={(event) => setTablesText(event.target.value)}
            placeholder={t("database.placeholder.tables")}
          />
        </label>
      </div>
      {effectiveMissingReason && <div style={s.databaseDialogHint}>{effectiveMissingReason}</div>}
      <pre style={s.databaseSqlPreview}>{resultText || t("database.comparePreviewEmpty")}</pre>
    </div>
  );
}
