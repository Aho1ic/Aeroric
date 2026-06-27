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

function selectElementContents(element: Element) {
  const selection = document.getSelection();
  const range = document.createRange();
  range.selectNodeContents(element);
  selection?.removeAllRanges();
  selection?.addRange(range);
  fireEvent.mouseUp(element);
}

function selectTextareaRange(textarea: HTMLTextAreaElement, start: number, end: number) {
  textarea.setSelectionRange(start, end);
  fireEvent.select(textarea);
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

  it("renames a note in place from the note list on double click", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Draft note");

    await user.dblClick(screen.getByRole("button", { name: "Draft note" }));
    const listTitleInput = screen.getByRole("textbox", { name: "Rename quick note" });
    expect(listTitleInput).toHaveValue("Draft note");

    await user.clear(listTitleInput);
    await user.type(listTitleInput, "Renamed note");
    fireEvent.keyDown(listTitleInput, { key: "Enter" });

    expect(screen.getByRole("button", { name: "Renamed note" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Renamed note");
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
    selectTextareaRange(body, 0, "selected".length);

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
    await user.type(body, "selected");
    selectElementContents(body);
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
    selectTextareaRange(body, 0, "color".length);

    fireEvent.change(screen.getByLabelText("Text color"), { target: { value: "#ff0000" } });
    expect(body.value).toBe('<span style="color:#ff0000">color</span>');

    selectTextareaRange(body, 0, body.value.length);
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
    selectTextareaRange(body, 0, "alpha beta".length);

    await user.click(screen.getByRole("button", { name: "Code block" }));
    selectTextareaRange(body, 0, body.value.length);
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

    await user.type(body, "selected");
    selectElementContents(body);

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
    const body = screen.getByRole("textbox", { name: "Quick note content" });
    await user.type(body, "selected");
    selectElementContents(body);

    expect(screen.getByRole("button", { name: "Bold" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Bullet list" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Heading" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Italic" })).toHaveAttribute("aria-pressed", "false");
  });

  it("inserts a rich text table from a hoverable size grid without requiring a selection", async () => {
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
      expect.stringContaining('style="border:1px solid'),
    );
    expect(execCommand).toHaveBeenCalledTimes(1);
  });

  it("keeps markdown formatting tools clickable without a selection but leaves content unchanged", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Needs selection");

    const body = screen.getByRole("textbox", { name: "Quick note content" }) as HTMLTextAreaElement;
    await user.type(body, "selected");
    selectTextareaRange(body, body.value.length, body.value.length);

    expect(screen.getByRole("button", { name: "Bold" })).not.toBeDisabled();
    expect(screen.getByLabelText("Text color")).not.toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Bold" }));
    expect(body.value).toBe("selected");

    selectTextareaRange(body, 0, body.value.length);

    expect(screen.getByRole("button", { name: "Bold" })).not.toBeDisabled();
    expect(screen.getByLabelText("Text color")).not.toBeDisabled();
  });

  it("keeps rich text toolbar clickable without a selection but does not run format commands", async () => {
    const user = userEvent.setup();
    const execCommand = vi.fn(() => true);
    const queryCommandState = vi.fn((command: string) => command === "bold");
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    Object.defineProperty(document, "queryCommandState", {
      configurable: true,
      value: queryCommandState,
    });
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    const body = screen.getByRole("textbox", { name: "Quick note content" });
    await user.type(body, "selected");

    const boldButtonBeforeSelection = screen.getByRole("button", { name: "Bold" });
    expect(boldButtonBeforeSelection).not.toBeDisabled();
    await user.click(boldButtonBeforeSelection);
    expect(execCommand).not.toHaveBeenCalled();

    selectElementContents(body);

    const boldButton = screen.getByRole("button", { name: "Bold" });
    expect(boldButton).not.toBeDisabled();
    expect(boldButton).toHaveAttribute("aria-pressed", "true");

    fireEvent.contextMenu(body);
    const boldMenuItem = screen.getByRole("menuitem", { name: "Bold" });
    expect(boldMenuItem).not.toBeDisabled();
    expect(boldMenuItem).toHaveAttribute("aria-checked", "true");
  });

  it("keeps rich text toolbar commands as toggles so active formatting can be cleared", async () => {
    const user = userEvent.setup();
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    Object.defineProperty(document, "queryCommandState", {
      configurable: true,
      value: vi.fn((command: string) => command === "bold"),
    });
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    const body = screen.getByRole("textbox", { name: "Quick note content" });
    await user.type(body, "selected");
    selectElementContents(body);

    const boldButton = screen.getByRole("button", { name: "Bold" });
    expect(boldButton).toHaveAttribute("aria-pressed", "true");
    await user.click(boldButton);

    expect(execCommand).toHaveBeenCalledWith("bold", false);
  });

  it("shows quick note names in bold in the memo list", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Bold list name");

    expect(screen.getByRole("button", { name: "Bold list name" })).toHaveStyle({
      fontWeight: "700",
    });
  });

  it("inserts selected rich text into a code block instead of replacing it with a placeholder", async () => {
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
    await user.type(body, "print(1)");
    selectElementContents(body);

    await user.click(screen.getByRole("button", { name: "Code block" }));

    expect(execCommand).toHaveBeenCalledWith(
      "insertHTML",
      false,
      expect.stringContaining("print(1)"),
    );
    expect(execCommand).toHaveBeenCalledWith(
      "insertHTML",
      false,
      expect.stringContaining("data-notebook-code-language"),
    );
    expect(execCommand).not.toHaveBeenCalledWith(
      "insertHTML",
      false,
      expect.stringContaining(">Code<"),
    );
  });

  it("renders the rich text table picker as a top layer and inserts tables with visible borders", async () => {
    const user = userEvent.setup();
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));
    await user.click(screen.getByRole("button", { name: "Table" }));

    expect(screen.getByRole("dialog", { name: "Table size" })).toHaveAttribute(
      "data-notebook-table-layer",
      "top",
    );
    await user.click(screen.getByRole("button", { name: "2 x 2" }));

    expect(execCommand).toHaveBeenCalledWith(
      "insertHTML",
      false,
      expect.stringContaining("border:1px solid"),
    );
  });

  it("keeps list markers inside the rich text body area", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Text" }));

    const body = screen.getByRole("textbox", { name: "Quick note content" });
    expect(body).toHaveStyle({
      padding: "12px 12px 12px 28px",
    });
  });

  it("does not prevent markdown clipboard and undo keyboard shortcuts", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    const body = screen.getByRole("textbox", { name: "Quick note content" });

    for (const key of ["c", "v", "x", "z"]) {
      const cancelled = !fireEvent.keyDown(body, { key, metaKey: true });
      expect(cancelled).toBe(false);
    }
  });

  it("persists manual quick note ordering after drag and drop", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "First");
    await user.click(screen.getByRole("button", { name: "New quick note" }));
    await user.click(screen.getByRole("menuitem", { name: "Markdown" }));
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Second");

    const first = screen.getByRole("button", { name: "First" });
    const second = screen.getByRole("button", { name: "Second" });
    fireEvent.dragStart(first);
    fireEvent.dragOver(second);
    fireEvent.drop(second);

    const stored = JSON.parse(localStorage.getItem("aeroric:notebook:v1") ?? "[]") as Array<{
      title: string;
    }>;
    expect(stored.map((note) => note.title)).toEqual(["First", "Second"]);
  });
});
