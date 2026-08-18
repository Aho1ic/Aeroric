import { describe, expect, it } from "vitest";
import { AGENT_OPTIONS } from "../agents";
import { taskCompletionCommand } from "../taskCompletion";
import type { AgentType, Task, TaskStatus } from "../types";

function task(agent: AgentType, status: TaskStatus): Task {
  return {
    id: `task-${agent}`,
    projectId: "project-1",
    prompt: "test",
    agent,
    permissionMode: "ask",
    status,
    createdAt: 1,
  };
}

describe("task completion routing", () => {
  it("uses the DSH lifecycle command for DSH tasks in any visible state", () => {
    expect(taskCompletionCommand(task("dsh", "running"), AGENT_OPTIONS)).toBe("complete_dsh_task");
    expect(taskCompletionCommand(task("dsh", "cancelled"), AGENT_OPTIONS)).toBe(
      "complete_dsh_task",
    );
  });

  it("keeps live Claude and Codex tasks on the PTY completion command", () => {
    expect(taskCompletionCommand(task("claude", "pending"), AGENT_OPTIONS)).toBe("complete_task");
    expect(taskCompletionCommand(task("codex", "input_required"), AGENT_OPTIONS)).toBe(
      "complete_task",
    );
  });

  it("needs no backend command for an already stopped non-DSH task", () => {
    expect(taskCompletionCommand(task("claude", "cancelled"), AGENT_OPTIONS)).toBeNull();
  });
});
