import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ModelOptionsMenu } from "../components/new-task/ModelOptionsMenu";

vi.mock("../hooks/useAgentOptions", () => ({
  useAgentOptions: () => [
    {
      value: "claude",
      label: "Claude Code",
      configFile: "",
      configLang: "json",
      codexLike: false,
    },
    {
      value: "codex",
      label: "Codex",
      configFile: "",
      configLang: "toml",
      codexLike: true,
    },
  ],
}));

function renderMenu(overrides: Partial<ComponentProps<typeof ModelOptionsMenu>> = {}) {
  localStorage.setItem("aeroric:language", "en");
  const props: ComponentProps<typeof ModelOptionsMenu> = {
    agent: "codex",
    models: ["gpt-5.6", "gpt-5.6-sol"],
    selectedModel: "gpt-5.6",
    onModelChange: vi.fn(),
    reasoningEffort: "high",
    onReasoningChange: vi.fn(),
    speed: "standard",
    onSpeedChange: vi.fn(),
    loading: false,
    error: null,
    ...overrides,
  };

  return render(
    <I18nProvider>
      <ModelOptionsMenu {...props} />
    </I18nProvider>,
  );
}

describe("ModelOptionsMenu", () => {
  it("shows the model and reasoning summary while collapsed", () => {
    renderMenu({ speed: "fast" });

    const trigger = screen.getByRole("combobox", { name: "Model" });

    expect(trigger).toHaveTextContent("gpt-5.6");
    expect(trigger).toHaveTextContent("High");
    expect(screen.getByTestId("model-summary-name")).toHaveStyle({
      color: "var(--text-primary)",
    });
    expect(screen.getByTestId("model-summary-reasoning")).toHaveStyle({
      color: "var(--text-secondary)",
    });
    expect(screen.getByTestId("fast-indicator")).toHaveAttribute("aria-label", "Fast");
  });

  it("reveals each submenu on hover without requiring a click", async () => {
    const user = userEvent.setup();
    renderMenu();

    await user.click(screen.getByRole("combobox", { name: "Model" }));

    const modelTrigger = screen.getByRole("button", { name: "Model" });
    const reasoningTrigger = screen.getByRole("button", { name: "Reasoning effort" });
    const speedTrigger = screen.getByRole("button", { name: "Speed" });

    expect(modelTrigger).toHaveAttribute("aria-haspopup", "menu");
    expect(modelTrigger.querySelector('[data-model-options-arrow="model"]')).toBeInTheDocument();

    await user.hover(modelTrigger);
    const content = document.querySelector<HTMLElement>("[data-model-options-content]");
    expect(content).toHaveAttribute("data-side", "bottom");
    expect(content?.style.width).toBe("fit-content");
    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();
    expect(screen.getByRole("menu", { name: /^Model$/ }).style.overflowY).toBe("auto");
    expect(screen.getByRole("menuitemradio", { name: "gpt-5.6" })).toBeInTheDocument();

    await user.hover(reasoningTrigger);
    expect(content).toHaveAttribute("data-side", "bottom");
    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();
    expect(screen.getByRole("menu", { name: /^Reasoning effort$/ })).toBeInTheDocument();

    await user.hover(speedTrigger);
    expect(content).toHaveAttribute("data-side", "bottom");
    expect(screen.getByRole("menu", { name: /^Speed$/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "Fast" })).toBeInTheDocument();
  });

  it("keeps labels on one line and sizes long model options to the content", async () => {
    const user = userEvent.setup();
    const longModel = "claude-sonnet-4-20250514-with-a-very-long-model-name";
    renderMenu({ models: ["short", longModel], selectedModel: longModel });

    await user.click(screen.getByRole("combobox", { name: "Model" }));
    const modelTrigger = screen.getByRole("button", { name: "Model" });
    await user.hover(modelTrigger);

    const content = document.querySelector<HTMLElement>("[data-model-options-content]");
    const modelPanel = screen.getByRole("menu", { name: /^Model$/ });
    const longModelOption = screen.getByRole("menuitemradio", { name: longModel });

    expect(content?.style.width).toBe("fit-content");
    expect(content?.style.maxWidth).toBe("calc(100vw - 20px)");
    expect(modelPanel.style.maxHeight).toContain("--radix-popover-content-available-height");
    expect(modelTrigger).toHaveStyle({ whiteSpace: "nowrap" });
    expect(longModelOption).toHaveStyle({ whiteSpace: "nowrap" });
  });

  it("keeps option selection callbacks unchanged", async () => {
    const user = userEvent.setup();
    const onReasoningChange = vi.fn();
    renderMenu({ onReasoningChange });

    await user.click(screen.getByRole("combobox", { name: "Model" }));
    fireEvent.mouseEnter(screen.getByRole("button", { name: "Reasoning effort" }));
    await user.click(screen.getByRole("menuitemradio", { name: "Low" }));

    expect(onReasoningChange).toHaveBeenCalledWith("low");
  });

  it("does not render the fast indicator in standard mode", () => {
    renderMenu({ speed: "standard" });

    expect(screen.queryByTestId("fast-indicator")).not.toBeInTheDocument();
  });
});
