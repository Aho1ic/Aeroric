import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { SshConnection } from "../types";
import {
  deriveRemoteProjectName,
  SshProjectPage,
  sshProjectInputForConnection,
} from "../components/ssh/SshProjectDialog";
import { SshConnectionList } from "../components/ssh/SshConnectionList";
import { SshWorkspace } from "../components/ssh/SshWorkspace";

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

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(SshProjectPage, {
          connections: [{ ...connection("/srv/apps/aeroric"), password: "secret-pass" }],
          onConnectionsChange: () => {},
          onClose: () => {},
          onOpen: () => {},
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "Copy password" }));

    expect(writeText).toHaveBeenCalledWith("secret-pass");
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

  it("copies the saved SSH password from a sidebar SSH connection card", async () => {
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
        React.createElement(SshConnectionList, {
          connections: [{ ...connection("/srv/apps/aeroric"), password: "card-secret" }],
          selectedId: null,
          onSelect: vi.fn(),
          onCreate: vi.fn(),
          onEdit: vi.fn(),
          onDelete: vi.fn(),
        }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "Copy password" }));

    expect(writeText).toHaveBeenCalledWith("card-secret");
  });

  it("copies the saved SSH password from a project page SSH workspace card", async () => {
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
        React.createElement(SshWorkspace, {
          connections: [{ ...connection("/srv/apps/aeroric"), password: "workspace-secret" }],
          onConnectionsChange: vi.fn(),
          active: true,
          themeVariant: "light",
          terminalFontSize: 11,
          monoFontFamily: "monospace",
        }),
      ),
    );

    const copyButton = screen.getByRole("button", { name: "Copy password" });
    await user.click(copyButton);

    expect(writeText).toHaveBeenCalledWith("workspace-secret");
    expect(copyButton).toHaveAttribute("data-copied", "true");
  });
});
