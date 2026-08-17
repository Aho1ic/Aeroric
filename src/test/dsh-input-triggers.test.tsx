import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import { DshComposer } from "../components/DshComposer";
import {
  DSH_MENU_CLOSED,
  detectDshTrigger,
  rankDshTriggerCandidates,
  reduceDshMenu,
  replaceDshTriggerToken,
  seedDshMenuGroups,
  type DshMenuState,
  type DshTriggerCandidate,
} from "../dshInputTriggers";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** Open a menu over `sources` and hand back the state a hit leaves behind. */
function openMenu(sources: string[]): DshMenuState {
  const seeded = seedDshMenuGroups(DSH_MENU_CLOSED, sources);
  return reduceDshMenu(seeded, {
    type: "hit",
    hit: { trigger: "/", query: "", position: "leading", span: { start: 0, end: 1, draftRev: 1 } },
  });
}

function candidates(...names: string[]): DshTriggerCandidate[] {
  return names.map((name) => ({ name }));
}

describe("detectDshTrigger", () => {
  it("detects a leading slash token under the caret", () => {
    const hit = detectDshTrigger("/comp", 5, { tier: "plain" });
    expect(hit).toEqual({
      trigger: "/",
      query: "comp",
      position: "leading",
      span: { start: 0, end: 5, draftRev: 0 },
    });
  });

  it("detects an inline mention and reports it as inline", () => {
    const hit = detectDshTrigger("ask @rev", 8, { tier: "plain" });
    expect(hit?.trigger).toBe("@");
    expect(hit?.query).toBe("rev");
    expect(hit?.position).toBe("inline");
    expect(hit?.span).toEqual({ start: 4, end: 8, draftRev: 0 });
  });

  it("treats a token after leading whitespace as leading", () => {
    expect(detectDshTrigger("  /go", 5, { tier: "plain" })?.position).toBe("leading");
  });

  it("ends the scan at whitespace", () => {
    expect(detectDshTrigger("/goal ship it", 13, { tier: "plain" })).toBeNull();
  });

  it("refuses a trigger glued to a word character", () => {
    expect(detectDshTrigger("user@host", 9, { tier: "plain" })).toBeNull();
    expect(detectDshTrigger("src/lib", 7, { tier: "plain" })).toBeNull();
  });

  it("keeps URLs and Windows paths quiet", () => {
    // The `/` of `//` and the one right after a scheme separator are dead, so a
    // pasted URL cannot open the command menu.
    expect(detectDshTrigger("see https://x", 13, { tier: "plain" })).toBeNull();
    expect(detectDshTrigger("C:/tmp", 6, { tier: "plain" })).toBeNull();
  });

  it("scans through a suppressed slash while keeping @ live", () => {
    expect(detectDshTrigger("/comp", 5, { tier: "claimed" })).toBeNull();
    const hit = detectDshTrigger("/goal @rev", 10, { tier: "claimed" });
    expect(hit?.trigger).toBe("@");
    expect(hit?.query).toBe("rev");
  });

  it("detects nothing at all while frozen", () => {
    expect(detectDshTrigger("/comp", 5, { tier: "frozen" })).toBeNull();
  });
});

describe("reduceDshMenu", () => {
  it("opens with one pending group per seeded source", () => {
    const state = openMenu(["command", "skill"]);
    expect(state.open).toBe(true);
    expect(state.generation).toBe(1);
    expect(state.groups.map((group) => group.status)).toEqual(["pending", "pending"]);
    expect(state.highlight).toBeNull();
  });

  it("highlights the first ready item and never moves it under the user", () => {
    let state = openMenu(["command", "skill"]);
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "skill",
      items: candidates("review"),
    });
    expect(state.highlight).toEqual({ source: "skill", index: 0 });
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "command",
      items: candidates("compact"),
    });
    // A still-valid highlight is sticky: the slower group settling above it must
    // not move the row Enter would pick.
    expect(state.highlight).toEqual({ source: "skill", index: 0 });
  });

  it("re-seeds the highlight when the group holding it goes away", () => {
    let state = openMenu(["command", "skill"]);
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "skill",
      items: candidates("review"),
    });
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "command",
      items: candidates("compact"),
    });
    state = reduceDshMenu(state, { type: "source-failed", generation: 1, source: "skill" });
    expect(state.highlight).toEqual({ source: "command", index: 0 });
  });

  it("drops a settlement from a superseded generation", () => {
    const state = openMenu(["command"]);
    const stale = reduceDshMenu(state, {
      type: "source-settled",
      generation: 0,
      source: "command",
      items: candidates("compact"),
    });
    expect(stale).toBe(state);
  });

  it("closes once every group is ready and empty", () => {
    let state = openMenu(["command"]);
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "command",
      items: [],
    });
    expect(state.open).toBe(false);
    expect(state.groups).toEqual([]);
    // The generation survives the close so a late settlement stays droppable.
    expect(state.generation).toBe(1);
  });

  it("removes a failed source and closes when it was the last one", () => {
    let state = openMenu(["command", "skill"]);
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "command",
      items: candidates("compact"),
    });
    state = reduceDshMenu(state, { type: "source-failed", generation: 1, source: "skill" });
    expect(state.groups.map((group) => group.source)).toEqual(["command"]);
    state = reduceDshMenu(state, { type: "source-failed", generation: 1, source: "command" });
    expect(state.open).toBe(false);
  });

  it("cycles the highlight across every ready group", () => {
    let state = openMenu(["command", "skill"]);
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "command",
      items: candidates("compact", "export"),
    });
    state = reduceDshMenu(state, {
      type: "source-settled",
      generation: 1,
      source: "skill",
      items: candidates("review"),
    });
    state = reduceDshMenu(state, { type: "move", dir: 1 });
    expect(state.highlight).toEqual({ source: "command", index: 1 });
    state = reduceDshMenu(state, { type: "move", dir: 1 });
    expect(state.highlight).toEqual({ source: "skill", index: 0 });
    state = reduceDshMenu(state, { type: "move", dir: 1 });
    expect(state.highlight).toEqual({ source: "command", index: 0 });
    state = reduceDshMenu(state, { type: "move", dir: -1 });
    expect(state.highlight).toEqual({ source: "skill", index: 0 });
  });
});

describe("rankDshTriggerCandidates", () => {
  it("keeps the roster untouched for an empty query", () => {
    const rows = candidates("compact", "export");
    expect(rankDshTriggerCandidates(rows, "")).toBe(rows);
  });

  it("finds a fuzzy subsequence and ranks prefixes first", () => {
    const rows = candidates("compact", "export", "permission");
    expect(rankDshTriggerCandidates(rows, "cmp").map((row) => row.name)).toEqual(["compact"]);
    const ranked = rankDshTriggerCandidates(candidates("export", "permission"), "p");
    expect(ranked.map((row) => row.name)).toEqual(["permission", "export"]);
  });

  it("drops names the query is not a subsequence of", () => {
    expect(rankDshTriggerCandidates(candidates("compact"), "zzz")).toEqual([]);
  });
});

describe("replaceDshTriggerToken", () => {
  it("splices the replacement over the span and reports the caret", () => {
    const next = replaceDshTriggerToken(
      "ask /comp now",
      { start: 4, end: 9, draftRev: 3 },
      "/compact ",
      3,
    );
    expect(next).toEqual({ text: "ask /compact  now", caret: 13 });
  });

  it("voids a pick taken against an older draft revision", () => {
    expect(
      replaceDshTriggerToken("/comp", { start: 0, end: 5, draftRev: 2 }, "/compact ", 3),
    ).toBeNull();
  });

  it("voids a span that no longer fits the draft", () => {
    expect(
      replaceDshTriggerToken("/c", { start: 0, end: 9, draftRev: 1 }, "/compact ", 1),
    ).toBeNull();
  });
});

function renderComposer() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <DshComposer taskId="task-1" sessionId="session-1" />
      </ToastProvider>
    </I18nProvider>,
  );
}

/** Answer the three catalog commands the composer's trigger sources pull. */
function mockCatalogs() {
  invokeMock.mockImplementation((command) => {
    if (command === "list_dsh_commands") {
      return Promise.resolve([
        { name: "compact", description: "Compress conversation history" },
        { name: "goal", description: "Manage the session goal", input: { hint: "<text>" } },
        { name: "model", description: "Switch model", input: { hint: "<id>" } },
      ]);
    }
    if (command === "list_dsh_llm_models") {
      return Promise.resolve({
        groups: [{ models: [{ id: "deepseek-v3", name: "DeepSeek V3" }] }],
      });
    }
    if (command === "list_dsh_skills") {
      return Promise.resolve([
        { id: "commit-helper", name: "commit-helper", description: "Write a commit message" },
        { id: "audit", name: "audit", whenToUse: "Review a diff", modelInvocable: false },
      ]);
    }
    if (command === "list_dsh_subagents") {
      return Promise.resolve([
        { sessionId: "child-1", running: true, mode: "explore", label: "reviewer" },
        { sessionId: "child-2", running: false, mode: "explore", label: "retired" },
      ]);
    }
    return Promise.resolve({ accepted: true });
  });
}

async function typeInComposer(input: string) {
  const box = screen.getByRole("textbox");
  await userEvent.click(box);
  await userEvent.type(box, input);
  return box;
}

describe("DSH composer trigger menu", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    mockCatalogs();
  });

  it("opens the grouped command menu from a slash under the caret", async () => {
    renderComposer();
    await typeInComposer("/co");
    const menu = await screen.findByRole("listbox", { name: "Trigger suggestions" });
    expect(within(menu).getByText("Commands")).toBeInTheDocument();
    expect(within(menu).getByRole("option", { name: /compact/ })).toBeInTheDocument();
    // `commit-helper` is the only skill whose name starts with `co`.
    expect(within(menu).getByText("Skills")).toBeInTheDocument();
    expect(within(menu).getByRole("option", { name: /commit-helper/ })).toBeInTheDocument();
    expect(within(menu).queryByRole("option", { name: /goal/ })).not.toBeInTheDocument();
  });

  it("completes the token from a clicked row without stealing focus", async () => {
    renderComposer();
    const box = await typeInComposer("ask /comp");
    const menu = await screen.findByRole("listbox", { name: "Trigger suggestions" });
    await userEvent.click(within(menu).getByRole("option", { name: /compact/ }));
    await waitFor(() => expect(box).toHaveValue("ask /compact "));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(box).toHaveFocus();
  });

  it("keeps the trigger menu shut for a pasted URL", async () => {
    renderComposer();
    const box = screen.getByRole("textbox");
    await userEvent.click(box);
    await userEvent.paste("read https://example.com/docs");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("picks the highlight on Enter and sends on the next one", async () => {
    renderComposer();
    const box = await typeInComposer("/compac");
    await screen.findByRole("listbox", { name: "Trigger suggestions" });
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(box).toHaveValue("/compact "));
    const prompts = () => invokeMock.mock.calls.filter(([name]) => name === "prompt_dsh_task");
    expect(prompts()).toHaveLength(0);
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(prompts()).toHaveLength(1));
    expect(prompts()[0]?.[1]).toMatchObject({ prompt: "/compact " });
  });

  it("moves the highlight with the arrow keys and dismisses on Escape", async () => {
    renderComposer();
    await typeInComposer("/");
    const menu = await screen.findByRole("listbox", { name: "Trigger suggestions" });
    await waitFor(() =>
      expect(menu).toHaveAttribute("aria-activedescendant", "dsh-trigger-option-command-0"),
    );
    await userEvent.keyboard("{ArrowDown}");
    expect(menu).toHaveAttribute("aria-activedescendant", "dsh-trigger-option-command-1");
    await userEvent.keyboard("{ArrowUp}");
    expect(menu).toHaveAttribute("aria-activedescendant", "dsh-trigger-option-command-0");
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("offers only running subagents under @", async () => {
    renderComposer();
    const box = await typeInComposer("hand off to @");
    const menu = await screen.findByRole("listbox", { name: "Trigger suggestions" });
    expect(within(menu).getByText("Subagents")).toBeInTheDocument();
    expect(within(menu).queryByText("Commands")).not.toBeInTheDocument();
    expect(within(menu).queryByRole("option", { name: /retired/ })).not.toBeInTheDocument();
    await userEvent.click(within(menu).getByRole("option", { name: /reviewer/ }));
    await waitFor(() => expect(box).toHaveValue("hand off to @reviewer "));
  });

  it("closes itself when nothing matches the query", async () => {
    renderComposer();
    await typeInComposer("/zzzz");
    await waitFor(() => expect(screen.queryByRole("listbox")).not.toBeInTheDocument());
  });

  it("marks a user-only skill in its description", async () => {
    renderComposer();
    await typeInComposer("/aud");
    const menu = await screen.findByRole("listbox", { name: "Trigger suggestions" });
    expect(within(menu).getByText(/user-only · Review a diff/)).toBeInTheDocument();
  });

  it("resolves a popupSelect argument through the picker after the token lands", async () => {
    renderComposer();
    const box = await typeInComposer("/mod");
    const menu = await screen.findByRole("listbox", { name: "Trigger suggestions" });
    await userEvent.click(within(menu).getByRole("option", { name: /model/ }));
    // The command token lands immediately; the picker only supplies the argument.
    await waitFor(() => expect(box).toHaveValue("/model "));
    const picker = await screen.findByRole("listbox");
    await userEvent.click(within(picker).getByRole("option", { name: "DeepSeek V3" }));
    await waitFor(() => expect(box).toHaveValue("/model deepseek-v3 "));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("resolves a popupSelect argument from the focused composer with the keyboard", async () => {
    renderComposer();
    const box = await typeInComposer("/mod");
    const menu = await screen.findByRole("listbox", { name: "Trigger suggestions" });
    await userEvent.click(within(menu).getByRole("option", { name: /model/ }));
    await waitFor(() => expect(box).toHaveValue("/model "));
    await screen.findByRole("option", { name: "DeepSeek V3" });

    expect(box).toHaveFocus();
    await userEvent.keyboard("{Enter}");

    await waitFor(() => expect(box).toHaveValue("/model deepseek-v3 "));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(box).toHaveFocus();
  });
});
