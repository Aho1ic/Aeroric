import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { dispatchFileViewerCommand } from "../components/file-viewer/editorCommandEvents";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@uiw/react-codemirror", async () => {
  const React = await import("react");

  function MockCodeMirror({
    value,
    onCreateEditor,
    onUpdate,
  }: {
    value: string;
    extensions?: unknown[];
    onCreateEditor?: (view: unknown) => void;
    onUpdate?: (update: unknown) => void;
  }) {
    const initializedRef = React.useRef(false);
    const view = React.useMemo(
      () => ({
        state: {
          doc: {
            lineAt: (_offset: number) => ({ number: 1, from: 0, to: value.length }),
          },
          selection: { main: { head: 14 } },
        },
        dispatch: () => undefined,
        focus: () => undefined,
      }),
      [value],
    );

    React.useEffect(() => {
      if (initializedRef.current) return;
      initializedRef.current = true;
      onCreateEditor?.(view);
      onUpdate?.({ state: view.state });
    }, [onCreateEditor, onUpdate, view]);

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
      domEventHandlers: () => [],
      theme: () => [],
      scrollIntoView: () => ({}),
    },
    GutterMarker: class {},
    StateEffect: {
      define: () => ({
        of: (value: unknown) => ({ value, is: () => true }),
      }),
    },
    StateField: {
      define: (config: unknown) => config,
    },
    ViewPlugin: {
      fromClass: () => [],
    },
    WidgetType: class {},
    gutter: () => [],
    hoverTooltip: () => [],
    showTooltip: {
      from: () => [],
    },
  };
});

describe("FileViewer LSP references", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("shows references and opens the selected reference", async () => {
    const onOpenDefinition = vi.fn();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        if (
          typeof args === "object" &&
          args !== null &&
          "path" in args &&
          args.path === "/tmp/aeroric/src/helper.ts"
        ) {
          return Promise.resolve(
            "export function value() {}\n\nexport function helper() { return 1; }\n",
          );
        }
        return Promise.resolve("const value = helper();\n");
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
      if (command === "lsp_references") {
        return Promise.resolve([
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
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

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
          onOpenDefinition={onOpenDefinition}
        />
      </I18nProvider>,
    );

    await screen.findByText("TS LSP");
    await screen.findByText("Ln 1, Col 15");
    fireEvent.click(screen.getByRole("button", { name: "Find References" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_references", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 14,
        }),
      });
    });

    const referencesDialog = await screen.findByRole("dialog", { name: "References (2)" });
    expect(referencesDialog).toBeInTheDocument();
    expect(await within(referencesDialog).findByText("const value = helper();")).toBeInTheDocument();
    expect(
      await within(referencesDialog).findByText("export function helper() { return 1; }"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "/tmp/aeroric/src/helper.ts:3:8" }));

    expect(onOpenDefinition).toHaveBeenCalledWith("/tmp/aeroric/src/helper.ts", "helper.ts", {
      line: 3,
      column: 8,
    });
  });

  it("runs find references from the editor command event", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve("const value = helper();\n");
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
      if (command === "lsp_references") {
        return Promise.resolve([
          {
            uri: "file:///tmp/aeroric/src/App.tsx",
            path: "/tmp/aeroric/src/App.tsx",
            range: {
              start: { line: 0, character: 14 },
              end: { line: 0, character: 20 },
            },
          },
        ]);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

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
        />
      </I18nProvider>,
    );

    await screen.findByText("TS LSP");
    await screen.findByText("Ln 1, Col 15");
    act(() => dispatchFileViewerCommand("findReferences"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_references", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 14,
        }),
      });
    });
    expect(await screen.findByText("References (1)")).toBeInTheDocument();
  });

  it("runs find references from the editor context menu", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve("const value = helper();\n");
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
      if (command === "lsp_references") {
        return Promise.resolve([
          {
            uri: "file:///tmp/aeroric/src/App.tsx",
            path: "/tmp/aeroric/src/App.tsx",
            range: {
              start: { line: 0, character: 14 },
              end: { line: 0, character: 20 },
            },
          },
        ]);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

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
        />
      </I18nProvider>,
    );

    await screen.findByText("TS LSP");
    await screen.findByText("Ln 1, Col 15");
    fireEvent.contextMenu(screen.getByLabelText("editor"), { clientX: 80, clientY: 90 });
    const menu = await screen.findByRole("menu", { name: "Editor Actions" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Find References" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_references", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 14,
        }),
      });
    });
    expect(await screen.findByText("References (1)")).toBeInTheDocument();
  });
});
