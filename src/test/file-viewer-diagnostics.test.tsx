import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";
import type { DiagnosticItem } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const dispatchSpy = vi.fn();
const clipboardWriteText = vi.fn();

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
            lineAt: (offset: number) => {
              const starts = [0, 6, 12];
              const index = offset < 6 ? 0 : offset < 12 ? 1 : 2;
              return {
                number: index + 1,
                from: starts[index],
                to: index === 2 ? value.length : starts[index + 1] - 1,
                length: (index === 2 ? value.length : starts[index + 1] - 1) - starts[index],
              };
            },
          },
          selection: { main: { head: 0 } },
        },
        dispatch: dispatchSpy,
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
      line: () => ({ range: () => ({}) }),
      mark: () => ({ range: () => ({}) }),
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

const diagnostics: DiagnosticItem[] = [
  {
    source: "tsc",
    severity: "error",
    message: "Missing semicolon",
    file: "/tmp/aeroric/src/App.tsx",
    line: 2,
    column: 3,
    code: "TS1005",
  },
  {
    source: "eslint",
    severity: "warning",
    message: "Unused value",
    file: "/tmp/aeroric/src/App.tsx",
    line: 3,
    column: 2,
    code: "no-unused-vars",
  },
];

describe("FileViewer diagnostics", () => {
  beforeEach(() => {
    dispatchSpy.mockReset();
    clipboardWriteText.mockReset();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: clipboardWriteText },
      configurable: true,
    });
    vi.mocked(invoke).mockReset();
  });

  it("shows current file diagnostics and moves to the next diagnostic with F2", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve("line1\nline2\nline3\n");
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
          diagnostics={diagnostics}
        />
      </I18nProvider>,
    );

    const editor = await screen.findByLabelText("editor");
    expect(await screen.findByText("2 problems")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "2 problems" }));
    expect(await screen.findByText("Current file diagnostics")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "All 2" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Errors 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Warnings 1" })).toBeInTheDocument();
    expect(screen.getByText("tsc (1)")).toBeInTheDocument();
    expect(screen.getByText("eslint (1)")).toBeInTheDocument();
    expect(screen.getByText("Missing semicolon")).toBeInTheDocument();
    expect(screen.getByText("(TS1005) · 2:3")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Warnings 1" }));
    expect(screen.queryByText("Missing semicolon")).not.toBeInTheDocument();
    expect(screen.getByText("Unused value")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Warnings 1" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy visible diagnostics" }));
    await waitFor(() => {
      expect(clipboardWriteText).toHaveBeenCalledWith(
        "[warning] /tmp/aeroric/src/App.tsx:3:2 eslint (no-unused-vars) - Unused value",
      );
    });
    expect(screen.getByRole("status")).toHaveTextContent("Copied 1 diagnostics");

    fireEvent.keyDown(editor, { key: "F2" });

    await waitFor(() => {
      expect(dispatchSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          selection: { anchor: 8 },
        }),
      );
    });
  });
});
