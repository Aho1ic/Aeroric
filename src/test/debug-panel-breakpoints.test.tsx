import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DebugPanel } from "../components/debug/DebugPanel";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderBreakpointPanel() {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "read_debug_configs") {
      return Promise.resolve({ version: 1, configs: [] });
    }
    return Promise.resolve(undefined);
  });

  render(
    <I18nProvider>
      <DebugPanel projectPath="/repo" width={380} onOpenLocation={vi.fn()} />
    </I18nProvider>,
  );
}

describe("DebugPanel breakpoint editor", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("edits breakpoint rows and keeps raw breakpoint text in sync", async () => {
    const user = userEvent.setup();
    renderBreakpointPanel();

    await screen.findByText("No debug configurations yet.");
    expect(screen.getByText("No breakpoints.")).toBeInTheDocument();

    await user.type(screen.getByLabelText("New breakpoint file"), "src/index.js");
    await user.clear(screen.getByLabelText("New breakpoint line"));
    await user.type(screen.getByLabelText("New breakpoint line"), "12");
    await user.click(screen.getByRole("button", { name: "Add breakpoint" }));

    const rawBreakpoints = screen.getByLabelText("Raw breakpoints");
    expect(rawBreakpoints).toHaveValue("src/index.js:12");

    let firstRow = screen.getByRole("group", { name: "Breakpoint 1" });
    await user.click(within(firstRow).getByRole("button", { name: "If" }));
    await waitFor(() => {
      expect(rawBreakpoints).toHaveValue("src/index.js:12 if true");
    });

    firstRow = screen.getByRole("group", { name: "Breakpoint 1" });
    const expression = within(firstRow).getByLabelText("Expression");
    await user.clear(expression);
    await user.type(expression, "count > 0");
    await user.tab();

    await waitFor(() => {
      expect(rawBreakpoints).toHaveValue("src/index.js:12 if count > 0");
    });

    firstRow = screen.getByRole("group", { name: "Breakpoint 1" });
    await user.click(within(firstRow).getByRole("button", { name: "Log" }));
    await waitFor(() => {
      expect(rawBreakpoints).toHaveValue("src/index.js:12 log count > 0");
    });

    firstRow = screen.getByRole("group", { name: "Breakpoint 1" });
    await user.click(within(firstRow).getByRole("button", { name: "Remove breakpoint" }));
    expect(rawBreakpoints).toHaveValue("");
    expect(screen.getByText("No breakpoints.")).toBeInTheDocument();
  });
});
