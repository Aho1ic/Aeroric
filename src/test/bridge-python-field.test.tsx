import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  BridgePythonField,
  type ChatBridgePythonStatus,
} from "../components/app-settings/BridgePythonField";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function status(overrides: Partial<ChatBridgePythonStatus> = {}): ChatBridgePythonStatus {
  return {
    usable: true,
    program: "/usr/bin/python3",
    version: "3.12",
    configured: false,
    failure: "",
    checked: [],
    ...overrides,
  };
}

function renderField(props: Partial<Parameters<typeof BridgePythonField>[0]> = {}) {
  return render(
    <I18nProvider>
      <BridgePythonField value="" onChange={() => {}} autoProbe {...props} />
    </I18nProvider>,
  );
}

describe("BridgePythonField", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  /// 预检必须在挂载时自动跑,让缺失在保存之前暴露,而不是等用户去启动终端。
  it("probes on mount so a missing Python surfaces before saving", async () => {
    mockInvoke.mockResolvedValue(status());
    renderField();
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("probe_chat_bridge_python", {
        bridgePythonPath: "",
      }),
    );
    expect(await screen.findByText(/3\.12/)).toBeInTheDocument();
  });

  /// 自动探测全失败时必须给出可执行的下一步,并点名商店桩不算——这正是用户那台新
  /// Windows 机器的情况。
  it("names the Store alias stub when auto-detect finds nothing usable", async () => {
    mockInvoke.mockResolvedValue(
      status({
        usable: false,
        program: "",
        version: "",
        checked: ["python3 -> Python was not found; run without arguments to install"],
      }),
    );
    renderField();
    expect(
      await screen.findByText(/Microsoft Store alias stub does not count/),
    ).toBeInTheDocument();
    expect(screen.getByText(/python3 -> Python was not found/)).toBeInTheDocument();
  });

  /// 显式配置的路径不可用时,报错要说这条解释器不行,而不是笼统地"去装 Python"。
  it("reports why a configured interpreter cannot run the bridge", async () => {
    mockInvoke.mockResolvedValue(
      status({
        usable: false,
        program: "",
        version: "",
        configured: true,
        failure: "Python 3.8 is too old (need 3.9+)",
      }),
    );
    renderField({ value: "/opt/py38/bin/python3" });
    expect(await screen.findByText(/Python 3\.8 is too old/)).toBeInTheDocument();
    expect(screen.queryByText(/Microsoft Store alias stub/)).not.toBeInTheDocument();
  });

  /// 改了路径就必须清掉旧结论,否则会把上一条路径的"可用"错配到新路径上。
  it("clears a stale verdict when the path changes", async () => {
    mockInvoke.mockResolvedValue(status());
    function Host() {
      const [value, setValue] = useState("");
      return <BridgePythonField value={value} onChange={setValue} autoProbe />;
    }
    render(
      <I18nProvider>
        <Host />
      </I18nProvider>,
    );
    expect(await screen.findByText(/3\.12/)).toBeInTheDocument();

    const input = screen.getByLabelText("Bridge Python interpreter");
    await userEvent.type(input, "/opt/py/bin/python3");
    expect(input).toHaveValue("/opt/py/bin/python3");
    expect(screen.queryByText(/3\.12/)).not.toBeInTheDocument();
    // 清结论不等于重新探测:改一个字符就打一次后端会把探测打成输入抖动。
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("re-probes the current path when the check button is pressed", async () => {
    mockInvoke.mockResolvedValue(status({ configured: true, program: "/opt/py/bin/python3" }));
    renderField({ value: "/opt/py/bin/python3" });
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledTimes(1));

    await userEvent.click(screen.getByRole("button", { name: "Check" }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledTimes(2));
    expect(mockInvoke).toHaveBeenLastCalledWith("probe_chat_bridge_python", {
      bridgePythonPath: "/opt/py/bin/python3",
    });
  });

  /// 预检本身失败(命令不存在、后端报错)不能卡在"检测中",否则用户以为还在跑。
  it("surfaces a probe failure instead of staying in the checking state", async () => {
    mockInvoke.mockRejectedValue(new Error("probe crashed"));
    renderField();
    expect(await screen.findByText(/probe crashed/)).toBeInTheDocument();
    expect(screen.queryByText("Checking…")).not.toBeInTheDocument();
  });

  it("does not probe while disabled", async () => {
    mockInvoke.mockResolvedValue(status());
    renderField({ disabled: true });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(mockInvoke).not.toHaveBeenCalled();
  });
});
