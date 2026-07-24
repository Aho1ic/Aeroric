import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { NotificationsProvider } from "../hooks/useNotifications";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("NotificationsProvider", () => {
  beforeEach(() => {
    vi.useRealTimers();
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue({ notifications: [], unreadCount: 0 });
  });

  it("forces a remote release check on startup and periodic polling respects backend cache", async () => {
    vi.useFakeTimers();

    render(
      <I18nProvider>
        <NotificationsProvider>
          <div>app</div>
        </NotificationsProvider>
      </I18nProvider>,
    );

    await Promise.resolve();
    await Promise.resolve();

    expect(invoke).toHaveBeenCalledWith("get_notifications", { force: true });

    vi.mocked(invoke).mockClear();
    await vi.advanceTimersByTimeAsync(60_000);

    expect(invoke).toHaveBeenCalledWith("get_notifications", { force: false });
    vi.useRealTimers();
  });
});
