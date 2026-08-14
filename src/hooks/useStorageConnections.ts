import { useCallback, useEffect, useMemo, useState } from "react";
import { storageApi } from "../lib/storageApi";
import type { StorageConnection, StorageProtocolDescriptor } from "../types/storage";

/**
 * 存储连接列表 + 协议描述符 + 已保存的凭据键名。
 *
 * 凭据值永远不出现在这里 —— `secretKeys` 只告诉表单"这个键已经存过了,留空即保留"。
 */
export function useStorageConnections() {
  const [connections, setConnections] = useState<StorageConnection[]>([]);
  const [descriptors, setDescriptors] = useState<StorageProtocolDescriptor[]>([]);
  const [secretKeys, setSecretKeys] = useState<Record<string, string[]>>({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextConnections, nextSecretKeys] = await Promise.all([
        storageApi.listConnections(),
        storageApi.secretKeys(),
      ]);
      setConnections(nextConnections);
      setSecretKeys(nextSecretKeys);
      setError(null);
      return nextConnections;
    } catch (cause) {
      setError(String(cause));
      throw cause;
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const nextDescriptors = await storageApi.protocols();
        if (!cancelled) setDescriptors(nextDescriptors);
      } catch (cause) {
        console.warn("Failed to load storage protocol descriptors", cause);
      }
    })();
    refresh().catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  const saveConnection = useCallback(async (connection: StorageConnection) => {
    const next = await storageApi.saveConnection(connection);
    setConnections(next);
    setSecretKeys(await storageApi.secretKeys());
    return next;
  }, []);

  const deleteConnection = useCallback(async (connectionId: string) => {
    const next = await storageApi.deleteConnection(connectionId);
    setConnections(next);
    setSecretKeys(await storageApi.secretKeys());
    return next;
  }, []);

  const descriptorByProtocol = useMemo(
    () => new Map(descriptors.map((item) => [item.protocol, item])),
    [descriptors],
  );

  return {
    connections,
    descriptors,
    descriptorByProtocol,
    secretKeys,
    loading,
    error,
    refresh,
    saveConnection,
    deleteConnection,
  };
}
