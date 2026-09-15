/**
 * 周报入口按钮的**位置**与触发行为。
 *
 * 位置这条是真实缺陷的回归:按钮早前挂在 `TimelineView` 外面、自带一条
 * `padding: "0 4px 8px"` 的 bar,于是它贴着窗口内容区最右上角、比面板内容右移一整个
 * padding,视觉上读作窗口装饰 —— 用户在"时间线右上角"找不到它,以为功能不存在。
 * 光看渲染没渲染钉不住这个:按钮当时确实在 DOM 里。所以断言的是**它在标题行里、
 * 与「时间线」标题同排**,那是面板 padding 之内、用户会看的地方。
 *
 * 触发行为只钉两条与位置无关会静默失效的:区间内没任务时不落盘(否则用户拿到空报告),
 * 以及输出目录已配时不再弹目录选择框(否则设置里那一项等于没用)。
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import { TimelineView } from "../components/TimelineView";
import { WeeklyReportButton } from "../components/WeeklyReportButton";
import type { Project, Task } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const pickExportDirMock = vi.hoisted(() => vi.fn());
vi.mock("../components/notebook/noteExport", () => ({ pickExportDir: pickExportDirMock }));

const PROJECT: Project = {
  id: "p1",
  name: "demo",
  path: "/tmp/demo",
} as Project;

function task(overrides: Partial<Task> = {}): Task {
  return {
    id: "t1",
    projectId: PROJECT.id,
    prompt: "改一处样式",
    agent: "claude",
    status: "done",
    createdAt: Date.now(),
    completedAt: Date.now(),
    ...overrides,
  } as Task;
}

function installBackend(outputDir: string) {
  vi.mocked(invoke).mockImplementation((command) => {
    switch (command) {
      case "load_app_settings":
        return Promise.resolve({
          weekly_report_settings: { week_start_day: 1, week_end_day: 0, output_dir: outputDir },
        });
      case "generate_weekly_report":
        return Promise.resolve(`${outputDir}/aeroric-weekly.md`);
      default:
        return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    }
  });
}

/** 与首页同一种装配:按钮由 `TimelineView` 作为 `headerAction` 渲染。 */
function renderTimeline(tasks: Task[]) {
  return render(
    <I18nProvider>
      <ToastProvider>
        <TimelineView
          projects={[PROJECT]}
          tasks={tasks}
          onTaskClick={() => {}}
          headerAction={<WeeklyReportButton tasks={tasks} projects={[PROJECT]} />}
        />
      </ToastProvider>
    </I18nProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  pickExportDirMock.mockReset();
});

describe("WeeklyReportButton", () => {
  it("按钮与标题同排,在面板内容里而不是面板外的一条 bar 上", async () => {
    installBackend("/tmp/reports");
    renderTimeline([task()]);

    const button = await screen.findByRole("button", { name: "Generate weekly report" });
    const title = screen.getByText("Timeline");
    const row = title.parentElement;
    expect(row).not.toBeNull();
    // 同一个标题行里既有标题也有按钮 —— 这是"用户会看的地方"的可断言形式。
    expect(within(row as HTMLElement).getByRole("button", { name: "Generate weekly report" })).toBe(
      button,
    );
  });

  it("区间内没有任务时不落盘", async () => {
    installBackend("/tmp/reports");
    // 去年的任务:既不在创建窗口内也不在完成窗口内。
    const stale = Date.now() - 400 * 24 * 60 * 60 * 1000;
    renderTimeline([task({ createdAt: stale, completedAt: stale })]);

    const button = await screen.findByRole("button", { name: "Generate weekly report" });
    await userEvent.click(button);

    // 用完整句:空状态文案也以 "No tasks" 开头,撞上会 Found multiple。
    await waitFor(() => expect(screen.getByText("No tasks in this week's range.")).toBeTruthy());
    expect(
      vi.mocked(invoke).mock.calls.filter(([name]) => name === "generate_weekly_report"),
    ).toEqual([]);
  });

  it("输出目录已配时不再弹目录选择框", async () => {
    installBackend("/tmp/reports");
    renderTimeline([task()]);

    const button = await screen.findByRole("button", { name: "Generate weekly report" });
    await userEvent.click(button);

    await waitFor(() =>
      expect(
        vi.mocked(invoke).mock.calls.filter(([name]) => name === "generate_weekly_report"),
      ).toHaveLength(1),
    );
    expect(pickExportDirMock).not.toHaveBeenCalled();
  });
});
