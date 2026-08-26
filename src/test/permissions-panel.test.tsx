import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import {
  PermissionsPanel,
  permissionsNeedingRestart,
} from "../components/app-settings/PermissionsPanel";
import type { SystemPermission, SystemPermissionReport, SystemPermissionStatus } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function permission(overrides: Partial<SystemPermission> & { id: string }): SystemPermission {
  return {
    status: "notGranted",
    canRequestInApp: true,
    canOpenSettings: true,
    needsRestart: false,
    probePrompts: false,
    ...overrides,
  };
}

function report(permissions: SystemPermission[], supported = true): SystemPermissionReport {
  return { platform: "macos", supported, permissions };
}

const DEFAULT_REPORT = report([
  permission({ id: "screen-recording", needsRestart: true }),
  permission({ id: "accessibility", status: "granted", needsRestart: true }),
  permission({ id: "full-disk-access", canRequestInApp: false, needsRestart: true }),
  permission({ id: "folder-desktop", status: "unknown", probePrompts: true }),
]);

function renderPanel() {
  return render(
    <I18nProvider>
      <PermissionsPanel />
    </I18nProvider>,
  );
}

function row(name: string) {
  return screen.getByRole("group", { name });
}

describe("PermissionsPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("lists every permission with its status and a settings shortcut", async () => {
    vi.mocked(invoke).mockImplementation((command) =>
      command === "list_system_permissions"
        ? Promise.resolve(structuredClone(DEFAULT_REPORT))
        : Promise.reject(new Error(`unexpected command: ${String(command)}`)),
    );
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    expect(screen.getByText("1 of 4 granted")).toBeInTheDocument();
    expect(within(row("Accessibility")).getByText("Granted")).toBeInTheDocument();
    expect(within(row("Screen Recording")).getByText("Not granted")).toBeInTheDocument();
    // 已授权的条目不再显示"获取",只保留系统设置入口。
    expect(within(row("Accessibility")).queryByRole("button", { name: /^Grant$/ })).toBeNull();
    expect(
      within(row("Accessibility")).getByRole("button", { name: "System Settings" }),
    ).toBeInTheDocument();
  });

  it("only offers System Settings for permissions macOS cannot grant in-app", async () => {
    vi.mocked(invoke).mockResolvedValue(structuredClone(DEFAULT_REPORT));
    renderPanel();

    await waitFor(() => expect(screen.getByText("Full Disk Access")).toBeInTheDocument());
    const manual = row("Full Disk Access");
    expect(within(manual).queryByRole("button", { name: /^Grant$/ })).toBeNull();
    expect(within(manual).getByRole("button", { name: "System Settings" })).toBeInTheDocument();
  });

  it("labels folder checks as prompting and explains why they read as unknown", async () => {
    vi.mocked(invoke).mockResolvedValue(structuredClone(DEFAULT_REPORT));
    renderPanel();

    await waitFor(() => expect(screen.getByText("Desktop Folder")).toBeInTheDocument());
    const folder = row("Desktop Folder");
    expect(within(folder).getByRole("button", { name: "Check" })).toBeInTheDocument();
    expect(within(folder).getByText(/not probed automatically/)).toBeInTheDocument();
  });

  it("requests a single permission and replaces only that row", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_system_permissions") {
        return Promise.resolve(structuredClone(DEFAULT_REPORT));
      }
      if (command === "request_system_permission") {
        return Promise.resolve(
          permission({ id: "screen-recording", status: "granted", needsRestart: true }),
        );
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    const user = userEvent.setup();
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    await user.click(within(row("Screen Recording")).getByRole("button", { name: "Grant" }));

    await waitFor(() =>
      expect(within(row("Screen Recording")).getByText("Granted")).toBeInTheDocument(),
    );
    expect(invoke).toHaveBeenCalledWith("request_system_permission", { id: "screen-recording" });
    // 其他条目不受影响,目录探测结果不会被整表刷新抹掉。
    expect(within(row("Desktop Folder")).getByText("Unknown")).toBeInTheDocument();
  });

  it("prompts for a restart once a restart-gated permission flips to granted", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_system_permissions") {
        return Promise.resolve(structuredClone(DEFAULT_REPORT));
      }
      if (command === "request_system_permission") {
        return Promise.resolve(
          permission({ id: "screen-recording", status: "granted", needsRestart: true }),
        );
      }
      if (command === "restart_app_for_permissions") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    const user = userEvent.setup();
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Restart Now" })).toBeNull();

    await user.click(within(row("Screen Recording")).getByRole("button", { name: "Grant" }));
    const restart = await screen.findByRole("button", { name: "Restart Now" });
    await user.click(restart);

    expect(invoke).toHaveBeenCalledWith("restart_app_for_permissions");
  });

  it("grants everything it can in one pass and names what is left to do by hand", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_system_permissions") {
        return Promise.resolve(structuredClone(DEFAULT_REPORT));
      }
      if (command === "request_all_system_permissions") {
        return Promise.resolve({
          report: report([
            permission({ id: "screen-recording", status: "granted", needsRestart: true }),
            permission({ id: "accessibility", status: "granted", needsRestart: true }),
            permission({
              id: "full-disk-access",
              canRequestInApp: false,
              needsRestart: true,
            }),
            permission({ id: "folder-desktop", status: "granted", probePrompts: true }),
          ]),
          requested: ["screen-recording", "folder-desktop"],
          manual: ["full-disk-access"],
        });
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    const user = userEvent.setup();
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Grant All" }));

    await waitFor(() => expect(screen.getByText("3 of 4 granted")).toBeInTheDocument());
    expect(
      screen.getByText(/only be enabled by hand in System Settings: Full Disk Access/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restart Now" })).toBeInTheDocument();
  });

  it("surfaces backend failures instead of leaving the list blank", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("permission probe exploded"));
    renderPanel();

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("permission probe exploded"),
    );
  });

  it("explains platforms without a per-app authorization model", async () => {
    vi.mocked(invoke).mockResolvedValue(report([], false));
    renderPanel();

    await waitFor(() =>
      expect(screen.getByText(/does not gate these capabilities/)).toBeInTheDocument(),
    );
    expect(screen.queryByRole("button", { name: "Grant All" })).toBeNull();
  });
});

describe("permissionsNeedingRestart", () => {
  const baseline: Record<string, SystemPermissionStatus> = {
    "screen-recording": "notGranted",
    accessibility: "granted",
    automation: "notGranted",
    "folder-desktop": "unknown",
  };

  it("reports permissions that flipped to granted and need a relaunch", () => {
    const flipped = permissionsNeedingRestart(baseline, [
      permission({ id: "screen-recording", status: "granted", needsRestart: true }),
      permission({ id: "folder-desktop", status: "granted", needsRestart: true }),
    ]);
    expect(flipped).toEqual(["screen-recording", "folder-desktop"]);
  });

  it("ignores permissions that were already granted before this session", () => {
    expect(
      permissionsNeedingRestart(baseline, [
        permission({ id: "accessibility", status: "granted", needsRestart: true }),
      ]),
    ).toEqual([]);
  });

  it("ignores permissions that take effect without a relaunch", () => {
    expect(
      permissionsNeedingRestart(baseline, [
        permission({ id: "automation", status: "granted", needsRestart: false }),
      ]),
    ).toEqual([]);
  });

  it("ignores permissions absent from the baseline", () => {
    expect(
      permissionsNeedingRestart(baseline, [
        permission({ id: "input-monitoring", status: "granted", needsRestart: true }),
      ]),
    ).toEqual([]);
  });
});
