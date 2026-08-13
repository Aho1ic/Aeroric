import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SftpPanel } from "../components/sftp/SftpPanel";
import { toTauriSftpEndpoint } from "../components/sftp/sftpOperations";
import {
  defaultSftpPathForEndpoint,
  groupSftpStorageConnections,
  sftpEndpointKey,
} from "../components/sftp/sftpTypes";
import type { SshConnection } from "../types";
import type { StorageCapability, StorageConnection } from "../types/storage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn().mockResolvedValue(true),
}));

const sshConnections: SshConnection[] = [
  {
    id: "conn-1",
    name: "Staging",
    host: "staging.example.com",
    port: 22,
    username: "deploy",
    remotePath: "/srv/staging",
    createdAt: 1,
  },
];

const storageConnections: StorageConnection[] = [
  {
    id: "store-1",
    name: "Media bucket",
    protocol: "s3",
    config: { bucket: "media", region: "us-east-1" },
    createdAt: 1,
  },
  {
    id: "store-2",
    name: "Team drive",
    group: "Cloud",
    protocol: "googleDrive",
    config: {},
    createdAt: 2,
  },
];

const FULL: StorageCapability = {
  readDir: true,
  read: true,
  write: true,
  createDir: true,
  delete: true,
  rename: true,
  copy: true,
  stat: true,
  richMetadata: true,
};

function mockInvoke(capability: Partial<StorageCapability> = {}) {
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "storage_capabilities") {
      return Promise.resolve({ ...FULL, ...capability });
    }
    if (command === "sftp_read_dir") {
      const endpoint = (args as { endpoint: { kind: string } }).endpoint;
      if (endpoint.kind === "storage") {
        return Promise.resolve([
          {
            name: "clip.mp4",
            path: "/clip.mp4",
            isDir: false,
            extension: "mp4",
            size: 2048,
            modifiedAtMs: null,
          },
        ]);
      }
      return Promise.resolve([]);
    }
    return Promise.resolve([]);
  });
}

function renderPanel() {
  return render(
    <I18nProvider>
      <SftpPanel
        sshConnections={sshConnections}
        storageConnections={storageConnections}
        localDefaultPath="/Users/me"
        active
        themeVariant="light"
        currentStorageConnectionId="store-1"
      />
    </I18nProvider>,
  );
}

describe("SFTP storage 端点", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue([]);
    Element.prototype.hasPointerCapture ??= () => false;
    Element.prototype.releasePointerCapture ??= () => {};
    Element.prototype.scrollIntoView ??= () => {};
  });

  it("端点只带连接 id,凭据不经过前端", () => {
    const tauriEndpoint = toTauriSftpEndpoint(
      { kind: "storage", connectionId: "store-1", connectionName: "Media bucket", path: "/a" },
      sshConnections,
    );
    expect(tauriEndpoint).toEqual({ kind: "storage", connectionId: "store-1", path: "/a" });
    expect(JSON.stringify(tauriEndpoint)).not.toContain("media");
  });

  it("端点 key 区分 ssh 与 storage 的同名 id", () => {
    expect(
      sftpEndpointKey({ kind: "ssh", connectionId: "x", connectionName: "n", path: "/p" }),
    ).not.toBe(
      sftpEndpointKey({ kind: "storage", connectionId: "x", connectionName: "n", path: "/p" }),
    );
  });

  it("storage 端点默认从根开始", () => {
    expect(defaultSftpPathForEndpoint("storage", undefined, "/Users/me")).toBe("/");
  });

  it("按分组归类存储连接,未分组落到默认组", () => {
    const groups = groupSftpStorageConnections(storageConnections, "Default");
    expect(groups.map((group) => group.label)).toEqual(["Default", "Cloud"]);
    expect(groups[0].connections[0].id).toBe("store-1");
  });

  it("默认把右侧面板打开到传入的存储连接", () => {
    mockInvoke();
    renderPanel();
    const triggers = screen.getAllByLabelText("Location");
    expect(triggers[0]).toHaveTextContent("Local");
    expect(triggers[1]).toHaveTextContent("Media bucket");
  });

  it("位置下拉列出存储连接及其服务名", async () => {
    const user = userEvent.setup();
    mockInvoke();
    renderPanel();

    await user.click(screen.getAllByLabelText("Location")[0]);
    const content = await waitFor(() => {
      const node = document.querySelector<HTMLElement>(".sftp-select-content");
      expect(node).not.toBeNull();
      return node!;
    });

    expect(within(content).getByText("Media bucket")).toBeInTheDocument();
    expect(within(content).getByText(/Amazon S3/)).toBeInTheDocument();
    expect(within(content).getByText("Team drive")).toBeInTheDocument();
    expect(within(content).getByText(/Google Drive/)).toBeInTheDocument();
  });

  it("读目录时把 storage 端点原样传给后端", async () => {
    const user = userEvent.setup();
    mockInvoke();
    renderPanel();

    await user.click(screen.getAllByRole("button", { name: "Open pane" })[1]);

    expect(await screen.findByText("clip.mp4")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("sftp_read_dir", {
      endpoint: { kind: "storage", connectionId: "store-1", path: "/" },
    });
  });

  it("按能力位禁用后端不支持的动作", async () => {
    const user = userEvent.setup();
    mockInvoke({ rename: false, createDir: false, delete: false });
    renderPanel();

    // 两个面板都要打开,才能对比 storage 与本地面板的按钮状态。
    const openButtons = screen.getAllByRole("button", { name: "Open pane" });
    await user.click(openButtons[0]);
    await user.click(openButtons[1]);
    await screen.findByText("clip.mp4");
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("storage_capabilities", {
        connectionId: "store-1",
      }),
    );

    // 右侧面板(storage)的按钮被禁用,左侧(本地)不受影响。
    await waitFor(() => {
      expect(screen.getAllByRole("button", { name: /^Folder$/ })[1]).toBeDisabled();
    });
    expect(screen.getAllByRole("button", { name: /^Rename$/ })[1]).toBeDisabled();
    expect(screen.getAllByRole("button", { name: /^Folder$/ })[0]).not.toBeDisabled();
  });

  it("能力位齐全时不禁用任何动作", async () => {
    const user = userEvent.setup();
    mockInvoke();
    renderPanel();

    const openButtons = screen.getAllByRole("button", { name: "Open pane" });
    await user.click(openButtons[0]);
    await user.click(openButtons[1]);
    await screen.findByText("clip.mp4");
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("storage_capabilities", {
        connectionId: "store-1",
      }),
    );

    expect(screen.getAllByRole("button", { name: /^Folder$/ })[1]).not.toBeDisabled();
  });
});
