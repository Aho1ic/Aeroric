import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@uiw/react-codemirror", async () => {
  const React = await import("react");
  function MockCodeMirror({
    value,
    onCreateEditor,
  }: {
    value: string;
    onCreateEditor?: (view: unknown) => void;
  }) {
    React.useEffect(() => {
      const view = {
        state: {
          doc: { lineAt: () => ({ number: 1, from: 0 }) },
          selection: { main: { head: 0 } },
        },
        dispatch: () => undefined,
        focus: () => undefined,
      };
      onCreateEditor?.(view);
    }, [onCreateEditor]);

    return <textarea aria-label="editor" readOnly value={value} />;
  }

  return {
    default: MockCodeMirror,
    Decoration: {
      set: () => [],
      widget: () => ({ range: () => ({}) }),
    },
    EditorView: {
      decorations: { compute: () => [] },
      theme: () => [],
      scrollIntoView: () => ({}),
    },
    GutterMarker: class {},
    WidgetType: class {},
    gutter: () => [],
  };
});

function renderFileViewer({
  remote = false,
}: {
  remote?: boolean;
} = {}) {
  render(
    <I18nProvider>
      <FileViewer
        tabs={[{ path: "/tmp/aeroric/src/App.tsx", name: "App.tsx" }]}
        activeFilePath="/tmp/aeroric/src/App.tsx"
        projectPath="/tmp/aeroric"
        onSelectTab={vi.fn()}
        onCloseTab={vi.fn()}
        onCloseOtherTabs={vi.fn()}
        onCloseTabsToRight={vi.fn()}
        onCloseAllTabs={vi.fn()}
        themeVariant="light"
        remote={
          remote
            ? {
                connection: {
                  id: "ssh-1",
                  name: "remote",
                  host: "127.0.0.1",
                  port: 22,
                  username: "dev",
                  createdAt: 1,
                },
                projectPath: "/tmp/aeroric",
              }
            : undefined
        }
      />
    </I18nProvider>,
  );
}

describe("FileViewer language server status", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("checks the local TS language server and shows the ready state", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve("const value = 1;\n");
      }
      if (command === "lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: true,
          languageId: "typescriptreact",
          command: { program: "typescript-language-server", args: ["--stdio"] },
          installHint: null,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderFileViewer();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_server_status", {
        projectPath: "/tmp/aeroric",
        filePath: "/tmp/aeroric/src/App.tsx",
      });
    });
    expect(await screen.findByText("TS LSP")).toBeInTheDocument();
  });

  it("does not check LSP for remote TS files", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_file_content") {
        return Promise.resolve("const remoteValue = 1;\n");
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderFileViewer({ remote: true });

    await screen.findByLabelText("editor");
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "lsp_server_status")).toBe(
      false,
    );
  });
});
