import { fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@uiw/react-codemirror", async () => {
  const React = await import("react");

  function flatten(extensions: unknown[]): unknown[] {
    return extensions.flat(Infinity);
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
    const lineStarts = React.useMemo(() => {
      const starts = [0];
      for (let i = 0; i < value.length; i += 1) {
        if (value[i] === "\n") starts.push(i + 1);
      }
      return starts;
    }, [value]);
    const view = React.useMemo(
      () => ({
        state: {
          doc: {
            lineAt: (offset: number) => {
              let index = 0;
              for (let i = 0; i < lineStarts.length; i += 1) {
                if (lineStarts[i] <= offset) index = i;
              }
              const nextStart = lineStarts[index + 1] ?? value.length + 1;
              return {
                number: index + 1,
                from: lineStarts[index],
                to: Math.max(lineStarts[index], nextStart - 1),
              };
            },
          },
          selection: { main: { head: 0 } },
        },
        dispatch: () => undefined,
        focus: () => undefined,
      }),
      [lineStarts, value],
    );

    React.useEffect(() => {
      onCreateEditor?.(view);
    }, [onCreateEditor, view]);

    const gutterButtons = flatten(extensions)
      .filter(
        (extension): extension is {
          __testGutter: {
            lineMarker: (
              view: unknown,
              line: { from: number },
            ) => { toDOM: () => HTMLElement } | null;
            domEventHandlers?: {
              mousedown?: (
                view: unknown,
                line: { from: number },
                event: MouseEvent,
              ) => boolean;
            };
          };
        } => Boolean((extension as { __testGutter?: unknown }).__testGutter),
      )
      .flatMap((extension) =>
        lineStarts.map((from) => {
          const marker = extension.__testGutter.lineMarker(view, { from });
          if (!marker) return null;
          const dom = marker.toDOM();
          return (
            <button
              key={`${from}:${dom.title}`}
              type="button"
              aria-label={dom.title}
              onMouseDown={(event) => {
                extension.__testGutter.domEventHandlers?.mousedown?.(
                  view,
                  { from },
                  event.nativeEvent,
                );
              }}
            />
          );
        }),
      );

    return (
      <>
        <textarea aria-label="editor" readOnly value={value} />
        {gutterButtons}
      </>
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
      domEventHandlers: () => [],
      theme: () => [],
      scrollIntoView: () => ({}),
    },
    GutterMarker: class {},
    WidgetType: class {},
    gutter: (options: unknown) => ({ __testGutter: options }),
    hoverTooltip: () => [],
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
    showTooltip: {
      from: () => [],
    },
  };
});

describe("FileViewer test run gutter", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("runs the named test from a test file gutter marker", async () => {
    const onRunTestTarget = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve('test("adds numbers", () => {});\n');
      }
      if (command === "lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: false,
          languageId: "typescript",
          command: null,
          installHint: "Install language server",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <FileViewer
          tabs={[{ path: "/tmp/aeroric/src/math.test.ts", name: "math.test.ts" }]}
          activeFilePath="/tmp/aeroric/src/math.test.ts"
          projectPath="/tmp/aeroric"
          onSelectTab={vi.fn()}
          onCloseTab={vi.fn()}
          onCloseOtherTabs={vi.fn()}
          onCloseTabsToRight={vi.fn()}
          onCloseAllTabs={vi.fn()}
          themeVariant="light"
          onRunTestTarget={onRunTestTarget}
        />
      </I18nProvider>,
    );

    const runButton = await screen.findByRole("button", { name: "Run test adds numbers" });
    fireEvent.mouseDown(runButton);

    expect(onRunTestTarget).toHaveBeenCalledWith({
      filePath: "/tmp/aeroric/src/math.test.ts",
      line: 1,
      testName: "adds numbers",
    });
  });

  it("debugs the named test from a test file gutter marker", async () => {
    const onDebugTestTarget = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve('test("adds numbers", () => {});\n');
      }
      if (command === "lsp_server_status") {
        return Promise.resolve({
          supported: true,
          available: false,
          languageId: "typescript",
          command: null,
          installHint: "Install language server",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <FileViewer
          tabs={[{ path: "/tmp/aeroric/src/math.test.ts", name: "math.test.ts" }]}
          activeFilePath="/tmp/aeroric/src/math.test.ts"
          projectPath="/tmp/aeroric"
          onSelectTab={vi.fn()}
          onCloseTab={vi.fn()}
          onCloseOtherTabs={vi.fn()}
          onCloseTabsToRight={vi.fn()}
          onCloseAllTabs={vi.fn()}
          themeVariant="light"
          onDebugTestTarget={onDebugTestTarget}
        />
      </I18nProvider>,
    );

    const debugButton = await screen.findByRole("button", { name: "Debug test adds numbers" });
    fireEvent.mouseDown(debugButton);

    expect(onDebugTestTarget).toHaveBeenCalledWith({
      filePath: "/tmp/aeroric/src/math.test.ts",
      line: 1,
      testName: "adds numbers",
    });
  });

  it("runs and debugs remote test gutter targets", async () => {
    const onRunTestTarget = vi.fn();
    const onDebugTestTarget = vi.fn();
    const connection = {
      id: "ssh-1",
      name: "prod",
      host: "example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "remote_read_file_content") {
        return Promise.resolve('test("adds numbers", () => {});\n');
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <FileViewer
          tabs={[{ path: "/srv/app/src/math.test.ts", name: "math.test.ts" }]}
          activeFilePath="/srv/app/src/math.test.ts"
          projectPath="/srv/app"
          onSelectTab={vi.fn()}
          onCloseTab={vi.fn()}
          onCloseOtherTabs={vi.fn()}
          onCloseTabsToRight={vi.fn()}
          onCloseAllTabs={vi.fn()}
          themeVariant="light"
          remote={{ connection, projectPath: "/srv/app" }}
          onRunTestTarget={onRunTestTarget}
          onDebugTestTarget={onDebugTestTarget}
        />
      </I18nProvider>,
    );

    const runButton = await screen.findByRole("button", { name: "Run test adds numbers" });
    fireEvent.mouseDown(runButton);

    expect(onRunTestTarget).toHaveBeenCalledWith({
      filePath: "/srv/app/src/math.test.ts",
      line: 1,
      testName: "adds numbers",
    });
    const debugButton = screen.getByRole("button", { name: "Debug test adds numbers" });
    fireEvent.mouseDown(debugButton);
    expect(onDebugTestTarget).toHaveBeenCalledWith({
      filePath: "/srv/app/src/math.test.ts",
      line: 1,
      testName: "adds numbers",
    });
  });
});
