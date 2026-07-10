import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbObject,
  DbxColumnInfo,
  DbxObjectInfo,
  TableChildObjectType,
} from "../../types";

export type SearchScope = "all" | "connections" | "databases" | "objects";
export type DbxObjectGroupKey =
  | "tables"
  | "views"
  | "procedures"
  | "functions"
  | "sequences"
  | "packages";
export type RedisSidebarScanState = { cursor: number; totalKeys: number };

export const databaseObjectNameCollator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: "base",
});

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

export function connectionBadgeColor(connection: DbConnectionConfig | AeroricDbConnectionConfig) {
  return (
    connectionConfigColor(connection) ??
    CONNECTION_BADGE_COLORS[stableNameHash(connection.name) % CONNECTION_BADGE_COLORS.length]
  );
}

export function connectionBadgeText(name: string) {
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

export function dbxObjectLabel(object: DbxObjectInfo) {
  return object.schema ? `${object.schema}.${object.name}` : object.name;
}

export function dbxObjectDisplayName(object: DbxObjectInfo) {
  return object.name;
}

export function dbxObjectKey(object: DbxObjectInfo) {
  return object.schema ? `${object.schema}.${object.name}` : object.name;
}

export function legacyConnectionNodeKey(connection: DbConnectionConfig) {
  return `legacy-connection:${connection.id}`;
}

export function legacyObjectNodeKey(object: DbObject) {
  return `legacy-object:${object.objectType}:${object.name}`;
}

export function dbxConnectionGroupNodeKey(groupName: string) {
  return `dbx-group:${groupName}`;
}

export function dbxUserAdminNodeKey(connectionId: string) {
  return `dbx-user-admin:${connectionId}`;
}

export function dbxConnectionNodeKey(connection: AeroricDbConnectionConfig) {
  return `dbx-connection:${connection.id}`;
}

export function dbxDatabaseNodeKey(databaseName: string) {
  return `dbx-database:${databaseName}`;
}

export function dbxSchemaNodeKey(databaseName: string, schemaName: string) {
  return `dbx-schema:${databaseName}:${schemaName}`;
}

export function dbxObjectGroupNodeKey(scopeKey: string, groupKey: DbxObjectGroupKey) {
  return `dbx-object-group:${scopeKey}:${groupKey}`;
}

export function dbxObjectNodeKey(object: DbxObjectInfo) {
  return `dbx-object:${normalizeDbxObjectType(object.object_type)}:${object.schema ?? ""}:${object.name}`;
}

export function dbxColumnNodeKey(object: DbxObjectInfo, column: DbxColumnInfo) {
  return `dbx-column:${dbxObjectKey(object)}:${column.name}`;
}

export function dbxTableChildObjectNodeKey(object: DbxObjectInfo, childObject: DbxObjectInfo) {
  return `dbx-table-child:${dbxObjectKey(object)}:${normalizeDbxObjectType(childObject.object_type)}:${childObject.name}`;
}

export function redisDatabaseNodeKey(connectionId: string, database: number) {
  return `redis-database:${connectionId}:${database}`;
}

export function redisKeyNodeKey(connectionId: string, database: number, keyRaw: string) {
  return `redis-key:${connectionId}:${database}:${keyRaw}`;
}

export function mongoDatabaseNodeKey(connectionId: string, database: string) {
  return `mongo-database:${connectionId}:${database}`;
}

export function mongoCollectionNodeKey(connectionId: string, database: string, collection: string) {
  return `mongo-collection:${connectionId}:${database}:${collection}`;
}

export function mongoDocumentId(document: unknown, fallback: number) {
  if (document && typeof document === "object" && "_id" in document)
    return String((document as { _id: unknown })._id);
  return `#${fallback + 1}`;
}

export function mongoDocumentNodeKey(
  connectionId: string,
  database: string,
  collection: string,
  document: unknown,
  index: number,
) {
  return `mongo-document:${connectionId}:${database}:${collection}:${mongoDocumentId(document, index)}`;
}

export function mongoDocumentLoadMoreNodeKey(
  connectionId: string,
  database: string,
  collection: string,
) {
  return `mongo-document-load-more:${connectionId}:${database}:${collection}`;
}

export function mongoDocumentPreview(document: unknown, index: number) {
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

export function normalizeDbxObjectType(objectType: string) {
  return objectType.toUpperCase().replace(/[\s-]+/g, "_");
}

export function dbxTableChildObjectType(
  object: DbxObjectInfo,
): Exclude<TableChildObjectType, "COLUMN"> | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType.includes("FOREIGN_KEY")) return "FOREIGN_KEY";
  if (objectType.includes("TRIGGER")) return "TRIGGER";
  if (objectType.includes("INDEX")) return "INDEX";
  return null;
}

export function isDbxTableChildObject(object: DbxObjectInfo) {
  return Boolean(dbxTableChildObjectType(object));
}

export function sameDatabaseName(
  left: string | null | undefined,
  right: string | null | undefined,
) {
  return (left ?? "").trim().toLowerCase() === (right ?? "").trim().toLowerCase();
}

export function dbxChildObjectBelongsToTable(
  childObject: DbxObjectInfo,
  tableObject: DbxObjectInfo,
) {
  if (!childObject.parent_name || !sameDatabaseName(childObject.parent_name, tableObject.name))
    return false;
  if (!tableObject.schema) return true;
  return (
    !childObject.parent_schema || sameDatabaseName(childObject.parent_schema, tableObject.schema)
  );
}

export function dbxChildObjectSearchText(childObject: DbxObjectInfo, parentObject: DbxObjectInfo) {
  return `${normalizeDbxObjectType(childObject.object_type)} ${dbxObjectLabel(parentObject)} ${childObject.name}`;
}

export function canDragDbxObjectReference(object: DbxObjectInfo) {
  const objectType = normalizeDbxObjectType(object.object_type);
  return objectType === "TABLE" || objectType === "VIEW";
}

export const DBX_OBJECT_GROUPS: Array<{
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

export function groupDbxObjects(objects: DbxObjectInfo[]) {
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

export function getDefaultDatabase(connection: AeroricDbConnectionConfig): string | null {
  const config = connection.dbx ?? connection;
  const value = (config as { database?: unknown }).database;
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

export function matchesSearch(value: string, query: string) {
  return !query || value.toLowerCase().includes(query);
}

export function scopeAllows(scope: SearchScope, kind: Exclude<SearchScope, "all">) {
  return scope === "all" || scope === kind;
}

export function connectionGroupName(connection: AeroricDbConnectionConfig): string {
  return connection.connectionGroup?.trim() ?? "";
}

export function sortDbxConnections(
  left: AeroricDbConnectionConfig,
  right: AeroricDbConnectionConfig,
) {
  if (Boolean(left.pinned) !== Boolean(right.pinned)) return left.pinned ? -1 : 1;
  return databaseObjectNameCollator.compare(left.name, right.name);
}

export function orderPinnedFirst<T>(items: T[], isPinned: (item: T) => boolean) {
  const pinned: T[] = [];
  const unpinned: T[] = [];
  for (const item of items) {
    if (isPinned(item)) pinned.push(item);
    else unpinned.push(item);
  }
  return [...pinned, ...unpinned];
}
