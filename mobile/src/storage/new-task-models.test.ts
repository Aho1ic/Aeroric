import { describe, expect, it, vi } from "vitest";

vi.mock("expo-secure-store", () => ({
  getItemAsync: vi.fn(),
  setItemAsync: vi.fn(),
}));

import * as SecureStore from "expo-secure-store";
import {
  loadLastModels,
  normalizeLastModels,
  rememberLastModel,
  saveLastModels,
} from "./new-task-models";

describe("new task model memory", () => {
  it("keeps only valid model selections", () => {
    expect(
      normalizeLastModels({
        claude: "claude-sonnet",
        codex: "  gpt-5.6  ",
        emptyAgent: "",
        invalid: 42,
      }),
    ).toEqual({ claude: "claude-sonnet", codex: "gpt-5.6" });
  });

  it("moves a reused agent to the newest position", () => {
    const remembered = rememberLastModel({ claude: "sonnet", codex: "gpt-5.6" }, "claude", "opus");

    expect(remembered).toEqual({ codex: "gpt-5.6", claude: "opus" });
  });

  it("does not write an invalid selection", () => {
    const existing = { claude: "sonnet" };

    expect(rememberLastModel(existing, "", "opus")).toBe(existing);
    expect(rememberLastModel(existing, "claude", "")).toBe(existing);
  });

  it("loads sanitized data and saves only sanitized data", async () => {
    vi.mocked(SecureStore.getItemAsync).mockResolvedValue(
      JSON.stringify({ claude: "  claude-sonnet  ", broken: 1 }),
    );

    await expect(loadLastModels()).resolves.toEqual({ claude: "claude-sonnet" });

    await saveLastModels({ codex: "gpt-5.6", broken: "" });
    expect(SecureStore.setItemAsync).toHaveBeenCalledWith(
      "aeroric.new-task-models.v1",
      JSON.stringify({ codex: "gpt-5.6" }),
    );
  });
});
