import { beforeEach, describe, expect, it, vi } from "vitest";
import { cancelTaskInvoke, launchLocalTask, type TaskLaunchDeps } from "../state/app/taskLaunch";
import {
  archiveTasksInList,
  renameTaskInList,
  toggleTaskStarInList,
  unarchiveTasksInList,
} from "../state/app/taskMutations";
import type { Task } from "../types";

vi.mock("../lib/api/invoke", () => ({
  invoke: vi.fn(async () => undefined),
}));

const task = (id: string, patch: Partial<Task> = {}): Task => ({
  id,
  projectId: "p1",
  prompt: "hello",
  agent: "claude",
  permissionMode: "ask",
  status: "done",
  createdAt: 1,
  ...patch,
});

describe("cancelTaskInvoke", () => {
  beforeEach(async () => {
    const { invoke } = await import("../lib/api/invoke");
    vi.mocked(invoke).mockClear();
  });

  it("uses cancel_task for local", async () => {
    const { invoke } = await import("../lib/api/invoke");
    await cancelTaskInvoke("t1", { kind: "local", projectPath: "/repo" });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("cancel_task", {
      taskId: "t1",
      projectPath: "/repo",
    });
  });

  it("uses cancel_remote_task for ssh", async () => {
    const { invoke } = await import("../lib/api/invoke");
    await cancelTaskInvoke("t1", { kind: "ssh" });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("cancel_remote_task", { taskId: "t1" });
  });
});

describe("launchLocalTask", () => {
  it("routes DSH family to run_dsh_task", async () => {
    const { invoke } = await import("../lib/api/invoke");
    vi.mocked(invoke).mockClear();
    const deps: TaskLaunchDeps = {
      createOutputChannel: () => ({}),
      writeErrorToTerminal: vi.fn(),
      terminalSize: { cols: 80, rows: 24 },
      onFailed: vi.fn(),
    };
    launchLocalTask(deps, {
      task: task("t1", { agent: "dsh" }),
      projectPath: "/repo",
      images: [],
      isDsh: true,
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
      "run_dsh_task",
      expect.objectContaining({ taskId: "t1", projectPath: "/repo" }),
    );
  });
});

describe("taskMutations", () => {
  it("archives archivable tasks only", () => {
    const list = [
      task("a", { status: "done", archivedAt: undefined }),
      task("b", { status: "running" }),
    ];
    const next = archiveTasksInList(list, ["a", "b"], 42);
    expect(next.find((t) => t.id === "a")?.archivedAt).toBe(42);
    expect(next.find((t) => t.id === "b")?.archivedAt).toBeUndefined();
  });

  it("unarchives and toggles star and renames", () => {
    const list = [task("a", { archivedAt: 1, starred: false, name: "old" })];
    let next = unarchiveTasksInList(list, ["a"]);
    expect(next[0].archivedAt).toBeUndefined();
    next = toggleTaskStarInList(next, "a");
    expect(next[0].starred).toBe(true);
    next = renameTaskInList(next, "a", "new");
    expect(next[0].name).toBe("new");
  });
});
