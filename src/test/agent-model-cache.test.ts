import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearAgentModelCache,
  getCachedAgentModels,
  refreshAgentModels,
} from "../hooks/agentModelCache";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("agent model cache", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    clearAgentModelCache();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("persists non-sensitive model presets and reuses them synchronously", async () => {
    vi.mocked(invoke).mockResolvedValue({
      models: ["gpt-5.6"],
      reasoning_effort: "high",
      reasoning_speed: "fast",
      api_key: "must-not-be-persisted",
    });

    await refreshAgentModels("codex");

    expect(getCachedAgentModels("codex")).toEqual({
      models: ["gpt-5.6"],
      reasoning_effort: "high",
      reasoning_speed: "fast",
    });
    expect(localStorage.getItem("aeroric:agent-model-cache:v1")).not.toContain("api_key");
    expect(localStorage.getItem("aeroric:agent-model-cache:v1")).toContain("cachedAt");
  });

  it("deduplicates concurrent backend refreshes", async () => {
    let resolveRequest: (value: { models: string[] }) => void = () => {};
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRequest = resolve;
        }),
    );

    const first = refreshAgentModels("claude");
    const second = refreshAgentModels("claude");
    expect(invoke).toHaveBeenCalledTimes(1);

    resolveRequest({ models: ["sonnet"] });
    await expect(first).resolves.toEqual({
      models: ["sonnet"],
      reasoning_effort: null,
      reasoning_speed: null,
    });
    await expect(second).resolves.toEqual({
      models: ["sonnet"],
      reasoning_effort: null,
      reasoning_speed: null,
    });
  });

  it("does not restore stale results after invalidation", async () => {
    const resolvers: Array<(value: { models: string[] }) => void> = [];
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvers.push(resolve);
        }),
    );

    const staleRequest = refreshAgentModels("codex");
    clearAgentModelCache();
    const freshRequest = refreshAgentModels("codex");
    expect(invoke).toHaveBeenCalledTimes(2);

    resolvers[0]({ models: ["stale"] });
    await staleRequest;
    expect(getCachedAgentModels("codex")).toBeNull();

    resolvers[1]({ models: ["fresh"] });
    await freshRequest;
    expect(getCachedAgentModels("codex")?.models).toEqual(["fresh"]);
  });

  it("expires cached models after five minutes", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_000_000);
    vi.mocked(invoke).mockResolvedValue({ models: ["gpt-5.6"] });
    await refreshAgentModels("codex");
    expect(getCachedAgentModels("codex")?.models).toEqual(["gpt-5.6"]);

    now.mockReturnValue(1_000_000 + 5 * 60 * 1000);
    expect(getCachedAgentModels("codex")).toBeNull();
    expect(localStorage.getItem("aeroric:agent-model-cache:v1")).toBe("{}");
    now.mockRestore();
  });

  it("discards legacy entries without a timestamp during hydration", async () => {
    localStorage.setItem(
      "aeroric:agent-model-cache:v1",
      JSON.stringify({ codex: { models: ["legacy"] } }),
    );
    vi.resetModules();
    const freshCache = await import("../hooks/agentModelCache");

    expect(freshCache.getCachedAgentModels("codex")).toBeNull();
    expect(localStorage.getItem("aeroric:agent-model-cache:v1")).toBe("{}");
    freshCache.clearAgentModelCache();
  });

  it("removes corrupt persistent data", async () => {
    localStorage.setItem("aeroric:agent-model-cache:v1", "not-json");
    vi.resetModules();
    const freshCache = await import("../hooks/agentModelCache");

    expect(freshCache.getCachedAgentModels("codex")).toBeNull();
    expect(localStorage.getItem("aeroric:agent-model-cache:v1")).toBeNull();
    freshCache.clearAgentModelCache();
  });
});
