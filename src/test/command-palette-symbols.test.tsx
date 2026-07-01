import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CommandPalette } from "../components/command-palette/CommandPalette";
import { I18nProvider } from "../i18n";
import type { LspSymbol } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const helperSymbol: LspSymbol = {
  name: "helper",
  kind: 12,
  detail: "function helper(): string",
  containerName: "App",
  uri: "file:///tmp/aeroric/src/App.tsx",
  path: "/tmp/aeroric/src/App.tsx",
  range: {
    start: { line: 2, character: 2 },
    end: { line: 4, character: 3 },
  },
  selectionRange: {
    start: { line: 2, character: 11 },
    end: { line: 2, character: 17 },
  },
};

const remoteHelperSymbol: LspSymbol = {
  ...helperSymbol,
  uri: "file:///srv/app/src/App.tsx",
  path: "/srv/app/src/App.tsx",
};

const remoteConnection = {
  id: "ssh-1",
  name: "remote",
  host: "127.0.0.1",
  port: 22,
  username: "dev",
  createdAt: 1,
};

describe("CommandPalette symbols", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("loads current file symbols for @ input and opens the selected symbol location", async () => {
    const onOpenFile = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_file_content") return Promise.resolve("function helper() {}\n");
      if (command === "lsp_document_symbols") return Promise.resolve([helperSymbol]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <CommandPalette
          projectPath="/tmp/aeroric"
          activeFilePath="/tmp/aeroric/src/App.tsx"
          initialInput="@ helper"
          commands={[]}
          onOpenFile={onOpenFile}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByText("helper"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_document_symbols", {
        request: {
          projectPath: "/tmp/aeroric",
          filePath: "/tmp/aeroric/src/App.tsx",
          content: "function helper() {}\n",
          line: 0,
          character: 0,
        },
      });
    });
    expect(onOpenFile).toHaveBeenCalledWith("/tmp/aeroric/src/App.tsx", "App.tsx", {
      line: 3,
      column: 12,
    });
  });

  it("loads workspace symbols for # input", async () => {
    const onOpenFile = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "lsp_workspace_symbols") return Promise.resolve([helperSymbol]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <CommandPalette
          projectPath="/tmp/aeroric"
          initialInput="# helper"
          commands={[]}
          onOpenFile={onOpenFile}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByText("helper"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_workspace_symbols", {
        projectPath: "/tmp/aeroric",
        query: "helper",
      });
    });
    expect(onOpenFile).toHaveBeenCalledWith("/tmp/aeroric/src/App.tsx", "App.tsx", {
      line: 3,
      column: 12,
    });
  });

  it("loads remote current file symbols for @ input", async () => {
    const onOpenFile = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_file_content") return Promise.resolve("function helper() {}\n");
      if (command === "remote_lsp_document_symbols") return Promise.resolve([remoteHelperSymbol]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <CommandPalette
          projectPath="ssh://ssh-1/srv/app"
          activeFilePath="/srv/app/src/App.tsx"
          initialInput="@ helper"
          commands={[]}
          onOpenFile={onOpenFile}
          onClose={vi.fn()}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByText("helper"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_lsp_document_symbols", {
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        request: {
          projectPath: "ssh://ssh-1/srv/app",
          filePath: "/srv/app/src/App.tsx",
          content: "function helper() {}\n",
          line: 0,
          character: 0,
        },
      });
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_read_file_content", {
      connection: remoteConnection,
      remotePath: "/srv/app/src/App.tsx",
      remoteProjectPath: "/srv/app",
    });
    expect(onOpenFile).toHaveBeenCalledWith("/srv/app/src/App.tsx", "App.tsx", {
      line: 3,
      column: 12,
    });
  });

  it("loads remote workspace symbols for # input", async () => {
    const onOpenFile = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_lsp_workspace_symbols") return Promise.resolve([remoteHelperSymbol]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <CommandPalette
          projectPath="ssh://ssh-1/srv/app"
          initialInput="# helper"
          commands={[]}
          onOpenFile={onOpenFile}
          onClose={vi.fn()}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByText("helper"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_lsp_workspace_symbols", {
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        projectPath: "ssh://ssh-1/srv/app",
        query: "helper",
      });
    });
    expect(onOpenFile).toHaveBeenCalledWith("/srv/app/src/App.tsx", "App.tsx", {
      line: 3,
      column: 12,
    });
  });
});
