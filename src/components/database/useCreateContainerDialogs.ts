/**
 * 「新建数据库」与「新建 schema」两个对话框的状态与动作。
 *
 * 从 `DatabaseView.tsx` 抽出,手法与同目录的 `useTableImportDialog.ts` 等一致。
 * 这两个放在同一个模块里:它们是同一条链路的两级容器(库 → schema),流程也一样
 * ——「让后端拼 DDL → 执行 → 重新加载父节点」,分成四个文件反而更难对照。
 * 两个 hook 之间没有共享 state。
 */

import { useCallback, useMemo, useState } from "react";

import type { AeroricDbConnectionConfig } from "../../types";
import { databaseApi } from "../../lib/databaseApi";
import { canSetCreateDatabaseCharset, dbxDriverProfile } from "./databaseViewModel";

export interface CreateDatabaseDialogDeps {
  dbxConnections: AeroricDbConnectionConfig[];
  /** DatabaseView 的全局忙碌态 / 错误条。 */
  setGlobalLoading: (loading: boolean) => void;
  setGlobalError: (error: string | null) => void;
  /** 建完库后重新加载该连接,新库才会出现在树里。 */
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
}

export interface CreateDatabaseDialogState {
  connection: AeroricDbConnectionConfig | null;
  name: string;
  charset: string;
  collation: string;
  /** 只有 MySQL 系的引擎能在建库时指定字符集,别的引擎不渲染那两个输入框。 */
  supportsCharset: boolean;
  open: (connection: AeroricDbConnectionConfig) => void;
  close: () => void;
  setName: (name: string) => void;
  setCharset: (charset: string) => void;
  setCollation: (collation: string) => void;
  submit: () => Promise<void>;
}

export interface CreateSchemaTarget {
  connectionId: string;
  database: string;
}

export interface CreateSchemaDialogDeps {
  dbxConnections: AeroricDbConnectionConfig[];
  setGlobalLoading: (loading: boolean) => void;
  setGlobalError: (error: string | null) => void;
  /** 建完 schema 后重新加载所属库。 */
  loadDbxDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void> | void;
}

export interface CreateSchemaDialogState {
  target: CreateSchemaTarget | null;
  connection: AeroricDbConnectionConfig | null;
  name: string;
  open: (connection: AeroricDbConnectionConfig, database: string) => void;
  close: () => void;
  setName: (name: string) => void;
  submit: () => Promise<void>;
}

const DEFAULT_CHARSET = "utf8mb4";
const DEFAULT_COLLATION = "utf8mb4_unicode_ci";

export function useCreateDatabaseDialog({
  dbxConnections,
  setGlobalLoading,
  setGlobalError,
  loadDbxConnection,
}: CreateDatabaseDialogDeps): CreateDatabaseDialogState {
  const [connectionId, setConnectionId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [charset, setCharset] = useState(DEFAULT_CHARSET);
  const [collation, setCollation] = useState(DEFAULT_COLLATION);

  const connection = useMemo(
    () => dbxConnections.find((item) => item.id === connectionId) ?? null,
    [connectionId, dbxConnections],
  );
  const supportsCharset = Boolean(connection && canSetCreateDatabaseCharset(connection));

  const open = useCallback(
    (nextConnection: AeroricDbConnectionConfig) => {
      setConnectionId(nextConnection.id);
      setName("");
      setCharset(DEFAULT_CHARSET);
      setCollation(DEFAULT_COLLATION);
      setGlobalError(null);
    },
    [setGlobalError],
  );

  // 关闭只清连接和库名:字符集留着上次的值,连着建几个库时不用重填。
  const close = useCallback(() => {
    setConnectionId(null);
    setName("");
  }, []);

  const submit = useCallback(async () => {
    const trimmed = name.trim();
    if (!connection || !trimmed) return;
    setGlobalLoading(true);
    setGlobalError(null);
    try {
      // DDL 由后端按引擎方言拼,前端不做字符串拼接。
      const sql = await databaseApi.dbxBuildCreateDatabaseSql({
        databaseType: connection.dbType,
        driverProfile: dbxDriverProfile(connection),
        name: trimmed,
        charset: canSetCreateDatabaseCharset(connection) ? charset : null,
        collation: canSetCreateDatabaseCharset(connection) ? collation : null,
      });
      await databaseApi.dbxExecuteQuery({ connectionId: connection.id, database: "", sql });
      close();
      await loadDbxConnection(connection);
    } catch (err) {
      setGlobalError(String(err));
    } finally {
      setGlobalLoading(false);
    }
  }, [
    charset,
    close,
    collation,
    connection,
    loadDbxConnection,
    name,
    setGlobalError,
    setGlobalLoading,
  ]);

  return {
    connection,
    name,
    charset,
    collation,
    supportsCharset,
    open,
    close,
    setName,
    setCharset,
    setCollation,
    submit,
  };
}

export function useCreateSchemaDialog({
  dbxConnections,
  setGlobalLoading,
  setGlobalError,
  loadDbxDatabase,
}: CreateSchemaDialogDeps): CreateSchemaDialogState {
  const [target, setTarget] = useState<CreateSchemaTarget | null>(null);
  const [name, setName] = useState("");

  const connection = useMemo(
    () => dbxConnections.find((item) => item.id === target?.connectionId) ?? null,
    [dbxConnections, target?.connectionId],
  );

  const open = useCallback((nextConnection: AeroricDbConnectionConfig, database: string) => {
    setTarget({ connectionId: nextConnection.id, database });
    setName("");
  }, []);

  const close = useCallback(() => {
    setTarget(null);
    setName("");
  }, []);

  const submit = useCallback(async () => {
    const trimmed = name.trim();
    if (!connection || !target || !trimmed) return;
    setGlobalLoading(true);
    setGlobalError(null);
    try {
      const sql = await databaseApi.dbxBuildCreateSchemaSql({
        databaseType: connection.dbType,
        name: trimmed,
      });
      await databaseApi.dbxExecuteQuery({
        connectionId: connection.id,
        database: target.database,
        sql,
      });
      // close() 会清掉 target,所以先取出刷新需要的库名。
      const parentDatabase = target.database;
      close();
      await loadDbxDatabase(connection, parentDatabase);
    } catch (err) {
      setGlobalError(String(err));
    } finally {
      setGlobalLoading(false);
    }
  }, [close, connection, loadDbxDatabase, name, setGlobalError, setGlobalLoading, target]);

  return { target, connection, name, open, close, setName, submit };
}
