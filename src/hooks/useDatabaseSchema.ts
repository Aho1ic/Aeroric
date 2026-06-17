import { useCallback, useMemo, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type { DbxDatabaseInfo, DbxObjectInfo } from "../types/database";

export type DatabaseTreeNode =
  | { kind: "connection"; key: string; connectionId: string; label: string }
  | { kind: "database"; key: string; connectionId: string; database: string; label: string }
  | {
      kind: "schema";
      key: string;
      connectionId: string;
      database: string | null;
      schema: string;
      label: string;
    }
  | {
      kind: "object";
      key: string;
      connectionId: string;
      database: string | null;
      schema: string | null;
      object: DbxObjectInfo;
      label: string;
    };

function nodeKey(
  connectionId: string,
  database?: string | null,
  schema?: string | null,
  objectName?: string | null,
) {
  return `${connectionId}:${database ?? ""}:${schema ?? ""}:${objectName ?? ""}`;
}

export function useDatabaseSchema() {
  const [databasesByConnection, setDatabasesByConnection] = useState<Record<string, DbxDatabaseInfo[]>>({});
  const [schemasByNode, setSchemasByNode] = useState<Record<string, string[]>>({});
  const [objectsByNode, setObjectsByNode] = useState<Record<string, DbxObjectInfo[]>>({});
  const [loadingNodes, setLoadingNodes] = useState<Record<string, boolean>>({});
  const [nodeErrors, setNodeErrors] = useState<Record<string, string>>({});

  const setNodeLoading = useCallback((key: string, loading: boolean) => {
    setLoadingNodes((current) => ({ ...current, [key]: loading }));
  }, []);

  const setNodeError = useCallback((key: string, error: string | null) => {
    setNodeErrors((current) => {
      const next = { ...current };
      if (error) next[key] = error;
      else delete next[key];
      return next;
    });
  }, []);

  const loadConnectionRoot = useCallback(
    async (connectionId: string) => {
      const key = nodeKey(connectionId);
      setNodeLoading(key, true);
      setNodeError(key, null);
      try {
        const databases = await databaseApi.dbxListDatabases(connectionId);
        setDatabasesByConnection((current) => ({ ...current, [connectionId]: databases }));
        return databases;
      } catch (err) {
        setNodeError(key, String(err));
        throw err;
      } finally {
        setNodeLoading(key, false);
      }
    },
    [setNodeError, setNodeLoading],
  );

  const loadDatabase = useCallback(
    async (connectionId: string, database: string) => {
      const key = nodeKey(connectionId, database);
      setNodeLoading(key, true);
      setNodeError(key, null);
      try {
        const schemas = await databaseApi.dbxListSchemas(connectionId, database);
        setSchemasByNode((current) => ({ ...current, [key]: schemas }));
        return schemas;
      } catch (err) {
        setNodeError(key, String(err));
        throw err;
      } finally {
        setNodeLoading(key, false);
      }
    },
    [setNodeError, setNodeLoading],
  );

  const loadSchema = useCallback(
    async (connectionId: string, database: string | null, schema: string | null) => {
      const key = nodeKey(connectionId, database, schema);
      setNodeLoading(key, true);
      setNodeError(key, null);
      try {
        const objects = await databaseApi.dbxListObjects(connectionId, database, schema);
        setObjectsByNode((current) => ({ ...current, [key]: objects }));
        return objects;
      } catch (err) {
        setNodeError(key, String(err));
        throw err;
      } finally {
        setNodeLoading(key, false);
      }
    },
    [setNodeError, setNodeLoading],
  );

  const refreshNode = useCallback(
    async (key: string) => {
      const [connectionId, database, schema] = key.split(":");
      if (!database) return loadConnectionRoot(connectionId);
      if (!schema) return loadDatabase(connectionId, database);
      return loadSchema(connectionId, database, schema);
    },
    [loadConnectionRoot, loadDatabase, loadSchema],
  );

  const allObjects = useMemo(
    () => Object.values(objectsByNode).flat(),
    [objectsByNode],
  );

  const searchTree = useCallback(
    (query: string) => {
      const normalized = query.trim().toLowerCase();
      if (!normalized) return allObjects;
      return allObjects.filter((object) => object.name.toLowerCase().includes(normalized));
    },
    [allObjects],
  );

  return {
    databasesByConnection,
    schemasByNode,
    objectsByNode,
    loadingNodes,
    nodeErrors,
    nodeKey,
    loadConnectionRoot,
    loadDatabase,
    loadSchema,
    refreshNode,
    searchTree,
  };
}
