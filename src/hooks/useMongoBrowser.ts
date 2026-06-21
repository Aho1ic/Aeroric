import { useCallback, useEffect, useRef, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type {
  MongoDeleteDocumentsRequest,
  MongoDocumentResult,
  MongoFindDocumentsRequest,
  MongoInsertDocumentRequest,
  MongoUpdateDocumentRequest,
} from "../types/database";

export function useMongoBrowser(connectionId: string | null) {
  const [databases, setDatabases] = useState<string[]>([]);
  const [collections, setCollections] = useState<string[]>([]);
  const [documents, setDocuments] = useState<unknown[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestGeneration = useRef(0);

  const resetBrowserState = useCallback(() => {
    requestGeneration.current += 1;
    setDatabases([]);
    setCollections([]);
    setDocuments([]);
    setTotal(0);
    setLoading(false);
    setError(null);
  }, []);

  useEffect(() => {
    resetBrowserState();
  }, [connectionId, resetBrowserState]);

  const requireConnection = useCallback(() => {
    if (!connectionId) {
      throw new Error("No MongoDB connection selected");
    }
    return connectionId;
  }, [connectionId]);

  const loadDatabases = useCallback(async () => {
    const generation = requestGeneration.current;
    setLoading(true);
    setError(null);
    try {
      const next = await databaseApi.dbxMongoListDatabases(requireConnection());
      if (requestGeneration.current === generation) setDatabases(next);
      return next;
    } catch (err) {
      if (requestGeneration.current === generation) setError(String(err));
      throw err;
    } finally {
      if (requestGeneration.current === generation) setLoading(false);
    }
  }, [requireConnection]);

  const loadCollections = useCallback(
    async (database: string) => {
      const generation = requestGeneration.current;
      setLoading(true);
      setError(null);
      try {
        const next = await databaseApi.dbxMongoListCollections(requireConnection(), database);
        if (requestGeneration.current === generation) setCollections(next);
        return next;
      } catch (err) {
        if (requestGeneration.current === generation) setError(String(err));
        throw err;
      } finally {
        if (requestGeneration.current === generation) setLoading(false);
      }
    },
    [requireConnection],
  );

  const findDocuments = useCallback(
    async (request: Omit<MongoFindDocumentsRequest, "connectionId"> & { append?: boolean }): Promise<MongoDocumentResult> => {
      const generation = requestGeneration.current;
      const { append, ...payload } = request;
      setLoading(true);
      setError(null);
      try {
        const result = await databaseApi.dbxMongoFindDocuments({
          ...payload,
          connectionId: requireConnection(),
        });
        if (requestGeneration.current === generation) {
          setDocuments((current) => (append ? [...current, ...result.documents] : result.documents));
          setTotal(result.total);
        }
        return result;
      } catch (err) {
        if (requestGeneration.current === generation) setError(String(err));
        throw err;
      } finally {
        if (requestGeneration.current === generation) setLoading(false);
      }
    },
    [requireConnection],
  );

  const insertDocument = useCallback(
    async (request: Omit<MongoInsertDocumentRequest, "connectionId">) =>
      databaseApi.dbxMongoInsertDocument({ ...request, connectionId: requireConnection() }),
    [requireConnection],
  );

  const updateDocument = useCallback(
    async (request: Omit<MongoUpdateDocumentRequest, "connectionId">) =>
      databaseApi.dbxMongoUpdateDocument({ ...request, connectionId: requireConnection() }),
    [requireConnection],
  );

  const deleteDocuments = useCallback(
    async (request: Omit<MongoDeleteDocumentsRequest, "connectionId">) =>
      databaseApi.dbxMongoDeleteDocuments({ ...request, connectionId: requireConnection() }),
    [requireConnection],
  );

  const clearDocuments = useCallback(() => {
    requestGeneration.current += 1;
    setDocuments([]);
    setTotal(0);
  }, []);

  const clearCollections = useCallback(() => {
    requestGeneration.current += 1;
    setCollections([]);
  }, []);

  return {
    databases,
    collections,
    documents,
    total,
    loading,
    error,
    loadDatabases,
    loadCollections,
    findDocuments,
    insertDocument,
    updateDocument,
    deleteDocuments,
    clearDocuments,
    clearCollections,
    resetBrowserState,
  };
}
