import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createLspCompletionSource } from "../components/file-viewer/lspCompletion";
import type { LspDocumentRequest } from "../hooks/languageServerState";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const request: LspDocumentRequest = {
  projectPath: "/tmp/aeroric",
  filePath: "/tmp/aeroric/src/App.tsx",
  content: "const result = helper",
  line: 0,
  character: 0,
};

describe("LSP completion source", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("requests completions at the editor position and maps items to completion options", async () => {
    vi.mocked(invoke).mockResolvedValue([
      {
        label: "helper",
        detail: "(alias) function helper(): string",
        documentation: "Returns the helper value.",
      },
      {
        label: "helperValue",
        detail: "const helperValue: string",
        documentation: null,
      },
    ]);
    const source = createLspCompletionSource({
      request,
      available: true,
      unavailableMessage: null,
      onError: vi.fn(),
    });

    const result = await source({
      pos: 19,
      explicit: true,
      state: {
        doc: {
          lineAt: (_offset: number) => ({ number: 1, from: 0, to: 21 }),
        },
      },
      matchBefore: (_expr: RegExp) => ({ from: 15, to: 19, text: "help" }),
    });

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_completion", {
      request: expect.objectContaining({
        filePath: "/tmp/aeroric/src/App.tsx",
        line: 0,
        character: 19,
      }),
    });
    expect(result).toEqual({
      from: 15,
      options: [
        {
          label: "helper",
          detail: "(alias) function helper(): string",
          info: "Returns the helper value.",
          type: "function",
        },
        {
          label: "helperValue",
          detail: "const helperValue: string",
          info: undefined,
          type: "variable",
        },
      ],
    });
  });

  it("returns null and shows a visible error when completion fails", async () => {
    const onError = vi.fn();
    vi.mocked(invoke).mockRejectedValue(new Error("Language server request timed out"));
    const source = createLspCompletionSource({
      request,
      available: true,
      unavailableMessage: null,
      onError,
    });

    const result = await source({
      pos: 19,
      explicit: true,
      state: {
        doc: {
          lineAt: (_offset: number) => ({ number: 1, from: 0, to: 21 }),
        },
      },
      matchBefore: (_expr: RegExp) => ({ from: 15, to: 19, text: "help" }),
    });

    expect(result).toBeNull();
    expect(onError).toHaveBeenCalledWith("Error: Language server request timed out");
  });
});
