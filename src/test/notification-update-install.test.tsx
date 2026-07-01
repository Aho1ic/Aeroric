import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { NotificationBell } from "../components/NotificationBell";
import { useNotifications } from "../hooks/useNotifications";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

vi.mock("../hooks/useNotifications", () => ({
  useNotifications: vi.fn(),
}));

describe("Notification release updater", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_pending_release_update") return Promise.resolve(null);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });
    vi.mocked(useNotifications).mockReturnValue({
      result: {
        unreadCount: 1,
        notifications: [
          {
            id: "release-42",
            level: "info",
            title: "Aeroric v9.9.9",
            body: "Release notes",
            bodyZh: null,
            createdAt: "2026-06-24",
            isRead: false,
            url: "https://github.com/Aho1ic/Aeroric/releases/tag/v9.9.9",
            releaseTag: "v9.9.9",
            updateInstallSupported: true,
          },
        ],
      },
      loading: false,
      error: null,
      fetchNotifications: vi.fn(),
      markRead: vi.fn(),
      markAllRead: vi.fn(),
    });
  });

  it("downloads the selected release first, then restarts to install it", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_pending_release_update") return Promise.resolve(null);
      if (command === "prepare_release_update") {
        return Promise.resolve({
          tagName: "v9.9.9",
          assetName: "Aeroric_9.9.9_aarch64.dmg",
          installerPath: "/Users/me/.aeroric/updates/v9.9.9/Aeroric_9.9.9_aarch64.dmg",
          readyToRestart: true,
        });
      }
      if (command === "restart_and_install_release_update") {
        return Promise.resolve({
          tagName: "v9.9.9",
          assetName: "Aeroric_9.9.9_aarch64.dmg",
          installedAppPath: "/Applications/Aeroric.app",
          restarted: true,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <NotificationBell />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("Releases"));
    await user.click(screen.getByRole("button", { name: "Download update v9.9.9" }));

    expect(invoke).toHaveBeenCalledWith("prepare_release_update", { tagName: "v9.9.9" });

    await user.click(screen.getByRole("button", { name: "Restart and update v9.9.9" }));

    expect(invoke).toHaveBeenCalledWith("restart_and_install_release_update", {
      tagName: "v9.9.9",
    });
  });

  it("shows restart update when the selected release is already downloaded", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_pending_release_update") {
        return Promise.resolve({
          tagName: "v9.9.9",
          assetName: "Aeroric_9.9.9_aarch64.dmg",
          installerPath: "/Users/me/.aeroric/updates/v9.9.9/Aeroric_9.9.9_aarch64.dmg",
          readyToRestart: true,
        });
      }
      if (command === "restart_and_install_release_update") {
        return Promise.resolve({
          tagName: "v9.9.9",
          assetName: "Aeroric_9.9.9_aarch64.dmg",
          installedAppPath: "/Applications/Aeroric.app",
          restarted: true,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <NotificationBell />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("Releases"));
    await screen.findByRole("button", { name: "Restart and update v9.9.9" });
    expect(screen.queryByRole("button", { name: "Download update v9.9.9" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Restart and update v9.9.9" }));

    expect(invoke).not.toHaveBeenCalledWith("prepare_release_update", expect.anything());
    expect(invoke).toHaveBeenCalledWith("restart_and_install_release_update", {
      tagName: "v9.9.9",
    });
  });

  it("restores the restart state from pending release data when reopening the bell", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_pending_release_update") {
        return Promise.resolve({
          tagName: "v9.9.9",
          assetName: "Aeroric_9.9.9_aarch64.dmg",
          installerPath: "/Users/me/.aeroric/updates/v9.9.9/Aeroric_9.9.9_aarch64.dmg",
          readyToRestart: true,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <NotificationBell />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("Releases"));
    expect(await screen.findByRole("button", { name: "Restart and update v9.9.9" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Download update v9.9.9" })).not.toBeInTheDocument();
  });

  it("does not show install controls for releases without compatible installer assets", async () => {
    const user = userEvent.setup();
    vi.mocked(useNotifications).mockReturnValue({
      result: {
        unreadCount: 1,
        notifications: [
          {
            id: "release-42",
            level: "info",
            title: "Aeroric v9.9.9",
            body: "Release notes",
            bodyZh: null,
            createdAt: "2026-06-24",
            isRead: false,
            url: "https://github.com/Aho1ic/Aeroric/releases/tag/v9.9.9",
            releaseTag: "v9.9.9",
            updateInstallSupported: false,
          },
        ],
      },
      loading: false,
      error: null,
      fetchNotifications: vi.fn(),
      markRead: vi.fn(),
      markAllRead: vi.fn(),
    });

    render(
      <I18nProvider>
        <NotificationBell />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("Releases"));

    expect(
      screen.queryByRole("button", { name: "Download update v9.9.9" }),
    ).not.toBeInTheDocument();
  });
});
