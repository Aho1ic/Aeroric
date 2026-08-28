/**
 * Markdown 表格 widget。
 *
 * 范围：源码 ↔ 解析后结构 (ParsedTable) ↔ 行可编辑 DOM 互转，以及围绕 widget
 * 的交互（cell 编辑提交、+ 行/列按钮、右键菜单、Tab 切换 cell）。
 *
 * 公共 API（被测试 / table-edit / 其他模块用）：
 * - ParsedTable / WysiwygTableAction
 * - parseTableSource / buildTableSource / applyWysiwygTableAction
 * - buildTableDom / TableWidget
 */

import { EditorView, WidgetType } from "@codemirror/view";

import { type Cleanup, eventElementTarget } from "./util";

export interface ParsedTable {
  header: string[];
  aligns: Array<"left" | "center" | "right" | null>;
  rows: string[][];
}

function tableColumnCount(parsed: ParsedTable): number {
  return Math.max(
    1,
    parsed.header.length,
    parsed.aligns.length,
    ...parsed.rows.map((r) => r.length),
  );
}

function normalizedTable(parsed: ParsedTable): ParsedTable {
  const cols = tableColumnCount(parsed);
  const header = parsed.header.slice();
  while (header.length < cols) header.push("");
  const aligns = parsed.aligns.slice();
  while (aligns.length < cols) aligns.push(null);
  const rows = parsed.rows.map((row) => {
    const next = row.slice();
    while (next.length < cols) next.push("");
    return next;
  });
  return { header, aligns, rows };
}

function buildAlignCell(align: "left" | "center" | "right" | null): string {
  if (align === "left") return ":---";
  if (align === "center") return ":---:";
  if (align === "right") return "---:";
  return "---";
}

function buildMarkdownRow(cells: string[]): string {
  return `| ${cells.map((cell) => cell.trim() || " ").join(" | ")} |`;
}

export function buildTableSource(parsed: ParsedTable): string {
  const table = normalizedTable(parsed);
  return [
    buildMarkdownRow(table.header),
    buildMarkdownRow(table.aligns.map(buildAlignCell)),
    ...table.rows.map(buildMarkdownRow),
  ].join("\n");
}

export type WysiwygTableAction = "insertRowBelow" | "insertColRight" | "deleteRow" | "deleteCol";

export function applyWysiwygTableAction(
  parsed: ParsedTable,
  row: number,
  col: number,
  action: WysiwygTableAction,
): ParsedTable {
  const table = normalizedTable(parsed);
  const cols = tableColumnCount(table);
  const safeCol = Math.max(0, Math.min(cols - 1, col));
  const safeRow = Math.max(0, Math.min(table.rows.length, row));
  if (action === "insertRowBelow") {
    const insertAt = safeRow <= 0 ? 0 : Math.min(table.rows.length, safeRow);
    table.rows.splice(insertAt, 0, Array(cols).fill(""));
  } else if (action === "insertColRight") {
    const insertAt = safeCol + 1;
    table.header.splice(insertAt, 0, "");
    table.aligns.splice(insertAt, 0, null);
    for (const bodyRow of table.rows) bodyRow.splice(insertAt, 0, "");
  } else if (action === "deleteRow") {
    if (safeRow > 0 && table.rows.length > 0) table.rows.splice(safeRow - 1, 1);
  } else if (action === "deleteCol") {
    if (cols > 1) {
      table.header.splice(safeCol, 1);
      table.aligns.splice(safeCol, 1);
      for (const bodyRow of table.rows) bodyRow.splice(safeCol, 1);
    }
  }
  return normalizedTable(table);
}

export function parseTableSource(src: string): ParsedTable {
  const lines = src.split(/\r?\n/).filter((l) => l.trim().startsWith("|"));
  if (lines.length < 2) return { header: [], aligns: [], rows: [] };
  const splitRow = (line: string): string[] => {
    const inner = line.trim().replace(/^\|/, "").replace(/\|$/, "");
    return inner.split("|").map((s) => s.trim());
  };
  const header = splitRow(lines[0]!);
  const alignRow = splitRow(lines[1]!);
  const aligns = alignRow.map((s) => {
    const left = s.startsWith(":");
    const right = s.endsWith(":");
    if (left && right) return "center" as const;
    if (left) return "left" as const;
    if (right) return "right" as const;
    return null;
  });
  const rows = lines.slice(2).map(splitRow);
  return normalizedTable({ header, aligns, rows });
}

function createTableCellEditor(value: string, row: number, col: number, label: string) {
  const editor = document.createElement("textarea");
  editor.className = "cm-md-table-cell";
  editor.spellcheck = false;
  editor.rows = 1;
  editor.value = value;
  editor.dataset.row = String(row);
  editor.dataset.col = String(col);
  editor.setAttribute("aria-label", label);
  return editor;
}

export function buildTableDom(parsed: ParsedTable): HTMLElement {
  const table = normalizedTable(parsed);
  const root = document.createElement("div");
  root.className = "cm-md-table-widget";
  root.setAttribute("contenteditable", "false");
  root.dataset.activeRow = "1";
  root.dataset.activeCol = "0";
  root.dataset.rowCount = String(table.rows.length);
  root.dataset.colCount = String(table.header.length);

  const tbl = document.createElement("table");

  const thead = document.createElement("thead");
  const trh = document.createElement("tr");
  table.header.forEach((cell, col) => {
    const th = document.createElement("th");
    const editor = createTableCellEditor(cell, 0, col, `表头 ${col + 1}`);
    th.dataset.row = "0";
    th.dataset.col = String(col);
    const align = table.aligns[col];
    if (align) th.style.textAlign = align;
    th.appendChild(editor);
    trh.appendChild(th);
  });
  thead.appendChild(trh);
  tbl.appendChild(thead);

  const tbody = document.createElement("tbody");
  table.rows.forEach((row, rowIdx) => {
    const tr = document.createElement("tr");
    row.forEach((cell, col) => {
      const td = document.createElement("td");
      const editor = createTableCellEditor(
        cell,
        rowIdx + 1,
        col,
        `第 ${rowIdx + 1} 行第 ${col + 1} 列`,
      );
      td.dataset.row = String(rowIdx + 1);
      td.dataset.col = String(col);
      const align = table.aligns[col];
      if (align) td.style.textAlign = align;
      td.appendChild(editor);
      tr.appendChild(td);
    });
    tbody.appendChild(tr);
  });
  tbl.appendChild(tbody);

  root.appendChild(tbl);

  const addCol = document.createElement("button");
  addCol.type = "button";
  addCol.className = "cm-md-table-edge-action cm-md-table-add-col";
  addCol.dataset.action = "insertColRight";
  addCol.dataset.edge = "col-end";
  addCol.textContent = "+";
  addCol.title = "在末尾新增列";
  addCol.setAttribute("aria-label", "在末尾新增列");
  root.appendChild(addCol);

  const addRow = document.createElement("button");
  addRow.type = "button";
  addRow.className = "cm-md-table-edge-action cm-md-table-add-row";
  addRow.dataset.action = "insertRowBelow";
  addRow.dataset.edge = "row-end";
  addRow.textContent = "+";
  addRow.title = "在末尾新增行";
  addRow.setAttribute("aria-label", "在末尾新增行");
  root.appendChild(addRow);

  const menu = document.createElement("div");
  menu.className = "cm-md-table-menu";
  menu.hidden = true;
  root.appendChild(menu);
  return root;
}

export class TableWidget extends WidgetType {
  /** 当前 DOM 上的 listener 拆除函数；destroy 时调用，避免大文档累积 widget 时
   *  table host 上 9 个 listener 闭包持有 view / pointerDown 等状态。 */
  private cleanup: Cleanup | null = null;
  constructor(private readonly source: string) {
    super();
  }
  /** 块高估计：每个表行约 36px + 边距。让 off-screen 表格在高度图里占对位置，
   *  减少快速滚动错位空白；上屏后用实测高。 */
  get estimatedHeight(): number {
    const rows = this.source.split("\n").filter((l) => l.trim().length > 0).length;
    return Math.max(2, rows) * 36 + 16;
  }
  eq(other: WidgetType): boolean {
    return other instanceof TableWidget && other.source === this.source;
  }
  toDOM(view: EditorView): HTMLElement {
    const dom = buildTableDom(parseTableSource(this.source));
    dom.dataset.sourceLength = String(this.source.length);
    this.cleanup = installTableDomHandlers(view, dom);
    // attach 进文档、完成布局后量一次每个 cell 的 scrollHeight，否则多行内容
    // 在未聚焦时被 textarea 的 rows=1 + overflow:hidden 裁掉。
    requestAnimationFrame(() => {
      if (dom.isConnected) resizeAllTableCells(dom);
    });
    return dom;
  }
  ignoreEvent() {
    return true;
  }
  destroy() {
    this.cleanup?.();
    this.cleanup = null;
  }
}

function tableRangeFromHost(
  view: EditorView,
  host: HTMLElement,
): { from: number; to: number; source: string } | null {
  const from = view.posAtDOM(host);
  const len = Number(host.dataset.sourceLength);
  if (from == null || !Number.isFinite(len) || len <= 0) return null;
  const to = Math.min(view.state.doc.length, from + len);
  if (to <= from) return null;
  return { from, to, source: view.state.doc.sliceString(from, to) };
}

function activeTableCell(host: HTMLElement): HTMLElement | null {
  const row = host.dataset.activeRow ?? "1";
  const col = host.dataset.activeCol ?? "0";
  const cells = Array.from(host.querySelectorAll<HTMLElement>(".cm-md-table-cell"));
  return (
    cells.find((cell) => cell.dataset.row === row && cell.dataset.col === col) ?? cells[0] ?? null
  );
}

function setActiveTableCell(host: HTMLElement, cell: HTMLElement) {
  host.dataset.activeRow = cell.dataset.row ?? "1";
  host.dataset.activeCol = cell.dataset.col ?? "0";
}

function tableCellText(cell: HTMLElement): string {
  if (cell instanceof HTMLTextAreaElement) return cell.value.replace(/\r?\n/g, " ").trim();
  return (cell.textContent ?? "").replace(/\r?\n/g, " ").trim();
}

function updateParsedTableCell(
  parsed: ParsedTable,
  row: number,
  col: number,
  value: string,
): ParsedTable {
  const table = normalizedTable(parsed);
  const cols = tableColumnCount(table);
  const safeCol = Math.max(0, Math.min(cols - 1, col));
  if (row <= 0) {
    table.header[safeCol] = value;
    return normalizedTable(table);
  }
  while (table.rows.length < row) table.rows.push(Array(cols).fill(""));
  table.rows[row - 1]![safeCol] = value;
  return normalizedTable(table);
}

function commitTableCellEdit(view: EditorView, cell: HTMLElement): boolean {
  const host = cell.closest<HTMLElement>(".cm-md-table-widget");
  if (!host) return false;
  const range = tableRangeFromHost(view, host);
  if (!range) return false;
  const row = Number(cell.dataset.row);
  const col = Number(cell.dataset.col);
  if (!Number.isFinite(row) || !Number.isFinite(col)) return false;
  const parsed = parseTableSource(range.source);
  const next = buildTableSource(updateParsedTableCell(parsed, row, col, tableCellText(cell)));
  if (next === range.source) return false;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert: next },
    userEvent: "input",
  });
  return true;
}

function selectElementContents(el: HTMLElement) {
  // 单元格是 <textarea>：Range.selectNodeContents 对 textarea 无效（其文本不是子节点），
  // 必须用 textarea.select()，否则 Tab 切到单元格时根本没选中内容。
  if (el instanceof HTMLTextAreaElement) {
    el.select();
    return;
  }
  const range = el.ownerDocument.createRange();
  range.selectNodeContents(el);
  const selection = el.ownerDocument.getSelection?.() ?? window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

/** 在（可能刚被 dispatch 重建过的）DOM 里按文档位置找到表格 widget host。 */
function findTableHostAtPos(view: EditorView, fromPos: number): HTMLElement | null {
  const hosts = view.dom.querySelectorAll<HTMLElement>(".cm-md-table-widget");
  for (const h of hosts) {
    const r = tableRangeFromHost(view, h);
    if (r && r.from === fromPos) return h;
  }
  return null;
}

/** 在指定 host 内按 row/col 聚焦单元格并选中内容。 */
function focusTableCellByRowCol(host: HTMLElement, row: string, col: string) {
  const next = host.querySelector<HTMLElement>(
    `.cm-md-table-cell[data-row="${row}"][data-col="${col}"]`,
  );
  if (!next) return;
  host.dataset.activeRow = row;
  host.dataset.activeCol = col;
  focusTableCell(next);
  selectElementContents(next);
}

type CaretDocument = Document & {
  caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
  caretRangeFromPoint?: (x: number, y: number) => Range | null;
};

function focusTableCell(cell: HTMLElement, event?: MouseEvent) {
  if (cell instanceof HTMLTextAreaElement) {
    cell.focus({ preventScroll: true });
    void event;
    return;
  }
  const doc = cell.ownerDocument as CaretDocument;
  cell.focus({ preventScroll: true });
  const selection = doc.getSelection?.() ?? window.getSelection();
  if (!selection) return;

  let range: Range | null = null;
  if (event) {
    const caret = doc.caretPositionFromPoint?.(event.clientX, event.clientY);
    if (caret && cell.contains(caret.offsetNode)) {
      range = doc.createRange();
      range.setStart(caret.offsetNode, caret.offset);
      range.collapse(true);
    } else {
      const fallbackRange = doc.caretRangeFromPoint?.(event.clientX, event.clientY);
      if (fallbackRange && cell.contains(fallbackRange.startContainer)) {
        range = fallbackRange;
      }
    }
  }

  if (!range) {
    range = doc.createRange();
    range.selectNodeContents(cell);
    range.collapse(false);
  }

  selection.removeAllRanges();
  selection.addRange(range);
}

function hasSelectionInsideTableCell(cell: HTMLElement): boolean {
  if (cell instanceof HTMLTextAreaElement) {
    return cell.selectionStart !== cell.selectionEnd;
  }
  const selection = cell.ownerDocument.getSelection?.() ?? window.getSelection();
  if (!selection || selection.isCollapsed) return false;
  const { anchorNode, focusNode } = selection;
  return !!anchorNode && !!focusNode && cell.contains(anchorNode) && cell.contains(focusNode);
}

function resizeTableCellEditor(cell: HTMLElement) {
  if (!(cell instanceof HTMLTextAreaElement)) return;
  cell.style.height = "auto";
  cell.style.height = `${Math.max(24, cell.scrollHeight)}px`;
}

/** 把 host 内所有 cell 按内容撑高。textarea 初始 rows=1 + overflow:hidden，
 *  未聚焦时多行内容会被裁掉，需要 attach 进 DOM 后量一次 scrollHeight。 */
function resizeAllTableCells(host: HTMLElement) {
  const cells = host.querySelectorAll<HTMLTextAreaElement>("textarea.cm-md-table-cell");
  cells.forEach((cell) => resizeTableCellEditor(cell));
}

function applyTableWidgetAction(
  view: EditorView,
  host: HTMLElement,
  row: number,
  col: number,
  action: WysiwygTableAction,
  pendingCell?: HTMLElement | null,
): boolean {
  const range = tableRangeFromHost(view, host);
  if (!range) return false;
  let parsed = parseTableSource(range.source);
  if (pendingCell) {
    const pendingRow = Number(pendingCell.dataset.row);
    const pendingCol = Number(pendingCell.dataset.col);
    if (Number.isFinite(pendingRow) && Number.isFinite(pendingCol)) {
      parsed = updateParsedTableCell(parsed, pendingRow, pendingCol, tableCellText(pendingCell));
    }
  }
  const next = buildTableSource(applyWysiwygTableAction(parsed, row, col, action));
  if (next === range.source) return false;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert: next },
    userEvent: "input",
  });
  return true;
}

function hideTableMenu(host: HTMLElement) {
  const menu = host.querySelector<HTMLElement>(".cm-md-table-menu");
  if (menu) menu.hidden = true;
}

function tableActionCoordsFromButton(host: HTMLElement, button: HTMLElement) {
  const active = activeTableCell(host);
  let row = Number(host.dataset.activeRow ?? active?.dataset.row ?? 1);
  let col = Number(host.dataset.activeCol ?? active?.dataset.col ?? 0);
  if (button.dataset.row != null) row = Number(button.dataset.row);
  if (button.dataset.col != null) col = Number(button.dataset.col);
  if (button.dataset.edge === "col-end") {
    col = Math.max(0, Number(host.dataset.colCount ?? 1) - 1);
  }
  if (button.dataset.edge === "row-end") {
    row = Math.max(0, Number(host.dataset.rowCount ?? 0));
  }
  const menu = button.closest<HTMLElement>(".cm-md-table-menu");
  if (menu) {
    row = Number(menu.dataset.row ?? row);
    col = Number(menu.dataset.col ?? col);
  }
  return { row, col, active };
}

/** 删除整张表：把 host 对应的源码区间连同其后多余的一个换行一起删掉。 */
function deleteWholeTable(view: EditorView, host: HTMLElement): boolean {
  const range = tableRangeFromHost(view, host);
  if (!range) return false;
  const doc = view.state.doc;
  let to = range.to;
  // 收掉块尾的换行，避免删完留下一行空行
  if (to < doc.length && doc.sliceString(to, to + 1) === "\n") to += 1;
  view.dispatch({
    changes: { from: range.from, to, insert: "" },
    selection: { anchor: range.from },
    userEvent: "delete",
  });
  view.focus();
  return true;
}

function runTableButtonAction(view: EditorView, host: HTMLElement, button: HTMLElement): boolean {
  const action = button.dataset.action;
  if (!action) return false;
  if (action === "deleteTable") {
    const ok = deleteWholeTable(view, host);
    hideTableMenu(host);
    return ok;
  }
  const { row, col, active } = tableActionCoordsFromButton(host, button);
  const ok = applyTableWidgetAction(view, host, row, col, action as WysiwygTableAction, active);
  hideTableMenu(host);
  return ok;
}

function appendTableMenuButton(
  menu: HTMLElement,
  action: WysiwygTableAction | "deleteTable",
  label: string,
  row: number,
  col: number,
  danger = false,
) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "cm-md-table-menu-item" + (danger ? " danger" : "");
  button.dataset.action = action;
  button.dataset.row = String(row);
  button.dataset.col = String(col);
  button.textContent = label;
  menu.appendChild(button);
}

function showTableMenu(host: HTMLElement, cell: HTMLElement, event: MouseEvent) {
  const menu = host.querySelector<HTMLElement>(".cm-md-table-menu");
  if (!menu) return;
  const row = Number(cell.dataset.row);
  const col = Number(cell.dataset.col);
  if (!Number.isFinite(row) || !Number.isFinite(col)) return;
  menu.replaceChildren();
  menu.dataset.row = String(row);
  menu.dataset.col = String(col);
  if (row <= 0) {
    appendTableMenuButton(menu, "insertColRight", "右侧插入列", row, col);
    appendTableMenuButton(menu, "deleteCol", "删除列", row, col);
  } else {
    appendTableMenuButton(menu, "insertRowBelow", "下方插入行", row, col);
    appendTableMenuButton(menu, "deleteRow", "删除行", row, col);
    appendTableMenuButton(menu, "insertColRight", "右侧插入列", row, col);
    appendTableMenuButton(menu, "deleteCol", "删除列", row, col);
  }
  appendTableMenuButton(menu, "deleteTable", "删除表格", row, col, true);
  menu.style.left = `${event.clientX}px`;
  menu.style.top = `${event.clientY}px`;
  menu.hidden = false;
}

function installTableDomHandlers(view: EditorView, host: HTMLElement): Cleanup {
  const cleanups: Cleanup[] = [];
  const on = <K extends keyof HTMLElementEventMap>(
    event: K,
    handler: (e: HTMLElementEventMap[K]) => void,
  ) => {
    host.addEventListener(event, handler);
    cleanups.push(() => host.removeEventListener(event, handler));
  };

  // 右键菜单的全局关闭：点菜单外 / Esc / 滚动都收起，避免 position:fixed 的菜单
  // 悬浮在无关内容上方。
  const onDocPointerDown = (e: PointerEvent) => {
    const menu = host.querySelector<HTMLElement>(".cm-md-table-menu");
    if (!menu || menu.hidden) return;
    if (e.target instanceof Node && menu.contains(e.target)) return;
    hideTableMenu(host);
  };
  const onDocKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") hideTableMenu(host);
  };
  const onDocScroll = () => hideTableMenu(host);
  document.addEventListener("pointerdown", onDocPointerDown, true);
  document.addEventListener("keydown", onDocKey, true);
  window.addEventListener("scroll", onDocScroll, true);
  cleanups.push(() => {
    document.removeEventListener("pointerdown", onDocPointerDown, true);
    document.removeEventListener("keydown", onDocKey, true);
    window.removeEventListener("scroll", onDocScroll, true);
  });

  let pointerDown: {
    cell: HTMLElement;
    x: number;
    y: number;
  } | null = null;
  let suppressNextCellClick = false;

  on("focusin", (event) => {
    const target = eventElementTarget(event);
    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (!cell || !host.contains(cell)) return;
    setActiveTableCell(host, cell);
    resizeTableCellEditor(cell);
    event.stopPropagation();
  });

  on("focusout", (event) => {
    const target = eventElementTarget(event);
    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (!cell || !host.contains(cell)) return;
    commitTableCellEdit(view, cell);
    event.stopPropagation();
  });

  on("keydown", (event) => {
    const target = eventElementTarget(event);
    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (!cell || !host.contains(cell)) return;
    // IME 组字中按 Enter/Tab 是在确认候选词，不能当作提交单元格 + 失焦，
    // 否则中文用户每次转换确认都会被踢出单元格。
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      commitTableCellEdit(view, cell);
      cell.blur();
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      event.stopPropagation();
      // 先在「提交前」的活动 DOM 里算出目标单元格的 row/col 与表格文档位置，
      // 因为 commit 会 dispatch 改文档，导致整个 widget DOM 被替换、旧节点失效。
      const cells = Array.from(host.querySelectorAll<HTMLElement>(".cm-md-table-cell"));
      const curIdx = cells.indexOf(cell);
      const target = cells[curIdx + (event.shiftKey ? -1 : 1)];
      const range = tableRangeFromHost(view, host);
      const targetRow = target?.dataset.row;
      const targetCol = target?.dataset.col;
      const dispatched = commitTableCellEdit(view, cell);
      if (!target || targetRow == null || targetCol == null) return;
      if (!dispatched) {
        // 内容未变，DOM 未重建：直接在原 host 聚焦
        focusTableCellByRowCol(host, targetRow, targetCol);
      } else if (range) {
        // 已重建：下一帧按文档位置找回新 host 再聚焦目标单元格
        requestAnimationFrame(() => {
          const fresh = findTableHostAtPos(view, range.from);
          if (fresh) focusTableCellByRowCol(fresh, targetRow, targetCol);
        });
      }
    }
  });

  on("mousedown", (event) => {
    const target = eventElementTarget(event);
    const tool = target?.closest<HTMLButtonElement>(
      ".cm-md-table-edge-action[data-action], .cm-md-table-menu-item[data-action]",
    );
    if (tool && host.contains(tool)) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }

    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (cell && host.contains(cell)) {
      setActiveTableCell(host, cell);
      hideTableMenu(host);
      pointerDown = { cell, x: event.clientX, y: event.clientY };
      event.stopPropagation();
    }
  });

  on("mouseup", (event) => {
    const target = eventElementTarget(event);
    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (!cell || !host.contains(cell)) return;
    const moved =
      !pointerDown ||
      pointerDown.cell !== cell ||
      Math.abs(pointerDown.x - event.clientX) > 4 ||
      Math.abs(pointerDown.y - event.clientY) > 4;
    suppressNextCellClick = moved || hasSelectionInsideTableCell(cell);
    if (!suppressNextCellClick) {
      focusTableCell(cell, event);
    }
    pointerDown = null;
    event.stopPropagation();
  });

  on("click", (event) => {
    const target = eventElementTarget(event);
    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (!cell || !host.contains(cell)) return;
    setActiveTableCell(host, cell);
    if (suppressNextCellClick) {
      suppressNextCellClick = false;
    } else if (cell.ownerDocument.activeElement !== cell) {
      focusTableCell(cell, event);
    }
    event.stopPropagation();
  });

  on("click", (event) => {
    const target = eventElementTarget(event);
    const tool = target?.closest<HTMLButtonElement>(
      ".cm-md-table-edge-action[data-action], .cm-md-table-menu-item[data-action]",
    );
    if (!tool || !host.contains(tool)) return;
    runTableButtonAction(view, host, tool);
    event.preventDefault();
    event.stopPropagation();
  });

  on("contextmenu", (event) => {
    const target = eventElementTarget(event);
    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (!cell || !host.contains(cell)) return;
    setActiveTableCell(host, cell);
    showTableMenu(host, cell, event);
    event.preventDefault();
    event.stopPropagation();
  });

  on("input", (event) => {
    const target = eventElementTarget(event);
    const cell = target?.closest<HTMLElement>(".cm-md-table-cell");
    if (!cell || !host.contains(cell)) return;
    resizeTableCellEditor(cell);
    event.stopPropagation();
  });

  return () => {
    for (const fn of cleanups) fn();
    cleanups.length = 0;
  };
}
