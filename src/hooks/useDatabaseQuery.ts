import { useCallback, useRef, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type { DbxQueryResult, ExecuteMultiRequest, ExecuteQueryRequest } from "../types/database";

export function useDatabaseQuery() {
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<DbxQueryResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const queryGenerationRef = useRef(0);

  const executeQuery = useCallback(async (request: ExecuteQueryRequest) => {
    const generation = ++queryGenerationRef.current;
    setRunning(true);
    setError(null);
    try {
      const result = await databaseApi.dbxExecuteQuery(request);
      if (generation !== queryGenerationRef.current) return result;
      setResults([result]);
      return result;
    } catch (err) {
      if (generation !== queryGenerationRef.current) throw err;
      setError(String(err));
      throw err;
    } finally {
      if (generation === queryGenerationRef.current) {
        setRunning(false);
      }
    }
  }, []);

  const executeMulti = useCallback(async (request: ExecuteMultiRequest) => {
    const generation = ++queryGenerationRef.current;
    setRunning(true);
    setError(null);
    try {
      const next = await databaseApi.dbxExecuteMulti(request);
      if (generation !== queryGenerationRef.current) return next;
      setResults(next);
      return next;
    } catch (err) {
      if (generation !== queryGenerationRef.current) throw err;
      setError(String(err));
      throw err;
    } finally {
      if (generation === queryGenerationRef.current) {
        setRunning(false);
      }
    }
  }, []);

  const cancelQuery = useCallback(async (executionId: string) => {
    await databaseApi.dbxCancelQuery(executionId);
  }, []);

  const closeResultSession = useCallback(
    async (
      connectionId: string,
      sessionId: string,
      database?: string | null,
      clientSessionId?: string | null,
    ) => {
      await databaseApi.dbxCloseResultSession({
        connectionId,
        sessionId,
        database,
        clientSessionId,
      });
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
