import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
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

  it("labels every control and draws an icon in it", () => {
    // `Button` drops children at every `icon-*` size, so the four controls used
    // to render as blank squares with no tooltip either.
    renderComposer();
    const names = [
      "Queue for the next turn",
      "Steer the current turn",
      "Attach image",
      "Send message",
    ];
    for (const name of names) {
      const control = screen.getByRole("button", { name });
      expect(control).toHaveAttribute("title", name);
      expect(control.querySelector("svg")).not.toBeNull();
    }
  });

  it("keeps a readable label on the two submission modes", () => {
    // The mode decides whether a submission interrupts the running turn, which
    // no bare glyph conveys, so both keep their text next to the icon.
    renderComposer();
    expect(screen.getByRole("button", { name: "Queue for the next turn" })).toHaveTextContent(
      "Queue",
    );
    expect(screen.getByRole("button", { name: "Steer the current turn" })).toHaveTextContent(
      "Steer",
    );
  });

  it("marks the active submission mode as pressed", () => {
    renderComposer();
    expect(screen.getByRole("button", { name: "Queue for the next turn" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Steer the current turn" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });
});
