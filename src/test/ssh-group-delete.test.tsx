import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshTerminalPanel } from "../components/ssh/SshTerminalPanel";
import { SSH_GROUPS_STORAGE_KEY, resetSshGroupNamesCache } from "../components/ssh/sshGroups";
import { DEFAULT_TERMINAL_FONT_SIZE, type SshConnection } from "../types";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invokeMock(command, args),
  isTauri: () => false,
  Channel: class {
    onmessage: ((data: string) => void) | null = null;
  },
}));

vi.mock("../components/terminalRuntime", () => ({
  createTerminalRuntime: () => ({
    term: { cols: 80, rows: 24, writeln: vi.fn() },
    writer: { write: vi.fn() },
    fit: vi.fn(),
    focus: vi.fn(),
    updateTheme: vi.fn(),
    updateFontSize: vi.fn(),
    updateFontFamily: vi.fn(),
    dispose: vi.fn(),
  }),
}));

function connection(overrides: Partial<SshConnection> = {}): SshConnection {
  return {
    id: "conn-1",
    name: "prod",
    host: "prod.example.com",
    port: 22,
    username: "deploy",
    createdAt: 1,
    ...overrides,
  };
}

function renderPanel(connections: SshConnection[], onConnectionsChange = vi.fn()) {
  render(
    <I18nProvider>
      <SshTerminalPanel
        connections={connections}
        onConnectionsChange={onConnectionsChange}
        active={false}
        width={320}
        themeVariant="dark"
        terminalFontSize={DEFAULT_TERMINAL_FONT_SIZE}
        monoFontFamily="jetbrains-mono"
      />
    </I18nProvider>,
  );
  return { onConnectionsChange };
}

describe("SSH group deletion in the sidebar panel", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    localStorage.clear();
    // 名单快照是模块级的,清 localStorage 清不掉它 —— 不重置会把上一条用例建的
    // 分组带进下一条。
    resetSshGroupNamesCache();
  });

  it("removes an empty group the user just created", async () => {
    const user = userEvent.setup();
    renderPanel([]);

    await user.click(screen.getByRole("button", { name: "New group" }));
    await user.type(screen.getByLabelText("Group name"), "edge");
    await user.keyboard("{Enter}");

    expect(screen.getByText("edge")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Delete group/ }));

    await waitFor(() => {
      expect(screen.queryByText("edge")).not.toBeInTheDocument();
    });
    expect(JSON.parse(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "[]")).toEqual([]);
  });

  /// 用户报的就是这条路径:新建分组后右键删除。
  it("removes a just-created group through the context menu", async () => {
    const user = userEvent.setup();
    renderPanel([]);

    await user.click(screen.getByRole("button", { name: "New group" }));
    await user.type(screen.getByLabelText("Group name"), "edge");
    await user.keyboard("{Enter}");

    fireEvent.contextMenu(screen.getByText("edge"), { clientX: 40, clientY: 40 });
    await user.click(screen.getByRole("menuitem", { name: "Delete group" }));

    await waitFor(() => {
      expect(screen.queryByText("edge")).not.toBeInTheDocument();
    });
    expect(JSON.parse(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "[]")).toEqual([]);
  });

  it("renames a group and carries its connections over", async () => {
    const user = userEvent.setup();
    const onConnectionsChange = vi.fn();
    renderPanel([connection({ group: "edge" })], onConnectionsChange);

    fireEvent.contextMenu(screen.getByText("edge"), { clientX: 40, clientY: 40 });
    await user.click(screen.getByRole("menuitem", { name: "Rename group" }));
    const input = screen.getByLabelText("Rename group");
    await user.clear(input);
    await user.type(input, "border");
    await user.keyboard("{Enter}");

    expect(onConnectionsChange).toHaveBeenCalledTimes(1);
    const next = onConnectionsChange.mock.calls[0][0] as SshConnection[];
    expect(next[0]).toMatchObject({ id: "conn-1", group: "border" });
  });

  /// 重命名一个只存在于连接上的分组时,新名字要进名单,否则移走最后一条连接就没了。
  it("registers the new name so an emptied group survives", async () => {
    const user = userEvent.setup();
    renderPanel([connection({ group: "edge" })]);

    fireEvent.contextMenu(screen.getByText("edge"), { clientX: 40, clientY: 40 });
    await user.click(screen.getByRole("menuitem", { name: "Rename group" }));
    const input = screen.getByLabelText("Rename group");
    await user.clear(input);
    await user.type(input, "border");
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(JSON.parse(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "[]")).toContain("border");
    });
  });

  it("removes a group that still holds connections and keeps them", async () => {
    const user = userEvent.setup();
    const onConnectionsChange = vi.fn();
    renderPanel([connection({ group: "edge" })], onConnectionsChange);

    expect(screen.getByText("edge")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Delete group/ }));

    expect(onConnectionsChange).toHaveBeenCalledTimes(1);
    const next = onConnectionsChange.mock.calls[0][0] as SshConnection[];
    expect(next).toHaveLength(1);
    expect(next[0]).not.toHaveProperty("group");
  });
});
