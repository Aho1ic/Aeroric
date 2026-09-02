import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { DshQuestionRequest } from "../components/DshQuestionDialog";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { DshQuestionDialog } = await import("../components/DshQuestionDialog");

/**
 * Agent 提问弹窗。它是 DSH RPC 的一端:提交/取消都要回 `respond_dsh_server_request`,
 * 回不成功就必须留在原地显示错误 —— 直接关掉会让对端永远等下去。
 */

function makeRequest(overrides: Partial<DshQuestionRequest> = {}): DshQuestionRequest {
  return {
    rpcId: "rpc-1",
    sessionId: "sess-1",
    questions: [
      {
        id: "q1",
        question: "Which database?",
        options: [{ label: "Postgres" }, { label: "SQLite" }],
      },
    ],
    ...overrides,
  };
}

function renderDialog(request: DshQuestionRequest | null = makeRequest()) {
  const onClose = vi.fn();
  const result = render(
    <I18nProvider>
      <DshQuestionDialog request={request} onClose={onClose} />
    </I18nProvider>,
  );
  return { ...result, onClose };
}

function dialog() {
  return screen.getByRole("dialog");
}

function submitButton() {
  return screen.getByRole("button", { name: /^(Submit|Submitting)/ });
}

function cancelButton() {
  return screen.getByRole("button", { name: "Cancel" });
}

/**
 * 选项按钮按「问题文本 → 该问题的区块 → 精确 label」三步取。
 * 直接在整个弹窗里按正则找会串题:多问题时 label "B" 同时命中另一题的 "Second?"。
 */
function optionButton(label: string, questionText?: string) {
  const scope = questionText
    ? (screen.getByText(questionText).parentElement as HTMLElement)
    : dialog();
  return within(scope)
    .getAllByRole("button")
    .find((b) => b.textContent?.trim().startsWith(label))!;
}

/** 提交时真正发出去的 answers 数组。 */
function submittedAnswers() {
  const call = invoke.mock.calls.find(
    ([name, args]) =>
      name === "respond_dsh_server_request" &&
      (args as { result: { ok: boolean } }).result.ok === true,
  );
  expect(call).toBeDefined();
  return (
    call![1] as {
      result: {
        value: { answer: { answers: Array<{ id: string; selected: string[]; custom?: string }> } };
      };
    }
  ).result.value.answer.answers;
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
});

describe("DshQuestionDialog 显隐", () => {
  it("request 为 null 时什么都不渲染", () => {
    const { container } = renderDialog(null);
    expect(container).toBeEmptyDOMElement();
  });

  it("有 request 时渲染弹窗与标题", () => {
    renderDialog();
    expect(dialog()).toBeInTheDocument();
    expect(screen.getByText("Agent Question")).toBeInTheDocument();
    expect(screen.getByText("Which database?")).toBeInTheDocument();
  });

  it("header 与 detail 有就渲染", () => {
    renderDialog(
      makeRequest({
        questions: [
          {
            id: "q1",
            question: "Pick one",
            header: "Storage",
            detail: "This affects migrations.",
            options: [{ label: "A" }],
          },
        ],
      }),
    );
    expect(screen.getByText("Storage")).toBeInTheDocument();
    expect(screen.getByText("This affects migrations.")).toBeInTheDocument();
  });

  it("没有 options 时只留自定义输入框", () => {
    renderDialog(makeRequest({ questions: [{ id: "q1", question: "Anything to add?" }] }));
    expect(screen.getByPlaceholderText("Other (optional)")).toBeInTheDocument();
    // 除了提交/取消之外没有选项按钮。
    expect(within(dialog()).getAllByRole("button")).toHaveLength(2);
  });

  it("选项的说明文案会渲染", () => {
    renderDialog(
      makeRequest({
        questions: [
          {
            id: "q1",
            question: "Pick",
            options: [{ label: "Fast", description: "Lower durability" }],
          },
        ],
      }),
    );
    expect(screen.getByText("Lower durability")).toBeInTheDocument();
  });

  it("多个问题全部渲染", () => {
    renderDialog(
      makeRequest({
        questions: [
          { id: "q1", question: "First?", options: [{ label: "A" }] },
          { id: "q2", question: "Second?", options: [{ label: "B" }] },
        ],
      }),
    );
    expect(screen.getByText("First?")).toBeInTheDocument();
    expect(screen.getByText("Second?")).toBeInTheDocument();
  });
});

describe("DshQuestionDialog 单选", () => {
  it("选一个之后再选另一个会替换掉前一个", async () => {
    renderDialog();
    fireEvent.click(optionButton("Postgres"));
    fireEvent.click(optionButton("SQLite"));
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: ["SQLite"] }]);
  });

  it("重复点同一个选项不会把它取消(单选没有反选)", async () => {
    renderDialog();
    fireEvent.click(optionButton("Postgres"));
    fireEvent.click(optionButton("Postgres"));
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: ["Postgres"] }]);
  });
});

describe("DshQuestionDialog 多选", () => {
  const multi = makeRequest({
    questions: [
      {
        id: "q1",
        question: "Which features?",
        multiSelect: true,
        options: [{ label: "A" }, { label: "B" }, { label: "C" }],
      },
    ],
  });

  it("可以同时选中多个,顺序按点击顺序", async () => {
    renderDialog(multi);
    fireEvent.click(optionButton("B"));
    fireEvent.click(optionButton("A"));
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: ["B", "A"] }]);
  });

  it("再点一次会取消该项,其余保留", async () => {
    renderDialog(multi);
    fireEvent.click(optionButton("A"));
    fireEvent.click(optionButton("B"));
    fireEvent.click(optionButton("A"));
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: ["B"] }]);
  });

  it("全部取消后回到空数组而不是 undefined", async () => {
    renderDialog(multi);
    fireEvent.click(optionButton("A"));
    fireEvent.click(optionButton("A"));
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: [] }]);
  });
});

describe("DshQuestionDialog 自定义答案", () => {
  it("填了就带上,首尾空白被去掉", async () => {
    renderDialog();
    fireEvent.change(screen.getByPlaceholderText("Other (optional)"), {
      target: { value: "  MySQL  " },
    });
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: [], custom: "MySQL" }]);
  });

  it("只填空白等于没填(custom 不出现)", async () => {
    renderDialog();
    fireEvent.change(screen.getByPlaceholderText("Other (optional)"), {
      target: { value: "   " },
    });
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: [], custom: undefined }]);
  });

  it("清空已填内容会把 custom 撤掉", async () => {
    renderDialog();
    const input = screen.getByPlaceholderText("Other (optional)");
    fireEvent.change(input, { target: { value: "MySQL" } });
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: [], custom: undefined }]);
  });

  it("选项与自定义可以同时给出", async () => {
    renderDialog();
    fireEvent.click(optionButton("Postgres"));
    fireEvent.change(screen.getByPlaceholderText("Other (optional)"), {
      target: { value: "with pgvector" },
    });
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([
      { id: "q1", selected: ["Postgres"], custom: "with pgvector" },
    ]);
  });

  it("多问题各自独立记录答案", async () => {
    renderDialog(
      makeRequest({
        questions: [
          { id: "q1", question: "First?", options: [{ label: "A" }, { label: "B" }] },
          { id: "q2", question: "Second?", options: [{ label: "C" }, { label: "D" }] },
        ],
      }),
    );
    // 先钉住作用域边界:`optionButton` 用 `.find()`,scope 取宽了不会报错、
    // 只会静默返回 undefined 或串到另一题。这两条断言保证按题隔离是真的 ——
    // 第一题的区块里找不到第二题的 C,反之同理。
    expect(optionButton("C", "First?")).toBeUndefined();
    expect(optionButton("B", "Second?")).toBeUndefined();
    fireEvent.click(optionButton("B", "First?"));
    fireEvent.click(optionButton("C", "Second?"));
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([
      { id: "q1", selected: ["B"] },
      { id: "q2", selected: ["C"] },
    ]);
  });

  it("一个都没选也能提交(答案是空的,不是不发)", async () => {
    renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(submittedAnswers()).toEqual([{ id: "q1", selected: [] }]);
  });
});

describe("RPC 契约:提交", () => {
  it("提交带上 rpcId / sessionId,且 sessionId 在 value 里也重复一份", async () => {
    renderDialog(makeRequest({ rpcId: "rpc-42", sessionId: "sess-42" }));
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    const [name, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(name).toBe("respond_dsh_server_request");
    expect(args.rpcId).toBe("rpc-42");
    expect(args.sessionId).toBe("sess-42");
    // value.sessionId 是协议要求的第二份,漏了对端认不出这条回包
    expect(args.result).toMatchObject({ ok: true, value: { sessionId: "sess-42" } });
  });

  it("提交成功才 onClose", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("提交失败:留在原地报错,不 onClose —— 关掉会让对端永远等", async () => {
    invoke.mockRejectedValueOnce(new Error("relay offline"));
    const { onClose } = renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("relay offline"));
    expect(onClose).not.toHaveBeenCalled();
    // submitting 必须复位,否则用户卡在这个弹窗里既提交不了也取消不了
    expect(submitButton()).not.toBeDisabled();
  });

  it("失败后可以再提交一次,第二次成功就关掉", async () => {
    invoke.mockRejectedValueOnce(new Error("boom"));
    const { onClose } = renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
    fireEvent.click(submitButton());
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("非 Error 抛出也能显示(String 兜底,不是显示 [object Object])", async () => {
    invoke.mockRejectedValueOnce("plain string failure");
    renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("plain string failure"),
    );
  });
});

describe("RPC 契约:取消", () => {
  /** 取消时发出去的 error 对象。 */
  function cancelError() {
    const call = invoke.mock.calls.find(
      ([name, args]) =>
        name === "respond_dsh_server_request" &&
        (args as { result: { ok: boolean } }).result.ok === false,
    );
    expect(call).toBeDefined();
    return (call![1] as { result: { error: Record<string, unknown> } }).result.error;
  }

  it("Cancel 也要回包,而且是 ok:false + code cancelled", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(cancelButton());
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(cancelError()).toEqual({
      code: "cancelled",
      message: "the user closed this question request",
      details: {},
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("取消失败同样留在原地报错", async () => {
    invoke.mockRejectedValueOnce(new Error("send failed"));
    const { onClose } = renderDialog();
    fireEvent.click(cancelButton());
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("send failed"));
    expect(onClose).not.toHaveBeenCalled();
    expect(cancelButton()).not.toBeDisabled();
  });

  it("点遮罩空白处 = 取消", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(dialog().parentElement as HTMLElement);
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(cancelError().code).toBe("cancelled");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("点弹窗内部不算取消(事件冒到遮罩上但 target 不是遮罩)", () => {
    // 这条钉住实现里的 `e.target === e.currentTarget`:去掉之后点标题就会误关。
    renderDialog();
    fireEvent.click(screen.getByRole("heading"));
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("提交中:重复回包的闸门", () => {
  /** 卡住 invoke,让组件停在 submitting 状态里。 */
  function deferInvoke() {
    let resolve!: () => void;
    invoke.mockImplementationOnce(
      () =>
        new Promise<void>((r) => {
          resolve = r;
        }),
    );
    return () => resolve();
  }

  it("提交中按钮变 Submitting 且两个按钮都禁用", async () => {
    deferInvoke();
    renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(submitButton()).toHaveTextContent("Submitting"));
    expect(submitButton()).toBeDisabled();
    expect(cancelButton()).toBeDisabled();
  });

  it("提交中再点提交不会发第二次(一个 rpcId 只能回一次)", async () => {
    deferInvoke();
    renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    fireEvent.click(submitButton());
    fireEvent.click(submitButton());
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("提交中点遮罩不会再发一条取消", async () => {
    // 遮罩点击是绕过按钮 disabled 的那条路径:同一个 rpcId 先收到 ok:true 再收到
    // cancelled,对端状态直接错乱。
    // 变异测试结论:这里有两道闸门 —— 遮罩 onClick 的 `!submitting`,和 handleCancel
    // 开头的 `if (submitting || ...) return`。单独摘掉任意一道本文件全绿(互相兜底),
    // 两道一起摘才被这条用例抓到。所以它守的是真实危险,只是钉不住具体哪一道。
    deferInvoke();
    renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    fireEvent.click(dialog().parentElement as HTMLElement);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("提交完成后闸门放开(证明上面几条不是因为一直没到过可点状态)", async () => {
    const release = deferInvoke();
    const { onClose } = renderDialog();
    fireEvent.click(submitButton());
    await waitFor(() => expect(submitButton()).toBeDisabled());
    release();
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });
});

describe("自定义输入框的焦点环", () => {
  function customInput() {
    return within(dialog()).getByRole("textbox");
  }

  it("聚焦加上焦点环,失焦复原", () => {
    // 焦点环是键盘用户唯一的位置指示,直接写在 inline style 上,
    // 没有对应 CSS 类可断言 —— 只能读 style。
    renderDialog();
    const input = customInput();
    // 初始是 "",不是 "none" —— style 里根本没声明 boxShadow,
    // "none" 只会在 blur 之后出现。
    expect(input.style.boxShadow).toBe("");

    fireEvent.focus(input);
    expect(input.style.borderColor).toBe("var(--ring)");
    expect(input.style.boxShadow).not.toBe("none");

    fireEvent.blur(input);
    expect(input.style.borderColor).toBe("var(--border-medium)");
    expect(input.style.boxShadow).toBe("none");
  });
});
