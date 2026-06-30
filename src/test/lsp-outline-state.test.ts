import { describe, expect, it } from "vitest";
import {
  activeSymbolBreadcrumbs,
  fileBreadcrumbSegments,
  lspSymbolToSelection,
  normalizeDocumentSymbols,
  outlineSymbolDepth,
} from "../components/file-viewer/lspOutline";
import type { LspSymbol } from "../types";

function symbol(overrides: Partial<LspSymbol> & Pick<LspSymbol, "name">): LspSymbol {
  return {
    name: overrides.name,
    kind: overrides.kind ?? 12,
    detail: overrides.detail ?? null,
    containerName: overrides.containerName ?? null,
    uri: overrides.uri ?? "file:///repo/src/App.tsx",
    path: overrides.path ?? "/repo/src/App.tsx",
    range: overrides.range ?? {
      start: { line: 0, character: 0 },
      end: { line: 10, character: 1 },
    },
    selectionRange: overrides.selectionRange ?? {
      start: { line: 0, character: 9 },
      end: { line: 0, character: 12 },
    },
  };
}

describe("lsp outline state", () => {
  it("limits document symbols and drops invalid entries", () => {
    const items = Array.from({ length: 4 }, (_, index) => symbol({ name: `symbol${index}` }));
    const outline = normalizeDocumentSymbols([symbol({ name: "" }), ...items], 3);

    expect(outline.truncated).toBe(true);
    expect(outline.symbols.map((item) => item.name)).toEqual(["symbol0", "symbol1", "symbol2"]);
  });

  it("builds active symbol breadcrumbs from outer to inner ranges", () => {
    const app = symbol({
      name: "App",
      range: { start: { line: 0, character: 0 }, end: { line: 8, character: 1 } },
      selectionRange: { start: { line: 0, character: 9 }, end: { line: 0, character: 12 } },
    });
    const render = symbol({
      name: "render",
      containerName: "App",
      range: { start: { line: 2, character: 2 }, end: { line: 5, character: 3 } },
      selectionRange: { start: { line: 2, character: 11 }, end: { line: 2, character: 17 } },
    });

    expect(activeSymbolBreadcrumbs([render, app], { line: 4, column: 8 }).map((item) => item.name))
      .toEqual(["App", "render"]);
    expect(outlineSymbolDepth(render, [app, render])).toBe(2);
  });

  it("converts symbols and file paths into UI navigation targets", () => {
    expect(
      lspSymbolToSelection(
        symbol({
          name: "helper",
          selectionRange: { start: { line: 4, character: 6 }, end: { line: 4, character: 12 } },
        }),
      ),
    ).toEqual({ line: 5, column: 7 });

    expect(fileBreadcrumbSegments("/repo", "/repo/src/components/App.tsx")).toEqual([
      { label: "src", title: "src" },
      { label: "components", title: "src/components" },
      { label: "App.tsx", title: "src/components/App.tsx" },
    ]);
  });
});
