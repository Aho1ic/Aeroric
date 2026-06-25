import { useCallback, useEffect, useMemo, useState } from "react";
import type { DragEvent, KeyboardEvent, MouseEvent } from "react";
import {
  Braces,
  ChevronDown,
  ChevronRight,
  Crosshair,
  Database,
  Eye,
  Folder,
  Hash,
  KeyRound,
  ListTree,
  Package,
  Pin,
  Plug,
  ScrollText,
  Search,
  Table2,
  Trash2,
  UsersRound,
  Zap,
} from "lucide-react";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbxColumnInfo,
  TableChildObjectType,
  DbObject,
  DbxDatabaseInfo,
  DbxObjectInfo,
  RedisDatabaseInfo,
  RedisKeyInfo,
} from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";
import { DbxButton } from "./DbxButton";

type SearchScope = "all" | "connections" | "databases" | "objects";
type DbxObjectGroupKey = "tables" | "views" | "procedures" | "functions" | "sequences" | "packages";
type RedisSidebarScanState = { cursor: number; totalKeys: number };

const databaseObjectNameCollator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: "base",
});
const EMPTY_PINNED_TREE_NODE_IDS = new Set<string>();

interface DatabaseSidebarTreeProps {
  connections: DbConnectionConfig[];
  dbxConnections: AeroricDbConnectionConfig[];
  extraDbxConnectionGroups?: string[];
  activeConnectionId: string | null;
  activeDbxConnectionId: string | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxSchema: string | null;
  activeObject: DbObject | null;
  activeDbxObject: DbxObjectInfo | null;
  userAdminActive: boolean;
  dbxHasSqlObjectBrowser: boolean;
  visibleDbxDatabases: DbxDatabaseInfo[];
  dbxSchemas: string[];
  legacyObjects: DbObject[];
  dbxObjects: DbxObjectInfo[];
  dbxColumnsByTable: Record<string, DbxColumnInfo[]>;
  redisDatabasesByConnection: Record<string, RedisDatabaseInfo[]>;
  redisKeysByDatabase: Record<string, RedisKeyInfo[]>;
  redisScanStateByDatabase: Record<string, RedisSidebarScanState>;
  mongoDatabasesByConnection: Record<string, string[]>;
  mongoCollectionsByDatabase: Record<string, string[]>;
  mongoDocumentsByCollection: Record<string, unknown[]>;
  mongoDocumentTotalsByCollection: Record<string, number>;
  activeMongoDocumentId: string | null;
  pinnedTreeNodeIds?: ReadonlySet<string>;
  onSelectConnection: (connection: DbConnectionConfig) => void;
  onSelectDbxConnection: (connection: AeroricDbConnectionConfig) => void;
  onDeleteConnection: (connectionId: string) => void;
  onDeleteDbxConnection: (connectionId: string) => void;
  onSelectDatabase: (connection: AeroricDbConnectionConfig, database: string | null) => void;
  onSelectDbxSchema: (
    connection: AeroricDbConnectionConfig,
    database: string,
    schema: string,
  ) => void;
  onSelectLegacyObject: (object: DbObject) => void;
  onSelectDbxObject: (object: DbxObjectInfo) => void;
  onOpenUserAdmin: (connection: AeroricDbConnectionConfig) => void;
  onOpenNoSqlWorkspace: () => void;
  onSelectRedisDatabase: (connection: AeroricDbConnectionConfig, database: number) => void;
  onExpandRedisDatabase: (connection: AeroricDbConnectionConfig, database: number) => void;
  onLoadMoreRedisKeys: (connection: AeroricDbConnectionConfig, database: number) => void;
  onSelectRedisKey: (
    connection: AeroricDbConnectionConfig,
    database: number,
    keyRaw: string,
  ) => void;
  onSelectMongoDatabase: (connection: AeroricDbConnectionConfig, database: string) => void;
  onExpandMongoDatabase: (connection: AeroricDbConnectionConfig, database: string) => void;
  onSelectMongoCollection: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
  ) => void;
  onExpandMongoCollection: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
  ) => void;
  onLoadMoreMongoDocuments: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
  ) => void;
  onSelectMongoDocument: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
    document: unknown,
  ) => void;
  onRenameConnection: (connection: DbConnectionConfig) => void;
  onRenameDbxConnection: (connection: AeroricDbConnectionConfig) => void;
  onRefreshConnection: (connection: DbConnectionConfig) => void;
  onRefreshDbxConnection: (connection: AeroricDbConnectionConfig) => void;
  onRefreshDatabase: (connection: AeroricDbConnectionConfig, database: string | null) => void;
  onRefreshDbxSchema: (
    connection: AeroricDbConnectionConfig,
    database: string,
    schema: string,
  ) => void;
  onCopyNodeName: (name: string) => void;
  onDropDatabase: (connection: AeroricDbConnectionConfig, database: string) => void;
  onDropDbxSchema: (
    connection: AeroricDbConnectionConfig,
    database: string,
    schema: string,
  ) => void;
  onDropDbxObject: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
  ) => void;
  onDropDbxColumn: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    column: DbxColumnInfo,
  ) => void;
  onDropDbxTableChildObject: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    childObject: DbxObjectInfo,
  ) => void;
  onConnectionContextMenu: (
    event: MouseEvent,
    connectionId: string,
    kind: "legacy" | "dbx",
  ) => void;
  onConnectionGroupContextMenu: (event: MouseEvent, groupName: string) => void;
  onUserAdminContextMenu: (event: MouseEvent, connectionId: string) => void;
  onDbxDatabaseContextMenu: (event: MouseEvent, connectionId: string, database: string) => void;
  onDbxSchemaContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: string,
    schema: string,
  ) => void;
  onDbxObjectContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: string | null,
    object: DbxObjectInfo,
  ) => void;
  onDbxColumnContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: string | null,
    object: DbxObjectInfo,
    column: DbxColumnInfo,
  ) => void;
  onDbxTableChildObjectContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: string | null,
    object: DbxObjectInfo,
    childObject: DbxObjectInfo,
  ) => void;
  onDbxObjectGroupContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: string,
    schema: string | null,
    groupKey: DbxObjectGroupKey,
    label: string,
  ) => void;
  onRedisDatabaseContextMenu: (event: MouseEvent, connectionId: string, database: number) => void;
  onRedisKeyContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: number,
    keyRaw: string,
  ) => void;
  onMongoDatabaseContextMenu: (event: MouseEvent, connectionId: string, database: string) => void;
  onMongoCollectionContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: string,
    collection: string,
  ) => void;
  onMongoDocumentContextMenu: (
    event: MouseEvent,
    connectionId: string,
    database: string,
    collection: string,
    document: unknown,
  ) => void;
}

const CONNECTION_BADGE_COLORS = [
  "#2563eb",
  "#0f766e",
  "#7c3aed",
  "#ca8a04",
  "#dc2626",
  "#0284c7",
  "#16a34a",
  "#c2410c",
] as const;

function stableNameHash(value: string) {
  let hash = 0;
  for (const char of value) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return hash;
}

function connectionConfigColor(connection: DbConnectionConfig | AeroricDbConnectionConfig) {
  const directColor = (connection as { color?: unknown }).color;
  if (typeof directColor === "string" && directColor.trim()) return directColor.trim();
  const dbx = (connection as AeroricDbConnectionConfig).dbx;
  if (dbx && typeof dbx === "object") {
    const color = (dbx as { color?: unknown }).color;
    if (typeof color === "string" && color.trim()) return color.trim();
  }
  return null;
}

function connectionBadgeColor(connection: DbConnectionConfig | AeroricDbConnectionConfig) {
  return (
    connectionConfigColor(connection) ??
    CONNECTION_BADGE_COLORS[stableNameHash(connection.name) % CONNECTION_BADGE_COLORS.length]
  );
}

function connectionBadgeText(name: string) {
  const trimmed = name.trim();
  if (!trimmed) return "DB";
  if (/^(localhost|127(?:\.\d{1,3}){3}|::1|\d{1,3}(?:\.\d{1,3}){3})$/i.test(trimmed)) return "IP";

  const hanText = Array.from(trimmed.match(/\p{Script=Han}/gu)?.join("") ?? "")
    .slice(0, 2)
    .join("");
  if (hanText) return hanText;

  const words = trimmed.match(/[A-Za-z0-9]+/g) ?? [];
  if (words.length >= 2)
    return words
      .slice(0, 2)
      .map((word) => word[0])
      .join("")
      .toUpperCase();
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return Array.from(trimmed).slice(0, 2).join("").toUpperCase();
}

function ConnectionNameBadge({
  connection,
  size = 22,
}: {
  connection: DbConnectionConfig | AeroricDbConnectionConfig;
  size?: number;
}) {
  const text = connectionBadgeText(connection.name);
  const color = connectionBadgeColor(connection);
  return (
    <span
      aria-hidden="true"
      style={{
        ...s.databaseConnectionNameBadge,
        width: size,
        height: size,
        background: `${color}22`,
        border: `1px solid ${color}77`,
        color,
        fontSize: Math.max(9, size * (text.length > 2 ? 0.34 : 0.42)),
      }}
    >
      {text}
    </span>
  );
}

function dbxObjectLabel(object: DbxObjectInfo) {
  return object.schema ? `${object.schema}.${object.name}` : object.name;
}

function dbxObjectDisplayName(object: DbxObjectInfo) {
  return object.name;
}

function dbxObjectKey(object: DbxObjectInfo) {
  return object.schema ? `${object.schema}.${object.name}` : object.name;
}

function legacyConnectionNodeKey(connection: DbConnectionConfig) {
  return `legacy-connection:${connection.id}`;
}

function legacyObjectNodeKey(object: DbObject) {
  return `legacy-object:${object.objectType}:${object.name}`;
}

function dbxConnectionGroupNodeKey(groupName: string) {
  return `dbx-group:${groupName}`;
}

function dbxUserAdminNodeKey(connectionId: string) {
  return `dbx-user-admin:${connectionId}`;
}

function dbxConnectionNodeKey(connection: AeroricDbConnectionConfig) {
  return `dbx-connection:${connection.id}`;
}

function dbxDatabaseNodeKey(databaseName: string) {
  return `dbx-database:${databaseName}`;
}

function dbxSchemaNodeKey(databaseName: string, schemaName: string) {
  return `dbx-schema:${databaseName}:${schemaName}`;
}

function dbxObjectGroupNodeKey(scopeKey: string, groupKey: DbxObjectGroupKey) {
  return `dbx-object-group:${scopeKey}:${groupKey}`;
}

function dbxObjectNodeKey(object: DbxObjectInfo) {
  return `dbx-object:${normalizeDbxObjectType(object.object_type)}:${object.schema ?? ""}:${object.name}`;
}

function dbxColumnNodeKey(object: DbxObjectInfo, column: DbxColumnInfo) {
  return `dbx-column:${dbxObjectKey(object)}:${column.name}`;
}

function dbxTableChildObjectNodeKey(object: DbxObjectInfo, childObject: DbxObjectInfo) {
  return `dbx-table-child:${dbxObjectKey(object)}:${normalizeDbxObjectType(childObject.object_type)}:${childObject.name}`;
}

function redisDatabaseNodeKey(connectionId: string, database: number) {
  return `redis-database:${connectionId}:${database}`;
}

function redisKeyNodeKey(connectionId: string, database: number, keyRaw: string) {
  return `redis-key:${connectionId}:${database}:${keyRaw}`;
}

function mongoDatabaseNodeKey(connectionId: string, database: string) {
  return `mongo-database:${connectionId}:${database}`;
}

function mongoCollectionNodeKey(connectionId: string, database: string, collection: string) {
  return `mongo-collection:${connectionId}:${database}:${collection}`;
}

function mongoDocumentId(document: unknown, fallback: number) {
  if (document && typeof document === "object" && "_id" in document)
    return String((document as { _id: unknown })._id);
  return `#${fallback + 1}`;
}

function mongoDocumentNodeKey(
  connectionId: string,
  database: string,
  collection: string,
  document: unknown,
  index: number,
) {
  return `mongo-document:${connectionId}:${database}:${collection}:${mongoDocumentId(document, index)}`;
}

function mongoDocumentLoadMoreNodeKey(connectionId: string, database: string, collection: string) {
  return `mongo-document-load-more:${connectionId}:${database}:${collection}`;
}

function mongoDocumentPreview(document: unknown, index: number) {
  if (!document || typeof document !== "object") return String(document ?? `Document ${index + 1}`);
  const record = document as Record<string, unknown>;
  const id = mongoDocumentId(document, index);
  const previewFields = Object.entries(record)
    .filter(([key]) => key !== "_id")
    .slice(0, 2)
    .map(
      ([key, value]) =>
        `${key}: ${typeof value === "object" ? JSON.stringify(value) : String(value)}`,
    );
  return previewFields.length > 0 ? `${id} ${previewFields.join(", ")}` : id;
}

function normalizeDbxObjectType(objectType: string) {
  return objectType.toUpperCase().replace(/[\s-]+/g, "_");
}

function dbxTableChildObjectType(
  object: DbxObjectInfo,
): Exclude<TableChildObjectType, "COLUMN"> | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType.includes("FOREIGN_KEY")) return "FOREIGN_KEY";
  if (objectType.includes("TRIGGER")) return "TRIGGER";
  if (objectType.includes("INDEX")) return "INDEX";
  return null;
}

function isDbxTableChildObject(object: DbxObjectInfo) {
  return Boolean(dbxTableChildObjectType(object));
}

function sameDatabaseName(left: string | null | undefined, right: string | null | undefined) {
  return (left ?? "").trim().toLowerCase() === (right ?? "").trim().toLowerCase();
}

function dbxChildObjectBelongsToTable(childObject: DbxObjectInfo, tableObject: DbxObjectInfo) {
  if (!childObject.parent_name || !sameDatabaseName(childObject.parent_name, tableObject.name))
    return false;
  if (!tableObject.schema) return true;
  return (
    !childObject.parent_schema || sameDatabaseName(childObject.parent_schema, tableObject.schema)
  );
}

function dbxChildObjectSearchText(childObject: DbxObjectInfo, parentObject: DbxObjectInfo) {
  return `${normalizeDbxObjectType(childObject.object_type)} ${dbxObjectLabel(parentObject)} ${childObject.name}`;
}

function canDragDbxObjectReference(object: DbxObjectInfo) {
  const objectType = normalizeDbxObjectType(object.object_type);
  return objectType === "TABLE" || objectType === "VIEW";
}

const DBX_OBJECT_GROUPS: Array<{
  key: DbxObjectGroupKey;
  labelKey: string;
  objectTypes: string[];
}> = [
  { key: "tables", labelKey: "database.tables", objectTypes: ["TABLE"] },
  { key: "views", labelKey: "database.views", objectTypes: ["VIEW"] },
  { key: "procedures", labelKey: "database.procedures", objectTypes: ["PROCEDURE"] },
  { key: "functions", labelKey: "database.functions", objectTypes: ["FUNCTION"] },
  { key: "sequences", labelKey: "database.sequences", objectTypes: ["SEQUENCE"] },
  { key: "packages", labelKey: "database.packages", objectTypes: ["PACKAGE", "PACKAGE_BODY"] },
];

function groupDbxObjects(objects: DbxObjectInfo[]) {
  const seen = new Set<string>();
  const uniqueObjects = objects.filter((object) => {
    const objectType = normalizeDbxObjectType(object.object_type);
    const key = `${objectType}\0${(object.schema ?? "").toLowerCase()}\0${object.name.toLowerCase()}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });

  return DBX_OBJECT_GROUPS.map((group) => {
    const supportedTypes = new Set(group.objectTypes);
    return {
      ...group,
      objects: uniqueObjects
        .filter((object) => supportedTypes.has(normalizeDbxObjectType(object.object_type)))
        .sort((left, right) =>
          databaseObjectNameCollator.compare(dbxObjectLabel(left), dbxObjectLabel(right)),
        ),
    };
  }).filter((group) => group.objects.length > 0);
}

function getDefaultDatabase(connection: AeroricDbConnectionConfig): string | null {
  const config = connection.dbx ?? connection;
  const value = (config as { database?: unknown }).database;
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function matchesSearch(value: string, query: string) {
  return !query || value.toLowerCase().includes(query);
}

function scopeAllows(scope: SearchScope, kind: Exclude<SearchScope, "all">) {
  return scope === "all" || scope === kind;
}

function connectionGroupName(connection: AeroricDbConnectionConfig): string {
  return connection.connectionGroup?.trim() ?? "";
}

function sortDbxConnections(left: AeroricDbConnectionConfig, right: AeroricDbConnectionConfig) {
  if (Boolean(left.pinned) !== Boolean(right.pinned)) return left.pinned ? -1 : 1;
  return databaseObjectNameCollator.compare(left.name, right.name);
}

function orderPinnedFirst<T>(items: T[], isPinned: (item: T) => boolean) {
  const pinned: T[] = [];
  const unpinned: T[] = [];
  for (const item of items) {
    if (isPinned(item)) pinned.push(item);
    else unpinned.push(item);
  }
  return [...pinned, ...unpinned];
}

function ExpansionGlyph({ expanded }: { expanded: boolean }) {
  const Icon = expanded ? ChevronDown : ChevronRight;
  return <Icon aria-hidden="true" size={11} style={s.databaseTreeChevron} />;
}

export function DatabaseSidebarTree({
  connections,
  dbxConnections,
  extraDbxConnectionGroups = [],
  activeConnectionId,
  activeDbxConnectionId,
  activeDbxConnection,
  activeDbxDatabase,
  activeDbxSchema,
  activeObject,
  activeDbxObject,
  userAdminActive,
  dbxHasSqlObjectBrowser,
  visibleDbxDatabases,
  dbxSchemas,
  legacyObjects,
  dbxObjects,
  dbxColumnsByTable,
  redisDatabasesByConnection,
  redisKeysByDatabase,
  redisScanStateByDatabase,
  mongoDatabasesByConnection,
  mongoCollectionsByDatabase,
  mongoDocumentsByCollection,
  mongoDocumentTotalsByCollection,
  activeMongoDocumentId,
  pinnedTreeNodeIds = EMPTY_PINNED_TREE_NODE_IDS,
  onSelectConnection,
  onSelectDbxConnection,
  onDeleteConnection,
  onDeleteDbxConnection,
  onSelectDatabase,
  onSelectDbxSchema,
  onSelectLegacyObject,
  onSelectDbxObject,
  onOpenUserAdmin,
  onOpenNoSqlWorkspace,
  onSelectRedisDatabase,
  onExpandRedisDatabase,
  onLoadMoreRedisKeys,
  onSelectRedisKey,
  onSelectMongoDatabase,
  onExpandMongoDatabase,
  onSelectMongoCollection,
  onExpandMongoCollection,
  onLoadMoreMongoDocuments,
  onSelectMongoDocument,
  onRenameConnection,
  onRenameDbxConnection,
  onRefreshConnection,
  onRefreshDbxConnection,
  onRefreshDatabase,
  onRefreshDbxSchema,
  onCopyNodeName,
  onDropDatabase,
  onDropDbxSchema,
  onDropDbxObject,
  onDropDbxColumn,
  onDropDbxTableChildObject,
  onConnectionContextMenu,
  onConnectionGroupContextMenu,
  onUserAdminContextMenu,
  onDbxDatabaseContextMenu,
  onDbxSchemaContextMenu,
  onDbxObjectContextMenu,
  onDbxColumnContextMenu,
  onDbxTableChildObjectContextMenu,
  onDbxObjectGroupContextMenu,
  onRedisDatabaseContextMenu,
  onRedisKeyContextMenu,
  onMongoDatabaseContextMenu,
  onMongoCollectionContextMenu,
  onMongoDocumentContextMenu,
}: DatabaseSidebarTreeProps) {
  const { t } = useI18n();
  const [expandedConnectionIds, setExpandedConnectionIds] = useState<Set<string>>(new Set());
  const [expandedDatabaseNames, setExpandedDatabaseNames] = useState<Set<string>>(new Set());
  const [expandedSchemaKeys, setExpandedSchemaKeys] = useState<Set<string>>(new Set());
  const [expandedObjectNodeKeys, setExpandedObjectNodeKeys] = useState<Set<string>>(new Set());
  const [collapsedObjectNodeKeys, setCollapsedObjectNodeKeys] = useState<Set<string>>(new Set());
  const [collapsedConnectionGroups, setCollapsedConnectionGroups] = useState<Set<string>>(
    new Set(),
  );
  const [collapsedObjectGroupKeys, setCollapsedObjectGroupKeys] = useState<Set<string>>(new Set());
  const [searchDraft, setSearchDraft] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchScope, setSearchScope] = useState<SearchScope>("all");
  const [selectedTreeNodeKeys, setSelectedTreeNodeKeys] = useState<Set<string>>(new Set());
  const [selectionAnchorKey, setSelectionAnchorKey] = useState<string | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => setSearchQuery(searchDraft.trim().toLowerCase()), 180);
    return () => window.clearTimeout(timer);
  }, [searchDraft]);

  useEffect(() => {
    setExpandedConnectionIds((current) => {
      const next = new Set(current);
      if (activeConnectionId) next.add(activeConnectionId);
      if (activeDbxConnectionId) next.add(activeDbxConnectionId);
      return next;
    });
  }, [activeConnectionId, activeDbxConnectionId]);

  useEffect(() => {
    if (!activeDbxDatabase) return;
    setExpandedDatabaseNames((current) => new Set(current).add(activeDbxDatabase));
  }, [activeDbxDatabase]);

  useEffect(() => {
    if (!activeDbxDatabase || !activeDbxSchema) return;
    setExpandedSchemaKeys((current) => {
      const next = new Set(current);
      next.add(`${activeDbxDatabase}:${activeDbxSchema}`);
      if (activeDbxConnection?.dbType === "mongodb" && activeDbxConnectionId) {
        next.add(`mongo:${activeDbxConnectionId}:${activeDbxDatabase}:${activeDbxSchema}`);
      }
      return next;
    });
  }, [activeDbxConnection?.dbType, activeDbxConnectionId, activeDbxDatabase, activeDbxSchema]);

  useEffect(() => {
    if (!activeDbxObject) return;
    const objectNodeKey = dbxObjectNodeKey(activeDbxObject);
    setExpandedObjectNodeKeys((current) => new Set(current).add(objectNodeKey));
    setCollapsedObjectNodeKeys((current) => {
      if (!current.has(objectNodeKey)) return current;
      const next = new Set(current);
      next.delete(objectNodeKey);
      return next;
    });
  }, [activeDbxObject]);

  const dbxRootObjects = useMemo(
    () => dbxObjects.filter((object) => !isDbxTableChildObject(object)),
    [dbxObjects],
  );
  const dbxTableChildObjects = useMemo(
    () => dbxObjects.filter(isDbxTableChildObject),
    [dbxObjects],
  );

  const dbxTableChildObjectsFor = useCallback(
    (object: DbxObjectInfo) =>
      dbxTableChildObjects.filter((childObject) =>
        dbxChildObjectBelongsToTable(childObject, object),
      ),
    [dbxTableChildObjects],
  );

  const dbxObjectsForSchema = useCallback(
    (schema: string) => dbxRootObjects.filter((object) => (object.schema ?? "") === schema),
    [dbxRootObjects],
  );

  const filteredLegacyConnections = useMemo(
    () =>
      connections.filter((connection) => {
        const matchesConnection =
          scopeAllows(searchScope, "connections") && matchesSearch(connection.name, searchQuery);
        const matchesActiveObject =
          connection.id === activeConnectionId &&
          scopeAllows(searchScope, "objects") &&
          legacyObjects.some((object) =>
            matchesSearch(`${object.objectType} ${object.name}`, searchQuery),
          );
        return matchesConnection || matchesActiveObject;
      }),
    [activeConnectionId, connections, legacyObjects, searchQuery, searchScope],
  );

  const filteredDbxConnections = useMemo(
    () =>
      dbxConnections.filter((connection) => {
        const matchesConnection =
          scopeAllows(searchScope, "connections") &&
          matchesSearch(`${connection.name} ${connection.dbType}`, searchQuery);
        const matchesActiveDatabase =
          connection.id === activeDbxConnectionId &&
          scopeAllows(searchScope, "databases") &&
          visibleDbxDatabases.some((database) => matchesSearch(database.name, searchQuery));
        const matchesActiveObject =
          connection.id === activeDbxConnectionId &&
          scopeAllows(searchScope, "objects") &&
          (connection.dbType === "redis"
            ? (redisDatabasesByConnection[connection.id] ?? []).some(
                (database) =>
                  matchesSearch(`redis db${database.db} ${database.keys}`, searchQuery) ||
                  (redisKeysByDatabase[`${connection.id}:${database.db}`] ?? []).some((key) =>
                    matchesSearch(
                      `redis key db${database.db} ${key.key_display} ${key.key_raw} ${key.key_type}`,
                      searchQuery,
                    ),
                  ),
              )
            : connection.dbType === "mongodb"
              ? (mongoDatabasesByConnection[connection.id] ?? []).some((database) => {
                  const matchesDatabase = matchesSearch(
                    `mongodb database ${database}`,
                    searchQuery,
                  );
                  const matchesCollection = (
                    mongoCollectionsByDatabase[`${connection.id}:${database}`] ?? []
                  ).some(
                    (collection) =>
                      matchesSearch(`mongodb collection ${database} ${collection}`, searchQuery) ||
                      (
                        mongoDocumentsByCollection[`${connection.id}:${database}:${collection}`] ??
                        []
                      ).some((document, index) =>
                        matchesSearch(
                          `mongodb document ${database} ${collection} ${mongoDocumentPreview(document, index)}`,
                          searchQuery,
                        ),
                      ),
                  );
                  return matchesDatabase || matchesCollection;
                })
              : (supportsDbxUserAdmin(connection.dbType) &&
                  matchesSearch(`${t("database.userAdmin")} users privileges`, searchQuery)) ||
                dbxSchemas.some((schema) => matchesSearch(`schema ${schema}`, searchQuery)) ||
                dbxRootObjects.some((object) => {
                  const matchesObject = matchesSearch(
                    `${object.object_type} ${dbxObjectLabel(object)}`,
                    searchQuery,
                  );
                  const matchesColumn = (dbxColumnsByTable[dbxObjectKey(object)] ?? []).some(
                    (column) =>
                      matchesSearch(
                        `column ${dbxObjectLabel(object)} ${column.name} ${column.data_type}`,
                        searchQuery,
                      ),
                  );
                  const matchesChildObject = dbxTableChildObjectsFor(object).some((childObject) =>
                    matchesSearch(dbxChildObjectSearchText(childObject, object), searchQuery),
                  );
                  return matchesObject || matchesColumn || matchesChildObject;
                }));
        return matchesConnection || matchesActiveDatabase || matchesActiveObject;
      }),
    [
      activeDbxConnectionId,
      dbxColumnsByTable,
      dbxConnections,
      dbxRootObjects,
      dbxSchemas,
      dbxTableChildObjectsFor,
      mongoCollectionsByDatabase,
      mongoDatabasesByConnection,
      mongoDocumentsByCollection,
      redisDatabasesByConnection,
      redisKeysByDatabase,
      searchQuery,
      searchScope,
      t,
      visibleDbxDatabases,
    ],
  );

  const filteredDbxConnectionGroups = useMemo(() => {
    const groups = new Map<string, AeroricDbConnectionConfig[]>();
    const ungrouped: AeroricDbConnectionConfig[] = [];
    const extraGroupNames = Array.from(
      new Set(extraDbxConnectionGroups.map((group) => group.trim()).filter(Boolean)),
    );
    for (const connection of filteredDbxConnections) {
      const groupName = connectionGroupName(connection);
      if (!groupName) {
        ungrouped.push(connection);
        continue;
      }
      groups.set(groupName, [...(groups.get(groupName) ?? []), connection]);
    }
    for (const groupName of extraGroupNames) {
      if (searchQuery && !matchesSearch(groupName, searchQuery)) continue;
      if (!scopeAllows(searchScope, "connections")) continue;
      groups.set(groupName, groups.get(groupName) ?? []);
    }
    return [
      ...Array.from(groups.entries())
        .sort(([left], [right]) => databaseObjectNameCollator.compare(left, right))
        .map(([groupName, items]) => ({
          key: `group:${groupName}`,
          groupName,
          label: groupName,
          connections: items.sort(sortDbxConnections),
        })),
      {
        key: "group:ungrouped",
        groupName: "",
        label: "",
        connections: ungrouped.sort(sortDbxConnections),
      },
    ].filter((group) => Boolean(group.groupName) || group.connections.length > 0);
  }, [extraDbxConnectionGroups, filteredDbxConnections, searchQuery, searchScope]);

  const filteredDatabases = useMemo(
    () =>
      orderPinnedFirst(
        visibleDbxDatabases.filter((database) => {
          const matchesDatabase =
            scopeAllows(searchScope, "databases") && matchesSearch(database.name, searchQuery);
          const matchesActiveObject =
            database.name === activeDbxDatabase &&
            scopeAllows(searchScope, "objects") &&
            (dbxSchemas.some((schema) => matchesSearch(`schema ${schema}`, searchQuery)) ||
              dbxRootObjects.some((object) => {
                const matchesObject = matchesSearch(
                  `${object.object_type} ${dbxObjectLabel(object)}`,
                  searchQuery,
                );
                const matchesColumn = (dbxColumnsByTable[dbxObjectKey(object)] ?? []).some(
                  (column) =>
                    matchesSearch(
                      `column ${dbxObjectLabel(object)} ${column.name} ${column.data_type}`,
                      searchQuery,
                    ),
                );
                const matchesChildObject = dbxTableChildObjectsFor(object).some((childObject) =>
                  matchesSearch(dbxChildObjectSearchText(childObject, object), searchQuery),
                );
                return matchesObject || matchesColumn || matchesChildObject;
              }));
          return matchesDatabase || matchesActiveObject;
        }),
        (database) => pinnedTreeNodeIds.has(dbxDatabaseNodeKey(database.name)),
      ),
    [
      activeDbxDatabase,
      dbxColumnsByTable,
      dbxRootObjects,
      dbxSchemas,
      dbxTableChildObjectsFor,
      pinnedTreeNodeIds,
      searchQuery,
      searchScope,
      visibleDbxDatabases,
    ],
  );

  const filteredDbxSchemas = useMemo(
    () =>
      scopeAllows(searchScope, "objects")
        ? orderPinnedFirst(
            dbxSchemas.filter((schema) => {
              const matchesSchema = matchesSearch(`schema ${schema}`, searchQuery);
              const matchesObject = dbxObjectsForSchema(schema).some((object) => {
                const matchesObjectName = matchesSearch(
                  `${object.object_type} ${dbxObjectLabel(object)}`,
                  searchQuery,
                );
                const matchesColumn = (dbxColumnsByTable[dbxObjectKey(object)] ?? []).some(
                  (column) =>
                    matchesSearch(
                      `column ${dbxObjectLabel(object)} ${column.name} ${column.data_type}`,
                      searchQuery,
                    ),
                );
                const matchesChildObject = dbxTableChildObjectsFor(object).some((childObject) =>
                  matchesSearch(dbxChildObjectSearchText(childObject, object), searchQuery),
                );
                return matchesObjectName || matchesColumn || matchesChildObject;
              });
              return matchesSchema || matchesObject;
            }),
            (schema) =>
              Boolean(
                activeDbxDatabase &&
                pinnedTreeNodeIds.has(dbxSchemaNodeKey(activeDbxDatabase, schema)),
              ),
          )
        : [],
    [
      activeDbxDatabase,
      dbxColumnsByTable,
      dbxObjectsForSchema,
      dbxSchemas,
      dbxTableChildObjectsFor,
      pinnedTreeNodeIds,
      searchQuery,
      searchScope,
    ],
  );

  const filteredLegacyObjects = useMemo(
    () =>
      scopeAllows(searchScope, "objects")
        ? legacyObjects.filter((object) =>
            matchesSearch(`${object.objectType} ${object.name}`, searchQuery),
          )
        : [],
    [legacyObjects, searchQuery, searchScope],
  );

  const filteredDbxObjects = useMemo(
    () =>
      scopeAllows(searchScope, "objects")
        ? dbxRootObjects.filter((object) => {
            const matchesObject = matchesSearch(
              `${object.object_type} ${dbxObjectLabel(object)}`,
              searchQuery,
            );
            const matchesColumn = (dbxColumnsByTable[dbxObjectKey(object)] ?? []).some((column) =>
              matchesSearch(
                `column ${dbxObjectLabel(object)} ${column.name} ${column.data_type}`,
                searchQuery,
              ),
            );
            const matchesChildObject = dbxTableChildObjectsFor(object).some((childObject) =>
              matchesSearch(dbxChildObjectSearchText(childObject, object), searchQuery),
            );
            return matchesObject || matchesColumn || matchesChildObject;
          })
        : [],
    [dbxColumnsByTable, dbxRootObjects, dbxTableChildObjectsFor, searchQuery, searchScope],
  );

  const orderDbxObjectsForTree = useCallback(
    (objects: DbxObjectInfo[]) =>
      orderPinnedFirst(objects, (object) => pinnedTreeNodeIds.has(dbxObjectNodeKey(object))),
    [pinnedTreeNodeIds],
  );

  const visibleDbxObjectNodeKeys = useCallback(
    (objects: DbxObjectInfo[], scopeKey: string) => {
      const keys: string[] = [];
      for (const group of groupDbxObjects(objects)) {
        const groupStateKey = `${scopeKey}:${group.key}`;
        keys.push(dbxObjectGroupNodeKey(scopeKey, group.key));
        const expanded = Boolean(searchQuery) || !collapsedObjectGroupKeys.has(groupStateKey);
        if (!expanded) continue;
        for (const object of orderDbxObjectsForTree(group.objects)) {
          keys.push(dbxObjectNodeKey(object));
          const objectColumns = dbxColumnsByTable[dbxObjectKey(object)] ?? [];
          const objectChildObjects = dbxTableChildObjectsFor(object);
          const visibleColumns = searchQuery
            ? objectColumns.filter((column) =>
                matchesSearch(
                  `column ${dbxObjectLabel(object)} ${column.name} ${column.data_type}`,
                  searchQuery,
                ),
              )
            : objectColumns;
          const visibleChildObjects = searchQuery
            ? objectChildObjects.filter((childObject) =>
                matchesSearch(dbxChildObjectSearchText(childObject, object), searchQuery),
              )
            : objectChildObjects;
          const isActiveObject =
            object.name === activeDbxObject?.name && object.schema === activeDbxObject?.schema;
          const objectNodeKey = dbxObjectNodeKey(object);
          const objectExpanded =
            Boolean(searchQuery) ||
            (!collapsedObjectNodeKeys.has(objectNodeKey) &&
              (isActiveObject || expandedObjectNodeKeys.has(objectNodeKey)));
          if (!objectExpanded) continue;
          for (const column of visibleColumns) keys.push(dbxColumnNodeKey(object, column));
          for (const childObject of visibleChildObjects)
            keys.push(dbxTableChildObjectNodeKey(object, childObject));
        }
      }
      return keys;
    },
    [
      activeDbxObject,
      collapsedObjectGroupKeys,
      collapsedObjectNodeKeys,
      dbxColumnsByTable,
      dbxTableChildObjectsFor,
      expandedObjectNodeKeys,
      orderDbxObjectsForTree,
      searchQuery,
    ],
  );

  const visibleTreeNodeKeys = useMemo(() => {
    const keys: string[] = [];
    for (const connection of filteredLegacyConnections) {
      keys.push(legacyConnectionNodeKey(connection));
      const expanded = expandedConnectionIds.has(connection.id) || Boolean(searchQuery);
      if (expanded && connection.id === activeConnectionId) {
        for (const object of filteredLegacyObjects) keys.push(legacyObjectNodeKey(object));
      }
    }

    for (const connectionGroup of filteredDbxConnectionGroups) {
      const groupCollapsed =
        Boolean(connectionGroup.groupName) &&
        collapsedConnectionGroups.has(connectionGroup.groupName) &&
        !searchQuery;
      if (connectionGroup.groupName)
        keys.push(dbxConnectionGroupNodeKey(connectionGroup.groupName));
      if (groupCollapsed) continue;
      for (const connection of connectionGroup.connections) {
        keys.push(dbxConnectionNodeKey(connection));
        const expanded = expandedConnectionIds.has(connection.id) || Boolean(searchQuery);
        const isActive = connection.id === activeDbxConnectionId;
        if (!expanded || !isActive || !activeDbxConnection) continue;
        if (!dbxHasSqlObjectBrowser) {
          if (connection.dbType === "redis") {
            for (const database of redisDatabasesByConnection[connection.id] ?? []) {
              keys.push(redisDatabaseNodeKey(connection.id, database.db));
              const databaseExpanded =
                expandedDatabaseNames.has(`redis:${connection.id}:${database.db}`) ||
                Boolean(searchQuery);
              if (!databaseExpanded) continue;
              for (const key of redisKeysByDatabase[`${connection.id}:${database.db}`] ?? []) {
                keys.push(redisKeyNodeKey(connection.id, database.db, key.key_raw));
              }
            }
          } else if (connection.dbType === "mongodb") {
            for (const database of mongoDatabasesByConnection[connection.id] ?? []) {
              keys.push(mongoDatabaseNodeKey(connection.id, database));
              const databaseExpanded = expandedDatabaseNames.has(database) || Boolean(searchQuery);
              if (!databaseExpanded) continue;
              for (const collection of mongoCollectionsByDatabase[`${connection.id}:${database}`] ??
                []) {
                keys.push(mongoCollectionNodeKey(connection.id, database, collection));
                const collectionExpansionKey = `mongo:${connection.id}:${database}:${collection}`;
                const collectionExpanded =
                  expandedSchemaKeys.has(collectionExpansionKey) || Boolean(searchQuery);
                if (!collectionExpanded) continue;
                const collectionKey = `${connection.id}:${database}:${collection}`;
                const loadedDocuments = mongoDocumentsByCollection[collectionKey] ?? [];
                for (const [index, document] of loadedDocuments.entries()) {
                  keys.push(
                    mongoDocumentNodeKey(connection.id, database, collection, document, index),
                  );
                }
                if (
                  loadedDocuments.length < (mongoDocumentTotalsByCollection[collectionKey] ?? 0)
                ) {
                  keys.push(mongoDocumentLoadMoreNodeKey(connection.id, database, collection));
                }
              }
            }
          }
          if (supportsDbxUserAdmin(connection.dbType))
            keys.push(dbxUserAdminNodeKey(connection.id));
          continue;
        }
        for (const database of filteredDatabases) {
          keys.push(dbxDatabaseNodeKey(database.name));
          const databaseExpanded = expandedDatabaseNames.has(database.name) || Boolean(searchQuery);
          if (!databaseExpanded || database.name !== activeDbxDatabase) continue;
          if (
            dbxSchemas.length === 1 &&
            filteredDbxSchemas.length === 1 &&
            filteredDbxSchemas[0] === database.name
          ) {
            keys.push(
              ...visibleDbxObjectNodeKeys(
                filteredDbxObjects.filter((object) => (object.schema ?? "") === database.name),
                database.name,
              ),
            );
          } else if (dbxSchemas.length > 0) {
            for (const schemaName of filteredDbxSchemas) {
              keys.push(dbxSchemaNodeKey(database.name, schemaName));
              const schemaKey = `${database.name}:${schemaName}`;
              const schemaExpanded =
                expandedSchemaKeys.has(schemaKey) ||
                Boolean(searchQuery) ||
                schemaName === activeDbxSchema ||
                filteredDbxSchemas.length === 1;
              if (!schemaExpanded) continue;
              keys.push(
                ...visibleDbxObjectNodeKeys(
                  filteredDbxObjects.filter((object) => (object.schema ?? "") === schemaName),
                  schemaKey,
                ),
              );
            }
          } else {
            keys.push(...visibleDbxObjectNodeKeys(filteredDbxObjects, database.name));
          }
        }
        if (supportsDbxUserAdmin(connection.dbType)) keys.push(dbxUserAdminNodeKey(connection.id));
      }
    }
    return keys;
  }, [
    activeConnectionId,
    activeDbxConnection,
    activeDbxConnectionId,
    activeDbxDatabase,
    activeDbxSchema,
    collapsedConnectionGroups,
    dbxHasSqlObjectBrowser,
    dbxSchemas.length,
    expandedConnectionIds,
    expandedDatabaseNames,
    expandedSchemaKeys,
    filteredDatabases,
    filteredDbxConnectionGroups,
    filteredDbxObjects,
    filteredDbxSchemas,
    filteredLegacyConnections,
    filteredLegacyObjects,
    mongoCollectionsByDatabase,
    mongoDatabasesByConnection,
    mongoDocumentsByCollection,
    mongoDocumentTotalsByCollection,
    redisDatabasesByConnection,
    redisKeysByDatabase,
    searchQuery,
    visibleDbxObjectNodeKeys,
  ]);

  useEffect(() => {
    setSelectedTreeNodeKeys((current) => {
      const visibleKeys = new Set(visibleTreeNodeKeys);
      const next = new Set([...current].filter((key) => visibleKeys.has(key)));
      return next.size === current.size ? current : next;
    });
    if (selectionAnchorKey && !visibleTreeNodeKeys.includes(selectionAnchorKey))
      setSelectionAnchorKey(null);
  }, [selectionAnchorKey, visibleTreeNodeKeys]);

  const hasResults =
    filteredLegacyConnections.length > 0 ||
    filteredDbxConnections.length > 0 ||
    filteredDatabases.length > 0 ||
    filteredDbxSchemas.length > 0 ||
    filteredLegacyObjects.length > 0 ||
    filteredDbxObjects.length > 0;

  const toggleConnection = (connectionId: string) => {
    setExpandedConnectionIds((current) => {
      const next = new Set(current);
      if (next.has(connectionId)) next.delete(connectionId);
      else next.add(connectionId);
      return next;
    });
  };

  const toggleConnectionGroup = (groupName: string) => {
    setCollapsedConnectionGroups((current) => {
      const next = new Set(current);
      if (next.has(groupName)) next.delete(groupName);
      else next.add(groupName);
      return next;
    });
  };

  const toggleDatabase = (database: string) => {
    setExpandedDatabaseNames((current) => {
      const next = new Set(current);
      if (next.has(database)) next.delete(database);
      else next.add(database);
      return next;
    });
  };

  const toggleSchema = (database: string, schema: string) => {
    const key = `${database}:${schema}`;
    setExpandedSchemaKeys((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const toggleObjectGroup = (scopeKey: string, groupKey: DbxObjectGroupKey) => {
    const key = `${scopeKey}:${groupKey}`;
    setCollapsedObjectGroupKeys((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const toggleObjectNode = (objectNodeKey: string, currentlyExpanded: boolean) => {
    setExpandedObjectNodeKeys((current) => {
      const next = new Set(current);
      if (currentlyExpanded) next.delete(objectNodeKey);
      else next.add(objectNodeKey);
      return next;
    });
    setCollapsedObjectNodeKeys((current) => {
      const next = new Set(current);
      if (currentlyExpanded) next.add(objectNodeKey);
      else next.delete(objectNodeKey);
      return next;
    });
  };

  const scopeButton = (scope: SearchScope, label: string) => (
    <DbxButton
      variant={searchScope === scope ? "default" : "outline"}
      size="xs"
      onClick={() => setSearchScope(scope)}
    >
      {label}
    </DbxButton>
  );

  const treeNodeClassName = "database-tree-node";

  const treeNodeSelectionStyle = (nodeKey: string, active = false) => {
    const selected = selectedTreeNodeKeys.has(nodeKey);
    return {
      ...(active || selected ? s.databaseListButtonActive : {}),
      ...(selected && !active ? { boxShadow: "inset 2px 0 0 var(--accent)" } : {}),
    };
  };

  const treeNodeSelectionAttrs = (nodeKey: string, active = false) => ({
    "aria-selected": selectedTreeNodeKeys.has(nodeKey) || active,
    "data-selected": selectedTreeNodeKeys.has(nodeKey) || active ? "true" : undefined,
  });

  const handleTreeNodeClick = (
    event: MouseEvent<HTMLButtonElement>,
    nodeKey: string,
    activate: () => void,
  ) => {
    const rangeSelection = event.shiftKey && selectionAnchorKey;
    if (rangeSelection) {
      const anchorIndex = visibleTreeNodeKeys.indexOf(selectionAnchorKey);
      const targetIndex = visibleTreeNodeKeys.indexOf(nodeKey);
      if (anchorIndex >= 0 && targetIndex >= 0) {
        const [start, end] =
          anchorIndex < targetIndex ? [anchorIndex, targetIndex] : [targetIndex, anchorIndex];
        setSelectedTreeNodeKeys(new Set(visibleTreeNodeKeys.slice(start, end + 1)));
        setSelectionAnchorKey(nodeKey);
        return;
      }
    }

    if (event.metaKey || event.ctrlKey) {
      setSelectedTreeNodeKeys((current) => {
        const next = new Set(current);
        if (next.has(nodeKey)) next.delete(nodeKey);
        else next.add(nodeKey);
        return next;
      });
      setSelectionAnchorKey(nodeKey);
      return;
    }

    setSelectedTreeNodeKeys(new Set([nodeKey]));
    setSelectionAnchorKey(nodeKey);
    activate();
  };

  const handleKeyShortcut = (
    event: KeyboardEvent,
    actions: {
      copyName: string;
      refresh?: () => void;
      delete?: () => void;
      rename?: () => void;
    },
  ) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "c") {
      event.preventDefault();
      onCopyNodeName(actions.copyName);
      return;
    }
    if (event.key === "F5") {
      event.preventDefault();
      actions.refresh?.();
      return;
    }
    if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      actions.delete?.();
      return;
    }
    if (event.key === "F2") {
      event.preventDefault();
      actions.rename?.();
    }
  };

  const tableChildObjectIcon = (childObject: DbxObjectInfo) => {
    const childType = dbxTableChildObjectType(childObject);
    if (childType === "INDEX") return <ListTree size={12} />;
    if (childType === "FOREIGN_KEY") return <KeyRound size={12} />;
    if (childType === "TRIGGER") return <Zap size={12} />;
    return <Hash size={12} />;
  };

  const objectGroupIcon = (groupKey: DbxObjectGroupKey) => {
    if (groupKey === "tables") return <Table2 size={13} />;
    if (groupKey === "views") return <Eye size={13} />;
    if (groupKey === "procedures") return <ScrollText size={13} />;
    if (groupKey === "functions") return <Braces size={13} />;
    if (groupKey === "packages") return <Package size={13} />;
    return <ListTree size={13} />;
  };

  const handleDbxObjectDragStart = (event: DragEvent<HTMLButtonElement>, object: DbxObjectInfo) => {
    const objectName = dbxObjectLabel(object);
    event.dataTransfer.effectAllowed = "copy";
    event.dataTransfer.setData("text/plain", objectName);
    event.dataTransfer.setData(
      "application/x-aeroric-database-object",
      JSON.stringify({
        name: object.name,
        schema: object.schema ?? null,
        objectType: normalizeDbxObjectType(object.object_type),
        reference: objectName,
      }),
    );
  };

  const pinnedNodeIcon = (nodeKey: string) =>
    pinnedTreeNodeIds.has(nodeKey) ? (
      <span
        aria-label={t("database.pinned")}
        role="img"
        style={{
          display: "inline-flex",
          alignItems: "center",
          color: "var(--accent)",
          flexShrink: 0,
        }}
      >
        <Pin aria-hidden="true" size={12} fill="currentColor" />
      </span>
    ) : null;

  const renderDbxObjectRows = (
    objects: DbxObjectInfo[],
    objectPaddingLeft = 42,
    childPaddingLeft = 56,
  ) => {
    if (!activeDbxConnection || !activeDbxDatabase || objects.length === 0) return null;
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 3 }}>
        {orderDbxObjectsForTree(objects).map((object) => {
          const objectNodeKey = dbxObjectNodeKey(object);
          const isActiveObject =
            object.name === activeDbxObject?.name && object.schema === activeDbxObject?.schema;
          const objectColumns = dbxColumnsByTable[dbxObjectKey(object)] ?? [];
          const objectChildObjects = dbxTableChildObjectsFor(object);
          const visibleColumns = searchQuery
            ? objectColumns.filter((column) =>
                matchesSearch(
                  `column ${dbxObjectLabel(object)} ${column.name} ${column.data_type}`,
                  searchQuery,
                ),
              )
            : objectColumns;
          const visibleChildObjects = searchQuery
            ? objectChildObjects.filter((childObject) =>
                matchesSearch(dbxChildObjectSearchText(childObject, object), searchQuery),
              )
            : objectChildObjects;
          const draggableObjectReference = canDragDbxObjectReference(object);
          const hasChildRows = visibleColumns.length > 0 || visibleChildObjects.length > 0;
          const objectExpanded =
            Boolean(searchQuery) ||
            (!collapsedObjectNodeKeys.has(objectNodeKey) &&
              (isActiveObject || expandedObjectNodeKeys.has(objectNodeKey)));
          const showChildRows = objectExpanded && hasChildRows;
          return (
            <div key={`${object.object_type}:${object.schema ?? ""}:${object.name}`}>
              <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
                {hasChildRows && (
                  <button
                    type="button"
                    aria-label={t(
                      objectExpanded ? "database.collapseTableNode" : "database.expandTableNode",
                      {
                        name: dbxObjectDisplayName(object),
                      },
                    )}
                    title={t(
                      objectExpanded ? "database.collapseTableNode" : "database.expandTableNode",
                      {
                        name: dbxObjectDisplayName(object),
                      },
                    )}
                    onClick={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      toggleObjectNode(objectNodeKey, objectExpanded);
                    }}
                    style={{
                      position: "absolute",
                      left: Math.max(4, objectPaddingLeft - 20),
                      zIndex: 1,
                      width: 18,
                      height: 18,
                      padding: 0,
                      border: "1px solid transparent",
                      borderRadius: 5,
                      background: "transparent",
                      color: "var(--text-hint)",
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      cursor: "pointer",
                    }}
                  >
                    <ExpansionGlyph expanded={objectExpanded} />
                  </button>
                )}
                <button
                  type="button"
                  className={treeNodeClassName}
                  aria-label={`${dbxObjectDisplayName(object)} ${object.object_type}`}
                  style={{
                    ...s.databaseListButton,
                    minHeight: 28,
                    padding: `5px 8px 5px ${objectPaddingLeft}px`,
                    ...treeNodeSelectionStyle(objectNodeKey, isActiveObject),
                  }}
                  {...treeNodeSelectionAttrs(objectNodeKey)}
                  draggable={draggableObjectReference}
                  onDragStart={
                    draggableObjectReference
                      ? (event) => handleDbxObjectDragStart(event, object)
                      : undefined
                  }
                  onClick={(event) =>
                    handleTreeNodeClick(event, objectNodeKey, () => onSelectDbxObject(object))
                  }
                  onKeyDown={(event) =>
                    handleKeyShortcut(event, {
                      copyName: dbxObjectLabel(object),
                      refresh: () => onSelectDbxObject(object),
                      delete: () => onDropDbxObject(activeDbxConnection, activeDbxDatabase, object),
                    })
                  }
                  onContextMenu={(event) =>
                    onDbxObjectContextMenu(event, activeDbxConnection.id, activeDbxDatabase, object)
                  }
                >
                  <Table2 size={13} />
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {dbxObjectDisplayName(object)}
                  </span>
                  {pinnedNodeIcon(objectNodeKey)}
                  <span
                    style={{ color: "var(--text-hint)", fontSize: 10, textTransform: "uppercase" }}
                  >
                    {object.object_type}
                  </span>
                </button>
              </div>
              {showChildRows && (
                <div style={{ display: "flex", flexDirection: "column", gap: 2, marginTop: 2 }}>
                  {visibleColumns.map((column) => {
                    const columnNodeKey = dbxColumnNodeKey(object, column);
                    return (
                      <button
                        key={`${dbxObjectKey(object)}:${column.name}`}
                        type="button"
                        className={treeNodeClassName}
                        style={{
                          ...s.databaseListButton,
                          minHeight: 25,
                          padding: `4px 8px 4px ${childPaddingLeft}px`,
                          color: "var(--text-muted)",
                          ...treeNodeSelectionStyle(columnNodeKey),
                        }}
                        {...treeNodeSelectionAttrs(columnNodeKey)}
                        onClick={(event) =>
                          handleTreeNodeClick(event, columnNodeKey, () => onSelectDbxObject(object))
                        }
                        onKeyDown={(event) =>
                          handleKeyShortcut(event, {
                            copyName: column.name,
                            refresh: () => onSelectDbxObject(object),
                            delete: () =>
                              onDropDbxColumn(
                                activeDbxConnection,
                                activeDbxDatabase,
                                object,
                                column,
                              ),
                          })
                        }
                        onContextMenu={(event) =>
                          onDbxColumnContextMenu(
                            event,
                            activeDbxConnection.id,
                            activeDbxDatabase,
                            object,
                            column,
                          )
                        }
                      >
                        <span
                          style={{
                            color: "var(--text-hint)",
                            fontFamily: "var(--font-mono)",
                            fontSize: 12,
                          }}
                        >
                          #
                        </span>
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {column.name}
                        </span>
                        <span
                          style={{
                            color: "var(--text-hint)",
                            fontSize: 10,
                            textTransform: "uppercase",
                          }}
                        >
                          {column.data_type}
                        </span>
                      </button>
                    );
                  })}
                  {visibleChildObjects.map((childObject) => {
                    const childType = dbxTableChildObjectType(childObject);
                    const childObjectNodeKey = dbxTableChildObjectNodeKey(object, childObject);
                    return (
                      <button
                        key={`${dbxObjectKey(object)}:${childObject.object_type}:${childObject.name}`}
                        type="button"
                        className={treeNodeClassName}
                        style={{
                          ...s.databaseListButton,
                          minHeight: 25,
                          padding: `4px 8px 4px ${childPaddingLeft}px`,
                          color: "var(--text-muted)",
                          ...treeNodeSelectionStyle(childObjectNodeKey),
                        }}
                        {...treeNodeSelectionAttrs(childObjectNodeKey)}
                        onClick={(event) =>
                          handleTreeNodeClick(event, childObjectNodeKey, () =>
                            onSelectDbxObject(object),
                          )
                        }
                        onKeyDown={(event) =>
                          handleKeyShortcut(event, {
                            copyName: childObject.name,
                            refresh: () => onSelectDbxObject(object),
                            delete: () =>
                              onDropDbxTableChildObject(
                                activeDbxConnection,
                                activeDbxDatabase,
                                object,
                                childObject,
                              ),
                          })
                        }
                        onContextMenu={(event) =>
                          onDbxTableChildObjectContextMenu(
                            event,
                            activeDbxConnection.id,
                            activeDbxDatabase,
                            object,
                            childObject,
                          )
                        }
                      >
                        {tableChildObjectIcon(childObject)}
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {childObject.name}
                        </span>
                        {childType && (
                          <span
                            style={{
                              color: "var(--text-hint)",
                              fontSize: 10,
                              textTransform: "uppercase",
                            }}
                          >
                            {childType.replace("_", " ")}
                          </span>
                        )}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
      </div>
    );
  };

  const renderDbxObjectGroups = (
    objects: DbxObjectInfo[],
    scopeKey: string,
    groupPaddingLeft = 36,
    objectPaddingLeft = 42,
    childPaddingLeft = 56,
    schemaForScope?: string,
  ) => {
    if (!activeDbxConnection || !activeDbxDatabase || objects.length === 0) return null;
    const groups = groupDbxObjects(objects);
    if (groups.length === 0) return null;
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 3 }}>
        {groups.map((group) => {
          const groupStateKey = `${scopeKey}:${group.key}`;
          const groupNodeKey = dbxObjectGroupNodeKey(scopeKey, group.key);
          const expanded = Boolean(searchQuery) || !collapsedObjectGroupKeys.has(groupStateKey);
          const label = t(group.labelKey);
          return (
            <div key={groupStateKey}>
              <button
                type="button"
                className={treeNodeClassName}
                aria-label={label}
                style={{
                  ...s.databaseListButton,
                  minHeight: 27,
                  padding: `5px 8px 5px ${groupPaddingLeft}px`,
                  color: "var(--text-muted)",
                  ...treeNodeSelectionStyle(groupNodeKey),
                }}
                {...treeNodeSelectionAttrs(groupNodeKey)}
                onClick={(event) =>
                  handleTreeNodeClick(event, groupNodeKey, () =>
                    toggleObjectGroup(scopeKey, group.key),
                  )
                }
                onKeyDown={(event) =>
                  handleKeyShortcut(event, {
                    copyName: label,
                    refresh: () =>
                      schemaForScope
                        ? onRefreshDbxSchema(activeDbxConnection, activeDbxDatabase, schemaForScope)
                        : onRefreshDatabase(activeDbxConnection, activeDbxDatabase),
                  })
                }
                onContextMenu={(event) =>
                  onDbxObjectGroupContextMenu(
                    event,
                    activeDbxConnection.id,
                    activeDbxDatabase,
                    schemaForScope ?? null,
                    group.key,
                    label,
                  )
                }
              >
                <ExpansionGlyph expanded={expanded} />
                {objectGroupIcon(group.key)}
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {label}
                </span>
                <span aria-hidden="true" style={{ color: "var(--text-hint)", fontSize: 10 }}>
                  {group.objects.length}
                </span>
              </button>
              {expanded && renderDbxObjectRows(group.objects, objectPaddingLeft, childPaddingLeft)}
            </div>
          );
        })}
      </div>
    );
  };

  const renderNoSqlTreeRows = (connection: AeroricDbConnectionConfig) => {
    if (connection.dbType === "redis") {
      const redisDatabases = orderPinnedFirst(
        (redisDatabasesByConnection[connection.id] ?? []).filter((database) =>
          matchesSearch(`redis db${database.db} ${database.keys}`, searchQuery),
        ),
        (database) => pinnedTreeNodeIds.has(redisDatabaseNodeKey(connection.id, database.db)),
      );
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 3 }}>
          {redisDatabases.map((database) => {
            const nodeKey = redisDatabaseNodeKey(connection.id, database.db);
            const databaseExpansionKey = `redis:${connection.id}:${database.db}`;
            const databaseExpanded =
              expandedDatabaseNames.has(databaseExpansionKey) || Boolean(searchQuery);
            const active = activeDbxDatabase === `db${database.db}`;
            const toggleRedisDatabase = (loadOnExpand = true) => {
              toggleDatabase(databaseExpansionKey);
              if (loadOnExpand && !databaseExpanded) onExpandRedisDatabase(connection, database.db);
            };
            const databaseKey = `${connection.id}:${database.db}`;
            const loadedKeys = redisKeysByDatabase[databaseKey] ?? [];
            const keys = loadedKeys.filter((key) =>
              matchesSearch(
                `redis key db${database.db} ${key.key_display} ${key.key_raw} ${key.key_type}`,
                searchQuery,
              ),
            );
            const scanState = redisScanStateByDatabase[databaseKey];
            const canLoadMoreKeys =
              !searchQuery && loadedKeys.length > 0 && (scanState?.cursor ?? 0) !== 0;
            const redisTotalKeys = scanState?.totalKeys ?? database.keys;
            return (
              <div key={database.db}>
                <button
                  type="button"
                  className={treeNodeClassName}
                  style={{
                    ...s.databaseListButton,
                    minHeight: 28,
                    padding: "5px 8px 5px 26px",
                    ...treeNodeSelectionStyle(nodeKey, active),
                  }}
                  {...treeNodeSelectionAttrs(nodeKey, active)}
                  onClick={(event) =>
                    handleTreeNodeClick(event, nodeKey, () => {
                      toggleRedisDatabase(false);
                      onSelectRedisDatabase(connection, database.db);
                    })
                  }
                  onContextMenu={(event) =>
                    onRedisDatabaseContextMenu(event, connection.id, database.db)
                  }
                  onKeyDown={(event) =>
                    handleKeyShortcut(event, {
                      copyName: `db${database.db}`,
                      refresh: () => onExpandRedisDatabase(connection, database.db),
                    })
                  }
                >
                  <span
                    onClick={(event) => {
                      event.stopPropagation();
                      toggleRedisDatabase();
                    }}
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      color: "var(--text-hint)",
                    }}
                  >
                    <ExpansionGlyph expanded={databaseExpanded} />
                  </span>
                  <Database size={13} />
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    db{database.db}
                  </span>
                  {getDefaultDatabase(connection) === String(database.db) && (
                    <span
                      style={{
                        fontSize: 10,
                        padding: "1px 5px",
                        borderRadius: 4,
                        background: "var(--accent-dim)",
                        color: "var(--accent)",
                        flexShrink: 0,
                      }}
                    >
                      {t("database.defaultDatabaseBadge")}
                    </span>
                  )}
                  {pinnedNodeIcon(nodeKey)}
                  <span style={{ color: "var(--text-hint)", fontSize: 11 }}>{database.keys}</span>
                </button>
                {databaseExpanded && keys.length > 0 && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 2, marginTop: 2 }}>
                    {keys.map((key) => {
                      const keyNodeKey = redisKeyNodeKey(connection.id, database.db, key.key_raw);
                      const keyActive =
                        activeDbxDatabase === `db${database.db}` && activeDbxSchema === key.key_raw;
                      return (
                        <button
                          key={key.key_raw}
                          type="button"
                          className={treeNodeClassName}
                          style={{
                            ...s.databaseListButton,
                            minHeight: 25,
                            padding: "4px 8px 4px 54px",
                            color: "var(--text-muted)",
                            ...treeNodeSelectionStyle(keyNodeKey, keyActive),
                          }}
                          {...treeNodeSelectionAttrs(keyNodeKey, keyActive)}
                          onClick={(event) =>
                            handleTreeNodeClick(event, keyNodeKey, () =>
                              onSelectRedisKey(connection, database.db, key.key_raw),
                            )
                          }
                          onContextMenu={(event) =>
                            onRedisKeyContextMenu(event, connection.id, database.db, key.key_raw)
                          }
                          onKeyDown={(event) =>
                            handleKeyShortcut(event, {
                              copyName: key.key_raw,
                              refresh: () => onExpandRedisDatabase(connection, database.db),
                            })
                          }
                        >
                          <Hash size={12} />
                          <span
                            style={{
                              flex: 1,
                              minWidth: 0,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {key.key_display || key.key_raw}
                          </span>
                          <span style={s.databasePill}>{key.key_type}</span>
                        </button>
                      );
                    })}
                    {canLoadMoreKeys && (
                      <button
                        type="button"
                        className={treeNodeClassName}
                        style={{
                          ...s.databaseListButton,
                          minHeight: 25,
                          padding: "4px 8px 4px 54px",
                          color: "var(--text-secondary)",
                          fontWeight: 650,
                        }}
                        onClick={() => onLoadMoreRedisKeys(connection, database.db)}
                      >
                        <ListTree size={12} />
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {t("database.loadMore")} ({loadedKeys.length}/{redisTotalKeys})
                        </span>
                      </button>
                    )}
                  </div>
                )}
              </div>
            );
          })}
          {redisDatabases.length === 0 && (
            <div style={s.databaseEmptyCompact}>{t("database.empty")}</div>
          )}
        </div>
      );
    }

    if (connection.dbType === "mongodb") {
      const mongoDatabases = (mongoDatabasesByConnection[connection.id] ?? []).filter(
        (database) => {
          const matchesDatabase = matchesSearch(`mongodb database ${database}`, searchQuery);
          const matchesCollection = (
            mongoCollectionsByDatabase[`${connection.id}:${database}`] ?? []
          ).some((collection) =>
            matchesSearch(`mongodb collection ${database} ${collection}`, searchQuery),
          );
          return matchesDatabase || matchesCollection;
        },
      );
      const orderedMongoDatabases = orderPinnedFirst(mongoDatabases, (database) =>
        pinnedTreeNodeIds.has(mongoDatabaseNodeKey(connection.id, database)),
      );
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 3 }}>
          {orderedMongoDatabases.map((database) => {
            const databaseNodeKey = mongoDatabaseNodeKey(connection.id, database);
            const databaseExpanded = expandedDatabaseNames.has(database) || Boolean(searchQuery);
            const toggleMongoDatabase = (loadOnExpand = true) => {
              toggleDatabase(database);
              if (loadOnExpand && !databaseExpanded) onExpandMongoDatabase(connection, database);
            };
            const collections = orderPinnedFirst(
              (mongoCollectionsByDatabase[`${connection.id}:${database}`] ?? []).filter(
                (collection) =>
                  matchesSearch(`mongodb collection ${database} ${collection}`, searchQuery),
              ),
              (collection) =>
                pinnedTreeNodeIds.has(mongoCollectionNodeKey(connection.id, database, collection)),
            );
            return (
              <div key={database}>
                <button
                  type="button"
                  className={treeNodeClassName}
                  style={{
                    ...s.databaseListButton,
                    minHeight: 28,
                    padding: "5px 8px 5px 26px",
                    ...treeNodeSelectionStyle(databaseNodeKey, activeDbxDatabase === database),
                  }}
                  {...treeNodeSelectionAttrs(databaseNodeKey, activeDbxDatabase === database)}
                  onClick={(event) =>
                    handleTreeNodeClick(event, databaseNodeKey, () => {
                      toggleMongoDatabase(false);
                      onSelectMongoDatabase(connection, database);
                    })
                  }
                  onContextMenu={(event) =>
                    onMongoDatabaseContextMenu(event, connection.id, database)
                  }
                  onKeyDown={(event) =>
                    handleKeyShortcut(event, {
                      copyName: database,
                      refresh: () => onExpandMongoDatabase(connection, database),
                    })
                  }
                >
                  <span
                    onClick={(event) => {
                      event.stopPropagation();
                      toggleMongoDatabase();
                    }}
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      color: "var(--text-hint)",
                    }}
                  >
                    <ExpansionGlyph expanded={databaseExpanded} />
                  </span>
                  <Database size={13} />
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {database}
                  </span>
                  {getDefaultDatabase(connection) === database && (
                    <span
                      style={{
                        fontSize: 10,
                        padding: "1px 5px",
                        borderRadius: 4,
                        background: "var(--accent-dim)",
                        color: "var(--accent)",
                        flexShrink: 0,
                      }}
                    >
                      {t("database.defaultDatabaseBadge")}
                    </span>
                  )}
                  {pinnedNodeIcon(databaseNodeKey)}
                </button>
                {databaseExpanded && collections.length > 0 && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 2, marginTop: 2 }}>
                    {collections.map((collection) => {
                      const collectionNodeKey = mongoCollectionNodeKey(
                        connection.id,
                        database,
                        collection,
                      );
                      const collectionExpansionKey = `mongo:${connection.id}:${database}:${collection}`;
                      const collectionExpanded =
                        expandedSchemaKeys.has(collectionExpansionKey) || Boolean(searchQuery);
                      const toggleMongoCollection = (loadOnExpand = true) => {
                        toggleSchema("mongo", `${connection.id}:${database}:${collection}`);
                        if (loadOnExpand && !collectionExpanded)
                          onExpandMongoCollection(connection, database, collection);
                      };
                      const documents = (
                        mongoDocumentsByCollection[`${connection.id}:${database}:${collection}`] ??
                        []
                      ).filter((document, index) =>
                        matchesSearch(
                          `mongodb document ${database} ${collection} ${mongoDocumentPreview(document, index)}`,
                          searchQuery,
                        ),
                      );
                      const collectionDocumentKey = `${connection.id}:${database}:${collection}`;
                      const loadedDocumentCount =
                        mongoDocumentsByCollection[collectionDocumentKey]?.length ?? 0;
                      const totalDocumentCount =
                        mongoDocumentTotalsByCollection[collectionDocumentKey] ?? 0;
                      const hasMoreDocuments = loadedDocumentCount < totalDocumentCount;
                      return (
                        <div key={collection}>
                          <button
                            type="button"
                            className={treeNodeClassName}
                            style={{
                              ...s.databaseListButton,
                              minHeight: 25,
                              padding: "4px 8px 4px 54px",
                              color: "var(--text-muted)",
                              ...treeNodeSelectionStyle(
                                collectionNodeKey,
                                activeDbxDatabase === database && activeDbxSchema === collection,
                              ),
                            }}
                            {...treeNodeSelectionAttrs(
                              collectionNodeKey,
                              activeDbxDatabase === database && activeDbxSchema === collection,
                            )}
                            onClick={(event) =>
                              handleTreeNodeClick(event, collectionNodeKey, () => {
                                toggleMongoCollection(false);
                                onSelectMongoCollection(connection, database, collection);
                              })
                            }
                            onContextMenu={(event) =>
                              onMongoCollectionContextMenu(
                                event,
                                connection.id,
                                database,
                                collection,
                              )
                            }
                            onKeyDown={(event) =>
                              handleKeyShortcut(event, {
                                copyName: collection,
                                refresh: () =>
                                  onExpandMongoCollection(connection, database, collection),
                              })
                            }
                          >
                            <span
                              onClick={(event) => {
                                event.stopPropagation();
                                toggleMongoCollection();
                              }}
                              style={{
                                display: "inline-flex",
                                alignItems: "center",
                                color: "var(--text-hint)",
                              }}
                            >
                              <ExpansionGlyph expanded={collectionExpanded} />
                            </span>
                            <ListTree size={12} />
                            <span
                              style={{
                                flex: 1,
                                minWidth: 0,
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                                whiteSpace: "nowrap",
                              }}
                            >
                              {collection}
                            </span>
                            {pinnedNodeIcon(collectionNodeKey)}
                          </button>
                          {collectionExpanded && documents.length > 0 && (
                            <div
                              style={{
                                display: "flex",
                                flexDirection: "column",
                                gap: 2,
                                marginTop: 2,
                              }}
                            >
                              {documents.map((document, index) => {
                                const documentNodeKey = mongoDocumentNodeKey(
                                  connection.id,
                                  database,
                                  collection,
                                  document,
                                  index,
                                );
                                const documentId = mongoDocumentId(document, index);
                                const documentActive =
                                  activeDbxDatabase === database &&
                                  activeDbxSchema === collection &&
                                  activeMongoDocumentId === documentId;
                                return (
                                  <button
                                    key={documentNodeKey}
                                    type="button"
                                    className={treeNodeClassName}
                                    style={{
                                      ...s.databaseListButton,
                                      minHeight: 24,
                                      padding: "4px 8px 4px 74px",
                                      color: "var(--text-muted)",
                                      ...treeNodeSelectionStyle(documentNodeKey, documentActive),
                                    }}
                                    {...treeNodeSelectionAttrs(documentNodeKey, documentActive)}
                                    onClick={(event) =>
                                      handleTreeNodeClick(event, documentNodeKey, () =>
                                        onSelectMongoDocument(
                                          connection,
                                          database,
                                          collection,
                                          document,
                                        ),
                                      )
                                    }
                                    onContextMenu={(event) =>
                                      onMongoDocumentContextMenu(
                                        event,
                                        connection.id,
                                        database,
                                        collection,
                                        document,
                                      )
                                    }
                                    onKeyDown={(event) =>
                                      handleKeyShortcut(event, {
                                        copyName: documentId,
                                        refresh: () =>
                                          onExpandMongoCollection(connection, database, collection),
                                      })
                                    }
                                  >
                                    <Braces size={12} />
                                    <span
                                      style={{
                                        flex: 1,
                                        minWidth: 0,
                                        overflow: "hidden",
                                        textOverflow: "ellipsis",
                                        whiteSpace: "nowrap",
                                      }}
                                    >
                                      {mongoDocumentPreview(document, index)}
                                    </span>
                                  </button>
                                );
                              })}
                              {hasMoreDocuments && (
                                <button
                                  type="button"
                                  className={treeNodeClassName}
                                  style={{
                                    ...s.databaseListButton,
                                    minHeight: 24,
                                    padding: "4px 8px 4px 74px",
                                    color: "var(--text-muted)",
                                  }}
                                  {...treeNodeSelectionAttrs(
                                    mongoDocumentLoadMoreNodeKey(
                                      connection.id,
                                      database,
                                      collection,
                                    ),
                                    false,
                                  )}
                                  onClick={(event) =>
                                    handleTreeNodeClick(
                                      event,
                                      mongoDocumentLoadMoreNodeKey(
                                        connection.id,
                                        database,
                                        collection,
                                      ),
                                      () =>
                                        onLoadMoreMongoDocuments(connection, database, collection),
                                    )
                                  }
                                  onKeyDown={(event) =>
                                    handleKeyShortcut(event, {
                                      copyName: t("database.loadMore"),
                                      refresh: () =>
                                        onLoadMoreMongoDocuments(connection, database, collection),
                                    })
                                  }
                                >
                                  <span
                                    style={{
                                      width: 12,
                                      textAlign: "center",
                                      color: "var(--text-hint)",
                                    }}
                                  >
                                    +
                                  </span>
                                  <span
                                    style={{
                                      flex: 1,
                                      minWidth: 0,
                                      overflow: "hidden",
                                      textOverflow: "ellipsis",
                                      whiteSpace: "nowrap",
                                    }}
                                  >
                                    {t("database.loadMore")} ({loadedDocumentCount}/
                                    {totalDocumentCount})
                                  </span>
                                </button>
                              )}
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })}
          {orderedMongoDatabases.length === 0 && (
            <div style={s.databaseEmptyCompact}>{t("database.empty")}</div>
          )}
        </div>
      );
    }

    return (
      <div style={{ padding: "4px 6px 0 34px", display: "flex", flexDirection: "column", gap: 6 }}>
        <DbxButton variant="outline" size="sm" onClick={onOpenNoSqlWorkspace}>
          {connection.dbType}
        </DbxButton>
      </div>
    );
  };

  return (
    <div style={s.databaseScroll}>
      <div style={s.databaseSection}>
        <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <label style={{ ...s.databaseSearchBox, flex: 1 }}>
            <Search size={13} />
            <input
              aria-label={t("database.sidebarSearch")}
              style={s.databaseSearchInput}
              placeholder={t("database.sidebarSearchPlaceholder")}
              value={searchDraft}
              onChange={(event) => setSearchDraft(event.target.value)}
            />
          </label>
          <DbxButton
            variant="ghost"
            size="icon-sm"
            icon={Crosshair}
            aria-label={t("database.locateActiveTab")}
            title={t("database.locateActiveTab")}
            onClick={() => {
              const targetKey = activeDbxObject
                ? dbxObjectNodeKey(activeDbxObject)
                : activeDbxSchema
                  ? dbxSchemaNodeKey(activeDbxDatabase ?? "", activeDbxSchema)
                  : activeDbxDatabase
                    ? dbxDatabaseNodeKey(activeDbxDatabase)
                    : activeDbxConnectionId
                      ? dbxConnectionNodeKey(activeDbxConnection!)
                      : null;
              if (targetKey) {
                setSelectedTreeNodeKeys(new Set([targetKey]));
                setSelectionAnchorKey(targetKey);
              }
            }}
          />
        </div>
        <div style={{ display: "flex", gap: 4, padding: "0 1px", flexWrap: "wrap" }}>
          {scopeButton("all", t("file.searchAllTypes"))}
          {scopeButton("connections", t("database.connections"))}
          {scopeButton("databases", t("database.databases"))}
          {scopeButton("objects", t("database.objects"))}
        </div>
      </div>

      <div style={s.databaseSection}>
        <div style={s.databaseSectionTitle}>{t("database.connections")}</div>
        {filteredLegacyConnections.map((connection) => {
          const connectionNodeKey = legacyConnectionNodeKey(connection);
          const expanded = expandedConnectionIds.has(connection.id) || Boolean(searchQuery);
          return (
            <div key={connection.id}>
              <button
                type="button"
                className={treeNodeClassName}
                style={{
                  ...s.databaseListButton,
                  ...treeNodeSelectionStyle(
                    connectionNodeKey,
                    connection.id === activeConnectionId,
                  ),
                }}
                {...treeNodeSelectionAttrs(connectionNodeKey)}
                onClick={(event) =>
                  handleTreeNodeClick(event, connectionNodeKey, () => {
                    toggleConnection(connection.id);
                    onSelectConnection(connection);
                  })
                }
                onKeyDown={(event) =>
                  handleKeyShortcut(event, {
                    copyName: connection.name,
                    refresh: () => onRefreshConnection(connection),
                    delete: () => onDeleteConnection(connection.id),
                    rename: () => onRenameConnection(connection),
                  })
                }
                onContextMenu={(event) => onConnectionContextMenu(event, connection.id, "legacy")}
              >
                <span
                  onClick={(event) => {
                    event.stopPropagation();
                    toggleConnection(connection.id);
                  }}
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    color: "var(--text-hint)",
                  }}
                >
                  <ExpansionGlyph expanded={expanded} />
                </span>
                {connection.endpoint.kind === "local" ? (
                  <ConnectionNameBadge connection={connection} />
                ) : (
                  <Plug size={14} />
                )}
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {connection.name}
                </span>
                <Trash2
                  size={13}
                  onClick={(event) => {
                    event.stopPropagation();
                    onDeleteConnection(connection.id);
                  }}
                />
              </button>
              {expanded &&
                connection.id === activeConnectionId &&
                filteredLegacyObjects.length > 0 && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 3 }}>
                    {filteredLegacyObjects.map((object) => {
                      const objectNodeKey = legacyObjectNodeKey(object);
                      return (
                        <button
                          key={`${object.objectType}:${object.name}`}
                          type="button"
                          className={treeNodeClassName}
                          style={{
                            ...s.databaseListButton,
                            minHeight: 28,
                            padding: "5px 8px 5px 26px",
                            ...treeNodeSelectionStyle(
                              objectNodeKey,
                              object.name === activeObject?.name,
                            ),
                          }}
                          {...treeNodeSelectionAttrs(objectNodeKey)}
                          onClick={(event) =>
                            handleTreeNodeClick(event, objectNodeKey, () =>
                              onSelectLegacyObject(object),
                            )
                          }
                          onKeyDown={(event) =>
                            handleKeyShortcut(event, {
                              copyName: object.name,
                              refresh: () => onSelectLegacyObject(object),
                            })
                          }
                        >
                          <Table2 size={13} />
                          <span
                            style={{
                              flex: 1,
                              minWidth: 0,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {object.name}
                          </span>
                          {object.rowCount != null && (
                            <span style={{ color: "var(--text-hint)", fontSize: 11 }}>
                              {object.rowCount}
                            </span>
                          )}
                        </button>
                      );
                    })}
                  </div>
                )}
            </div>
          );
        })}

        {filteredDbxConnectionGroups.map((connectionGroup) => {
          const groupCollapsed =
            Boolean(connectionGroup.groupName) &&
            collapsedConnectionGroups.has(connectionGroup.groupName) &&
            !searchQuery;
          return (
            <div key={connectionGroup.key}>
              {connectionGroup.groupName && (
                <button
                  type="button"
                  className={treeNodeClassName}
                  style={{
                    ...s.databaseListButton,
                    minHeight: 27,
                    padding: "5px 8px",
                    color: "var(--text-muted)",
                    ...treeNodeSelectionStyle(dbxConnectionGroupNodeKey(connectionGroup.groupName)),
                  }}
                  {...treeNodeSelectionAttrs(dbxConnectionGroupNodeKey(connectionGroup.groupName))}
                  onClick={(event) =>
                    handleTreeNodeClick(
                      event,
                      dbxConnectionGroupNodeKey(connectionGroup.groupName),
                      () => toggleConnectionGroup(connectionGroup.groupName),
                    )
                  }
                  onContextMenu={(event) =>
                    onConnectionGroupContextMenu(event, connectionGroup.groupName)
                  }
                >
                  <ExpansionGlyph expanded={!groupCollapsed} />
                  <Folder size={13} />
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {connectionGroup.label}
                  </span>
                  <span aria-hidden="true" style={{ color: "var(--text-hint)", fontSize: 10 }}>
                    {connectionGroup.connections.length}
                  </span>
                </button>
              )}
              {!groupCollapsed &&
                connectionGroup.connections.map((connection) => {
                  const expanded = expandedConnectionIds.has(connection.id) || Boolean(searchQuery);
                  const isActive = connection.id === activeDbxConnectionId;
                  const connectionNodeKey = dbxConnectionNodeKey(connection);
                  const userAdminNodeKey = dbxUserAdminNodeKey(connection.id);
                  const showUserAdminNode =
                    supportsDbxUserAdmin(connection.dbType) &&
                    (!searchQuery ||
                      matchesSearch(`${t("database.userAdmin")} users privileges`, searchQuery));
                  return (
                    <div key={connection.id}>
                      <button
                        type="button"
                        className={treeNodeClassName}
                        style={{
                          ...s.databaseListButton,
                          ...treeNodeSelectionStyle(connectionNodeKey, isActive),
                        }}
                        {...treeNodeSelectionAttrs(connectionNodeKey)}
                        onClick={(event) =>
                          handleTreeNodeClick(event, connectionNodeKey, () => {
                            toggleConnection(connection.id);
                            onSelectDbxConnection(connection);
                          })
                        }
                        onKeyDown={(event) =>
                          handleKeyShortcut(event, {
                            copyName: connection.name,
                            refresh: () => onRefreshDbxConnection(connection),
                            delete: () => onDeleteDbxConnection(connection.id),
                            rename: () => onRenameDbxConnection(connection),
                          })
                        }
                        onContextMenu={(event) =>
                          onConnectionContextMenu(event, connection.id, "dbx")
                        }
                      >
                        <span
                          onClick={(event) => {
                            event.stopPropagation();
                            toggleConnection(connection.id);
                          }}
                          style={{
                            display: "inline-flex",
                            alignItems: "center",
                            color: "var(--text-hint)",
                          }}
                        >
                          <ExpansionGlyph expanded={expanded} />
                        </span>
                        <ConnectionNameBadge connection={connection} />
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {connection.name}
                        </span>
                        {connection.pinned && (
                          <Pin size={12} fill="currentColor" style={{ color: "var(--accent)" }} />
                        )}
                        <span
                          style={{
                            color: "var(--text-hint)",
                            fontSize: 10,
                            textTransform: "uppercase",
                          }}
                        >
                          {connection.dbType}
                        </span>
                        <Trash2
                          size={13}
                          onClick={(event) => {
                            event.stopPropagation();
                            onDeleteDbxConnection(connection.id);
                          }}
                        />
                      </button>

                      {expanded &&
                        isActive &&
                        activeDbxConnection &&
                        dbxHasSqlObjectBrowser &&
                        filteredDatabases.length > 0 && (
                          <div
                            style={{
                              display: "flex",
                              flexDirection: "column",
                              gap: 3,
                              marginTop: 3,
                            }}
                          >
                            {filteredDatabases.map((database) => {
                              const databaseNodeKey = dbxDatabaseNodeKey(database.name);
                              const databaseExpanded =
                                expandedDatabaseNames.has(database.name) || Boolean(searchQuery);
                              return (
                                <div key={database.name}>
                                  <button
                                    type="button"
                                    className={treeNodeClassName}
                                    style={{
                                      ...s.databaseListButton,
                                      minHeight: 30,
                                      padding: "5px 8px 5px 18px",
                                      ...treeNodeSelectionStyle(
                                        databaseNodeKey,
                                        database.name === activeDbxDatabase,
                                      ),
                                    }}
                                    {...treeNodeSelectionAttrs(databaseNodeKey)}
                                    onClick={(event) =>
                                      handleTreeNodeClick(event, databaseNodeKey, () => {
                                        toggleDatabase(database.name);
                                        onSelectDatabase(activeDbxConnection, database.name);
                                      })
                                    }
                                    onKeyDown={(event) =>
                                      handleKeyShortcut(event, {
                                        copyName: database.name,
                                        refresh: () =>
                                          onRefreshDatabase(activeDbxConnection, database.name),
                                        delete: () =>
                                          onDropDatabase(activeDbxConnection, database.name),
                                      })
                                    }
                                    onContextMenu={(event) =>
                                      onDbxDatabaseContextMenu(
                                        event,
                                        activeDbxConnection.id,
                                        database.name,
                                      )
                                    }
                                  >
                                    <span
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        toggleDatabase(database.name);
                                      }}
                                      style={{
                                        display: "inline-flex",
                                        alignItems: "center",
                                        color: "var(--text-hint)",
                                      }}
                                    >
                                      <ExpansionGlyph expanded={databaseExpanded} />
                                    </span>
                                    <Database size={13} />
                                    <span
                                      style={{
                                        flex: 1,
                                        minWidth: 0,
                                        overflow: "hidden",
                                        textOverflow: "ellipsis",
                                        whiteSpace: "nowrap",
                                      }}
                                    >
                                      {database.name}
                                    </span>
                                    {pinnedNodeIcon(databaseNodeKey)}
                                  </button>
                                  {databaseExpanded &&
                                    database.name === activeDbxDatabase &&
                                    dbxSchemas.length === 1 &&
                                    filteredDbxSchemas.length === 1 &&
                                    filteredDbxSchemas[0] === database.name &&
                                    renderDbxObjectGroups(
                                      filteredDbxObjects.filter(
                                        (object) => (object.schema ?? "") === database.name,
                                      ),
                                      database.name,
                                    )}
                                  {databaseExpanded &&
                                    database.name === activeDbxDatabase &&
                                    !(
                                      dbxSchemas.length === 1 &&
                                      filteredDbxSchemas.length === 1 &&
                                      filteredDbxSchemas[0] === database.name
                                    ) &&
                                    dbxSchemas.length > 0 &&
                                    filteredDbxSchemas.length > 0 && (
                                      <div
                                        style={{
                                          display: "flex",
                                          flexDirection: "column",
                                          gap: 3,
                                          marginTop: 3,
                                        }}
                                      >
                                        {filteredDbxSchemas.map((schemaName) => {
                                          const schemaKey = `${database.name}:${schemaName}`;
                                          const schemaNodeKey = dbxSchemaNodeKey(
                                            database.name,
                                            schemaName,
                                          );
                                          const schemaExpanded =
                                            expandedSchemaKeys.has(schemaKey) ||
                                            Boolean(searchQuery) ||
                                            schemaName === activeDbxSchema ||
                                            filteredDbxSchemas.length === 1;
                                          const schemaObjects = filteredDbxObjects.filter(
                                            (object) => (object.schema ?? "") === schemaName,
                                          );
                                          return (
                                            <div key={schemaKey}>
                                              <button
                                                type="button"
                                                className={treeNodeClassName}
                                                style={{
                                                  ...s.databaseListButton,
                                                  minHeight: 28,
                                                  padding: "5px 8px 5px 30px",
                                                  ...treeNodeSelectionStyle(
                                                    schemaNodeKey,
                                                    schemaName === activeDbxSchema,
                                                  ),
                                                }}
                                                {...treeNodeSelectionAttrs(schemaNodeKey)}
                                                onClick={(event) =>
                                                  handleTreeNodeClick(event, schemaNodeKey, () => {
                                                    toggleSchema(database.name, schemaName);
                                                    onSelectDbxSchema(
                                                      activeDbxConnection,
                                                      database.name,
                                                      schemaName,
                                                    );
                                                  })
                                                }
                                                onKeyDown={(event) =>
                                                  handleKeyShortcut(event, {
                                                    copyName: schemaName,
                                                    refresh: () =>
                                                      onRefreshDbxSchema(
                                                        activeDbxConnection,
                                                        database.name,
                                                        schemaName,
                                                      ),
                                                    delete: () =>
                                                      onDropDbxSchema(
                                                        activeDbxConnection,
                                                        database.name,
                                                        schemaName,
                                                      ),
                                                  })
                                                }
                                                onContextMenu={(event) =>
                                                  onDbxSchemaContextMenu(
                                                    event,
                                                    activeDbxConnection.id,
                                                    database.name,
                                                    schemaName,
                                                  )
                                                }
                                              >
                                                <span
                                                  onClick={(event) => {
                                                    event.stopPropagation();
                                                    toggleSchema(database.name, schemaName);
                                                  }}
                                                  style={{
                                                    display: "inline-flex",
                                                    alignItems: "center",
                                                    color: "var(--text-hint)",
                                                  }}
                                                >
                                                  <ExpansionGlyph expanded={schemaExpanded} />
                                                </span>
                                                <ListTree size={13} />
                                                <span
                                                  style={{
                                                    flex: 1,
                                                    minWidth: 0,
                                                    overflow: "hidden",
                                                    textOverflow: "ellipsis",
                                                    whiteSpace: "nowrap",
                                                  }}
                                                >
                                                  {schemaName}
                                                </span>
                                                {pinnedNodeIcon(schemaNodeKey)}
                                              </button>
                                              {schemaExpanded &&
                                                renderDbxObjectGroups(
                                                  schemaObjects,
                                                  schemaKey,
                                                  36,
                                                  42,
                                                  56,
                                                  schemaName,
                                                )}
                                            </div>
                                          );
                                        })}
                                      </div>
                                    )}
                                  {databaseExpanded &&
                                    database.name === activeDbxDatabase &&
                                    dbxSchemas.length === 0 &&
                                    renderDbxObjectGroups(filteredDbxObjects, database.name)}
                                </div>
                              );
                            })}
                          </div>
                        )}

                      {expanded &&
                        isActive &&
                        activeDbxConnection &&
                        !dbxHasSqlObjectBrowser &&
                        renderNoSqlTreeRows(activeDbxConnection)}

                      {expanded && isActive && showUserAdminNode && (
                        <button
                          type="button"
                          className={treeNodeClassName}
                          style={{
                            ...s.databaseListButton,
                            minHeight: 28,
                            padding: "5px 8px 5px 18px",
                            color: "var(--text-muted)",
                            ...treeNodeSelectionStyle(userAdminNodeKey, userAdminActive),
                          }}
                          {...treeNodeSelectionAttrs(userAdminNodeKey, userAdminActive)}
                          onClick={(event) =>
                            handleTreeNodeClick(event, userAdminNodeKey, () =>
                              onOpenUserAdmin(connection),
                            )
                          }
                          onContextMenu={(event) => onUserAdminContextMenu(event, connection.id)}
                          onKeyDown={(event) =>
                            handleKeyShortcut(event, {
                              copyName: t("database.userAdmin"),
                            })
                          }
                        >
                          <UsersRound size={13} />
                          <span
                            style={{
                              flex: 1,
                              minWidth: 0,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {t("database.userAdmin")}
                          </span>
                        </button>
                      )}
                    </div>
                  );
                })}
            </div>
          );
        })}

        {searchQuery && !hasResults && (
          <div style={s.databaseEmptyCompact}>{t("database.sidebarSearchNoResults")}</div>
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
                <div
                  key={column.name}
                  style={{
                    fontSize: 11.5,
                    color: "var(--text-secondary)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={`${column.name} ${column.dataType}`}
                >
                  {column.primaryKey ? "* " : ""}
                  {column.name}
                  <span style={{ color: "var(--text-hint)" }}> {column.dataType || "TEXT"}</span>
                </div>
              ))}
            </div>
            {(activeObject.indexes.length > 0 ||
              activeObject.foreignKeys.length > 0 ||
              activeObject.triggers.length > 0) && (
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
  );
}
