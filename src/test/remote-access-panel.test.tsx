import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "../lib/appDialog";
import { I18nProvider } from "../i18n";
import { RemoteAccessPanel } from "../components/app-settings/RemoteAccessPanel";

const eventMocks = vi.hoisted(() => ({
  pairedHandler: null as ((event: { payload: { deviceName: string } }) => void) | null,
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_event: string, handler: (event: { payload: { deviceName: string } }) => void) => {
    eventMocks.pairedHandler = handler;
    return Promise.resolve(eventMocks.unlisten);
  }),
}));

vi.mock("../lib/appDialog", () => ({
  confirm: vi.fn(),
}));

interface RemoteStatus {
  enabled: boolean;
  running: boolean;
  networkExposed: boolean;
  port: number;
  lanIp: string | null;
  lanAddresses: { interfaceName: string; ip: string }[];
  onlineCount: number;
  relayUrl: string | null;
  relayToken: string | null;
  publicEndpoints: string[];
  relayState: string;
}

interface RemoteDevice {
  id: string;
  name: string;
  createdAt: number;
  lastSeenAt: number;
  online: boolean;
}

interface RemoteInvite {
  pairingUrl: string;
  endpoint: string;
  expiresInSeconds: number;
}

const baseStatus: RemoteStatus = {
  enabled: true,
  running: true,
  networkExposed: true,
  port: 6790,
  lanIp: "192.168.1.10",
  lanAddresses: [
    { interfaceName: "en0", ip: "192.168.1.10" },
    { interfaceName: "utun4", ip: "100.125.106.127" },
    { interfaceName: "utun5", ip: "10.0.0.2" },
    { interfaceName: "utun1024", ip: "198.18.0.1" },
  ],
  onlineCount: 0,
  relayUrl: null,
  relayToken: null,
  publicEndpoints: [],
  relayState: "off",
};

const baseInvite: RemoteInvite = {
  pairingUrl: "aeroric://pair?code=single-use-code",
  endpoint: "ws://192.168.1.10:6790",
  expiresInSeconds: 600,
};

const pairedDevice: RemoteDevice = {
  id: "device-1",
  name: "Pixel 9 Pro",
  createdAt: 1_720_000_000_000,
  lastSeenAt: 1_720_000_030_000,
  online: true,
};

function cloneStatus(status: RemoteStatus): RemoteStatus {
  return {
    ...status,
    lanAddresses: status.lanAddresses.map((address) => ({ ...address })),
    publicEndpoints: [...status.publicEndpoints],
  };
}

function normalizeEndpoint(value: string): string {
  return value.trim().replace(/\/+$/, "");
}

function installBackend(options?: {
  status?: RemoteStatus;
  devices?: RemoteDevice[];
  invite?: RemoteInvite;
}) {
  const state = {
    status: cloneStatus(options?.status ?? baseStatus),
    devices: (options?.devices ?? []).map((device) => ({ ...device })),
    invite: { ...(options?.invite ?? baseInvite) },
  };

  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "remote_server_status") {
      return Promise.resolve(cloneStatus(state.status));
    }
    if (command === "remote_list_devices") {
      return Promise.resolve(state.devices.map((device) => ({ ...device })));
    }
    if (command === "remote_server_start") {
      const port = (args as { port?: number } | undefined)?.port ?? state.status.port;
      state.status = {
        ...state.status,
        enabled: true,
        running: true,
        port,
      };
      return Promise.resolve(cloneStatus(state.status));
    }
    if (command === "remote_server_stop") {
      state.status = {
        ...state.status,
        enabled: false,
        running: false,
        onlineCount: 0,
      };
      return Promise.resolve(cloneStatus(state.status));
    }
    if (command === "remote_select_lan_ip") {
      const lanIp = (args as { lanIp: string }).lanIp;
      state.status = {
        ...state.status,
        lanIp,
      };
      state.invite = {
        ...state.invite,
        endpoint: `ws://${lanIp}:${state.status.port}`,
      };
      return Promise.resolve(cloneStatus(state.status));
    }
    if (command === "remote_create_invite") {
      state.status = {
        ...state.status,
        networkExposed: true,
      };
      return Promise.resolve({ ...state.invite });
    }
    if (command === "remote_revoke_device") {
      const deviceId = (args as { deviceId: string }).deviceId;
      state.devices = state.devices.filter((device) => device.id !== deviceId);
      return Promise.resolve();
    }
    if (command === "remote_update_config") {
      const payload = args as {
        relayUrl: string;
        relayToken: string;
        publicEndpoints: string[];
      };
      state.status = {
        ...state.status,
        relayUrl: normalizeEndpoint(payload.relayUrl) || null,
        relayToken: payload.relayToken.trim() || null,
        publicEndpoints: [
          ...new Set(payload.publicEndpoints.map(normalizeEndpoint).filter(Boolean)),
        ],
        relayState: payload.relayUrl.trim() ? "connecting" : "off",
      };
      return Promise.resolve(cloneStatus(state.status));
    }
    return Promise.reject(new Error(`unexpected command: ${String(command)}`));
  });

  return state;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function renderPanel() {
  return render(
    <I18nProvider>
      <RemoteAccessPanel />
    </I18nProvider>,
  );
}

async function flushEffects() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function getAdvancedToggle() {
  return screen.getByRole("button", { name: /advanced public access settings/i });
}

const clipboardWrite = vi.fn<(text: string) => Promise<void>>();

function installClipboardMock() {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: clipboardWrite },
  });
}

describe("RemoteAccessPanel", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(listen).mockClear();
    eventMocks.pairedHandler = null;
    eventMocks.unlisten.mockReset();
    clipboardWrite.mockReset();
    clipboardWrite.mockResolvedValue();
    installClipboardMock();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not advertise a LAN endpoint before pairing exposes the listener", async () => {
    installBackend({
      status: {
        ...baseStatus,
        networkExposed: false,
      },
    });

    renderPanel();

    expect(
      await screen.findByText("Listening locally until you generate a pairing QR code"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy LAN address" })).not.toBeInTheDocument();
  });

  it("refreshes listener exposure after a successful pairing invite", async () => {
    const user = userEvent.setup();
    installBackend({
      status: {
        ...baseStatus,
        networkExposed: false,
      },
    });

    renderPanel();
    expect(
      await screen.findByText("Listening locally until you generate a pairing QR code"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Generate pairing QR code" }));

    expect(await screen.findAllByText("ws://192.168.1.10:6790")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Copy LAN address" })).toBeInTheDocument();
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_server_status"));
  });

  it("shows loading, retries a failed initial load, and copies the LAN address", async () => {
    const user = userEvent.setup();
    installClipboardMock();
    let shouldFail = true;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status") {
        return shouldFail
          ? Promise.reject(new Error("status unavailable"))
          : Promise.resolve(cloneStatus(baseStatus));
      }
      if (command === "remote_list_devices") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });

    renderPanel();
    expect(screen.getByText("Loading remote access status…")).toBeInTheDocument();
    expect(await screen.findByRole("alert")).toHaveTextContent("status unavailable");
    expect(screen.getByRole("heading", { name: "Status unavailable" })).toBeInTheDocument();
    expect(screen.getAllByText("Status unavailable").length).toBeGreaterThan(0);
    expect(screen.getByLabelText("Port")).toBeDisabled();

    await user.click(getAdvancedToggle());
    const relayInput = screen.getByLabelText("Relay URL");
    expect(relayInput).toBeDisabled();

    shouldFail = false;
    await user.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("heading", { name: "Running" })).toBeInTheDocument();
    expect(relayInput).toBeEnabled();
    expect(screen.getByText("ws://192.168.1.10:6790")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Copy LAN address" }));
    expect(clipboardWrite).toHaveBeenCalledWith("ws://192.168.1.10:6790");
    expect(await screen.findByText("Address copied")).toBeInTheDocument();
  });

  it("lists every local IPv4 address and uses the selected one for pairing", async () => {
    const user = userEvent.setup();
    installBackend();
    renderPanel();

    const addressSelect = await screen.findByLabelText("Local IP");
    expect(addressSelect).toHaveValue("192.168.1.10");
    expect(screen.getByRole("option", { name: "192.168.1.10 (en0)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "100.125.106.127 (utun4)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "10.0.0.2 (utun5)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "198.18.0.1 (utun1024)" })).toBeInTheDocument();

    await user.selectOptions(addressSelect, "10.0.0.2");
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_select_lan_ip", {
        lanIp: "10.0.0.2",
      }),
    );
    expect(await screen.findByText("ws://10.0.0.2:6790")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Generate pairing QR code" }));
    expect(await screen.findAllByText("ws://10.0.0.2:6790")).toHaveLength(2);
  });

  it("distinguishes an enabled server that failed to start", async () => {
    installBackend({
      status: {
        ...baseStatus,
        running: false,
        lanIp: null,
      },
    });

    renderPanel();

    expect(
      await screen.findByRole("heading", { name: "Enabled, but failed to start" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Remote server" })).toBeChecked();
    expect(screen.getByText("The LAN address appears after the server starts")).toBeInTheDocument();
  });

  it("disables starting and does not invoke the backend when the port is empty", async () => {
    installBackend({
      status: {
        ...baseStatus,
        enabled: false,
        running: false,
      },
    });
    renderPanel();

    const serverSwitch = await screen.findByRole("switch", { name: "Remote server" });
    const portInput = screen.getByLabelText("Port");
    fireEvent.change(portInput, { target: { value: "" } });

    expect(portInput).toHaveValue("");
    expect(portInput).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Enter a port from 1024 to 65535.")).toBeInTheDocument();
    expect(serverSwitch).toBeDisabled();

    fireEvent.click(serverSwitch);
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "remote_server_start"),
    ).toBe(false);
  });

  it("still stops an enabled failed-start server when the port is empty", async () => {
    installBackend({
      status: {
        ...baseStatus,
        enabled: true,
        running: false,
        lanIp: null,
      },
    });
    renderPanel();

    expect(
      await screen.findByRole("heading", { name: "Enabled, but failed to start" }),
    ).toBeInTheDocument();
    const serverSwitch = screen.getByRole("switch", { name: "Remote server" });
    const portInput = screen.getByLabelText("Port");
    fireEvent.change(portInput, { target: { value: "" } });

    expect(portInput).toHaveValue("");
    expect(portInput).toHaveAttribute("aria-invalid", "true");
    expect(serverSwitch).toBeChecked();
    expect(serverSwitch).toBeEnabled();

    fireEvent.click(serverSwitch);
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_server_stop", {});
      expect(serverSwitch).not.toBeChecked();
    });
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "remote_server_start"),
    ).toBe(false);
  });

  it("validates the port, starts and stops the service, and clears an old invite", async () => {
    const user = userEvent.setup();
    installBackend({
      status: {
        ...baseStatus,
        enabled: false,
        running: false,
        lanIp: "192.168.1.10",
      },
    });
    renderPanel();

    const serverSwitch = await screen.findByRole("switch", { name: "Remote server" });
    const portInput = screen.getByLabelText("Port");
    await user.clear(portInput);
    await user.type(portInput, "80");
    await user.click(serverSwitch);

    expect(screen.getByText("Enter a port from 1024 to 65535.")).toBeInTheDocument();
    expect(portInput).toHaveAttribute("aria-invalid", "true");
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("remote_server_start", expect.anything());

    await user.clear(portInput);
    await user.type(portInput, "7000");
    await user.click(serverSwitch);
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_server_start", { port: 7000 });
      expect(serverSwitch).toHaveAttribute("aria-checked", "true");
      expect(portInput).toBeDisabled();
    });

    await user.click(screen.getByRole("button", { name: "Generate pairing QR code" }));
    expect(await screen.findByRole("button", { name: "Copy pairing link" })).toBeInTheDocument();

    await user.click(serverSwitch);
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_server_stop", {});
      expect(serverSwitch).toHaveAttribute("aria-checked", "false");
    });
    expect(screen.queryByRole("button", { name: "Copy pairing link" })).not.toBeInTheDocument();
  });

  it("ignores a stale in-flight poll after starting and refreshes again after the mutation", async () => {
    vi.useFakeTimers();
    const initialStatus: RemoteStatus = {
      ...baseStatus,
      enabled: false,
      running: false,
      onlineCount: 0,
    };
    const startedStatus: RemoteStatus = {
      ...baseStatus,
      enabled: true,
      running: true,
      onlineCount: 1,
    };
    const stalePollStatus = deferred<RemoteStatus>();
    const stalePollDevices = deferred<RemoteDevice[]>();
    const postMutationStatus = deferred<RemoteStatus>();
    const postMutationDevices = deferred<RemoteDevice[]>();
    let statusCalls = 0;
    let deviceCalls = 0;

    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status") {
        statusCalls += 1;
        if (statusCalls === 1) return Promise.resolve(cloneStatus(initialStatus));
        if (statusCalls === 2) return stalePollStatus.promise;
        if (statusCalls === 3) return postMutationStatus.promise;
      }
      if (command === "remote_list_devices") {
        deviceCalls += 1;
        if (deviceCalls === 1) return Promise.resolve([]);
        if (deviceCalls === 2) return stalePollDevices.promise;
        if (deviceCalls === 3) return postMutationDevices.promise;
      }
      if (command === "remote_server_start") {
        return Promise.resolve(cloneStatus(startedStatus));
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    renderPanel();
    await flushEffects();

    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    await flushEffects();
    expect(statusCalls).toBe(2);
    expect(deviceCalls).toBe(2);

    await act(async () => {
      fireEvent.click(screen.getByRole("switch", { name: "Remote server" }));
      await Promise.resolve();
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_server_start", { port: 6790 });
    expect(screen.getByRole("switch", { name: "Remote server" })).toBeChecked();
    expect(screen.getByText("ws://192.168.1.10:6790")).toBeInTheDocument();

    await act(async () => {
      stalePollStatus.resolve(cloneStatus(initialStatus));
      stalePollDevices.resolve([]);
      await Promise.all([stalePollStatus.promise, stalePollDevices.promise]);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(statusCalls).toBe(3);
    expect(deviceCalls).toBe(3);
    expect(screen.getByRole("switch", { name: "Remote server" })).toBeChecked();
    expect(screen.getByText("ws://192.168.1.10:6790")).toBeInTheDocument();

    await act(async () => {
      postMutationStatus.resolve({
        ...startedStatus,
        onlineCount: 4,
      });
      postMutationDevices.resolve([{ ...pairedDevice }]);
      await Promise.all([postMutationStatus.promise, postMutationDevices.promise]);
      await Promise.resolve();
    });

    expect(screen.getByRole("heading", { name: "Running" })).toBeInTheDocument();
    expect(screen.getAllByText("4 active connection(s)").length).toBeGreaterThan(0);
    expect(screen.getByText(pairedDevice.name)).toBeInTheDocument();
  });

  it("keeps the starting title while the post-start refresh is still pending", async () => {
    const initialStatus: RemoteStatus = {
      ...baseStatus,
      enabled: false,
      running: false,
      lanIp: null,
    };
    const startedStatus: RemoteStatus = {
      ...baseStatus,
      enabled: true,
      running: true,
    };
    const postStartStatus = deferred<RemoteStatus>();
    const postStartDevices = deferred<RemoteDevice[]>();
    let statusCalls = 0;
    let deviceCalls = 0;

    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status") {
        statusCalls += 1;
        return statusCalls === 1
          ? Promise.resolve(cloneStatus(initialStatus))
          : postStartStatus.promise;
      }
      if (command === "remote_list_devices") {
        deviceCalls += 1;
        return deviceCalls === 1 ? Promise.resolve([]) : postStartDevices.promise;
      }
      if (command === "remote_server_start") {
        return Promise.resolve(cloneStatus(startedStatus));
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    renderPanel();

    const serverSwitch = await screen.findByRole("switch", { name: "Remote server" });
    await waitFor(() => expect(serverSwitch).toBeEnabled());
    await act(async () => {
      fireEvent.click(serverSwitch);
      await Promise.resolve();
      await Promise.resolve();
    });

    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_server_start", { port: 6790 }),
    );
    expect(statusCalls).toBe(2);
    expect(deviceCalls).toBe(2);
    expect(screen.getByRole("heading", { name: "Starting…" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Stopping…" })).not.toBeInTheDocument();
    expect(serverSwitch).toBeChecked();
    expect(serverSwitch).toBeDisabled();

    await act(async () => {
      postStartStatus.resolve(cloneStatus(startedStatus));
      postStartDevices.resolve([]);
      await Promise.all([postStartStatus.promise, postStartDevices.promise]);
      await Promise.resolve();
    });

    expect(screen.getByRole("heading", { name: "Running" })).toBeInTheDocument();
    expect(serverSwitch).toBeEnabled();
  });

  it("refreshes backend state after a failed start and shows the enabled start-failure status", async () => {
    const user = userEvent.setup();
    let backendStatus: RemoteStatus = {
      ...baseStatus,
      enabled: false,
      running: false,
      lanIp: null,
    };
    let statusCalls = 0;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status") {
        statusCalls += 1;
        return Promise.resolve(cloneStatus(backendStatus));
      }
      if (command === "remote_list_devices") return Promise.resolve([]);
      if (command === "remote_server_start") {
        backendStatus = {
          ...backendStatus,
          enabled: true,
          running: false,
        };
        return Promise.reject(new Error("failed to bind the remote port"));
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    renderPanel();

    const serverSwitch = await screen.findByRole("switch", { name: "Remote server" });
    await user.click(serverSwitch);

    expect(
      await screen.findByRole("heading", { name: "Enabled, but failed to start" }),
    ).toBeInTheDocument();
    expect(serverSwitch).toBeChecked();
    expect(screen.getByLabelText("Port")).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent("failed to bind the remote port");
    expect(statusCalls).toBe(2);
  });

  it("keeps advanced settings collapsed and only saves normalized dirty values", async () => {
    const user = userEvent.setup();
    installBackend({
      status: {
        ...baseStatus,
        relayUrl: "wss://relay.mine.dev",
        relayToken: "secret",
        publicEndpoints: ["wss://edge.mine.dev"],
        relayState: "online",
      },
    });
    renderPanel();

    expect(await screen.findByText("Relay: connected")).toBeInTheDocument();
    const advancedToggle = getAdvancedToggle();
    expect(advancedToggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByLabelText("Relay URL")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Generate pairing QR code" }));
    expect(await screen.findByRole("button", { name: "Copy pairing link" })).toBeInTheDocument();
    await user.click(advancedToggle);

    const relayInput = screen.getByLabelText("Relay URL");
    const tokenInput = screen.getByLabelText("Relay token");
    const endpointsInput = screen.getByLabelText("Custom public endpoints");
    const saveButton = screen.getByRole("button", { name: "Save public access settings" });
    expect(getAdvancedToggle()).toHaveAttribute("aria-expanded", "true");
    expect(saveButton).toBeDisabled();

    fireEvent.change(relayInput, { target: { value: "  wss://relay.mine.dev///  " } });
    fireEvent.change(tokenInput, { target: { value: "  secret  " } });
    fireEvent.change(endpointsInput, {
      target: { value: "wss://edge.mine.dev/\n wss://edge.mine.dev/// " },
    });
    expect(saveButton).toBeDisabled();

    fireEvent.change(endpointsInput, {
      target: {
        value: "wss://edge.mine.dev/\n wss://edge.mine.dev///\n  wss://tunnel.mine.dev///  ",
      },
    });
    expect(saveButton).toBeEnabled();
    expect(screen.getByText("Unsaved changes")).toBeInTheDocument();

    await user.click(saveButton);
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_update_config", {
        relayUrl: "wss://relay.mine.dev",
        relayToken: "secret",
        publicEndpoints: ["wss://edge.mine.dev", "wss://tunnel.mine.dev"],
      });
    });
    expect(
      await screen.findByText("Saved. Re-generate the pairing QR code to share new endpoints."),
    ).toBeInTheDocument();
    expect(saveButton).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Copy pairing link" })).not.toBeInTheDocument();
  });

  it("shows a save error in the advanced settings without discarding the draft", async () => {
    const user = userEvent.setup();
    installBackend();
    renderPanel();
    await screen.findByRole("switch", { name: "Remote server" });
    await user.click(getAdvancedToggle());

    const relayInput = screen.getByLabelText("Relay URL");
    await user.type(relayInput, "https://not-websocket.example.com");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_update_config") {
        return Promise.reject("Relay URL must start with ws:// or wss://");
      }
      if (command === "remote_server_status") return Promise.resolve(cloneStatus(baseStatus));
      if (command === "remote_list_devices") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });

    await user.click(screen.getByRole("button", { name: "Save public access settings" }));

    expect(
      await screen.findByText(
        "Could not save public access settings: Relay URL must start with ws:// or wss://",
      ),
    ).toBeInTheDocument();
    expect(relayInput).toHaveValue("https://not-websocket.example.com");
  });

  it("counts down an invite, copies its link, and hides it after expiry", async () => {
    vi.useFakeTimers();
    installBackend({
      invite: {
        ...baseInvite,
        expiresInSeconds: 2,
      },
    });
    renderPanel();

    await flushEffects();
    expect(screen.getByRole("switch", { name: "Remote server" })).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Generate pairing QR code" }));
      await Promise.resolve();
    });
    expect(screen.getByText("Expires in 0:02")).toBeInTheDocument();
    expect(screen.getByLabelText("Pairing QR code generated")).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy pairing link" }));
      await Promise.resolve();
    });
    expect(clipboardWrite).toHaveBeenCalledWith(baseInvite.pairingUrl);
    expect(screen.getByText("Pairing link copied")).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000);
    });
    expect(screen.getAllByText("This pairing code has expired.").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "Copy pairing link" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Regenerate QR code" })).toBeInTheDocument();
  });

  it("handles remote-device-paired, refreshes devices, and removes the invite", async () => {
    const user = userEvent.setup();
    const backend = installBackend();
    const { unmount } = renderPanel();

    await screen.findByRole("switch", { name: "Remote server" });
    expect(vi.mocked(listen)).toHaveBeenCalledWith("remote-device-paired", expect.any(Function));
    await user.click(screen.getByRole("button", { name: "Generate pairing QR code" }));
    expect(await screen.findByRole("button", { name: "Copy pairing link" })).toBeInTheDocument();

    backend.devices = [{ ...pairedDevice }];
    await act(async () => {
      eventMocks.pairedHandler?.({ payload: { deviceName: pairedDevice.name } });
    });

    expect(await screen.findByText(`Paired with ${pairedDevice.name}.`)).toBeInTheDocument();
    expect(screen.getByText(pairedDevice.name)).toBeInTheDocument();
    expect(screen.getByText("Online")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy pairing link" })).not.toBeInTheDocument();

    unmount();
    await waitFor(() => expect(eventMocks.unlisten).toHaveBeenCalledOnce());
  });

  it("does not restore a consumed invite when pairing finishes before its response", async () => {
    const user = userEvent.setup();
    const pendingInvite = deferred<RemoteInvite>();
    const backend = installBackend({
      status: {
        ...baseStatus,
        networkExposed: false,
      },
    });
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "remote_create_invite") return pendingInvite.promise;
      if (command === "remote_server_status") {
        return Promise.resolve(cloneStatus(backend.status));
      }
      if (command === "remote_list_devices") {
        return Promise.resolve(backend.devices.map((device) => ({ ...device })));
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)} ${String(args)}`));
    });
    renderPanel();

    await screen.findByRole("switch", { name: "Remote server" });
    await waitFor(() => expect(eventMocks.pairedHandler).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Generate pairing QR code" }));
    expect(screen.getByRole("button", { name: "Generating…" })).toBeDisabled();

    backend.status = {
      ...backend.status,
      networkExposed: true,
    };
    backend.devices = [{ ...pairedDevice }];
    await act(async () => {
      eventMocks.pairedHandler?.({ payload: { deviceName: pairedDevice.name } });
      await Promise.resolve();
    });

    await act(async () => {
      pendingInvite.resolve({ ...baseInvite });
      await pendingInvite.promise;
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(await screen.findByText(`Paired with ${pairedDevice.name}.`)).toBeInTheDocument();
    expect(screen.queryByLabelText("Pairing QR code generated")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy pairing link" })).not.toBeInTheDocument();
  });

  it("does not show a late invite failure after pairing", async () => {
    const user = userEvent.setup();
    const pendingInvite = deferred<RemoteInvite>();
    const backend = installBackend();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "remote_create_invite") return pendingInvite.promise;
      if (command === "remote_server_status") {
        return Promise.resolve(cloneStatus(backend.status));
      }
      if (command === "remote_list_devices") {
        return Promise.resolve(backend.devices.map((device) => ({ ...device })));
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)} ${String(args)}`));
    });
    renderPanel();

    await screen.findByRole("switch", { name: "Remote server" });
    await waitFor(() => expect(eventMocks.pairedHandler).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Generate pairing QR code" }));

    backend.devices = [{ ...pairedDevice }];
    await act(async () => {
      eventMocks.pairedHandler?.({ payload: { deviceName: pairedDevice.name } });
      await Promise.resolve();
    });

    await act(async () => {
      pendingInvite.reject(new Error("late invite failure"));
      try {
        await pendingInvite.promise;
      } catch {
        // The component owns this rejection; awaiting it here merely flushes
        // the deferred promise without creating an unhandled test rejection.
      }
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(await screen.findByText(`Paired with ${pairedDevice.name}.`)).toBeInTheDocument();
    expect(
      screen.queryByText("Could not generate a pairing QR code: Error: late invite failure"),
    ).not.toBeInTheDocument();
  });

  it("requires confirmation before revoking a device and refreshes after success", async () => {
    const user = userEvent.setup();
    installBackend({ devices: [pairedDevice] });
    vi.mocked(confirm).mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    renderPanel();

    expect(await screen.findByText(pairedDevice.name)).toBeInTheDocument();
    const revokeButton = screen.getByRole("button", { name: /Revoke device/i });
    await user.click(revokeButton);

    expect(vi.mocked(confirm)).toHaveBeenCalledWith(
      `Revoke ${pairedDevice.name}? The device will be disconnected immediately and must pair again to reconnect.`,
      expect.objectContaining({
        kind: "warning",
        title: "Revoke paired device?",
      }),
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("remote_revoke_device", expect.anything());

    await user.click(revokeButton);
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_revoke_device", {
        deviceId: pairedDevice.id,
      });
    });
    expect(await screen.findByText("No devices paired yet.")).toBeInTheDocument();
  });

  it("polls every five seconds without overlapping or overwriting an edited draft", async () => {
    vi.useFakeTimers();
    const pendingStatus = deferred<RemoteStatus>();
    const pendingDevices = deferred<RemoteDevice[]>();
    let statusCalls = 0;
    let deviceCalls = 0;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status") {
        statusCalls += 1;
        return statusCalls === 1
          ? Promise.resolve({
              ...cloneStatus(baseStatus),
              relayUrl: "wss://initial.example.com",
            })
          : pendingStatus.promise;
      }
      if (command === "remote_list_devices") {
        deviceCalls += 1;
        return deviceCalls === 1 ? Promise.resolve([]) : pendingDevices.promise;
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    renderPanel();

    await flushEffects();
    expect(screen.getByRole("switch", { name: "Remote server" })).toBeInTheDocument();
    fireEvent.click(getAdvancedToggle());
    const relayInput = screen.getByLabelText("Relay URL");
    fireEvent.change(relayInput, { target: { value: "wss://draft.example.com" } });

    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    await flushEffects();
    expect(statusCalls).toBe(2);
    expect(deviceCalls).toBe(2);

    act(() => {
      vi.advanceTimersByTime(10_000);
    });
    await flushEffects();
    expect(statusCalls).toBe(2);
    expect(deviceCalls).toBe(2);

    await act(async () => {
      pendingStatus.resolve({
        ...cloneStatus(baseStatus),
        relayUrl: "wss://polled.example.com",
        relayState: "online",
      });
      pendingDevices.resolve([{ ...pairedDevice, name: "Polled phone" }]);
      await Promise.all([pendingStatus.promise, pendingDevices.promise]);
    });

    expect(relayInput).toHaveValue("wss://draft.example.com");
    expect(screen.getByText("Polled phone")).toBeInTheDocument();
  });

  it("synchronizes clean port and public drafts from the five-second poll", async () => {
    vi.useFakeTimers();
    const backend = installBackend({
      status: {
        ...baseStatus,
        relayUrl: "wss://initial.example.com",
        relayToken: "initial-token",
        publicEndpoints: ["wss://initial-edge.example.com"],
        relayState: "online",
      },
    });
    renderPanel();
    await flushEffects();

    fireEvent.click(getAdvancedToggle());
    const portInput = screen.getByLabelText("Port");
    const relayInput = screen.getByLabelText("Relay URL");
    const tokenInput = screen.getByLabelText("Relay token");
    const endpointsInput = screen.getByLabelText("Custom public endpoints");
    expect(portInput).toHaveValue("6790");
    expect(relayInput).toHaveValue("wss://initial.example.com");

    backend.status = {
      ...backend.status,
      port: 7001,
      relayUrl: "wss://polled.example.com",
      relayToken: "polled-token",
      publicEndpoints: ["wss://polled-edge.example.com", "wss://backup.example.com"],
      relayState: "online",
    };
    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    await flushEffects();

    expect(portInput).toHaveValue("7001");
    expect(relayInput).toHaveValue("wss://polled.example.com");
    expect(tokenInput).toHaveValue("polled-token");
    expect(endpointsInput).toHaveValue("wss://polled-edge.example.com\nwss://backup.example.com");
    expect(screen.getByRole("button", { name: "Save public access settings" })).toBeDisabled();
    expect(screen.queryByText("Unsaved changes")).not.toBeInTheDocument();
  });
});
