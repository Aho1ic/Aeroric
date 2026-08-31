import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NoteQuickCapture } from "../components/notebook/NoteQuickCapture";
import type { CaptureTarget } from "../components/notebook/noteCapture";

/* `t` 把参数拼进返回值:只断言键名的话「落点提示指向哪个文件」这类内容就测不到 ——
   键名对了而路径是空的、是另一个目标的,断言照样通过。 */
function t(key: string, vars?: Record<string, string>): string {
  const suffix = vars ? `:${Object.values(vars).join(",")}` : "";
  return `${key}${suffix}`;
}

const PATHS: Record<CaptureTarget, string> = {
  today: "Daily/2026-08-28.md",
  inbox: "Inbox.md",
};

function renderCapture(overrides: Partial<React.ComponentProps<typeof NoteQuickCapture>> = {}) {
  const onSubmit = vi.fn();
  const onClose = vi.fn();
  const view = render(
    <NoteQuickCapture
      paths={PATHS}
      busy={false}
      error={null}
      onSubmit={onSubmit}
      onClose={onClose}
      t={t}
      {...overrides}
    />,
  );
  return { onSubmit, onClose, view };
}

function input(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: "notebook.captureInput" }) as HTMLTextAreaElement;
}

function saveButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "notebook.captureSave" }) as HTMLButtonElement;
}

describe("NoteQuickCapture", () => {
  it("focuses the textarea on open", () => {
    // 这个窗的全部用途就是马上开始打字。
    renderCapture();
    expect(document.activeElement).toBe(input());
  });

  it("defaults to today's daily note", () => {
    renderCapture();
    expect(screen.getByRole("radio", { name: "notebook.captureToday" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "notebook.captureInbox" })).not.toBeChecked();
  });

  it("shows the target path and updates it when the target changes", () => {
    renderCapture();
    expect(screen.getByText("notebook.captureTargetHint:Daily/2026-08-28.md")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", { name: "notebook.captureInbox" }));

    expect(screen.getByText("notebook.captureTargetHint:Inbox.md")).toBeInTheDocument();
  });

  it("disables save until there is non-blank text", () => {
    renderCapture();
    expect(saveButton()).toBeDisabled();

    fireEvent.change(input(), { target: { value: "   \n\t" } });
    expect(saveButton()).toBeDisabled();

    fireEvent.change(input(), { target: { value: "记一句" } });
    expect(saveButton()).toBeEnabled();
  });

  it("submits the raw text and the chosen target", () => {
    /* 提交的是原文,不是 trim 过的:拼法在模型层(`appendCapture` 自己 trim),
       两处都 trim 只会让「到底哪一层负责」变得不确定。 */
    const { onSubmit } = renderCapture();
    fireEvent.click(screen.getByRole("radio", { name: "notebook.captureInbox" }));
    fireEvent.change(input(), { target: { value: "  记一句  " } });

    fireEvent.click(saveButton());

    expect(onSubmit).toHaveBeenCalledWith("inbox", "  记一句  ");
  });

  it("saves on ⌘↩", () => {
    const { onSubmit } = renderCapture();
    fireEvent.change(input(), { target: { value: "记一句" } });

    fireEvent.keyDown(input(), { key: "Enter", metaKey: true });

    expect(onSubmit).toHaveBeenCalledWith("today", "记一句");
  });

  it("saves on ctrl+↩ too", () => {
    const { onSubmit } = renderCapture();
    fireEvent.change(input(), { target: { value: "记一句" } });

    fireEvent.keyDown(input(), { key: "Enter", ctrlKey: true });

    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("leaves a bare ↩ to the textarea", () => {
    // 裸回车是换行。捕获经常是好几行。
    const { onSubmit } = renderCapture();
    fireEvent.change(input(), { target: { value: "第一行" } });

    fireEvent.keyDown(input(), { key: "Enter" });

    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("does not submit on ⌘↩ when the text is blank", () => {
    const { onSubmit } = renderCapture();
    fireEvent.keyDown(input(), { key: "Enter", metaKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("closes on Escape", () => {
    const { onClose } = renderCapture();
    fireEvent.keyDown(input(), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not let Escape bubble out of the dialog", () => {
    /* 面板和宿主也有 Esc 的处理,不拦住的话一次按键关两层 —— 而用户只看得见
       最上面这一层。 */
    const onOuter = vi.fn();
    render(
      // 这层 div 只是测「事件冒不冒上来」的探针,不是给用户点的控件。
      <div onKeyDown={onOuter}>
        <NoteQuickCapture
          paths={PATHS}
          busy={false}
          error={null}
          onSubmit={vi.fn()}
          onClose={vi.fn()}
          t={t}
        />
      </div>,
    );

    fireEvent.keyDown(screen.getByRole("textbox", { name: "notebook.captureInput" }), {
      key: "Escape",
    });

    expect(onOuter).not.toHaveBeenCalled();
  });

  it("ignores Escape and ⌘↩ while an IME is composing", () => {
    /* 组字中的 Escape 是「取消候选词」,组字中的回车是「确认候选词」。不挡的话中文
       输入法下打第一个字就把窗关了或者提交了。 */
    const { onSubmit, onClose } = renderCapture();
    fireEvent.change(input(), { target: { value: "记" } });

    fireEvent.keyDown(input(), { key: "Escape", isComposing: true });
    fireEvent.keyDown(input(), { key: "Enter", metaKey: true, isComposing: true });

    expect(onClose).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("locks the controls while saving and says so", () => {
    renderCapture({ busy: true });
    expect(input()).toBeDisabled();
    expect(screen.getByRole("button", { name: "notebook.captureSaving" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "common.cancel" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "notebook.captureToday" })).toBeDisabled();
  });

  it("shows the error as an alert and keeps the typed text", () => {
    /* 失败时窗不关、文字不清:捕获的那句话只存在这个 textarea 里,关掉就没了。
       所以报错必须和内容出现在同一个地方。 */
    const { view } = renderCapture();
    fireEvent.change(input(), { target: { value: "记一句" } });

    view.rerender(
      <NoteQuickCapture
        paths={PATHS}
        busy={false}
        error="disk is on fire"
        onSubmit={vi.fn()}
        onClose={vi.fn()}
        t={t}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("disk is on fire");
    expect(input()).toHaveValue("记一句");
    // 还能再试一次 —— 出错之后按钮仍然是可用的。
    expect(saveButton()).toBeEnabled();
  });

  it("groups the two targets as one radio group", () => {
    // 两个落点是互斥的一项选择,不是两个独立开关。读屏靠这个念出「二选一」。
    renderCapture();
    const group = screen.getByRole("radiogroup", { name: "notebook.captureTargetLabel" });
    expect(group).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(2);
  });
});
