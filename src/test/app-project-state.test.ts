import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  COLLAPSED_PROJECT_GROUPS_KEY,
  PROJECT_RAIL_WIDTH_STORAGE_KEY,
  createDefaultProjectViewState,
  deriveProjectName,
  isLiveTerminalTaskStatus,
  loadCollapsedProjectGroups,
  loadProjectRailWidth,
  normalizeInterruptedTasksOnStartup,
  normalizeRemotePath,
  normalizeSshProjectNames,
  saveCollapsedProjectGroups,
  shouldIgnoreTaskStatusTransition,
} from "../appProjectState";
import type { Project, SshConnection, Task, TaskStatus } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const ALL_STATUSES: TaskStatus[] = [
  "todo",
  "pending",
  "running",
  "input_required",
  "detached",
  "interrupted",
  "done",
  "failed",
  "cancelled",
];

function task(overrides: Partial<Task> & { id: string; status: TaskStatus }): Task {
  return {
    projectId: "p1",
    prompt: "run it",
    agent: "claude",
    permissionMode: "default",
    createdAt: 1000,
    ...overrides,
  } as Task;
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("loadProjectRailWidth", () => {
  it("returns null when nothing is stored", () => {
    expect(loadProjectRailWidth()).toBeNull();
  });

  it("returns null for non-positive or unparsable values", () => {
    for (const stored of ["0", "-40", "not-a-number"]) {
      localStorage.setItem(PROJECT_RAIL_WIDTH_STORAGE_KEY, stored);
      expect(loadProjectRailWidth()).toBeNull();
    }
  });

  it("clamps a stored width up to the rail minimum", () => {
    localStorage.setItem(PROJECT_RAIL_WIDTH_STORAGE_KEY, "80");
    expect(loadProjectRailWidth()).toBe(220);
  });

  it("keeps a stored width above the minimum and rounds it", () => {
    localStorage.setItem(PROJECT_RAIL_WIDTH_STORAGE_KEY, "301.6");
    expect(loadProjectRailWidth()).toBe(302);
  });
});

describe("collapsed project groups", () => {
  it("round-trips through localStorage", () => {
    saveCollapsedProjectGroups(new Set(["work", "personal"]));
    expect(loadCollapsedProjectGroups()).toEqual(new Set(["work", "personal"]));
  });

  it("returns an empty set when nothing is stored", () => {
    expect(loadCollapsedProjectGroups()).toEqual(new Set());
  });

  it("recovers from malformed or non-array payloads", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

    localStorage.setItem(COLLAPSED_PROJECT_GROUPS_KEY, "{not json");
    expect(loadCollapsedProjectGroups()).toEqual(new Set());
    expect(consoleError).toHaveBeenCalled();

    localStorage.setItem(COLLAPSED_PROJECT_GROUPS_KEY, JSON.stringify({ work: true }));
    expect(loadCollapsedProjectGroups()).toEqual(new Set());
  });
});

describe("deriveProjectName", () => {
  it("takes the last path segment for both separators", () => {
    expect(deriveProjectName("/Users/dev/projects/aeroric")).toBe("aeroric");
    expect(deriveProjectName("C:\\Users\\dev\\aeroric")).toBe("aeroric");
  });

  it("ignores trailing separators", () => {
    expect(deriveProjectName("/Users/dev/aeroric//")).toBe("aeroric");
    expect(deriveProjectName("C:\\Users\\dev\\aeroric\\")).toBe("aeroric");
  });

  it("falls back to the original path when there is no segment", () => {
    expect(deriveProjectName("/")).toBe("/");
    expect(deriveProjectName("")).toBe("");
  });
});

describe("normalizeRemotePath", () => {
  it("makes the path absolute and trims surrounding whitespace", () => {
    expect(normalizeRemotePath("  srv/app  ")).toBe("/srv/app");
    expect(normalizeRemotePath("/srv/app")).toBe("/srv/app");
  });
});

describe("normalizeSshProjectNames", () => {
  const connections: SshConnection[] = [
    {
      id: "c1",
      name: "prod-box",
      host: "10.0.0.1",
      port: 22,
      username: "dev",
      createdAt: 1,
    },
  ];

  function sshProject(overrides: Partial<Project> = {}): Project {
    return {
      id: "p1",
      name: "app",
      path: "ssh://c1/srv/app",
      location: { kind: "ssh", connectionId: "c1", remotePath: "/srv/app" },
      lastOpenedAt: 1,
      ...overrides,
    };
  }

  it("renames projects still carrying the legacy derived name", () => {
    const projects = [sshProject()];
    const next = normalizeSshProjectNames(projects, connections);
    expect(next).not.toBe(projects);
    expect(next[0].name).toBe("prod-box");
  });

  it("keeps the same array identity when nothing changes", () => {
    const projects = [sshProject({ name: "prod-box" })];
    expect(normalizeSshProjectNames(projects, connections)).toBe(projects);
  });

  it("leaves user-chosen names alone", () => {
    const projects = [sshProject({ name: "My Server" })];
    expect(normalizeSshProjectNames(projects, connections)).toBe(projects);
  });

  it("leaves local projects and unknown connections alone", () => {
    const local: Project = { id: "p2", name: "local", path: "/tmp/local", lastOpenedAt: 1 };
    const orphan = sshProject({
      id: "p3",
      location: { kind: "ssh", connectionId: "missing", remotePath: "/srv/app" },
    });
    const projects = [local, orphan];
    expect(normalizeSshProjectNames(projects, connections)).toBe(projects);
  });
});

describe("createDefaultProjectViewState", () => {
  it("starts on the new-task form with nothing selected", () => {
    expect(createDefaultProjectViewState()).toEqual({ selectedTaskId: null, isNewTask: true });
  });
});

describe("normalizeInterruptedTasksOnStartup", () => {
  it("leaves finished and todo tasks untouched", () => {
    const tasks = ALL_STATUSES.filter(
      (status) => !["pending", "running", "input_required", "detached"].includes(status),
    ).map((status) => task({ id: status, status }));
    const result = normalizeInterruptedTasksOnStartup(tasks, new Set());
    expect(result.tasks).toEqual(tasks);
    expect(result.changedProjectIds).toEqual(new Set());
  });

  it("marks active tasks without a live child as interrupted", () => {
    const tasks = [
      task({ id: "a", status: "running" }),
      task({ id: "b", status: "input_required", projectId: "p2" }),
      task({ id: "c", status: "pending" }),
      task({ id: "d", status: "detached" }),
    ];
    const result = normalizeInterruptedTasksOnStartup(tasks, new Set());
    expect(result.tasks.map((item) => item.status)).toEqual([
      "interrupted",
      "interrupted",
      "interrupted",
      "interrupted",
    ]);
    expect(result.changedProjectIds).toEqual(new Set(["p1", "p2"]));
    for (const item of result.tasks) {
      expect(item.attentionRequestedAt).toBeGreaterThan(0);
    }
  });

  it("marks active tasks with a live child as detached instead", () => {
    const tasks = [task({ id: "a", status: "running" })];
    const result = normalizeInterruptedTasksOnStartup(tasks, new Set(["a"]));
    expect(result.tasks[0].status).toBe("detached");
    expect(result.changedProjectIds).toEqual(new Set(["p1"]));
  });

  it("re-attaches a task persisted as interrupted whose child survived", () => {
    const tasks = [task({ id: "a", status: "interrupted" })];
    const result = normalizeInterruptedTasksOnStartup(tasks, new Set(["a"]));
    expect(result.tasks[0].status).toBe("detached");
    expect(result.changedProjectIds).toEqual(new Set(["p1"]));
  });

  it("reports no change when the persisted status already matches", () => {
    const detached = [task({ id: "a", status: "detached" })];
    const detachedResult = normalizeInterruptedTasksOnStartup(detached, new Set(["a"]));
    expect(detachedResult.tasks[0]).toBe(detached[0]);
    expect(detachedResult.changedProjectIds).toEqual(new Set());

    const interrupted = [task({ id: "b", status: "interrupted" })];
    const interruptedResult = normalizeInterruptedTasksOnStartup(interrupted, new Set());
    expect(interruptedResult.tasks[0]).toBe(interrupted[0]);
    expect(interruptedResult.changedProjectIds).toEqual(new Set());
  });

  it("preserves an existing attention timestamp", () => {
    const tasks = [task({ id: "a", status: "running", attentionRequestedAt: 42 })];
    const result = normalizeInterruptedTasksOnStartup(tasks, new Set());
    expect(result.tasks[0].attentionRequestedAt).toBe(42);
  });
});

describe("shouldIgnoreTaskStatusTransition", () => {
  it("ignores late live updates for a detached task", () => {
    expect(shouldIgnoreTaskStatusTransition("detached", "running")).toBe(true);
    expect(shouldIgnoreTaskStatusTransition("detached", "input_required")).toBe(true);
  });

  it("still accepts terminal updates for a detached task", () => {
    for (const next of ["done", "failed", "cancelled", "interrupted"] as TaskStatus[]) {
      expect(shouldIgnoreTaskStatusTransition("detached", next)).toBe(false);
    }
  });

  it("does not interfere with any other current status", () => {
    for (const current of ALL_STATUSES.filter((status) => status !== "detached")) {
      expect(shouldIgnoreTaskStatusTransition(current, "running")).toBe(false);
      expect(shouldIgnoreTaskStatusTransition(current, "input_required")).toBe(false);
    }
  });
});

describe("isLiveTerminalTaskStatus", () => {
  it("keeps the terminal mounted while the agent may still write", () => {
    expect(isLiveTerminalTaskStatus("pending")).toBe(true);
    expect(isLiveTerminalTaskStatus("running")).toBe(true);
    // 等待用户输入时进程仍在跑，终端不能卸载。
    expect(isLiveTerminalTaskStatus("input_required")).toBe(true);
  });

  it("does not keep it mounted for detached or finished tasks", () => {
    for (const status of ALL_STATUSES.filter(
      (item) => !["pending", "running", "input_required"].includes(item),
    )) {
      expect(isLiveTerminalTaskStatus(status)).toBe(false);
    }
  });
});
