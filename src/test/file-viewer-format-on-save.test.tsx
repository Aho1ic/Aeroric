import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@uiw/react-codemirror", async () => {
  const React = await import("react");
  function MockCodeMirror({
    value,
    onChange,
    onCreateEditor,
  }: {
    value: string;
    onChange: (value: string) => void;
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

    return (
      <textarea
        aria-label="editor"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    );
  }

  return {
    default: MockCodeMirror,
    EditorView: {
      theme: () => [],
      scrollIntoView: () => ({}),
    },
  };
});

function renderFileViewer() {
  render(
    <I18nProvider>
      <FileViewer
        tabs={[{ path: "/tmp/aeroric/a.txt", name: "a.txt" }]}
        activeFilePath="/tmp/aeroric/a.txt"
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

describe("FileViewer format on save", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("formats and refreshes local file content after autosave when enabled", async () => {
    let readCount = 0;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: true } });
      }
      if (command === "read_file_content") {
        readCount += 1;
        return Promise.resolve(readCount === 1 ? "initial" : "formatted");
      }
      if (command === "write_file_content" || command === "format_file") {
        return Promise.resolve(undefined);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderFileViewer();

    const editor = await screen.findByLabelText("editor");
    vi.useFakeTimers();
    fireEvent.change(editor, { target: { value: "changed" } });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    vi.useRealTimers();

    await waitFor(() => {
      expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "format_file")).toBe(
        true,
      );
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("write_file_content", {
      path: "/tmp/aeroric/a.txt",
      content: "changed",
      projectPath: "/tmp/aeroric",
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("format_file", {
      projectPath: "/tmp/aeroric",
      filePath: "/tmp/aeroric/a.txt",
    });
    await waitFor(() => expect(editor).toHaveValue("formatted"));
  });
});
