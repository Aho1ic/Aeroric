import { describe, expect, it } from "vitest";
import {
  buildLspDocumentRequest,
  isLspSupportedFile,
  languageServerStatusMessage,
} from "../hooks/languageServerState";

describe("language server state", () => {
  it("supports local TypeScript and JavaScript files", () => {
    expect(isLspSupportedFile("/repo/src/App.tsx")).toBe(true);
    expect(isLspSupportedFile("/repo/src/main.ts")).toBe(true);
    expect(isLspSupportedFile("/repo/src/view.jsx")).toBe(true);
    expect(isLspSupportedFile("/repo/src/index.js")).toBe(true);
    expect(isLspSupportedFile("/repo/src/lib.rs")).toBe(false);
  });

  it("builds an LSP document request with zero-based positions", () => {
    expect(
      buildLspDocumentRequest({
        projectPath: "/repo",
        filePath: "/repo/src/App.tsx",
        content: "const value = 1;\n",
        line: 12,
        column: 5,
      }),
    ).toEqual({
      projectPath: "/repo",
      filePath: "/repo/src/App.tsx",
      content: "const value = 1;\n",
      line: 11,
      character: 4,
    });
  });

  it("uses install hints when the language server is unavailable", () => {
    expect(
      languageServerStatusMessage({
        supported: true,
        available: false,
        languageId: "typescriptreact",
        command: { program: "typescript-language-server", args: ["--stdio"] },
        installHint: "pnpm add -D typescript-language-server typescript",
      }),
    ).toBe("pnpm add -D typescript-language-server typescript");
  });
});
