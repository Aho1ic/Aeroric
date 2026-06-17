import { useCallback, useState } from "react";
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

  const requireConnection = useCallback(() => {
    if (!connectionId) {
      throw new Error("No MongoDB connection selected");
    }
    return connectionId;
  }, [connectionId]);

  const loadDatabases = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await databaseApi.dbxMongoListDatabases(requireConnection());
      setDatabases(next);
      return next;
    } catch (err) {
      setError(String(err));
      throw err;
    } finally {
      setLoading(false);
    }
  }, [requireConnection]);

  const loadCollections = useCallback(
    async (database: string) => {
      setLoading(true);
      setError(null);
      try {
        const next = await databaseApi.dbxMongoListCollections(requireConnection(), database);
        setCollections(next);
        return next;
      } catch (err) {
        setError(String(err));
        throw err;
      } finally {
        setLoading(false);
      }
    },
    [requireConnection],
  );

  const findDocuments = useCallback(
    async (request: Omit<MongoFindDocumentsRequest, "connectionId">): Promise<MongoDocumentResult> => {
      setLoading(true);
      setError(null);
      try {
        const result = await databaseApi.dbxMongoFindDocuments({
          ...request,
          connectionId: requireConnection(),
        });
        setDocuments(result.documents);
        setTotal(result.total);
        return result;
      } catch (err) {
        setError(String(err));
        throw err;
      } finally {
        setLoading(false);
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
  };
}
