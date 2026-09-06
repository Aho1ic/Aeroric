import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { OmpUiRequest } from "../ompUiRequests";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { OmpApprovalDialog } = await import("../components/OmpApprovalDialog");

/**
 * omp 审批弹窗(extension_ui_request 的 confirm 方法)。允许/拒绝都要回
 * `respond_omp_server_request`,回不成功必须留在原地显示错误。
 */

function makeRequest(overrides: Partial<OmpUiRequest> = {}): OmpUiRequest {
  return {
    taskId: "task-1",
    requestId: "req-1",
    method: "confirm",
    title: "Run bash command?",
    message: "rm -rf ./build",
    ...overrides,
  };
}

function renderDialog(request: OmpUiRequest | null = makeRequest()) {
  const onClose = vi.fn();
  const result = render(
    <I18nProvider>
      <OmpApprovalDialog request={request} onClose={onClose} />
    </I18nProvider>,
  );
  return { ...result, onClose };
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
});

describe("OmpApprovalDialog 显隐", () => {
  it("request 为 null 时什么都不渲染", () => {
    const { container } = renderDialog(null);
    expect(container).toBeEmptyDOMElement();
  });

  it("渲染 title 与 message", () => {
    renderDialog();
    expect(screen.getByRole("dialog")).toHaveTextContent("Run bash command?");
    expect(screen.getByRole("dialog")).toHaveTextContent("rm -rf ./build");
  });
});

describe("OmpApprovalDialog 回应", () => {
  it("Allow 回 confirmed:true 并关闭", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: /^Allow once/ }));
    await vi.waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    const call = invoke.mock.calls.find(([name]) => name === "respond_omp_server_request");
    expect(call).toBeDefined();
    expect(call![1]).toMatchObject({
      taskId: "task-1",
      requestId: "req-1",
      response: { confirmed: true },
    });
  });

  it("Reject 回 confirmed:false 并关闭", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Reject" }));
    await vi.waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    const call = invoke.mock.calls.find(([name]) => name === "respond_omp_server_request");
    expect((call![1] as { response: { confirmed: boolean } }).response.confirmed).toBe(false);
  });

  it("回应失败时不关闭弹窗并显示错误", async () => {
    invoke.mockRejectedValueOnce(new Error("omp task is not running"));
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: /^Allow once/ }));
    await screen.findByRole("alert");
    expect(onClose).not.toHaveBeenCalled();
  });
});
