import { describe, expect, it } from "vitest";
import {
  combineDbxGridWhereCondition,
  dbxFilterModeForCellAction,
  dbxGridContextRowIndexes,
  dbxOrderByForColumn,
  dbxPendingCellEditsToDirtyRows,
  filterDbxGridColumnOptions,
  filterDbxGridRows,
  invertDbxGridColumns,
  nextDbxOrderByForColumn,
  pruneDbxGridColumnWidths,
  pruneDbxGridHiddenColumns,
  stageDbxPendingCellEdit,
  toggleDbxGridColumn,
  toggleDbxGridRowSelection,
  visibleDbxGridColumns,
  type DatabaseRow,
} from "../components/database/databaseGridState";

const rows: DatabaseRow[] = [
  { rowId: null, keyValues: [], values: [1, "Alpha"] },
  { rowId: null, keyValues: [], values: [2, "Beta"] },
];

describe("databaseGridState", () => {
  it("cycles a quoted column through ascending, descending, and cleared sorting", () => {
    expect(nextDbxOrderByForColumn("", "created at")).toBe('"created at" ASC');
    expect(nextDbxOrderByForColumn('"created at" ASC', "created at")).toBe('"created at" DESC');
    expect(nextDbxOrderByForColumn('"created at" desc', "created at")).toBe("");
    expect(dbxOrderByForColumn("created at", "DESC")).toBe('"created at" DESC');
    expect(dbxOrderByForColumn("created at", null)).toBe("");
  });

  it("maps context filter actions and combines conditions", () => {
    expect(dbxFilterModeForCellAction("filterNotLike")).toBe("not-like");
    expect(dbxFilterModeForCellAction("copyValue")).toBeNull();
    expect(combineDbxGridWhereCondition("", '"id" = 1')).toBe('"id" = 1');
    expect(combineDbxGridWhereCondition("active = 1", '"id" = 1')).toBe(
      '(active = 1) AND ("id" = 1)',
    );
  });

  it("derives visible columns and current-page search results", () => {
    expect(visibleDbxGridColumns(["id", "name"], new Set(["id"]))).toEqual([
      { column: "name", index: 1 },
    ]);
    expect(filterDbxGridRows(rows, "bet")).toEqual([rows[1]]);
    expect(filterDbxGridRows(rows, "")).toBe(rows);
    expect(filterDbxGridColumnOptions(["id", "display_name"], "name")).toEqual(["display_name"]);
  });

  it("prunes unavailable hidden columns and widths without changing stable values", () => {
    const hidden = new Set(["missing"]);
    expect(pruneDbxGridHiddenColumns(hidden, ["id"])).toEqual(new Set());
    const stableHidden = new Set(["id"]);
    expect(pruneDbxGridHiddenColumns(stableHidden, ["id", "name"])).toBe(stableHidden);

    const widths = { id: 90, missing: 120 };
    expect(pruneDbxGridColumnWidths(widths, ["id"])).toEqual({ id: 90 });
    const stableWidths = { id: 90 };
    expect(pruneDbxGridColumnWidths(stableWidths, ["id"])).toBe(stableWidths);
  });

  it("keeps at least one column visible when toggling or inverting", () => {
    const columns = ["id", "name"];
    expect(toggleDbxGridColumn(new Set(), columns, "id")).toEqual(new Set(["id"]));
    expect(toggleDbxGridColumn(new Set(["id"]), columns, "name")).toEqual(new Set(["id"]));
    expect(toggleDbxGridColumn(new Set(["id"]), columns, "id")).toEqual(new Set());
    expect(invertDbxGridColumns(new Set(), columns)).toEqual(new Set(["name"]));
    expect(invertDbxGridColumns(new Set(["id"]), columns)).toEqual(new Set(["name"]));
  });

  it("toggles rows and creates contiguous shift selections", () => {
    expect(toggleDbxGridRowSelection(new Set(), 2, null, 5, false)).toEqual({
      selectedRows: new Set([2]),
      selectionAnchor: 2,
    });
    expect(toggleDbxGridRowSelection(new Set([2]), 2, 2, 5, false)).toEqual({
      selectedRows: new Set(),
      selectionAnchor: 2,
    });
    expect(toggleDbxGridRowSelection(new Set([1]), 4, 1, 5, true)).toEqual({
      selectedRows: new Set([1, 2, 3, 4]),
      selectionAnchor: 4,
    });
  });

  it("stages, overwrites, and removes pending cell edits", () => {
    const staged = stageDbxPendingCellEdit(
      {},
      { rowIndex: 1, columnIndex: 2, column: "name", value: "next", original: "old" },
    );
    expect(staged["1:2"]?.value).toBe("next");
    const overwritten = stageDbxPendingCellEdit(staged, {
      rowIndex: 1,
      columnIndex: 2,
      column: "name",
      value: "newer",
      original: "old",
    });
    expect(overwritten["1:2"]?.value).toBe("newer");
    expect(
      stageDbxPendingCellEdit(overwritten, {
        rowIndex: 1,
        columnIndex: 2,
        column: "name",
        value: "old",
        original: "old",
      }),
    ).toEqual({});
  });

  it("groups pending edits by row and resolves selected context rows", () => {
    const dirtyRows = dbxPendingCellEditsToDirtyRows(
      {
        "2:1": {
          rowIndex: 2,
          columnIndex: 1,
          column: "name",
          value: "Alpha",
          original: "",
        },
        "2:3": {
          rowIndex: 2,
          columnIndex: 3,
          column: "active",
          value: "true",
          original: "false",
        },
        "4:0": {
          rowIndex: 4,
          columnIndex: 0,
          column: "id",
          value: "9",
          original: "8",
        },
      },
      (value) => `converted:${value}`,
    );
    expect(dirtyRows).toEqual([
      [
        2,
        [
          [1, "converted:Alpha"],
          [3, "converted:true"],
        ],
      ],
      [4, [[0, "converted:9"]]],
    ]);
    expect(dbxGridContextRowIndexes(new Set([3, 1]), 3)).toEqual([1, 3]);
    expect(dbxGridContextRowIndexes(new Set([3, 1]), 2)).toEqual([2]);
  });
});
