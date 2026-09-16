import type { ReactNode } from "react";
import { vi } from "vitest";
import { AppProviders } from "../state/app";
import type { TaskActions } from "../state/app";

export function noopTaskActions(): TaskActions {
  return {
    deleteTask: vi.fn(),
    deleteTasks: vi.fn(),
    archiveTasks: vi.fn(),
    unarchiveTasks: vi.fn(),
    deleteAllTasks: vi.fn(),
    toggleTaskStar: vi.fn(),
    renameTask: vi.fn(),
    generateTaskName: vi.fn(async () => {}),
    updateTodo: vi.fn(),
    cancelTask: vi.fn(),
    resumeTask: vi.fn(),
    runTodoTask: vi.fn(),
    mergeWorktree: vi.fn(async () => {}),
    discardWorktree: vi.fn(async () => {}),
    reconnectTask: vi.fn(),
    markTaskDone: vi.fn(),
  };
}

export function withTaskActions(children: ReactNode, actions?: TaskActions) {
  return <AppProviders taskActions={actions ?? noopTaskActions()}>{children}</AppProviders>;
}
