import { useCallback, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type { DbxQueryResult, ExecuteQueryRequest } from "../types/database";

export function useDatabaseQuery() {
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<DbxQueryResult[]>([]);
  const [error, setError] = useState<string | null>(null);

  const executeQuery = useCallback(async (request: ExecuteQueryRequest) => {
    setRunning(true);
    setError(null);
    try {
      const result = await databaseApi.dbxExecuteQuery(request);
      setResults([result]);
      return result;
    } catch (err) {
      setError(String(err));
      throw err;
    } finally {
      setRunning(false);
    }
  }, []);

  const executeMulti = useCallback(async (request: ExecuteQueryRequest) => {
    setRunning(true);
    setError(null);
    try {
      const next = await databaseApi.dbxExecuteMulti(request);
      setResults(next);
      return next;
    } catch (err) {
      setError(String(err));
      throw err;
    } finally {
      setRunning(false);
    }
  }, []);

  const cancelQuery = useCallback(async (executionId: string) => {
    await databaseApi.dbxCancelQuery(executionId);
  }, []);

  const closeResultSession = useCallback(
    async (connectionId: string, sessionId: string, database?: string | null, clientSessionId?: string | null) => {
      await databaseApi.dbxCloseResultSession({ connectionId, sessionId, database, clientSessionId });
      setResults((current) => current.filter((result) => result.session_id !== sessionId));
    },
    [],
  );

  return {
    running,
    results,
    error,
    executeQuery,
    executeMulti,
    cancelQuery,
    closeResultSession,
  };
}
