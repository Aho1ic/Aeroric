import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React, { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { FileExplorer } from "../components/FileExplorer";
import { REMOTE_IDE_COMMAND_TIMEOUT_MS } from "../hooks/useCancellableInvoke";
import type { SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
}));

class ResizeObserverMock {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}

vi.stubGlobal("ResizeObserver", ResizeObserverMock);

const entries = [
  {
    name: "app.tsx",
    path: "/repo/app.tsx",
    is_dir: false,
    extension: "tsx",
    is_gitignored: false,
    modifiedAtMs: 200,
  },
  {
    name: "README.md",
    path: "/repo/README.md",
    is_dir: false,
    extension: "md",
    is_gitignored: false,
    modifiedAtMs: 100,
  },
];

const connection: SshConnection = {
  id: "ssh-1",
  name: "prod",
  host: "example.com",
  port: 22,
  username: "deploy",
  createdAt: 1,
};

function renderExplorer() {
  return render(
    React.createElement(
      I18nProvider,
      null,
      React.createElement(FileExplorer, {
        projectPath: "/repo",
        projectName: "repo",
        onFileSelect: vi.fn(),
        themeVariant: "light",
      }),
    ),
  );
}

describe("FileExplorer UI", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_dir_entries") return Promise.resolve(entries);
      return Promise.resolve(undefined);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps file icons separated from names", async () => {
    renderExplorer();

    const fileRow = await screen.findByText("app.tsx");
    const icon = fileRow.parentElement?.querySelector("[data-kind='code']") as HTMLElement;

    expect(icon).toBeInTheDocument();
    expect(icon).toHaveStyle({ width: "18px", display: "inline-flex", marginRight: "4px" });
  });

  it("toggles sort direction from the active field button and removes the old direction button", async () => {
    const user = userEvent.setup();
    renderExplorer();

    await screen.findByText("app.tsx");
    const sortControls = screen.getByRole("button", { name: /Modified/i })
      .parentElement as HTMLElement;
    const nameButton = within(sortControls).getByRole("button", { name: /Name/i });
    const modifiedButton = within(sortControls).getByRole("button", {
      name: /Modified descending/i,
    });

    expect(within(modifiedButton).getByTestId("sort-arrow-down")).toBeInTheDocument();
    expect(
      within(sortControls).queryByRole("button", { name: /^Asc$|^Desc$/i }),
    ).not.toBeInTheDocument();

    await user.click(modifiedButton);
    expect(within(modifiedButton).getByTestId("sort-arrow-up")).toBeInTheDocument();

    await user.click(nameButton);
    expect(within(nameButton).getByTestId("sort-arrow-up")).toBeInTheDocument();
  });

  it("opens sqlite database files in the database workspace", async () => {
    const user = userEvent.setup();
    const onFileSelect = vi.fn();
    const onOpenDatabaseFile = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_dir_entries") {
        return Promise.resolve([
          {
            name: "app.db",
            path: "/repo/app.db",
            is_dir: false,
            extension: "db",
            is_gitignored: false,
            modifiedAtMs: 300,
          },
        ]);
      }
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(FileExplorer, {
          projectPath: "/repo",
          projectName: "repo",
          onFileSelect,
          onOpenDatabaseFile,
          themeVariant: "light",
        }),
      ),
    );

    await user.click(await screen.findByText("app.db"));

    expect(onOpenDatabaseFile).toHaveBeenCalledWith("/repo/app.db", "app.db");
    expect(onFileSelect).not.toHaveBeenCalled();
  });

  it("shows the absolute current directory and navigates through project breadcrumbs", async () => {
    const user = userEvent.setup();
    const scrollTo = vi.fn();
    const originalScrollTo = HTMLElement.prototype.scrollTo;
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: scrollTo,
    });
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command !== "read_dir_entries") return Promise.resolve(undefined);
      const path = (args as { path?: string } | undefined)?.path;
      if (path === "/repo/src") {
        return Promise.resolve([
          {
            name: "components",
            path: "/repo/src/components",
            is_dir: true,
            extension: undefined,
            is_gitignored: false,
            modifiedAtMs: 100,
          },
        ]);
      }
      return Promise.resolve([
        {
          name: "src",
          path: "/repo/src",
          is_dir: true,
          extension: undefined,
          is_gitignored: false,
          modifiedAtMs: 200,
        },
      ]);
    });

    try {
      renderExplorer();

      expect(await screen.findByRole("button", { name: "/repo" })).toHaveAttribute(
        "aria-current",
        "location",
      );
      expect(screen.getByRole("button", { name: "/" })).toBeDisabled();

      await user.click(await screen.findByText("src"));

      expect(screen.getByRole("button", { name: "/repo/src" })).toHaveAttribute(
        "aria-current",
        "location",
      );
      expect(scrollTo).toHaveBeenCalledWith({ left: 0, behavior: "smooth" });

      await user.click(screen.getByRole("button", { name: "/repo" }));

      expect(screen.getByRole("button", { name: "/repo" })).toHaveAttribute(
        "aria-current",
        "location",
      );
      expect(scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "smooth" });
    } finally {
      Object.defineProperty(HTMLElement.prototype, "scrollTo", {
        configurable: true,
        value: originalScrollTo,
      });
    }
  });

  it("shows a visible timeout when remote directory reads hang", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_dir_entries") return new Promise(() => {});
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(FileExplorer, {
          projectPath: "/srv/app",
          projectName: "app",
          onFileSelect: vi.fn(),
          remote: { connection, projectPath: "/srv/app" },
          themeVariant: "light",
        }),
      ),
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(screen.getByTestId("file-explorer-error")).toHaveTextContent(
      /remote_read_dir_entries.*timed out after 60s/i,
    );
  });
});
