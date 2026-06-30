import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";
import type { LspInlayHint } from "../types";

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
          selection: { main: { head: 0 } },
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

const hints: LspInlayHint[] = [
  {
    label: ": number",
    position: { line: 0, character: 14 },
    kind: 1,
    tooltip: "Return type",
    paddingLeft: true,
    paddingRight: false,
  },
];

describe("FileViewer inlay hints", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("requests LSP inlay hints for local code files and shows loading state", async () => {
    let resolveHints: (value: LspInlayHint[]) => void = () => undefined;
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
      if (command === "lsp_document_symbols") return Promise.resolve([]);
      if (command === "lsp_inlay_hints") {
        return new Promise<LspInlayHint[]>((resolve) => {
          resolveHints = resolve;
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

    expect(await screen.findByText("Hints...")).toBeInTheDocument();
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_inlay_hints", {
        request: expect.objectContaining({
          projectPath: "/tmp/aeroric",
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 0,
        }),
      });
    });

    resolveHints(hints);

    await waitFor(() => {
      expect(screen.queryByText("Hints...")).not.toBeInTheDocument();
    });
  });
});
