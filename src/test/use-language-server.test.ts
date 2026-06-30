import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useLanguageServer } from "../hooks/useLanguageServer";
import type { LspServerStatus } from "../hooks/languageServerState";
import type { SshConnection } from "../types";

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

  it("opens, changes, and closes a local LSP document lifecycle", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: true,
          languageId: "typescript",
          command: { program: "typescript-language-server", args: ["--stdio"] },
          installHint: null,
        });
      }
      return Promise.resolve({});
    });

    const { rerender, unmount } = renderHook(
      ({ content }: { content: string }) =>
        useLanguageServer({
          projectPath: "/repo",
          filePath: "/repo/src/App.ts",
          content,
          cursorLine: 1,
          cursorColumn: 1,
        }),
      { initialProps: { content: "const value = 1;\n" } },
    );

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_open_document", {
        projectPath: "/repo",
        filePath: "/repo/src/App.ts",
        content: "const value = 1;\n",
        version: 1,
      });
    });

    rerender({ content: "const value = 2;\n" });

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_change_document", {
        projectPath: "/repo",
        filePath: "/repo/src/App.ts",
        content: "const value = 2;\n",
        version: 2,
      });
    });

    unmount();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_close_document", {
        projectPath: "/repo",
        filePath: "/repo/src/App.ts",
      });
    });
  });

  it("uses remote LSP lifecycle commands for SSH projects", async () => {
    const connection: SshConnection = {
      id: "conn-1",
      name: "Staging",
      host: "staging.example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: true,
          languageId: "typescriptreact",
          command: { program: "typescript-language-server", args: ["--stdio"] },
          installHint: null,
        });
      }
      return Promise.resolve({});
    });

    const { unmount } = renderHook(() =>
      useLanguageServer({
        projectPath: "/srv/app",
        filePath: "/srv/app/src/App.tsx",
        content: "export function App() { return null; }\n",
        cursorLine: 1,
        cursorColumn: 1,
        remote: { connection, projectPath: "/srv/app" },
      }),
    );

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_lsp_open_document", {
        connection,
        remoteProjectPath: "/srv/app",
        projectPath: "/srv/app",
        filePath: "/srv/app/src/App.tsx",
        content: "export function App() { return null; }\n",
        version: 1,
      });
    });

    unmount();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_lsp_close_document", {
        connection,
        remoteProjectPath: "/srv/app",
        projectPath: "/srv/app",
        filePath: "/srv/app/src/App.tsx",
      });
    });
  });

  it("surfaces a remote LSP tool-missing state without opening a lifecycle document", async () => {
    const connection: SshConnection = {
      id: "conn-1",
      name: "Staging",
      host: "staging.example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    const installHint = "Install typescript-language-server and typescript on the remote host";
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: false,
          languageId: "typescript",
          command: { program: "typescript-language-server", args: ["--stdio"] },
          installHint,
        });
      }
      return Promise.resolve({});
    });

    const { result, unmount } = renderHook(() =>
      useLanguageServer({
        projectPath: "/srv/app",
        filePath: "/srv/app/src/App.ts",
        content: "const value = 1;\n",
        cursorLine: 1,
        cursorColumn: 1,
        remote: { connection, projectPath: "/srv/app" },
      }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.status?.available).toBe(false);
    expect(result.current.message).toContain(installHint);
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "remote_lsp_open_document",
      expect.objectContaining({ filePath: "/srv/app/src/App.ts" }),
    );

    unmount();
  });

  it("falls back to a remote install hint when the server status invoke rejects", async () => {
    const connection: SshConnection = {
      id: "conn-1",
      name: "Staging",
      host: "staging.example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_lsp_server_status") {
        return Promise.reject(new Error("ssh: command not found"));
      }
      return Promise.resolve({});
    });

    const { result, unmount } = renderHook(() =>
      useLanguageServer({
        projectPath: "/srv/app",
        filePath: "/srv/app/src/App.ts",
        content: "const value = 1;\n",
        cursorLine: 1,
        cursorColumn: 1,
        remote: { connection, projectPath: "/srv/app" },
      }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.status?.available).toBe(false);
    expect(result.current.message).toContain(
      "Install typescript-language-server and typescript on the remote host",
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "remote_lsp_open_document",
      expect.objectContaining({ filePath: "/srv/app/src/App.ts" }),
    );

    unmount();
  });
});
