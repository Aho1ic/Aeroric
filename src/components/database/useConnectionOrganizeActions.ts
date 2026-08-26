/**
 * 侧边树上「整理连接」的十一支动作:改名(legacy 与 dbx 各一支)、写元数据、连接置顶、
 * 树节点置顶,以及额外分组的增删改与「把连接移进某个分组」。
 *
 * 从 `DatabaseView.tsx` 抽出:这一批原本在文件里就是连续的一段。除了 `togglePinnedTreeNode`
 * 之外都在动同一份连接配置 —— 要么走 `dbxSaveConnection` + 重新 `dbxListConnections`,
 * 要么走 legacy 的 `saveConnections`;`togglePinnedTreeNode` 只是恰好夹在这段里,依赖上跟
 * 分组那几支一样都是「改 state 顺手落盘」,所以一并收进来,没有单独留在原处。
 *
 * 分支顺序、i18n key 与每一处落盘都与原来逐字一致 —— 尤其 `moveDbxConnectionToGroup` 的
 * `allowEmpty`:清空输入是「移出分组」,`null` 才是取消,两者不能混。
 *
 * `saveDbxConnectionMetadata` 与 `renameExtraDbxConnectionGroup` / `removeExtraDbxConnectionGroup`
 * 只有这段里的兄弟在调,所以留在模块内,不进返回值。
 *
 * 与原文的差别只有依赖数组:原先直接闭包捕获的 `setLoading` / `setError` / `setDbxConnections`
 * 与两支 `setState` 现在从 `deps` 进来,`react-hooks/exhaustive-deps` 不再认得它们是稳定引用,
 * 于是补进了相应的依赖数组 —— 这些 setter 的身份本来就不变,行为不受影响。
 */

import { useCallback, type Dispatch, type SetStateAction } from "react";

import { useI18n } from "../../i18n";
import { confirm, prompt } from "../../lib/appDialog";
import { databaseApi } from "../../lib/databaseApi";
import type { AeroricDbConnectionConfig, DbConnectionConfig } from "../../types";
import { saveExtraDbxConnectionGroups, savePinnedTreeNodeIds } from "./databaseViewModel";

export interface ConnectionOrganizeActionsDeps {
  connections: DbConnectionConfig[];
  saveConnections: (next: DbConnectionConfig[]) => void;
  dbxConnections: AeroricDbConnectionConfig[];
  setDbxConnections: (next: AeroricDbConnectionConfig[]) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  /** 这两支要用 updater 形式读旧值,所以拿的是完整的 setState 而不是收窄过的签名。 */
  setPinnedTreeNodeIds: Dispatch<SetStateAction<Set<string>>>;
  setExtraDbxConnectionGroups: Dispatch<SetStateAction<string[]>>;
}

export interface ConnectionOrganizeActions {
  renameLegacyConnection: (connection: DbConnectionConfig) => Promise<void>;
  renameDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  toggleDbxConnectionPinned: (connection: AeroricDbConnectionConfig) => Promise<void>;
  togglePinnedTreeNode: (nodeId: string) => void;
  addExtraDbxConnectionGroup: (groupName: string) => void;
  moveDbxConnectionToGroup: (connection: AeroricDbConnectionConfig) => Promise<void>;
  renameDbxConnectionGroup: (groupName: string) => Promise<void>;
  deleteDbxConnectionGroup: (groupName: string) => Promise<void>;
}

export function useConnectionOrganizeActions(
  deps: ConnectionOrganizeActionsDeps,
): ConnectionOrganizeActions {
  const { t } = useI18n();
  const {
    connections,
    saveConnections,
    dbxConnections,
    setDbxConnections,
    setLoading,
    setError,
    setPinnedTreeNodeIds,
    setExtraDbxConnectionGroups,
  } = deps;

  const renameLegacyConnection = useCallback(
    async (connection: DbConnectionConfig) => {
      const nextName = await prompt(t("database.renameConnectionPrompt"), {
        title: t("database.renameConnectionPrompt"),
        defaultValue: connection.name,
      });
      if (!nextName || nextName === connection.name) return;
      saveConnections(
        connections.map((item) => (item.id === connection.id ? { ...item, name: nextName } : item)),
      );
    },
    [connections, saveConnections, t],
  );

  const renameDbxConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const nextName = await prompt(t("database.renameConnectionPrompt"), {
        title: t("database.renameConnectionPrompt"),
        defaultValue: connection.name,
      });
      if (!nextName || nextName === connection.name) return;
      const currentDbx =
        connection.dbx && typeof connection.dbx === "object"
          ? (connection.dbx as Record<string, unknown>)
          : {};
      const nextConnection: AeroricDbConnectionConfig = {
        ...connection,
        name: nextName,
        dbx: {
          ...currentDbx,
          name: nextName,
        },
      };
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxSaveConnection(nextConnection);
        setDbxConnections(await databaseApi.dbxListConnections());
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [setDbxConnections, setError, setLoading, t],
  );

  const saveDbxConnectionMetadata = useCallback(
    async (connection: AeroricDbConnectionConfig, patch: Partial<AeroricDbConnectionConfig>) => {
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxSaveConnection({ ...connection, ...patch });
        setDbxConnections(await databaseApi.dbxListConnections());
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [setDbxConnections, setError, setLoading],
  );

  const toggleDbxConnectionPinned = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      await saveDbxConnectionMetadata(connection, { pinned: !connection.pinned });
    },
    [saveDbxConnectionMetadata],
  );

  const togglePinnedTreeNode = useCallback(
    (nodeId: string) => {
      setPinnedTreeNodeIds((current) => {
        const next = new Set(current);
        if (next.has(nodeId)) next.delete(nodeId);
        else next.add(nodeId);
        savePinnedTreeNodeIds(next);
        return next;
      });
    },
    [setPinnedTreeNodeIds],
  );

  const addExtraDbxConnectionGroup = useCallback(
    (groupName: string) => {
      const normalized = groupName.trim();
      if (!normalized) return;
      setExtraDbxConnectionGroups((current) => {
        if (current.some((group) => group.trim() === normalized)) return current;
        const next = [...current, normalized].sort((left, right) =>
          left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
        );
        saveExtraDbxConnectionGroups(next);
        return next;
      });
    },
    [setExtraDbxConnectionGroups],
  );

  const renameExtraDbxConnectionGroup = useCallback(
    (oldName: string, newName: string) => {
      setExtraDbxConnectionGroups((current) => {
        const next = Array.from(
          new Set(
            current
              .map((group) => (group.trim() === oldName ? newName : group))
              .filter((group) => group.trim().length > 0),
          ),
        ).sort((left, right) =>
          left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
        );
        saveExtraDbxConnectionGroups(next);
        return next;
      });
    },
    [setExtraDbxConnectionGroups],
  );

  const removeExtraDbxConnectionGroup = useCallback(
    (groupName: string) => {
      setExtraDbxConnectionGroups((current) => {
        const next = current.filter((group) => group.trim() !== groupName);
        saveExtraDbxConnectionGroups(next);
        return next;
      });
    },
    [setExtraDbxConnectionGroups],
  );

  const moveDbxConnectionToGroup = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      // allowEmpty:清空输入就是"移出分组",不能和取消混为一谈。
      const nextGroup = await prompt(t("database.connectionGroupPrompt"), {
        title: t("database.moveToGroup"),
        defaultValue: connection.connectionGroup ?? "",
        allowEmpty: true,
      });
      if (nextGroup === null) return;
      await saveDbxConnectionMetadata(connection, { connectionGroup: nextGroup || null });
    },
    [saveDbxConnectionMetadata, t],
  );

  const renameDbxConnectionGroup = useCallback(
    async (groupName: string) => {
      const nextGroup = await prompt(t("database.renameConnectionGroupPrompt"), {
        title: t("database.renameConnectionGroup"),
        defaultValue: groupName,
      });
      if (!nextGroup || nextGroup === groupName) return;
      setLoading(true);
      setError(null);
      try {
        renameExtraDbxConnectionGroup(groupName, nextGroup);
        await Promise.all(
          dbxConnections
            .filter((connection) => connection.connectionGroup?.trim() === groupName)
            .map((connection) =>
              databaseApi.dbxSaveConnection({ ...connection, connectionGroup: nextGroup }),
            ),
        );
        setDbxConnections(await databaseApi.dbxListConnections());
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [dbxConnections, renameExtraDbxConnectionGroup, setDbxConnections, setError, setLoading, t],
  );

  const deleteDbxConnectionGroup = useCallback(
    async (groupName: string) => {
      const ok = await confirm(t("database.confirmDeleteConnectionGroup", { name: groupName }), {
        title: t("database.deleteConnectionGroup"),
        kind: "warning",
        okLabel: t("database.deleteConnectionGroup"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      setLoading(true);
      setError(null);
      try {
        removeExtraDbxConnectionGroup(groupName);
        await Promise.all(
          dbxConnections
            .filter((connection) => connection.connectionGroup?.trim() === groupName)
            .map((connection) =>
              databaseApi.dbxSaveConnection({ ...connection, connectionGroup: null }),
            ),
        );
        setDbxConnections(await databaseApi.dbxListConnections());
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [dbxConnections, removeExtraDbxConnectionGroup, setDbxConnections, setError, setLoading, t],
  );

  return {
    renameLegacyConnection,
    renameDbxConnection,
    toggleDbxConnectionPinned,
    togglePinnedTreeNode,
    addExtraDbxConnectionGroup,
    moveDbxConnectionToGroup,
    renameDbxConnectionGroup,
    deleteDbxConnectionGroup,
  };
}
