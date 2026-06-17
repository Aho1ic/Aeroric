import { useCallback, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type {
  RedisDatabaseInfo,
  RedisKeyInfo,
  RedisScanKeysRequest,
  RedisScanResult,
  RedisSetTtlRequest,
  RedisSetValueRequest,
  RedisValue,
} from "../types/database";

export function useRedisBrowser(connectionId: string | null) {
  const [databases, setDatabases] = useState<RedisDatabaseInfo[]>([]);
  const [keys, setKeys] = useState<RedisKeyInfo[]>([]);
  const [cursor, setCursor] = useState(0);
  const [selectedValue, setSelectedValue] = useState<RedisValue | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const requireConnection = useCallback(() => {
    if (!connectionId) {
      throw new Error("No Redis connection selected");
    }
    return connectionId;
  }, [connectionId]);

  const loadDatabases = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await databaseApi.dbxRedisListDatabases(requireConnection());
      setDatabases(next);
      return next;
    } catch (err) {
      setError(String(err));
      throw err;
    } finally {
      setLoading(false);
    }
  }, [requireConnection]);

  const scanKeys = useCallback(
    async (request: Omit<RedisScanKeysRequest, "connectionId">): Promise<RedisScanResult> => {
      setLoading(true);
      setError(null);
      try {
        const result = await databaseApi.dbxRedisScanKeys({
          ...request,
          connectionId: requireConnection(),
        });
        setCursor(result.cursor);
        setKeys((current) => (request.cursor && request.cursor > 0 ? [...current, ...result.keys] : result.keys));
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

  const loadValue = useCallback(
    async (db: number, keyRaw: string) => {
      setLoading(true);
      setError(null);
      try {
        const value = await databaseApi.dbxRedisGetValue({
          connectionId: requireConnection(),
          db,
          keyRaw,
        });
        setSelectedValue(value);
        return value;
      } catch (err) {
        setError(String(err));
        throw err;
      } finally {
        setLoading(false);
      }
    },
    [requireConnection],
  );

  const setValue = useCallback(
    async (request: Omit<RedisSetValueRequest, "connectionId">) => {
      await databaseApi.dbxRedisSetValue({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const deleteKey = useCallback(
    async (db: number, keyRaw: string) => {
      await databaseApi.dbxRedisDeleteKey({ connectionId: requireConnection(), db, keyRaw });
      setKeys((current) => current.filter((key) => key.key_raw !== keyRaw));
      setSelectedValue((current) => (current?.key_raw === keyRaw ? null : current));
    },
    [requireConnection],
  );

  const setTtl = useCallback(
    async (request: Omit<RedisSetTtlRequest, "connectionId">) => {
      await databaseApi.dbxRedisSetTtl({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  return {
    databases,
    keys,
    cursor,
    selectedValue,
    loading,
    error,
    loadDatabases,
    scanKeys,
    loadValue,
    setValue,
    deleteKey,
    setTtl,
  };
}
