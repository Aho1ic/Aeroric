import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SftpPanel } from "../components/sftp/SftpPanel";
import type { SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn().mockResolvedValue(true),
}));

const connections: SshConnection[] = [
  {
    id: "conn-1",
    name: "Staging",
    host: "staging.example.com",
    port: 22,
    username: "deploy",
    remotePath: "/srv/staging",
    createdAt: 1,
  },
  {
    id: "conn-2",
    name: "Production",
    host: "prod.example.com",
    port: 22,
    username: "deploy",
    remotePath: "/srv/app",
    createdAt: 2,
  },
];

describe("SftpPanel", () => {
  it("defaults to Local on the left and the current SSH project connection on the right", () => {
    render(
      <I18nProvider>
        <SftpPanel
          sshConnections={connections}
          localDefaultPath="/Users/me"
          active
          themeVariant="light"
          currentSshConnectionId="conn-2"
        />
      </I18nProvider>,
    );

    const triggers = screen.getAllByLabelText("Location");
    expect(triggers[0]).toHaveTextContent("Local");
    expect(triggers[1]).toHaveTextContent("Production");
    expect(screen.getByDisplayValue("/srv/app")).toBeInTheDocument();
  });

  it("shows transfer progress and task details while copying files", async () => {
    const user = userEvent.setup();
    const transferControl: { finish?: () => void } = {};
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "sftp_read_dir") {
        const endpoint = (args as { endpoint: { kind: string } }).endpoint;
        if (endpoint.kind === "local") {
          return Promise.resolve([
            {
              name: "README.md",
              path: "/Users/me/README.md",
              isDir: false,
              extension: "md",
              size: 128,
              modifiedAtMs: null,
            },
          ]);
        }
        return Promise.resolve([]);
      }
      if (command === "sftp_copy_paths") {
        return new Promise((resolve) => {
          transferControl.finish = () => resolve(undefined);
        });
      }
      return Promise.resolve([]);
    });

    render(
      <I18nProvider>
        <SftpPanel
          sshConnections={connections}
          localDefaultPath="/Users/me"
          active
          themeVariant="light"
          currentSshConnectionId="conn-2"
        />
      </I18nProvider>,
    );

    const openButtons = screen.getAllByRole("button", { name: "Open pane" });
    await user.click(openButtons[0]);
    await user.click(openButtons[1]);
    await user.click(await screen.findByText("README.md"));
    await user.click(screen.getAllByRole("button", { name: "Copy" })[0]);

    const progressButton = await screen.findByRole("button", { name: /Transfer progress/i });
    expect(progressButton).toHaveAttribute("aria-busy", "true");

    await user.hover(progressButton);
    expect(screen.getByText(/Copying README.md/i)).toBeInTheDocument();

    transferControl.finish?.();
    await waitFor(() => expect(progressButton).toHaveAttribute("aria-busy", "false"));
    await user.hover(progressButton);
    expect(screen.getByText(/Copied README.md/i)).toBeInTheDocument();
  });

  it("defaults panes to modification time descending sort", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "sftp_read_dir") {
        return Promise.resolve([
          {
            name: "old.txt",
            path: "/Users/me/old.txt",
            isDir: false,
            extension: "txt",
            size: 1,
            modifiedAtMs: 100,
          },
          {
            name: "new.txt",
            path: "/Users/me/new.txt",
            isDir: false,
            extension: "txt",
            size: 1,
            modifiedAtMs: 300,
          },
        ]);
      }
      return Promise.resolve([]);
    });

    render(
      <I18nProvider>
        <SftpPanel
          sshConnections={connections}
          localDefaultPath="/Users/me"
          active
          themeVariant="light"
          currentSshConnectionId="conn-2"
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getAllByRole("button", { name: "Open pane" })[0]);

    const rows = await screen.findAllByText(/\.txt$/);
    expect(rows.map((row) => row.textContent)).toEqual(["new.txt", "old.txt"]);
    expect(screen.getAllByRole("button", { name: "Modified" })[0]).toHaveClass("active");
    expect(screen.getAllByRole("button", { name: "Desc" })[0]).toBeInTheDocument();
  });
});
