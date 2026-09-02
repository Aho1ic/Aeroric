import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { McpPanel } from "../components/app-settings/McpPanel";
import { APP_SETTINGS_CHANGED_EVENT, type McpSettings } from "../components/app-settings/types";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

type TestResult =
  | { status: "success"; message: string; serverName?: string; serverVersion?: string }
  | { status: "error"; message: string; stderr?: string }
  | { status: "timeout"; message: string };

const emptySettings: McpSettings = { servers: {}, enabled: false };

const withOneServer: McpSettings = {
  enabled: true,
  servers: {
    filesystem: {
      name: "filesystem",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      env: { API_KEY: "secret", DEBUG: "true" },
      enabled: true,
    },
  },
};

/**
 * `get_mcp_settings` 回 `loaded`;`set_mcp_settings` 默认把收到的 settings 原样回传
 * (后端真实行为:回落盘后的结果)。其余命令按用例覆盖。
 */
function installBackend(
  loaded: McpSettings | Error,
  overrides: Partial<{
    save: (settings: McpSettings) => Promise<unknown>;
    test: () => Promise<unknown>;
  }> = {},
) {
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "get_mcp_settings") {
      return loaded instanceof Error
        ? Promise.reject(loaded)
        : Promise.resolve(structuredClone(loaded));
    }
    if (command === "set_mcp_settings") {
      const settings = (args as { settings: McpSettings }).settings;
      return overrides.save ? overrides.save(settings) : Promise.resolve(structuredClone(settings));
    }
    if (command === "test_mcp_server") {
      return overrides.test
        ? overrides.test()
        : Promise.resolve({ status: "success", message: "" });
    }
    return Promise.reject(new Error(`unexpected command: ${String(command)}`));
  });
}

function renderPanel() {
  return render(
    <I18nProvider>
      <McpPanel />
    </I18nProvider>,
  );
}

function callsTo(command: string) {
  return vi.mocked(invoke).mock.calls.filter(([c]) => c === command);
}

function savedSettings(index = 0): McpSettings {
  return (callsTo("set_mcp_settings")[index]?.[1] as { settings: McpSettings }).settings;
}

/** 等首屏加载完(loading 文案消失)。 */
async function waitForLoaded() {
  await waitFor(() => expect(screen.queryByText("Loading...")).not.toBeInTheDocument());
}

/**
 * 面板底部那个保存(落盘)。弹窗里也有个"Save",所以要排掉弹窗内的;
 * 文案在保存过程中会变成"Saving...",所以按前缀匹配而不是全等。
 */
function saveButton() {
  return screen
    .getAllByRole("button", { name: /^Sav(e|ing)/ })
    .find((b) => !b.closest('[role="dialog"]'))!;
}

/** 弹窗里的按钮 —— 与面板底部同名,必须限定在弹窗内取。 */
function inDialog(name: string | RegExp) {
  return within(screen.getByRole("dialog")).getByRole("button", { name });
}

async function openAddDialog() {
  fireEvent.click(screen.getByRole("button", { name: /Add MCP Server/ }));
  return screen.findByRole("dialog", { name: "Add MCP Server" });
}

describe("McpPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("加载", () => {
    it("挂载时读设置并渲染出来", async () => {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();

      expect(callsTo("get_mcp_settings")).toHaveLength(1);
      expect(screen.getByText("filesystem")).toBeInTheDocument();
      // 命令行 = command + args 用空格拼起来。
      expect(
        screen.getByText("npx -y @modelcontextprotocol/server-filesystem /tmp"),
      ).toBeInTheDocument();
    });

    it("读取失败时显示错误,并且停止 loading", async () => {
      installBackend(new Error("mcp config unreadable"));
      renderPanel();
      await waitForLoaded();
      expect(screen.getByText(/mcp config unreadable/)).toBeInTheDocument();
    });

    it("没有 server 时给空状态", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      expect(screen.getByText("No MCP servers configured yet.")).toBeInTheDocument();
    });

    it("刚加载完是 pristine,保存按钮禁用", async () => {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();
      // 只有改动过才允许保存,否则会把没变的配置反复写盘。
      expect(saveButton()).toBeDisabled();
    });
  });

  describe("保存", () => {
    it("改了总开关后可以保存,并广播设置变更事件", async () => {
      installBackend(withOneServer);
      const onChanged = vi.fn();
      window.addEventListener(APP_SETTINGS_CHANGED_EVENT, onChanged);
      try {
        renderPanel();
        await waitForLoaded();

        fireEvent.click(screen.getByLabelText("Enable MCP"));
        await waitFor(() => expect(saveButton()).not.toBeDisabled());
        fireEvent.click(saveButton());

        await waitFor(() => expect(callsTo("set_mcp_settings")).toHaveLength(1));
        expect(savedSettings().enabled).toBe(false);
        // 别的面板靠这个事件重读配置,漏掉就会显示旧值。
        expect(onChanged).toHaveBeenCalledTimes(1);
      } finally {
        window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, onChanged);
      }
    });

    it("保存成功后回到 pristine,保存按钮重新禁用", async () => {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByLabelText("Enable MCP"));
      await waitFor(() => expect(saveButton()).not.toBeDisabled());
      fireEvent.click(saveButton());

      await waitFor(() => expect(screen.getByText("Saved")).toBeInTheDocument());
      // 基线要跟着后端回来的结果走,否则保存完仍显示"有未保存改动"。
      expect(saveButton()).toBeDisabled();
    });

    it("保存成功提示 2 秒后消失", async () => {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByLabelText("Enable MCP"));
      await waitFor(() => expect(saveButton()).not.toBeDisabled());

      vi.useFakeTimers();
      fireEvent.click(saveButton());
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(screen.getByText("Saved")).toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_900);
      });
      expect(screen.getByText("Saved")).toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(200);
      });
      expect(screen.queryByText("Saved")).not.toBeInTheDocument();
      vi.useRealTimers();
    });

    it("采用后端回传的结果,而不是本地那份", async () => {
      // 后端会规范化(比如补 enabled 默认值),前端必须用回传值。
      installBackend(withOneServer, {
        save: () =>
          Promise.resolve({
            enabled: false,
            servers: { normalized: { name: "normalized", command: "uvx", enabled: true } },
          } satisfies McpSettings),
      });
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByLabelText("Enable MCP"));
      await waitFor(() => expect(saveButton()).not.toBeDisabled());
      fireEvent.click(saveButton());

      await waitFor(() => expect(screen.getByText("normalized")).toBeInTheDocument());
      expect(screen.queryByText("filesystem")).not.toBeInTheDocument();
      expect(saveButton()).toBeDisabled();
    });

    it("保存进行中按钮禁用,连点不会写两次", async () => {
      let finish: ((s: McpSettings) => void) | undefined;
      let pending: McpSettings | undefined;
      installBackend(withOneServer, {
        save: (settings) => {
          pending = settings;
          return new Promise<McpSettings>((resolve) => {
            finish = resolve;
          });
        },
      });
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByLabelText("Enable MCP"));
      await waitFor(() => expect(saveButton()).not.toBeDisabled());

      fireEvent.click(saveButton());
      await waitFor(() => expect(saveButton()).toHaveTextContent("Saving..."));
      // isDirty 在响应回来前一直是 true,只靠它挡不住第二次点击。
      expect(saveButton()).toBeDisabled();
      fireEvent.click(saveButton());
      await act(async () => {
        await Promise.resolve();
      });
      expect(callsTo("set_mcp_settings")).toHaveLength(1);

      await act(async () => {
        finish?.(pending!);
        await Promise.resolve();
      });
      await waitFor(() => expect(saveButton()).toHaveTextContent("Save"));
    });

    it("保存失败时显示错误,且不广播事件、不显示已保存", async () => {
      const onChanged = vi.fn();
      window.addEventListener(APP_SETTINGS_CHANGED_EVENT, onChanged);
      try {
        installBackend(withOneServer, {
          save: () => Promise.reject(new Error("disk is read-only")),
        });
        renderPanel();
        await waitForLoaded();
        fireEvent.click(screen.getByLabelText("Enable MCP"));
        await waitFor(() => expect(saveButton()).not.toBeDisabled());
        fireEvent.click(saveButton());

        await waitFor(() => expect(screen.getByText(/disk is read-only/)).toBeInTheDocument());
        expect(screen.queryByText("Saved")).not.toBeInTheDocument();
        expect(onChanged).not.toHaveBeenCalled();
        // 失败后按钮要能再点,不能卡在 saving。
        expect(saveButton()).not.toBeDisabled();
      } finally {
        window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, onChanged);
      }
    });
  });

  describe("单个 server 的开关", () => {
    it("勾掉某个 server 只改它的 enabled,不动别的字段", async () => {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();

      const rowToggle = screen.getAllByRole("checkbox")[1];
      fireEvent.click(rowToggle);
      await waitFor(() => expect(saveButton()).not.toBeDisabled());
      fireEvent.click(saveButton());

      await waitFor(() => expect(callsTo("set_mcp_settings")).toHaveLength(1));
      expect(savedSettings().servers.filesystem).toEqual({
        ...withOneServer.servers.filesystem,
        enabled: false,
      });
    });

    it("server 没写 enabled 时按启用显示", async () => {
      installBackend({ enabled: true, servers: { a: { name: "a", command: "uvx" } } });
      renderPanel();
      await waitForLoaded();
      expect(screen.getAllByRole("checkbox")[1]).toBeChecked();
    });
  });

  describe("新增 server", () => {
    it("名字为空时报必填,不写进列表", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();

      fireEvent.click(inDialog("Save"));
      await waitFor(() => expect(screen.getByText("Server name is required")).toBeInTheDocument());
      // 弹窗留着让用户改。
      expect(screen.getByRole("dialog", { name: "Add MCP Server" })).toBeInTheDocument();
    });

    it("只有空白字符的名字也算空", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();

      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "   " } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText("Server name is required")).toBeInTheDocument());
    });

    it("命令为空时报必填", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();

      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "fs" } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "  " } });
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText("Command is required")).toBeInTheDocument());
    });

    it("重名会被拦下来,不覆盖已有的那个", async () => {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();

      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "filesystem" } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "other" } });
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText(/already exists/)).toBeInTheDocument());
      expect(screen.getByRole("dialog", { name: "Add MCP Server" })).toBeInTheDocument();
      // 原来的命令没被改掉。
      fireEvent.click(inDialog("Cancel"));
      expect(
        screen.getByText("npx -y @modelcontextprotocol/server-filesystem /tmp"),
      ).toBeInTheDocument();
    });

    it("参数与环境变量按行解析,名字与命令两端裁空白", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();

      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "  fs  " } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "  npx  " } });
      fireEvent.change(screen.getByLabelText("Arguments"), {
        target: { value: "-y\n  @scope/pkg  \n\n/tmp\n" },
      });
      fireEvent.change(screen.getByLabelText("Environment Variables"), {
        // 第二行没有 `=`,应被丢掉;值里带 `=` 要保留;两端空白要裁掉
        // (不裁的话 key 会带前导空格,进程环境里就是另一个变量名)。
        target: { value: "  API_KEY=a=b  \nBROKEN\n\tDEBUG=true\n" },
      });
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText("fs")).toBeInTheDocument());
      // 存下去之后弹窗要关掉,否则用户以为没生效会再点一次。
      expect(screen.queryByRole("dialog", { name: "Add MCP Server" })).not.toBeInTheDocument();
      fireEvent.click(saveButton());
      await waitFor(() => expect(callsTo("set_mcp_settings")).toHaveLength(1));

      expect(savedSettings().servers.fs).toEqual({
        name: "fs",
        command: "npx",
        args: ["-y", "@scope/pkg", "/tmp"],
        env: { API_KEY: "a=b", DEBUG: "true" },
        enabled: true,
      });
    });

    it("等号在行首的不算环境变量", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();

      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "fs" } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.change(screen.getByLabelText("Environment Variables"), {
        target: { value: "=novalue\nOK=1" },
      });
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText("fs")).toBeInTheDocument());
      fireEvent.click(saveButton());
      await waitFor(() => expect(callsTo("set_mcp_settings")).toHaveLength(1));
      expect(savedSettings().servers.fs.env).toEqual({ OK: "1" });
    });

    it("新增时可以直接设为停用", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();

      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "fs" } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(screen.getByLabelText("Enabled"));
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText("fs")).toBeInTheDocument());
      fireEvent.click(saveButton());
      await waitFor(() => expect(callsTo("set_mcp_settings")).toHaveLength(1));
      expect(savedSettings().servers.fs.enabled).toBe(false);
    });

    it("加进列表只是暂存,没点保存不会落盘", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "fs" } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText("fs")).toBeInTheDocument());
      expect(callsTo("set_mcp_settings")).toHaveLength(0);
      expect(saveButton()).not.toBeDisabled();
    });

    it("Cancel 关闭弹窗且不加入列表", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: "ghost" } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Cancel"));

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Add MCP Server" })).not.toBeInTheDocument(),
      );
      expect(screen.queryByText("ghost")).not.toBeInTheDocument();
      expect(saveButton()).toBeDisabled();
    });

    it("点遮罩关闭弹窗,点弹窗本体不关", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      const dialog = await openAddDialog();

      fireEvent.mouseDown(dialog);
      expect(screen.getByRole("dialog", { name: "Add MCP Server" })).toBeInTheDocument();

      fireEvent.mouseDown(dialog.parentElement!);
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Add MCP Server" })).not.toBeInTheDocument(),
      );
    });
  });

  describe("编辑 server", () => {
    async function openEdit() {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByRole("button", { name: "Edit MCP Server" }));
      return screen.findByRole("dialog", { name: "Edit MCP Server" });
    }

    it("弹窗按现有值回填,参数和环境变量按行展开", async () => {
      await openEdit();
      expect(screen.getByLabelText("Server Name")).toHaveValue("filesystem");
      expect(screen.getByLabelText("Command")).toHaveValue("npx");
      expect(screen.getByLabelText("Arguments")).toHaveValue(
        "-y\n@modelcontextprotocol/server-filesystem\n/tmp",
      );
      expect(screen.getByLabelText("Environment Variables")).toHaveValue(
        "API_KEY=secret\nDEBUG=true",
      );
      expect(screen.getByLabelText("Enabled")).toBeChecked();
    });

    it("编辑态不允许改名(改名要走删了重建)", async () => {
      await openEdit();
      expect(screen.getByLabelText("Server Name")).toBeDisabled();
    });

    it("空 args / env 回填成空字符串,没写 enabled 的按启用回填", async () => {
      installBackend({ enabled: true, servers: { bare: { name: "bare", command: "uvx" } } });
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByRole("button", { name: "Edit MCP Server" }));
      await screen.findByRole("dialog", { name: "Edit MCP Server" });

      expect(screen.getByLabelText("Arguments")).toHaveValue("");
      expect(screen.getByLabelText("Environment Variables")).toHaveValue("");
      // 老配置里没有 enabled 字段。回填成 false 的话,用户只是改个命令
      // 就会把一个本来在跑的 server 顺手停掉。
      expect(screen.getByLabelText("Enabled")).toBeChecked();
    });

    it("改命令后写回同一个 key", async () => {
      await openEdit();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "uvx" } });
      fireEvent.click(inDialog("Save"));

      await waitFor(() => expect(screen.getByText(/^uvx/)).toBeInTheDocument());
      fireEvent.click(saveButton());
      await waitFor(() => expect(callsTo("set_mcp_settings")).toHaveLength(1));

      expect(Object.keys(savedSettings().servers)).toEqual(["filesystem"]);
      expect(savedSettings().servers.filesystem.command).toBe("uvx");
    });
  });

  describe("连通性测试", () => {
    it("命令为空时测试按钮禁用", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      expect(inDialog("Test Server")).toBeDisabled();

      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      await waitFor(() => expect(inDialog("Test Server")).not.toBeDisabled());
    });

    it("测试时把当前表单(裁过空白的)发给后端", async () => {
      installBackend(emptySettings);
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Server Name"), { target: { value: " fs " } });
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: " npx " } });
      fireEvent.change(screen.getByLabelText("Arguments"), { target: { value: "-y\n\npkg" } });
      fireEvent.click(inDialog("Test Server"));

      await waitFor(() => expect(callsTo("test_mcp_server")).toHaveLength(1));
      expect(callsTo("test_mcp_server")[0]?.[1]).toEqual({
        config: { name: "fs", command: "npx", args: ["-y", "pkg"], env: {}, enabled: true },
      });
    });

    it("成功时显示服务端自报的名字和版本", async () => {
      installBackend(emptySettings, {
        test: () =>
          Promise.resolve({
            status: "success",
            message: "connected",
            serverName: "filesystem",
            serverVersion: "1.2.3",
          } satisfies TestResult),
      });
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Test Server"));

      const status = await screen.findByRole("status");
      expect(status).toHaveTextContent("connected");
      expect(status).toHaveTextContent("filesystem");
      expect(status).toHaveTextContent("v1.2.3");
    });

    it("失败时把 stderr 一起显示出来(排障要看它)", async () => {
      installBackend(emptySettings, {
        test: () =>
          Promise.resolve({
            status: "error",
            message: "spawn failed",
            stderr: "npm ERR! 404 not found",
          } satisfies TestResult),
      });
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Test Server"));

      const status = await screen.findByRole("status");
      expect(status).toHaveTextContent("spawn failed");
      expect(status).toHaveTextContent("npm ERR! 404 not found");
    });

    it("超时按失败展示(告警图标,不是对勾)", async () => {
      installBackend(emptySettings, {
        test: () =>
          Promise.resolve({ status: "timeout", message: "timed out" } satisfies TestResult),
      });
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Test Server"));

      const status = await screen.findByRole("status");
      expect(status).toHaveTextContent("timed out");
      // 只断言文案的话,timeout 被当成 success 渲染(绿色 + 对勾)也照样通过。
      expect(status.querySelector(".lucide-triangle-alert")).not.toBeNull();
      expect(status.querySelector(".lucide-check")).toBeNull();
    });

    it("成功用对勾,不是告警图标", async () => {
      installBackend(emptySettings, {
        test: () =>
          Promise.resolve({ status: "success", message: "connected" } satisfies TestResult),
      });
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Test Server"));

      const status = await screen.findByRole("status");
      expect(status.querySelector(".lucide-check")).not.toBeNull();
      expect(status.querySelector(".lucide-triangle-alert")).toBeNull();
    });

    it("重新测试时先清掉上一次的结果", async () => {
      // 上一轮的"连接成功"留在屏幕上,而这一轮改了命令正在测:用户会照着
      // 过期的结论点保存。
      let finish: ((r: TestResult) => void) | undefined;
      let round = 0;
      installBackend(emptySettings, {
        test: () => {
          round += 1;
          if (round === 1) {
            return Promise.resolve({ status: "success", message: "first ok" } satisfies TestResult);
          }
          return new Promise<TestResult>((resolve) => {
            finish = resolve;
          });
        },
      });
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Test Server"));
      await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("first ok"));

      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "uvx" } });
      fireEvent.click(inDialog("Test Server"));
      await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());

      await act(async () => {
        finish?.({ status: "error", message: "second failed" });
        await Promise.resolve();
      });
      await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("second failed"));
    });

    it("IPC 直接 reject 时也显示成失败,不是白屏", async () => {
      installBackend(emptySettings, { test: () => Promise.reject(new Error("no such command")) });
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Test Server"));

      const status = await screen.findByRole("status");
      expect(status).toHaveTextContent("no such command");
    });

    it("测试过程中按钮显示进行中并禁用", async () => {
      let finish: ((r: TestResult) => void) | undefined;
      installBackend(emptySettings, {
        test: () =>
          new Promise<TestResult>((resolve) => {
            finish = resolve;
          }),
      });
      renderPanel();
      await waitForLoaded();
      await openAddDialog();
      fireEvent.change(screen.getByLabelText("Command"), { target: { value: "npx" } });
      fireEvent.click(inDialog("Test Server"));

      const testing = await Promise.resolve(inDialog("Testing MCP server..."));
      expect(testing).toBeDisabled();
      // 再点也不会打出第二次。
      fireEvent.click(testing);
      expect(callsTo("test_mcp_server")).toHaveLength(1);

      await act(async () => {
        finish?.({ status: "success", message: "ok" });
        await Promise.resolve();
      });
      await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("ok"));
    });

    // 清测试结果有两处:`closeDialog` 和 `openAddDialog`/`openEditDialog`。
    // 变异测试的结论:单独摘掉任何一处本文件仍全绿,两处同时摘掉才被抓到 ——
    // 它们互相兜底。这里断言的是"重开之后看不到旧结果"这个可观察行为,
    // 而不是某一处赋值,所以不为单处补测试。
    it("重开弹窗会清掉上次的测试结果", async () => {
      installBackend(withOneServer, {
        test: () =>
          Promise.resolve({ status: "success", message: "connected" } satisfies TestResult),
      });
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByRole("button", { name: "Edit MCP Server" }));
      await screen.findByRole("dialog", { name: "Edit MCP Server" });
      fireEvent.click(inDialog("Test Server"));
      await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("connected"));

      fireEvent.click(inDialog("Cancel"));
      await openAddDialog();
      // 上一次的结果留着会让人以为新表单已经测过了。
      expect(screen.queryByRole("status")).not.toBeInTheDocument();
    });
  });

  describe("删除 server", () => {
    async function openDeleteConfirm() {
      installBackend(withOneServer);
      renderPanel();
      await waitForLoaded();
      fireEvent.click(screen.getByRole("button", { name: "Delete MCP Server" }));
      return screen.findByRole("dialog", { name: "Delete MCP Server" });
    }

    it("确认框里带上要删的名字", async () => {
      const dialog = await openDeleteConfirm();
      expect(dialog).toHaveTextContent('Delete MCP server "filesystem"?');
    });

    it("确认后从列表移除,确认框关掉,保存时不再包含它", async () => {
      await openDeleteConfirm();
      fireEvent.click(inDialog("Delete"));

      await waitFor(() => expect(screen.queryByText("filesystem")).not.toBeInTheDocument());
      // 确认框不关的话会停在"删除 xxx?"上,而那个 server 已经没了。
      expect(screen.queryByRole("dialog", { name: "Delete MCP Server" })).not.toBeInTheDocument();
      fireEvent.click(saveButton());
      await waitFor(() => expect(callsTo("set_mcp_settings")).toHaveLength(1));
      expect(savedSettings().servers).toEqual({});
    });

    it("取消不删", async () => {
      await openDeleteConfirm();
      fireEvent.click(inDialog("Cancel"));

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Delete MCP Server" })).not.toBeInTheDocument(),
      );
      expect(screen.getByText("filesystem")).toBeInTheDocument();
      expect(saveButton()).toBeDisabled();
    });

    it("点遮罩等于取消", async () => {
      const dialog = await openDeleteConfirm();
      fireEvent.mouseDown(dialog.parentElement!);

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Delete MCP Server" })).not.toBeInTheDocument(),
      );
      expect(screen.getByText("filesystem")).toBeInTheDocument();
    });

    it("删除只是暂存,没点保存不落盘", async () => {
      await openDeleteConfirm();
      fireEvent.click(inDialog("Delete"));
      await waitFor(() => expect(screen.queryByText("filesystem")).not.toBeInTheDocument());
      expect(callsTo("set_mcp_settings")).toHaveLength(0);
    });
  });
});
