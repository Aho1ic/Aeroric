/**
 * 右键菜单这一层的接线与派生:关掉菜单的那支回调、侧边树十四个「弹菜单」回调的工厂,
 * 外加菜单要用的那批「当前这条菜单对应的连接 / 状态」—— 八种节点各自对应的连接(按
 * `DatabaseContextMenu` 要的 `connections` 形状分好组)、连接有没有可移动的目标分组、
 * 这条连接是不是当前打开的那条,以及右键的这个节点有没有被置顶。
 *
 * 从 `DatabaseView.tsx` 抽出。原文里这些分散在三处(`closeContextMenu` 与
 * `sidebarContextMenus` 在最前面,七支按节点类型找连接的在中间,连接节点那四支在后面),
 * 中间隔着十几个 hook;共同点是全都只吃 `contextMenu` / `setContextMenu` 与几份连接状态。
 * 合成一层放在最前面是安全的,也是必要的 —— `closeContextMenu` 有三个对话框 hook 要吃它,
 * 那三支都排在原来那两段派生之前。
 *
 * 派生值全部保留成普通 `const` 而不是 `useMemo`:原文就是每次渲染重算,这些都是一次 `find`
 * 或几个布尔,包一层 memo 反而要多比一遍依赖,行为也会因为引用稳定性变化而变。
 * `contextMenuConnections` 那个对象同理 —— 原文就是在 JSX 里现场拼的字面量,这里照旧不 memo。
 * `closeContextMenu` 的 `useCallback` 与 `sidebarContextMenus` 的 `useMemo` 逐字保留:
 * 它们的引用会进下游 hook 的依赖数组。
 *
 * 逐字保留的几处:
 * - 每支都先对 `contextMenu.kind` 判型再 `find`,找不到用 `?? null`;判型条件不能合并 ——
 *   `dbx` 那支认 `dbx` 与 `user-admin` 两种,`noSql` 那支认 redis / mongo 那五种,
 *   其余六支各认一种。
 * - `contextMenuDbxConnectionHasMoveTargets` 的三个或条件:这条连接自己已经在某个分组里、
 *   有额外分组、或者别的连接带分组 —— 顺序与短路都保持原样。
 * - `contextMenuConnectionActive` 分 legacy / dbx 两条比,其余菜单一律 `false`。
 */

import { useCallback, useMemo } from "react";

import type { AeroricDbConnectionConfig } from "../../types";
import type { DatabaseContextMenuConnections } from "./DatabaseContextMenu";
import {
  buildDatabaseSidebarContextMenuHandlers,
  type DatabaseSidebarContextMenuHandlers,
} from "./databaseSidebarContextMenus";
import { contextMenuPinnedNodeId, type DatabaseContextMenuState } from "./databaseViewModel";

export interface ContextMenuConnectionsDeps {
  contextMenu: DatabaseContextMenuState;
  setContextMenu: (menu: DatabaseContextMenuState) => void;
  dbxConnections: AeroricDbConnectionConfig[];
  extraDbxConnectionGroups: string[];
  activeConnectionId: string | null;
  activeDbxConnectionId: string | null;
  pinnedTreeNodeIds: Set<string>;
}

export interface ContextMenuConnections {
  closeContextMenu: () => void;
  sidebarContextMenus: DatabaseSidebarContextMenuHandlers;
  contextMenuConnections: DatabaseContextMenuConnections;
  contextMenuDbxConnectionHasMoveTargets: boolean;
  contextMenuConnectionActive: boolean;
  contextMenuTreeNodePinned: boolean;
}

export function useContextMenuConnections(
  deps: ContextMenuConnectionsDeps,
): ContextMenuConnections {
  const {
    contextMenu,
    setContextMenu,
    dbxConnections,
    extraDbxConnectionGroups,
    activeConnectionId,
    activeDbxConnectionId,
    pinnedTreeNodeIds,
  } = deps;

  const closeContextMenu = useCallback(() => setContextMenu(null), [setContextMenu]);
  // 侧边树那十四个右键回调都只是「拼一条菜单状态」,统一在 databaseSidebarContextMenus 里造。
  const sidebarContextMenus = useMemo(
    () => buildDatabaseSidebarContextMenuHandlers(setContextMenu),
    [setContextMenu],
  );

  const contextMenuDbxDatabaseConnection =
    contextMenu?.kind === "dbx-database"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxSchemaConnection =
    contextMenu?.kind === "dbx-schema"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxObjectConnection =
    contextMenu?.kind === "dbx-object"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxColumnConnection =
    contextMenu?.kind === "dbx-column"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxTableChildConnection =
    contextMenu?.kind === "dbx-table-child"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxObjectGroupConnection =
    contextMenu?.kind === "dbx-object-group"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuNoSqlConnection =
    contextMenu?.kind === "redis-database" ||
    contextMenu?.kind === "redis-key" ||
    contextMenu?.kind === "mongo-database" ||
    contextMenu?.kind === "mongo-collection" ||
    contextMenu?.kind === "mongo-document"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;

  const contextMenuDbxConnection =
    contextMenu?.kind === "dbx" || contextMenu?.kind === "user-admin"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxConnectionHasMoveTargets = contextMenuDbxConnection
    ? Boolean(contextMenuDbxConnection.connectionGroup?.trim()) ||
      extraDbxConnectionGroups.some((group) => group.trim().length > 0) ||
      dbxConnections.some(
        (connection) =>
          connection.id !== contextMenuDbxConnection.id &&
          Boolean(connection.connectionGroup?.trim()),
      )
    : false;
  const contextMenuConnectionActive =
    contextMenu?.kind === "legacy"
      ? activeConnectionId === contextMenu.connectionId
      : contextMenu?.kind === "dbx"
        ? activeDbxConnectionId === contextMenu.connectionId
        : false;
  const currentContextMenuPinnedNodeId = contextMenuPinnedNodeId(contextMenu);
  const contextMenuTreeNodePinned = currentContextMenuPinnedNodeId
    ? pinnedTreeNodeIds.has(currentContextMenuPinnedNodeId)
    : false;

  return {
    closeContextMenu,
    sidebarContextMenus,
    contextMenuConnections: {
      dbx: contextMenuDbxConnection,
      database: contextMenuDbxDatabaseConnection,
      schema: contextMenuDbxSchemaConnection,
      object: contextMenuDbxObjectConnection,
      objectGroup: contextMenuDbxObjectGroupConnection,
      tableChild: contextMenuDbxTableChildConnection,
      column: contextMenuDbxColumnConnection,
      noSql: contextMenuNoSqlConnection,
    },
    contextMenuDbxConnectionHasMoveTargets,
    contextMenuConnectionActive,
    contextMenuTreeNodePinned,
  };
}
