import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useLanguageServer } from "../hooks/useLanguageServer";
import type { LspServerStatus } from "../hooks/languageServerState";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

describe("useLanguageServer", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("ignores an in-flight status result after the hook is disabled", async () => {
    const pending = deferred<LspServerStatus>();
    vi.mocked(invoke).mockReturnValueOnce(pending.promise);

    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) =>
        useLanguageServer({
          projectPath: "/repo",
          filePath: "/repo/src/App.ts",
          content: "const value = 1;\n",
          cursorLine: 1,
          cursorColumn: 1,
          enabled,
        }),
      { initialProps: { enabled: true } },
    );

    await waitFor(() => expect(result.current.loading).toBe(true));
    rerender({ enabled: false });

    await act(async () => {
      pending.resolve({
        supported: true,
        available: true,
        languageId: "typescript",
        command: { program: "typescript-language-server", args: ["--stdio"] },
        installHint: null,
      });
      await pending.promise;
    });

    expect(result.current.supported).toBe(false);
    expect(result.current.status).toBeNull();
    expect(result.current.loading).toBe(false);
  });
});
