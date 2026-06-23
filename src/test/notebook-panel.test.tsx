import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";

function renderNotebook() {
  return render(
    <I18nProvider>
      <NotebookPanel />
    </I18nProvider>,
  );
}

describe("NotebookPanel", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("creates notes, shows a note list, and renders markdown in reading mode", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New memo" }));
    await user.clear(screen.getByRole("textbox", { name: "Memo name" }));
    await user.type(screen.getByRole("textbox", { name: "Memo name" }), "Deploy notes");
    await user.type(screen.getByRole("textbox", { name: "Memo content" }), "# Release\n\n**Ship it**");

    expect(screen.getByRole("button", { name: "Deploy notes" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Read" }));

    expect(screen.getByRole("heading", { name: "Release" })).toBeInTheDocument();
    expect(screen.getByText("Ship it")).toBeInTheDocument();
    expect(document.querySelector(".notebook-markdown-preview script")).toBeNull();
  });
});
