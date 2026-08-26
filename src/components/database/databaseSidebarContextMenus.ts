/**
 * 侧边树上十四个「右键菜单」回调的工厂。
 *
 * 从 `DatabaseView.tsx` 抽出:原来它们是 `<DatabaseSidebarTree>` 上十四段几乎一模一样
 * 的内联闭包 —— `event.preventDefault()` 之后把光标位置和节点信息拼成一条
 * `DatabaseContextMenuState`。这里只把重复的那两步收进 `contextMenuAnchor`,payload 仍然
 * 是各自的对象字面量,好让 TS 继续按联合类型逐支校验(换成一个泛型 helper 就得加断言,
 * 反而把 kind 与字段的对应关系放开了)。
 */

import type { MouseEvent } from "react";

import { dbxTableChildObjectType, type DatabaseContextMenuState } from "./databaseViewModel";
import type { DatabaseSidebarTreeProps } from "./DatabaseSidebarTree";

/** 从树的 props 里挑出这十四个,签名跟着 `DatabaseSidebarTree` 走,不另抄一份。 */
export type DatabaseSidebarContextMenuHandlers = Pick<
  DatabaseSidebarTreeProps,
  | "onConnectionContextMenu"
  | "onConnectionGroupContextMenu"
  | "onUserAdminContextMenu"
  | "onDbxDatabaseContextMenu"
  | "onDbxSchemaContextMenu"
  | "onDbxObjectContextMenu"
  | "onDbxColumnContextMenu"
  | "onDbxTableChildObjectContextMenu"
  | "onDbxObjectGroupContextMenu"
  | "onRedisDatabaseContextMenu"
  | "onRedisKeyContextMenu"
  | "onMongoDatabaseContextMenu"
  | "onMongoCollectionContextMenu"
  | "onMongoDocumentContextMenu"
>;

/** 吃掉浏览器自带的右键菜单,顺带算出自家菜单要弹在哪。 */
function contextMenuAnchor(event: MouseEvent): { x: number; y: number } {
  event.preventDefault();
  return { x: event.clientX, y: event.clientY };
}

export function buildDatabaseSidebarContextMenuHandlers(
  setContextMenu: (menu: DatabaseContextMenuState) => void,
): DatabaseSidebarContextMenuHandlers {
  return {
    onConnectionContextMenu: (event, connectionId, kind) =>
      setContextMenu({ ...contextMenuAnchor(event), connectionId, kind }),
    onConnectionGroupContextMenu: (event, groupName) =>
      setContextMenu({ ...contextMenuAnchor(event), groupName, kind: "connection-group" }),
    onUserAdminContextMenu: (event, connectionId) =>
      setContextMenu({ ...contextMenuAnchor(event), connectionId, kind: "user-admin" }),
    onDbxDatabaseContextMenu: (event, connectionId, database) =>
      setContextMenu({ ...contextMenuAnchor(event), connectionId, database, kind: "dbx-database" }),
    onDbxSchemaContextMenu: (event, connectionId, database, schema) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        schema,
        kind: "dbx-schema",
      }),
    onDbxObjectContextMenu: (event, connectionId, database, object) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        object,
        kind: "dbx-object",
      }),
    onDbxColumnContextMenu: (event, connectionId, database, object, column) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        object,
        column,
        kind: "dbx-column",
      }),
    onDbxTableChildObjectContextMenu: (event, connectionId, database, object, childObject) => {
      // 认不出子对象类型就不弹菜单 —— 但默认菜单在判断之前就已经吃掉了,顺序不能反。
      const anchor = contextMenuAnchor(event);
      const childObjectType = dbxTableChildObjectType(childObject);
      if (!childObjectType) return;
      setContextMenu({
        ...anchor,
        connectionId,
        database,
        object,
        childObject,
        childObjectType,
        kind: "dbx-table-child",
      });
    },
    onDbxObjectGroupContextMenu: (event, connectionId, database, schema, groupKey, label) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        schema,
        groupKey,
        label,
        kind: "dbx-object-group",
      }),
    onRedisDatabaseContextMenu: (event, connectionId, database) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        kind: "redis-database",
      }),
    onRedisKeyContextMenu: (event, connectionId, database, keyRaw) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        keyRaw,
        kind: "redis-key",
      }),
    onMongoDatabaseContextMenu: (event, connectionId, database) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        kind: "mongo-database",
      }),
    onMongoCollectionContextMenu: (event, connectionId, database, collection) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        collection,
        kind: "mongo-collection",
      }),
    onMongoDocumentContextMenu: (event, connectionId, database, collection, document) =>
      setContextMenu({
        ...contextMenuAnchor(event),
        connectionId,
        database,
        collection,
        document,
        kind: "mongo-document",
      }),
  };
}
