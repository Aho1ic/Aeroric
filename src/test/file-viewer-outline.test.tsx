import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";
import type { LspSymbol } from "../types";

const editorSpies = vi.hoisted(() => ({
  dispatch: vi.fn(),
  focus: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@uiw/react-codemirror", async () => {
  const React = await import("react");

  function lineAt(value: string, offset: number) {
    let from = 0;
    let number = 1;
    while (from <= value.length) {
      const nextBreak = value.indexOf("\n", from);
      const to = nextBreak === -1 ? value.length : nextBreak;
      if (offset <= to || nextBreak === -1) return { number, from, to };
      from = nextBreak + 1;
      number += 1;
    }
    return { number, from: value.length, to: value.length };
  }

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
    const head = Math.max(0, value.indexOf("helper"));
    const view = React.useMemo(
      () => ({
        state: {
          doc: {
            lineAt: (offset: number) => lineAt(value, offset),
          },
          selection: { main: { head } },
        },
        viewport: { from: Math.max(0, value.indexOf("return")), to: value.length },
        dispatch: editorSpies.dispatch,
        focus: editorSpies.focus,
      }),
      [head, value],
    );

    React.useEffect(() => {
      if (initializedRef.current) return;
      initializedRef.current = true;
      onCreateEditor?.(view);
      onUpdate?.({ state: view.state, view });
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
      scrollIntoView: (offset: number) => ({ offset }),
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

const content = "function App() {\n  function render() {\n    return helper();\n  }\n}\n";

const symbols: LspSymbol[] = [
  {
    name: "App",
    kind: 12,
    detail: "function App()",
    containerName: null,
    uri: "file:///repo/src/App.tsx",
    path: "/repo/src/App.tsx",
    range: { start: { line: 0, character: 0 }, end: { line: 4, character: 1 } },
    selectionRange: { start: { line: 0, character: 9 }, end: { line: 0, character: 12 } },
  },
  {
    name: "render",
    kind: 12,
    detail: "function render()",
    containerName: "App",
    uri: "file:///repo/src/App.tsx",
    path: "/repo/src/App.tsx",
    range: { start: { line: 1, character: 2 }, end: { line: 3, character: 3 } },
    selectionRange: { start: { line: 1, character: 11 }, end: { line: 1, character: 17 } },
  },
];

function renderFileViewer() {
  render(
    <I18nProvider>
      <FileViewer
        tabs={[{ path: "/repo/src/App.tsx", name: "App.tsx" }]}
        activeFilePath="/repo/src/App.tsx"
        projectPath="/repo"
        onSelectTab={vi.fn()}
        onCloseTab={vi.fn()}
        onCloseOtherTabs={vi.fn()}
        onCloseTabsToRight={vi.fn()}
        onCloseAllTabs={vi.fn()}
        themeVariant="light"
      />
    </I18nProvider>,
  );
}

describe("FileViewer code outline", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    editorSpies.dispatch.mockReset();
    editorSpies.focus.mockReset();
  });

  it("loads document symbols, renders outline and breadcrumbs, and jumps to symbols", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") return Promise.resolve(content);
      if (command === "lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: true,
          languageId: "typescriptreact",
          command: { program: "typescript-language-server", args: ["--stdio"] },
          installHint: null,
        });
      }
      if (command === "lsp_document_symbols") return Promise.resolve(symbols);
      if (command === "lsp_inlay_hints") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderFileViewer();

    const outline = await screen.findByRole("navigation", { name: "Outline" });
    expect(within(outline).getByText("Loading outline...")).toBeInTheDocument();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_document_symbols", {
        request: expect.objectContaining({
          projectPath: "/repo",
          filePath: "/repo/src/App.tsx",
          line: 0,
          character: 0,
        }),
      });
    });
    expect(within(outline).getByRole("button", { name: "App" })).toBeInTheDocument();
    expect(within(outline).getByRole("button", { name: "render" })).toBeInTheDocument();

    const breadcrumbs = screen.getByRole("navigation", { name: "Breadcrumbs" });
    expect(within(breadcrumbs).getByText("src")).toBeInTheDocument();
    expect(within(breadcrumbs).getByText("App.tsx")).toBeInTheDocument();
    expect(within(breadcrumbs).getByText("App")).toBeInTheDocument();
    expect(within(breadcrumbs).getByText("render")).toBeInTheDocument();

    const stickyScroll = screen.getByRole("navigation", { name: "Sticky Scroll" });
    expect(within(stickyScroll).getByText("App")).toBeInTheDocument();
    expect(within(stickyScroll).getByText("render")).toBeInTheDocument();

    fireEvent.click(within(outline).getByRole("button", { name: "render" }));

    expect(editorSpies.dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        selection: { anchor: content.indexOf("render") },
      }),
    );
    expect(editorSpies.focus).toHaveBeenCalled();

    editorSpies.dispatch.mockClear();
    fireEvent.click(within(stickyScroll).getByRole("button", { name: "render" }));

    expect(editorSpies.dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        selection: { anchor: content.indexOf("render") },
      }),
    );
  });

  it("keeps the outline loading state visible while symbols are pending", async () => {
    let resolveSymbols: (value: LspSymbol[]) => void = () => undefined;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") return Promise.resolve(content);
      if (command === "lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: true,
          languageId: "typescriptreact",
          command: { program: "typescript-language-server", args: ["--stdio"] },
          installHint: null,
        });
      }
      if (command === "lsp_document_symbols") {
        return new Promise<LspSymbol[]>((resolve) => {
          resolveSymbols = resolve;
        });
      }
      if (command === "lsp_inlay_hints") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderFileViewer();

    const outline = await screen.findByRole("navigation", { name: "Outline" });
    expect(within(outline).getByText("Loading outline...")).toBeInTheDocument();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "lsp_document_symbols",
        expect.objectContaining({ request: expect.any(Object) }),
      );
    });
    resolveSymbols([]);
    expect(await within(outline).findByText("No symbols found.")).toBeInTheDocument();
  });
});
