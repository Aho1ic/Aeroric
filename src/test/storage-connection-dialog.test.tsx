import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { StorageConnectionDialog } from "../components/storage/StorageConnectionDialog";
import type {
  StorageCapability,
  StorageConnection,
  StorageProtocol,
  StorageProtocolDescriptor,
} from "../types/storage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const FULL_CAPABILITY: StorageCapability = {
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
  protocol: StorageProtocol,
  over: Partial<StorageProtocolDescriptor> = {},
): StorageProtocolDescriptor {
  return {
    protocol,
    displayName: protocol,
    capability: FULL_CAPABILITY,
    requiredConfigKeys: [],
    secretKeys: [],
    defaultEndpoint: null,
    oauth: false,
    systemMount: false,
    deprecated: false,
    ...over,
  };
}

const DESCRIPTORS: StorageProtocolDescriptor[] = [
  descriptor("s3", { requiredConfigKeys: ["bucket", "region"] }),
  descriptor("webdavHttps", { requiredConfigKeys: ["endpoint"] }),
  descriptor("dropbox", { oauth: true }),
  descriptor("baiduNetdisk", { oauth: true }),
  descriptor("smb", { requiredConfigKeys: ["host", "share"] }),
  descriptor("afp", { requiredConfigKeys: ["host", "share"], systemMount: true, deprecated: true }),
];

describe("StorageConnectionDialog", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    // 默认:非 OAuth 协议查询凭据来源时返回 null。
    vi.mocked(invoke).mockResolvedValue(null);
    // jsdom 未实现这两个 API,Radix Select 打开时会调用它们。
    Element.prototype.hasPointerCapture ??= () => false;
    Element.prototype.scrollIntoView ??= () => {};
  });

  function renderDialog(over: Partial<React.ComponentProps<typeof StorageConnectionDialog>> = {}) {
    const onSave = vi.fn();
    const onClose = vi.fn();
    render(
      <I18nProvider>
        <StorageConnectionDialog
          descriptors={DESCRIPTORS}
          onClose={onClose}
          onSave={onSave}
          {...over}
        />
      </I18nProvider>,
    );
    return { onSave, onClose };
  }

  /**
   * 在协议下拉里选一项。
   *
   * Radix Select 会同时渲染一个隐藏的原生 `<select>`,所以每个选项文案在文档里
   * 出现两次;这里只在浮层内部查找。
   */
  async function pickProtocol(user: ReturnType<typeof userEvent.setup>, label: string) {
    await user.click(screen.getByLabelText("Service"));
    const content = await waitFor(() => {
      const node = document.querySelector<HTMLElement>(".radix-select-content");
      expect(node).not.toBeNull();
      return node!;
    });
    await user.click(within(content).getByText(label));
  }

  it("renders the protocol dropdown above the dialog overlay", async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.click(screen.getByLabelText("Service"));

    const content = await waitFor(() => {
      const node = document.querySelector<HTMLElement>(".radix-select-content");
      expect(node).not.toBeNull();
      return node!;
    });
    const overlay = document.querySelector<HTMLElement>('[role="dialog"]')!.parentElement!;
    expect(Number(content.style.zIndex)).toBeGreaterThan(Number(overlay.style.zIndex));
  });

  it("shows the fields of the picked protocol only", async () => {
    const user = userEvent.setup();
    renderDialog();

    // 默认 S3。
    expect(screen.getByLabelText(/^Bucket/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/^Share/)).not.toBeInTheDocument();

    await pickProtocol(user, "SMB");

    expect(await screen.findByLabelText(/^Share/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/^Bucket/)).not.toBeInTheDocument();
  });

  it("blocks saving until the required fields are filled", async () => {
    const user = userEvent.setup();
    const { onSave } = renderDialog();

    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).not.toHaveBeenCalled();
    expect(await screen.findByText("A name is required")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Name"), "Media");
    await user.type(screen.getByLabelText(/^Bucket/), "media");
    await user.type(screen.getByLabelText(/^Region/), "us-east-1");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0]).toMatchObject({
      name: "Media",
      protocol: "s3",
      config: { bucket: "media", region: "us-east-1" },
    });
  });

  it("routes credential inputs into secrets, never into config", async () => {
    const user = userEvent.setup();
    const { onSave } = renderDialog();

    await user.type(screen.getByLabelText("Name"), "Media");
    await user.type(screen.getByLabelText(/^Bucket/), "media");
    await user.type(screen.getByLabelText(/^Region/), "us-east-1");
    await user.type(screen.getByLabelText("Access key ID"), "AKIA");
    await user.type(screen.getByLabelText("Secret access key"), "s3cr3t");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    const saved = onSave.mock.calls[0][0] as StorageConnection;
    expect(saved.secrets).toEqual({ accessKeyId: "AKIA", secretAccessKey: "s3cr3t" });
    expect(JSON.stringify(saved.config)).not.toContain("s3cr3t");
  });

  it("rejects an http endpoint on the https-only WebDAV protocol", async () => {
    const user = userEvent.setup();
    const { onSave } = renderDialog();

    await pickProtocol(user, "WebDAV (HTTPS)");

    await user.type(screen.getByLabelText("Name"), "Nextcloud");
    await user.type(screen.getByLabelText(/^Endpoint/), "http://dav.example.com/dav");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(
      await screen.findByText("This connection requires an https:// endpoint"),
    ).toBeInTheDocument();
    expect(onSave).not.toHaveBeenCalled();
  });

  it("warns that AFP is deprecated and mounted by the system", async () => {
    const user = userEvent.setup();
    renderDialog();

    await pickProtocol(user, "AFP");

    expect(await screen.findByText(/Apple deprecated the AFP client/)).toBeInTheDocument();
    expect(screen.getByText(/mounted by the operating system/)).toBeInTheDocument();
  });

  it("states that Baidu Netdisk needs a self-registered app and passes both credentials", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "storage_oauth_credential_options") {
        return {
          builtinAvailable: false,
          requiresClientSecret: true,
          supportsPkce: false,
          scope: "basic,netdisk",
        };
      }
      if (command === "storage_oauth_authorize") {
        return { secrets: { accessToken: "tok", refreshToken: "ref", clientId: "cid" } };
      }
      return null;
    });
    const { onSave } = renderDialog();

    await pickProtocol(user, "Baidu Netdisk");

    expect(await screen.findByText(/requires a client secret/)).toBeInTheDocument();
    expect(screen.getByText("Scopes requested: basic,netdisk")).toBeInTheDocument();
    // 未填凭据前不能授权。
    expect(screen.getByRole("button", { name: /Authorize/ })).toBeDisabled();

    await user.type(screen.getByLabelText(/^Client ID/), "cid");
    await user.type(screen.getByLabelText(/^Client secret/), "csecret");
    await user.click(screen.getByRole("button", { name: /Authorize/ }));

    expect(await screen.findByText("Authorized")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("storage_oauth_authorize", {
      protocol: "baiduNetdisk",
      source: "userProvided",
      clientId: "cid",
      clientSecret: "csecret",
    });

    await user.type(screen.getByLabelText("Name"), "Netdisk");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    expect((onSave.mock.calls[0][0] as StorageConnection).secrets).toMatchObject({
      accessToken: "tok",
      refreshToken: "ref",
    });
  });

  it("offers the built-in app only when the backend says it exists", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "storage_oauth_credential_options") {
        return {
          builtinAvailable: true,
          requiresClientSecret: false,
          supportsPkce: true,
          scope: "files.content.write",
        };
      }
      return null;
    });
    renderDialog();

    await pickProtocol(user, "Dropbox");

    expect(await screen.findByLabelText("Use the built-in Aeroric app")).toBeChecked();
    // 内置模式不需要用户填 client id。
    expect(screen.queryByLabelText(/^Client ID/)).not.toBeInTheDocument();

    await user.click(screen.getByLabelText("Use my own app credentials"));
    expect(screen.getByLabelText(/^Client ID/)).toBeInTheDocument();
    // PKCE public client 不该索要 secret。
    expect(screen.queryByLabelText(/^Client secret/)).not.toBeInTheDocument();
  });

  it("keeps a saved credential when its field is left blank", async () => {
    const user = userEvent.setup();
    const existing: StorageConnection = {
      id: "conn-1",
      name: "Media",
      protocol: "s3",
      config: { bucket: "media", region: "us-east-1" },
      createdAt: 5,
    };
    const { onSave } = renderDialog({
      connection: existing,
      savedSecretKeys: ["accessKeyId", "secretAccessKey"],
    });

    expect(screen.getByLabelText("Access key ID")).toHaveAttribute(
      "placeholder",
      "Saved — leave blank to keep",
    );

    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSave).toHaveBeenCalled());
    // 空值不提交,后端据此保留已存凭据。
    expect(onSave.mock.calls[0][0]).not.toHaveProperty("secrets");
    expect(onSave.mock.calls[0][0]).toMatchObject({ id: "conn-1", createdAt: 5 });
  });

  it("discloses that credentials are stored as plaintext", () => {
    renderDialog();
    expect(screen.getByText(/stored as plaintext in a local owner-only file/)).toBeInTheDocument();
  });
});
