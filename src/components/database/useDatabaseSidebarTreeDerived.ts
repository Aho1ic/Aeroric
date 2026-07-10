import { useCallback, useMemo } from "react";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbObject,
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxObjectInfo,
  RedisDatabaseInfo,
  RedisKeyInfo,
} from "../../types";
import { supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";
import {
  connectionGroupName,
  databaseObjectNameCollator,
  dbxChildObjectBelongsToTable,
  dbxChildObjectSearchText,
  dbxColumnNodeKey,
  dbxConnectionGroupNodeKey,
  dbxConnectionNodeKey,
  dbxDatabaseNodeKey,
  dbxObjectGroupNodeKey,
  dbxObjectKey,
  dbxObjectLabel,
  dbxObjectNodeKey,
  dbxSchemaNodeKey,
  dbxTableChildObjectNodeKey,
  dbxUserAdminNodeKey,
  groupDbxObjects,
  isDbxTableChildObject,
  legacyConnectionNodeKey,
  legacyObjectNodeKey,
  matchesSearch,
  mongoCollectionNodeKey,
  mongoDatabaseNodeKey,
  mongoDocumentLoadMoreNodeKey,
  mongoDocumentNodeKey,
  mongoDocumentPreview,
  orderPinnedFirst,
  redisDatabaseNodeKey,
  redisKeyNodeKey,
  scopeAllows,
  sortDbxConnections,
  type SearchScope,
} from "./databaseSidebarTreeState";

interface UseDatabaseSidebarTreeDerivedOptions {
  connections: DbConnectionConfig[];
  dbxConnections: AeroricDbConnectionConfig[];
  extraDbxConnectionGroups: string[];
  activeConnectionId: string | null;
  activeDbxConnectionId: string | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxSchema: string | null;
  activeDbxObject: DbxObjectInfo | null;
  dbxHasSqlObjectBrowser: boolean;
  visibleDbxDatabases: DbxDatabaseInfo[];
  dbxSchemas: string[];
  legacyObjects: DbObject[];
  dbxObjects: DbxObjectInfo[];
  dbxColumnsByTable: Record<string, DbxColumnInfo[]>;
  redisDatabasesByConnection: Record<string, RedisDatabaseInfo[]>;
  redisKeysByDatabase: Record<string, RedisKeyInfo[]>;
  mongoDatabasesByConnection: Record<string, string[]>;
  mongoCollectionsByDatabase: Record<string, string[]>;
  mongoDocumentsByCollection: Record<string, unknown[]>;
  mongoDocumentTotalsByCollection: Record<string, number>;
  pinnedTreeNodeIds: ReadonlySet<string>;
  searchQuery: string;
  searchScope: SearchScope;
  userAdminLabel: string;
  expandedConnectionIds: ReadonlySet<string>;
  expandedDatabaseNames: ReadonlySet<string>;
  expandedSchemaKeys: ReadonlySet<string>;
  expandedObjectNodeKeys: ReadonlySet<string>;
  collapsedConnectionGroups: ReadonlySet<string>;
  collapsedObjectGroupKeys: ReadonlySet<string>;
  collapsedObjectNodeKeys: ReadonlySet<string>;
}

export function useDatabaseSidebarTreeDerived({
  connections,
  dbxConnections,
  extraDbxConnectionGroups,
  activeConnectionId,
  activeDbxConnectionId,
  activeDbxConnection,
  activeDbxDatabase,
  activeDbxSchema,
  activeDbxObject,
  dbxHasSqlObjectBrowser,
  visibleDbxDatabases,
  dbxSchemas,
  legacyObjects,
  dbxObjects,
  dbxColumnsByTable,
  redisDatabasesByConnection,
  redisKeysByDatabase,
  mongoDatabasesByConnection,
  mongoCollectionsByDatabase,
  mongoDocumentsByCollection,
  mongoDocumentTotalsByCollection,
  pinnedTreeNodeIds,
  searchQuery,
  searchScope,
  userAdminLabel,
  expandedConnectionIds,
  expandedDatabaseNames,
  expandedSchemaKeys,
  expandedObjectNodeKeys,
  collapsedConnectionGroups,
  collapsedObjectGroupKeys,
  collapsedObjectNodeKeys,
}: UseDatabaseSidebarTreeDerivedOptions) {
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
                  matchesSearch(`${userAdminLabel} users privileges`, searchQuery)) ||
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
      userAdminLabel,
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

  return {
    dbxTableChildObjectsFor,
    filteredLegacyConnections,
    filteredDbxConnections,
    filteredDbxConnectionGroups,
    filteredDatabases,
    filteredDbxSchemas,
    filteredLegacyObjects,
    filteredDbxObjects,
    orderDbxObjectsForTree,
    visibleTreeNodeKeys,
  };
}
