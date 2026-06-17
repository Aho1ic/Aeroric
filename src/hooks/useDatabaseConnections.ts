import { useCallback, useEffect, useMemo, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type { AeroricDbConnectionConfig } from "../types/database";

export function useDatabaseConnections() {
  const [connections, setConnections] = useState<AeroricDbConnectionConfig[]>([]);
  const [activeConnectionId, setActiveConnectionId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshConnections = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await databaseApi.dbxListConnections();
      setConnections(next);
      setActiveConnectionId((current) => current ?? next[0]?.id ?? null);
      return next;
    } catch (err) {
      setError(String(err));
      throw err;
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refreshConnections().catch(() => undefined);
  }, [refreshConnections]);

  const saveConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxSaveConnection(connection);
        await refreshConnections();
        setActiveConnectionId(connection.id);
      } catch (err) {
        setError(String(err));
        throw err;
      } finally {
        setLoading(false);
      }
    },
    [refreshConnections],
  );

  const deleteConnection = useCallback(
    async (connectionId: string) => {
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxDeleteConnection(connectionId);
        const next = await refreshConnections();
        setActiveConnectionId((current) =>
          current === connectionId ? next[0]?.id ?? null : current,
        );
      } catch (err) {
        setError(String(err));
        throw err;
      } finally {
        setLoading(false);
      }
    },
    [refreshConnections],
  );

  const testConnection = useCallback(async (connection: AeroricDbConnectionConfig) => {
    setError(null);
    try {
      await databaseApi.dbxTestConnection(connection);
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, []);

  const connect = useCallback(async (connectionId: string) => {
    setError(null);
    try {
      await databaseApi.dbxConnect(connectionId);
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, []);

  const disconnect = useCallback(async (connectionId: string) => {
    setError(null);
    try {
      await databaseApi.dbxDisconnect(connectionId);
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, []);

  const activeConnection = useMemo(
    () => connections.find((connection) => connection.id === activeConnectionId) ?? null,
    [activeConnectionId, connections],
  );

  return {
    connections,
    activeConnectionId,
    activeConnection,
    loading,
    error,
    refreshConnections,
    saveConnection,
    deleteConnection,
    testConnection,
    connect,
    disconnect,
    setActiveConnectionId,
  };
}
