import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";
import type { LocalHistoryEntry } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
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

describe("FileViewer local history", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    vi.mocked(confirm).mockResolvedValue(true);
  });

  it("opens local history and restores a selected snapshot", async () => {
    const entry: LocalHistoryEntry = {
      id: "1000",
      filePath: "/tmp/aeroric/src/app.txt",
      relativePath: "src/app.txt",
      createdAtMs: 1000,
      size: 4,
    };
    let restored = false;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "read_file_content") {
        return Promise.resolve(restored ? "old\n" : "current\n");
      }
      if (command === "list_local_history") {
        return Promise.resolve([entry]);
      }
      if (command === "read_local_history_entry") {
        return Promise.resolve({ entry, content: "old\n" });
      }
      if (command === "restore_local_history_entry") {
        restored = true;
        return Promise.resolve({ entry, content: "old\n" });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderFileViewer();

    fireEvent.click(await screen.findByRole("button", { name: "Tab actions" }));
    fireEvent.click(await screen.findByRole("button", { name: "Show Local History" }));

    const dialog = await screen.findByRole("dialog", { name: "Local History" });
    expect(await within(dialog).findByText(/old/)).toBeInTheDocument();
    expect(within(dialog).getByText(/current/)).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "Restore" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("restore_local_history_entry", {
        projectPath: "/tmp/aeroric",
        filePath: "/tmp/aeroric/src/app.txt",
        entryId: "1000",
      });
    });
    await waitFor(() => expect(screen.getByLabelText("editor")).toHaveValue("old\n"));
  });
});
