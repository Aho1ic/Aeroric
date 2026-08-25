import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshWorkspace } from "../components/ssh/SshWorkspace";
import {
  SSH_GROUPS_STORAGE_KEY,
  resetSshGroupNamesCache,
  saveSshGroupNames,
} from "../components/ssh/sshGroups";
import { DEFAULT_TERMINAL_FONT_SIZE, type SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
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

function renderWorkspace(connections: SshConnection[], onConnectionsChange = vi.fn()) {
  render(
    <I18nProvider>
      <SshWorkspace
        connections={connections}
        onConnectionsChange={onConnectionsChange}
        active={false}
        themeVariant="dark"
        terminalFontSize={DEFAULT_TERMINAL_FONT_SIZE}
        monoFontFamily="jetbrains-mono"
        layout="full"
        onLayoutChange={vi.fn()}
      />
    </I18nProvider>,
  );
  return { onConnectionsChange };
}

describe("SSH group management on the welcome-page workspace", () => {
  beforeEach(() => {
    localStorage.clear();
    resetSshGroupNamesCache();
  });

  /// 这是用户最初报的场景:分组在右侧栏建好,却在这个视图里删不掉。
  it("shows an empty group created elsewhere and deletes it from the card view", async () => {
    const user = userEvent.setup();
    saveSshGroupNames(["edge"]);
    renderWorkspace([connection()]);

    expect(screen.getByText("edge")).toBeInTheDocument();

    fireEvent.contextMenu(screen.getByText("edge"), { clientX: 30, clientY: 30 });
    await user.click(screen.getByRole("menuitem", { name: "Delete group" }));

    await waitFor(() => {
      expect(screen.queryByText("edge")).not.toBeInTheDocument();
    });
    expect(JSON.parse(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "[]")).toEqual([]);
  });

  it("renames a group here and carries its connections over", async () => {
    const user = userEvent.setup();
    saveSshGroupNames(["edge"]);
    const { onConnectionsChange } = renderWorkspace([connection({ group: "edge" })]);

    fireEvent.contextMenu(screen.getByText("edge"), { clientX: 30, clientY: 30 });
    await user.click(screen.getByRole("menuitem", { name: "Rename group" }));
    const input = screen.getByLabelText("Rename group");
    await user.clear(input);
    await user.type(input, "border");
    await user.keyboard("{Enter}");

    const next = onConnectionsChange.mock.calls[0][0] as SshConnection[];
    expect(next[0]).toMatchObject({ id: "conn-1", group: "border" });
    await waitFor(() => {
      expect(JSON.parse(localStorage.getItem(SSH_GROUPS_STORAGE_KEY) ?? "[]")).toEqual(["border"]);
    });
  });

  it("keeps the connections when a group holding them is deleted", async () => {
    const user = userEvent.setup();
    saveSshGroupNames(["edge"]);
    const { onConnectionsChange } = renderWorkspace([connection({ group: "edge" })]);

    fireEvent.contextMenu(screen.getByText("edge"), { clientX: 30, clientY: 30 });
    await user.click(screen.getByRole("menuitem", { name: "Delete group" }));

    const next = onConnectionsChange.mock.calls[0][0] as SshConnection[];
    expect(next).toHaveLength(1);
    expect(next[0]).not.toHaveProperty("group");
  });

  /// 默认分组只是未分组连接的容器,不该给出改名/删除入口。
  it("refuses the context menu on the default group", () => {
    renderWorkspace([connection()]);

    fireEvent.contextMenu(screen.getByText("Default"), { clientX: 30, clientY: 30 });

    expect(screen.queryByRole("menuitem", { name: "Delete group" })).not.toBeInTheDocument();
  });
});
