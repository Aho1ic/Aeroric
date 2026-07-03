import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView } from "../components/NewTaskView";
import type { Project } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation((command: string) => {
    if (command === "list_project_files") return Promise.resolve([]);
    if (command === "get_project_git_branches") return Promise.resolve([]);
    if (command === "detect_configured_agent_models") {
      return Promise.resolve({ models: ["gpt-5.5", "gpt-5.1"] });
    }
    if (command === "get_hook_readiness") {
      return Promise.resolve([{ agent: "claude", usable: true }]);
    }
    if (command === "load_app_settings") return Promise.resolve({});
    return Promise.resolve({});
  }),
}));

const project: Project = {
  id: "project-1",
  name: "aeroric",
  path: "/tmp/aeroric",
  lastOpenedAt: 1,
};

describe("NewTaskView start terminal", () => {
  it("submits an immediate agent task when the prompt is empty", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "",
        agent: "claude",
        permissionMode: "ask",
        immediate: true,
      }),
    );
  });

  it("passes the selected terminal model when starting an empty terminal", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(await screen.findByRole("button", { name: /Model/ }));
    await user.click(await screen.findByText("gpt-5.1"));
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "",
        immediate: true,
        agentModel: "gpt-5.1",
      }),
    );
  });

  it("passes a manually entered terminal model that is not in the detected list", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(await screen.findByRole("button", { name: /Model|gpt-5.5/ }));
    await user.clear(await screen.findByPlaceholderText("Custom model"));
    await user.type(screen.getByPlaceholderText("Custom model"), "mimo-v2.5-pro");
    await user.click(screen.getByRole("button", { name: "Use" }));
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "",
        immediate: true,
        agentModel: "mimo-v2.5-pro",
      }),
    );
  });
});
