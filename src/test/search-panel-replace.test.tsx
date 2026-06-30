import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SearchPanel } from "../components/search/SearchPanel";
import { I18nProvider } from "../i18n";
import { REMOTE_IDE_COMMAND_TIMEOUT_MS } from "../hooks/useCancellableInvoke";
import type { ReplacePreview } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const preview: ReplacePreview = {
  query: "oldName",
  replacement: "newName",
  totalMatches: 2,
  truncated: false,
  files: [
    {
      path: "/tmp/aeroric/src/App.tsx",
      name: "App.tsx",
      matches: [
        {
          path: "/tmp/aeroric/src/App.tsx",
          name: "App.tsx",
          line: 3,
          column: 12,
          lineText: "const oldName = 1;",
          matchText: "oldName",
          replacementText: "newName",
          start: 6,
          end: 13,
        },
      ],
    },
    {
      path: "/tmp/aeroric/src/utils.ts",
      name: "utils.ts",
      matches: [
        {
          path: "/tmp/aeroric/src/utils.ts",
          name: "utils.ts",
          line: 8,
          column: 10,
          lineText: "return oldName;",
          matchText: "oldName",
          replacementText: "newName",
          start: 42,
          end: 49,
        },
      ],
    },
  ],
};

const connection = {
  id: "ssh-1",
  name: "prod",
  host: "example.com",
  port: 22,
  username: "deploy",
  createdAt: 1,
};

describe("SearchPanel replace", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("runs text search through the remote search command for SSH projects", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_search_text") {
        return Promise.resolve([
          {
            path: "/srv/app/src/App.tsx",
            name: "App.tsx",
            line: 2,
            column: 7,
            lineText: "const title = 'Aeroric';",
            matchText: "title",
          },
        ]);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <SearchPanel
          projectPath="/srv/app"
          width={320}
          onOpenMatch={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.change(screen.getByPlaceholderText("Search text in project"), {
      target: { value: "title" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_search_text", {
        connection,
        remoteProjectPath: "/srv/app",
        query: "title",
        options: {
          caseSensitive: false,
          regex: false,
          wholeWord: false,
          includeGlob: null,
          excludeGlob: null,
          limit: 300,
        },
      });
    });
    expect(await screen.findByText("App.tsx")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Preview" })).toBeEnabled();
  });

  it("runs structured search through the remote structured command for SSH projects", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_search_structured") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <SearchPanel
          projectPath="/srv/app"
          width={320}
          onOpenMatch={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.change(screen.getByPlaceholderText("Search text in project"), {
      target: { value: "function $Name$($Args$)" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Structured" }));
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_search_structured", {
        connection,
        remoteProjectPath: "/srv/app",
        pattern: "function $Name$($Args$)",
        options: {
          caseSensitive: false,
          regex: false,
          wholeWord: false,
          includeGlob: null,
          excludeGlob: null,
          limit: 300,
        },
      });
    });
  });

  it("shows a visible timeout when remote search hangs", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_search_text") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <SearchPanel
          projectPath="/srv/app"
          width={320}
          onOpenMatch={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.change(screen.getByPlaceholderText("Search text in project"), {
      target: { value: "title" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(screen.getByText(/remote_search_text.*timed out after 60s/i)).toBeInTheDocument();
  });

  it("runs structured search when structured mode is enabled", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "search_structured") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <SearchPanel projectPath="/tmp/aeroric" width={320} onOpenMatch={vi.fn()} />
      </I18nProvider>,
    );

    fireEvent.change(screen.getByPlaceholderText("Search text in project"), {
      target: { value: "function $Name$($Args$)" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Structured" }));
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("search_structured", {
        projectPath: "/tmp/aeroric",
        pattern: "function $Name$($Args$)",
        options: {
          caseSensitive: false,
          regex: false,
          wholeWord: false,
          includeGlob: null,
          excludeGlob: null,
          limit: 300,
        },
      });
    });
  });

  it("applies only checked files from the replace preview", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "replace_text_preview") return Promise.resolve(preview);
      if (command === "apply_text_replacements") {
        return Promise.resolve({
          filesChanged: 1,
          replacementsApplied: 1,
          replacementsSkipped: 0,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <SearchPanel projectPath="/tmp/aeroric" width={320} onOpenMatch={vi.fn()} />
      </I18nProvider>,
    );

    fireEvent.change(screen.getByPlaceholderText("Search text in project"), {
      target: { value: "oldName" },
    });
    fireEvent.change(screen.getByPlaceholderText("Replace text"), {
      target: { value: "newName" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));

    const appCheckbox = await screen.findByRole("checkbox", {
      name: "Include App.tsx in replacement",
    });
    const utilsCheckbox = screen.getByRole("checkbox", {
      name: "Include utils.ts in replacement",
    });
    expect(appCheckbox).toBeChecked();
    expect(utilsCheckbox).toBeChecked();

    fireEvent.click(utilsCheckbox);
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("apply_text_replacements", {
        projectPath: "/tmp/aeroric",
        replacements: [
          {
            path: "/tmp/aeroric/src/App.tsx",
            start: 6,
            end: 13,
            matchText: "oldName",
            replacementText: "newName",
          },
        ],
      });
    });
  });

  it("applies remote replacement previews through SSH commands", async () => {
    const remotePreview: ReplacePreview = {
      ...preview,
      files: preview.files.map((file) => ({
        ...file,
        path: file.path.replace("/tmp/aeroric", "/srv/app"),
        matches: file.matches.map((match) => ({
          ...match,
          path: match.path.replace("/tmp/aeroric", "/srv/app"),
        })),
      })),
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_replace_text_preview") return Promise.resolve(remotePreview);
      if (command === "remote_apply_text_replacements") {
        return Promise.resolve({
          filesChanged: 1,
          replacementsApplied: 1,
          replacementsSkipped: 0,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <SearchPanel
          projectPath="/srv/app"
          width={320}
          onOpenMatch={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.change(screen.getByPlaceholderText("Search text in project"), {
      target: { value: "oldName" },
    });
    fireEvent.change(screen.getByPlaceholderText("Replace text"), {
      target: { value: "newName" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));

    const utilsCheckbox = await screen.findByRole("checkbox", {
      name: "Include utils.ts in replacement",
    });
    fireEvent.click(utilsCheckbox);
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_replace_text_preview", {
        connection,
        remoteProjectPath: "/srv/app",
        query: "oldName",
        replacement: "newName",
        options: {
          caseSensitive: false,
          regex: false,
          wholeWord: false,
          includeGlob: null,
          excludeGlob: null,
          limit: 300,
        },
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_apply_text_replacements", {
        connection,
        remoteProjectPath: "/srv/app",
        replacements: [
          {
            path: "/srv/app/src/App.tsx",
            start: 6,
            end: 13,
            matchText: "oldName",
            replacementText: "newName",
          },
        ],
      });
    });
  });
});
