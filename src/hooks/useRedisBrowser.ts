import { useCallback, useEffect, useRef, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type {
  RedisDatabaseInfo,
  RedisCommandRequest,
  RedisCreateKeyRequest,
  RedisHashFieldRequest,
  RedisHashSetRequest,
  RedisKeyInfo,
  RedisListIndexRequest,
  RedisListPushRequest,
  RedisListSetRequest,
  RedisLoadMoreRequest,
  RedisScanKeysRequest,
  RedisScanResult,
  RedisSetAddRequest,
  RedisSetMemberRequest,
  RedisSetTtlRequest,
  RedisSetValueRequest,
  RedisValue,
  RedisZaddRequest,
} from "../types/database";

function mergeRedisValuePage(current: RedisValue, page: RedisValue): RedisValue {
  const currentItems = Array.isArray(current.value) ? current.value : [];
  const pageItems = Array.isArray(page.value) ? page.value : [];
  return {
    ...current,
    value: [...currentItems, ...pageItems],
    total: current.total ?? page.total ?? null,
    scan_cursor: page.scan_cursor ?? null,
    value_is_binary: current.value_is_binary || page.value_is_binary,
  };
}

export function useRedisBrowser(connectionId: string | null) {
  const [databases, setDatabases] = useState<RedisDatabaseInfo[]>([]);
  const [keys, setKeys] = useState<RedisKeyInfo[]>([]);
  const [cursor, setCursor] = useState(0);
  const [totalKeys, setTotalKeys] = useState(0);
  const [selectedValue, setSelectedValue] = useState<RedisValue | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestGeneration = useRef(0);
  const scanGeneration = useRef(0);

  const resetBrowserState = useCallback(() => {
    requestGeneration.current += 1;
    scanGeneration.current += 1;
    setDatabases([]);
    setKeys([]);
    setCursor(0);
    setTotalKeys(0);
    setSelectedValue(null);
    setLoading(false);
    setError(null);
  }, []);

  useEffect(() => {
    resetBrowserState();
  }, [connectionId, resetBrowserState]);

  const requireConnection = useCallback(() => {
    if (!connectionId) {
      throw new Error("No Redis connection selected");
    }
    return connectionId;
  }, [connectionId]);

  const loadDatabases = useCallback(async () => {
    const generation = requestGeneration.current;
    setLoading(true);
    setError(null);
    try {
      const next = await databaseApi.dbxRedisListDatabases(requireConnection());
      if (requestGeneration.current === generation) setDatabases(next);
      return next;
    } catch (err) {
      if (requestGeneration.current === generation) setError(String(err));
      throw err;
    } finally {
      if (requestGeneration.current === generation) setLoading(false);
    }
  }, [requireConnection]);

  const scanKeys = useCallback(
    async (request: Omit<RedisScanKeysRequest, "connectionId">): Promise<RedisScanResult> => {
      const generation = requestGeneration.current;
      if (!request.cursor || request.cursor <= 0) scanGeneration.current += 1;
      const scan = scanGeneration.current;
      setLoading(true);
      setError(null);
      try {
        const result = await databaseApi.dbxRedisScanKeys({
          ...request,
          connectionId: requireConnection(),
        });
        if (requestGeneration.current === generation && scanGeneration.current === scan) {
          setCursor(result.cursor);
          setTotalKeys(result.total_keys);
          setKeys((current) =>
            request.cursor && request.cursor > 0 ? [...current, ...result.keys] : result.keys,
          );
        }
        return result;
      } catch (err) {
        if (requestGeneration.current === generation && scanGeneration.current === scan)
          setError(String(err));
        throw err;
      } finally {
        if (requestGeneration.current === generation && scanGeneration.current === scan)
          setLoading(false);
      }
    },
    [requireConnection],
  );

  const loadValue = useCallback(
    async (db: number, keyRaw: string) => {
      const generation = requestGeneration.current;
      setLoading(true);
      setError(null);
      try {
        const value = await databaseApi.dbxRedisGetValue({
          connectionId: requireConnection(),
          db,
          keyRaw,
        });
        if (requestGeneration.current === generation) setSelectedValue(value);
        return value;
      } catch (err) {
        if (requestGeneration.current === generation) setError(String(err));
        throw err;
      } finally {
        if (requestGeneration.current === generation) setLoading(false);
      }
    },
    [requireConnection],
  );

  const loadMoreValue = useCallback(
    async (request: Omit<RedisLoadMoreRequest, "connectionId">) => {
      const generation = requestGeneration.current;
      setLoading(true);
      setError(null);
      try {
        const value = await databaseApi.dbxRedisLoadMore({
          ...request,
          connectionId: requireConnection(),
        });
        if (requestGeneration.current === generation) {
          setSelectedValue((current) => {
            if (!current || current.key_raw !== request.keyRaw) return current;
            return mergeRedisValuePage(current, value);
          });
        }
        return value;
      } catch (err) {
        if (requestGeneration.current === generation) setError(String(err));
        throw err;
      } finally {
        if (requestGeneration.current === generation) setLoading(false);
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
      const generation = requestGeneration.current;
      await databaseApi.dbxRedisDeleteKey({ connectionId: requireConnection(), db, keyRaw });
      if (requestGeneration.current === generation) {
        setKeys((current) => current.filter((key) => key.key_raw !== keyRaw));
        setSelectedValue((current) => (current?.key_raw === keyRaw ? null : current));
      }
    },
    [requireConnection],
  );

  const setTtl = useCallback(
    async (request: Omit<RedisSetTtlRequest, "connectionId">) => {
      await databaseApi.dbxRedisSetTtl({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const createKey = useCallback(
    async (request: Omit<RedisCreateKeyRequest, "connectionId">) => {
      await databaseApi.dbxRedisCreateKey({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const deleteHashField = useCallback(
    async (request: Omit<RedisHashFieldRequest, "connectionId">) => {
      await databaseApi.dbxRedisHashDel({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const setHashField = useCallback(
    async (request: Omit<RedisHashSetRequest, "connectionId">) => {
      await databaseApi.dbxRedisHashSet({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const removeListItem = useCallback(
    async (request: Omit<RedisListIndexRequest, "connectionId">) => {
      await databaseApi.dbxRedisListRemove({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const setListItem = useCallback(
    async (request: Omit<RedisListSetRequest, "connectionId">) => {
      await databaseApi.dbxRedisListSet({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const pushListItem = useCallback(
    async (request: Omit<RedisListPushRequest, "connectionId">) => {
      await databaseApi.dbxRedisListPush({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const removeSetMember = useCallback(
    async (request: Omit<RedisSetMemberRequest, "connectionId">) => {
      await databaseApi.dbxRedisSetRemove({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const addSetMember = useCallback(
    async (request: Omit<RedisSetAddRequest, "connectionId">) => {
      await databaseApi.dbxRedisSetAdd({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const removeZsetMember = useCallback(
    async (request: Omit<RedisSetMemberRequest, "connectionId">) => {
      await databaseApi.dbxRedisZrem({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const addZsetMember = useCallback(
    async (request: Omit<RedisZaddRequest, "connectionId">) => {
      await databaseApi.dbxRedisZadd({ ...request, connectionId: requireConnection() });
    },
    [requireConnection],
  );

  const clearSelectedValue = useCallback(() => {
    setSelectedValue(null);
  }, []);

  const clearKeyspaceState = useCallback(() => {
    requestGeneration.current += 1;
    scanGeneration.current += 1;
    setKeys([]);
    setCursor(0);
    setTotalKeys(0);
    setSelectedValue(null);
    setError(null);
  }, []);

  const executeCommand = useCallback(
    async (request: Omit<RedisCommandRequest, "connectionId">) =>
      databaseApi.dbxRedisExecuteCommand({ ...request, connectionId: requireConnection() }),
    [requireConnection],
  );

  return {
    databases,
    keys,
    cursor,
    totalKeys,
    selectedValue,
    loading,
    error,
    loadDatabases,
    scanKeys,
    loadValue,
    loadMoreValue,
    setValue,
    deleteKey,
    setTtl,
    createKey,
    deleteHashField,
    setHashField,
    removeListItem,
    setListItem,
    pushListItem,
    removeSetMember,
    addSetMember,
    removeZsetMember,
    addZsetMember,
    clearSelectedValue,
    clearKeyspaceState,
    executeCommand,
    resetBrowserState,
  };
}
