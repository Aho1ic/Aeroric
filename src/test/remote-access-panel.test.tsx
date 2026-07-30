import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { RemoteAccessPanel } from "../components/app-settings/RemoteAccessPanel";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

const baseStatus = {
  enabled: true,
  running: true,
  port: 6790,
  lanIp: "192.168.1.10",
  onlineCount: 0,
  relayUrl: null as string | null,
  relayToken: null as string | null,
  publicEndpoints: [] as string[],
  relayState: "off",
};

function renderPanel() {
  return render(
    <I18nProvider>
      <RemoteAccessPanel />
    </I18nProvider>,
  );
}

describe("RemoteAccessPanel public access settings", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status") return Promise.resolve({ ...baseStatus });
      if (command === "remote_list_devices") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
  });

  it("saves relay and custom endpoints via remote_update_config", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "remote_server_status") return Promise.resolve({ ...baseStatus });
      if (command === "remote_list_devices") return Promise.resolve([]);
      if (command === "remote_update_config") {
        const payload = args as {
          relayUrl: string;
          relayToken: string;
          publicEndpoints: string[];
        };
        return Promise.resolve({
          ...baseStatus,
          relayUrl: payload.relayUrl,
          relayToken: payload.relayToken,
          publicEndpoints: payload.publicEndpoints,
          relayState: "connecting",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });

    renderPanel();
    const relayInput = await screen.findByPlaceholderText("wss://relay.example.com");
    await user.type(relayInput, "wss://relay.mine.dev");
    const endpointsInput = screen.getByPlaceholderText(/100\.64\.0\.5/);
    await user.type(endpointsInput, "ws://100.64.0.5:6790\n\nwss://tunnel.mine.dev");

    await user.click(screen.getByRole("button", { name: "Save public access settings" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_update_config", {
        relayUrl: "wss://relay.mine.dev",
        relayToken: "",
        // 空行被过滤,顺序保持
        publicEndpoints: ["ws://100.64.0.5:6790", "wss://tunnel.mine.dev"],
      });
    });
    expect(
      await screen.findByText("Saved. Re-generate the pairing QR code to share new endpoints."),
    ).toBeTruthy();
  });

  it("shows relay state when relay is configured and surfaces backend errors", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status")
        return Promise.resolve({
          ...baseStatus,
          relayUrl: "wss://relay.mine.dev",
          relayState: "online",
        });
      if (command === "remote_list_devices") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });

    renderPanel();
    expect(await screen.findByText("Relay: connected")).toBeTruthy();

    // 保存被后端拒绝(非法 URL)时展示错误
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_server_status")
        return Promise.resolve({ ...baseStatus, relayUrl: "wss://relay.mine.dev" });
      if (command === "remote_list_devices") return Promise.resolve([]);
      if (command === "remote_update_config")
        return Promise.reject("Relay URL must start with ws:// or wss://");
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });
    await user.click(screen.getByRole("button", { name: "Save public access settings" }));
    expect(await screen.findByText(/Relay URL must start with/)).toBeTruthy();
  });
});
