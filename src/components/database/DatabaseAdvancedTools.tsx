import { useEffect, useMemo, useState } from "react";
import { Play, RefreshCcw } from "lucide-react";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import s from "../../styles";
import type {
  AeroricDbConnectionConfig,
  DbxColumnInfo,
  DbxDatabaseType,
  DbxObjectInfo,
} from "../../types";
import { DbxButton } from "./DbxButton";

export type DatabaseAdvancedToolMode = "transfer" | "schema-diff" | "data-compare";

interface Props {
  connectionId: string;
  mode: DatabaseAdvancedToolMode;
  database?: string | null;
  schema?: string | null;
  table?: string | null;
  availableConnections?: AeroricDbConnectionConfig[];
  sourceObjects?: DbxObjectInfo[];
  sourceColumnsByTable?: Record<string, DbxColumnInfo[]>;
  sourceDatabaseType?: DbxDatabaseType | null;
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
  return {
    name: object.name,
    table_type: object.object_type === "view" ? "VIEW" : "TABLE",
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

export function DatabaseAdvancedTools({
  connectionId,
  mode,
  database,
  schema,
  table,
  availableConnections = [],
  sourceObjects = [],
  sourceColumnsByTable = {},
  sourceDatabaseType,
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
  const [sourceDatabase, setSourceDatabase] = useState(database ?? "");
  const [sourceSchema, setSourceSchema] = useState(schema ?? "");
  const [targetDatabase, setTargetDatabase] = useState(database ?? "");
  const [targetSchema, setTargetSchema] = useState(schema ?? "");
  const [tablesText, setTablesText] = useState(
    table ?? sourceObjects.find((object) => object.object_type === "table")?.name ?? "",
  );
  const [resultText, setResultText] = useState("");
  const [loading, setLoading] = useState(false);

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
        table ?? sourceObjects.find((object) => object.object_type === "table")?.name ?? "",
      );
    }
  }, [sourceObjects, table, tablesText]);

  const selectedTables = useMemo(() => tableNamesFromText(tablesText), [tablesText]);
  const selectedSourceObjects = useMemo(() => {
    const wanted = new Set(selectedTables);
    const matched = sourceObjects.filter((object) => wanted.has(object.name));
    const matchedNames = new Set(matched.map((object) => object.name));
    const syntheticObjects = selectedTables
      .filter((tableName) => !matchedNames.has(tableName))
      .map((tableName) => ({
        name: tableName,
        object_type: "table",
        schema: sourceSchema || null,
      }));
    return [...matched, ...syntheticObjects];
  }, [selectedTables, sourceObjects, sourceSchema]);

  const missingReason = !connectionId
    ? t("database.selectDbxSqlConnection")
    : !targetConnectionId.trim()
      ? t("database.selectTargetConnection")
      : selectedTables.length === 0
        ? t("database.selectDbxTable")
        : "";

  function sourceDetails() {
    return selectedSourceObjects.map((object) => {
      const key = tableKey(object.schema ?? sourceSchema, object.name);
      return {
        name: object.name,
        columns: columnsForDetail(
          sourceColumnsByTable[key] ?? sourceColumnsByTable[object.name] ?? [],
        ),
        indexes: [],
        foreign_keys: [],
        triggers: [],
        ddl: null,
      };
    });
  }

  async function run() {
    if (missingReason) {
      setResultText(missingReason);
      return;
    }
    setLoading(true);
    setResultText("");
    try {
      if (mode === "transfer") {
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
          mode: "append",
          batchSize: 500,
        };
        await databaseApi.dbxStartTransfer(request);
        setResultText(t("database.transferStarted"));
      } else if (mode === "schema-diff") {
        const tableInfos = selectedSourceObjects.map(objectToTableInfo);
        const details = sourceDetails();
        const result = await databaseApi.dbxPrepareSchemaDiff({
          sourceTables: tableInfos,
          targetTables: tableInfos,
          sourceDetails: details,
          targetDetails: details,
          sourceFunctions: [],
          targetFunctions: [],
          sourceSequences: [],
          targetSequences: [],
          sourceRules: [],
          targetRules: [],
          sourceOwners: [],
          targetOwners: [],
          databaseType: sourceDatabaseType ?? "mysql",
          targetSchema,
          ignoreComments: false,
          cascadeDelete: false,
        });
        setResultText(JSON.stringify(result, null, 2));
      } else {
        const sourceTable = selectedTables[0] ?? table ?? "";
        const result = await databaseApi.dbxPrepareDataCompareFromTables({
          sourceConnectionId: connectionId,
          sourceDatabase,
          sourceSchema,
          sourceTable,
          targetConnectionId,
          targetDatabase,
          targetSchema,
          targetTable: sourceTable,
          columns: [],
          keyColumns: [],
          fetchBatchSize: 1000,
        });
        setResultText(JSON.stringify(result, null, 2));
      }
    } catch (err) {
      setResultText(String(err));
    } finally {
      setLoading(false);
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
        <DbxButton
          variant="default"
          size="sm"
          icon={loading ? RefreshCcw : Play}
          onClick={() => void run()}
          disabled={loading || Boolean(missingReason)}
        >
          {mode === "transfer" ? t("database.startTransfer") : t("database.compare")}
        </DbxButton>
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
            placeholder="users, orders"
          />
        </label>
      </div>
      {missingReason && <div style={s.databaseDialogHint}>{missingReason}</div>}
      <pre style={s.databaseSqlPreview}>{resultText || t("database.comparePreviewEmpty")}</pre>
    </div>
  );
}
