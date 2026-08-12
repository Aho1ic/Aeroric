import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HomeLocalRouterToggle } from "../components/WelcomePage";
import { LocalRouterPanel } from "../components/app-settings/LocalRouterPanel";
import {
  DEFAULT_LOCAL_ROUTER_SETTINGS,
  type AppSettings,
  type LocalRouterSettings,
  type LocalRouterStatus,
  type LocalRouterTargetStatus,
} from "../components/app-settings/types";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("../components/recursive-hero-effect/recursive-hero-effect", () => ({
  createRecursiveHeroEffect: vi.fn(() => ({
    destroy: vi.fn(),
    setReducedMotion: vi.fn(),
  })),
}));

const enabledSettings: LocalRouterSettings = {
  ...DEFAULT_LOCAL_ROUTER_SETTINGS,
  show_on_home: true,
  enabled: true,
};

const runningStatus: LocalRouterStatus = {
  desired_enabled: true,
  running: true,
  starting: false,
  listen_url: "http://127.0.0.1:18080",
  total_requests: 23,
  successful_requests: 20,
  failed_requests: 3,
  active_requests: 2,
  input_tokens: 1_200,
  output_tokens: 340,
  cache_creation_tokens: 80,
  cache_read_tokens: 560,
  last_error: null,
  targets: [],
};

const claudeTargets: LocalRouterTargetStatus[] = [
  {
    agent: "claude",
    target_id: "claude",
    target_name: "Claude Code",
    base_url: "https://api.anthropic.com",
    active: true,
    queue_position: null,
    models: [],
    enable_1m_context: false,
    enable_chat_completions_proxy: false,
    healthy: true,
    circuit: {
      state: "closed",
      consecutive_failures: 0,
      consecutive_successes: 0,
      total_requests: 0,
      failed_requests: 0,
      last_success_at: null,
      last_failure_at: null,
      last_error: null,
    },
  },
  {
    agent: "claude",
    target_id: "claude-backup",
    target_name: "Claude Backup",
    base_url: "https://backup.example.test",
    active: false,
    queue_position: null,
    models: [],
    enable_1m_context: false,
    enable_chat_completions_proxy: false,
    healthy: true,
    circuit: {
      state: "closed",
      consecutive_failures: 0,
      consecutive_successes: 0,
      total_requests: 0,
      failed_requests: 0,
      last_success_at: null,
      last_failure_at: null,
      last_error: null,
    },
  },
];

function appSettings(localRouterSettings = enabledSettings): AppSettings {
  return {
    claude_path: "",
    claude_gpt55_path: "",
    codex_path: "",
    claude_config_path: "",
    claude_gpt55_config_path: "",
    codex_config_path: "",
    send_shortcut: "enter",
    terminal_shift_enter_newline: true,
    local_router_settings: localRouterSettings,
  };
}

function renderWithI18n(node: React.ReactNode) {
  return render(<I18nProvider>{node}</I18nProvider>);
}

describe("LocalRouterPanel", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "load_app_settings") return Promise.resolve(appSettings());
      if (command === "get_local_router_status") return Promise.resolve(runningStatus);
      if (command === "update_local_router_settings") return Promise.resolve(appSettings());
      if (command === "set_local_router_enabled") return Promise.resolve(runningStatus);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });
  });

  it("loads router state and saves agent, address, and usage settings", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, payload) => {
      if (command === "load_app_settings") return Promise.resolve(appSettings());
      if (command === "get_local_router_status") return Promise.resolve(runningStatus);
      if (command === "update_local_router_settings") {
        const settings = (payload as { settings: LocalRouterSettings }).settings;
        return Promise.resolve(appSettings(settings));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<LocalRouterPanel />);

    expect(await screen.findByText("Running at http://127.0.0.1:18080")).toBeInTheDocument();
    expect(screen.getByText("23")).toBeInTheDocument();
    expect(screen.getByText("20")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();

    await user.click(screen.getByRole("switch", { name: "Claude Code" }));
    await user.click(screen.getByRole("switch", { name: "Show router switch on the home page" }));
    await user.click(screen.getByRole("switch", { name: "Record request usage" }));
    await user.clear(screen.getByLabelText("Port"));
    await user.type(screen.getByLabelText("Port"), "18081");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("update_local_router_settings", {
        settings: {
          ...enabledSettings,
          show_on_home: false,
          claude_enabled: false,
          record_usage: false,
          listen_port: 18081,
        },
      }),
    );
  });

  it("uses the lifecycle command for the router service switch", async () => {
    const user = userEvent.setup();
    const stoppedStatus: LocalRouterStatus = {
      ...runningStatus,
      desired_enabled: false,
      running: false,
      listen_url: null,
    };
    const startingStatus: LocalRouterStatus = {
      ...stoppedStatus,
      desired_enabled: true,
      starting: true,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "load_app_settings") {
        return Promise.resolve(appSettings({ ...enabledSettings, enabled: false }));
      }
      if (command === "get_local_router_status") return Promise.resolve(stoppedStatus);
      if (command === "set_local_router_enabled") return Promise.resolve(startingStatus);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<LocalRouterPanel />);

    const serviceSwitch = await screen.findByRole("switch", { name: "Local router service" });
    expect(serviceSwitch).not.toBeChecked();
    await user.click(serviceSwitch);

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_local_router_enabled", { enabled: true });
    await waitFor(() => expect(serviceSwitch).toBeChecked());
  });

  it("initializes the failover queue in target order before saving", async () => {
    const user = userEvent.setup();
    const status = { ...runningStatus, targets: claudeTargets };
    vi.mocked(invoke).mockImplementation((command, payload) => {
      if (command === "load_app_settings") return Promise.resolve(appSettings());
      if (command === "get_local_router_status") return Promise.resolve(status);
      if (command === "get_local_router_requests") return Promise.resolve([]);
      if (command === "update_local_router_settings") {
        const next = (payload as { settings: LocalRouterSettings }).settings;
        return Promise.resolve(appSettings(next));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<LocalRouterPanel />);

    const failoverSwitch = await screen.findByRole("switch", { name: "Auto failover" });
    await user.click(failoverSwitch);
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("update_local_router_settings", {
        settings: {
          ...enabledSettings,
          claude: {
            ...enabledSettings.claude,
            auto_failover_enabled: true,
            active_target: "claude",
            failover_queue: ["claude", "claude-backup"],
          },
        },
      }),
    );
  });

  it("filters router targets and highlights failover queue members", async () => {
    const user = userEvent.setup();
    const settings: LocalRouterSettings = {
      ...enabledSettings,
      claude: {
        ...enabledSettings.claude,
        auto_failover_enabled: true,
        active_target: "claude",
        failover_queue: ["claude", "claude-backup"],
      },
    };
    const status = { ...runningStatus, targets: claudeTargets };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "load_app_settings") return Promise.resolve(appSettings(settings));
      if (command === "get_local_router_status") return Promise.resolve(status);
      if (command === "get_local_router_requests") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<LocalRouterPanel />);

    const search = await screen.findByRole("searchbox", { name: "Search router targets" });
    expect(screen.getByRole("article", { name: "Claude Code" })).toBeInTheDocument();
    await user.type(search, "backup");

    expect(screen.queryByRole("article", { name: "Claude Code" })).not.toBeInTheDocument();
    const backup = screen.getByRole("article", { name: "Claude Backup" });
    expect(backup).toBeInTheDocument();
    expect(backup.getAttribute("style")).toContain("var(--success)");
  });

  it("blocks saving when failover only references unavailable targets", async () => {
    const invalidSettings: LocalRouterSettings = {
      ...enabledSettings,
      claude: {
        ...enabledSettings.claude,
        auto_failover_enabled: true,
        active_target: "missing",
        failover_queue: ["missing"],
      },
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "load_app_settings") {
        return Promise.resolve(appSettings(invalidSettings));
      }
      if (command === "get_local_router_status") {
        return Promise.resolve({ ...runningStatus, targets: claudeTargets });
      }
      if (command === "get_local_router_requests") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<LocalRouterPanel />);

    expect(
      await screen.findByText("The failover queue does not contain an available target."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("requires and saves an access token for non-loopback listeners", async () => {
    const user = userEvent.setup();
    const networkSettings: LocalRouterSettings = {
      ...enabledSettings,
      listen_host: "0.0.0.0",
      access_token: "aeroric-0123456789abcdef0123456789abcdef",
    };
    vi.mocked(invoke).mockImplementation((command, payload) => {
      if (command === "load_app_settings") return Promise.resolve(appSettings(networkSettings));
      if (command === "get_local_router_status") return Promise.resolve(runningStatus);
      if (command === "get_local_router_requests") return Promise.resolve([]);
      if (command === "update_local_router_settings") {
        const next = (payload as { settings: LocalRouterSettings }).settings;
        return Promise.resolve(appSettings(next));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<LocalRouterPanel />);

    const tokenInput = await screen.findByLabelText("Access token");
    await user.clear(tokenInput);
    await user.type(tokenInput, "too-short");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    const replacement = "aeroric-fedcba9876543210fedcba9876543210";
    await user.clear(tokenInput);
    await user.type(tokenInput, replacement);
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("update_local_router_settings", {
        settings: { ...networkSettings, access_token: replacement },
      }),
    );
  });
});

describe("HomeLocalRouterToggle", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
  });

  it("stays hidden when the home-page option is disabled", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "load_app_settings") {
        return Promise.resolve(appSettings({ ...enabledSettings, show_on_home: false }));
      }
      if (command === "get_local_router_status") return Promise.resolve(runningStatus);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<HomeLocalRouterToggle />);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("load_app_settings"));
    expect(screen.queryByRole("switch", { name: "Toggle local router" })).not.toBeInTheDocument();
  });

  it("shows current status and toggles the service from the home page", async () => {
    const user = userEvent.setup();
    const stoppedStatus: LocalRouterStatus = {
      ...runningStatus,
      desired_enabled: false,
      running: false,
      listen_url: null,
    };
    let currentSettings = { ...enabledSettings, enabled: false };
    let currentStatus = stoppedStatus;
    vi.mocked(invoke).mockImplementation((command, payload) => {
      if (command === "load_app_settings") {
        return Promise.resolve(appSettings(currentSettings));
      }
      if (command === "get_local_router_status") return Promise.resolve(currentStatus);
      if (command === "set_local_router_enabled") {
        expect(payload).toEqual({ enabled: true });
        currentSettings = { ...currentSettings, enabled: true };
        currentStatus = runningStatus;
        return Promise.resolve(currentStatus);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderWithI18n(<HomeLocalRouterToggle />);

    const homeSwitch = await screen.findByRole("switch", { name: "Toggle local router" });
    expect(homeSwitch).not.toBeChecked();
    expect(homeSwitch).toHaveAttribute("title", "Local router: Stopped");

    await user.click(homeSwitch);

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_local_router_enabled", { enabled: true });
    await waitFor(() => expect(homeSwitch).toBeChecked());
  });
});
