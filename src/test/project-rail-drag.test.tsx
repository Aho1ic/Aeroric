import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project, Task } from "../types";
import { ProjectRail } from "../components/ProjectRail";

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
  UpdateBanner: () => null,
}));

function project(id: string, name: string, orderIndex: number): Project {
  return {
    id,
    name,
    path: `/tmp/${id}`,
    lastOpenedAt: orderIndex + 1,
    orderIndex,
  };
}

function task(id: string, projectId: string, createdAt: number): Task {
  return {
    id,
    projectId,
    prompt: `Task ${id}`,
    agent: "claude",
    permissionMode: "ask",
    status: "done",
    createdAt,
  };
}

describe("ProjectRail project dragging", () => {
  it("collapses and expands project groups independently from project task lists", () => {
    localStorage.setItem("aeroric:language", "en");
    render(
      <I18nProvider>
        <ProjectRail
          projects={[
            { ...project("p1", "Alpha", 0), group: "Work" },
            { ...project("p2", "Beta", 1), group: "Work" },
          ]}
          projectGroups={["Work"]}
          allTasks={[]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
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
          singleProjectMode
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Collapse project group Work" }));

    expect(screen.queryByRole("button", { name: "Alpha" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand project group Work" }));
    expect(screen.getByRole("button", { name: "Beta" })).toBeInTheDocument();
  });

  it("reports project rail width changes from the resize separator", () => {
    localStorage.setItem("aeroric:language", "en");
    const onProjectRailWidthChange = vi.fn();
    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0)]}
          allTasks={[]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
          onSwitch={vi.fn()}
          onOpen={vi.fn()}
          onBack={vi.fn()}
          onNewTask={vi.fn()}
          onSelectTask={vi.fn()}
          onDeleteTask={vi.fn()}
          onToggleTaskStar={vi.fn()}
          onRunTodo={vi.fn()}
          projectRailWidth={252}
          onProjectRailWidthChange={onProjectRailWidthChange}
          themeVariant="light"
          onToggleTheme={vi.fn()}
          singleProjectMode
        />
      </I18nProvider>,
    );

    const separator = screen.getByRole("separator", { name: "Resize project sidebar" });
    fireEvent.pointerDown(separator, { pointerId: 1, button: 0, clientX: 252 });
    fireEvent.pointerMove(separator, { pointerId: 1, clientX: 332 });
    fireEvent.pointerUp(separator, { pointerId: 1, clientX: 332 });

    expect(onProjectRailWidthChange).toHaveBeenLastCalledWith(332);
  });

  it("keeps three project task lists open and closes the oldest on the fourth", () => {
    const projects = [
      project("p1", "Alpha", 0),
      project("p2", "Beta", 1),
      project("p3", "Gamma", 2),
      project("p4", "Delta", 3),
    ];
    const tasks = [
      task("alpha-task", "p1", 1),
      task("beta-task", "p2", 2),
      task("gamma-task", "p3", 3),
      task("delta-task", "p4", 4),
    ];

    function Harness() {
      const [activeProjectId, setActiveProjectId] = useState("p1");
      return (
        <I18nProvider>
          <ProjectRail
            projects={projects}
            allTasks={tasks}
            activeProjectId={activeProjectId}
            selectedTaskId={null}
            isNewTask={false}
            onSwitch={(nextProject) => setActiveProjectId(nextProject.id)}
            onOpen={vi.fn()}
            onBack={vi.fn()}
            onNewTask={vi.fn()}
            onSelectTask={vi.fn()}
            onDeleteTask={vi.fn()}
            onToggleTaskStar={vi.fn()}
            onRunTodo={vi.fn()}
            themeVariant="light"
            onToggleTheme={vi.fn()}
          />
        </I18nProvider>
      );
    }

    render(<Harness />);

    fireEvent.click(screen.getByRole("button", { name: "Beta" }));
    fireEvent.click(screen.getByRole("button", { name: "Gamma" }));
    expect(screen.getByText("Task alpha-task")).toBeInTheDocument();
    expect(screen.getByText("Task beta-task")).toBeInTheDocument();
    expect(screen.getByText("Task gamma-task")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delta" }));
    expect(screen.queryByText("Task alpha-task")).not.toBeInTheDocument();
    expect(screen.getByText("Task beta-task")).toBeInTheDocument();
    expect(screen.getByText("Task gamma-task")).toBeInTheDocument();
    expect(screen.getByText("Task delta-task")).toBeInTheDocument();
  });

  it("opens the agent settings section from the project rail footer", () => {
    const listener = vi.fn();
    window.addEventListener("aeroric:open-app-settings", listener);

    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0)]}
          allTasks={[] as Task[]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
          onSwitch={vi.fn()}
          onOpen={vi.fn()}
          onBack={vi.fn()}
          onNewTask={vi.fn()}
          onSelectTask={vi.fn()}
          onDeleteTask={vi.fn()}
          onToggleTaskStar={vi.fn()}
          onRunTodo={vi.fn()}
          onReorderProjects={vi.fn()}
          themeVariant="light"
          onToggleTheme={vi.fn()}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Agent settings" }));

    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener.mock.calls[0][0]).toMatchObject({
      detail: { initialNav: "__all_agent_configs__" },
    });

    window.removeEventListener("aeroric:open-app-settings", listener);
  });

  it("does not reorder projects when dragging from the project name area", () => {
    const onReorderProjects = vi.fn();
    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0), project("p2", "Beta", 1)]}
          allTasks={[] as Task[]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
          onSwitch={vi.fn()}
          onOpen={vi.fn()}
          onBack={vi.fn()}
          onNewTask={vi.fn()}
          onSelectTask={vi.fn()}
          onDeleteTask={vi.fn()}
          onToggleTaskStar={vi.fn()}
          onRunTodo={vi.fn()}
          onReorderProjects={onReorderProjects}
          themeVariant="light"
          onToggleTheme={vi.fn()}
          singleProjectMode
        />
      </I18nProvider>,
    );

    const alpha = screen.getByRole("button", { name: "Alpha" });
    const beta = screen.getByRole("button", { name: "Beta" });
    const alphaRow = alpha.closest("[data-project-rail-row]") as HTMLDivElement;
    const betaRow = beta.closest("[data-project-rail-row]") as HTMLDivElement;
    alphaRow.setPointerCapture = vi.fn();
    alphaRow.releasePointerCapture = vi.fn();
    alphaRow.getBoundingClientRect = () =>
      ({
        top: 0,
        bottom: 38,
        height: 38,
        left: 0,
        right: 252,
        width: 252,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      }) as DOMRect;
    betaRow.getBoundingClientRect = () =>
      ({
        top: 44,
        bottom: 82,
        height: 38,
        left: 0,
        right: 252,
        width: 252,
        x: 0,
        y: 44,
        toJSON: () => ({}),
      }) as DOMRect;

    fireEvent.pointerDown(alphaRow, { pointerId: 1, button: 0, clientY: 12 });
    fireEvent.pointerMove(alphaRow, { pointerId: 1, clientY: 58 });
    fireEvent.pointerUp(alphaRow, { pointerId: 1, clientY: 58 });

    expect(onReorderProjects).not.toHaveBeenCalled();
  });

  it("reports a new project order after dragging from the project icon handle", () => {
    const onReorderProjects = vi.fn();
    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0), project("p2", "Beta", 1)]}
          allTasks={[] as Task[]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
          onSwitch={vi.fn()}
          onOpen={vi.fn()}
          onBack={vi.fn()}
          onNewTask={vi.fn()}
          onSelectTask={vi.fn()}
          onDeleteTask={vi.fn()}
          onToggleTaskStar={vi.fn()}
          onRunTodo={vi.fn()}
          onReorderProjects={onReorderProjects}
          themeVariant="light"
          onToggleTheme={vi.fn()}
          singleProjectMode
        />
      </I18nProvider>,
    );

    const alphaHandle = screen.getByRole("button", { name: "Drag project Alpha" });
    const beta = screen.getByRole("button", { name: "Beta" });
    const alphaRow = alphaHandle.closest("[data-project-rail-row]") as HTMLDivElement;
    const betaRow = beta.closest("[data-project-rail-row]") as HTMLDivElement;
    alphaHandle.setPointerCapture = vi.fn();
    alphaHandle.releasePointerCapture = vi.fn();
    alphaRow.getBoundingClientRect = () =>
      ({
        top: 0,
        bottom: 38,
        height: 38,
        left: 0,
        right: 252,
        width: 252,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      }) as DOMRect;
    betaRow.getBoundingClientRect = () =>
      ({
        top: 44,
        bottom: 82,
        height: 38,
        left: 0,
        right: 252,
        width: 252,
        x: 0,
        y: 44,
        toJSON: () => ({}),
      }) as DOMRect;

    fireEvent.pointerDown(alphaHandle, { pointerId: 1, button: 0, clientY: 12 });
    fireEvent.pointerMove(alphaHandle, { pointerId: 1, clientY: 58 });
    fireEvent.pointerUp(alphaHandle, { pointerId: 1, clientY: 58 });

    expect(onReorderProjects).toHaveBeenCalledWith(["p2", "p1"]);
  });

  it("uses localized text for the project drag handle", () => {
    localStorage.setItem("aeroric:language", "zh");
    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0)]}
          allTasks={[] as Task[]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
          onSwitch={vi.fn()}
          onOpen={vi.fn()}
          onBack={vi.fn()}
          onNewTask={vi.fn()}
          onSelectTask={vi.fn()}
          onDeleteTask={vi.fn()}
          onToggleTaskStar={vi.fn()}
          onRunTodo={vi.fn()}
          onReorderProjects={vi.fn()}
          themeVariant="light"
          onToggleTheme={vi.fn()}
          singleProjectMode
        />
      </I18nProvider>,
    );

    expect(screen.getByRole("button", { name: "拖动项目 Alpha" })).toBeInTheDocument();
  });

  it("opens the Agent terminal initial page instead of the latest task when clicking a project", () => {
    const beta = project("p2", "Beta", 1);
    const onSwitch = vi.fn();
    const onSelectTask = vi.fn();

    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0), beta]}
          allTasks={[task("old-beta-task", "p2", 100), task("new-beta-task", "p2", 200)]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={true}
          onSwitch={onSwitch}
          onOpen={vi.fn()}
          onBack={vi.fn()}
          onNewTask={vi.fn()}
          onSelectTask={onSelectTask}
          onDeleteTask={vi.fn()}
          onToggleTaskStar={vi.fn()}
          onRunTodo={vi.fn()}
          onReorderProjects={vi.fn()}
          themeVariant="light"
          onToggleTheme={vi.fn()}
          singleProjectMode
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Beta" }));

    expect(onSwitch).toHaveBeenCalledWith(beta);
    expect(onSelectTask).not.toHaveBeenCalled();
  });

  it("selects a task range with Shift and deletes it in one batch", () => {
    localStorage.setItem("aeroric:language", "en");
    const onDeleteTask = vi.fn();
    const onDeleteTasks = vi.fn();
    const onSelectTask = vi.fn();
    const tasks = [
      task("oldest", "p1", 100),
      task("third", "p1", 200),
      task("second", "p1", 300),
      task("newest", "p1", 400),
    ];

    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0)]}
          allTasks={tasks}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
          onSwitch={vi.fn()}
          onOpen={vi.fn()}
          onBack={vi.fn()}
          onNewTask={vi.fn()}
          onSelectTask={onSelectTask}
          onDeleteTask={onDeleteTask}
          onDeleteTasks={onDeleteTasks}
          onToggleTaskStar={vi.fn()}
          onRunTodo={vi.fn()}
          themeVariant="light"
          onToggleTheme={vi.fn()}
          singleProjectMode
        />
      </I18nProvider>,
    );

    const secondTask = screen.getByText("Task second").closest("button");
    const oldestTask = screen.getByText("Task oldest").closest("button");
    expect(secondTask).not.toBeNull();
    expect(oldestTask).not.toBeNull();

    fireEvent.click(secondTask!);
    fireEvent.click(oldestTask!, { shiftKey: true });

    expect(onSelectTask).toHaveBeenCalledTimes(1);
    expect(secondTask).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Task third").closest("button")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(oldestTask).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("3 selected")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));

    expect(onDeleteTasks).toHaveBeenCalledTimes(1);
    expect(onDeleteTasks).toHaveBeenCalledWith(["second", "third", "oldest"]);
    expect(onDeleteTask).not.toHaveBeenCalled();
  });

  it("expands the task list when clicking the already-active project", () => {
    localStorage.setItem("aeroric:language", "en");
    render(
      <I18nProvider>
        <ProjectRail
          projects={[project("p1", "Alpha", 0)]}
          allTasks={[task("only-task", "p1", 100)]}
          activeProjectId="p1"
          selectedTaskId={null}
          isNewTask={false}
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
          singleProjectMode
        />
      </I18nProvider>,
    );

    // "Hide tasks" 同时用于侧栏整体折叠按钮和项目行的箭头，这里只取项目行内的那个。
    const projectRow = screen.getByRole("button", { name: "Alpha" }).closest("div");
    expect(projectRow).not.toBeNull();

    // 先手动折叠，再点项目名，验证历史对话列表被重新展开。
    fireEvent.click(within(projectRow!).getByRole("button", { name: "Hide tasks" }));
    expect(screen.queryByText("Task only-task")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Alpha" }));

    expect(screen.getByText("Task only-task")).toBeInTheDocument();
  });

  it("restores the shared project list scroll position on a remounted rail", () => {
    localStorage.setItem("aeroric:language", "en");
    const renderRail = () =>
      render(
        <I18nProvider>
          <ProjectRail
            projects={[project("p1", "Alpha", 0), project("p2", "Beta", 1)]}
            allTasks={[] as Task[]}
            activeProjectId="p1"
            selectedTaskId={null}
            isNewTask={false}
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
            singleProjectMode
          />
        </I18nProvider>,
      );

    const findListContainer = (container: HTMLElement) => {
      const el = container.querySelector<HTMLElement>('[style*="overflow-y: auto"]');
      expect(el).not.toBeNull();
      return el!;
    };

    const first = renderRail();
    const firstList = findListContainer(first.container);
    // jsdom 没有布局，scrollTop 需要显式伪造后再派发 scroll 事件。
    Object.defineProperty(firstList, "scrollTop", { configurable: true, value: 140 });
    fireEvent.scroll(firstList);
    first.unmount();

    // 切换项目会换掉整条侧栏实例；新实例应恢复到共享的滚动位置而非跳回顶部。
    const second = renderRail();
    expect(findListContainer(second.container).scrollTop).toBe(140);

    // 复位共享状态，避免污染后续用例。
    const secondList = findListContainer(second.container);
    Object.defineProperty(secondList, "scrollTop", { configurable: true, value: 0 });
    fireEvent.scroll(secondList);
  });
});
