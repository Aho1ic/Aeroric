import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import {
  PermissionsPanel,
  permissionsNeedingRestart,
} from "../components/app-settings/PermissionsPanel";
import type {
  SystemPermission,
  SystemPermissionIdentity,
  SystemPermissionReport,
  SystemPermissionStatus,
} from "../types";

const { flushTasksBeforeExitMock } = vi.hoisted(() => ({
  flushTasksBeforeExitMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../taskFlush", () => ({
  flushTasksBeforeExit: flushTasksBeforeExitMock,
}));

function permission(overrides: Partial<SystemPermission> & { id: string }): SystemPermission {
  const status = overrides.status ?? "notGranted";
  return {
    status,
    // 默认两个视角一致(系统与本进程同步),需要测"未生效"的用例显式覆盖。
    systemStatus: status,
    processStatus: status,
    restartRequired: false,
    canRequestInApp: true,
    canOpenSettings: true,
    canReset: false,
    needsRestart: false,
    probePrompts: false,
    reportOnly: false,
    ...overrides,
  };
}

function identity(overrides: Partial<SystemPermissionIdentity> = {}): SystemPermissionIdentity {
  return {
    subject: "com.aeroric.desktop",
    signature: "developer-id",
    stableAcrossUpdates: true,
    ...overrides,
  };
}

function report(
  permissions: SystemPermission[],
  supported = true,
  overrides: Partial<SystemPermissionReport> = {},
): SystemPermissionReport {
  return {
    platform: "macos",
    supported,
    permissions,
    identity: identity(),
    freshProbe: true,
    ...overrides,
  };
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
    flushTasksBeforeExitMock.mockReset();
    flushTasksBeforeExitMock.mockResolvedValue(undefined);
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

  it("waits for task persistence before restarting and stays open when saving fails", async () => {
    let releaseFlush!: () => void;
    flushTasksBeforeExitMock.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          releaseFlush = resolve;
        }),
    );
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_system_permissions") {
        return Promise.resolve(
          report([
            permission({
              id: "screen-recording",
              status: "granted",
              restartRequired: true,
              needsRestart: true,
            }),
          ]),
        );
      }
      if (command === "restart_app_for_permissions") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    const user = userEvent.setup();
    renderPanel();
    const restart = await screen.findByRole("button", { name: "Restart Now" });

    await user.click(restart);
    expect(invoke).not.toHaveBeenCalledWith("restart_app_for_permissions");
    releaseFlush();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("restart_app_for_permissions"));

    flushTasksBeforeExitMock.mockRejectedValueOnce(new Error("disk full"));
    await user.click(restart);
    expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "restart_app_for_permissions"),
    ).toHaveLength(1);
  });

  it("does not issue duplicate restart requests while the save handshake is pending", async () => {
    let releaseFlush!: () => void;
    flushTasksBeforeExitMock.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          releaseFlush = resolve;
        }),
    );
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_system_permissions") {
        return Promise.resolve(
          report([
            permission({
              id: "screen-recording",
              status: "granted",
              restartRequired: true,
              needsRestart: true,
            }),
          ]),
        );
      }
      if (command === "restart_app_for_permissions") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    renderPanel();
    const restart = await screen.findByRole("button", { name: "Restart Now" });

    fireEvent.click(restart);
    fireEvent.click(restart);
    expect(flushTasksBeforeExitMock).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalledWith("restart_app_for_permissions");

    releaseFlush();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("restart_app_for_permissions"));
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "restart_app_for_permissions"),
    ).toHaveLength(1);
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

  /**
   * 这条覆盖用户报的那个 bug:系统设置里已开、本进程还没拿到。
   * 面板必须报"已获取 + 重启生效",而不是"未获取"。
   */
  it("shows a system-granted permission as granted and asks for a restart, not as denied", async () => {
    vi.mocked(invoke).mockResolvedValue(
      report([
        permission({
          id: "screen-recording",
          status: "granted",
          systemStatus: "granted",
          processStatus: "notGranted",
          restartRequired: true,
          needsRestart: true,
        }),
      ]),
    );
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    const screenRow = row("Screen Recording");
    expect(within(screenRow).getByText("Granted")).toBeInTheDocument();
    expect(within(screenRow).queryByText("Not granted")).toBeNull();
    expect(within(screenRow).getByText(/still cannot use it/)).toBeInTheDocument();
    // 首次加载就要出重启横幅:用户往往是先去设置里开了开关才来看这个面板。
    expect(screen.getByRole("button", { name: "Restart Now" })).toBeInTheDocument();
  });

  it("explains an ad-hoc signature, which is why a granted switch can still read as denied", async () => {
    vi.mocked(invoke).mockResolvedValue(
      report([permission({ id: "screen-recording", canReset: true })], true, {
        identity: identity({
          signature: "adhoc",
          stableAcrossUpdates: false,
          warning: "unstableSignature",
        }),
      }),
    );
    renderPanel();

    await waitFor(() => expect(screen.getByText(/ad-hoc signed/)).toBeInTheDocument());
    expect(screen.getByText(/com\.aeroric\.desktop/)).toBeInTheDocument();
  });

  it("warns when running outside an app bundle, where grants land on the terminal instead", async () => {
    vi.mocked(invoke).mockResolvedValue(
      report([permission({ id: "screen-recording" })], true, {
        identity: identity({
          subject: "/repo/target/debug/aeroric",
          signature: "linker-signed",
          stableAcrossUpdates: false,
          warning: "notBundled",
        }),
      }),
    );
    renderPanel();

    await waitFor(() =>
      expect(screen.getByText(/running outside an app bundle/)).toBeInTheDocument(),
    );
  });

  it("re-authorizes a stuck permission by clearing the record and asking again", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_system_permissions") {
        return Promise.resolve(
          report([permission({ id: "screen-recording", canReset: true, needsRestart: true })]),
        );
      }
      if (command === "reset_system_permission") {
        return Promise.resolve(permission({ id: "screen-recording", canReset: true }));
      }
      if (command === "request_system_permission") {
        return Promise.resolve(
          permission({ id: "screen-recording", status: "granted", canReset: true }),
        );
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    const user = userEvent.setup();
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    await user.click(within(row("Screen Recording")).getByRole("button", { name: "Re-authorize" }));

    await waitFor(() =>
      expect(within(row("Screen Recording")).getByText("Granted")).toBeInTheDocument(),
    );
    expect(invoke).toHaveBeenCalledWith("reset_system_permission", { id: "screen-recording" });
    expect(invoke).toHaveBeenCalledWith("request_system_permission", { id: "screen-recording" });
    // 授权刚重建,不该再挂着上一次的"待重启"。
    expect(screen.queryByRole("button", { name: "Restart Now" })).toBeNull();
  });

  it("hides the re-authorize button once a permission is granted", async () => {
    vi.mocked(invoke).mockResolvedValue(
      report([permission({ id: "screen-recording", status: "granted", canReset: true })]),
    );
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    expect(
      within(row("Screen Recording")).queryByRole("button", { name: "Re-authorize" }),
    ).toBeNull();
  });

  /** Linux:没有应用级开关,摆按钮就是骗人;成因才是可行动的信息。 */
  it("reports Linux capabilities without offering buttons that would do nothing", async () => {
    vi.mocked(invoke).mockResolvedValue(
      report(
        [
          permission({
            id: "screen-recording",
            status: "notGranted",
            reportOnly: true,
            canRequestInApp: false,
            canOpenSettings: false,
            detail: "Wayland needs xdg-desktop-portal for screen capture, and it is not installed",
          }),
        ],
        true,
        { platform: "linux", freshProbe: false },
      ),
    );
    renderPanel();

    await waitFor(() => expect(screen.getByText("Screen Recording")).toBeInTheDocument());
    const screenRow = row("Screen Recording");
    expect(within(screenRow).getByText(/xdg-desktop-portal/)).toBeInTheDocument();
    expect(within(screenRow).getByText(/no per-app switch/)).toBeInTheDocument();
    expect(within(screenRow).queryByRole("button")).toBeNull();
    // 没有任何项目能在应用内请求时,"一键获取"不该出现。
    expect(screen.queryByRole("button", { name: "Grant All" })).toBeNull();
  });

  it("says so when the system's current answer could not be read", async () => {
    vi.mocked(invoke).mockResolvedValue(
      report([permission({ id: "screen-recording" })], true, {
        freshProbe: false,
        freshProbeError: "Probe process timed out",
      }),
    );
    renderPanel();

    await waitFor(() => expect(screen.getByText(/may be out of date/)).toBeInTheDocument());
    expect(screen.getByText(/Probe process timed out/)).toBeInTheDocument();
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

  /** 后端的 restartRequired 不需要基线:它由新进程探测直接得出。 */
  it("trusts the backend's restartRequired even with no baseline to compare against", () => {
    expect(
      permissionsNeedingRestart({}, [
        permission({
          id: "screen-recording",
          status: "granted",
          processStatus: "notGranted",
          restartRequired: true,
          needsRestart: true,
        }),
      ]),
    ).toEqual(["screen-recording"]);
  });
});
