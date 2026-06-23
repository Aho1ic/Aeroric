import { fireEvent, render, screen } from "@testing-library/react";
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
    await user.type(screen.getByRole("textbox", { name: "Memo name" }), "Deploy notes");
    await user.click(screen.getByRole("radio", { name: "Markdown" }));
    await user.click(screen.getByRole("button", { name: "Create memo" }));
    await user.type(screen.getByRole("textbox", { name: "Memo content" }), "# Release\n\n**Ship it**");

    expect(screen.getByRole("button", { name: "Deploy notes" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Read" }));

    expect(screen.getByRole("heading", { name: "Release" })).toBeInTheDocument();
    expect(screen.getByText("Ship it")).toBeInTheDocument();
    expect(document.querySelector(".notebook-markdown-preview script")).toBeNull();
  });

  it("creates txt notes without showing markdown formatting controls", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New memo" }));
    await user.type(screen.getByRole("textbox", { name: "Memo name" }), "Plain note");
    await user.click(screen.getByRole("radio", { name: "TXT" }));
    await user.click(screen.getByRole("button", { name: "Create memo" }));

    expect(screen.getByRole("button", { name: "Plain note" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
  });

  it("applies markdown formatting to selected text from the toolbar", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New memo" }));
    await user.type(screen.getByRole("textbox", { name: "Memo name" }), "Format note");
    await user.click(screen.getByRole("button", { name: "Create memo" }));
    const body = screen.getByRole("textbox", { name: "Memo content" }) as HTMLTextAreaElement;
    await user.type(body, "selected");
    body.setSelectionRange(0, "selected".length);

    await user.click(screen.getByRole("button", { name: "Bold" }));

    expect(body.value).toBe("**selected**");
  });

  it("applies txt formatting to selected text with the same toolbar", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New memo" }));
    await user.type(screen.getByRole("textbox", { name: "Memo name" }), "Txt format note");
    await user.click(screen.getByRole("radio", { name: "TXT" }));
    await user.click(screen.getByRole("button", { name: "Create memo" }));
    const body = screen.getByRole("textbox", { name: "Memo content" }) as HTMLTextAreaElement;
    await user.type(body, "first\nsecond");
    body.setSelectionRange(0, "first\nsecond".length);

    await user.click(screen.getByRole("button", { name: "Numbered list" }));

    expect(body.value).toBe("1. first\n2. second");
  });

  it("applies selected text and background colors from color pickers", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New memo" }));
    await user.type(screen.getByRole("textbox", { name: "Memo name" }), "Color note");
    await user.click(screen.getByRole("button", { name: "Create memo" }));
    const body = screen.getByRole("textbox", { name: "Memo content" }) as HTMLTextAreaElement;
    await user.type(body, "color");
    body.setSelectionRange(0, "color".length);

    fireEvent.change(screen.getByLabelText("Text color"), { target: { value: "#ff0000" } });
    expect(body.value).toBe('<span style="color:#ff0000">color</span>');

    body.setSelectionRange(0, body.value.length);
    fireEvent.change(screen.getByLabelText("Background color"), {
      target: { value: "#00ff00" },
    });
    expect(body.value).toBe(
      '<span style="background-color:#00ff00"><span style="color:#ff0000">color</span></span>',
    );
  });

  it("builds structural markdown snippets from selected text instead of examples", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New memo" }));
    await user.type(screen.getByRole("textbox", { name: "Memo name" }), "Structure note");
    await user.click(screen.getByRole("button", { name: "Create memo" }));
    const body = screen.getByRole("textbox", { name: "Memo content" }) as HTMLTextAreaElement;
    await user.type(body, "alpha beta");
    body.setSelectionRange(0, "alpha beta".length);

    await user.click(screen.getByRole("button", { name: "Code block" }));
    body.setSelectionRange(0, body.value.length);
    await user.click(screen.getByRole("button", { name: "Table" }));

    expect(body.value).toContain("```");
    expect(body.value).toContain("alpha beta");
    expect(body.value).toContain("| Column 1 | Column 2 |");
    expect(body.value).not.toContain("| Value 1 | Value 2 |");
  });

  it("folds and restores the memo list", async () => {
    const user = userEvent.setup();
    renderNotebook();

    expect(screen.getAllByText("No memos yet")).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Collapse note list" }));
    expect(screen.getAllByText("No memos yet")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Expand note list" }));
    expect(screen.getAllByText("No memos yet")).toHaveLength(2);
  });

  it("migrates existing notes to markdown format", async () => {
    localStorage.setItem(
      "aeroric:notebook:v1",
      JSON.stringify([{ id: "legacy", title: "Legacy", body: "# Old", updatedAt: 1 }]),
    );

    renderNotebook();

    await userEvent.setup().click(screen.getByRole("button", { name: "Read" }));

    expect(screen.getByRole("heading", { name: "Old" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
  });

  it("does not render the old half-screen toggle in project mode", () => {
    render(
      <I18nProvider>
        <NotebookPanel />
      </I18nProvider>,
    );

    expect(screen.queryByRole("button", { name: "Full screen" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Half screen" })).not.toBeInTheDocument();
  });
});
