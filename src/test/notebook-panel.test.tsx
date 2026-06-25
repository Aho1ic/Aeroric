import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
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

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Deploy notes");
    expect(screen.queryByRole("button", { name: "Create quick note" })).not.toBeInTheDocument();
    await user.type(
      screen.getByRole("textbox", { name: "Quick note content" }),
      "# Release\n\n**Ship it**",
    );

    expect(screen.getByRole("button", { name: "Deploy notes" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Read" }));

    expect(screen.getByRole("heading", { name: "Release" })).toBeInTheDocument();
    expect(screen.getByText("Ship it")).toBeInTheDocument();
    expect(document.querySelector(".notebook-markdown-preview script")).toBeNull();
  });

  it("creates rich text notes from the plus popover with enter", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    expect(screen.getByRole("menuitem", { name: "Markdown" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Text" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "TXT" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Rich note");

    expect(screen.getByRole("button", { name: "Rich note" })).toBeInTheDocument();
    expect(screen.getByText("Text")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Quick note content" })).toHaveAttribute(
      "contenteditable",
      "true",
    );
  });

  it("creates a blank markdown note after choosing a format from the plus menu and focuses the title", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));

    expect(screen.getByRole("menuitem", { name: "Markdown" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Text" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Quick note name" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));

    const titleInputs = screen.getAllByRole("textbox", { name: "Quick note name" });
    expect(titleInputs).toHaveLength(1);
    expect(titleInputs[0]).toHaveFocus();
    expect(titleInputs[0]).toHaveValue("");
    expect(screen.getByText("Markdown")).toBeInTheDocument();
  });

  it("cancels new note creation when the format menu loses focus", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    expect(screen.getByRole("menuitem", { name: "Markdown" })).toBeInTheDocument();
    fireEvent.mouseDown(document.body);

    await waitFor(() =>
      expect(screen.queryByRole("menuitem", { name: "Markdown" })).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("textbox", { name: "Quick note name" })).not.toBeInTheDocument();
  });

  it("applies markdown formatting to selected text from the toolbar", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Format note");
    const body = screen.getByRole("textbox", { name: "Quick note content" }) as HTMLTextAreaElement;
    await user.type(body, "selected");
    body.setSelectionRange(0, "selected".length);

    await user.click(screen.getByRole("button", { name: "Bold" }));

    expect(body.value).toBe("**selected**");
  });

  it("applies rich text formatting through editing commands", async () => {
    const user = userEvent.setup();
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Rich format note");
    const body = screen.getByRole("textbox", { name: "Quick note content" });

    await user.click(body);
    await user.click(screen.getByRole("button", { name: "Bold" }));
    await user.click(screen.getByRole("button", { name: "Numbered list" }));

    expect(execCommand).toHaveBeenCalledWith("bold", false);
    expect(execCommand).toHaveBeenCalledWith("insertOrderedList", false);
  });

  it("applies selected text and background colors from color pickers", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Color note");
    const body = screen.getByRole("textbox", { name: "Quick note content" }) as HTMLTextAreaElement;
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

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Structure note");
    const body = screen.getByRole("textbox", { name: "Quick note content" }) as HTMLTextAreaElement;
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

  it("keeps the memo list visible without collapse controls", () => {
    renderNotebook();

    expect(screen.getAllByText("No quick notes yet")).toHaveLength(2);
    expect(
      screen.queryByRole("button", { name: "Collapse quick note list" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Expand quick note list" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/memo/i)).not.toBeInTheDocument();
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

  it("migrates existing txt notes to rich text format", async () => {
    localStorage.setItem(
      "aeroric:notebook:v1",
      JSON.stringify([
        { id: "txt-note", title: "Legacy TXT", body: "first\nsecond", format: "txt", updatedAt: 1 },
      ]),
    );

    renderNotebook();

    expect(screen.getByText("Text")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Quick note content" })).toHaveAttribute(
      "contenteditable",
      "true",
    );

    await userEvent.setup().click(screen.getByRole("button", { name: "Read" }));

    expect(screen.getByText("first")).toBeInTheDocument();
    expect(screen.getByText("second")).toBeInTheDocument();
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

  it("keeps rich text typing in natural insertion order", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Typing note");

    const body = screen.getByRole("textbox", { name: "Quick note content" });
    await user.click(body);
    await user.type(body, "1234");

    expect(body).toHaveTextContent("1234");
    expect(body).not.toHaveTextContent("4321");
  });

  it("shows Chinese rich text context menu actions", async () => {
    const user = userEvent.setup();
    localStorage.setItem("aeroric:language", "zh");
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "新建随手记" }));
    await user.click(screen.getByRole("menuitem", { name: "文本" }));
    await user.type(screen.getByRole("textbox", { name: "随手记名称" }), "中文菜单");

    fireEvent.contextMenu(screen.getByRole("textbox", { name: "随手记内容" }));

    expect(screen.getByRole("menuitem", { name: "剪切" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "复制" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "粘贴" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "粗体" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "表格" })).toBeInTheDocument();
  });

  it("shows Chinese markdown context menu actions", async () => {
    const user = userEvent.setup();
    localStorage.setItem("aeroric:language", "zh");
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "新建随手记" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));

    fireEvent.contextMenu(screen.getByRole("textbox", { name: "随手记内容" }));

    expect(screen.getByRole("menuitem", { name: "剪切" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "复制" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "粘贴" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "粗体" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "表格" })).toBeInTheDocument();
  });

  it("restores the rich text selection before applying and clearing background color", async () => {
    const user = userEvent.setup();
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    const body = screen.getByRole("textbox", { name: "Quick note content" });

    const selection = document.getSelection();
    const range = document.createRange();
    range.selectNodeContents(body);
    selection?.removeAllRanges();
    selection?.addRange(range);
    fireEvent.mouseUp(body);

    fireEvent.mouseDown(screen.getByLabelText("Background color"));
    fireEvent.change(screen.getByLabelText("Background color"), {
      target: { value: "#00ff00" },
    });
    await user.click(screen.getByRole("button", { name: "No color" }));

    expect(execCommand).toHaveBeenCalledWith("hiliteColor", false, "#00ff00");
    expect(execCommand).toHaveBeenCalledWith("hiliteColor", false, "transparent");
  });

  it("reflects current rich text selection formatting in toolbar pressed states", async () => {
    const user = userEvent.setup();
    const queryCommandState = vi.fn(
      (command: string) => command === "bold" || command === "insertUnorderedList",
    );
    Object.defineProperty(document, "queryCommandState", {
      configurable: true,
      value: queryCommandState,
    });
    Object.defineProperty(document, "queryCommandValue", {
      configurable: true,
      value: vi.fn(() => "h1"),
    });
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "State note");
    fireEvent.mouseUp(screen.getByRole("textbox", { name: "Quick note content" }));

    expect(screen.getByRole("button", { name: "Bold" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Bullet list" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Heading" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Italic" })).toHaveAttribute("aria-pressed", "false");
  });

  it("inserts a rich text table from a hoverable size grid", async () => {
    const user = userEvent.setup();
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Table note");

    await user.click(screen.getByRole("button", { name: "Table" }));
    const sizeCell = screen.getByRole("button", { name: "3 x 4" });
    fireEvent.mouseEnter(sizeCell);

    expect(screen.getByText("3 x 4")).toBeInTheDocument();
    await user.click(sizeCell);

    expect(execCommand).toHaveBeenCalledWith(
      "insertHTML",
      false,
      expect.stringContaining("<table"),
    );
    expect(execCommand).toHaveBeenCalledWith(
      "insertHTML",
      false,
      expect.stringContaining("<td><br></td>"),
    );
  });
});
