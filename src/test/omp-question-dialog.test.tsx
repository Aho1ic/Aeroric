import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { OmpUiRequest } from "../ompUiRequests";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { OmpQuestionDialog } = await import("../components/OmpQuestionDialog");

/**
 * omp 提问弹窗:`ask` 工具等 extension_ui_request 的 select/input/editor 三种
 * 方法。select 回所选 label,value 永远经 `respond_omp_server_request` 出站。
 */

function makeRequest(overrides: Partial<OmpUiRequest> = {}): OmpUiRequest {
  return {
    taskId: "task-1",
    requestId: "req-1",
    method: "select",
    title: "Which database?",
    message: "Pick a storage backend",
    options: ["Postgres", "SQLite"],
    optionDetails: [{ description: "default" }, { description: "embedded" }],
    ...overrides,
  };
}

function renderDialog(request: OmpUiRequest | null = makeRequest()) {
  const onClose = vi.fn();
  const result = render(
    <I18nProvider>
      <OmpQuestionDialog request={request} onClose={onClose} />
    </I18nProvider>,
  );
  return { ...result, onClose };
}

function submitButton() {
  return screen.getByRole("button", { name: /^(Submit|Submitting)/ });
}

function sentValue(): unknown {
  const call = invoke.mock.calls.find(([name]) => name === "respond_omp_server_request");
  expect(call).toBeDefined();
  return (call![1] as { response: { value?: unknown } }).response.value;
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
});

describe("OmpQuestionDialog select", () => {
  it("渲染选项与描述,未选择时提交禁用", () => {
    renderDialog();
    expect(screen.getByRole("dialog")).toHaveTextContent("Postgres");
    expect(screen.getByRole("dialog")).toHaveTextContent("embedded");
    expect(submitButton()).toBeDisabled();
  });

  it("选择选项后提交回所选 label", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByText("SQLite"));
    fireEvent.click(submitButton());
    await vi.waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    expect(sentValue()).toBe("SQLite");
  });
});

describe("OmpQuestionDialog input/editor", () => {
  it("input 方法输入文本后提交", async () => {
    const { onClose } = renderDialog(
      makeRequest({ method: "input", title: "Project name?", placeholder: "aeroric" }),
    );
    fireEvent.change(screen.getByPlaceholderText("aeroric"), {
      target: { value: "my-project" },
    });
    fireEvent.click(submitButton());
    await vi.waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    expect(sentValue()).toBe("my-project");
  });

  it("editor 方法带 prefill 可提交,清空后禁用", () => {
    renderDialog(makeRequest({ method: "editor", prefill: "draft text" }));
    const editor = screen.getByDisplayValue("draft text");
    expect(submitButton()).toBeEnabled();
    fireEvent.change(editor, { target: { value: "  " } });
    expect(submitButton()).toBeDisabled();
  });
});

describe("OmpQuestionDialog 取消", () => {
  it("Cancel 回 cancelled 帧并关闭", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await vi.waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    const call = invoke.mock.calls.find(([name]) => name === "respond_omp_server_request");
    expect((call![1] as { response: { cancelled: boolean } }).response.cancelled).toBe(true);
  });

  it("取消失败时不关闭并显示错误", async () => {
    invoke.mockRejectedValueOnce(new Error("omp task is not running"));
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await screen.findByRole("alert");
    expect(onClose).not.toHaveBeenCalled();
  });
});
