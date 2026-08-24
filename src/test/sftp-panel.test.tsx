import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SftpPanel } from "../components/sftp/SftpPanel";
import type { SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("../lib/appDialog", () => ({
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
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue([]);
    if (!Element.prototype.hasPointerCapture) {
      Element.prototype.hasPointerCapture = () => false;
    }
    if (!Element.prototype.releasePointerCapture) {
      Element.prototype.releasePointerCapture = () => {};
    }
    if (!Element.prototype.scrollIntoView) {
      Element.prototype.scrollIntoView = () => {};
    }
  });

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

  it("shows machine identity details in the remote endpoint selector", async () => {
    const user = userEvent.setup();
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

    await user.click(screen.getAllByLabelText("Location")[0]);

    expect(await screen.findByText("deploy@staging.example.com:22")).toBeInTheDocument();
    expect(screen.getByText("deploy@prod.example.com:22")).toBeInTheDocument();
    expect(screen.getByText("/Users/me")).toBeInTheDocument();
  });

  it("highlights remote machine group names in the endpoint selector", async () => {
    const user = userEvent.setup();
    render(
      <I18nProvider>
        <SftpPanel
          sshConnections={[
            { ...connections[0], group: "Production" },
            { ...connections[1], group: "Production" },
          ]}
          localDefaultPath="/Users/me"
          active
          themeVariant="light"
          currentSshConnectionId="conn-2"
        />
      </I18nProvider>,
    );

    await user.click(screen.getAllByLabelText("Location")[0]);

    const groupLabels = await screen.findAllByText("Production");
    const groupLabel = groupLabels.find((label) => label.closest(".sftp-select-group-label"));
    expect(groupLabel?.closest(".sftp-select-group-label")).toHaveClass(
      "sftp-select-group-label-highlight",
    );
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

    await user.click(progressButton);
    const history = screen.getByRole("region", { name: /Transfer history/i });
    expect(within(history).getByText("README.md")).toBeInTheDocument();
    expect(within(history).getByText("Running")).toBeInTheDocument();

    transferControl.finish?.();
    await waitFor(() => expect(progressButton).toHaveAttribute("aria-busy", "false"));
    await waitFor(() => expect(within(history).getByText("Completed")).toBeInTheDocument());
  });

  it("shows the transfer icon until a transfer starts, and keeps history reachable", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockResolvedValue([]);

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

    // 平时没有任务:按钮常驻但不显示百分比。
    const progressButton = screen.getByRole("button", { name: /Transfer progress/i });
    expect(progressButton).toHaveAttribute("aria-busy", "false");
    expect(progressButton.querySelector(".sftp-progress-count")).toBeNull();

    await user.click(progressButton);
    const history = screen.getByRole("region", { name: /Transfer history/i });
    expect(within(history).getByText(/No transfer tasks/i)).toBeInTheDocument();
  });

  it("defaults panes to modification time descending sort", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "sftp_read_dir") {
        const endpoint = (args as { endpoint: { kind: string } }).endpoint;
        if (endpoint.kind !== "local") return Promise.resolve([]);
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
    const modifiedHeader = screen.getAllByRole("button", { name: /^Modified/ })[0];
    expect(modifiedHeader).toHaveClass("sorted");
    expect(modifiedHeader).toHaveAttribute("aria-sort", "descending");
  });

  it("shows placeholders for missing sizes while preserving zero-byte file sizes", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command !== "sftp_read_dir") return Promise.resolve([]);
      const endpoint = (args as { endpoint: { kind: string } }).endpoint;
      if (endpoint.kind !== "local") return Promise.resolve([]);
      return Promise.resolve([
        { name: "folder", path: "/Users/me/folder", isDir: true },
        { name: "empty.txt", path: "/Users/me/empty.txt", isDir: false, size: 0 },
        { name: "unknown.txt", path: "/Users/me/unknown.txt", isDir: false },
      ]);
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

    expect(
      screen
        .getByText("folder", { selector: ".sftp-row-name" })
        .closest(".sftp-row")
        ?.querySelector(".sftp-row-size"),
    ).toHaveTextContent("--");
    expect(
      screen
        .getByText("empty.txt", { selector: ".sftp-row-name" })
        .closest(".sftp-row")
        ?.querySelector(".sftp-row-size"),
    ).toHaveTextContent("0 B");
    expect(
      screen
        .getByText("unknown.txt", { selector: ".sftp-row-name" })
        .closest(".sftp-row")
        ?.querySelector(".sftp-row-size"),
    ).toHaveTextContent("--");
  });

  it("sorts by a column when its header is clicked and toggles direction", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "sftp_read_dir") {
        const endpoint = (args as { endpoint: { kind: string } }).endpoint;
        if (endpoint.kind !== "local") return Promise.resolve([]);
        return Promise.resolve([
          { name: "b.txt", path: "/Users/me/b.txt", isDir: false, size: 1, modifiedAtMs: 300 },
          { name: "a.txt", path: "/Users/me/a.txt", isDir: false, size: 2, modifiedAtMs: 100 },
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

    await user.click(screen.getAllByRole("button", { name: "Open pane" })[0]);
    expect((await screen.findAllByText(/\.txt$/)).map((row) => row.textContent)).toEqual([
      "b.txt",
      "a.txt",
    ]);

    await user.click(screen.getAllByRole("button", { name: /^Name/ })[0]);
    expect(screen.getAllByText(/\.txt$/).map((row) => row.textContent)).toEqual(["a.txt", "b.txt"]);

    await user.click(screen.getAllByRole("button", { name: /^Name/ })[0]);
    expect(screen.getAllByRole("button", { name: /^Name/ })[0]).toHaveAttribute(
      "aria-sort",
      "descending",
    );
    expect(screen.getAllByText(/\.txt$/).map((row) => row.textContent)).toEqual(["b.txt", "a.txt"]);
  });

  it("reorders columns when a header is dragged onto another header", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "sftp_read_dir") {
        const endpoint = (args as { endpoint: { kind: string } }).endpoint;
        if (endpoint.kind !== "local") return Promise.resolve([]);
        return Promise.resolve([
          { name: "a.txt", path: "/Users/me/a.txt", isDir: false, size: 1, modifiedAtMs: 1 },
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

    await user.click(screen.getAllByRole("button", { name: "Open pane" })[0]);
    await screen.findByText("a.txt");

    const head = document.querySelectorAll(".sftp-list-head")[0];
    const labels = () =>
      Array.from(head.querySelectorAll(".sftp-list-head-cell")).map((cell) =>
        cell.textContent?.replace(/[▲▼\s]/g, ""),
      );
    expect(labels()).toEqual(["Name", "Modified", "Size", "Type"]);

    const dataTransfer = {
      data: {} as Record<string, string>,
      setData(key: string, value: string) {
        this.data[key] = value;
      },
      getData(key: string) {
        return this.data[key];
      },
      effectAllowed: "none",
    };
    const modifiedHeader = screen.getAllByRole("button", { name: /^Modified/ })[0];
    const sizeHeader = screen.getAllByRole("button", { name: /^Size/ })[0];
    fireEvent.dragStart(sizeHeader, { dataTransfer });
    fireEvent.dragOver(modifiedHeader, { dataTransfer });
    fireEvent.drop(modifiedHeader, { dataTransfer });

    await waitFor(() => expect(labels()).toEqual(["Name", "Size", "Modified", "Type"]));
  });

  it("copies every cmd-selected file in one transfer", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "sftp_read_dir") {
        const endpoint = (args as { endpoint: { kind: string } }).endpoint;
        if (endpoint.kind !== "local") return Promise.resolve([]);
        return Promise.resolve([
          { name: "a.txt", path: "/Users/me/a.txt", isDir: false, modifiedAtMs: 2 },
          { name: "b.txt", path: "/Users/me/b.txt", isDir: false, modifiedAtMs: 1 },
        ]);
      }
      return Promise.resolve(undefined);
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

    await user.click(screen.getAllByRole("button", { name: "Open pane" })[0]);
    await user.click(screen.getByText("a.txt"));
    fireEvent.click(screen.getByText("b.txt"), { metaKey: true });
    await user.click(screen.getAllByRole("button", { name: "Copy" })[0]);

    await waitFor(() => {
      const transfers = vi
        .mocked(invoke)
        .mock.calls.filter(([command]) => command === "sftp_copy_paths");
      expect(transfers).toHaveLength(2);
      expect(transfers.map(([, args]) => (args as { paths: string[] }).paths[0]).sort()).toEqual([
        "/Users/me/a.txt",
        "/Users/me/b.txt",
      ]);
    });
  });

  it("keeps expanded folders open after a copy refresh", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command !== "sftp_read_dir") return Promise.resolve(undefined);
      const endpoint = (args as { endpoint: { kind: string; path: string } }).endpoint;
      if (endpoint.kind !== "local") return Promise.resolve([]);
      const path = endpoint.path;
      if (path === "/Users/me") {
        return Promise.resolve([
          { name: "src", path: "/Users/me/src", isDir: true, modifiedAtMs: 2 },
          { name: "README.md", path: "/Users/me/README.md", isDir: false, modifiedAtMs: 1 },
        ]);
      }
      if (path === "/Users/me/src") {
        return Promise.resolve([
          { name: "main.ts", path: "/Users/me/src/main.ts", isDir: false, modifiedAtMs: 1 },
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

    await user.click(screen.getAllByRole("button", { name: "Open pane" })[0]);
    const srcRow = screen.getByText("src").closest(".sftp-row");
    expect(srcRow).not.toBeNull();
    await user.click(srcRow!);
    await user.click(srcRow!);
    expect(await screen.findByText("main.ts")).toBeInTheDocument();
    await user.click(screen.getByText("README.md"));
    await user.click(screen.getAllByRole("button", { name: "Copy" })[0]);
    expect(await screen.findByText("main.ts")).toBeInTheDocument();
  });
});
