import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createLspSignatureHelpSource } from "../components/file-viewer/lspSignatureHelp";
import type { LspDocumentRequest } from "../hooks/languageServerState";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const request: LspDocumentRequest = {
  projectPath: "/tmp/aeroric",
  filePath: "/tmp/aeroric/src/App.tsx",
  content: "helper(",
  line: 0,
  character: 0,
};

describe("LSP signature help source", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("requests signature help after a trigger character and renders the active parameter", async () => {
    vi.mocked(invoke).mockResolvedValue({
      signatures: [
        {
          label: "helper(name: string, count: number): string",
          documentation: "Builds a **label**.<script>alert(1)</script>",
          parameters: [
            { label: "name: string", documentation: "Display name." },
            { label: "count: number", documentation: "Repeat **count**." },
          ],
        },
      ],
      activeSignature: 0,
      activeParameter: 1,
    });
    const source = createLspSignatureHelpSource({
      request,
      available: true,
      unavailableMessage: null,
      onError: vi.fn(),
    });

    const tooltip = await source({
      state: {
        doc: {
          lineAt: (_offset: number) => ({ number: 1, from: 0 }),
          sliceString: (_from: number, _to: number) => "(",
        },
      },
    }, 7);

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_signature_help", {
      request: expect.objectContaining({
        filePath: "/tmp/aeroric/src/App.tsx",
        line: 0,
        character: 7,
      }),
    });
    const dom = tooltip?.create().dom;
    expect(dom?.textContent).toContain("helper(name: string, count: number): string");
    expect(dom?.textContent).toContain("count: number: Repeat count.");
    expect(dom?.textContent).toContain("Builds a label.");
    expect(dom?.querySelector(".cm-lsp-signature-parameter strong")?.textContent).toBe("count");
    expect(dom?.querySelector(".cm-lsp-signature-docs strong")?.textContent).toBe("label");
    expect(dom?.querySelector("script")).toBeNull();
  });

  it("skips implicit requests when the previous character is not a signature trigger", async () => {
    const source = createLspSignatureHelpSource({
      request,
      available: true,
      unavailableMessage: null,
      onError: vi.fn(),
    });

    const tooltip = await source({
      state: {
        doc: {
          lineAt: (_offset: number) => ({ number: 1, from: 0 }),
          sliceString: (_from: number, _to: number) => "r",
        },
      },
    }, 7);

    expect(tooltip).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
