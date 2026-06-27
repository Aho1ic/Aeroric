import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project, Task } from "../types";
import { ProjectRail } from "../components/ProjectRail";

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
  it("reports a new project order when an expanded project row is dragged over another row", () => {
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

    fireEvent.dragStart(alpha);
    fireEvent.dragOver(beta);
    fireEvent.drop(beta);

    expect(onReorderProjects).toHaveBeenCalledWith(["p2", "p1"]);
  });
});
