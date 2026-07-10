import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
import type {
  AeroricDbConnectionConfig,
  DbObject,
  DbQueryResult,
  DbxColumnInfo,
} from "../../types";
import {
  cellPreviewText,
  clampDbxGridColumnWidth,
  dbxGridColumnType,
  estimateDbxGridColumnWidth,
  initialDbxGridColumnWidths,
} from "../../lib/databaseUtils";
import {
  filterDbxGridColumnOptions,
  filterDbxGridRows,
  invertDbxGridColumns,
  pruneDbxGridColumnWidths,
  pruneDbxGridHiddenColumns,
  stageDbxPendingCellEdit,
  toggleDbxGridColumn,
  toggleDbxGridRowSelection as toggleDbxGridRowSelectionState,
  visibleDbxGridColumns,
  type DatabaseRow,
  type DbxPendingCellEdits,
  type TableExportFormat,
} from "./databaseGridState";

export const DBX_GRID_PAGE_SIZE_OPTIONS = [50, 100, 200, 500, 1000] as const;
export const DBX_GRID_DEFAULT_COLUMN_WIDTH = 180;

export type DbxGridCellCoord = {
  rowIndex: number;
  columnIndex: number;
  column: string;
};

export type DbxGridCellDetail = DbxGridCellCoord & {
  value: unknown;
  columnInfo?: DbxColumnInfo;
};

type UseDbxDataGridOptions = {
  initialPageSize: number;
  tableColumns: string[];
  rawTableRows: DatabaseRow[];
  queryResult: DbQueryResult | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxGridColumns: DbxColumnInfo[];
  activeObject: DbObject | null;
  showRowIdColumn: boolean;
};

type InitializeLoadedGridOptions = {
  sameDbxObject: boolean;
  columns: string[];
  rows: DatabaseRow[];
  columnTypes: string[];
  whereInput: string;
  orderByInput: string;
};

export function useDbxDataGrid({
  initialPageSize,
  tableColumns,
  rawTableRows,
  queryResult,
  activeDbxConnection,
  activeDbxGridColumns,
  activeObject,
  showRowIdColumn,
}: UseDbxDataGridOptions) {
  const [dbxGridWhereInput, setDbxGridWhereInput] = useState("");
  const [dbxGridOrderByInput, setDbxGridOrderByInput] = useState("");
  const [dbxGridSearch, setDbxGridSearch] = useState("");
  const [dbxGridColumnSearch, setDbxGridColumnSearch] = useState("");
  const [dbxGridHiddenColumns, setDbxGridHiddenColumns] = useState<Set<string>>(new Set());
  const [dbxDataToolsOpen, setDbxDataToolsOpen] = useState(false);
  const [dbxDataToolsMode, setDbxDataToolsMode] = useState<"root" | "export">("root");
  const [dbxFieldFilterOpen, setDbxFieldFilterOpen] = useState(false);
  const [dbxGridColumnWidths, setDbxGridColumnWidths] = useState<Record<string, number>>({});
  const [resizingDbxGridColumn, setResizingDbxGridColumn] = useState<string | null>(null);
  const [dbxGridPageSize, setDbxGridPageSize] = useState(initialPageSize);
  const [dbxGridSelectedRows, setDbxGridSelectedRows] = useState<Set<number>>(new Set());
  const [dbxGridSelectionAnchor, setDbxGridSelectionAnchor] = useState<number | null>(null);
  const [dbxGridExportFormat, setDbxGridExportFormat] = useState<TableExportFormat>("csv");
  const [dbxCellPreview, setDbxCellPreview] = useState<{
    column: string;
    value: unknown;
  } | null>(null);
  const [dbxCellDetail, setDbxCellDetail] = useState<DbxGridCellDetail | null>(null);
  const [dbxSelectedCell, setDbxSelectedCell] = useState<DbxGridCellCoord | null>(null);
  const [dbxEditingCell, setDbxEditingCell] = useState<DbxGridCellCoord | null>(null);
  const [dbxPendingCellEdits, setDbxPendingCellEdits] = useState<DbxPendingCellEdits>({});
  const [dbxHoveredCell, setDbxHoveredCell] = useState<Omit<DbxGridCellCoord, "column"> | null>(
    null,
  );
  const [dbxRowPreview, setDbxRowPreview] = useState<{
    rowIndex: number;
    row: DatabaseRow;
  } | null>(null);
  const [dbxRowPreviewSearch, setDbxRowPreviewSearch] = useState("");
  const [dbxColumnPreview, setDbxColumnPreview] = useState<{
    column: string;
    columnIndex: number;
  } | null>(null);
  const [dbxColumnPreviewSearch, setDbxColumnPreviewSearch] = useState("");
  const dbxGridColumnResizeStartRef = useRef({
    column: "",
    x: 0,
    width: DBX_GRID_DEFAULT_COLUMN_WIDTH,
  });

  const visibleTableColumns = useMemo(
    () => visibleDbxGridColumns(tableColumns, dbxGridHiddenColumns),
    [dbxGridHiddenColumns, tableColumns],
  );
  const visibleDbxGridDataColumnsWidth = useMemo(
    () =>
      visibleTableColumns.reduce(
        (sum, { column }) => sum + (dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH),
        0,
      ),
    [dbxGridColumnWidths, visibleTableColumns],
  );
  const dbxGridTableMinWidth = useMemo(() => {
    if (!queryResult || !activeDbxConnection) return undefined;
    return 42 + 74 + (showRowIdColumn ? 86 : 0) + visibleDbxGridDataColumnsWidth;
  }, [activeDbxConnection, queryResult, showRowIdColumn, visibleDbxGridDataColumnsWidth]);
  const activeDbxGridColumnsByName = useMemo(
    () => new Map(activeDbxGridColumns.map((column) => [column.name.toLowerCase(), column])),
    [activeDbxGridColumns],
  );
  const filteredDbxGridColumnOptions = useMemo(
    () => filterDbxGridColumnOptions(tableColumns, dbxGridColumnSearch),
    [dbxGridColumnSearch, tableColumns],
  );
  const tableRows = useMemo(
    () => filterDbxGridRows(rawTableRows, dbxGridSearch),
    [dbxGridSearch, rawTableRows],
  );
  const formattedDbxCellPreview = useMemo(
    () => (dbxCellPreview ? cellPreviewText(dbxCellPreview.value) : null),
    [dbxCellPreview],
  );
  const dbxRowPreviewFields = useMemo(
    () =>
      dbxRowPreview
        ? visibleTableColumns.map(({ column, index }) => ({
            column,
            type: dbxGridColumnType(queryResult, index),
            value: dbxRowPreview.row.values[index] ?? null,
            preview: cellPreviewText(dbxRowPreview.row.values[index] ?? null).text,
          }))
        : [],
    [dbxRowPreview, queryResult, visibleTableColumns],
  );
  const filteredDbxRowPreviewFields = useMemo(() => {
    const query = dbxRowPreviewSearch.trim().toLowerCase();
    if (!query) return dbxRowPreviewFields;
    return dbxRowPreviewFields.filter((field) =>
      [field.column, field.type ?? "", field.preview].some((value) =>
        value.toLowerCase().includes(query),
      ),
    );
  }, [dbxRowPreviewFields, dbxRowPreviewSearch]);
  const dbxColumnPreviewFields = useMemo(() => {
    if (!dbxColumnPreview || !queryResult || !activeDbxConnection) return [];
    return tableRows.map((row) => {
      const rowIndex = queryResult.rows.indexOf(row);
      const value = row.values[dbxColumnPreview.columnIndex] ?? null;
      return {
        rowNumber: rowIndex >= 0 ? rowIndex + 1 : 0,
        value,
        preview: cellPreviewText(value).text,
      };
    });
  }, [activeDbxConnection, dbxColumnPreview, queryResult, tableRows]);
  const filteredDbxColumnPreviewFields = useMemo(() => {
    const query = dbxColumnPreviewSearch.trim().toLowerCase();
    if (!query) return dbxColumnPreviewFields;
    return dbxColumnPreviewFields.filter(
      (field) =>
        field.preview.toLowerCase().includes(query) || String(field.rowNumber).includes(query),
    );
  }, [dbxColumnPreviewFields, dbxColumnPreviewSearch]);
  const dbxPendingCellEditCount = Object.keys(dbxPendingCellEdits).length;

  useEffect(() => {
    setDbxGridHiddenColumns((current) => pruneDbxGridHiddenColumns(current, tableColumns));
  }, [tableColumns]);

  useEffect(() => {
    setDbxGridColumnWidths((current) => pruneDbxGridColumnWidths(current, tableColumns));
  }, [tableColumns]);

  useEffect(() => {
    if (!resizingDbxGridColumn) return undefined;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const handlePointerMove = (event: PointerEvent) => {
      const { column, width, x } = dbxGridColumnResizeStartRef.current;
      const nextWidth = clampDbxGridColumnWidth(width + event.clientX - x);
      setDbxGridColumnWidths((current) =>
        current[column] === nextWidth ? current : { ...current, [column]: nextWidth },
      );
    };
    const handlePointerUp = () => {
      setResizingDbxGridColumn(null);
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [resizingDbxGridColumn]);

  const initializeLoadedGrid = useCallback(
    ({
      sameDbxObject,
      columns,
      rows,
      columnTypes,
      whereInput,
      orderByInput,
    }: InitializeLoadedGridOptions) => {
      setDbxPendingCellEdits({});
      setDbxGridSelectedRows(new Set());
      setDbxGridWhereInput(whereInput);
      setDbxGridOrderByInput(orderByInput);
      if (!sameDbxObject) {
        setDbxGridSearch("");
        setDbxGridColumnSearch("");
        setDbxGridHiddenColumns(new Set());
        setDbxGridColumnWidths(initialDbxGridColumnWidths(columns, rows, columnTypes));
      } else {
        setDbxGridColumnWidths((current) =>
          Object.keys(current).length === 0
            ? initialDbxGridColumnWidths(columns, rows, columnTypes)
            : current,
        );
      }
    },
    [],
  );

  const resetGridPresentation = useCallback(() => {
    setDbxGridWhereInput("");
    setDbxGridOrderByInput("");
    setDbxGridSearch("");
    setDbxGridColumnSearch("");
    setDbxGridHiddenColumns(new Set());
    setDbxGridColumnWidths({});
    setDbxPendingCellEdits({});
  }, []);

  const toggleDbxGridColumnVisibility = useCallback(
    (column: string) => {
      setDbxGridHiddenColumns((current) => toggleDbxGridColumn(current, tableColumns, column));
    },
    [tableColumns],
  );

  const showAllDbxGridColumns = useCallback(() => {
    setDbxGridHiddenColumns(new Set());
  }, []);

  const invertDbxGridColumnVisibility = useCallback(() => {
    setDbxGridHiddenColumns((current) => invertDbxGridColumns(current, tableColumns));
  }, [tableColumns]);

  const startDbxGridColumnResize = useCallback(
    (column: string, event: ReactPointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      dbxGridColumnResizeStartRef.current = {
        column,
        x: event.clientX,
        width: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
      };
      setResizingDbxGridColumn(column);
    },
    [dbxGridColumnWidths],
  );

  const autoFitDbxGridColumn = useCallback(
    (column: string) => {
      const columnIndex = tableColumns.indexOf(column);
      if (columnIndex < 0) return;
      const dbxColumnInfo = activeDbxGridColumnsByName.get(column.toLowerCase());
      const legacyColumnInfo = activeObject?.columns.find(
        (item) => item.name.toLowerCase() === column.toLowerCase(),
      );
      const columnType =
        dbxColumnInfo?.data_type ??
        dbxGridColumnType(queryResult, columnIndex) ??
        legacyColumnInfo?.dataType ??
        "";
      const nextWidth = estimateDbxGridColumnWidth(column, columnIndex, rawTableRows, columnType);
      setDbxGridColumnWidths((current) =>
        current[column] === nextWidth ? current : { ...current, [column]: nextWidth },
      );
      setResizingDbxGridColumn(null);
    },
    [activeDbxGridColumnsByName, activeObject, queryResult, rawTableRows, tableColumns],
  );

  const toggleDbxGridRowSelection = useCallback(
    (rowIndex: number, event?: ReactMouseEvent) => {
      if (rowIndex < 0) return;
      setDbxSelectedCell(null);
      setDbxGridSelectedRows(
        (current) =>
          toggleDbxGridRowSelectionState(
            current,
            rowIndex,
            dbxGridSelectionAnchor,
            queryResult?.rows.length ?? 0,
            Boolean(event?.shiftKey),
          ).selectedRows,
      );
      setDbxGridSelectionAnchor(rowIndex);
    },
    [dbxGridSelectionAnchor, queryResult?.rows.length],
  );

  const stageDbxCellEdit = useCallback(
    (rowIndex: number, columnIndex: number, column: string, value: string, original: string) => {
      setDbxPendingCellEdits((current) =>
        stageDbxPendingCellEdit(current, {
          rowIndex,
          columnIndex,
          column,
          value,
          original,
        }),
      );
    },
    [],
  );

  return {
    state: {
      dbxGridWhereInput,
      setDbxGridWhereInput,
      dbxGridOrderByInput,
      setDbxGridOrderByInput,
      dbxGridSearch,
      setDbxGridSearch,
      dbxGridColumnSearch,
      setDbxGridColumnSearch,
      dbxGridHiddenColumns,
      setDbxGridHiddenColumns,
      dbxDataToolsOpen,
      setDbxDataToolsOpen,
      dbxDataToolsMode,
      setDbxDataToolsMode,
      dbxFieldFilterOpen,
      setDbxFieldFilterOpen,
      dbxGridColumnWidths,
      setDbxGridColumnWidths,
      resizingDbxGridColumn,
      setResizingDbxGridColumn,
      dbxGridPageSize,
      setDbxGridPageSize,
      dbxGridSelectedRows,
      setDbxGridSelectedRows,
      dbxGridExportFormat,
      setDbxGridExportFormat,
      dbxCellPreview,
      setDbxCellPreview,
      dbxCellDetail,
      setDbxCellDetail,
      dbxSelectedCell,
      setDbxSelectedCell,
      dbxEditingCell,
      setDbxEditingCell,
      dbxPendingCellEdits,
      setDbxPendingCellEdits,
      dbxHoveredCell,
      setDbxHoveredCell,
      dbxRowPreview,
      setDbxRowPreview,
      dbxRowPreviewSearch,
      setDbxRowPreviewSearch,
      dbxColumnPreview,
      setDbxColumnPreview,
      dbxColumnPreviewSearch,
      setDbxColumnPreviewSearch,
    },
    derived: {
      visibleTableColumns,
      visibleDbxGridDataColumnsWidth,
      dbxGridTableMinWidth,
      activeDbxGridColumnsByName,
      filteredDbxGridColumnOptions,
      tableRows,
      formattedDbxCellPreview,
      dbxRowPreviewFields,
      filteredDbxRowPreviewFields,
      dbxColumnPreviewFields,
      filteredDbxColumnPreviewFields,
      dbxPendingCellEditCount,
    },
    actions: {
      initializeLoadedGrid,
      resetGridPresentation,
      toggleDbxGridColumnVisibility,
      showAllDbxGridColumns,
      invertDbxGridColumnVisibility,
      startDbxGridColumnResize,
      autoFitDbxGridColumn,
      toggleDbxGridRowSelection,
      stageDbxCellEdit,
    },
  };
}

export type DbxDataGridController = ReturnType<typeof useDbxDataGrid>;
