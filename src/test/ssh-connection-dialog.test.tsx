import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshConnectionDialog } from "../components/ssh/SshConnectionDialog";

describe("SshConnectionDialog", () => {
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
});
