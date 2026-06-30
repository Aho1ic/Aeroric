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
  const domEventHandlers = vi.fn((handlers: Record<string, unknown>) => ({
    __aeroricHandlers: handlers,
  }));

  function collectHandlers(
    extensions: unknown[],
  ): Record<string, (event: Event, view: unknown) => boolean> {
    const handlers: Record<string, (event: Event, view: unknown) => boolean> = {};
    for (const extension of extensions.flat(Infinity) as Array<{
      __aeroricHandlers?: Record<string, (event: Event, view: unknown) => boolean>;
    }>) {
      Object.assign(handlers, extension.__aeroricHandlers ?? {});
    }
    return handlers;
  }

  function MockCodeMirror({
    value,
    extensions = [],
    onCreateEditor,
  }: {
    value: string;
    extensions?: unknown[];
    onCreateEditor?: (view: unknown) => void;
  }) {
    const view = React.useMemo(
      () => ({
        state: {
          doc: {
            lineAt: (_offset: number) => ({ number: 1, from: 0, to: value.length }),
          },
          selection: { main: { head: 0 } },
        },
        posAtCoords: () => 6,
        dispatch: () => undefined,
        focus: () => undefined,
      }),
      [value],
    );

    React.useEffect(() => {
      onCreateEditor?.(view);
    }, [onCreateEditor, view]);

    const handlers = collectHandlers(extensions);

    return (
      <textarea
        aria-label="editor"
        readOnly
        value={value}
        onMouseDown={(event) => {
          handlers.mousedown?.(event.nativeEvent, view);
        }}
      />
    );
  }

  return {
    default: MockCodeMirror,
    Decoration: {
      set: () => [],
      widget: () => ({ range: () => ({}) }),
    },
    EditorView: {
      decorations: { compute: () => [] },
      domEventHandlers,
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

describe("FileViewer LSP navigation", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("opens the definition target on Cmd-click", async () => {
    const onOpenDefinition = vi.fn();
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
      if (command === "lsp_definition") {
        return Promise.resolve([
          {
            uri: "file:///tmp/aeroric/src/helper.ts",
            path: "/tmp/aeroric/src/helper.ts",
            range: {
              start: { line: 2, character: 4 },
              end: { line: 2, character: 10 },
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

    const editor = await screen.findByLabelText("editor");
    fireEvent.mouseDown(editor, { button: 0, metaKey: true, clientX: 10, clientY: 10 });

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_definition", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 6,
        }),
      });
    });
    expect(onOpenDefinition).toHaveBeenCalledWith("/tmp/aeroric/src/helper.ts", "helper.ts", {
      line: 3,
      column: 5,
    });
  });

  it("shows a visible message when Cmd-click finds no definition", async () => {
    const onOpenDefinition = vi.fn();
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
      if (command === "lsp_definition") {
        return Promise.resolve([]);
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
    fireEvent.mouseDown(screen.getByLabelText("editor"), {
      button: 0,
      metaKey: true,
      clientX: 10,
      clientY: 10,
    });

    expect(await screen.findByText("No definition found.")).toBeInTheDocument();
    expect(onOpenDefinition).not.toHaveBeenCalled();
  });

  it("shows the install hint instead of calling definition when LSP is unavailable", async () => {
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
          available: false,
          languageId: null,
          command: null,
          installHint: "pnpm add -D typescript-language-server typescript",
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
          onOpenDefinition={vi.fn()}
        />
      </I18nProvider>,
    );

    await screen.findByText("LSP unavailable");
    fireEvent.mouseDown(screen.getByLabelText("editor"), {
      button: 0,
      metaKey: true,
      clientX: 10,
      clientY: 10,
    });

    expect(
      await screen.findByText("pnpm add -D typescript-language-server typescript"),
    ).toBeInTheDocument();
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "lsp_definition")).toBe(
      false,
    );
  });

  it("shows a visible error when the definition request fails", async () => {
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
      if (command === "lsp_definition") {
        return Promise.reject(new Error("Language server request timed out"));
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
          onOpenDefinition={vi.fn()}
        />
      </I18nProvider>,
    );

    await screen.findByText("TS LSP");
    fireEvent.mouseDown(screen.getByLabelText("editor"), {
      button: 0,
      metaKey: true,
      clientX: 10,
      clientY: 10,
    });

    expect(await screen.findByText("Error: Language server request timed out")).toBeInTheDocument();
  });
});
