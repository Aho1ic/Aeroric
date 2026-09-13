import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { SshConnection } from "../types";
import {
  deriveRemoteProjectName,
  SshProjectPage,
  sshProjectInputForConnection,
} from "../components/ssh/SshProjectDialog";
import { SshConnectionList } from "../components/ssh/SshConnectionList";
import { SshWorkspace } from "../components/ssh/SshWorkspace";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke,
}));

function connection(remotePath?: string): SshConnection {
  return {
    id: "conn-1",
    name: "Prod",
    host: "example.com",
    port: 22,
    username: "deploy",
    remotePath,
    createdAt: 1,
  };
}

describe("SSH project opening", () => {
  beforeEach(() => {
    Element.prototype.hasPointerCapture ??= () => false;
    Element.prototype.scrollIntoView ??= () => {};
  });

  it("derives a remote project name from the final path segment", () => {
    expect(deriveRemoteProjectName("/srv/apps/aeroric/", "Prod")).toBe("aeroric");
    expect(deriveRemoteProjectName("   ", "Prod")).toBe("Prod");
  });

  it("uses the SSH connection name for the opened remote project", () => {
    expect(sshProjectInputForConnection(connection("/srv/apps/aeroric"))).toEqual({
      connectionId: "conn-1",
      remotePath: "/srv/apps/aeroric",
      name: "Prod",
    });
    expect(sshProjectInputForConnection(connection())).toBeNull();
  });

  it("copies the saved SSH password from a project card", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    // 连接记录只带 hasPassword,明文按需经 get_ssh_connection_password 取。
    invoke.mockResolvedValue("secret-pass");

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [{ ...connection("/srv/apps/aeroric"), hasPassword: true }],
          onConnectionsChange: () => {},
          onClose: () => {},
          onOpen: () => {},
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "Copy password" }));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("secret-pass"));
    expect(screen.getByRole("button", { name: "Copy password" })).toHaveAttribute(
      "data-copied",
      "true",
    );
  });

  it("opens a terminal, not a project, from the home SSH context menu", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    const onOpenSftp = vi.fn();
    const onOpenTerminal = vi.fn();
    const sshPage = () => screen.getByRole("button", { name: /Prod.*example\.com:22/ });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          onConnectionsChange: vi.fn(),
          onClose: vi.fn(),
          onOpen,
          onOpenSftp,
          onOpenTerminal,
        }),
      ),
    );

    // Connect → SSH 是"用 ssh 连上去",不是建项目。带 remotePath 的连接也一样。
    fireEvent.contextMenu(sshPage(), { clientX: 80, clientY: 80 });
    await user.click(screen.getByRole("menuitem", { name: "Connect" }));
    await user.click(screen.getByRole("menuitem", { name: "SSH" }));
    expect(onOpenTerminal).toHaveBeenCalledWith(expect.objectContaining({ id: "conn-1" }));
    expect(onOpen).not.toHaveBeenCalled();

    fireEvent.contextMenu(sshPage(), { clientX: 80, clientY: 80 });
    await user.click(screen.getByRole("menuitem", { name: "Connect" }));
    await user.click(screen.getByRole("menuitem", { name: "SFTP" }));
    expect(onOpenSftp).toHaveBeenCalledWith(expect.objectContaining({ id: "conn-1" }));
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("opens a terminal on card double-click and only creates a project from the footer", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    const onOpenTerminal = vi.fn();

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          onConnectionsChange: vi.fn(),
          onClose: vi.fn(),
          onOpen,
          onOpenTerminal,
        }),
      ),
    );

    await user.dblClick(screen.getByRole("button", { name: /Prod.*example\.com:22/ }));
    expect(onOpenTerminal).toHaveBeenCalledWith(expect.objectContaining({ id: "conn-1" }));
    expect(onOpen).not.toHaveBeenCalled();

    // 页脚按钮是唯一建项目的手势,不能被一起改掉。
    await user.click(screen.getByRole("button", { name: "Open SSH project" }));
    expect(onOpen).toHaveBeenCalledWith({
      connectionId: "conn-1",
      remotePath: "/srv/apps/aeroric",
      name: "Prod",
    });
  });

  it("copies a paste-ready SSH command from the home SSH context menu without the password", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [{ ...connection("/srv/apps/aeroric"), port: 2222, hasPassword: true }],
          onConnectionsChange: vi.fn(),
          onClose: vi.fn(),
          onOpen: vi.fn(),
        }),
      ),
    );

    fireEvent.contextMenu(screen.getByRole("button", { name: /Prod.*example\.com:2222/ }), {
      clientX: 80,
      clientY: 80,
    });
    await user.click(screen.getByRole("menuitem", { name: "Copy SSH command" }));
    // 复制命令不夹带 SSHPASS,明文经独立的"Copy password"动作按需取。
    expect(writeText).toHaveBeenCalledWith("ssh -p 2222 deploy@example.com");
  });

  it("physically deletes a connection from the home SSH context menu", async () => {
    const user = userEvent.setup();
    const onConnectionsChange = vi.fn();
    const onDeleteConnection = vi.fn();

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          onConnectionsChange,
          onDeleteConnection,
          onClose: vi.fn(),
          onOpen: vi.fn(),
        }),
      ),
    );

    fireEvent.contextMenu(screen.getByRole("button", { name: /Prod.*example\.com:22/ }), {
      clientX: 80,
      clientY: 80,
    });
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));

    expect(onDeleteConnection).toHaveBeenCalledWith("conn-1");
    expect(onConnectionsChange).not.toHaveBeenCalled();
  });

  it("disables project-card password copy when no password is saved", () => {
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          onConnectionsChange: () => {},
          onClose: () => {},
          onOpen: () => {},
        }),
      ),
    );

    expect(screen.getByRole("button", { name: "Copy password" })).toBeDisabled();
  });

  it("uses a real dropdown for existing groups when creating an SSH connection", async () => {
    const user = userEvent.setup();

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          groups: ["Production", "Staging"],
          onConnectionsChange: vi.fn(),
          onClose: vi.fn(),
          onOpen: vi.fn(),
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "New connection" }));

    const groupSelect = screen.getByRole("combobox", { name: "Group" });
    expect(groupSelect).toHaveClass("radix-select-trigger");
    groupSelect.focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("option", { name: "Production" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Staging" })).toBeInTheDocument();
  });

  it("shows the SSH password storage hint without a bordered note box", async () => {
    const user = userEvent.setup();

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          onConnectionsChange: vi.fn(),
          onClose: vi.fn(),
          onOpen: vi.fn(),
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "Edit" }));

    expect(
      screen.getByText(
        "Passwords are stored as plaintext in a local owner-only file under Aeroric app data (Unix mode 0600), not in the OS keychain. They are passed to system SSH via sshpass environment variables. Prefer SSH keys when possible; leave the password blank to use interactive prompts instead.",
      ),
    ).toHaveStyle({ borderStyle: "none" });
  });

  it("shows the new group hint without a bordered note box", async () => {
    const user = userEvent.setup();

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          onConnectionsChange: vi.fn(),
          onClose: vi.fn(),
          onOpen: vi.fn(),
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "New group" }));

    expect(screen.getByText("A group is saved when you create a connection in it.")).toHaveStyle({
      borderStyle: "none",
    });
  });

  it("renders SSH edit dialogs above project split panes and terminals", async () => {
    const user = userEvent.setup();

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [connection("/srv/apps/aeroric")],
          onConnectionsChange: vi.fn(),
          onClose: vi.fn(),
          onOpen: vi.fn(),
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "Edit" }));

    const dialog = screen.getByRole("dialog", { name: "Edit connection" });
    const overlay = dialog.parentElement as HTMLElement;
    expect(Number(overlay.style.zIndex)).toBeGreaterThan(2000);
    expect(overlay.parentElement).toBe(document.body);
  });

  it("copies the saved SSH password from a sidebar SSH connection card", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    invoke.mockResolvedValue("card-secret");

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshConnectionList, {
          connections: [{ ...connection("/srv/apps/aeroric"), hasPassword: true }],
          selectedId: null,
          onSelect: vi.fn(),
          onCreate: vi.fn(),
          onEdit: vi.fn(),
          onDelete: vi.fn(),
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "Copy password" }));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("card-secret"));
  });

  it("opens SFTP from the sidebar SSH connection context menu", async () => {
    const user = userEvent.setup();
    const onConnect = vi.fn();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshConnectionList, {
          connections: [connection("/srv/apps/aeroric")],
          selectedId: null,
          onSelect: vi.fn(),
          onCreate: vi.fn(),
          onEdit: vi.fn(),
          onDelete: vi.fn(),
          onConnect,
        }),
      ),
    );

    fireEvent.contextMenu(screen.getByRole("button", { name: /Prod.*example\.com:22/ }), {
      clientX: 80,
      clientY: 80,
    });
    await user.click(screen.getByRole("menuitem", { name: "Connect" }));
    await user.click(screen.getByRole("menuitem", { name: "SFTP" }));

    expect(onConnect).toHaveBeenCalledWith(expect.objectContaining({ id: "conn-1" }), "sftp");
  });

  it("deletes from the sidebar SSH connection context menu", async () => {
    const user = userEvent.setup();
    const onDelete = vi.fn();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshConnectionList, {
          connections: [connection("/srv/apps/aeroric")],
          selectedId: null,
          onSelect: vi.fn(),
          onCreate: vi.fn(),
          onEdit: vi.fn(),
          onDelete,
        }),
      ),
    );

    fireEvent.contextMenu(screen.getByRole("button", { name: /Prod.*example\.com:22/ }), {
      clientX: 80,
      clientY: 80,
    });
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));

    expect(onDelete).toHaveBeenCalledWith("conn-1");
  });

  it("copies the saved SSH password from a project page SSH workspace card", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    invoke.mockResolvedValue("workspace-secret");

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshWorkspace, {
          connections: [{ ...connection("/srv/apps/aeroric"), hasPassword: true }],
          onConnectionsChange: vi.fn(),
          active: true,
          themeVariant: "light",
          terminalFontSize: 11,
          monoFontFamily: "monospace",
          layout: "full",
          onLayoutChange: vi.fn(),
        }),
      ),
    );

    const copyButton = screen.getByRole("button", { name: "Copy password" });
    await user.click(copyButton);

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("workspace-secret"));
    expect(copyButton).toHaveAttribute("data-copied", "true");
  });
});
