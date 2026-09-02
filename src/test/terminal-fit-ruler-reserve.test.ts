import { afterEach, describe, expect, it, vi } from "vitest";
import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";
import { safeFit } from "../components/terminalShared";

/**
 * FitAddon 0.11.0 只要 `scrollback !== 0` 就无条件扣 14px 给 overview ruler：
 *   `scrollback === 0 ? 0 : (options.overviewRuler?.width || 14)`
 * 我们从不启用 overview ruler，而 `|| 14` 会把 `width: 0` 也当成假值，所以改选项无效。
 * 这 14px 在 12px 等宽字体下约合 2 列 —— 终端右侧白留一条，diff 底色的右边界跟着内缩。
 *
 * 这些用例锁住"补回那 2 列，且只在确实更宽时才动"。
 */

afterEach(() => vi.restoreAllMocks());

const CELL_WIDTH = 7;

/**
 * 造一个能被 `terminalCellWidth` / `proposeColsWithoutRulerReserve` 读到的终端。
 *
 * `_core._renderService.dimensions.css.cell` 是 xterm 的内部结构，项目里
 * terminalCopyHelper / terminalInputFix 已经走同样的路子读 `_core`。
 */
function makeTerm(options: {
  cols: number;
  rows: number;
  parentWidth: number;
  padding?: number;
  cellWidth?: number | null;
}): { term: Terminal; resize: ReturnType<typeof vi.fn> } {
  const padding = options.padding ?? 0;
  const parent = document.createElement("div");
  const element = document.createElement("div");
  parent.style.width = `${options.parentWidth}px`;
  element.style.paddingLeft = `${padding}px`;
  element.style.paddingRight = `${padding}px`;
  parent.appendChild(element);
  document.body.appendChild(parent);

  const resize = vi.fn();
  const cellWidth = options.cellWidth === undefined ? CELL_WIDTH : options.cellWidth;
  const term = {
    cols: options.cols,
    rows: options.rows,
    element,
    resize: resize.mockImplementation((cols: number, rows: number) => {
      (term as { cols: number }).cols = cols;
      (term as { rows: number }).rows = rows;
    }),
    _core:
      cellWidth === null
        ? {}
        : { _renderService: { dimensions: { css: { cell: { width: cellWidth, height: 17 } } } } },
  } as unknown as Terminal;
  return { term, resize };
}

function makeFitAddon(cols: number, rows: number): FitAddon {
  return {
    proposeDimensions: vi.fn(() => ({ cols, rows })),
    fit: vi.fn(),
  } as unknown as FitAddon;
}

function visibleContainer(): HTMLElement {
  const container = document.createElement("div");
  vi.spyOn(container, "getBoundingClientRect").mockReturnValue({
    width: 1600,
    height: 480,
  } as DOMRect);
  return container;
}

describe("safeFit reclaims the overview ruler reserve", () => {
  it("widens past FitAddon's proposal by the reserved columns", () => {
    // 可用宽 1603px / 7px 每格 = 229 列。FitAddon 会先扣 14px → floor(1589/7) = 227。
    const { term, resize } = makeTerm({ cols: 227, rows: 28, parentWidth: 1603 });
    const fitAddon = makeFitAddon(227, 28);

    expect(safeFit(fitAddon, term, visibleContainer())).toEqual({ cols: 229, rows: 28 });
    expect(resize).toHaveBeenCalledWith(229, 28);
  });

  it("leaves FitAddon's fit and row count untouched", () => {
    const { term } = makeTerm({ cols: 227, rows: 28, parentWidth: 1603 });
    const fitAddon = makeFitAddon(227, 28);

    const size = safeFit(fitAddon, term, visibleContainer());

    // fit() 仍要跑：renderer clear 只有它做得了，行数也由它落地。
    expect(fitAddon.fit).toHaveBeenCalledOnce();
    expect(size?.rows).toBe(28);
  });

  it("subtracts the element's own horizontal padding", () => {
    // 父元素 1600px，xterm 元素左右各 16px padding → 可用 1568px / 7 = 224 列。
    const { term } = makeTerm({ cols: 222, rows: 28, parentWidth: 1600, padding: 16 });
    const fitAddon = makeFitAddon(222, 28);

    expect(safeFit(fitAddon, term, visibleContainer())?.cols).toBe(224);
  });

  it("does not narrow the terminal when its own math comes out smaller", () => {
    // 我们算出来更窄（口径与 FitAddon 有出入）时以 FitAddon 为准，不动列数。
    const { term, resize } = makeTerm({ cols: 240, rows: 28, parentWidth: 1603 });
    const fitAddon = makeFitAddon(240, 28);

    expect(safeFit(fitAddon, term, visibleContainer())).toEqual({ cols: 240, rows: 28 });
    expect(resize).not.toHaveBeenCalled();
  });

  it("falls back to FitAddon when the cell width is unavailable", () => {
    // renderer 还没建好（或上游改了内部结构）→ 读不到 cell.width，退回 FitAddon 的结果。
    const { term, resize } = makeTerm({
      cols: 227,
      rows: 28,
      parentWidth: 1603,
      cellWidth: null,
    });
    const fitAddon = makeFitAddon(227, 28);

    expect(safeFit(fitAddon, term, visibleContainer())).toEqual({ cols: 227, rows: 28 });
    expect(resize).not.toHaveBeenCalled();
  });

  it("still refuses to fit a zero-sized container", () => {
    // 原有防线不能被这条改动削弱：0 尺寸容器上 FitAddon 会退化到 cols=2，
    // 放过去就是 SIGWINCH 把 TUI 排成一字一行。
    const { term } = makeTerm({ cols: 227, rows: 28, parentWidth: 1603 });
    const fitAddon = makeFitAddon(227, 28);
    const container = document.createElement("div");
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue({
      width: 0,
      height: 0,
    } as DOMRect);

    expect(safeFit(fitAddon, term, container)).toBeNull();
    expect(fitAddon.fit).not.toHaveBeenCalled();
  });

  it("still refuses a degenerate proposal", () => {
    const { term } = makeTerm({ cols: 2, rows: 1, parentWidth: 1603 });
    const fitAddon = makeFitAddon(2, 1);

    expect(safeFit(fitAddon, term, visibleContainer())).toBeNull();
    expect(fitAddon.fit).not.toHaveBeenCalled();
  });
});
