import { beforeEach, describe, expect, it, vi } from "vitest";
import { useProjectsStore } from "../state/app/projectsStore";
import { useTasksStore } from "../state/app/tasksStore";
import type { Project, Task } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

const project = (id: string, name: string): Project => ({
  id,
  name,
  path: `/p/${id}`,
  lastOpenedAt: 1,
});

const task = (id: string, projectId: string): Task => ({
  id,
  projectId,
  prompt: "hello",
  agent: "claude",
  permissionMode: "ask",
  status: "todo",
  createdAt: 1,
});

beforeEach(() => {
  useProjectsStore.setState({
    projects: [],
    selectedProjectId: null,
    collapsedGroups: new Set(),
    railWidth: null,
  });
  useTasksStore.setState({ tasksByProject: {} });
});

describe("projectsStore", () => {
  it("adds a project and selects it", () => {
    useProjectsStore.getState().addProject(project("a", "Alpha"));
    const state = useProjectsStore.getState();
    expect(state.projects.map((p) => p.id)).toEqual(["a"]);
    expect(state.selectedProjectId).toBe("a");
  });

  it("renames and removes with selection fallback", () => {
    useProjectsStore.getState().addProject(project("a", "Alpha"));
    useProjectsStore.getState().addProject(project("b", "Beta"));
    useProjectsStore.getState().renameProject("a", "Alpha2");
    expect(useProjectsStore.getState().projects.find((p) => p.id === "a")?.name).toBe("Alpha2");
    useProjectsStore.getState().removeProject("b");
    expect(useProjectsStore.getState().projects.map((p) => p.id)).toEqual(["a"]);
    expect(useProjectsStore.getState().selectedProjectId).toBe("a");
  });
});

describe("tasksStore", () => {
  it("upserts and patches tasks per project", () => {
    useTasksStore.getState().upsertTask("p1", task("t1", "p1"));
    useTasksStore.getState().updateTask("p1", "t1", { status: "running" });
    const list = useTasksStore.getState().tasksByProject.p1;
    expect(list).toHaveLength(1);
    expect(list[0].status).toBe("running");
    useTasksStore.getState().removeTask("p1", "t1");
    expect(useTasksStore.getState().tasksByProject.p1).toHaveLength(0);
  });
});
