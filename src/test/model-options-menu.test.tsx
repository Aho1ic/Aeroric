import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
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

function openMainMenu() {
  fireEvent.click(screen.getByRole("combobox", { name: "Model" }));
  return screen.getByRole("menu", { name: "Model options" });
}

function hoverPanel(panel: "model" | "reasoning" | "speed", x = 100, y = 100) {
  const trigger = screen.getByRole("button", {
    name: panel === "model" ? "Model" : panel === "reasoning" ? "Reasoning effort" : "Speed",
  });
  fireEvent.mouseEnter(trigger, { clientX: x, clientY: y });
  return trigger;
}

function advanceTimers(milliseconds: number) {
  act(() => {
    vi.advanceTimersByTime(milliseconds);
  });
}

function getSubmenuContent(panel: "model" | "reasoning" | "speed") {
  return document.querySelector<HTMLElement>(
    `[data-model-options-submenu-content="${panel}"][data-side="right"]`,
  );
}

describe("ModelOptionsMenu", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

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

  it("shows only the three category entries in the main menu", () => {
    renderMenu();

    const mainMenu = openMainMenu();

    expect(mainMenu).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Model" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reasoning effort" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Speed" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitemradio")).not.toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();
  });

  it("does not open a submenu before the 300ms stationary delay", () => {
    vi.useFakeTimers();
    renderMenu();
    openMainMenu();

    hoverPanel("model");
    advanceTimers(299);

    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();
  });

  it("keeps waiting while the pointer is moving noticeably", () => {
    vi.useFakeTimers();
    renderMenu();
    openMainMenu();

    const trigger = hoverPanel("model", 100, 100);
    for (let step = 1; step <= 4; step += 1) {
      advanceTimers(100);
      fireEvent.mouseMove(trigger, { clientX: 100 + step * 12, clientY: 100 });
    }

    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();

    advanceTimers(299);
    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();
    advanceTimers(1);
    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();
  });

  it("opens after 300ms when only slight pointer jitter occurs", () => {
    vi.useFakeTimers();
    renderMenu();
    openMainMenu();

    const trigger = hoverPanel("model", 100, 100);
    advanceTimers(299);
    fireEvent.mouseMove(trigger, { clientX: 101, clientY: 101 });
    advanceTimers(1);

    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();
  });

  it("keeps the main menu below and replaces the child menu only after the next delay", () => {
    vi.useFakeTimers();
    renderMenu();
    const mainMenu = openMainMenu();

    hoverPanel("model");
    advanceTimers(300);

    const mainContent = document.querySelector<HTMLElement>("[data-model-options-content]");
    expect(mainContent).toHaveAttribute("data-side", "bottom");
    expect(getSubmenuContent("model")).toHaveAttribute("data-side", "right");
    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();

    const reasoningTrigger = screen.getByRole("button", { name: "Reasoning effort" });
    fireEvent.mouseEnter(reasoningTrigger, { clientX: 200, clientY: 100 });

    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: /^Reasoning effort$/ })).not.toBeInTheDocument();

    advanceTimers(299);
    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();
    advanceTimers(1);

    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();
    expect(screen.getByRole("menu", { name: /^Reasoning effort$/ })).toBeInTheDocument();
    expect(mainContent).toHaveAttribute("data-side", "bottom");
    expect(getSubmenuContent("reasoning")).toHaveAttribute("data-side", "right");
    expect(mainMenu).toBeInTheDocument();
  });

  it("keeps labels and long model options on one line with content-sized menus", () => {
    vi.useFakeTimers();
    const longModel = "claude-sonnet-4-20250514-with-a-very-long-model-name";
    renderMenu({ models: ["short", longModel], selectedModel: longModel });

    openMainMenu();
    const modelTrigger = hoverPanel("model");
    advanceTimers(300);

    const content = document.querySelector<HTMLElement>("[data-model-options-content]");
    const mainMenu = screen.getByRole("menu", { name: "Model options" });
    const modelPanel = screen.getByRole("menu", { name: /^Model$/ });
    const longModelOption = screen.getByRole("menuitemradio", { name: longModel });

    expect(content?.style.width).toBe("fit-content");
    expect(content?.style.maxWidth).toBe("calc(100vw - 20px)");
    expect(modelPanel.style.maxHeight).toContain("--radix-popover-content-available-height");
    expect(modelTrigger).toHaveStyle({ whiteSpace: "nowrap" });
    expect(screen.getByRole("button", { name: "Reasoning effort" })).toHaveStyle({
      whiteSpace: "nowrap",
    });
    expect(screen.getByRole("button", { name: "Speed" })).toHaveStyle({
      whiteSpace: "nowrap",
    });
    expect(longModelOption).toHaveStyle({ whiteSpace: "nowrap" });
    expect(mainMenu).not.toContainElement(longModelOption);
    expect(getSubmenuContent("model")).toHaveAttribute("data-side", "right");
  });

  it("keeps both menus open after leaving an entry and selecting a child option", () => {
    vi.useFakeTimers();
    const onReasoningChange = vi.fn();
    renderMenu({ onReasoningChange });
    openMainMenu();

    const reasoningTrigger = hoverPanel("reasoning");
    advanceTimers(300);
    const reasoningMenu = screen.getByRole("menu", { name: /^Reasoning effort$/ });

    fireEvent.mouseLeave(reasoningTrigger);
    expect(reasoningMenu).toBeInTheDocument();

    fireEvent.click(screen.getByRole("menuitemradio", { name: "Low" }));

    expect(onReasoningChange).toHaveBeenCalledWith("low");
    expect(screen.getByRole("menu", { name: "Model options" })).toBeInTheDocument();
    expect(screen.getByRole("menu", { name: /^Reasoning effort$/ })).toBeInTheDocument();
  });

  it("closes the parent and child menus on an outside pointer event", () => {
    vi.useFakeTimers();
    renderMenu();
    openMainMenu();
    hoverPanel("model");
    advanceTimers(300);

    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();
    fireEvent.pointerDown(document.body);

    expect(screen.queryByRole("menu", { name: "Model options" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();
  });

  it("closes both menus with Escape while a child menu is open", () => {
    vi.useFakeTimers();
    renderMenu();
    openMainMenu();
    hoverPanel("model");
    advanceTimers(300);

    fireEvent.keyDown(document, { key: "Escape", code: "Escape" });

    expect(screen.queryByRole("menu", { name: "Model options" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: /^Model$/ })).not.toBeInTheDocument();
  });

  it("keeps option selection callbacks unchanged", () => {
    vi.useFakeTimers();
    const onModelChange = vi.fn();
    renderMenu({ onModelChange });
    openMainMenu();
    hoverPanel("model");
    advanceTimers(300);

    fireEvent.click(screen.getByRole("menuitemradio", { name: "gpt-5.6-sol" }));

    expect(onModelChange).toHaveBeenCalledWith("gpt-5.6-sol");
    expect(screen.getByRole("menu", { name: /^Model$/ })).toBeInTheDocument();
  });

  it("does not render the fast indicator in standard mode", () => {
    renderMenu({ speed: "standard" });

    expect(screen.queryByTestId("fast-indicator")).not.toBeInTheDocument();
  });
});
