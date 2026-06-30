import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  lspPositionToDocOffset,
  normalizeInlayHints,
  requestLspInlayHints,
} from "../components/file-viewer/lspInlayHints";
import type { LspInlayHint } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function hint(overrides: Partial<LspInlayHint>): LspInlayHint {
  return {
    label: overrides.label ?? ": string",
    position: overrides.position ?? { line: 0, character: 5 },
    kind: overrides.kind ?? null,
    tooltip: overrides.tooltip ?? null,
    paddingLeft: overrides.paddingLeft ?? false,
    paddingRight: overrides.paddingRight ?? false,
  };
}

function doc(text: string) {
  const starts: number[] = [0];
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] === "\n") starts.push(index + 1);
  }
  return {
    lines: starts.length,
    line: (number: number) => {
      const from = starts[number - 1] ?? 0;
      const next = starts[number];
      const to = next === undefined ? text.length : Math.max(from, next - 1);
      return { from, to };
    },
    sliceString: (from: number, to: number) => text.slice(from, to),
  };
}

describe("lsp inlay hints state", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("normalizes, de-duplicates, sorts, and limits inlay hints", () => {
    const hints = normalizeInlayHints(
      [
        hint({ label: "  ", position: { line: 0, character: 1 } }),
        hint({ label: ": number", position: { line: 2, character: 5 } }),
        hint({ label: ": string", position: { line: 1, character: 2 } }),
        hint({ label: ": string", position: { line: 1, character: 2 } }),
        hint({ label: "name:", position: { line: 0, character: 3 }, paddingRight: true }),
      ],
      2,
    );

    expect(hints).toEqual([
      hint({ label: "name:", position: { line: 0, character: 3 }, paddingRight: true }),
      hint({ label: ": string", position: { line: 1, character: 2 } }),
    ]);
  });

  it("converts LSP UTF-16 positions to document offsets", () => {
    const text = "const emoji = '👍';\nconst value = 1;";
    const editorDoc = doc(text);

    expect(lspPositionToDocOffset(editorDoc, { line: 0, character: 19 })).toBe(
      text.indexOf(";\n") + 1,
    );
    expect(lspPositionToDocOffset(editorDoc, { line: 0, character: 16 })).toBeNull();
    expect(lspPositionToDocOffset(editorDoc, { line: 1, character: 5 })).toBe(
      text.indexOf(" value"),
    );
  });

  it("requests inlay hints through the Tauri command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([hint({ label: ": string" })]);

    await expect(
      requestLspInlayHints({
        projectPath: "/repo",
        filePath: "/repo/src/App.tsx",
        content: "const value = 1;\n",
        line: 0,
        character: 0,
      }),
    ).resolves.toEqual([hint({ label: ": string" })]);

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_inlay_hints", {
      request: {
        projectPath: "/repo",
        filePath: "/repo/src/App.tsx",
        content: "const value = 1;\n",
        line: 0,
        character: 0,
      },
    });
  });
});
