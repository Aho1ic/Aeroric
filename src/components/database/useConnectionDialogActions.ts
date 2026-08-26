/**
 * 「连接对话框」这一层的五支动作,加上它自己那份 `newConnectionGroup`:
 * 新增一条 legacy 连接、打开新建 / 编辑对话框、关掉对话框,以及 dbx 连接存好之后的收尾。
 *
 * 从 `DatabaseView.tsx` 抽出。`newConnectionGroup` 只被 `openNewConnectionDialog` 写、
 * 只被 `<ConnectionDialog initialConnectionGroup>` 读,所以状态一并搬进来,由返回值带出去。
 *
 * 原文里前四支排在 `useDbxDataLoaders` 之前、`handleConnectionSaved` 排在它之后;
 * `handleConnectionSaved` 要用 `loadDbxConnection`,所以整块落在加载器之后 —— 前四支在那之间
 * 没有任何调用者,顺序仍然成立。
 *
 * 逐字保留的两处:`openNewConnectionDialog` 的入参是 `unknown`,只有确实是字符串才 `trim()`
 * 后当分组名,其余情况一律按 `null`(侧边树把菜单事件直接塞进来,不能假定类型);
 * `openEditDbxConnectionDialog` 走的是 `setConnectionDialog(true, connection.id)` 两参形式,
 * 与新建那支的单参形式不是一回事。
 *
 * 与原文唯一的差别:原先直接闭包捕获的 `setError` / `setDbxConnections` 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 它们的身份本来就不变,行为不受影响。
 */

import { useCallback, useState } from "react";

import { createConnectionName } from "../../lib/databaseUtils";
import type { AeroricDbConnectionConfig, DbConnectionConfig, DbEndpoint } from "../../types";

export interface ConnectionDialogActionsDeps {
  connections: DbConnectionConfig[];
  saveConnections: (next: DbConnectionConfig[]) => void;
  inspect: (connection: DbConnectionConfig) => Promise<void>;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  setError: (error: string | null) => void;
  setDbxConnections: (next: AeroricDbConnectionConfig[]) => void;
  setActiveConnectionId: (id: string | null) => void;
  /** 来自 workspace store:第二个参数是「要编辑的 dbx 连接 id」。 */
  setConnectionDialog: (open: boolean, editingDbxConnectionId?: string) => void;
}

export interface ConnectionDialogActions {
  newConnectionGroup: string | null;
  addConnection: (endpoint: DbEndpoint) => void;
  openNewConnectionDialog: (connectionGroup?: unknown) => void;
  openEditDbxConnectionDialog: (connection: AeroricDbConnectionConfig) => void;
  closeConnectionDialog: () => void;
  handleConnectionSaved: (
    next: AeroricDbConnectionConfig[],
    connection: AeroricDbConnectionConfig,
  ) => Promise<void>;
}

export function useConnectionDialogActions(
  deps: ConnectionDialogActionsDeps,
): ConnectionDialogActions {
  const {
    connections,
    saveConnections,
    inspect,
    loadDbxConnection,
    setError,
    setDbxConnections,
    setActiveConnectionId,
    setConnectionDialog,
  } = deps;
  const [newConnectionGroup, setNewConnectionGroup] = useState<string | null>(null);

  const addConnection = useCallback(
    (endpoint: DbEndpoint) => {
      const now = Date.now();
      const connection: DbConnectionConfig = {
        id: `db:${now}:${Math.random().toString(36).slice(2)}`,
        name: createConnectionName(endpoint),
        endpoint,
        readOnly: false,
        createdAt: now,
        lastOpenedAt: now,
      };
      const next = [connection, ...connections];
      saveConnections(next);
      setActiveConnectionId(connection.id);
      inspect(connection);
    },
    [connections, inspect, saveConnections, setActiveConnectionId],
  );

  const openNewConnectionDialog = useCallback(
    (connectionGroup: unknown = null) => {
      setNewConnectionGroup(typeof connectionGroup === "string" ? connectionGroup.trim() : null);
      setError(null);
      setConnectionDialog(true);
    },
    [setConnectionDialog, setError],
  );

  const openEditDbxConnectionDialog = useCallback(
    (connection: AeroricDbConnectionConfig) => {
      setError(null);
      setConnectionDialog(true, connection.id);
    },
    [setConnectionDialog, setError],
  );

  const closeConnectionDialog = useCallback(() => {
    setConnectionDialog(false);
  }, [setConnectionDialog]);

  const handleConnectionSaved = useCallback(
    (next: AeroricDbConnectionConfig[], connection: AeroricDbConnectionConfig) => {
      setDbxConnections(next);
      setConnectionDialog(false);
      return loadDbxConnection(connection);
    },
    [loadDbxConnection, setConnectionDialog, setDbxConnections],
  );

  return {
    newConnectionGroup,
    addConnection,
    openNewConnectionDialog,
    openEditDbxConnectionDialog,
    closeConnectionDialog,
    handleConnectionSaved,
  };
}
