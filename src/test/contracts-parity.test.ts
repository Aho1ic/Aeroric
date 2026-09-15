import { describe, expect, it } from "vitest";
import {
  type AgentType,
  type PermissionMode,
  type ProjectLocation,
  type ProtocolFamily,
  type TaskStatus,
  type BuiltInAgentType,
} from "@aeroric/remote-contracts";
import * as desktop from "../types";

/**
 * 桌面 types 必须从 contracts re-export 跨端词表，而不是再抄一份字面量。
 * 这里锁的是「同一类型身份」：值级断言无意义，用赋值兼容 + 关键字面量成员存在性。
 */
describe("remote-contracts shared vocabulary", () => {
  it("re-exports agent/task/location unions from desktop types", () => {
    const family: ProtocolFamily = "omp";
    const mode: PermissionMode = "ask";
    const status: TaskStatus = "running";
    const builtin: BuiltInAgentType = "claude_gpt55";
    const agent: AgentType = builtin;
    const location: ProjectLocation = { kind: "local", path: "/x" };

    // 桌面侧同名导出必须接受 contracts 的值（同一类型）。
    const desktopFamily: desktop.ProtocolFamily = family;
    const desktopMode: desktop.PermissionMode = mode;
    const desktopStatus: desktop.TaskStatus = status;
    const desktopAgent: desktop.AgentType = agent;
    const desktopLocation: desktop.ProjectLocation = location;

    expect(desktopFamily).toBe("omp");
    expect(desktopMode).toBe("ask");
    expect(desktopStatus).toBe("running");
    expect(desktopAgent).toBe("claude_gpt55");
    expect(desktopLocation.kind).toBe("local");
  });

  it("TaskStatus covers the lifecycle set used by boot normalization", () => {
    const statuses: TaskStatus[] = [
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
    expect(new Set(statuses).size).toBe(9);
  });
});
