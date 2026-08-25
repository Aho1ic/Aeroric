import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshConnectionDialog } from "../components/ssh/SshConnectionDialog";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invokeMock(command, args),
  isTauri: () => false,
}));

describe("SSH proxy option", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({ proxy_settings: { url: "http://127.0.0.1:7890" } });
    Element.prototype.hasPointerCapture ??= () => false;
    Element.prototype.scrollIntoView ??= () => {};
  });

  /// 用户要知道勾了之后会走哪个代理,而不是一句泛泛的"使用代理"。
  it("shows the proxy address taken from the app settings", async () => {
    render(
      <I18nProvider>
        <SshConnectionDialog onClose={vi.fn()} onSave={vi.fn()} />
      </I18nProvider>,
    );

    expect(invokeMock).toHaveBeenCalledWith("load_app_settings", undefined);
    await waitFor(() => {
      expect(screen.getByText(/http:\/\/127\.0\.0\.1:7890/)).toBeInTheDocument();
    });
  });

  it("warns when no proxy is configured yet", async () => {
    invokeMock.mockResolvedValue({ proxy_settings: { url: "" } });

    render(
      <I18nProvider>
        <SshConnectionDialog onClose={vi.fn()} onSave={vi.fn()} />
      </I18nProvider>,
    );

    await waitFor(() => {
      expect(screen.getByText(/No proxy is configured yet/)).toBeInTheDocument();
    });
  });

  it("saves the opt-in so the connection always uses the proxy", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();

    render(
      <I18nProvider>
        <SshConnectionDialog onClose={vi.fn()} onSave={onSave} />
      </I18nProvider>,
    );

    await user.type(screen.getByLabelText("Name"), "via-proxy");
    await user.type(screen.getByLabelText("Host"), "prod.example.com");
    await user.type(screen.getByLabelText("Username"), "deploy");
    await user.click(screen.getByRole("checkbox", { name: /Connect through the global proxy/ }));
    await user.click(screen.getByRole("button", { name: /Save/ }));

    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0]).toMatchObject({
      name: "via-proxy",
      host: "prod.example.com",
      useProxy: true,
    });
  });

  it("leaves the proxy off by default so existing connections are unchanged", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();

    render(
      <I18nProvider>
        <SshConnectionDialog onClose={vi.fn()} onSave={onSave} />
      </I18nProvider>,
    );

    await user.type(screen.getByLabelText("Name"), "direct");
    await user.type(screen.getByLabelText("Host"), "prod.example.com");
    await user.type(screen.getByLabelText("Username"), "deploy");
    await user.click(screen.getByRole("button", { name: /Save/ }));

    expect(onSave.mock.calls[0][0]).not.toHaveProperty("useProxy");
  });

  /// 编辑已勾选的连接时必须回显,否则保存一次就把代理关掉了。
  it("keeps the checkbox on when editing a connection that already uses the proxy", async () => {
    render(
      <I18nProvider>
        <SshConnectionDialog
          connection={{
            id: "c1",
            name: "prod",
            host: "prod.example.com",
            port: 22,
            username: "deploy",
            useProxy: true,
            createdAt: 1,
          }}
          onClose={vi.fn()}
          onSave={vi.fn()}
        />
      </I18nProvider>,
    );

    expect(
      screen.getByRole("checkbox", { name: /Connect through the global proxy/ }),
    ).toBeChecked();
  });
});
