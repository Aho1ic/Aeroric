/**
 * 侧边树上 redis / mongo 那五种节点的右键动作执行器,合在一个 `runNoSqlContextMenuAction` 里。
 *
 * 从 `DatabaseView.tsx` 抽出:原函数先按 `menu.kind` 分成 redis-database / redis-key /
 * mongo-database / mongo-document 四段,落到最后的是 mongo-collection —— 这个「最后一段不带
 * kind 判断」的写法与分支顺序都保持原样,顺序换了行为就变了。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 这些 setter 的身份本来就不变,行为不受影响。
 */

import { useCallback } from "react";

import { useI18n } from "../../i18n";
import { confirm } from "../../lib/appDialog";
import { databaseApi } from "../../lib/databaseApi";
import type { AeroricDbConnectionConfig, DbExecuteResult, DbQueryResult } from "../../types";
import {
  contextMenuPinnedNodeId,
  mongoDocumentId,
  mongoDocumentRawId,
  type DatabaseContextMenuState,
  type DbWorkspaceMode,
  type NoSqlContextMenuAction,
} from "./databaseViewModel";

export interface NoSqlContextMenuActionsDeps {
  contextMenu: DatabaseContextMenuState | null;
  setContextMenu: (menu: DatabaseContextMenuState | null) => void;
  dbxConnections: AeroricDbConnectionConfig[];
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  setActiveDbxSchema: (schema: string | null) => void;
  setActiveMongoDocumentId: (id: string | null) => void;
  setActiveMongoWorkspaceDatabase: (database: string | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setSql: (sql: string) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  togglePinnedTreeNode: (nodeId: string) => void;
  copyNodeName: (name: string) => void;
  saveDbxDefaultDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  /** 这五支加载器原本都返回拉到的列表,这里只 await 不看结果。 */
  loadRedisSidebarDatabases: (connection: AeroricDbConnectionConfig) => Promise<unknown>;
  loadMongoSidebarDatabases: (connection: AeroricDbConnectionConfig) => Promise<unknown>;
  loadRedisSidebarKeys: (
    connection: AeroricDbConnectionConfig,
    database: number,
    append?: boolean,
  ) => Promise<unknown>;
  loadMongoSidebarCollections: (
    connection: AeroricDbConnectionConfig,
    database: string,
  ) => Promise<unknown>;
  loadMongoSidebarDocuments: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
  ) => Promise<unknown>;
  selectRedisSidebarDatabase: (connection: AeroricDbConnectionConfig, database: number) => void;
  selectRedisSidebarKey: (
    connection: AeroricDbConnectionConfig,
    database: number,
    keyRaw: string,
  ) => void;
  selectMongoSidebarDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string,
  ) => Promise<void>;
  selectMongoSidebarCollection: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
  ) => Promise<void>;
  selectMongoSidebarDocument: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
    document: unknown,
  ) => Promise<void>;
}

export function useNoSqlContextMenuActions(deps: NoSqlContextMenuActionsDeps): {
  runNoSqlContextMenuAction: (action: NoSqlContextMenuAction) => Promise<void>;
} {
  const { t } = useI18n();
  const {
    contextMenu,
    setContextMenu,
    dbxConnections,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setActiveMongoDocumentId,
    setActiveMongoWorkspaceDatabase,
    setWorkspaceMode,
    setSql,
    setSqlResult,
    setQueryResult,
    togglePinnedTreeNode,
    copyNodeName,
    saveDbxDefaultDatabase,
    loadRedisSidebarDatabases,
    loadMongoSidebarDatabases,
    loadRedisSidebarKeys,
    loadMongoSidebarCollections,
    loadMongoSidebarDocuments,
    selectRedisSidebarDatabase,
    selectRedisSidebarKey,
    selectMongoSidebarDatabase,
    selectMongoSidebarCollection,
    selectMongoSidebarDocument,
  } = deps;

  const runNoSqlContextMenuAction = useCallback(
    async (action: NoSqlContextMenuAction) => {
      const menu =
        contextMenu?.kind === "redis-database" ||
        contextMenu?.kind === "redis-key" ||
        contextMenu?.kind === "mongo-database" ||
        contextMenu?.kind === "mongo-collection" ||
        contextMenu?.kind === "mongo-document"
          ? contextMenu
          : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection) return;
      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }

      if (menu.kind === "redis-database") {
        if (action === "copyName") {
          copyNodeName(`db${menu.database}`);
          return;
        }
        if (action === "newQuery") {
          setActiveConnectionId(null);
          setActiveDbxConnectionId(connection.id);
          setActiveDbxDatabase(`db${menu.database}`);
          setActiveDbxSchema(null);
          setActiveMongoDocumentId(null);
          setActiveMongoWorkspaceDatabase(null);
          setWorkspaceMode("query");
          setSql("");
          setSqlResult(null);
          setQueryResult(null);
          return;
        }
        if (action === "openWorkspace") {
          selectRedisSidebarDatabase(connection, menu.database);
          return;
        }
        if (action === "setDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, String(menu.database));
          return;
        }
        if (action === "clearDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, null);
          return;
        }
        if (action === "flushRedisDb") {
          if (connection.readOnly) return;
          const ok = await confirm(t("database.confirmFlushRedisDb", { db: menu.database }), {
            title: t("database.redisFlushDb"),
            kind: "warning",
            okLabel: t("database.redisFlushDbConfirm"),
            cancelLabel: t("common.cancel"),
          });
          if (!ok) return;
          await databaseApi.dbxRedisExecuteCommand({
            connectionId: connection.id,
            db: menu.database,
            command: "FLUSHDB",
            skipSafetyCheck: true,
          });
          await loadRedisSidebarKeys(connection, menu.database);
          await loadRedisSidebarDatabases(connection);
          return;
        }
        await loadRedisSidebarDatabases(connection);
        return;
      }

      if (menu.kind === "redis-key") {
        if (action === "copyName") {
          copyNodeName(menu.keyRaw);
          return;
        }
        if (action === "openWorkspace") {
          selectRedisSidebarKey(connection, menu.database, menu.keyRaw);
          return;
        }
        if (action === "refresh") {
          await loadRedisSidebarKeys(connection, menu.database);
          return;
        }
        if (connection.readOnly) return;
        const ok = await confirm(t("database.confirmDeleteRedisKey", { name: menu.keyRaw }), {
          title: t("database.redisDeleteKey"),
          kind: "warning",
          okLabel: t("database.redisDeleteKey"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxRedisDeleteKey({
          connectionId: connection.id,
          db: menu.database,
          keyRaw: menu.keyRaw,
        });
        await loadRedisSidebarKeys(connection, menu.database);
        return;
      }

      if (menu.kind === "mongo-database") {
        if (action === "copyName") {
          copyNodeName(menu.database);
          return;
        }
        if (action === "newQuery") {
          setActiveConnectionId(null);
          setActiveDbxConnectionId(connection.id);
          setActiveDbxDatabase(menu.database);
          setActiveDbxSchema(null);
          setActiveMongoDocumentId(null);
          setActiveMongoWorkspaceDatabase(null);
          setWorkspaceMode("query");
          setSql("");
          setSqlResult(null);
          setQueryResult(null);
          return;
        }
        if (action === "openWorkspace") {
          await selectMongoSidebarDatabase(connection, menu.database);
          return;
        }
        if (action === "setDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, menu.database);
          return;
        }
        if (action === "clearDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, null);
          return;
        }
        await loadMongoSidebarDatabases(connection);
        await loadMongoSidebarCollections(connection, menu.database);
        return;
      }

      if (menu.kind === "mongo-document") {
        const documentId = mongoDocumentId(menu.document);
        if (action === "copyName") {
          copyNodeName(documentId);
          return;
        }
        if (action === "openWorkspace") {
          await selectMongoSidebarDocument(
            connection,
            menu.database,
            menu.collection,
            menu.document,
          );
          return;
        }
        if (action === "refresh") {
          await loadMongoSidebarDocuments(connection, menu.database, menu.collection);
          return;
        }
        const rawId = mongoDocumentRawId(menu.document);
        if (connection.readOnly || rawId == null) return;
        const ok = await confirm(
          t("database.confirmDeleteMongoDocument", { collection: menu.collection, id: documentId }),
          {
            title: t("database.mongoDeleteDocument"),
            kind: "warning",
            okLabel: t("database.mongoDeleteDocument"),
            cancelLabel: t("common.cancel"),
          },
        );
        if (!ok) return;
        await databaseApi.dbxMongoDeleteDocuments({
          connectionId: connection.id,
          database: menu.database,
          collection: menu.collection,
          filterJson: JSON.stringify({ _id: rawId }),
          many: false,
        });
        setActiveMongoDocumentId(null);
        await loadMongoSidebarDocuments(connection, menu.database, menu.collection);
        return;
      }

      if (action === "copyName") {
        copyNodeName(menu.collection);
        return;
      }
      if (action === "openWorkspace") {
        await selectMongoSidebarCollection(connection, menu.database, menu.collection);
        return;
      }
      await loadMongoSidebarCollections(connection, menu.database);
    },
    [
      contextMenu,
      setContextMenu,
      dbxConnections,
      loadMongoSidebarCollections,
      togglePinnedTreeNode,
      loadRedisSidebarDatabases,
      copyNodeName,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      selectRedisSidebarDatabase,
      saveDbxDefaultDatabase,
      t,
      loadRedisSidebarKeys,
      selectRedisSidebarKey,
      loadMongoSidebarDatabases,
      selectMongoSidebarDatabase,
      loadMongoSidebarDocuments,
      selectMongoSidebarDocument,
      selectMongoSidebarCollection,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setActiveMongoDocumentId,
      setActiveMongoWorkspaceDatabase,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  return { runNoSqlContextMenuAction };
}
