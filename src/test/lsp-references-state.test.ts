import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  findLspReferences,
  lspReferencePreviewLine,
  lspReferenceToOpenTarget,
} from "../components/file-viewer/lspReferences";
import type { LspDocumentRequest } from "../hooks/languageServerState";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const request: LspDocumentRequest = {
  projectPath: "/tmp/aeroric",
  filePath: "/tmp/aeroric/src/App.tsx",
  content: "const value = helper();\n",
  line: 0,
  character: 14,
};

describe("LSP references state", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("requests references and converts locations to open targets", async () => {
    vi.mocked(invoke).mockResolvedValue([
      {
        uri: "file:///tmp/aeroric/src/App.tsx",
        path: "/tmp/aeroric/src/App.tsx",
        range: {
          start: { line: 0, character: 14 },
          end: { line: 0, character: 20 },
        },
      },
      {
        uri: "file:///tmp/aeroric/src/helper.ts",
        path: "/tmp/aeroric/src/helper.ts",
        range: {
          start: { line: 2, character: 7 },
          end: { line: 2, character: 13 },
        },
      },
    ]);

    const references = await findLspReferences(request);

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_references", { request });
    expect(references.map(lspReferenceToOpenTarget)).toEqual([
      {
        path: "/tmp/aeroric/src/App.tsx",
        name: "App.tsx",
        selection: { line: 1, column: 15 },
      },
      {
        path: "/tmp/aeroric/src/helper.ts",
        name: "helper.ts",
        selection: { line: 3, column: 8 },
      },
    ]);
  });

  it("extracts a trimmed preview line for a reference", () => {
    expect(
      lspReferencePreviewLine(
        "const value = helper();\n\nexport function helper() { return 1; }\n",
        {
          uri: "file:///tmp/aeroric/src/helper.ts",
          path: "/tmp/aeroric/src/helper.ts",
          range: {
            start: { line: 2, character: 16 },
            end: { line: 2, character: 22 },
          },
        },
      ),
    ).toEqual({
      line: 3,
      column: 17,
      text: "export function helper() { return 1; }",
    });
  });

  it("clips long reference preview lines", () => {
    const preview = lspReferencePreviewLine(
      `const value = "${"x".repeat(40)}";\n`,
      {
        uri: "file:///tmp/aeroric/src/App.tsx",
        path: "/tmp/aeroric/src/App.tsx",
        range: {
          start: { line: 0, character: 6 },
          end: { line: 0, character: 11 },
        },
      },
      20,
    );

    expect(preview.text).toHaveLength(20);
    expect(preview.text.endsWith("...")).toBe(true);
  });
});
