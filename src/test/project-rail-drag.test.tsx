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
  it("reports a new project order after a long-press pointer drag over another row", () => {
    vi.useFakeTimers();
    const onReorderProjects = vi.fn();
    try {
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
      const alphaRow = alpha.closest("div") as HTMLDivElement;
      const betaRow = beta.closest("div") as HTMLDivElement;
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
      vi.advanceTimersByTime(200);
      fireEvent.pointerMove(alphaRow, { pointerId: 1, clientY: 58 });
      fireEvent.pointerUp(alphaRow, { pointerId: 1, clientY: 58 });

      expect(onReorderProjects).toHaveBeenCalledWith(["p2", "p1"]);
    } finally {
      vi.useRealTimers();
    }
  });
});
