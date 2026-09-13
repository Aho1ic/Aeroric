/* 归档:多选工具条、分区渲染、以及侧栏状态指示对归档任务的态度。
 *
 * 为什么测 ProjectRail 而不是 TaskList:ProjectRail 是产品里真正渲染任务列表的组件
 * (TaskList 只被自己的测试引用)。归档若只在 TaskList 生效,用户一个字都看不见。
 */

import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project, Task, TaskStatus } from "../types";
import { ProjectRail } from "../components/ProjectRail";

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
  UpdateBanner: () => null,
}));

function project(id: string, name: string): Project {
  return { id, name, path: `/tmp/${id}`, lastOpenedAt: 1 };
}

function task(overrides: Partial<Task> & { id: string; status: TaskStatus }): Task {
  return {
    projectId: "p1",
    prompt: `Task ${overrides.id}`,
    agent: "claude",
    permissionMode: "ask",
    createdAt: 1000,
    ...overrides,
  } as Task;
}

function renderRail(tasks: Task[], handlers: Partial<Parameters<typeof ProjectRail>[0]> = {}) {
  localStorage.setItem("aeroric:language", "en");
  return render(
    <I18nProvider>
      <ProjectRail
        projects={[project("p1", "Alpha")]}
        allTasks={tasks}
        activeProjectId="p1"
        selectedTaskId={null}
        isNewTask={false}
        attentionBadge
        onSwitch={vi.fn()}
        onOpen={vi.fn()}
        onBack={vi.fn()}
        onNewTask={vi.fn()}
        onSelectTask={vi.fn()}
        onDeleteTask={vi.fn()}
        onToggleTaskStar={vi.fn()}
        onRunTodo={vi.fn()}
        themeVariant="light"
        onToggleTheme={vi.fn()}
        {...handlers}
      />
    </I18nProvider>,
  );
}

/** 用 cmd+click 把若干任务加进多选(与产品里的手势一致)。 */
function multiSelect(promptTexts: string[]) {
  for (const text of promptTexts) {
    fireEvent.click(screen.getByRole("button", { name: new RegExp(text) }), { metaKey: true });
  }
}

describe("ProjectRail 归档", () => {
  it("批量归档只发一次调用,且已排除活动中的任务", () => {
    // 活动中的任务归档后会从列表消失,而它仍在等用户反应 —— 这类任务必须被过滤掉,
    // 否则用户会以为"归档了 3 条",实际有一条还在跑却再也看不见。
    const onArchiveTasks = vi.fn();
    renderRail(
      [
        task({ id: "t-done", status: "done" }),
        task({ id: "t-failed", status: "failed" }),
        task({ id: "t-running", status: "running" }),
      ],
      { onArchiveTasks },
    );

    multiSelect(["Task t-done", "Task t-failed", "Task t-running"]);
    fireEvent.click(screen.getByRole("button", { name: "Archive selected" }));

    expect(onArchiveTasks).toHaveBeenCalledTimes(1);
    expect(onArchiveTasks).toHaveBeenCalledWith(["t-done", "t-failed"]);
  });

  it("全选都是活动任务时归档按钮不可点", () => {
    const onArchiveTasks = vi.fn();
    renderRail([task({ id: "t-running", status: "running" })], { onArchiveTasks });

    multiSelect(["Task t-running"]);
    expect(screen.getByRole("button", { name: "Archive selected" })).toBeDisabled();
  });

  /* issue #45:三条 input_required、角标显示 3,归档两条后角标仍是 3。
   *
   * 关键是别被"黄点正常"误导:3 条里归档 2 条还剩 1 条,黄点前后都是 attention,
   * 看起来一样。真正要钉的是那个数字。 */
  it("角标数字排除已归档的待确认任务", () => {
    const pending = (id: string, archivedAt?: number) =>
      task({ id, status: "input_required", ...(archivedAt ? { archivedAt } : {}) });

    const { unmount } = renderRail([pending("a"), pending("b"), pending("c")]);
    expect(screen.getByText("3")).toBeInTheDocument();
    unmount();

    // 归档两条后重渲染(等价于 setTasks 后的下一帧)。
    renderRail([pending("a"), pending("b", 2000), pending("c", 2000)]);
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.queryByText("3")).toBeNull();
  });

  /* issue #45 的另一半:`input_required` 必须**能**归档。
   *
   * 归档过滤原先用 `!isActiveTaskStatus(status)`,而它包含 `input_required` ——
   * 于是那三条根本archive不了,按钮是灰的。用户看到的"角标不变"正是这个:什么都没归档。
   * 归档与自动删除的语义在这里必须分开:自动删除跳过活动任务是怕删掉还在跑的东西;
   * 归档只是从主列表移走,`input_required` 恰恰是最该能收起来的一类(它会一直亮着)。
   * 真正不该归档的只有"正在产出"的那两个状态。 */
  it("待确认任务可以归档,运行中的不行", () => {
    const onArchiveTasks = vi.fn();
    renderRail(
      [
        task({ id: "t-attention", status: "input_required" }),
        task({ id: "t-detached", status: "detached" }),
        task({ id: "t-running", status: "running" }),
        task({ id: "t-pending", status: "pending" }),
      ],
      { onArchiveTasks },
    );

    multiSelect(["Task t-attention", "Task t-detached", "Task t-running", "Task t-pending"]);
    fireEvent.click(screen.getByRole("button", { name: "Archive selected" }));

    expect(onArchiveTasks).toHaveBeenCalledWith(["t-attention", "t-detached"]);
  });

  it("已归档任务落到独立分区,并给出取消归档而非收藏", () => {
    const onUnarchiveTasks = vi.fn();
    renderRail(
      [
        task({ id: "t-live", status: "done" }),
        task({ id: "t-old", status: "done", archivedAt: 5 }),
      ],
      { onUnarchiveTasks },
    );

    expect(screen.getByText("Archived")).toBeInTheDocument();

    const archived = screen.getByRole("button", { name: /Task t-old/ });
    expect(within(archived).getByRole("button", { name: "Unarchive" })).toBeInTheDocument();
    // 归档态占用星标位:两个按钮同时出现说明没做替换,行内会挤成三个动作。
    expect(within(archived).queryByRole("button", { name: "Star task" })).toBeNull();

    fireEvent.click(within(archived).getByRole("button", { name: "Unarchive" }));
    expect(onUnarchiveTasks).toHaveBeenCalledWith(["t-old"]);

    // 未归档的那条仍是收藏位,没被归档态污染。
    const live = screen.getByRole("button", { name: /Task t-live/ });
    expect(within(live).queryByRole("button", { name: "Unarchive" })).toBeNull();
  });

  it("待确认角标不把已归档任务算进去", () => {
    // 归档的语义是"我处理完了,从主列表移走"。角标还算它,项目会永远看着像有活。
    // 场景故意混合:一条待确认未归档 + 一条待确认已归档 —— 角标必须是 1 而不是 2。
    // 只放一条已归档的测不出来:那时项目状态本就不是 attention,角标根本不渲染。
    renderRail([
      task({ id: "t-live", status: "input_required", attentionRequestedAt: 2000 }),
      task({ id: "t-old", status: "input_required", attentionRequestedAt: 2000, archivedAt: 9 }),
    ]);

    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.queryByText("2")).toBeNull();
  });

  it("项目里只剩已归档的待确认任务时侧栏不再报警", () => {
    // 覆盖 getProjectStatus 一侧:它若不过滤归档,这里会亮出黄色警示点。
    const { container } = renderRail([
      task({ id: "t-old", status: "input_required", attentionRequestedAt: 2000, archivedAt: 9 }),
    ]);

    const warningDots = [...container.querySelectorAll("span")].filter((el) =>
      el.getAttribute("style")?.includes("var(--color-warning)"),
    );
    expect(warningDots).toHaveLength(0);
  });
});
