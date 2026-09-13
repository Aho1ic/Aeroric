/**
 * `CleanupReportPanel` 的行为。
 *
 * 这个面板控制的是一个**不可逆**动作(物理删除对话记录),所以要钉住三件事:
 * 开关默认是关的、切到哪个模式就只显示那一组字段、每次改动都立刻发对应的 setter 命令。
 * 第一条错了会在用户不知情时删数据;第三条错了表现为"改完设置重启就回默认",而
 * 面板看起来一切正常。
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import { I18nProvider } from "../i18n";
import { CleanupReportPanel } from "../components/app-settings/CleanupReportPanel";
import { APP_SETTINGS_CHANGED_EVENT } from "../components/app-settings/types";
import type { AutoCleanupSettings, WeeklyReportSettings } from "../components/app-settings/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const pickExportDirMock = vi.hoisted(() => vi.fn());
vi.mock("../components/notebook/noteExport", () => ({
  pickExportDir: pickExportDirMock,
}));

interface Stored {
  auto_cleanup_settings?: Partial<AutoCleanupSettings>;
  weekly_report_settings?: Partial<WeeklyReportSettings>;
}

/** 从 mock 收到的 args 里取一个字段。用 `in` 收窄而不是内联断言:载荷键写错时立刻炸,而不是静默返回 undefined。 */
function payloadField(args: unknown, key: string): unknown {
  if (!args || typeof args !== "object" || !(key in args)) {
    throw new Error(`payload is missing "${key}": ${JSON.stringify(args)}`);
  }
  return (args as Record<string, unknown>)[key];
}

function installBackend(stored: Stored = {}) {
  vi.mocked(invoke).mockImplementation((command, args) => {
    switch (command) {
      case "load_app_settings":
        return Promise.resolve(structuredClone(stored));
      case "update_auto_cleanup_settings":
        return Promise.resolve({
          auto_cleanup_settings: payloadField(args, "autoCleanupSettings"),
          weekly_report_settings: stored.weekly_report_settings,
        });
      case "update_weekly_report_settings":
        return Promise.resolve({
          auto_cleanup_settings: stored.auto_cleanup_settings,
          weekly_report_settings: payloadField(args, "weeklyReportSettings"),
        });
      default:
        return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    }
  });
}

function renderPanel() {
  localStorage.setItem("aeroric:language", "en");
  return render(
    <I18nProvider>
      <CleanupReportPanel />
    </I18nProvider>,
  );
}

function toggle() {
  return screen.getByRole("switch", { name: "Enable scheduled auto-delete" });
}

/** 最后一次某命令的载荷字段。没有这条命令的调用时直接炸,而不是返回 undefined 让断言变空转。 */
function lastPayloadField(command: string, key: string): unknown {
  const calls = vi.mocked(invoke).mock.calls.filter(([name]) => name === command);
  if (calls.length === 0) throw new Error(`${command} was never invoked`);
  return payloadField(calls[calls.length - 1][1], key);
}

describe("CleanupReportPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    pickExportDirMock.mockReset();
    // jsdom 未实现这几个 API,Radix Select 打开时会调用它们。
    Element.prototype.hasPointerCapture ??= () => false;
    Element.prototype.releasePointerCapture ??= () => {};
    Element.prototype.scrollIntoView ??= () => {};
  });

  it("旧配置(整段缺失)下开关是关的,默认区间是周一到周日", async () => {
    // 默认开启的物理删除会在用户毫不知情时清掉半年记录 —— 这条断言就是那道闸。
    installBackend({});
    renderPanel();

    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "false"));
    expect(screen.getByLabelText("Week starts on")).toHaveTextContent("Monday");
    expect(screen.getByLabelText("Week ends on")).toHaveTextContent("Sunday");
    expect(screen.getByLabelText("Retention")).toHaveTextContent("30 days");
  });

  it("开关翻开立刻落盘并广播设置变更", async () => {
    // 不广播的表现是:改完配置得重启才生效,而面板显示已经生效。
    installBackend({});
    const changed = vi.fn();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, changed);
    try {
      const user = userEvent.setup();
      renderPanel();
      await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "false"));

      await user.click(toggle());

      await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "true"));
      expect(lastPayloadField("update_auto_cleanup_settings", "autoCleanupSettings")).toMatchObject(
        { enabled: true },
      );
      expect(changed).toHaveBeenCalled();
    } finally {
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, changed);
    }
  });

  it("weekly 模式显示星期与时间,interval 模式换成间隔天数", async () => {
    // 两组字段同时出现会让用户以为"每周日 20:00 且每 7 天"都生效,而实际只用一组。
    installBackend({ auto_cleanup_settings: { mode: "weekly" } });
    const user = userEvent.setup();
    renderPanel();
    await waitFor(() => expect(screen.getByLabelText("Day of week")).toBeInTheDocument());
    expect(screen.getByLabelText("Time")).toHaveTextContent("20:00");
    expect(screen.queryByLabelText("Interval")).toBeNull();

    await user.click(screen.getByLabelText("Schedule"));
    await user.click(await screen.findByRole("option", { name: "Every N days" }));

    await waitFor(() => expect(screen.getByLabelText("Interval")).toBeInTheDocument());
    expect(screen.queryByLabelText("Day of week")).toBeNull();
    expect(screen.queryByLabelText("Time")).toBeNull();
    expect(lastPayloadField("update_auto_cleanup_settings", "autoCleanupSettings")).toMatchObject({
      mode: "interval",
    });
  });

  it("周起始日改动发到周报 setter,不串到清理 setter", async () => {
    installBackend({});
    const user = userEvent.setup();
    renderPanel();
    await waitFor(() => expect(screen.getByLabelText("Week starts on")).toBeInTheDocument());

    await user.click(screen.getByLabelText("Week starts on"));
    const listbox = await screen.findByRole("listbox");
    await user.click(within(listbox).getByRole("option", { name: "Sunday" }));

    await waitFor(() =>
      expect(
        lastPayloadField("update_weekly_report_settings", "weeklyReportSettings"),
      ).toMatchObject({ week_start_day: 0 }),
    );
    expect(
      vi.mocked(invoke).mock.calls.filter(([name]) => name === "update_auto_cleanup_settings"),
    ).toEqual([]);
  });

  it("浏览按钮选中的目录直接落盘,取消则什么都不写", async () => {
    installBackend({});
    const user = userEvent.setup();
    renderPanel();
    await waitFor(() => expect(screen.getByLabelText("Browse…")).toBeInTheDocument());

    pickExportDirMock.mockResolvedValueOnce(null);
    await user.click(screen.getByLabelText("Browse…"));
    await waitFor(() => expect(pickExportDirMock).toHaveBeenCalledTimes(1));
    expect(
      vi.mocked(invoke).mock.calls.filter(([name]) => name === "update_weekly_report_settings"),
    ).toEqual([]);

    pickExportDirMock.mockResolvedValueOnce("/Users/me/reports");
    await user.click(screen.getByLabelText("Browse…"));
    await waitFor(() =>
      expect(
        lastPayloadField("update_weekly_report_settings", "weeklyReportSettings"),
      ).toMatchObject({ output_dir: "/Users/me/reports" }),
    );
  });

  it("手改过的越界值在面板里被夹回合法范围", async () => {
    // 后端也会夹一次;前端这一层保证越界值不会先在 UI 上显示成 "99" 再被静默改掉。
    installBackend({
      auto_cleanup_settings: { mode: "interval", interval_days: 0, retain_days: 99999 },
    });
    renderPanel();

    await waitFor(() => expect(screen.getByLabelText("Interval")).toHaveTextContent("1 days"));
    // 3650 不在下拉选项里,回退成原值文本而不是崩掉。
    expect(screen.getByLabelText("Retention")).toHaveTextContent("3650");
  });

  it("收到 APP_SETTINGS_CHANGED_EVENT 后面板重读并展示新值", async () => {
    /* 这个面板自己会广播同一条事件,但它不是这段设置的唯一写入方:自动清理跑完会
       更新 `last_run_at`,周报按钮也会写 `output_dir`。只在挂载时读一次的话,别处改完
       这里显示的还是打开那一刻的快照。 */
    const stored: Stored = {
      auto_cleanup_settings: { retain_days: 30 },
      weekly_report_settings: { output_dir: "/old" },
    };
    installBackend(stored);
    renderPanel();
    await waitFor(() => expect(screen.getByLabelText("Retention")).toHaveTextContent("30 days"));

    stored.auto_cleanup_settings = { retain_days: 90 };
    stored.weekly_report_settings = { output_dir: "/Users/me/reports" };
    window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));

    await waitFor(() => expect(screen.getByLabelText("Retention")).toHaveTextContent("90 days"));
    expect(screen.getByDisplayValue("/Users/me/reports")).toBeInTheDocument();
  });
});
