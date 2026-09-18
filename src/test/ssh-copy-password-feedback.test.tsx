import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshConnectionList } from "../components/ssh/SshConnectionList";
import type { SshConnection } from "../types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke,
}));

const connection: SshConnection = {
  id: "conn-1",
  name: "Prod box",
  host: "10.0.0.5",
  port: 22,
  username: "deploy",
  // 连接记录只带 hasPassword,明文按需经 get_ssh_connection_password 取。
  hasPassword: true,
  createdAt: 0,
};

function renderList() {
  render(
    <I18nProvider>
      <SshConnectionList
        connections={[connection]}
        selectedId={null}
        onSelect={vi.fn()}
        onCreate={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
      />
    </I18nProvider>,
  );
}

describe("SSH copy password click feedback", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    invoke.mockReset();
    invoke.mockResolvedValue("s3cret");
  });

  it("marks the copy action as copied after a successful write", async () => {
    // userEvent.setup() 会自己装一份 clipboard stub，必须在它之后再覆盖。
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    renderList();

    const copyAction = screen.getByRole("button", { name: "Copy password" });
    expect(copyAction).not.toHaveAttribute("data-copied");

    await user.click(copyAction);

    expect(invoke).toHaveBeenCalledWith("get_ssh_connection_password", { connectionId: "conn-1" });
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("s3cret");
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Copy password" })).toHaveAttribute(
        "data-copied",
        "true",
      );
    });
  });

  it("keeps the idle state when the clipboard write fails", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    /* mockRestore 必须在 finally 里:下面任一断言抛出就走不到还原,console.warn 会对
       本文件**剩下的所有用例**永久静音(本文件没有 afterEach,配置也没开 restoreMocks)。
       静音之后 React 的 act 告警、effect 抛错这类回归在 CI 里彻底看不见。 */
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      renderList();

      await user.click(screen.getByRole("button", { name: "Copy password" }));

      await waitFor(() => expect(warn).toHaveBeenCalled());
      expect(screen.getByRole("button", { name: "Copy password" })).not.toHaveAttribute(
        "data-copied",
      );
    } finally {
      warn.mockRestore();
    }
  });
});
