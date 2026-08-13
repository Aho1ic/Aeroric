import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { StorageConnectionPage } from "../components/storage/StorageConnectionPage";
import type { StorageConnection, StorageProtocolDescriptor } from "../types/storage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const CAPABILITY = {
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

function descriptor(
  protocol: StorageConnection["protocol"],
  over: Partial<StorageProtocolDescriptor> = {},
): StorageProtocolDescriptor {
  return {
    protocol,
    displayName: protocol,
    capability: CAPABILITY,
    requiredConfigKeys: [],
    secretKeys: [],
    defaultEndpoint: null,
    oauth: false,
    systemMount: false,
    deprecated: false,
    ...over,
  };
}

const connections: StorageConnection[] = [
  {
    id: "store-1",
    name: "Media bucket",
    protocol: "s3",
    config: { bucket: "media", region: "us-east-1" },
    createdAt: 1,
  },
  {
    id: "store-2",
    name: "Legacy share",
    protocol: "afp",
    config: { host: "10.0.0.2", share: "public" },
    createdAt: 2,
  },
];

function mockBackend(over: Record<string, unknown> = {}) {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command in over) return Promise.resolve(over[command]);
    if (command === "storage_list_connections") return Promise.resolve(connections);
    if (command === "storage_secret_keys") {
      return Promise.resolve({ "store-1": ["accessKeyId", "secretAccessKey"] });
    }
    if (command === "storage_protocols") {
      return Promise.resolve([
        descriptor("s3"),
        descriptor("afp", { systemMount: true, deprecated: true }),
      ]);
    }
    return Promise.resolve(undefined);
  });
}

describe("StorageConnectionPage", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    Element.prototype.hasPointerCapture ??= () => false;
    Element.prototype.scrollIntoView ??= () => {};
  });

  it("lists saved connections with their service and summary", async () => {
    mockBackend();
    render(
      <I18nProvider>
        <StorageConnectionPage onClose={vi.fn()} />
      </I18nProvider>,
    );

    expect(await screen.findByText("Media bucket")).toBeInTheDocument();
    expect(screen.getByText("Amazon S3")).toBeInTheDocument();
    expect(screen.getByText("media · us-east-1")).toBeInTheDocument();
    expect(screen.getByText("10.0.0.2/public")).toBeInTheDocument();
  });

  it("flags deprecated services and offers unmount only for system mounts", async () => {
    mockBackend();
    render(
      <I18nProvider>
        <StorageConnectionPage onClose={vi.fn()} />
      </I18nProvider>,
    );

    await screen.findByText("Legacy share");
    expect(screen.getByText("Deprecated")).toBeInTheDocument();
    // 只有 AFP 卡片有卸载按钮。
    expect(screen.getAllByRole("button", { name: "Unmount" })).toHaveLength(1);
  });

  it("tests the selected connection and reports the result", async () => {
    const user = userEvent.setup();
    mockBackend();
    render(
      <I18nProvider>
        <StorageConnectionPage onClose={vi.fn()} />
      </I18nProvider>,
    );

    await screen.findByText("Media bucket");
    await user.click(screen.getByRole("button", { name: "Test connection" }));

    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("storage_test_connection", {
        connectionId: "store-1",
      }),
    );
    expect(await screen.findByText("Media bucket connected")).toBeInTheDocument();
  });

  it("surfaces a failed connection test instead of silently passing", async () => {
    const user = userEvent.setup();
    mockBackend();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "storage_list_connections") return Promise.resolve(connections);
      if (command === "storage_secret_keys") return Promise.resolve({});
      if (command === "storage_protocols") return Promise.resolve([descriptor("s3")]);
      if (command === "storage_test_connection") {
        return Promise.reject(new Error("Access denied. Check the credentials."));
      }
      return Promise.resolve(undefined);
    });
    render(
      <I18nProvider>
        <StorageConnectionPage onClose={vi.fn()} />
      </I18nProvider>,
    );

    await screen.findByText("Media bucket");
    await user.click(screen.getByRole("button", { name: "Test connection" }));

    expect(await screen.findByText(/Access denied/)).toBeInTheDocument();
  });

  it("opens the picked connection in the browser callback", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    mockBackend();
    render(
      <I18nProvider>
        <StorageConnectionPage onClose={vi.fn()} onOpen={onOpen} />
      </I18nProvider>,
    );

    await user.click(await screen.findByText("Legacy share"));
    await user.click(screen.getByRole("button", { name: "Browse" }));

    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: "store-2" }));
  });

  it("shows the empty state when no connection exists", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "storage_list_connections") return Promise.resolve([]);
      if (command === "storage_secret_keys") return Promise.resolve({});
      if (command === "storage_protocols") return Promise.resolve([descriptor("s3")]);
      return Promise.resolve(undefined);
    });
    render(
      <I18nProvider>
        <StorageConnectionPage onClose={vi.fn()} />
      </I18nProvider>,
    );

    expect(await screen.findByText("No storage connections")).toBeInTheDocument();
  });
});
