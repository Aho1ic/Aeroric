import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project, Task } from "../types";
import { ProjectRail } from "../components/ProjectRail";

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
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

describe("ProjectRail project dragging", () => {
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
      detail: { initialNav: "codex" },
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
});
