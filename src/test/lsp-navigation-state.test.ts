import { describe, expect, it } from "vitest";
import { lspLocationToOpenTarget } from "../components/file-viewer/lspNavigation";

describe("lsp navigation state", () => {
  it("converts an LSP location into an Aeroric open-file target", () => {
    expect(
      lspLocationToOpenTarget({
        uri: "file:///repo/src/utils.ts",
        path: "/repo/src/utils.ts",
        range: {
          start: { line: 4, character: 2 },
          end: { line: 4, character: 12 },
        },
      }),
    ).toEqual({
      path: "/repo/src/utils.ts",
      name: "utils.ts",
      selection: { line: 5, column: 3 },
    });
  });
});
