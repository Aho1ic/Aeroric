import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import { DshComposer } from "../components/DshComposer";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

const invokeMock = vi.mocked(invoke);

function renderComposer() {
  localStorage.setItem("aeroric:language", "en");
  return render(
    <I18nProvider>
      <ToastProvider>
        <DshComposer taskId="task-1" sessionId="session-1" />
      </ToastProvider>
    </I18nProvider>,
  );
}

describe("DSH composer controls", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("labels every icon control and draws an icon in it", () => {
    // `Button` drops children at every `icon-*` size, so these controls used to
    // render as blank squares with no tooltip either.
    renderComposer();
    for (const name of ["Attach image", "Send message"]) {
      const control = screen.getByRole("button", { name });
      expect(control).toHaveAttribute("title", name);
      expect(control.querySelector("svg")).not.toBeNull();
    }
  });

  it("offers the two submission modes as one labelled choice", () => {
    // Queue and steer are exclusive, so they read as one select rather than two
    // pressed buttons. The label stays visible: whether a submission interrupts
    // the running turn is not conveyed by a glyph.
    renderComposer();
    const mode = screen.getByRole("combobox", { name: "Submission mode" });
    expect(mode).toHaveValue("queue");
    expect(mode).toHaveAttribute("title", "Queue for the next turn");
    expect(Array.from(mode.querySelectorAll("option")).map((option) => option.textContent)).toEqual(
      ["Queue", "Steer"],
    );
  });

  it("switches the submission mode and names the one in effect", async () => {
    renderComposer();
    const mode = screen.getByRole("combobox", { name: "Submission mode" });
    await userEvent.selectOptions(mode, "steer");
    expect(mode).toHaveValue("steer");
    expect(mode).toHaveAttribute("title", "Steer the current turn");
  });
});
