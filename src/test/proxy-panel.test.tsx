import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { ProxyPanel } from "../components/app-settings/ProxyPanel";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const loadedSettings = {
  claude_path: "",
  claude_gpt55_path: "",
  codex_path: "",
  claude_config_path: "",
  claude_gpt55_config_path: "",
  codex_config_path: "",
  proxy_settings: { url: "http://127.0.0.1:7890", no_proxy: "", username: "", password: "" },
  agent_proxy_enabled: {},
  custom_agents: [],
  send_shortcut: "enter",
  terminal_shift_enter_newline: true,
};

type TestResult = {
  success: boolean;
  reason: string;
  detail?: string;
  statusCode?: number;
  latencyMs?: number;
};

function installBackend(result: TestResult | Error) {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "load_app_settings") {
      return Promise.resolve(structuredClone(loadedSettings));
    }
    if (command === "test_proxy_connection") {
      return result instanceof Error ? Promise.reject(result) : Promise.resolve({ ...result });
    }
    return Promise.reject(new Error(`unexpected command: ${String(command)}`));
  });
}

function renderPanel() {
  return render(
    <I18nProvider>
      <ProxyPanel />
    </I18nProvider>,
  );
}

async function waitForLoaded() {
  await waitFor(() => {
    expect(screen.getByLabelText("Proxy URL")).toHaveValue("http://127.0.0.1:7890");
  });
}

describe("ProxyPanel test connection", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("sends the in-progress proxy edits and reports success with latency", async () => {
    installBackend({ success: true, reason: "ok", statusCode: 204, latencyMs: 128 });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.type(screen.getByLabelText("Username"), "alice");
    await user.click(screen.getByRole("button", { name: "Test Proxy" }));

    expect(await screen.findByText("Proxy connection successful (128 ms)")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("test_proxy_connection", {
      proxySettings: {
        url: "http://127.0.0.1:7890",
        no_proxy: "",
        username: "alice",
        password: "",
      },
    });
  });

  it("maps a failure reason code to localized copy", async () => {
    installBackend({ success: false, reason: "proxy_auth_required" });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Test Proxy" }));

    expect(
      await screen.findByText(
        "Proxy connection failed: The proxy requires authentication. Check the username and password.",
      ),
    ).toBeInTheDocument();
  });

  it("interpolates the status code for http_error", async () => {
    installBackend({ success: false, reason: "http_error", statusCode: 502, latencyMs: 40 });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Test Proxy" }));

    expect(
      await screen.findByText("Proxy connection failed: The proxy returned HTTP 502."),
    ).toBeInTheDocument();
  });

  it("falls back to the raw reason for unknown codes", async () => {
    installBackend({ success: false, reason: "moon_phase" });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Test Proxy" }));

    expect(
      await screen.findByText("Proxy connection failed: Unknown error (moon_phase)."),
    ).toBeInTheDocument();
  });

  it("surfaces a rejected invoke without crashing", async () => {
    installBackend(new Error("ipc down"));
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Test Proxy" }));

    expect(await screen.findByText(/Proxy connection failed: Error: ipc down/)).toBeInTheDocument();
  });

  it("clears a stale result when the proxy config changes", async () => {
    installBackend({ success: true, reason: "ok", latencyMs: 12 });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Test Proxy" }));
    expect(await screen.findByText("Proxy connection successful (12 ms)")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Proxy URL"), "1");
    expect(screen.queryByText("Proxy connection successful (12 ms)")).not.toBeInTheDocument();
  });

  it("disables the button while the proxy URL is empty", async () => {
    installBackend({ success: true, reason: "ok" });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.clear(screen.getByLabelText("Proxy URL"));
    expect(screen.getByRole("button", { name: "Test Proxy" })).toBeDisabled();
  });
});
