import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { AgentConfigSwitchDialog } from "../components/AgentConfigSwitchDialog";
import type { Task } from "../types";

const { refreshAgentModels } = vi.hoisted(() => ({
  refreshAgentModels: vi.fn((agent: string) =>
    Promise.resolve({
      models: agent === "claude" ? ["claude-sonnet"] : ["gpt-5.6"],
      reasoning_effort: "high",
      reasoning_speed: "standard",
    }),
  ),
}));

vi.mock("../hooks/useAgentOptions", () => ({
  useAgentOptions: () => [
    {
      value: "claude",
      label: "Claude Code",
      configFile: "/tmp/claude.json",
      configLang: "json",
      codexLike: false,
    },
    {
      value: "codex",
      label: "Codex",
      configFile: "/tmp/codex.toml",
      configLang: "toml",
      codexLike: true,
    },
    {
      value: "claude-team",
      label: "Claude Team",
      configFile: "/tmp/claude-team.json",
      configLang: "json",
      codexLike: false,
      custom: true,
    },
    {
      value: "codex-team",
      label: "Codex Team",
      configFile: "/tmp/codex-team.toml",
      configLang: "toml",
      codexLike: true,
      custom: true,
    },
  ],
}));

vi.mock("../hooks/agentModelCache", () => ({
  getCachedAgentModels: () => null,
  refreshAgentModels,
}));

const task: Task = {
  id: "task-1",
  projectId: "project-1",
  prompt: "finish the implementation",
  agent: "codex",
  selectedModel: "gpt-5.6",
  reasoningEffort: "medium",
  speed: "standard",
  permissionMode: "ask",
  status: "running",
  createdAt: 1,
};

describe("AgentConfigSwitchDialog", () => {
  it("loads the target configuration and submits all continuation settings", async () => {
    localStorage.setItem("aeroric:language", "en");
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <I18nProvider>
        <AgentConfigSwitchDialog task={task} open onClose={vi.fn()} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await waitFor(() =>
      expect(screen.getByRole("option", { name: "gpt-5.6" })).toBeInTheDocument(),
    );
    const claudeGroup = screen.getByRole("group", { name: "Claude configurations" });
    const codexGroup = screen.getByRole("group", { name: "Codex configurations" });
    expect(within(claudeGroup).getByRole("button", { name: "Claude Team" })).toBeInTheDocument();
    expect(within(codexGroup).getByRole("button", { name: "Codex Team" })).toBeInTheDocument();
    fireEvent.click(within(claudeGroup).getByRole("button", { name: "Claude Code" }));

    await waitFor(() =>
      expect(screen.getByRole("option", { name: "claude-sonnet" })).toBeInTheDocument(),
    );
    fireEvent.change(screen.getByRole("combobox", { name: "Model" }), {
      target: { value: "claude-sonnet" },
    });
    fireEvent.click(screen.getByRole("button", { name: "High" }));
    fireEvent.click(screen.getByRole("button", { name: "Fast" }));
    fireEvent.click(screen.getByRole("button", { name: "Full Access" }));
    fireEvent.click(screen.getByRole("button", { name: "Switch and continue" }));

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        agent: "claude",
        selectedModel: "claude-sonnet",
        reasoningEffort: "high",
        speed: "fast",
        permissionMode: "full_access",
      }),
    );
    expect(refreshAgentModels).toHaveBeenCalledWith("codex");
    expect(refreshAgentModels).toHaveBeenCalledWith("claude");
  });

  it("shows localized Chinese permission choices", async () => {
    localStorage.setItem("aeroric:language", "zh");

    render(
      <I18nProvider>
        <AgentConfigSwitchDialog task={task} open onClose={vi.fn()} onSubmit={vi.fn()} />
      </I18nProvider>,
    );

    expect(await screen.findByRole("button", { name: "请求确认" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "替我审批" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "完全访问" })).toBeInTheDocument();
  });
});
