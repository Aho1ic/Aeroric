import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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

describe("FileViewer LSP rename", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("previews and applies a confirmed rename workspace edit", async () => {
    const edit = {
      files: [
        {
          uri: "file:///tmp/aeroric/src/App.tsx",
          path: "/tmp/aeroric/src/App.tsx",
          edits: [
            {
              range: {
                start: { line: 0, character: 14 },
                end: { line: 0, character: 20 },
              },
              newText: "renamedHelper",
            },
          ],
        },
        {
          uri: "file:///tmp/aeroric/src/helper.ts",
          path: "/tmp/aeroric/src/helper.ts",
          edits: [
            {
              range: {
                start: { line: 2, character: 16 },
                end: { line: 2, character: 22 },
              },
              newText: "renamedHelper",
            },
          ],
        },
      ],
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve("const value = renamedHelper();\n");
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
      if (command === "lsp_rename") {
        return Promise.resolve(edit);
      }
      if (command === "lsp_apply_workspace_edit") {
        return Promise.resolve({
          filesChanged: 2,
          editsApplied: 2,
          editsSkipped: 0,
        });
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
    fireEvent.click(screen.getByRole("button", { name: "Rename Symbol" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New symbol name" }), {
      target: { value: "renamedHelper" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Preview Rename" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_rename", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 14,
        }),
        newName: "renamedHelper",
      });
    });
    expect(await screen.findByText("Rename Preview (2 files, 2 edits)")).toBeInTheDocument();
    expect(screen.getByText("/tmp/aeroric/src/helper.ts")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Apply Rename" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_apply_workspace_edit", {
        projectPath: "/tmp/aeroric",
        edit,
      });
    });
    expect(await screen.findByText("Rename applied: 2 files, 2 edits")).toBeInTheDocument();
  });
});
