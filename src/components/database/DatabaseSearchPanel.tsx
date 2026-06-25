import { useMemo, useRef, useState } from "react";
import { RefreshCcw, Search, Square, Table2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import s from "../../styles";
import type {
  AeroricDbConnectionConfig,
  DatabaseSearchColumn,
  DbxColumnInfo,
  DbxObjectInfo,
} from "../../types";
import { DbxButton } from "./DbxButton";

interface SearchResultItem {
  id: string;
  object: DbxObjectInfo;
  matchedColumns: string[];
  preview: string;
  whereInput: string;
}

interface Props {
  connection: AeroricDbConnectionConfig;
  database: string | null;
  schema: string | null;
  objects: DbxObjectInfo[];
  onOpenResult: (object: DbxObjectInfo, whereInput: string) => void;
}

const MAX_TABLES = 200;

const TEXT_TYPES = [
  "char",
  "text",
  "clob",
  "varchar",
  "nvarchar",
  "nchar",
  "uuid",
  "uniqueidentifier",
  "enum",
];
const NUMBER_TYPES = [
  "int",
  "serial",
  "number",
  "numeric",
  "decimal",
  "float",
  "double",
  "real",
  "money",
];
const SKIPPED_TYPES = ["blob", "binary", "bytea", "image", "geometry", "geography"];

function normalizedObjectType(object: DbxObjectInfo): string {
  return object.object_type.toUpperCase().replace(/[\s-]+/g, "_");
}

function isSearchableTable(object: DbxObjectInfo): boolean {
  const objectType = normalizedObjectType(object);
  return objectType.includes("TABLE") && !objectType.includes("VIEW");
}

function columnForSearch(column: DbxColumnInfo): DatabaseSearchColumn {
  return {
    name: column.name,
    data_type: column.data_type,
    is_primary_key: column.is_primary_key,
  };
}

function isTextSearchColumn(column: DatabaseSearchColumn): boolean {
  const type = column.data_type.toLowerCase();
  if (SKIPPED_TYPES.some((skipped) => type.includes(skipped))) return false;
  return TEXT_TYPES.some((textType) => type.includes(textType));
}

function isNumericSearchColumn(column: DatabaseSearchColumn): boolean {
  const type = column.data_type.toLowerCase();
  if (SKIPPED_TYPES.some((skipped) => type.includes(skipped))) return false;
  return NUMBER_TYPES.some((numberType) => type.includes(numberType));
}

function parseNumericTerm(term: string): string | null {
  const trimmed = term.trim();
  return /^[+-]?(?:\d+|\d+\.\d+|\.\d+)$/.test(trimmed) ? trimmed : null;
}

function findMatchedColumns(
  resultColumns: string[],
  row: unknown[],
  columns: DatabaseSearchColumn[],
  term: string,
): string[] {
  const query = term.trim().toLowerCase();
  const numericTerm = parseNumericTerm(term);
  if (!query) return [];
  return resultColumns.filter((columnName, index) => {
    const value = row[index];
    if (value === null || value === undefined) return false;
    const column = columns.find((item) => item.name.toLowerCase() === columnName.toLowerCase());
    if (!column) return false;
    if (numericTerm && isNumericSearchColumn(column) && String(value).trim() === numericTerm)
      return true;
    return isTextSearchColumn(column) && String(value).toLowerCase().includes(query);
  });
}

function valuePreview(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  const text = String(value).replace(/\s+/g, " ").trim();
  return text.length > 90 ? `${text.slice(0, 90)}...` : text;
}

function rowPreview(resultColumns: string[], row: unknown[], matchedColumns: string[]): string {
  const columns = matchedColumns.length ? matchedColumns.slice(0, 3) : resultColumns.slice(0, 3);
  return columns
    .map((column) => {
      const index = resultColumns.findIndex((name) => name.toLowerCase() === column.toLowerCase());
      return index >= 0 ? `${column}: ${valuePreview(row[index])}` : "";
    })
    .filter(Boolean)
    .join(" | ");
}

function makeExecutionId(): string {
  return `database-search-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function DatabaseSearchPanel({
  connection,
  database,
  schema,
  objects,
  onOpenResult,
}: Props) {
  const { t } = useI18n();
  const [keyword, setKeyword] = useState("");
  const [limit, setLimit] = useState("20");
  const [running, setRunning] = useState(false);
  const [progressDone, setProgressDone] = useState(0);
  const [progressTotal, setProgressTotal] = useState(0);
  const [results, setResults] = useState<SearchResultItem[]>([]);
  const [errors, setErrors] = useState<Array<{ table: string; message: string }>>([]);
  const [limitedTables, setLimitedTables] = useState(false);
  const activeExecutionId = useRef("");
  const cancelled = useRef(false);
  const runId = useRef(0);

  const searchTables = useMemo(() => {
    const filtered = objects.filter(
      (object) => isSearchableTable(object) && (!schema || object.schema === schema),
    );
    return filtered.slice(0, MAX_TABLES);
  }, [objects, schema]);

  const parsedLimit = Math.min(100, Math.max(1, Number.parseInt(limit, 10) || 20));
  const canSearch = Boolean(database && keyword.trim() && !running && searchTables.length > 0);
  const scopeLabel = [connection.name, database, schema].filter(Boolean).join(" / ");

  async function stopSearch() {
    cancelled.current = true;
    runId.current += 1;
    setRunning(false);
    const executionId = activeExecutionId.current;
    activeExecutionId.current = "";
    if (executionId) {
      await databaseApi.dbxCancelQuery(executionId).catch(() => undefined);
    }
  }

  async function startSearch() {
    if (!canSearch || !database) return;
    const currentRun = ++runId.current;
    cancelled.current = false;
    setRunning(true);
    setResults([]);
    setErrors([]);
    setProgressDone(0);
    setProgressTotal(searchTables.length);
    setLimitedTables(
      objects.filter((object) => isSearchableTable(object) && (!schema || object.schema === schema))
        .length > MAX_TABLES,
    );

    try {
      for (const object of searchTables) {
        if (cancelled.current || currentRun !== runId.current) break;
        const tableSchema = object.schema ?? schema ?? null;
        try {
          const columns = (
            await databaseApi.dbxGetColumns(connection.id, object.name, database, tableSchema)
          ).map(columnForSearch);
          const query = await databaseApi.dbxBuildDatabaseSearchSql({
            databaseType: connection.dbType,
            schema: tableSchema,
            tableName: object.name,
            columns,
            term: keyword,
            limit: parsedLimit,
          });
          if (!query || cancelled.current || currentRun !== runId.current) continue;

          const executionId = makeExecutionId();
          activeExecutionId.current = executionId;
          const result = await databaseApi.dbxExecuteQuery({
            connectionId: connection.id,
            database,
            schema: tableSchema,
            sql: query.sql,
            maxRows: parsedLimit,
            executionId,
          });
          if (activeExecutionId.current === executionId) activeExecutionId.current = "";
          if (cancelled.current || currentRun !== runId.current) break;

          const nextItems: SearchResultItem[] = [];
          for (const [rowIndex, row] of result.rows.entries()) {
            const matchedColumns = findMatchedColumns(result.columns, row, columns, keyword);
            const whereInput = await databaseApi.dbxBuildSearchResultWhere({
              databaseType: connection.dbType,
              columns,
              resultColumns: result.columns,
              row,
              matchedColumns,
            });
            nextItems.push({
              id: `${object.schema ?? ""}.${object.name}:${rowIndex}:${nextItems.length}`,
              object,
              matchedColumns,
              preview: rowPreview(result.columns, row, matchedColumns),
              whereInput,
            });
          }
          if (nextItems.length) setResults((current) => [...current, ...nextItems]);
        } catch (err) {
          setErrors((current) => [
            ...current,
            {
              table: object.schema ? `${object.schema}.${object.name}` : object.name,
              message: String(err),
            },
          ]);
        } finally {
          setProgressDone((current) => current + 1);
        }
      }
    } finally {
      if (currentRun === runId.current) {
        setRunning(false);
        activeExecutionId.current = "";
      }
    }
  }

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{t("database.databaseSearch")}</div>
          <div style={s.databaseDialogHint}>
            {scopeLabel || t("database.selectDbxSqlConnection")}
          </div>
        </div>
      </div>

      <div style={s.databaseDialogFormGrid}>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.databaseSearchKeyword")}</span>
          <input
            aria-label={t("database.databaseSearchKeyword")}
            style={s.databaseDialogInput}
            value={keyword}
            disabled={running}
            placeholder={t("database.databaseSearchPlaceholder")}
            onChange={(event) => setKeyword(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void startSearch();
              }
            }}
          />
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.databaseSearchLimit")}</span>
          <input
            aria-label={t("database.databaseSearchLimit")}
            style={s.databaseDialogInput}
            type="number"
            min={1}
            max={100}
            value={limit}
            disabled={running}
            onChange={(event) => setLimit(event.target.value)}
          />
        </label>
        <div style={{ display: "flex", alignItems: "flex-end", gap: 8 }}>
          <DbxButton
            variant="default"
            size="sm"
            icon={running ? RefreshCcw : Search}
            disabled={!canSearch}
            onClick={() => void startSearch()}
          >
            {running ? t("database.databaseSearchSearching") : t("database.databaseSearchRun")}
          </DbxButton>
          {running && (
            <DbxButton
              variant="destructive"
              size="sm"
              icon={Square}
              onClick={() => void stopSearch()}
            >
              {t("database.databaseSearchStop")}
            </DbxButton>
          )}
        </div>
      </div>

      <div style={{ ...s.databaseDialogHint, marginTop: 12 }}>
        {t("database.databaseSearchProgress", {
          done: progressDone,
          total: progressTotal || searchTables.length,
        })}
        {limitedTables ? ` ${t("database.databaseSearchLimited", { count: MAX_TABLES })}` : ""}
      </div>

      {errors.length > 0 && (
        <div style={{ ...s.databaseEmpty, color: "var(--danger)", textAlign: "left" }}>
          {errors.slice(0, 3).map((error) => (
            <div key={`${error.table}:${error.message}`}>
              {error.table}: {error.message}
            </div>
          ))}
        </div>
      )}

      <div style={{ ...s.databaseWorkspaceHeader, marginTop: 14 }}>
        <div style={s.databaseWorkspaceTitle}>
          {t("database.databaseSearchResults", { count: results.length })}
        </div>
      </div>
      <div style={{ display: "grid", gap: 8 }}>
        {results.map((item) => (
          <DbxButton
            key={item.id}
            variant="outline"
            size="sm"
            icon={Table2}
            onClick={() => onOpenResult(item.object, item.whereInput)}
            style={{
              height: "auto",
              justifyContent: "flex-start",
              padding: "9px 10px",
              textAlign: "left",
            }}
          >
            <span style={{ minWidth: 0 }}>
              <span style={{ display: "block", color: "var(--text-primary)" }}>
                {item.object.schema
                  ? `${item.object.schema}.${item.object.name}`
                  : item.object.name}
                {item.matchedColumns.length ? ` (${item.matchedColumns.join(", ")})` : ""}
              </span>
              <span style={{ display: "block", color: "var(--text-hint)", marginTop: 2 }}>
                {item.preview}
              </span>
            </span>
          </DbxButton>
        ))}
        {results.length === 0 && (
          <div style={s.databaseEmpty}>
            {running ? t("database.databaseSearchWaiting") : t("database.databaseSearchNoResults")}
          </div>
        )}
      </div>
    </div>
  );
}
