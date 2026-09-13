import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
// 被测对象是随 Rust 二进制 include_str! 打包进 omp 托管 home 的那份真脚本,
// 不是复刻件:改了它这里立刻能看出来。
import aeroricOmpHook from "../../src-tauri/src/aeroric-omp-hook.js";
import type { OmpExtensionContext } from "../../src-tauri/src/aeroric-omp-hook.js";

type Handler = (event: unknown, ctx: OmpExtensionContext | undefined) => Promise<void>;

/** omp 的 ExtensionAPI 只用到 pi.on;handler 第二参是 ExtensionContext。 */
function fakePi() {
  const handlers = new Map<string, Handler>();
  return {
    pi: { on: (name: string, fn: Handler) => void handlers.set(name, fn) },
    handlers,
    emit: async (name: string, event: unknown, ctx: OmpExtensionContext | undefined) => {
      const fn = handlers.get(name);
      if (!fn) throw new Error(`hook 没有订阅 ${name}`);
      await fn(event, ctx);
    },
  };
}

/** 交互式 TUI 会话:runner 的 createContext() 给 hasUI: true。 */
const MAIN = { hasUI: true };
/** 子代理会话:创建时写死 hasUI: false。 */
const SUBAGENT = { hasUI: false };

describe("omp hook session scoping", () => {
  let eventDir: string;
  const saved = {
    task: process.env.AERORIC_TASK_ID,
    dir: process.env.AERORIC_EVENT_DIR,
    agent: process.env.AERORIC_AGENT,
  };

  function written(): string[] {
    let raw: string;
    try {
      raw = readFileSync(join(eventDir, "events.jsonl"), "utf8");
    } catch {
      return [];
    }
    return raw
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line).event as string);
  }

  beforeEach(() => {
    eventDir = mkdtempSync(join(tmpdir(), "aeroric-omp-hook-"));
    process.env.AERORIC_TASK_ID = "task-1";
    process.env.AERORIC_EVENT_DIR = eventDir;
    process.env.AERORIC_AGENT = "sota_omp";
  });

  afterEach(() => {
    rmSync(eventDir, { recursive: true, force: true });
    for (const [key, value] of [
      ["AERORIC_TASK_ID", saved.task],
      ["AERORIC_EVENT_DIR", saved.dir],
      ["AERORIC_AGENT", saved.agent],
    ] as const) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });

  it("ignores subagent lifecycle events entirely", async () => {
    const { pi, emit } = fakePi();
    aeroricOmpHook(pi);

    await emit("agent_start", { type: "agent_start" }, SUBAGENT);
    await emit("agent_end", { type: "agent_end", messages: [] }, SUBAGENT);

    // 子代理终态写进主任务的 events.jsonl 就是黄叹号误报的根因。
    expect(written()).toEqual([]);
  });

  it("keeps the task running while parallel subagents settle", async () => {
    const { pi, emit } = fakePi();
    aeroricOmpHook(pi);

    // 复刻线上观察到的形状:主会话一条 prompt,3 个子代理并行起停。
    await emit("agent_start", { type: "agent_start" }, MAIN);
    for (let i = 0; i < 3; i += 1) {
      await emit("agent_start", { type: "agent_start" }, SUBAGENT);
    }
    for (let i = 0; i < 3; i += 1) {
      await emit("agent_end", { type: "agent_end", messages: [] }, SUBAGENT);
    }
    await emit("tool_execution_start", { type: "tool_execution_start" }, MAIN);
    await emit("agent_end", { type: "agent_end", messages: [] }, MAIN);

    // 主会话真正停下前不得出现 Stop,停下后必须有且只有一条。
    expect(written()).toEqual(["UserPromptSubmit", "PostToolUse", "Stop"]);
  });

  it("clears a stale stop when the main session resumes tool work", async () => {
    const { pi, emit } = fakePi();
    aeroricOmpHook(pi);

    await emit("agent_start", { type: "agent_start" }, MAIN);
    // 压缩的 deferredHandoff 分支会在主会话上发出 willContinue 未设的 agent_end,
    // hasUI 门挡不住它,只能靠工具重新跑起来复位。
    await emit("agent_end", { type: "agent_end", messages: [] }, MAIN);
    await emit("tool_execution_start", { type: "tool_execution_start" }, MAIN);

    expect(written()).toEqual(["UserPromptSubmit", "Stop", "PostToolUse"]);
  });

  it("writes one line per status change, not per event", async () => {
    const { pi, emit } = fakePi();
    aeroricOmpHook(pi);

    await emit("agent_start", { type: "agent_start" }, MAIN);
    for (let i = 0; i < 5; i += 1) {
      await emit("tool_execution_start", { type: "tool_execution_start" }, MAIN);
    }
    await emit("agent_end", { type: "agent_end", messages: [] }, MAIN);
    await emit("agent_end", { type: "agent_end", messages: [] }, MAIN);

    expect(written()).toEqual(["UserPromptSubmit", "PostToolUse", "Stop"]);
  });

  it("treats willContinue as an auto-continuation, not a settle", async () => {
    const { pi, emit } = fakePi();
    aeroricOmpHook(pi);

    await emit("agent_start", { type: "agent_start" }, MAIN);
    await emit("agent_end", { type: "agent_end", messages: [], willContinue: true }, MAIN);

    expect(written()).toEqual(["UserPromptSubmit"]);
  });

  it("registers nothing when the task env is absent", () => {
    delete process.env.AERORIC_TASK_ID;
    const { pi, handlers } = fakePi();
    aeroricOmpHook(pi);

    // 用户自己跑 omp(哪怕用同一个托管 home)必须零副作用。
    expect(handlers.size).toBe(0);
  });

  it("records the task id and agent that event_watcher keys on", async () => {
    const { pi, emit } = fakePi();
    aeroricOmpHook(pi);

    await emit("agent_start", { type: "agent_start" }, MAIN);

    const line = JSON.parse(readFileSync(join(eventDir, "events.jsonl"), "utf8").trim());
    expect(line).toMatchObject({ task_id: "task-1", agent: "sota_omp", codex_like: false });
  });
});
