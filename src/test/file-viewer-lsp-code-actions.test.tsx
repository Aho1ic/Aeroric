import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import {
  diagnosticToLspCodeActionDiagnostic,
  diagnosticsForLspCodeAction,
} from "../components/file-viewer/lspCodeActions";
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

describe("LSP code action diagnostics", () => {
  it("converts current-line diagnostics to LSP code action context", () => {
    const diagnostics = diagnosticsForLspCodeAction(
      {
        projectPath: "/tmp/aeroric",
        filePath: "/tmp/aeroric/src/App.tsx",
        content: "const value = helper();\n",
        line: 0,
        character: 14,
      },
      [
        {
          source: "lsp:typescript",
          severity: "error",
          message: "Cannot find name 'helper'.",
          file: "/tmp/aeroric/src/App.tsx",
          line: 1,
          column: 15,
          code: "2304",
        },
        {
          source: "eslint",
          severity: "warning",
          message: "Other line",
          file: "/tmp/aeroric/src/App.tsx",
          line: 2,
          column: 1,
        },
      ],
    );

    expect(diagnostics).toEqual([
      {
        range: {
          start: { line: 0, character: 14 },
          end: { line: 0, character: 14 },
        },
        severity: 1,
        source: "typescript",
        message: "Cannot find name 'helper'.",
        code: "2304",
      },
    ]);
  });

  it("keeps non-LSP diagnostic sources when converting diagnostics", () => {
    expect(
      diagnosticToLspCodeActionDiagnostic({
        source: "eslint",
        severity: "warning",
        message: "Unused value",
        file: "/tmp/aeroric/src/App.tsx",
        line: 3,
        column: 2,
        code: "no-unused-vars",
      }),
    ).toMatchObject({
      range: {
        start: { line: 2, character: 1 },
        end: { line: 2, character: 1 },
      },
      severity: 2,
      source: "eslint",
      code: "no-unused-vars",
    });
  });
});

describe("FileViewer LSP code actions", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("lists quick fixes and applies the selected workspace edit", async () => {
    const edit = {
      files: [
        {
          uri: "file:///tmp/aeroric/src/App.tsx",
          path: "/tmp/aeroric/src/App.tsx",
          edits: [
            {
              range: {
                start: { line: 0, character: 0 },
                end: { line: 0, character: 0 },
              },
              newText: "import { helper } from './helper';\n",
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
      if (command === "lsp_code_actions") {
        return Promise.resolve([
          {
            title: "Add missing import",
            kind: "quickfix",
            edit,
          },
        ]);
      }
      if (command === "lsp_apply_workspace_edit") {
        return Promise.resolve({
          filesChanged: 1,
          editsApplied: 1,
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
          diagnostics={[
            {
              source: "lsp:typescript",
              severity: "error",
              message: "Cannot find name 'helper'.",
              file: "/tmp/aeroric/src/App.tsx",
              line: 1,
              column: 15,
              code: "2304",
            },
          ]}
        />
      </I18nProvider>,
    );

    await screen.findByText("TS LSP");
    await screen.findByText("Ln 1, Col 15");
    fireEvent.click(screen.getByRole("button", { name: "Quick Fix" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_code_actions", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 14,
        }),
        diagnostics: [
          {
            range: {
              start: { line: 0, character: 14 },
              end: { line: 0, character: 14 },
            },
            severity: 1,
            source: "typescript",
            message: "Cannot find name 'helper'.",
            code: "2304",
          },
        ],
      });
    });

    fireEvent.click(await screen.findByRole("button", { name: "Add missing import" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_apply_workspace_edit", {
        projectPath: "/tmp/aeroric",
        edit,
      });
    });
    expect(await screen.findByText("Quick fix applied: 1 files, 1 edits")).toBeInTheDocument();
  });

  it("executes command-only quick fixes", async () => {
    const command = {
      title: "Organize Imports",
      command: "_typescript.organizeImports",
      arguments: ["file:///tmp/aeroric/src/App.tsx"],
    };
    vi.mocked(invoke).mockImplementation((commandName) => {
      if (commandName === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (commandName === "read_file_content") {
        return Promise.resolve("const value = helper();\n");
      }
      if (commandName === "lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: true,
          languageId: "typescriptreact",
          command: { program: "typescript-language-server", args: ["--stdio"] },
          installHint: null,
        });
      }
      if (commandName === "lsp_code_actions") {
        return Promise.resolve([
          {
            title: "Organize imports",
            kind: "source.organizeImports",
            command,
          },
        ]);
      }
      if (commandName === "lsp_execute_command") {
        return Promise.resolve(null);
      }
      return Promise.reject(new Error(`unexpected command: ${commandName}`));
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
    fireEvent.click(screen.getByRole("button", { name: "Quick Fix" }));
    fireEvent.click(await screen.findByRole("button", { name: "Organize imports" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_execute_command", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 14,
        }),
        command,
      });
    });
    expect(
      await screen.findByText("Quick fix command executed: Organize Imports"),
    ).toBeInTheDocument();
  });
});
