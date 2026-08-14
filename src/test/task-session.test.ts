import { describe, expect, it } from "vitest";
import {
  canAdoptSessionForAgent,
  canNativeResumeWithAgent,
  getTaskSessionFields,
  hasTaskContinuationContext,
  hasTaskSessionPath,
  resolveTaskSessionOwner,
} from "../taskSession";
import { getTaskSessionFieldsByFamily } from "../taskSession";
import type { Task } from "../types";

const baseTask: Task = {
  id: "task-1",
  projectId: "project-1",
  prompt: "continue the task",
  agent: "claude",
  permissionMode: "ask",
  status: "failed",
  createdAt: 1,
};

describe("task session ownership", () => {
  it("keeps the original session owner after the visible task agent changes", () => {
    const task: Task = {
      ...baseTask,
      agent: "codex-team",
      claudeSessionId: "claude-session",
      claudeSessionPath: "/tmp/claude-session.jsonl",
      sessionAgent: "claude-team",
      sessionCodexLike: false,
    };

    expect(resolveTaskSessionOwner(task)).toEqual({
      agent: "claude-team",
      codexLike: false,
      family: "claude",
    });
    expect(getTaskSessionFields(task, false).sessionId).toBe("claude-session");
  });

  it("infers the legacy session family from the only populated field set", () => {
    expect(
      resolveTaskSessionOwner({
        ...baseTask,
        agent: "claude",
        codexSessionPath: "/tmp/codex-session.jsonl",
      }).codexLike,
    ).toBe(true);
  });

  it("allows native resume only for the same family and session home", () => {
    const builtinCodexTask: Task = {
      ...baseTask,
      agent: "claude",
      codexSessionId: "codex-session",
      sessionAgent: "codex",
      sessionCodexLike: true,
    };
    expect(canNativeResumeWithAgent(builtinCodexTask, "claude_gpt55")).toBe(true);
    expect(canNativeResumeWithAgent(builtinCodexTask, "claude")).toBe(false);

    const customTask: Task = {
      ...baseTask,
      agent: "team-a",
      claudeSessionId: "claude-session",
      sessionAgent: "team-a",
      sessionCodexLike: false,
    };
    const options = [
      {
        value: "team-a",
        label: "Team A",
        configFile: "/tmp/team-a",
        configLang: "json" as const,
        codexLike: false,
        family: "claude" as const,
        custom: true,
      },
      {
        value: "team-b",
        label: "Team B",
        configFile: "/tmp/team-b",
        configLang: "json" as const,
        codexLike: false,
        family: "claude" as const,
        custom: true,
      },
    ];
    expect(canNativeResumeWithAgent(customTask, "team-a", options)).toBe(true);
    expect(canNativeResumeWithAgent(customTask, "team-b", options)).toBe(false);
  });

  it("recognizes prompt-only continuation context for interrupted tasks", () => {
    expect(hasTaskContinuationContext(baseTask)).toBe(true);
    expect(hasTaskContinuationContext({ ...baseTask, prompt: "" })).toBe(false);
  });

  it("adopts the transcript when two same-family configs use different homes", () => {
    const options = [
      {
        value: "codex-a",
        label: "Codex A",
        configFile: "/tmp/codex-a",
        configLang: "toml" as const,
        codexLike: true,
        family: "codex" as const,
        custom: true,
      },
      {
        value: "codex-b",
        label: "Codex B",
        configFile: "/tmp/codex-b",
        configLang: "toml" as const,
        codexLike: true,
        family: "codex" as const,
        custom: true,
      },
      {
        value: "claude-a",
        label: "Claude A",
        configFile: "/tmp/claude-a",
        configLang: "json" as const,
        codexLike: false,
        family: "claude" as const,
        custom: true,
      },
    ];
    const codexTask: Task = {
      ...baseTask,
      agent: "codex-a",
      codexSessionId: "codex-session",
      codexSessionPath: "/tmp/rollout.jsonl",
      sessionAgent: "codex-a",
      sessionCodexLike: true,
    };

    // Different home, same CLI family: adopt the file so native resume works.
    expect(canAdoptSessionForAgent(codexTask, "codex-b", options)).toBe(true);
    // Same config resumes natively already, nothing to adopt.
    expect(canAdoptSessionForAgent(codexTask, "codex-a", options)).toBe(false);
    // Cross-family switches have incompatible transcript formats.
    expect(canAdoptSessionForAgent(codexTask, "claude-a", options)).toBe(false);
  });

  it("does not adopt between built-in agents that already share a home", () => {
    const builtinCodexTask: Task = {
      ...baseTask,
      agent: "codex",
      codexSessionId: "codex-session",
      sessionAgent: "codex",
      sessionCodexLike: true,
    };

    expect(canAdoptSessionForAgent(builtinCodexTask, "claude_gpt55")).toBe(false);
  });
});

describe("dsh session family", () => {
  const dshTask: Task = {
    ...baseTask,
    agent: "dsh",
    dshSessionId: "dsh-session",
    dshSessionPath: "/tmp/dsh/session.jsonl",
    sessionAgent: "dsh",
    sessionFamily: "dsh",
  };

  it("resolves dsh owners from sessionFamily", () => {
    expect(resolveTaskSessionOwner(dshTask)).toEqual({
      agent: "dsh",
      family: "dsh",
      codexLike: false,
    });
  });

  it("selects dsh session fields by family", () => {
    const fields = getTaskSessionFieldsByFamily(dshTask, "dsh");
    expect(fields.sessionId).toBe("dsh-session");
    expect(fields.sessionPath).toBe("/tmp/dsh/session.jsonl");
  });

  it("never offers native resume or adoption for dsh sessions", () => {
    expect(canNativeResumeWithAgent(dshTask, "dsh")).toBe(false);
    expect(canNativeResumeWithAgent(dshTask, "claude")).toBe(false);
    expect(canAdoptSessionForAgent(dshTask, "dsh")).toBe(false);
    expect(canAdoptSessionForAgent(dshTask, "codex")).toBe(false);
  });

  it("infers dsh family for legacy tasks with only dsh session fields", () => {
    const legacy: Task = {
      ...baseTask,
      agent: "dsh",
      dshSessionId: "dsh-session",
    };
    expect(resolveTaskSessionOwner(legacy).family).toBe("dsh");
  });

  it("counts dsh session fields as continuation context", () => {
    expect(hasTaskContinuationContext({ ...baseTask, prompt: "", dshSessionId: "x" })).toBe(true);
  });

  it("keeps cancelled dsh tasks visible when a transcript path exists", () => {
    expect(hasTaskSessionPath(dshTask)).toBe(true);
    expect(hasTaskSessionPath({ ...dshTask, dshSessionPath: undefined })).toBe(false);
  });
});
