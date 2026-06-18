import { useCallback, useEffect, useMemo, useState } from "react";
import { confirm, open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ChevronLeft,
  ChevronRight,
  Database,
  FileCode,
  FilePlus,
  Plus,
  Play,
  Plug,
  Shield,
  SlidersHorizontal,
  RefreshCcw,
  Server,
  Trash2,
  Wrench,
  GitCompare,
  GitMerge,
  Network,
  Table2,
  Copy,
} from "lucide-react";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DatabaseDriverManifest,
  DbxColumnInfo,
  DbxDatabaseType,
  DbxDatabaseInfo,
  DbxObjectInfo,
  DbxQueryResult,
  DbObject,
  DbQueryResult,
  DbSchema,
  SshConnection,
} from "../../types";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import s from "../../styles";
import { DatabaseAdvancedTools, type DatabaseAdvancedToolMode } from "./DatabaseAdvancedTools";
import { ErDiagramPanel } from "./ErDiagramPanel";
import { MongoBrowser } from "./MongoBrowser";
import { RedisBrowser } from "./RedisBrowser";
import { TableStructurePanel } from "./TableStructurePanel";

interface Props {
  projectRoot?: string;
  remoteConnection?: SshConnection;
  remoteProjectPath?: string;
  sshConnections?: SshConnection[];
}

const PAGE_SIZE = 100;
type DatabaseRow = { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }> };
type DbWizardStep = "type" | "config";
type DbConfigTab = "connection" | "tls" | "transport" | "advanced";
type DbWorkspaceMode =
  | "table"
  | "query"
  | "sql-file"
  | "drivers"
  | "redis"
  | "mongo"
  | "transfer"
  | "schema-diff"
  | "data-compare"
  | "er-diagram"
  | "table-structure";
type DatabaseContextMenuState = {
  x: number;
  y: number;
  connectionId: string;
  kind: "legacy" | "dbx";
} | null;
type DbProfileKey =
  | "sqlite"
  | "mysql"
  | "postgres"
  | "redis"
  | "mongodb"
  | "sqlserver"
  | "oracle"
  | "duckdb"
  | "clickhouse";

interface DbProfile {
  key: DbProfileKey;
  label: string;
  accent: string;
  port: number;
  user: string;
  localFile?: boolean;
  iconText: string;
}

const DB_PROFILES: DbProfile[] = [
  { key: "sqlite", label: "SQLite", accent: "#3f8fce", port: 0, user: "", localFile: true, iconText: "S" },
  { key: "mysql", label: "MySQL", accent: "#2f6f9f", port: 3306, user: "root", iconText: "My" },
  { key: "postgres", label: "PostgreSQL", accent: "#336791", port: 5432, user: "postgres", iconText: "Pg" },
  { key: "redis", label: "Redis", accent: "#d82c20", port: 6379, user: "", iconText: "R" },
  { key: "mongodb", label: "MongoDB", accent: "#13aa52", port: 27017, user: "", iconText: "M" },
  { key: "sqlserver", label: "SQL Server", accent: "#cc2927", port: 1433, user: "sa", iconText: "MS" },
  { key: "oracle", label: "Oracle", accent: "#f80000", port: 1521, user: "system", iconText: "O" },
  { key: "duckdb", label: "DuckDB", accent: "#b68b00", port: 0, user: "", localFile: true, iconText: "D" },
  { key: "clickhouse", label: "ClickHouse", accent: "#d6a700", port: 8123, user: "default", iconText: "C" },
];

function profileForDbType(dbType: string | null | undefined): DbProfile {
  return DB_PROFILES.find((profile) => profile.key === dbType) ?? DB_PROFILES[0];
}

function DatabaseProfileIcon({ profile, size = 23 }: { profile: DbProfile; size?: number }) {
  return (
    <span
      aria-hidden="true"
      style={{
        width: size,
        height: size,
        borderRadius: profile.key === "redis" ? 5 : profile.key === "mongodb" ? 999 : 7,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        background: `${profile.accent}22`,
        border: `1px solid ${profile.accent}66`,
        color: profile.accent,
        fontSize: Math.max(9, size * 0.42),
        fontWeight: 800,
        fontFamily: "var(--font-ui)",
        lineHeight: 1,
      }}
    >
      {profile.iconText}
    </span>
  );
}

function endpointLabel(endpoint: DbEndpoint): string {
  if (endpoint.kind === "local") return endpoint.path;
  return `${endpoint.connection.name}: ${endpoint.path}`;
}

function valueToText(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "string") return value;
  return String(value);
}

function textToCellValue(value: string): string | null {
  return value.trim().toUpperCase() === "NULL" ? null : value;
}

function rowKeyFor(row: DatabaseRow) {
  return {
    rowId: row.rowId ?? null,
    keyValues: row.keyValues
      .filter((item) => item.column !== "__aeroric_rowid__")
      .map((item) => ({
        column: item.column,
        value: item.value === null || item.value === undefined ? null : String(item.value),
      })),
  };
}

function createConnectionName(endpoint: DbEndpoint): string {
  const path = endpoint.kind === "local" ? endpoint.path : endpoint.path;
  const name = path.split("/").filter(Boolean).pop();
  return name || "SQLite";
}

function quoteSqlName(name: string): string {
  return `"${name.replace(/"/g, '""')}"`;
}

function dbxRowsToDatabaseRows(rows: unknown[][]) {
  return rows.map((row) => ({ rowId: null, keyValues: [], values: row }));
}

function dbxQueryToExecuteResult(result: DbxQueryResult, message?: string): DbExecuteResult {
  return {
    columns: result.columns,
    rows: dbxRowsToDatabaseRows(result.rows),
    rowsAffected: result.affected_rows,
    message: message ?? `${result.affected_rows} affected · ${result.execution_time_ms} ms`,
  };
}

function dbxSqlFileResultsToExecuteResult(results: DbxQueryResult[]): DbExecuteResult {
  const last = results[results.length - 1] ?? null;
  const statementCount = results.length;
  return {
    columns: last?.columns ?? [],
    rows: dbxRowsToDatabaseRows(last?.rows ?? []),
    rowsAffected: results.reduce((sum, item) => sum + (item.affected_rows ?? 0), 0),
    message: `${statementCount} statement${statementCount === 1 ? "" : "s"} executed`,
  };
}

function isSqlDbxConnection(connection: AeroricDbConnectionConfig | null | undefined) {
  return Boolean(connection && !["redis", "mongodb"].includes(connection.dbType));
}

function configuredTargetDatabase(connection: AeroricDbConnectionConfig | null | undefined): string | null {
  if (!connection?.dbx || typeof connection.dbx !== "object") return null;
  const database = (connection.dbx as { database?: unknown }).database;
  return typeof database === "string" && database.trim() ? database.trim() : null;
}

function dbxObjectKey(object: DbxObjectInfo) {
  return object.schema ? `${object.schema}.${object.name}` : object.name;
}

function GuidancePanel({ title, message }: { title: string; message: string }) {
  return (
    <div style={s.databaseWorkspacePanel}>
      <div>
        <div style={s.databaseWorkspaceTitle}>{title}</div>
        <div style={s.databaseDialogHint}>{message}</div>
      </div>
    </div>
  );
}

export function DatabaseView({
  projectRoot,
}: Props) {
  const { t } = useI18n();
  const [connections, setConnections] = useState<DbConnectionConfig[]>([]);
  const [dbxConnections, setDbxConnections] = useState<AeroricDbConnectionConfig[]>([]);
  const [activeConnectionId, setActiveConnectionId] = useState<string | null>(null);
  const [activeDbxConnectionId, setActiveDbxConnectionId] = useState<string | null>(null);
  const [schema, setSchema] = useState<DbSchema | null>(null);
  const [dbxDatabases, setDbxDatabases] = useState<DbxDatabaseInfo[]>([]);
  const [dbxObjects, setDbxObjects] = useState<DbxObjectInfo[]>([]);
  const [activeDbxDatabase, setActiveDbxDatabase] = useState<string | null>(null);
  const [activeDbxObject, setActiveDbxObject] = useState<DbxObjectInfo | null>(null);
  const [activeObject, setActiveObject] = useState<DbObject | null>(null);
  const [queryResult, setQueryResult] = useState<DbQueryResult | null>(null);
  const [sqlResult, setSqlResult] = useState<DbExecuteResult | null>(null);
  const [page, setPage] = useState(1);
  const [sql, setSql] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [workspaceMode, setWorkspaceMode] = useState<DbWorkspaceMode>("table");
  const [connectionDialogOpen, setConnectionDialogOpen] = useState(false);
  const [driverManifest, setDriverManifest] = useState<DatabaseDriverManifest | null>(null);
  const [wizardStep, setWizardStep] = useState<DbWizardStep>("type");
  const [selectedProfileKey, setSelectedProfileKey] = useState<DbProfileKey>("sqlite");
  const [configTab, setConfigTab] = useState<DbConfigTab>("connection");
  const [draftName, setDraftName] = useState("SQLite");
  const [draftHost, setDraftHost] = useState("127.0.0.1");
  const [draftPort, setDraftPort] = useState("0");
  const [draftUser, setDraftUser] = useState("");
  const [draftDatabase, setDraftDatabase] = useState("");
  const [draftPassword, setDraftPassword] = useState("");
  const [draftFilePath, setDraftFilePath] = useState("");
  const [draftReadOnly, setDraftReadOnly] = useState(false);
  const [draftUrlParams, setDraftUrlParams] = useState("");
  const [draftConnectionString, setDraftConnectionString] = useState("");
  const [draftConnectTimeoutSecs, setDraftConnectTimeoutSecs] = useState("15");
  const [draftQueryTimeoutSecs, setDraftQueryTimeoutSecs] = useState("300");
  const [draftCaCertPath, setDraftCaCertPath] = useState("");
  const [draftClientCertPath, setDraftClientCertPath] = useState("");
  const [draftClientKeyPath, setDraftClientKeyPath] = useState("");
  const [tlsEnabled, setTlsEnabled] = useState(false);
  const [transportEnabled, setTransportEnabled] = useState(false);
  const [sqlFilePath, setSqlFilePath] = useState("");
  const [sqlFilePreview, setSqlFilePreview] = useState("");
  const [sqlFileTimeoutSecs, setSqlFileTimeoutSecs] = useState("60");
  const [dbxColumnsByTable, setDbxColumnsByTable] = useState<Record<string, DbxColumnInfo[]>>({});
  const [contextMenu, setContextMenu] = useState<DatabaseContextMenuState>(null);

  const activeConnection = useMemo(
    () => connections.find((connection) => connection.id === activeConnectionId) ?? null,
    [activeConnectionId, connections],
  );
  const activeDbxConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === activeDbxConnectionId) ?? null,
    [activeDbxConnectionId, dbxConnections],
  );

  const activeEndpoint = activeConnection?.endpoint ?? null;
  const selectedProfile = DB_PROFILES.find((profile) => profile.key === selectedProfileKey) ?? DB_PROFILES[0];
  const dbxHasSqlObjectBrowser = isSqlDbxConnection(activeDbxConnection);
  const sqlDbxConnections = useMemo(() => dbxConnections.filter((connection) => isSqlDbxConnection(connection)), [dbxConnections]);
  const dbxTableObjects = useMemo(() => dbxObjects.filter((object) => object.object_type === "table"), [dbxObjects]);
  const selectedDbxTable = useMemo(
    () => activeDbxObject && activeDbxObject.object_type === "table" ? activeDbxObject : dbxTableObjects[0] ?? null,
    [activeDbxObject, dbxTableObjects],
  );
  const activeSqlCapable = Boolean(activeEndpoint || (activeDbxConnection && dbxHasSqlObjectBrowser));
  const tableRows = queryResult?.rows ?? sqlResult?.rows ?? [];
  const tableColumns = queryResult?.columns ?? sqlResult?.columns ?? [];
  const totalPages =
    queryResult?.totalRows && queryResult.totalRows > 0
      ? Math.max(1, Math.ceil(queryResult.totalRows / queryResult.pageSize))
      : null;
  const activeDbxTargetDatabase = configuredTargetDatabase(activeDbxConnection);
  const visibleDbxDatabases = activeDbxTargetDatabase
    ? dbxDatabases.filter((database) => database.name === activeDbxTargetDatabase)
    : dbxDatabases;

  const saveConnections = useCallback((next: DbConnectionConfig[]) => {
    setConnections(next);
    databaseApi.saveConnections(next).catch((err) => {
      setError(String(err));
    });
  }, []);

  const inspect = useCallback(
    async (connection: DbConnectionConfig) => {
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const nextSchema = await databaseApi.inspect(connection.endpoint, projectRoot);
        setSchema(nextSchema);
        const firstTable =
          nextSchema.objects.find((object) => object.objectType === "table") ??
          nextSchema.objects[0] ??
          null;
        setActiveObject(firstTable);
        setPage(1);
        if (firstTable) {
          const result = await databaseApi.queryTable(
            connection.endpoint,
            firstTable.name,
            1,
            PAGE_SIZE,
            projectRoot,
          );
          setQueryResult(result);
          setSql(`SELECT * FROM ${quoteSqlName(firstTable.name)}`);
        } else {
          setQueryResult(null);
        }
        if (connections.some((item) => item.id === connection.id)) {
          const now = Date.now();
          saveConnections(
            connections.map((item) =>
              item.id === connection.id ? { ...item, lastOpenedAt: now } : item,
            ),
          );
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [connections, projectRoot, saveConnections],
  );

  useEffect(() => {
    databaseApi.loadConnections()
      .then((items) => {
        setConnections(items);
        if (items[0]) {
          setActiveConnectionId(items[0].id);
          inspect(items[0]);
        }
      })
      .catch((err) => setError(String(err)));
    databaseApi.dbxListConnections()
      .then((items) => {
        setDbxConnections(items);
        if (!activeConnectionId && !items[0]) return;
      })
      .catch((err) => setError(String(err)));
    // Load once; inspect is intentionally not a dependency here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const addConnection = useCallback(
    (endpoint: DbEndpoint) => {
      const now = Date.now();
      const connection: DbConnectionConfig = {
        id: `db:${now}:${Math.random().toString(36).slice(2)}`,
        name: createConnectionName(endpoint),
        endpoint,
        readOnly: false,
        createdAt: now,
        lastOpenedAt: now,
      };
      const next = [connection, ...connections];
      saveConnections(next);
      setActiveConnectionId(connection.id);
      inspect(connection);
    },
    [connections, inspect, saveConnections],
  );

  const chooseLocalDbFile = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
      defaultPath: projectRoot,
    });
    if (typeof selected === "string") setDraftFilePath(selected);
  }, [projectRoot]);

  const resetConnectionDraft = useCallback((profile: DbProfile = selectedProfile) => {
    setWizardStep("type");
    setConfigTab("connection");
    setDraftName(profile.label);
    setDraftHost(profile.localFile ? "" : "127.0.0.1");
    setDraftPort(String(profile.port));
    setDraftUser(profile.user);
    setDraftDatabase("");
    setDraftPassword("");
    setDraftFilePath("");
    setDraftReadOnly(false);
    setDraftUrlParams("");
    setDraftConnectionString("");
    setDraftConnectTimeoutSecs("15");
    setDraftQueryTimeoutSecs("300");
    setDraftCaCertPath("");
    setDraftClientCertPath("");
    setDraftClientKeyPath("");
    setTlsEnabled(false);
    setTransportEnabled(false);
  }, [selectedProfile]);

  const openNewConnectionDialog = useCallback(() => {
    resetConnectionDraft(selectedProfile);
    setError(null);
    setConnectionDialogOpen(true);
  }, [resetConnectionDraft, selectedProfile]);

  const openDriverManager = useCallback(async () => {
    setWorkspaceMode("drivers");
    setError(null);
    setLoading(true);
    try {
      const manifest = await databaseApi.dbxDriverManifest();
      setDriverManifest(manifest);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const handleProfileSelect = useCallback((key: DbProfileKey) => {
    const profile = DB_PROFILES.find((item) => item.key === key) ?? DB_PROFILES[0];
    setSelectedProfileKey(key);
    resetConnectionDraft(profile);
  }, [resetConnectionDraft]);

  const buildDbxConnectionDraft = useCallback((): AeroricDbConnectionConfig => {
    const now = Date.now();
    const id = `dbx:${now}:${Math.random().toString(36).slice(2)}`;
    const port = Number.parseInt(draftPort, 10);
    const connectTimeoutSecs = Number.parseInt(draftConnectTimeoutSecs, 10);
    const queryTimeoutSecs = Number.parseInt(draftQueryTimeoutSecs, 10);
    const normalizedPort = Number.isFinite(port) && port >= 0 ? port : selectedProfile.port;
    const database = draftDatabase.trim();
    const host = selectedProfile.localFile ? draftFilePath.trim() : draftHost.trim();
    if (!host) {
      throw new Error(selectedProfile.localFile ? t("database.filePathRequired") : t("database.hostRequired"));
    }
    const name = draftName.trim() || selectedProfile.label;
    const dbType = selectedProfile.key as DbxDatabaseType;
    const dbx = {
      id,
      name,
      db_type: dbType,
      driver_profile: selectedProfile.key,
      driver_label: selectedProfile.label,
      url_params: draftUrlParams.trim() || null,
      host,
      port: normalizedPort,
      username: draftUser.trim(),
      password: draftPassword,
      database: database || null,
      connection_string: draftConnectionString.trim() || null,
      connect_timeout_secs: Number.isFinite(connectTimeoutSecs) && connectTimeoutSecs > 0 ? connectTimeoutSecs : 15,
      query_timeout_secs: Number.isFinite(queryTimeoutSecs) && queryTimeoutSecs > 0 ? queryTimeoutSecs : 300,
      ssl: tlsEnabled,
      ca_cert_path: draftCaCertPath.trim(),
      client_cert_path: draftClientCertPath.trim(),
      client_key_path: draftClientKeyPath.trim(),
      read_only: draftReadOnly,
      transport_layers: [],
    };

    return {
      id,
      name,
      dbType,
      readOnly: draftReadOnly,
      projectScope: projectRoot
        ? {
            kind: "local",
            projectRoot,
            remoteProjectPath: null,
            sshConnectionId: null,
          }
        : null,
      dbx,
      createdAt: now,
      lastOpenedAt: now,
    };
  }, [
    draftDatabase,
    draftCaCertPath,
    draftClientCertPath,
    draftClientKeyPath,
    draftConnectTimeoutSecs,
    draftConnectionString,
    draftFilePath,
    draftHost,
    draftName,
    draftPassword,
    draftPort,
    draftQueryTimeoutSecs,
    draftReadOnly,
    draftUrlParams,
    draftUser,
    projectRoot,
    selectedProfile.key,
    selectedProfile.label,
    selectedProfile.localFile,
    selectedProfile.port,
    t,
    tlsEnabled,
  ]);

  const handleNewQuery = useCallback(() => {
    if (!activeSqlCapable) return;
    setWorkspaceMode("query");
    setSql("");
    setSqlResult(null);
    setQueryResult(null);
    setActiveObject(null);
    setActiveDbxObject(null);
  }, [activeSqlCapable]);

  const chooseSqlFile = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "SQL", extensions: ["sql"] }],
      defaultPath: projectRoot,
    });
    if (typeof selected !== "string") return;
    setSqlFilePath(selected);
    try {
      const preview = await databaseApi.readSqlFile(selected);
      setSqlFilePreview(preview.slice(0, 8000));
    } catch {
      setSqlFilePreview("");
    }
  }, [projectRoot]);

  const handleExecuteSqlFile = useCallback(() => {
    setWorkspaceMode("sql-file");
    setError(activeSqlCapable ? null : t("database.selectSqlConnection"));
    setSqlResult(null);
    setQueryResult(null);
  }, [activeSqlCapable, t]);

  const openAdvancedTool = useCallback((mode: DatabaseAdvancedToolMode) => {
    setWorkspaceMode(mode);
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"));
    setSqlResult(null);
    setQueryResult(null);
  }, [activeDbxConnection, dbxHasSqlObjectBrowser, t]);

  const loadDbxColumnsForTables = useCallback(
    async (objects: DbxObjectInfo[], connection = activeDbxConnection, database = activeDbxDatabase) => {
      if (!connection || !isSqlDbxConnection(connection)) return;
      const nextColumns: Record<string, DbxColumnInfo[]> = {};
      for (const object of objects.filter((item) => item.object_type === "table").slice(0, 12)) {
        try {
          nextColumns[dbxObjectKey(object)] = await databaseApi.dbxGetColumns(
            connection.id,
            object.name,
            database,
            object.schema ?? null,
          );
        } catch {
          nextColumns[dbxObjectKey(object)] = [];
        }
      }
      setDbxColumnsByTable((current) => ({ ...current, ...nextColumns }));
    },
    [activeDbxConnection, activeDbxDatabase],
  );

  const openErDiagram = useCallback(async () => {
    setWorkspaceMode("er-diagram");
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"));
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser) return;
    let objects = dbxObjects;
    if (objects.length === 0) {
      try {
        objects = await databaseApi.dbxListObjects(activeDbxConnection.id, activeDbxDatabase, null);
        setDbxObjects(objects);
      } catch (err) {
        setError(String(err));
        return;
      }
    }
    await loadDbxColumnsForTables(objects, activeDbxConnection, activeDbxDatabase);
  }, [activeDbxConnection, activeDbxDatabase, dbxHasSqlObjectBrowser, dbxObjects, loadDbxColumnsForTables, t]);

  const openTableStructure = useCallback(async () => {
    setWorkspaceMode("table-structure");
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxTable"));
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser) return;
    const targetObject = selectedDbxTable;
    if (!targetObject) return;
    setActiveDbxObject(targetObject);
    try {
      await loadDbxColumnsForTables([targetObject], activeDbxConnection, activeDbxDatabase);
    } catch (err) {
      setError(String(err));
    }
  }, [activeDbxConnection, activeDbxDatabase, dbxHasSqlObjectBrowser, loadDbxColumnsForTables, selectedDbxTable, t]);

  const executeSqlFileFromPanel = useCallback(async () => {
    if (!sqlFilePath.trim()) return;
    setLoading(true);
    setError(null);
    try {
      if (activeDbxConnection && dbxHasSqlObjectBrowser) {
        const timeoutSecs = Number.parseInt(sqlFileTimeoutSecs, 10);
        const results = await databaseApi.dbxExecuteSqlFile({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          path: sqlFilePath.trim(),
          timeoutSecs: Number.isFinite(timeoutSecs) && timeoutSecs > 0 ? timeoutSecs : undefined,
        });
        setSqlResult(dbxSqlFileResultsToExecuteResult(results));
        setQueryResult(null);
      } else if (activeEndpoint) {
        const fileSql = await databaseApi.readSqlFile(sqlFilePath.trim());
        const result = await databaseApi.executeSql({
          endpoint: activeEndpoint,
          sql: fileSql,
          page: 1,
          pageSize: PAGE_SIZE,
          readOnly: activeConnection?.readOnly ?? false,
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
    activeConnection?.readOnly,
    activeDbxConnection,
    activeDbxDatabase,
    activeEndpoint,
    dbxHasSqlObjectBrowser,
    projectRoot,
    sqlFilePath,
    sqlFileTimeoutSecs,
  ]);

  const loadDbxDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null) => {
      setLoading(true);
      setError(null);
      try {
        const objects = await databaseApi.dbxListObjects(connection.id, database, null);
        setActiveDbxDatabase(database);
        setDbxObjects(objects);
        setActiveDbxObject(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const loadDbxConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setSchema(null);
      setActiveObject(null);
      setActiveDbxObject(null);
      setQueryResult(null);
      setSqlResult(null);
      setSql("");
      setWorkspaceMode("table");
      setError(null);
      setDbxDatabases([]);
      setDbxObjects([]);

      if (["redis", "mongodb"].includes(connection.dbType)) {
        setActiveDbxDatabase(null);
        setWorkspaceMode(connection.dbType === "redis" ? "redis" : "mongo");
        return;
      }

      setLoading(true);
      try {
        await databaseApi.dbxConnect(connection.id);
        const databases = await databaseApi.dbxListDatabases(connection.id);
        const targetDatabase = configuredTargetDatabase(connection);
        const visibleDatabases = targetDatabase
          ? databases.filter((database) => database.name === targetDatabase)
          : databases;
        if (targetDatabase && visibleDatabases.length === 0) {
          setActiveDbxDatabase(null);
          setError(t("database.configuredDatabaseMissing", { database: targetDatabase }));
          return;
        }
        setDbxDatabases(visibleDatabases);
        const database = targetDatabase ?? visibleDatabases[0]?.name ?? null;
        setActiveDbxDatabase(database);
        const objects = await databaseApi.dbxListObjects(connection.id, database, null);
        setDbxObjects(objects);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [t],
  );

  const loadDbxObject = useCallback(
    async (object: DbxObjectInfo, nextPage: number) => {
      if (!activeDbxConnection) return;
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const result = await databaseApi.dbxQueryTableData({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: object.schema ?? null,
          table: object.name,
          page: nextPage,
          pageSize: PAGE_SIZE,
        });
        setActiveDbxObject(object);
        setWorkspaceMode("table");
        setActiveObject({
          name: object.name,
          objectType: object.object_type,
          columns: [],
          indexes: [],
          foreignKeys: [],
          triggers: [],
          editable: false,
          primaryKeys: [],
          hasRowId: false,
        });
        setPage(nextPage);
        setQueryResult({
          columns: result.result.columns,
          rows: dbxRowsToDatabaseRows(result.result.rows),
          page: nextPage,
          pageSize: PAGE_SIZE,
          totalRows: result.totalRows ?? null,
          editable: false,
          primaryKeys: [],
          hasRowId: false,
        });
        setSql(result.sql);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeDbxConnection, activeDbxDatabase],
  );

  const submitConnection = useCallback(async () => {
    if (selectedProfile.key !== "sqlite") {
      setLoading(true);
      setError(null);
      try {
        const connection = buildDbxConnectionDraft();
        await databaseApi.dbxSaveConnection(connection);
        const next = await databaseApi.dbxListConnections();
        setDbxConnections(next);
        setConnectionDialogOpen(false);
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
      return;
    }
    const path = draftFilePath.trim();
    if (!path) return;
    addConnection({ kind: "local", path });
    setConnectionDialogOpen(false);
  }, [addConnection, buildDbxConnectionDraft, draftFilePath, loadDbxConnection, selectedProfile.key]);

  const testDbxConnectionDraft = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      await databaseApi.dbxTestConnection(buildDbxConnectionDraft());
      setError(t("database.connectionTestOk"));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [buildDbxConnectionDraft, t]);

  const handleSelectConnection = useCallback(
    (connection: DbConnectionConfig) => {
      setActiveDbxConnectionId(null);
      setDbxDatabases([]);
      setDbxObjects([]);
      setActiveDbxDatabase(null);
      setActiveDbxObject(null);
      setActiveConnectionId(connection.id);
      setWorkspaceMode("table");
      inspect(connection);
    },
    [inspect],
  );

  const handleSelectDbxConnection = useCallback(
    (connection: AeroricDbConnectionConfig) => {
      loadDbxConnection(connection);
    },
    [loadDbxConnection],
  );

  const handleDeleteConnection = useCallback(
    async (connectionId: string) => {
      const ok = await confirm(t("database.confirmDeleteConnection"), {
        title: t("database.deleteConnection"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      const next = connections.filter((connection) => connection.id !== connectionId);
      saveConnections(next);
      if (activeConnectionId === connectionId) {
        setActiveConnectionId(next[0]?.id ?? null);
        setSchema(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      }
    },
    [activeConnectionId, connections, saveConnections, t],
  );

  const handleDeleteDbxConnection = useCallback(
    async (connectionId: string) => {
      const ok = await confirm(t("database.confirmDeleteConnection"), {
        title: t("database.deleteConnection"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxDeleteConnection(connectionId);
        const next = await databaseApi.dbxListConnections();
        setDbxConnections(next);
        if (activeDbxConnectionId === connectionId) {
          setActiveDbxConnectionId(next[0]?.id ?? null);
          setSchema(null);
          setActiveObject(null);
          setQueryResult(null);
          setSqlResult(null);
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeDbxConnectionId, t],
  );

  const toggleReadOnly = useCallback(() => {
    if (!activeConnection) return;
    const next = connections.map((connection) =>
      connection.id === activeConnection.id
        ? { ...connection, readOnly: !connection.readOnly }
        : connection,
    );
    saveConnections(next);
  }, [activeConnection, connections, saveConnections]);

  const loadTable = useCallback(
    async (object: DbObject, nextPage: number) => {
      if (!activeEndpoint) return;
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const result = await databaseApi.queryTable(
          activeEndpoint,
          object.name,
          nextPage,
          PAGE_SIZE,
          projectRoot,
        );
        setActiveObject(object);
        setWorkspaceMode("table");
        setPage(nextPage);
        setQueryResult(result);
        setSql(`SELECT * FROM ${quoteSqlName(object.name)}`);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeEndpoint, projectRoot],
  );

  const refresh = useCallback(() => {
    if (activeConnection) inspect(activeConnection);
    if (activeDbxConnection) void loadDbxConnection(activeDbxConnection);
  }, [activeConnection, activeDbxConnection, inspect, loadDbxConnection]);

  const copyDbxConnection = useCallback(async (connection: AeroricDbConnectionConfig) => {
    const copy: AeroricDbConnectionConfig = {
      ...connection,
      id: `dbx:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      name: `${connection.name} Copy`,
      createdAt: Date.now(),
      lastOpenedAt: Date.now(),
    };
    await databaseApi.dbxSaveConnection(copy);
    setDbxConnections(await databaseApi.dbxListConnections());
  }, []);

  const copyLegacyConnection = useCallback(
    (connection: DbConnectionConfig) => {
      const now = Date.now();
      saveConnections([
        {
          ...connection,
          id: `db:${now}:${Math.random().toString(36).slice(2)}`,
          name: `${connection.name} Copy`,
          createdAt: now,
          lastOpenedAt: now,
        },
        ...connections,
      ]);
    },
    [connections, saveConnections],
  );

  const runContextMenuAction = useCallback(
    async (
      action:
        | "close"
        | "newQuery"
        | "executeSqlFile"
        | "refresh"
        | "copy"
        | "delete",
    ) => {
      const menu = contextMenu;
      setContextMenu(null);
      if (!menu) return;
      const legacy = connections.find((connection) => connection.id === menu.connectionId) ?? null;
      const dbx = dbxConnections.find((connection) => connection.id === menu.connectionId) ?? null;

      if (action === "newQuery") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        handleNewQuery();
        return;
      }
      if (action === "executeSqlFile") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        handleExecuteSqlFile();
        return;
      }
      if (action === "refresh") {
        if (legacy) await inspect(legacy);
        if (dbx) await loadDbxConnection(dbx);
        return;
      }
      if (action === "close") {
        if (dbx) await databaseApi.dbxDisconnect(dbx.id);
        if (activeDbxConnectionId === menu.connectionId) {
          setActiveDbxConnectionId(null);
          setDbxDatabases([]);
          setDbxObjects([]);
          setActiveDbxDatabase(null);
          setActiveDbxObject(null);
        }
        if (activeConnectionId === menu.connectionId) {
          setActiveConnectionId(null);
          setSchema(null);
          setActiveObject(null);
          setQueryResult(null);
        }
        return;
      }
      if (action === "copy") {
        if (legacy) copyLegacyConnection(legacy);
        if (dbx) await copyDbxConnection(dbx);
        return;
      }
      if (action === "delete") {
        if (legacy) handleDeleteConnection(legacy.id);
        if (dbx) await handleDeleteDbxConnection(dbx.id);
        return;
      }
    },
    [
      activeConnectionId,
      activeDbxConnectionId,
      connections,
      contextMenu,
      copyDbxConnection,
      copyLegacyConnection,
      dbxConnections,
      handleDeleteConnection,
      handleDeleteDbxConnection,
      handleExecuteSqlFile,
      handleNewQuery,
      handleSelectConnection,
      inspect,
      loadDbxConnection,
    ],
  );

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
          sql,
          pageSize: PAGE_SIZE,
        });
        setSqlResult(dbxQueryToExecuteResult(result));
        setQueryResult(null);
      } else if (activeEndpoint) {
        const result = await databaseApi.executeSql({
          endpoint: activeEndpoint,
          sql,
          page: 1,
          pageSize: PAGE_SIZE,
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        setSqlResult(result);
        setQueryResult(null);
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
    activeEndpoint,
    dbxHasSqlObjectBrowser,
    projectRoot,
    sql,
  ]);

  const updateCell = useCallback(
    async (
      row: DatabaseRow,
      column: string,
      value: string,
      original: string,
    ) => {
      if (!activeEndpoint || !activeObject || value === original || activeConnection?.readOnly) return;
      setError(null);
      try {
        await databaseApi.updateCell({
          endpoint: activeEndpoint,
          table: activeObject.name,
          rowKey: rowKeyFor(row),
          column,
          value: textToCellValue(value),
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        await loadTable(activeObject, page);
      } catch (err) {
        setError(String(err));
      }
    },
    [activeConnection?.readOnly, activeEndpoint, activeObject, loadTable, page, projectRoot],
  );

  const insertRow = useCallback(async () => {
    if (!activeEndpoint || !activeObject || activeConnection?.readOnly) return;
    setError(null);
    const sample = Object.fromEntries(
      activeObject.columns
        .filter((column) => !column.primaryKey && !column.defaultValue)
        .map((column) => [column.name, null]),
    );
    const raw = window.prompt(t("database.insertJsonPrompt"), JSON.stringify(sample));
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const values = Object.entries(parsed).map(([column, value]) => ({
        column,
        value: value === null || value === undefined ? null : String(value),
      }));
      await databaseApi.insertRow({
        endpoint: activeEndpoint,
        table: activeObject.name,
        values,
        readOnly: activeConnection?.readOnly ?? false,
        projectRoot,
      });
      await loadTable(activeObject, page);
    } catch (err) {
      setError(String(err));
    }
  }, [activeConnection?.readOnly, activeEndpoint, activeObject, loadTable, page, projectRoot, t]);

  const deleteRow = useCallback(
    async (row: DatabaseRow) => {
      if (!activeEndpoint || !activeObject || activeConnection?.readOnly) return;
      const ok = await confirm(t("database.confirmDeleteRow"), {
        title: t("database.deleteConnection"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      setError(null);
      try {
        await databaseApi.deleteRow({
          endpoint: activeEndpoint,
          table: activeObject.name,
          rowKey: rowKeyFor(row),
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        await loadTable(activeObject, page);
      } catch (err) {
        setError(String(err));
      }
    },
    [activeConnection?.readOnly, activeEndpoint, activeObject, loadTable, page, projectRoot, t],
  );

  return (
    <div style={s.databaseRoot}>
      <div style={s.databaseTopToolbar}>
        <button type="button" style={s.databaseToolbarButton} onClick={openNewConnectionDialog}>
          <Database size={14} />
          <span>{t("database.newConnection")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={handleNewQuery} disabled={!activeSqlCapable}>
          <FilePlus size={14} />
          <span>{t("database.newQuery")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={handleExecuteSqlFile} disabled={loading}>
          <FileCode size={14} />
          <span>{t("database.executeSqlFile")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={openDriverManager}>
          <Wrench size={14} />
          <span>{t("database.driverManager")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={() => openAdvancedTool("transfer")}>
          <GitMerge size={14} />
          <span>{t("database.dataTransfer")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={() => openAdvancedTool("schema-diff")}>
          <GitCompare size={14} />
          <span>{t("database.schemaDiff")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={() => openAdvancedTool("data-compare")}>
          <Network size={14} />
          <span>{t("database.dataCompare")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={() => void openErDiagram()}>
          <Table2 size={14} />
          <span>{t("database.erDiagram")}</span>
        </button>
        <button type="button" style={s.databaseToolbarButton} onClick={() => void openTableStructure()}>
          <SlidersHorizontal size={14} />
          <span>{t("database.tableStructure")}</span>
        </button>
      </div>
      <aside style={s.databaseSidebar}>
        <div style={s.databaseSidebarHeader}>
          <div style={s.databaseTitleRow}>
            <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
              <Database size={16} />
              <span style={s.databaseTitle}>{t("database.title")}</span>
            </div>
            <button type="button" style={s.databaseIconButton} onClick={refresh} disabled={(!activeConnection && !activeDbxConnection) || loading} title={t("common.refresh")}>
              <RefreshCcw size={13} />
            </button>
          </div>
          {activeConnection && (
            <button
              type="button"
              style={{
                ...s.databaseSmallButton,
                justifyContent: "flex-start",
                padding: "0 9px",
                background: activeConnection.readOnly ? "var(--warning-surface)" : "var(--bg-card)",
                color: activeConnection.readOnly ? "var(--warning)" : "var(--text-secondary)",
              }}
              onClick={toggleReadOnly}
            >
              <span>{activeConnection.readOnly ? t("database.readOnlyOn") : t("database.readOnlyOff")}</span>
            </button>
          )}
        </div>

        <div style={s.databaseScroll}>
          <div style={s.databaseSection}>
            <div style={s.databaseSectionTitle}>{t("database.connections")}</div>
            {connections.map((connection) => (
              <button
                key={connection.id}
                type="button"
                style={{
                  ...s.databaseListButton,
                  ...(connection.id === activeConnectionId ? s.databaseListButtonActive : {}),
                }}
                onClick={() => handleSelectConnection(connection)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setContextMenu({
                    x: event.clientX,
                    y: event.clientY,
                    connectionId: connection.id,
                    kind: "legacy",
                  });
                }}
              >
                {connection.endpoint.kind === "local" ? (
                  <DatabaseProfileIcon profile={profileForDbType("sqlite")} size={16} />
                ) : (
                  <Plug size={14} />
                )}
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {connection.name}
                </span>
                <Trash2
                  size={13}
                  onClick={(event) => {
                    event.stopPropagation();
                    handleDeleteConnection(connection.id);
                  }}
                />
              </button>
            ))}
            {dbxConnections.map((connection) => (
              <button
                key={connection.id}
                type="button"
                style={{
                  ...s.databaseListButton,
                  ...(connection.id === activeDbxConnectionId ? s.databaseListButtonActive : {}),
                }}
                onClick={() => handleSelectDbxConnection(connection)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setContextMenu({
                    x: event.clientX,
                    y: event.clientY,
                    connectionId: connection.id,
                    kind: "dbx",
                  });
                }}
              >
                <DatabaseProfileIcon profile={profileForDbType(connection.dbType)} size={16} />
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {connection.name}
                </span>
                <span style={{ color: "var(--text-hint)", fontSize: 10, textTransform: "uppercase" }}>{connection.dbType}</span>
                <Trash2
                  size={13}
                  onClick={(event) => {
                    event.stopPropagation();
                    handleDeleteDbxConnection(connection.id);
                  }}
                />
              </button>
            ))}
          </div>

          {activeDbxConnection && visibleDbxDatabases.length > 0 && (
            <div style={s.databaseSection}>
              <div style={s.databaseSectionTitle}>{t("database.databases")}</div>
              {visibleDbxDatabases.map((database) => (
                <button
                  key={database.name}
                  type="button"
                  style={{
                    ...s.databaseListButton,
                    ...(database.name === activeDbxDatabase ? s.databaseListButtonActive : {}),
                  }}
                  onClick={() => loadDbxDatabase(activeDbxConnection, database.name)}
                >
                  <Database size={14} />
                  <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {database.name}
                  </span>
                </button>
              ))}
            </div>
          )}

          <div style={s.databaseSection}>
            <div style={s.databaseSectionTitle}>{t("database.objects")}</div>
            {schema?.objects.map((object) => (
              <button
                key={`${object.objectType}:${object.name}`}
                type="button"
                style={{
                  ...s.databaseListButton,
                  ...(object.name === activeObject?.name ? s.databaseListButtonActive : {}),
                }}
                onClick={() => loadTable(object, 1)}
              >
                <Database size={14} />
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {object.name}
                </span>
                {object.rowCount != null && (
                  <span style={{ color: "var(--text-hint)", fontSize: 11 }}>{object.rowCount}</span>
                )}
              </button>
            ))}
            {activeDbxConnection && dbxHasSqlObjectBrowser && dbxObjects.map((object) => (
              <button
                key={`${object.object_type}:${object.schema ?? ""}:${object.name}`}
                type="button"
                style={{
                  ...s.databaseListButton,
                  ...(object.name === activeDbxObject?.name && object.schema === activeDbxObject?.schema
                    ? s.databaseListButtonActive
                    : {}),
                }}
                onClick={() => loadDbxObject(object, 1)}
              >
                <Database size={14} />
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {object.schema ? `${object.schema}.${object.name}` : object.name}
                </span>
                <span style={{ color: "var(--text-hint)", fontSize: 10, textTransform: "uppercase" }}>{object.object_type}</span>
              </button>
            ))}
            {activeDbxConnection && !dbxHasSqlObjectBrowser && (
              <div style={{ padding: "0 6px", display: "flex", flexDirection: "column", gap: 6 }}>
                <button
                  type="button"
                  style={s.databaseSmallButton}
                  onClick={() => setWorkspaceMode(activeDbxConnection.dbType === "redis" ? "redis" : "mongo")}
                >
                  {activeDbxConnection.dbType === "redis" ? "Redis" : "MongoDB"}
                </button>
              </div>
            )}
          </div>

          {activeObject && (
            <div style={s.databaseSection}>
              <div style={s.databaseSectionTitle}>{t("database.structure")}</div>
              <div style={{ padding: "0 6px", display: "flex", flexDirection: "column", gap: 8 }}>
                <div style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.5 }}>
                  {activeObject.primaryKeys.length > 0
                    ? t("database.primaryKeys", { keys: activeObject.primaryKeys.join(", ") })
                    : activeObject.hasRowId
                      ? t("database.rowidEditable")
                      : t("database.notEditable")}
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  {activeObject.columns.map((column) => (
                    <div key={column.name} style={{ fontSize: 11.5, color: "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={`${column.name} ${column.dataType}`}>
                      {column.primaryKey ? "* " : ""}
                      {column.name}
                      <span style={{ color: "var(--text-hint)" }}> {column.dataType || "TEXT"}</span>
                    </div>
                  ))}
                </div>
                {(activeObject.indexes.length > 0 || activeObject.foreignKeys.length > 0 || activeObject.triggers.length > 0) && (
                  <div style={{ fontSize: 11, color: "var(--text-hint)", lineHeight: 1.5 }}>
                    {t("database.objectStats", {
                      indexes: activeObject.indexes.length,
                      foreignKeys: activeObject.foreignKeys.length,
                      triggers: activeObject.triggers.length,
                    })}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </aside>

      <main style={s.databaseMain}>
        <div style={s.databaseTopbar}>
          <div style={{ minWidth: 0 }}>
            <div style={s.databaseTitle}>
              {workspaceMode === "query"
                ? t("database.newQuery")
                : workspaceMode === "sql-file"
                  ? t("database.executeSqlFile")
                  : workspaceMode === "drivers"
                    ? t("database.driverManager")
                    : workspaceMode === "redis"
                      ? "Redis"
                      : workspaceMode === "mongo"
                        ? "MongoDB"
                        : workspaceMode === "transfer"
                          ? t("database.dataTransfer")
                          : workspaceMode === "schema-diff"
                            ? t("database.schemaDiff")
                            : workspaceMode === "data-compare"
                              ? t("database.dataCompare")
                              : workspaceMode === "er-diagram"
                                ? t("database.erDiagram")
                                : workspaceMode === "table-structure"
                                  ? t("database.tableStructure")
                                  : activeObject?.name ?? t("database.noSelection")}
            </div>
            <div style={s.databasePath}>
              {activeEndpoint
                ? endpointLabel(activeEndpoint)
                : activeDbxConnection
                  ? `${activeDbxConnection.dbType}: ${activeDbxConnection.name}`
                  : t("database.chooseConnection")}
            </div>
          </div>
          {error && <div style={s.databaseError} title={error}>{error}</div>}
        </div>

        {workspaceMode === "sql-file" ? (
          <div style={s.databaseWorkspacePanel}>
            <div style={s.databaseWorkspaceHeader}>
              <div>
                <div style={s.databaseWorkspaceTitle}>{t("database.executeSqlFile")}</div>
                <div style={s.databaseDialogHint}>{t("database.sqlFileHint")}</div>
              </div>
              <button type="button" style={s.databaseSmallButton} onClick={chooseSqlFile} disabled={loading}>
                <FileCode size={13} />
                <span>{t("database.chooseSqlFile")}</span>
              </button>
            </div>
            <div style={s.databaseDialogFormGrid}>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.filePath")}</span>
                <input
                  style={s.databaseDialogInput}
                  value={sqlFilePath}
                  onChange={(event) => setSqlFilePath(event.target.value)}
                  placeholder="/path/to/script.sql"
                />
              </label>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.timeoutSecs")}</span>
                <input
                  style={s.databaseDialogInput}
                  value={sqlFileTimeoutSecs}
                  onChange={(event) => setSqlFileTimeoutSecs(event.target.value)}
                />
              </label>
            </div>
            <pre style={s.databaseSqlPreview}>
              {sqlFilePreview || t("database.sqlFilePreviewEmpty")}
            </pre>
            <div style={s.databaseDialogFooter}>
              <button
                type="button"
                style={s.databaseSmallButton}
                onClick={executeSqlFileFromPanel}
                disabled={loading || !sqlFilePath.trim() || !activeSqlCapable}
              >
                <Play size={13} />
                <span>{t("database.executeSqlFile")}</span>
              </button>
            </div>
          </div>
        ) : workspaceMode === "drivers" ? (
          <div style={s.databaseWorkspacePanel}>
            <div style={s.databaseWorkspaceHeader}>
              <div>
                <div style={s.databaseWorkspaceTitle}>{t("database.driverManager")}</div>
                <div style={s.databaseDialogHint}>{t("database.driverManagerHint")}</div>
              </div>
              <button type="button" style={s.databaseSmallButton} onClick={openDriverManager} disabled={loading}>
                <RefreshCcw size={13} />
                <span>{t("common.refresh")}</span>
              </button>
            </div>
            <div style={s.databaseTableWrap}>
              <table style={s.databaseTable}>
                <thead>
                  <tr>
                    <th style={s.databaseTh}>{t("database.driver")}</th>
                    <th style={s.databaseTh}>{t("database.runtime")}</th>
                    <th style={s.databaseTh}>{t("database.defaultPort")}</th>
                    <th style={s.databaseTh}>{t("database.supportLevel")}</th>
                    <th style={s.databaseTh}>{t("database.capabilities")}</th>
                  </tr>
                </thead>
                <tbody>
                  {(driverManifest?.drivers ?? []).map((driver) => {
                    const enabledCapabilities = Object.entries(driver.capabilities)
                      .filter(([, enabled]) => enabled)
                      .map(([key]) => key);
                    return (
                      <tr key={driver.dbType}>
                        <td style={s.databaseTd}>{driver.label}</td>
                        <td style={s.databaseTd}>{driver.runtimeMode}</td>
                        <td style={s.databaseTd}>{driver.defaultPort ?? "-"}</td>
                        <td style={s.databaseTd}>{driver.supportLevel}</td>
                        <td style={s.databaseTd} title={enabledCapabilities.join(", ")}>
                          {enabledCapabilities.slice(0, 6).join(", ")}
                          {enabledCapabilities.length > 6 ? "..." : ""}
                        </td>
                      </tr>
                    );
                  })}
                  {!driverManifest && (
                    <tr>
                      <td style={s.databaseTd} colSpan={5}>
                        {loading ? t("database.loading") : t("database.empty")}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        ) : workspaceMode === "redis" && activeDbxConnection ? (
          <RedisBrowser connectionId={activeDbxConnection.id} readOnly={activeDbxConnection.readOnly} />
        ) : workspaceMode === "mongo" && activeDbxConnection ? (
          <MongoBrowser connectionId={activeDbxConnection.id} readOnly={activeDbxConnection.readOnly} />
        ) : (workspaceMode === "transfer" || workspaceMode === "schema-diff" || workspaceMode === "data-compare") && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel
            title={
              workspaceMode === "transfer"
                ? t("database.dataTransfer")
                : workspaceMode === "schema-diff"
                  ? t("database.schemaDiff")
                  : t("database.dataCompare")
            }
            message={t("database.selectDbxSqlConnection")}
          />
        ) : (workspaceMode === "transfer" || workspaceMode === "schema-diff" || workspaceMode === "data-compare") && activeDbxConnection ? (
          <DatabaseAdvancedTools
            connectionId={activeDbxConnection.id}
            mode={workspaceMode}
            database={activeDbxDatabase}
            schema={selectedDbxTable?.schema ?? null}
            table={selectedDbxTable?.name ?? null}
            availableConnections={sqlDbxConnections}
            sourceObjects={dbxTableObjects}
            sourceColumnsByTable={dbxColumnsByTable}
            sourceDatabaseType={activeDbxConnection.dbType}
          />
        ) : workspaceMode === "er-diagram" && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel title={t("database.erDiagram")} message={t("database.selectDbxSqlConnection")} />
        ) : workspaceMode === "er-diagram" && activeDbxConnection ? (
          <ErDiagramPanel
            tables={dbxTableObjects}
            columnsByTable={dbxColumnsByTable}
          />
        ) : workspaceMode === "table-structure" && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel title={t("database.tableStructure")} message={t("database.selectDbxTable")} />
        ) : workspaceMode === "table-structure" && !selectedDbxTable ? (
          <GuidancePanel title={t("database.tableStructure")} message={t("database.selectDbxTable")} />
        ) : workspaceMode === "table-structure" && selectedDbxTable ? (
          <TableStructurePanel
            connectionId={activeDbxConnection?.id}
            database={activeDbxDatabase}
            schema={selectedDbxTable.schema ?? null}
            databaseType={activeDbxConnection?.dbType ?? null}
            tableName={selectedDbxTable.name}
            columns={
              dbxColumnsByTable[
                selectedDbxTable.schema ? `${selectedDbxTable.schema}.${selectedDbxTable.name}` : selectedDbxTable.name
              ] ?? []
            }
            readOnly={activeDbxConnection?.readOnly ?? true}
          />
        ) : (
          <>
        <div style={s.databaseSqlPanel}>
          <textarea
            style={s.databaseSqlInput}
            value={sql}
            onChange={(event) => setSql(event.target.value)}
            spellCheck={false}
            placeholder={t("database.sqlPlaceholder")}
          />
          <button type="button" style={{ ...s.databaseSmallButton, width: 86, height: "auto" }} onClick={runSql} disabled={!activeSqlCapable || loading}>
            <Play size={14} />
            <span>{t("database.run")}</span>
          </button>
        </div>

        <div style={s.databaseToolbar}>
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={!activeObject || !queryResult?.editable || activeConnection?.readOnly || loading}
            onClick={insertRow}
          >
            <Plus size={13} />
            <span>{t("database.insert")}</span>
          </button>
          <button
            type="button"
            style={s.databaseIconButton}
            disabled={!activeObject || page <= 1 || loading}
            onClick={() => {
              if (activeDbxConnection && activeDbxObject) {
                loadDbxObject(activeDbxObject, Math.max(1, page - 1));
              } else if (activeObject) {
                loadTable(activeObject, Math.max(1, page - 1));
              }
            }}
          >
            <ChevronLeft size={13} />
          </button>
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {t("database.page", {
              page,
              total: totalPages ?? "?",
            })}
          </span>
          <button
            type="button"
            style={s.databaseIconButton}
            disabled={!activeObject || loading || (totalPages != null && page >= totalPages)}
            onClick={() => {
              if (activeDbxConnection && activeDbxObject) {
                loadDbxObject(activeDbxObject, page + 1);
              } else if (activeObject) {
                loadTable(activeObject, page + 1);
              }
            }}
          >
            <ChevronRight size={13} />
          </button>
          <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
            {loading
              ? t("database.loading")
              : activeConnection?.readOnly
                ? t("database.readOnlyBadge")
                : sqlResult?.message}
          </span>
        </div>

        {tableColumns.length === 0 ? (
          <div style={s.databaseEmpty}>{t("database.empty")}</div>
        ) : (
          <div style={s.databaseTableWrap}>
            <table style={s.databaseTable}>
              <thead>
                <tr>
                  <th style={{ ...s.databaseTh, width: 86 }}>rowid</th>
                  {queryResult && <th style={{ ...s.databaseTh, width: 74 }}>{t("database.actions")}</th>}
                  {tableColumns.map((column) => (
                    <th key={column} style={s.databaseTh} title={column}>
                      {column}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {tableRows.map((row, rowIndex) => (
                  <tr key={`${row.rowId ?? "sql"}:${rowIndex}`}>
                    <td style={{ ...s.databaseTd, color: "var(--text-hint)" }}>{row.rowId ?? "-"}</td>
                    {queryResult && (
                      <td style={s.databaseTd}>
                        <button
                          type="button"
                          style={{ ...s.databaseSmallButton, height: 24, padding: "0 8px" }}
                          disabled={!queryResult.editable || activeConnection?.readOnly}
                          onClick={() => deleteRow(row)}
                        >
                          <Trash2 size={12} />
                        </button>
                      </td>
                    )}
                    {tableColumns.map((column, columnIndex) => {
                      const original = valueToText(row.values[columnIndex]);
                      const editable = Boolean(
                        queryResult &&
                          activeObject?.objectType === "table" &&
                          queryResult.editable &&
                          !activeConnection?.readOnly,
                      );
                      return (
                        <td key={`${column}:${columnIndex}`} style={s.databaseTd} title={original}>
                          {editable ? (
                            <input
                              style={s.databaseCellInput}
                              defaultValue={original}
                              onFocus={(event) => {
                                event.currentTarget.style.borderColor = "var(--border-focus)";
                                event.currentTarget.style.background = "var(--bg-input)";
                              }}
                              onBlur={(event) => {
                                event.currentTarget.style.borderColor = "transparent";
                                event.currentTarget.style.background = "transparent";
                                updateCell(row, column, event.currentTarget.value, original);
                              }}
                            />
                          ) : (
                            original
                          )}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
          </>
        )}
      </main>
      {connectionDialogOpen && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setConnectionDialogOpen(false);
          }}
        >
          <form
            role="dialog"
            aria-modal="true"
            aria-label={t("database.newConnection")}
            style={s.databaseConnectionDialog}
            onSubmit={(event) => {
              event.preventDefault();
              if (wizardStep === "type") {
                setWizardStep("config");
              } else {
                submitConnection();
              }
            }}
          >
            <div style={s.databaseDialogHeader}>{t("database.newConnection")}</div>
            <div style={s.databaseDialogBody}>
              {wizardStep === "type" ? (
                <>
                  <div style={s.databaseDialogIntro}>{t("database.chooseDatabaseType")}</div>
                  <div style={s.databaseTypeGrid}>
                    {DB_PROFILES.map((profile) => (
                      <button
                        key={profile.key}
                        type="button"
                        style={{
                          ...s.databaseTypeCard,
                          ...(selectedProfileKey === profile.key ? s.databaseTypeCardSelected : {}),
                        }}
                        onClick={() => handleProfileSelect(profile.key)}
                      >
                        <span style={{ ...s.databaseTypeIcon, background: `${profile.accent}1f`, color: profile.accent }}>
                          <DatabaseProfileIcon profile={profile} size={25} />
                        </span>
                        <span style={s.databaseTypeLabel}>{profile.label}</span>
                      </button>
                    ))}
                  </div>
                </>
              ) : (
                <>
                  <div style={s.databaseConfigHeader}>
                    <button type="button" style={s.databaseTypeMiniCard} onClick={() => setWizardStep("type")}>
                      <span style={{ ...s.databaseTypeIconSmall, background: `${selectedProfile.accent}1f`, color: selectedProfile.accent }}>
                        <DatabaseProfileIcon profile={selectedProfile} size={16} />
                      </span>
                      <span>{selectedProfile.label}</span>
                    </button>
                  </div>
                  <div style={s.databaseConfigTabs}>
                    {[
                      ["connection", t("database.connectionInfo"), Database],
                      ["tls", t("database.tlsSsl"), Shield],
                      ["transport", t("database.sshTunnelProxy"), Server],
                      ["advanced", t("database.advanced"), SlidersHorizontal],
                    ].map(([key, label, Icon]) => (
                      <button
                        key={key as string}
                        type="button"
                        style={{
                          ...s.databaseConfigTab,
                          ...(configTab === key ? s.databaseConfigTabActive : {}),
                        }}
                        onClick={() => setConfigTab(key as DbConfigTab)}
                      >
                        <Icon size={13} />
                        <span>{label as string}</span>
                      </button>
                    ))}
                  </div>
                  {configTab === "connection" ? (
                    <div style={s.databaseDialogFormGrid}>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.connectionName")}</span>
                        <input style={s.databaseDialogInput} value={draftName} onChange={(event) => setDraftName(event.target.value)} autoFocus />
                      </label>
                      {selectedProfile.localFile ? (
                        <label style={s.databaseDialogField}>
                          <span style={s.databaseDialogLabel}>{t("database.filePath")}</span>
                          <div style={s.databaseInputButtonRow}>
                            <input style={s.databaseDialogInput} value={draftFilePath} onChange={(event) => setDraftFilePath(event.target.value)} placeholder="/path/to/database.db" />
                            <button type="button" style={s.databaseIconButton} onClick={chooseLocalDbFile}>
                              <FilePlus size={13} />
                            </button>
                          </div>
                        </label>
                      ) : (
                        <>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.host")}</span>
                            <input style={s.databaseDialogInput} value={draftHost} onChange={(event) => setDraftHost(event.target.value)} />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.port")}</span>
                            <input style={s.databaseDialogInput} value={draftPort} onChange={(event) => setDraftPort(event.target.value)} />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.user")}</span>
                            <input style={s.databaseDialogInput} value={draftUser} onChange={(event) => setDraftUser(event.target.value)} />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.password")}</span>
                            <input style={s.databaseDialogInput} type="password" value={draftPassword} onChange={(event) => setDraftPassword(event.target.value)} />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.databaseName")}</span>
                            <input style={s.databaseDialogInput} value={draftDatabase} onChange={(event) => setDraftDatabase(event.target.value)} />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.connectionString")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftConnectionString}
                              onChange={(event) => setDraftConnectionString(event.target.value)}
                              placeholder="jdbc:postgresql://localhost:5432/postgres"
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.urlParams")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftUrlParams}
                              onChange={(event) => setDraftUrlParams(event.target.value)}
                              placeholder="sslmode=require&connectTimeout=15"
                            />
                          </label>
                        </>
                      )}
                    </div>
                  ) : configTab === "tls" ? (
                    <div style={s.databaseDialogPanel}>
                      <label style={s.databaseSwitchRow}>
                        <input type="checkbox" checked={tlsEnabled} onChange={(event) => setTlsEnabled(event.target.checked)} />
                        <span>{t("database.enableTls")}</span>
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.caCertPath")}</span>
                        <input style={s.databaseDialogInput} value={draftCaCertPath} onChange={(event) => setDraftCaCertPath(event.target.value)} />
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.clientCertPath")}</span>
                        <input style={s.databaseDialogInput} value={draftClientCertPath} onChange={(event) => setDraftClientCertPath(event.target.value)} />
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.clientKeyPath")}</span>
                        <input style={s.databaseDialogInput} value={draftClientKeyPath} onChange={(event) => setDraftClientKeyPath(event.target.value)} />
                      </label>
                      <div style={s.databaseDialogHint}>{t("database.tlsHint")}</div>
                    </div>
                  ) : configTab === "transport" ? (
                    <div style={s.databaseDialogPanel}>
                      <label style={s.databaseSwitchRow}>
                        <input type="checkbox" checked={transportEnabled} onChange={(event) => setTransportEnabled(event.target.checked)} />
                        <span>{t("database.enableTransport")}</span>
                      </label>
                      <div style={s.databaseDialogHint}>{t("database.transportHint")}</div>
                    </div>
                  ) : (
                    <div style={s.databaseDialogPanel}>
                      <label style={s.databaseSwitchRow}>
                        <input type="checkbox" checked={draftReadOnly} onChange={(event) => setDraftReadOnly(event.target.checked)} />
                        <span>{t("database.openReadOnly")}</span>
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.connectTimeoutSecs")}</span>
                        <input style={s.databaseDialogInput} value={draftConnectTimeoutSecs} onChange={(event) => setDraftConnectTimeoutSecs(event.target.value)} />
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.queryTimeoutSecs")}</span>
                        <input style={s.databaseDialogInput} value={draftQueryTimeoutSecs} onChange={(event) => setDraftQueryTimeoutSecs(event.target.value)} />
                      </label>
                      <div style={s.databaseDialogHint}>{t("database.advancedHint")}</div>
                    </div>
                  )}
                </>
              )}
            </div>
            <div style={s.databaseDialogFooter}>
              <button type="button" style={s.databaseSmallButton} onClick={() => setConnectionDialogOpen(false)}>
                {t("common.cancel")}
              </button>
              {wizardStep === "config" && selectedProfile.key !== "sqlite" && (
                <button
                  type="button"
                  style={s.databaseSmallButton}
                  onClick={testDbxConnectionDraft}
                  disabled={loading || (selectedProfile.localFile ? !draftFilePath.trim() : !draftHost.trim())}
                >
                  <Plug size={13} />
                  <span>{t("database.testConnection")}</span>
                </button>
              )}
              <button
                type="submit"
                style={s.databaseSmallButton}
                disabled={
                  loading ||
                  (wizardStep === "config" &&
                    (selectedProfile.localFile ? !draftFilePath.trim() : !draftHost.trim()))
                }
              >
                {wizardStep === "type" ? <ChevronRight size={13} /> : <Plus size={13} />}
                <span>{wizardStep === "type" ? t("database.next") : t("database.addConnection")}</span>
              </button>
            </div>
          </form>
        </div>
      )}
      {contextMenu && (
        <>
          <div style={s.fileCtxBackdrop} onClick={() => setContextMenu(null)} />
          <div
            role="menu"
            style={{
              ...s.fileCtxMenu,
              left: contextMenu.x,
              top: contextMenu.y,
              minWidth: 190,
            }}
          >
            {[
              ["close", "database.closeConnection"],
              ["newQuery", "database.newQuery"],
              ["executeSqlFile", "database.executeSqlFile"],
              ["refresh", "database.refresh"],
              ["copy", "database.copyConnection"],
              ["delete", "database.deleteConnection"],
            ].map(([action, labelKey]) => (
              <button
                key={action}
                type="button"
                role="menuitem"
                style={{ ...s.fileCtxMenuItem, display: "flex", alignItems: "center", gap: 8 }}
                onClick={() => {
                  void runContextMenuAction(action as Parameters<typeof runContextMenuAction>[0]);
                }}
              >
                {action === "copy" && <Copy size={13} />}
                <span>{t(labelKey)}</span>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
