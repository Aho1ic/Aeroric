import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

let hoverSource:
  | ((
      view: {
        state: {
          doc: {
            lineAt: (offset: number) => { number: number; from: number; to: number };
          };
        };
      },
      pos: number,
    ) => unknown)
  | null = null;

vi.mock("@uiw/react-codemirror", async () => {
  const React = await import("react");
  const hoverTooltip = vi.fn((source: typeof hoverSource) => {
    hoverSource = source;
    return { __aeroricHover: true };
  });

  function MockCodeMirror({
    value,
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
            lineAt: (offset: number) => ({ number: 1, from: 0, to: value.length + offset }),
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
    hoverTooltip,
    showTooltip: {
      from: () => [],
    },
  };
});

describe("FileViewer LSP hover", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    hoverSource = null;
  });

  it("requests hover details at the hovered editor position and renders text safely", async () => {
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
      if (command === "lsp_hover") {
        return Promise.resolve({
          contents: "helper **value**\n<script>alert(1)</script>",
          range: null,
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

    await waitFor(() => expect(hoverSource).toBeTypeOf("function"));
    const tooltip = await hoverSource!(
      {
        state: {
          doc: {
            lineAt: (_offset) => ({ number: 1, from: 0, to: 24 }),
          },
        },
      },
      14,
    );

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_hover", {
        request: expect.objectContaining({
          filePath: "/tmp/aeroric/src/App.tsx",
          line: 0,
          character: 14,
        }),
      });
    });
    expect(tooltip).toMatchObject({ pos: 14, above: true });
    const dom = (tooltip as { create: () => { dom: HTMLElement } }).create().dom;
    expect(dom.textContent?.trim()).toBe("helper value");
    expect(dom.querySelector("strong")?.textContent).toBe("value");
    expect(dom.querySelector("script")).toBeNull();
  });

  it("shows a visible message when hover fails", async () => {
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
      if (command === "lsp_hover") {
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
        />
      </I18nProvider>,
    );

    await waitFor(() => expect(hoverSource).toBeTypeOf("function"));
    const tooltip = await hoverSource!(
      {
        state: {
          doc: {
            lineAt: (_offset) => ({ number: 1, from: 0, to: 24 }),
          },
        },
      },
      14,
    );

    expect(tooltip).toBeNull();
    expect(await screen.findByText("Error: Language server request timed out")).toBeInTheDocument();
  });
});
