import { useCallback, useState } from "react";
import { databaseApi } from "../lib/databaseApi";
import type {
  DataGridSaveStatementOptions,
  GridSaveRequest,
  SqlPreviewResponse,
  TableDataRequest,
  TableDataResponse,
} from "../types/database";

export function useDataGrid() {
  const [data, setData] = useState<TableDataResponse | null>(null);
  const [preview, setPreview] = useState<SqlPreviewResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const queryTableData = useCallback(async (request: TableDataRequest) => {
    setLoading(true);
    setError(null);
    try {
      const result = await databaseApi.dbxQueryTableData(request);
      setData(result);
      return result;
    } catch (err) {
      setError(String(err));
      throw err;
    } finally {
      setLoading(false);
    }
  }, []);

  const previewGridSql = useCallback(async (options: DataGridSaveStatementOptions) => {
    setError(null);
    try {
      const result = await databaseApi.dbxPreviewGridSql(options);
      setPreview(result);
      return result;
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, []);

  const commitGridChange = useCallback(async (kind: "update" | "insert" | "delete", request: GridSaveRequest) => {
    setLoading(true);
    setError(null);
    try {
      const result =
        kind === "update"
          ? await databaseApi.dbxUpdateCell(request)
          : kind === "insert"
            ? await databaseApi.dbxInsertRow(request)
            : await databaseApi.dbxDeleteRows(request);
      setPreview(result);
      return result;
    } catch (err) {
      setError(String(err));
      throw err;
    } finally {
      setLoading(false);
    }
  }, []);

  return {
    data,
    preview,
    loading,
    error,
    queryTableData,
    previewGridSql,
    commitGridChange,
  };
}
