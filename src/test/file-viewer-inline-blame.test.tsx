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

function renderFileViewer() {
  render(
    <I18nProvider>
      <FileViewer
        tabs={[{ path: "/tmp/aeroric/src/app.txt", name: "app.txt" }]}
        activeFilePath="/tmp/aeroric/src/app.txt"
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
}

describe("FileViewer inline blame", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("loads git blame for the active local file on demand", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve("one\ntwo\n");
      }
      if (command === "git_blame_file") {
        return Promise.resolve({
          filePath: "src/app.txt",
          lines: [
            {
              line: 1,
              commit: "abcdef123456",
              shortCommit: "abcdef1",
              author: "Ada",
              authorTime: 1,
              summary: "Add app",
              content: "one",
            },
          ],
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderFileViewer();

    fireEvent.click(await screen.findByRole("button", { name: "Blame" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("git_blame_file", {
        projectPath: "/tmp/aeroric",
        filePath: "src/app.txt",
      });
    });
  });
});
