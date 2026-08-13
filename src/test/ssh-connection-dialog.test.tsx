import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshConnectionDialog } from "../components/ssh/SshConnectionDialog";

describe("SshConnectionDialog", () => {
  beforeEach(() => {
    // jsdom 未实现这两个 API,Radix Select 打开时会调用它们。
    Element.prototype.hasPointerCapture ??= () => false;
    Element.prototype.scrollIntoView ??= () => {};
  });

  it("toggles the SSH password visibility", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <SshConnectionDialog onClose={vi.fn()} onSave={vi.fn()} />
      </I18nProvider>,
    );

    const passwordInput = screen.getByLabelText("Password");
    expect(passwordInput).toHaveAttribute("type", "password");

    await user.click(screen.getByRole("button", { name: "Show password" }));

    expect(passwordInput).toHaveAttribute("type", "text");
    expect(screen.getByRole("button", { name: "Hide password" })).toBeInTheDocument();
  });

  it("renders the group dropdown above the dialog overlay", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <SshConnectionDialog
          groups={["Production", "Staging"]}
          onClose={vi.fn()}
          onSave={vi.fn()}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("combobox", { name: "Group" }));

    const content = document.querySelector<HTMLElement>(".radix-select-content");
    expect(content).not.toBeNull();

    // 回归:此前 select content(2000)低于 sshDialogOverlay(3000),被遮罩完全盖住。
    const contentZ = Number(content!.style.zIndex);
    const overlayZ = Number(
      document.querySelector<HTMLElement>('[role="dialog"]')!.parentElement!.style.zIndex,
    );
    expect(contentZ).toBeGreaterThan(overlayZ);
  });

  it("writes the picked group back into the draft", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();

    render(
      <I18nProvider>
        <SshConnectionDialog groups={["Production", "Staging"]} onClose={vi.fn()} onSave={onSave} />
      </I18nProvider>,
    );

    await user.type(screen.getByLabelText("Name"), "prod-1");
    await user.type(screen.getByLabelText("Host"), "prod.example.com");
    await user.type(screen.getByLabelText("Username"), "deploy");

    const groupSelect = screen.getByRole("combobox", { name: "Group" });
    await user.click(groupSelect);
    await user.click(screen.getByRole("option", { name: "Staging" }));

    expect(groupSelect).toHaveTextContent("Staging");

    await user.click(screen.getByRole("button", { name: /Save/ }));

    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0]).toMatchObject({
      name: "prod-1",
      group: "Staging",
      host: "prod.example.com",
      username: "deploy",
    });
  });
});
