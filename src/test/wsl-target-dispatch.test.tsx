import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { GitChanges } from "../components/GitChanges";
import { FileExplorer } from "../components/FileExplorer";
import { createWslShellId } from "../components/wsl/WslTerminalPanel";
import {
  targetCommand,
  targetFileArgs,
  targetProjectArgs,
  taskCommandName,
} from "../projectTarget";
import type { LocalTarget, SshConnection, SshTarget, WslTarget } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: vi.fn() }));

class ResizeObserverMock {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}

vi.stubGlobal("ResizeObserver", ResizeObserverMock);

const connection: SshConnection = {
  id: "ssh-1",
  name: "prod",
  host: "example.com",
  port: 22,
  username: "deploy",
  createdAt: 1,
};

const local: LocalTarget = { kind: "local", path: "/Users/me/app" };
const ssh: SshTarget = { kind: "ssh", connection, projectPath: "/srv/app" };
const wsl: WslTarget = { kind: "wsl", distribution: "Ubuntu", projectPath: "/home/dev/app" };

describe("三目标命令分派", () => {
  it("按 target 选择命令名", () => {
    const pick = (target: LocalTarget | SshTarget | WslTarget) =>
      targetCommand(target, "read_dir_entries", "remote_read_dir_entries", "wsl_read_dir_entries");

    expect(pick(local)).toBe("read_dir_entries");
    expect(pick(ssh)).toBe("remote_read_dir_entries");
    expect(pick(wsl)).toBe("wsl_read_dir_entries");
  });

  it("按 target 组装项目参数", () => {
    expect(targetProjectArgs(local)).toEqual({ projectPath: "/Users/me/app" });
    expect(targetProjectArgs(ssh)).toEqual({ connection, remoteProjectPath: "/srv/app" });
    expect(targetProjectArgs(wsl)).toEqual({
      distribution: "Ubuntu",
      linuxProjectPath: "/home/dev/app",
    });
  });

  it("按 target 组装文件参数", () => {
    expect(targetFileArgs(local, "/Users/me/app/src")).toEqual({
      path: "/Users/me/app/src",
      projectPath: "/Users/me/app",
    });
    expect(targetFileArgs(ssh, "/srv/app/src")).toEqual({
      connection,
      remotePath: "/srv/app/src",
      remoteProjectPath: "/srv/app",
    });
    expect(targetFileArgs(wsl, "/home/dev/app/src")).toEqual({
      distribution: "Ubuntu",
      linuxPath: "/home/dev/app/src",
      linuxProjectPath: "/home/dev/app",
    });
  });

  it("任务 run/resume/cancel 命令名按位置分派", () => {
    expect(
      ["run", "resume", "cancel"].map((action) => taskCommandName("local", action as "run")),
    ).toEqual(["run_task", "resume_task", "cancel_task"]);
    expect(
      ["run", "resume", "cancel"].map((action) => taskCommandName("ssh", action as "run")),
    ).toEqual(["run_remote_task", "resume_remote_task", "cancel_remote_task"]);
    expect(
      ["run", "resume", "cancel"].map((action) => taskCommandName("wsl", action as "run")),
    ).toEqual(["run_wsl_task", "resume_wsl_task", "cancel_wsl_task"]);
  });

  it("WSL 终端 shellId 使用 wsl: 前缀并转义发行版名", () => {
    const id = createWslShellId("project-1", "Ubuntu 22.04/dev");
    expect(id.startsWith("wsl:project-1:Ubuntu_22.04_dev:")).toBe(true);
  });
});

describe("WSL 文件与 Git 命令分派", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("WSL 项目的文件浏览走 wsl_* 命令并带发行版参数", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_wsl_project_config") return Promise.resolve(null);
      if (command === "wsl_read_dir_entries") {
        return Promise.resolve([
          {
            name: "app.tsx",
            path: "/home/dev/app/app.tsx",
            is_dir: false,
            extension: "tsx",
            is_gitignored: false,
            modifiedAtMs: 1,
          },
        ]);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <FileExplorer
          projectPath="/home/dev/app"
          projectName="app"
          onFileSelect={vi.fn()}
          themeVariant="light"
          remote={wsl}
        />
      </I18nProvider>,
    );

    expect(await screen.findByText("app.tsx")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("wsl_read_dir_entries", {
      distribution: "Ubuntu",
      linuxPath: "/home/dev/app",
      linuxProjectPath: "/home/dev/app",
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("read_wsl_project_config", {
      distribution: "Ubuntu",
      linuxProjectPath: "/home/dev/app",
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("read_dir_entries", expect.anything());
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "remote_read_dir_entries",
      expect.anything(),
    );
  });

  it("WSL 项目的 Git 读写走 wsl_git_* 命令", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "wsl_git_changes") {
        return Promise.resolve([{ path: "src/App.tsx", status: "M", staged: false }]);
      }
      if (command === "wsl_git_stage") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    });

    render(
      <I18nProvider>
        <GitChanges
          projectPath="/home/dev/app"
          currentTaskCreatedAt={null}
          onFileSelect={vi.fn()}
          remote={wsl}
        />
      </I18nProvider>,
    );

    expect(await screen.findByText("App.tsx")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("wsl_git_changes", {
      distribution: "Ubuntu",
      linuxProjectPath: "/home/dev/app",
    });

    const row = screen.getByText("App.tsx").closest("[role='button']") as HTMLElement;
    fireEvent.mouseEnter(row);
    await user.click(within(row).getByTitle("Stage"));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("wsl_git_stage", {
        distribution: "Ubuntu",
        linuxProjectPath: "/home/dev/app",
        filePath: "src/App.tsx",
      });
    });
  });
});
