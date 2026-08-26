/**
 * 「当前选中了什么」这一层的派生值:三条连接(当前 legacy / 当前 dbx / 正在编辑的 dbx)、
 * 当前 endpoint 与它的 SQL 能力、可用于 SQL 的连接子集、表对象子集,
 * 以及「表属性」面板要的那一组(当前对象、它的 key、列、子对象,再按类型分出索引 / 外键 / 触发器)。
 *
 * 从 `DatabaseView.tsx` 抽出:原文里这一整段就是连续的,共同点是全为纯派生 —— 只从
 * 那几份 useState / store 里的原始状态算出来,不碰加载器,也不改任何状态。
 *
 * 调用位置必须仍在 `useTableInfoPanel` 之前:那支 hook 要吃这里的
 * `selectedDbxInfoObject` / `...Key` / `...Columns` / 三份子列表。
 *
 * `useMemo` 与普通 `const` 的分布逐字保留 —— 原文里返回数组或对象的那几支包了 memo
 * (下游 hook 与组件按引用比较),返回字符串或布尔的那几支没包。改动这个分布会改变
 * 下游的重渲染时机。
 *
 * 逐字保留的几处:
 * - `selectedDbxTable`:当前对象本身是表就用它,否则退回表列表里的第一个,再退回 `null`。
 * - `selectedDbxInfoObject` 比它宽一档 —— 视图也算(`isDbxViewObject`),否则退回
 *   `selectedDbxTable`。两支的判定范围不能互换。
 * - `selectedDbxInfoColumns` 与 `activeDbxGridColumns` 那类取不到时一律回退到同一个
 *   `EMPTY_DBX_COLUMNS` 常量(不是新建 `[]`),这是为了让引用稳定、下游不白重渲染。
 * - 三份子列表都从 `selectedDbxInfoChildObjects` 再筛,筛的是 `dbxTableChildObjectType`
 *   的三个字面量 `INDEX` / `FOREIGN_KEY` / `TRIGGER`。
 * - `visibleDbxDatabases`:连接上配了「只看这一个库」(`configuredTargetDatabase`)时按名字精确筛,
 *   没配就原样透出整份 `dbxDatabases`。中间那支 `activeDbxTargetDatabase` 只有它一个消费者,
 *   所以留在内部不外传。两支都是普通 `const`(原文就是每次渲染重算)。
 */

import { useMemo } from "react";

import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbEndpoint,
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxObjectInfo,
} from "../../types";
import {
  EMPTY_DBX_COLUMNS,
  configuredTargetDatabase,
  dbxChildObjectBelongsToTable,
  dbxObjectKey,
  dbxTableChildObjectType,
  isDbxTableObject,
  isDbxViewObject,
  isSqlDbxConnection,
} from "./databaseViewModel";

export interface DbxSelectionDerivedDeps {
  connections: DbConnectionConfig[];
  dbxConnections: AeroricDbConnectionConfig[];
  activeConnectionId: string | null;
  activeDbxConnectionId: string | null;
  editingDbxConnectionId: string | null;
  activeDbxObject: DbxObjectInfo | null;
  dbxObjects: DbxObjectInfo[];
  dbxColumnsByTable: Record<string, DbxColumnInfo[]>;
  dbxDatabases: DbxDatabaseInfo[];
}

export interface DbxSelectionDerived {
  activeConnection: DbConnectionConfig | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  editingDbxConnection: AeroricDbConnectionConfig | null;
  activeEndpoint: DbEndpoint | null;
  dbxHasSqlObjectBrowser: boolean;
  sqlDbxConnections: AeroricDbConnectionConfig[];
  dbxTableObjects: DbxObjectInfo[];
  selectedDbxTable: DbxObjectInfo | null;
  selectedDbxInfoObject: DbxObjectInfo | null;
  selectedDbxInfoObjectKey: string;
  selectedDbxInfoColumns: DbxColumnInfo[];
  selectedDbxInfoIndexes: DbxObjectInfo[];
  selectedDbxInfoForeignKeys: DbxObjectInfo[];
  selectedDbxInfoTriggers: DbxObjectInfo[];
  visibleDbxDatabases: DbxDatabaseInfo[];
}

export function useDbxSelectionDerived(deps: DbxSelectionDerivedDeps): DbxSelectionDerived {
  const {
    connections,
    dbxConnections,
    activeConnectionId,
    activeDbxConnectionId,
    editingDbxConnectionId,
    activeDbxObject,
    dbxObjects,
    dbxColumnsByTable,
    dbxDatabases,
  } = deps;

  const activeConnection = useMemo(
    () => connections.find((connection) => connection.id === activeConnectionId) ?? null,
    [activeConnectionId, connections],
  );
  const activeDbxConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === activeDbxConnectionId) ?? null,
    [activeDbxConnectionId, dbxConnections],
  );
  const editingDbxConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === editingDbxConnectionId) ?? null,
    [dbxConnections, editingDbxConnectionId],
  );

  const activeEndpoint = activeConnection?.endpoint ?? null;
  const dbxHasSqlObjectBrowser = isSqlDbxConnection(activeDbxConnection);
  const sqlDbxConnections = useMemo(
    () => dbxConnections.filter((connection) => isSqlDbxConnection(connection)),
    [dbxConnections],
  );
  const dbxTableObjects = useMemo(
    () => dbxObjects.filter((object) => isDbxTableObject(object)),
    [dbxObjects],
  );
  const selectedDbxTable = useMemo(
    () =>
      activeDbxObject && isDbxTableObject(activeDbxObject)
        ? activeDbxObject
        : (dbxTableObjects[0] ?? null),
    [activeDbxObject, dbxTableObjects],
  );
  const selectedDbxInfoObject = useMemo(
    () =>
      activeDbxObject && (isDbxTableObject(activeDbxObject) || isDbxViewObject(activeDbxObject))
        ? activeDbxObject
        : selectedDbxTable,
    [activeDbxObject, selectedDbxTable],
  );
  const selectedDbxInfoObjectKey = selectedDbxInfoObject ? dbxObjectKey(selectedDbxInfoObject) : "";
  const selectedDbxInfoColumns = selectedDbxInfoObject
    ? (dbxColumnsByTable[selectedDbxInfoObjectKey] ?? EMPTY_DBX_COLUMNS)
    : EMPTY_DBX_COLUMNS;
  const selectedDbxInfoChildObjects = useMemo(
    () =>
      selectedDbxInfoObject
        ? dbxObjects.filter(
            (object) =>
              Boolean(dbxTableChildObjectType(object)) &&
              dbxChildObjectBelongsToTable(object, selectedDbxInfoObject),
          )
        : [],
    [dbxObjects, selectedDbxInfoObject],
  );
  const selectedDbxInfoIndexes = useMemo(
    () =>
      selectedDbxInfoChildObjects.filter((object) => dbxTableChildObjectType(object) === "INDEX"),
    [selectedDbxInfoChildObjects],
  );
  const selectedDbxInfoForeignKeys = useMemo(
    () =>
      selectedDbxInfoChildObjects.filter(
        (object) => dbxTableChildObjectType(object) === "FOREIGN_KEY",
      ),
    [selectedDbxInfoChildObjects],
  );
  const selectedDbxInfoTriggers = useMemo(
    () =>
      selectedDbxInfoChildObjects.filter((object) => dbxTableChildObjectType(object) === "TRIGGER"),
    [selectedDbxInfoChildObjects],
  );
  const activeDbxTargetDatabase = configuredTargetDatabase(activeDbxConnection);
  const visibleDbxDatabases = activeDbxTargetDatabase
    ? dbxDatabases.filter((database) => database.name === activeDbxTargetDatabase)
    : dbxDatabases;

  return {
    activeConnection,
    activeDbxConnection,
    editingDbxConnection,
    activeEndpoint,
    dbxHasSqlObjectBrowser,
    sqlDbxConnections,
    dbxTableObjects,
    selectedDbxTable,
    selectedDbxInfoObject,
    selectedDbxInfoObjectKey,
    selectedDbxInfoColumns,
    selectedDbxInfoIndexes,
    selectedDbxInfoForeignKeys,
    selectedDbxInfoTriggers,
    visibleDbxDatabases,
  };
}
