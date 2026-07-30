import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { WslProjectDialog } from "../components/wsl/WslProjectDialog";
import { normalizeWslProjectPath, upsertWslProject } from "../appProjectState";
import type { Project, WslDistribution, WslDistributionProbe, WslSettings } from "../types";
import { resolveProjectLocation, wslProjectPath } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const distributions: WslDistribution[] = [
  { name: "Ubuntu-22.04", state: "Running", version: 2, isDefault: true },
  { name: "Debian", state: "Stopped", version: 2, isDefault: false },
];

const settings: WslSettings = { distributions: {} };

const probe: WslDistributionProbe = {
  distribution: "Ubuntu-22.04",
  state: "Running",
  version: 2,
  home: "/home/dev",
  shell: "/bin/bash",
  user: "dev",
};

function mockBackend(overrides: Record<string, unknown> = {}) {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command in overrides) {
      const value = overrides[command];
      return value instanceof Error ? Promise.reject(value) : Promise.resolve(value);
    }
    switch (command) {
      case "list_wsl_distributions":
        return Promise.resolve(distributions.map((item) => ({ ...item })));
      case "load_wsl_settings":
        return Promise.resolve(structuredClone(settings));
      case "probe_wsl_distribution":
        return Promise.resolve({ ...probe });
      case "validate_wsl_project_path":
        return Promise.resolve(null);
      default:
        return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    }
  });
}

function renderDialog(onOpen = vi.fn(), onClose = vi.fn()) {
  render(
    <I18nProvider>
      <WslProjectDialog open onOpen={onOpen} onClose={onClose} />
    </I18nProvider>,
  );
  return { onOpen, onClose };
}

describe("WslProjectDialog", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockBackend();
  });

  it("列出发行版, 预选默认项并用 HOME 预填路径", async () => {
    renderDialog();

    expect(await screen.findByRole("button", { name: "Ubuntu-22.04" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Debian" })).toHaveAttribute("aria-pressed", "false");
    await waitFor(() => {
      expect(screen.getByRole("textbox")).toHaveValue("/home/dev");
    });
    expect(screen.getByText("dev · /bin/bash · HOME /home/dev")).toBeTruthy();
  });

  it("校验绝对路径后调用后端并回传项目输入", async () => {
    const user = userEvent.setup();
    const { onOpen, onClose } = renderDialog();
    const input = await screen.findByRole("textbox");
    await waitFor(() => expect(input).toHaveValue("/home/dev"));

    await user.clear(input);
    await user.type(input, "/home/dev/app/");
    await user.click(screen.getByRole("button", { name: "Open WSL Project" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("validate_wsl_project_path", {
        distribution: "Ubuntu-22.04",
        linuxPath: "/home/dev/app/",
      });
    });
    expect(onOpen).toHaveBeenCalledWith({
      name: "app",
      distribution: "Ubuntu-22.04",
      linuxPath: "/home/dev/app",
    });
    expect(onClose).toHaveBeenCalled();
  });

  it("拒绝非绝对路径, 不调用后端", async () => {
    const user = userEvent.setup();
    const { onOpen } = renderDialog();
    const input = await screen.findByRole("textbox");
    await waitFor(() => expect(input).toHaveValue("/home/dev"));

    await user.clear(input);
    await user.type(input, "home/dev/app");
    await user.click(screen.getByRole("button", { name: "Open WSL Project" }));

    expect(
      await screen.findByText("Choose a distribution and enter an absolute Linux path."),
    ).toBeTruthy();
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "validate_wsl_project_path",
      expect.anything(),
    );
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("后端校验失败时展示错误且不打开项目", async () => {
    const user = userEvent.setup();
    mockBackend({ validate_wsl_project_path: new Error("/home/dev/missing is not a directory") });
    const { onOpen, onClose } = renderDialog();
    const input = await screen.findByRole("textbox");
    await waitFor(() => expect(input).toHaveValue("/home/dev"));

    await user.click(screen.getByRole("button", { name: "Open WSL Project" }));

    expect(await screen.findByText(/is not a directory/)).toBeTruthy();
    expect(onOpen).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("没有发行版时提示并禁用打开按钮", async () => {
    mockBackend({ list_wsl_distributions: [] });
    renderDialog();

    expect(await screen.findByText("No WSL distributions were found.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open WSL Project" })).toBeDisabled();
  });

  it("枚举失败时展示错误, 刷新后恢复列表", async () => {
    const user = userEvent.setup();
    mockBackend({ list_wsl_distributions: new Error("wsl.exe exited with code 1") });
    renderDialog();
    expect(await screen.findByText(/wsl.exe exited with code 1/)).toBeTruthy();

    mockBackend();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByRole("button", { name: "Ubuntu-22.04" })).toBeTruthy();
  });
});

describe("WSL 项目持久化", () => {
  const now = 1700000000000;

  it("生成可持久化的 wsl:// 路径并保留发行版名转义", () => {
    expect(wslProjectPath("Ubuntu-22.04", "/home/dev/app")).toBe("wsl://Ubuntu-22.04/home/dev/app");
    expect(wslProjectPath("My Distro", "home/dev")).toBe("wsl://My%20Distro/home/dev");
  });

  it("归一化 Linux 路径, 根路径保持不变", () => {
    expect(normalizeWslProjectPath(" /home/dev/app/ ")).toBe("/home/dev/app");
    expect(normalizeWslProjectPath("/")).toBe("/");
    expect(normalizeWslProjectPath("home/dev")).toBe("/home/dev");
  });

  it("新建 WSL 项目时置顶并写入 wsl location", () => {
    const local: Project = {
      id: "p-local",
      name: "local",
      path: "/Users/me/app",
      lastOpenedAt: 1,
      orderIndex: 0,
    };
    const result = upsertWslProject(
      [local],
      { name: "app", distribution: "Ubuntu-22.04", linuxPath: "/home/dev/app/" },
      now,
    );

    expect(result.reused).toBe(false);
    expect(result.project).toMatchObject({
      id: `${now}`,
      name: "app",
      path: "wsl://Ubuntu-22.04/home/dev/app",
      location: { kind: "wsl", distribution: "Ubuntu-22.04", linuxPath: "/home/dev/app" },
      lastOpenedAt: now,
    });
    expect(result.projects.map((project) => project.id)).toEqual([`${now}`, "p-local"]);
    expect(result.projects.map((project) => project.orderIndex)).toEqual([0, 1]);
    expect(resolveProjectLocation(result.project)).toEqual({
      kind: "wsl",
      distribution: "Ubuntu-22.04",
      linuxPath: "/home/dev/app",
    });
  });

  it("同发行版同路径复用既有项目而不重复创建", () => {
    const existing: Project = {
      id: "p-wsl",
      name: "app",
      path: "wsl://Ubuntu-22.04/home/dev/app",
      location: { kind: "wsl", distribution: "Ubuntu-22.04", linuxPath: "/home/dev/app" },
      lastOpenedAt: 1,
      orderIndex: 3,
    };
    const result = upsertWslProject(
      [existing],
      { name: "renamed", distribution: "Ubuntu-22.04", linuxPath: "/home/dev/app" },
      now,
    );

    expect(result.reused).toBe(true);
    expect(result.projects).toHaveLength(1);
    expect(result.project).toMatchObject({
      id: "p-wsl",
      name: "app",
      orderIndex: 3,
      lastOpenedAt: now,
    });
  });

  it("不同发行版的同一路径视为不同项目", () => {
    const existing: Project = {
      id: "p-wsl",
      name: "app",
      path: "wsl://Ubuntu-22.04/home/dev/app",
      location: { kind: "wsl", distribution: "Ubuntu-22.04", linuxPath: "/home/dev/app" },
      lastOpenedAt: 1,
      orderIndex: 0,
    };
    const result = upsertWslProject(
      [existing],
      { name: "app", distribution: "Debian", linuxPath: "/home/dev/app" },
      now,
    );

    expect(result.reused).toBe(false);
    expect(result.projects).toHaveLength(2);
    expect(result.project.path).toBe("wsl://Debian/home/dev/app");
  });
});
