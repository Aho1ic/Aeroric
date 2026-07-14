import type { AeroricDbConnectionConfig, DbxDatabaseType } from "../../types";
import { parseConnectionUrl } from "./databaseConnectionUrl";

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export type DbProfileKey =
  | "sqlite"
  | "mysql"
  | "postgres"
  | "redis"
  | "mongodb"
  | "sqlserver"
  | "oracle"
  | "duckdb"
  | "clickhouse";

export type DbProfileViewMode = "icon" | "list";
export type DbWizardStep = "type" | "config";
export type DbConfigTab = "connection" | "tls" | "transport" | "advanced";
export type RedisConnectionMode = "standalone" | "sentinel" | "cluster";

export interface DbProfile {
  key: DbProfileKey;
  label: string;
  accent: string;
  port: number;
  user: string;
  localFile?: boolean;
  iconText: string;
}

export type TransportLayerDraft = {
  id: string;
  type: "ssh" | "proxy";
  enabled: boolean;
  name: string;
  host: string;
  port: string;
  user: string;
  username: string;
  password: string;
  keyPath: string;
  keyPassphrase: string;
  connectTimeoutSecs: string;
  exposeLan: boolean;
  useSshAgent: boolean;
  proxyType: "socks5" | "http";
};

export const DB_PROFILES: DbProfile[] = [
  {
    key: "sqlite",
    label: "SQLite",
    accent: "#3f8fce",
    port: 0,
    user: "",
    localFile: true,
    iconText: "S",
  },
  { key: "mysql", label: "MySQL", accent: "#2f6f9f", port: 3306, user: "root", iconText: "My" },
  {
    key: "postgres",
    label: "PostgreSQL",
    accent: "#336791",
    port: 5432,
    user: "postgres",
    iconText: "Pg",
  },
  { key: "redis", label: "Redis", accent: "#d82c20", port: 6379, user: "", iconText: "R" },
  { key: "mongodb", label: "MongoDB", accent: "#13aa52", port: 27017, user: "", iconText: "M" },
  {
    key: "sqlserver",
    label: "SQL Server",
    accent: "#cc2927",
    port: 1433,
    user: "sa",
    iconText: "MS",
  },
  { key: "oracle", label: "Oracle", accent: "#f80000", port: 1521, user: "system", iconText: "O" },
  {
    key: "duckdb",
    label: "DuckDB",
    accent: "#b68b00",
    port: 0,
    user: "",
    localFile: true,
    iconText: "D",
  },
  {
    key: "clickhouse",
    label: "ClickHouse",
    accent: "#d6a700",
    port: 8123,
    user: "default",
    iconText: "C",
  },
];

export function normalizeRedisNodeList(value: string): string {
  return value
    .split(/[\n,]+/)
    .map((node) => node.trim())
    .filter(Boolean)
    .join("\n");
}

function uniqueTrimmedValues(values: string[], caseInsensitive: boolean): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const trimmed = value.trim();
    if (!trimmed) continue;
    const key = caseInsensitive ? trimmed.toLowerCase() : trimmed;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(trimmed);
  }
  return result;
}

export function normalizeDelimitedList(value: string): string[] {
  return uniqueTrimmedValues(value.split(/[\n,]+/), true);
}

export function normalizeLineList(value: string): string[] {
  return uniqueTrimmedValues(value.split(/\r?\n/), false);
}

export function firstRedisEndpoint(
  nodes: string,
  defaultPort: number,
): { host: string; port: number } | null {
  const first = normalizeRedisNodeList(nodes).split("\n")[0]?.trim();
  if (!first) return null;
  const match = first.match(/^\[([^\]]+)\](?::(\d+))?$/) ?? first.match(/^([^:]+)(?::(\d+))?$/);
  if (!match) return null;
  const host = match[1]?.trim();
  if (!host) return null;
  const port = Number.parseInt(match[2] ?? "", 10);
  return { host, port: Number.isFinite(port) && port > 0 ? port : defaultPort };
}

export function createTransportLayerDraft(
  type: "ssh" | "proxy",
  index: number,
): TransportLayerDraft {
  return {
    id: `transport:${Date.now()}:${Math.random().toString(36).slice(2)}`,
    type,
    enabled: true,
    name: type === "ssh" ? `SSH ${index}` : `Proxy ${index}`,
    host: "",
    port: type === "ssh" ? "22" : "1080",
    user: "",
    username: "",
    password: "",
    keyPath: "",
    keyPassphrase: "",
    connectTimeoutSecs: "5",
    exposeLan: false,
    useSshAgent: false,
    proxyType: "socks5",
  };
}

export function transportLayerPayload(layer: TransportLayerDraft): Record<string, unknown> {
  const port = Number.parseInt(layer.port, 10);
  const connectTimeoutSecs = Number.parseInt(layer.connectTimeoutSecs, 10);
  if (layer.type === "proxy") {
    return {
      id: layer.id,
      type: "proxy",
      enabled: layer.enabled,
      name: layer.name.trim(),
      proxy_type: layer.proxyType,
      host: layer.host.trim(),
      port: Number.isFinite(port) && port > 0 ? port : 1080,
      username: layer.username.trim(),
      password: layer.password,
    };
  }
  return {
    id: layer.id,
    type: "ssh",
    enabled: layer.enabled,
    name: layer.name.trim(),
    host: layer.host.trim(),
    port: Number.isFinite(port) && port > 0 ? port : 22,
    user: layer.user.trim(),
    password: layer.password,
    key_path: layer.keyPath.trim(),
    key_passphrase: layer.keyPassphrase,
    connect_timeout_secs:
      Number.isFinite(connectTimeoutSecs) && connectTimeoutSecs > 0 ? connectTimeoutSecs : 5,
    expose_lan: layer.exposeLan,
    use_ssh_agent: layer.useSshAgent,
  };
}

export function dbxConfigRecord(
  connection: AeroricDbConnectionConfig | null | undefined,
): Record<string, unknown> {
  return connection?.dbx && typeof connection.dbx === "object"
    ? (connection.dbx as Record<string, unknown>)
    : {};
}

export function dbxString(config: Record<string, unknown>, key: string, fallback = ""): string {
  const value = config[key];
  return typeof value === "string" ? value : fallback;
}

export function dbxNumberString(
  config: Record<string, unknown>,
  key: string,
  fallback: string,
): string {
  const value = config[key];
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  if (typeof value === "string" && value.trim()) return value;
  return fallback;
}

export function dbxBoolean(
  config: Record<string, unknown>,
  key: string,
  fallback = false,
): boolean {
  const value = config[key];
  return typeof value === "boolean" ? value : fallback;
}

export function dbxStringList(config: Record<string, unknown>, key: string): string[] {
  const value = config[key];
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

export function profileForDbxConnection(connection: AeroricDbConnectionConfig): DbProfile {
  const config = dbxConfigRecord(connection);
  const driverProfile = dbxString(config, "driver_profile");
  return (
    DB_PROFILES.find((profile) => profile.key === driverProfile) ??
    DB_PROFILES.find((profile) => profile.key === connection.dbType) ??
    DB_PROFILES[0]
  );
}

export function transportLayerDraftFromPayload(value: unknown, index: number): TransportLayerDraft {
  const layer = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const type = layer.type === "proxy" ? "proxy" : "ssh";
  return {
    id: dbxString(layer, "id", `transport:existing:${index}`),
    type,
    enabled: dbxBoolean(layer, "enabled", true),
    name: dbxString(layer, "name", type === "ssh" ? `SSH ${index}` : `Proxy ${index}`),
    host: dbxString(layer, "host"),
    port: dbxNumberString(layer, "port", type === "ssh" ? "22" : "1080"),
    user: dbxString(layer, "user"),
    username: dbxString(layer, "username"),
    password: dbxString(layer, "password"),
    keyPath: dbxString(layer, "key_path"),
    keyPassphrase: dbxString(layer, "key_passphrase"),
    connectTimeoutSecs: dbxNumberString(layer, "connect_timeout_secs", "5"),
    exposeLan: dbxBoolean(layer, "expose_lan"),
    useSshAgent: dbxBoolean(layer, "use_ssh_agent"),
    proxyType: layer.proxy_type === "http" ? "http" : "socks5",
  };
}

export interface ConnectionDraft {
  profile: DbProfile;
  name: string;
  connectionGroup: string;
  color: string;
  host: string;
  port: string;
  user: string;
  database: string;
  password: string;
  filePath: string;
  readOnly: boolean;
  initScript: string;
  agentJavaOptions: string;
  isProduction: boolean;
  productionDatabases: string;
  urlParams: string;
  connectionString: string;
  connectTimeoutSecs: string;
  queryTimeoutSecs: string;
  idleTimeoutSecs: string;
  keepaliveIntervalSecs: string;
  caCertPath: string;
  clientCertPath: string;
  clientKeyPath: string;
  redisConnectionMode: RedisConnectionMode;
  redisSentinelMaster: string;
  redisSentinelNodes: string;
  redisSentinelUsername: string;
  redisSentinelPassword: string;
  redisSentinelTls: boolean;
  redisClusterNodes: string;
  redisKeySeparator: string;
  mongoUseUrl: boolean;
  tlsEnabled: boolean;
  tlsMode: string;
  oracleConnectionType: "service_name" | "sid";
  oracleSysdba: boolean;
  transportEnabled: boolean;
  transportLayers: TransportLayerDraft[];
}

export interface BuildDbxConnectionContext {
  editingConnection: AeroricDbConnectionConfig | null;
  projectRoot: string | null;
  t: TranslateFn;
}

export function buildDbxConnectionConfig(
  draft: ConnectionDraft,
  ctx: BuildDbxConnectionContext,
): AeroricDbConnectionConfig {
  const { editingConnection, projectRoot, t } = ctx;
  const selectedProfile = draft.profile;
  const now = Date.now();
  const id = editingConnection?.id ?? `dbx:${now}:${Math.random().toString(36).slice(2)}`;
  const existingDbx = editingConnection ? { ...dbxConfigRecord(editingConnection) } : {};
  delete existingDbx.oracle_sysdba;
  const port = Number.parseInt(draft.port, 10);
  const connectTimeoutSecs = Number.parseInt(draft.connectTimeoutSecs, 10);
  const queryTimeoutSecs = Number.parseInt(draft.queryTimeoutSecs, 10);
  const idleTimeoutSecs = Number.parseInt(draft.idleTimeoutSecs, 10);
  const keepaliveIntervalSecs = Number.parseInt(draft.keepaliveIntervalSecs, 10);
  let normalizedPort = Number.isFinite(port) && port >= 0 ? port : selectedProfile.port;
  const rawConnectionString = draft.connectionString.trim();
  const parsedMongoUrl =
    selectedProfile.key === "mongodb" && draft.mongoUseUrl && rawConnectionString
      ? parseConnectionUrl(rawConnectionString, "mongodb")
      : null;
  let database = draft.database.trim();
  const redisSentinelNodes = normalizeRedisNodeList(draft.redisSentinelNodes);
  const redisClusterNodes = normalizeRedisNodeList(draft.redisClusterNodes);
  let host = selectedProfile.localFile ? draft.filePath.trim() : draft.host.trim();
  let username = draft.user.trim();
  let password = draft.password;
  let urlParams = draft.urlParams.trim();
  if (selectedProfile.key === "postgres" && draft.tlsMode) {
    const params = new URLSearchParams(urlParams);
    params.set("sslmode", draft.tlsMode);
    urlParams = params.toString();
  } else if (selectedProfile.key === "mysql" && draft.tlsMode) {
    const params = new URLSearchParams(urlParams);
    params.set("ssl-mode", draft.tlsMode);
    urlParams = params.toString();
  }
  if (parsedMongoUrl) {
    host = parsedMongoUrl.host ?? host;
    const parsedMongoPort = Number.parseInt(parsedMongoUrl.port ?? "", 10);
    if (Number.isFinite(parsedMongoPort) && parsedMongoPort > 0) normalizedPort = parsedMongoPort;
    username = parsedMongoUrl.user ?? username;
    password = parsedMongoUrl.password ?? password;
    database = parsedMongoUrl.database ?? database;
    urlParams = parsedMongoUrl.urlParams ?? urlParams;
  }
  if (selectedProfile.key === "redis" && draft.redisConnectionMode === "sentinel") {
    const firstNode = firstRedisEndpoint(redisSentinelNodes, 26379);
    if (firstNode) {
      host = firstNode.host;
      normalizedPort = firstNode.port;
    }
  } else if (selectedProfile.key === "redis" && draft.redisConnectionMode === "cluster") {
    const firstNode = firstRedisEndpoint(redisClusterNodes, 6379);
    if (firstNode) {
      host = firstNode.host;
      normalizedPort = firstNode.port;
    }
  }
  if (!host) {
    throw new Error(
      selectedProfile.localFile ? t("database.filePathRequired") : t("database.hostRequired"),
    );
  }
  const name = draft.name.trim() || selectedProfile.label;
  const dbType = selectedProfile.key as DbxDatabaseType;
  const productionDatabases = normalizeDelimitedList(draft.productionDatabases);
  const isProduction =
    draft.isProduction || (Boolean(selectedProfile.localFile) && productionDatabases.length > 0);
  const dbx = {
    ...existingDbx,
    id,
    name,
    db_type: dbType,
    driver_profile: selectedProfile.key,
    driver_label: selectedProfile.label,
    color: draft.color,
    url_params: urlParams || null,
    host,
    port: normalizedPort,
    username,
    password,
    database: database || null,
    connection_string:
      dbType === "mongodb"
        ? draft.mongoUseUrl
          ? rawConnectionString || null
          : null
        : rawConnectionString || null,
    connect_timeout_secs:
      Number.isFinite(connectTimeoutSecs) && connectTimeoutSecs > 0 ? connectTimeoutSecs : 5,
    query_timeout_secs:
      Number.isFinite(queryTimeoutSecs) && queryTimeoutSecs >= 0 ? queryTimeoutSecs : 30,
    idle_timeout_secs:
      Number.isFinite(idleTimeoutSecs) && idleTimeoutSecs >= 0 ? idleTimeoutSecs : 60,
    keepalive_interval_secs:
      Number.isFinite(keepaliveIntervalSecs) && keepaliveIntervalSecs >= 0
        ? keepaliveIntervalSecs
        : 0,
    ssl: draft.tlsEnabled,
    ca_cert_path: draft.caCertPath.trim(),
    client_cert_path: draft.clientCertPath.trim(),
    client_key_path: draft.clientKeyPath.trim(),
    sysdba: dbType === "oracle" && draft.oracleSysdba,
    oracle_connection_type: dbType === "oracle" ? draft.oracleConnectionType : null,
    read_only: draft.readOnly,
    init_script: draft.initScript.trim() || null,
    agent_java_options: normalizeLineList(draft.agentJavaOptions),
    is_production: isProduction,
    production_databases: isProduction ? [] : productionDatabases,
    transport_layers: draft.transportEnabled
      ? draft.transportLayers.map(transportLayerPayload)
      : [],
    ...(dbType === "redis"
      ? {
          redis_connection_mode: draft.redisConnectionMode,
          redis_sentinel_master:
            draft.redisConnectionMode === "sentinel" ? draft.redisSentinelMaster.trim() : undefined,
          redis_sentinel_nodes:
            draft.redisConnectionMode === "sentinel" ? redisSentinelNodes : undefined,
          redis_sentinel_username:
            draft.redisConnectionMode === "sentinel"
              ? draft.redisSentinelUsername.trim()
              : undefined,
          redis_sentinel_password:
            draft.redisConnectionMode === "sentinel" ? draft.redisSentinelPassword : undefined,
          redis_sentinel_tls:
            draft.redisConnectionMode === "sentinel" ? draft.redisSentinelTls : undefined,
          redis_cluster_nodes:
            draft.redisConnectionMode === "cluster" ? redisClusterNodes : undefined,
          redis_key_separator: draft.redisKeySeparator.trim() || ":",
        }
      : {}),
  };

  return {
    id,
    name,
    dbType,
    readOnly: draft.readOnly,
    projectScope: editingConnection
      ? (editingConnection.projectScope ?? null)
      : projectRoot
        ? {
            kind: "local",
            projectRoot,
            remoteProjectPath: null,
            sshConnectionId: null,
          }
        : null,
    migratedFromLegacy: editingConnection?.migratedFromLegacy,
    connectionGroup: editingConnection
      ? (editingConnection.connectionGroup ?? null)
      : draft.connectionGroup.trim() || null,
    pinned: editingConnection?.pinned,
    dbx,
    createdAt: editingConnection?.createdAt ?? now,
    lastOpenedAt: now,
  };
}
