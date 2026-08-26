/**
 * 会改写「某条 dbx 连接自己那份 `dbx` 配置」的两支动作:把某个库设成默认库,
 * 以及新建一个 DuckDB 文件并 ATTACH 到当前连接上。外加一支 `reloadImportedObject`。
 *
 * 从 `DatabaseView.tsx` 抽出。前两支的形状是一样的:读出 `connection.dbx`(不是对象就当空对象)、
 * 改掉其中一个字段、`dbxSaveConnection` 落盘、重拉一遍连接列表,最后按情况
 * `loadDbxConnection` 重新加载 —— 差别只在改哪个字段,以及重新加载的条件。
 *
 * `reloadImportedObject` 只是 `loadDbxObject` 的一层适配(把入参顺序拧成
 * `useTableImportDialog` 要的样子),它唯一的消费者就是那个对话框。四行的东西不值得单开一个文件,
 * 又正好和上面两支挨在一起、依赖同一批加载器,所以一并收进来。
 *
 * 三支在原文里分别排在 `useVisibleDatabasesDialog`、`useDatabaseExportDialog`、
 * `useCreateDatabaseDialog` 之后,中间夹着那几个对话框 hook。整块提到最前面那支的位置是安全的:
 * 三支只依赖 `useDbxDataLoaders` 给出的两支加载器(声明在更前面),而它们的调用方
 * (`useDbxTreeContextMenuActions` / `useNoSqlContextMenuActions` /
 * `useConnectionContextMenuActions` / `useTableImportDialog`)全都在更后面。
 *
 * 逐字保留的几处:
 * - `saveDbxDefaultDatabase` 传 `null` 时是 `delete nextDbx.database`,不是写成 `null`。
 * - 它只在「当前正打开的就是这条连接」时才重新加载;`createDuckDbAttachedDatabaseFile`
 *   则是无条件重新加载。
 * - 新建 DuckDB 那支:先 `dbxConnect` 再列库,库名用 `uniqueDuckDbAttachedDatabaseName` 去重,
 *   ATTACH 语句由后端 `dbxBuildDuckDbAttachDatabaseSql` 拼(前端不自己拼 SQL),
 *   执行时 `database: ""`;用户在保存框里按取消(返回值不是非空字符串)就直接退出,不报错。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 它们的身份本来就不变,行为不受影响。
 */

import { useCallback } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";

import { databaseApi } from "../../lib/databaseApi";
import type { AeroricDbConnectionConfig, DbxObjectInfo } from "../../types";
import {
  dbxAttachedDatabaseRecords,
  duckDbAttachedDatabaseNameFromPath,
  ensureDuckDbFileExtension,
  uniqueDuckDbAttachedDatabaseName,
} from "./databaseViewModel";

export interface DbxConnectionConfigActionsDeps {
  activeDbxConnectionId: string | null;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  loadDbxObject: (
    object: DbxObjectInfo,
    targetPage: number,
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setDbxConnections: (next: AeroricDbConnectionConfig[]) => void;
}

export interface DbxConnectionConfigActions {
  saveDbxDefaultDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  createDuckDbAttachedDatabaseFile: (connection: AeroricDbConnectionConfig) => Promise<void>;
  reloadImportedObject: (
    object: DbxObjectInfo,
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
}

export function useDbxConnectionConfigActions(
  deps: DbxConnectionConfigActionsDeps,
): DbxConnectionConfigActions {
  const {
    activeDbxConnectionId,
    loadDbxConnection,
    loadDbxObject,
    setLoading,
    setError,
    setDbxConnections,
  } = deps;

  const saveDbxDefaultDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null) => {
      const currentDbx =
        connection.dbx && typeof connection.dbx === "object"
          ? (connection.dbx as Record<string, unknown>)
          : {};
      const nextDbx = { ...currentDbx };
      if (database) {
        nextDbx.database = database;
      } else {
        delete nextDbx.database;
      }
      const nextConnection: AeroricDbConnectionConfig = {
        ...connection,
        dbx: nextDbx,
      };
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxSaveConnection(nextConnection);
        setDbxConnections(await databaseApi.dbxListConnections());
        if (activeDbxConnectionId === connection.id) {
          await loadDbxConnection(nextConnection);
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeDbxConnectionId, loadDbxConnection, setDbxConnections, setError, setLoading],
  );

  const createDuckDbAttachedDatabaseFile = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const selectedPath = await saveDialog({
        defaultPath: "database.duckdb",
        filters: [{ name: "DuckDB", extensions: ["duckdb", "db"] }],
      });
      if (typeof selectedPath !== "string" || !selectedPath.trim()) return;

      const path = ensureDuckDbFileExtension(selectedPath.trim());
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxConnect(connection.id);
        const databases = await databaseApi.dbxListDatabases(connection.id);
        const name = uniqueDuckDbAttachedDatabaseName(
          duckDbAttachedDatabaseNameFromPath(path),
          databases.map((database) => database.name),
        );
        const sql = await databaseApi.dbxBuildDuckDbAttachDatabaseSql(path, name);
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database: "",
          sql,
        });

        const currentDbx =
          connection.dbx && typeof connection.dbx === "object"
            ? (connection.dbx as Record<string, unknown>)
            : {};
        const nextConnection: AeroricDbConnectionConfig = {
          ...connection,
          dbx: {
            ...currentDbx,
            attached_databases: [...dbxAttachedDatabaseRecords(connection), { name, path }],
          },
        };
        await databaseApi.dbxSaveConnection(nextConnection);
        setDbxConnections(await databaseApi.dbxListConnections());
        await loadDbxConnection(nextConnection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxConnection, setDbxConnections, setError, setLoading],
  );

  const reloadImportedObject = useCallback(
    (object: DbxObjectInfo, connection: AeroricDbConnectionConfig, database: string | null) =>
      loadDbxObject(object, 1, connection, database),
    [loadDbxObject],
  );

  return { saveDbxDefaultDatabase, createDuckDbAttachedDatabaseFile, reloadImportedObject };
}
