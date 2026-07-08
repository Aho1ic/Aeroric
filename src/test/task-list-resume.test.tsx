import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TaskList } from "../components/task-panel/TaskList";
import { I18nProvider } from "../i18n";
import type { Task } from "../types";

function task(overrides: Partial<Task>): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    prompt: "继续已经完成的任务",
    agent: "codex",
    permissionMode: "ask",
    status: "done",
    createdAt: Date.now(),
    codexSessionId: "019f3f6e-9cf2",
    ...overrides,
  };
}

describe("TaskList resume actions", () => {
  it("continues completed tasks with a saved session instead of running todo", async () => {
    const user = userEvent.setup();
    const onResumeTask = vi.fn();
    const onRunTodo = vi.fn();

    render(
      <I18nProvider>
        <TaskList
          tasks={[task({})]}
          taskDisplayWindow="all"
          query=""
          selectedId={null}
          isNewTask={false}
          onSelectTask={vi.fn()}
          onDeleteTask={vi.fn()}
          onToggleTaskStar={vi.fn()}
          onRunTodo={onRunTodo}
          onResumeTask={onResumeTask}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Continue" }));

    expect(onResumeTask).toHaveBeenCalledWith("task-1");
    expect(onRunTodo).not.toHaveBeenCalled();
  });
});
