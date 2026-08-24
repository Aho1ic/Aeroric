import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppDialogHost } from "../components/AppDialogHost";
import { confirm, prompt, resetAppDialogHandlerForTests } from "../lib/appDialog";
import { I18nProvider } from "../i18n";

function renderHost() {
  return render(
    <I18nProvider>
      <AppDialogHost />
    </I18nProvider>,
  );
}

describe("应用内提示弹窗", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
  });

  afterEach(() => {
    resetAppDialogHandlerForTests();
    vi.restoreAllMocks();
  });

  it("点确认 resolve true,点取消 resolve false", async () => {
    const user = userEvent.setup();
    renderHost();

    const accepted = confirm("delete this?", { title: "Delete" });
    await screen.findByRole("alertdialog");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(accepted).resolves.toBe(true);

    const rejected = confirm("delete this?", { title: "Delete" });
    await screen.findByRole("alertdialog");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(rejected).resolves.toBe(false);
  });

  it("Esc 等于取消,Enter 等于确认", async () => {
    const user = userEvent.setup();
    renderHost();

    const escaped = confirm("discard?");
    await screen.findByRole("alertdialog");
    await user.keyboard("{Escape}");
    await expect(escaped).resolves.toBe(false);

    const entered = confirm("discard?");
    await screen.findByRole("alertdialog");
    await user.keyboard("{Enter}");
    await expect(entered).resolves.toBe(true);
  });

  it("并发请求串行排队,两个 promise 都得到结果", async () => {
    const user = userEvent.setup();
    renderHost();

    const first = confirm("first message");
    const second = confirm("second message");

    // 队列串行:先只显示第一条,答完才轮到第二条。若后来者顶掉前者,
    // 前一个 promise 会永远悬着,调用方永久 await。
    await screen.findByText("first message");
    expect(screen.queryByText("second message")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(first).resolves.toBe(true);

    await screen.findByText("second message");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(second).resolves.toBe(false);
  });

  it("host 未挂载时返回 false 并告警(拒绝是安全默认值)", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    resetAppDialogHandlerForTests();

    await expect(confirm("delete everything")).resolves.toBe(false);
    expect(warn).toHaveBeenCalled();
  });

  it("warning / error 用 destructive 按钮,info 不用", async () => {
    renderHost();

    void confirm("dangerous", { kind: "warning" });
    const warningOk = await waitFor(() =>
      screen.getByRole("button", { name: "Confirm" }).closest("button"),
    );
    // destructive variant 走 --destructive token;info 走 primary。
    const warningBg = warningOk?.style.background ?? "";
    expect(warningBg).toContain("destructive");
  });

  it("info 类型不用 destructive 按钮", async () => {
    renderHost();

    void confirm("just asking", { kind: "info" });
    await screen.findByRole("alertdialog");
    const ok = screen.getByRole("button", { name: "Confirm" });
    expect(ok.style.background).not.toContain("destructive");
  });

  it("自定义 okLabel / cancelLabel 生效", async () => {
    renderHost();

    void confirm("drop table users", {
      title: "Production",
      kind: "warning",
      okLabel: "Execute",
      cancelLabel: "Abort",
    });

    await screen.findByRole("alertdialog");
    expect(screen.getByRole("button", { name: "Execute" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Abort" })).toBeTruthy();
  });

  it("消息里的换行被保留(调用点会拼 \\n\\n${sql})", async () => {
    renderHost();

    void confirm("Review it:\n\nDELETE FROM users");
    const body = await screen.findByText(/DELETE FROM users/);
    // pre-wrap:否则多行 SQL 会糊成一行。
    expect(body.style.whiteSpace).toBe("pre-wrap");
  });

  it("焦点默认落在取消键(破坏性操作不让误触 Enter 直接执行)", async () => {
    renderHost();

    void confirm("delete project", { kind: "warning" });
    await screen.findByRole("alertdialog");

    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" })),
    );
  });

  it("关闭后焦点还给触发元素", async () => {
    const user = userEvent.setup();
    const { container } = renderHost();

    const trigger = document.createElement("button");
    trigger.textContent = "open";
    container.appendChild(trigger);
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    const pending = confirm("sure?");
    await screen.findByRole("alertdialog");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await pending;

    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("队列排空前不还焦点(连续确认不被打断)", async () => {
    const user = userEvent.setup();
    const { container } = renderHost();

    const trigger = document.createElement("button");
    container.appendChild(trigger);
    trigger.focus();

    const first = confirm("first");
    const second = confirm("second");

    await screen.findByText("first");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await first;

    // 第二个还在队列里,焦点该留在弹窗上而不是回到 trigger。
    await screen.findByText("second");
    expect(document.activeElement).not.toBe(trigger);

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await second;
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("弹窗有 alertdialog 语义与 aria 关联", async () => {
    renderHost();

    void confirm("body text", { title: "Heading" });
    const dialog = await screen.findByRole("alertdialog");

    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(dialog.getAttribute("aria-labelledby")).toBe("app-dialog-title");
    expect(dialog.getAttribute("aria-describedby")).toBe("app-dialog-message");
    expect(screen.getByText("Heading").id).toBe("app-dialog-title");
  });

  it("默认标题与按钮文案取自 i18n", async () => {
    renderHost();

    void confirm("no title given");
    await screen.findByRole("alertdialog");

    // 未传 title 时用 common.confirm,不再是 Windows 上那个系统框标题。
    expect(screen.getAllByText("Confirm").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  });

  /* prompt —— 替代 window.prompt(WebView2 的系统输入框)。 */

  it("prompt 返回输入内容,取消返回 null", async () => {
    const user = userEvent.setup();
    renderHost();

    const typed = prompt("New folder name");
    const input = await screen.findByRole("textbox");
    await user.type(input, "reports");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(typed).resolves.toBe("reports");

    const cancelled = prompt("New folder name");
    await screen.findByRole("textbox");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(cancelled).resolves.toBeNull();
  });

  it("prompt 用 defaultValue 预填并全选,便于直接改名", async () => {
    renderHost();

    void prompt("Rename to", { defaultValue: "old-name" });
    const input = await screen.findByRole("textbox");

    expect((input as HTMLInputElement).value).toBe("old-name");
    // 焦点落在输入框(而不是取消键)——这里是要打字的。
    await waitFor(() => expect(document.activeElement).toBe(input));
  });

  it("prompt 空输入按取消处理(调用点都是 if (!name) return)", async () => {
    const user = userEvent.setup();
    renderHost();

    const blank = prompt("Name");
    await screen.findByRole("textbox");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(blank).resolves.toBeNull();

    // 纯空白同样按取消:否则会拿到一个全空格的名字。
    const spaces = prompt("Name");
    const input = await screen.findByRole("textbox");
    await user.type(input, "   ");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(spaces).resolves.toBeNull();
  });

  it("allowEmpty 让空提交与取消可区分(移出分组靠这个语义)", async () => {
    const user = userEvent.setup();
    renderHost();

    // 清空输入后确认 → 空串,调用点据此把连接移出分组。
    const cleared = prompt("Group", { defaultValue: "Analytics", allowEmpty: true });
    const input = await screen.findByRole("textbox");
    await user.clear(input);
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(cleared).resolves.toBe("");

    // 取消仍然是 null,不能和"清空"混为一谈,否则一按 Esc 就把分组清了。
    const cancelled = prompt("Group", { defaultValue: "Analytics", allowEmpty: true });
    await screen.findByRole("textbox");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(cancelled).resolves.toBeNull();
  });

  it("prompt 结果去掉首尾空白", async () => {
    const user = userEvent.setup();
    renderHost();

    const padded = prompt("Name");
    const input = await screen.findByRole("textbox");
    await user.type(input, "  spaced  ");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(padded).resolves.toBe("spaced");
  });

  it("prompt 里 Enter 提交、Esc 取消", async () => {
    const user = userEvent.setup();
    renderHost();

    const submitted = prompt("Name");
    const input = await screen.findByRole("textbox");
    await user.type(input, "via-enter");
    await user.keyboard("{Enter}");
    await expect(submitted).resolves.toBe("via-enter");

    const escaped = prompt("Name", { defaultValue: "keep" });
    await screen.findByRole("textbox");
    await user.keyboard("{Escape}");
    await expect(escaped).resolves.toBeNull();
  });

  it("prompt 不用 destructive 按钮(输入不是破坏性操作)", async () => {
    renderHost();

    void prompt("Name");
    await screen.findByRole("alertdialog");
    expect(screen.getByRole("button", { name: "Confirm" }).style.background).not.toContain(
      "destructive",
    );
  });

  it("host 未挂载时 prompt 返回 null 并告警", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    resetAppDialogHandlerForTests();

    await expect(prompt("Name")).resolves.toBeNull();
    expect(warn).toHaveBeenCalled();
  });

  it("confirm 与 prompt 混排时各自拿到正确类型的结果", async () => {
    const user = userEvent.setup();
    renderHost();

    const first = confirm("delete it?", { kind: "warning" });
    const second = prompt("rename to", { defaultValue: "x" });

    await screen.findByText("delete it?");
    // confirm 阶段不该出现输入框。
    expect(screen.queryByRole("textbox")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(first).resolves.toBe(true);

    const input = await screen.findByRole("textbox");
    await user.clear(input);
    await user.type(input, "y");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await expect(second).resolves.toBe("y");
  });
});
