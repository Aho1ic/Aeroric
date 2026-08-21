import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { DshSessionEvent } from "../dshSessionFeatures";
import type { Task } from "../types";
import { DshLiveBars, DshTerminalHeaderActions } from "../components/DshLiveBars";
import { RunningView } from "../components/RunningView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

vi.mock("../components/TerminalView", () => ({
  TerminalView: () => <div data-testid="terminal-surface" />,
}));

vi.mock("../components/DshComposer", () => ({
  DshComposer: () => <div data-testid="dsh-composer" />,
}));

// Locating a row scrolls it into view, which jsdom does not implement.
Element.prototype.scrollIntoView ??= () => {};

/** One turn: a prompt, a reply, and the tool call the reply ordered. */
function turnEvents(): DshSessionEvent[] {
  return [
    { type: "turn/start", seq: 1, time: 1_000, data: { turn: 1 } },
    { type: "user/message", seq: 2, time: 1_010, data: { turn: 1, content: "list the files" } },
    { type: "step/start", seq: 3, time: 1_100, data: { turn: 1, step: 1 } },
    {
      type: "assistant/message",
      seq: 4,
      time: 1_400,
      data: { turn: 1, step: 1, content: "on it" },
    },
    {
      type: "tool/call",
      seq: 5,
      time: 1_500,
      data: { turn: 1, step: 1, callId: "c1", name: "bash", arguments: '{"command":"ls -1"}' },
    },
    {
      type: "tool/result",
      seq: 6,
      time: 5_228,
      data: { turn: 1, step: 1, callId: "c1", content: "README.md\nsrc" },
    },
  ];
}

/** The same turn, preceded by the request header that declares `bash`. */
function schemaEvents(): DshSessionEvent[] {
  const header: DshSessionEvent = {
    type: "request/header",
    seq: 4,
    time: 1_120,
    data: {
      turn: 1,
      step: 1,
      header: {
        tools: [
          {
            name: "bash",
            description: "Run a bash command in the workspace.",
            parameters: {
              type: "object",
              properties: {
                command: { type: "string", description: "The bash command to run." },
              },
              required: ["command"],
            },
          },
        ],
      },
    },
  };
  // The header sits between the step start and the reply, so the seqs after it
  // shift up by one and the ledger still reads the events in order.
  const tail = turnEvents()
    .slice(3)
    .map((event) => ({ ...event, seq: (event.seq as number) + 1 }));
  return [...turnEvents().slice(0, 3), header, ...tail];
}

const dshTask: Task = {
  id: "task-1",
  projectId: "project-1",
  prompt: "list the files",
  agent: "dsh",
  permissionMode: "ask",
  status: "running",
  createdAt: 1_700_000_000_000,
  sessionFamily: "dsh",
  dshSessionId: "session-1",
};

function mockHistory(events: readonly DshSessionEvent[]) {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "get_dsh_session_history") {
      return Promise.resolve({
        events: events.map((event) => ({ event })),
        hasMore: false,
        projections: { values: {} },
      });
    }
    return Promise.resolve(null);
  });
}

function renderRunningView() {
  return render(
    <I18nProvider>
      <RunningView
        task={dshTask}
        projectPath="/tmp/project"
        onCancel={vi.fn()}
        onReconnect={vi.fn()}
        onMarkDone={vi.fn()}
        onInput={vi.fn()}
        onResize={vi.fn()}
        onRegisterTerminal={vi.fn(() => 1)}
        onTerminalReady={vi.fn()}
        onSnapshot={vi.fn()}
        getRestoreState={() => ({})}
        onRename={vi.fn()}
        onGenerateName={vi.fn(async () => {})}
        themeVariant="light"
        terminalFontSize={11}
        monoFontFamily="monospace"
        liveBars={<DshLiveBars sessionId="session-1" live={{ planMode: false }} />}
        headerActions={<DshTerminalHeaderActions sessionId="session-1" />}
        dshTrajectory={{ sessionId: "session-1" }}
      />
    </I18nProvider>,
  );
}

/** Open the panel the way a user does: from the trigger above the terminal. */
async function openTrajectory(events?: readonly DshSessionEvent[]) {
  if (events !== undefined) mockHistory(events);
  renderRunningView();
  await userEvent.click(screen.getByRole("button", { name: /^Trajectory/ }));
  return await screen.findByRole("dialog", { name: "DeepSeek Harness trajectory" });
}

/** jsdom has no layout and no pointer capture; a click on the overview needs both. */
function measureTrack(track: HTMLElement, width = 400) {
  track.setPointerCapture = vi.fn();
  vi.spyOn(track, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: width,
    bottom: 42,
    width,
    height: 42,
    toJSON: () => ({}),
  });
}

/** Press and release at one fraction of the track's width: a locating click. */
function clickTrack(track: HTMLElement, at: number) {
  const x = track.getBoundingClientRect().width * at;
  fireEvent.pointerDown(track, { button: 0, pointerId: 1, clientX: x });
  fireEvent.pointerUp(track, { pointerId: 1, clientX: x });
}

function ledgerRows(panel: HTMLElement) {
  return Array.from(panel.querySelectorAll<HTMLElement>(".dsh-ledger-row"));
}

function detailColumn(panel: HTMLElement) {
  return within(panel).getByRole("complementary", { name: "Row detail" });
}

function detailTabs(panel: HTMLElement) {
  return within(detailColumn(panel)).getByRole("navigation", { name: "Detail views" });
}

describe("DSH trajectory panel placement", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockHistory(turnEvents());
  });

  it("names the trigger after the trajectory it opens", () => {
    renderRunningView();
    const trigger = screen.getByRole("button", { name: /^Trajectory/ });
    expect(trigger).toHaveAttribute("title", "Trajectory");
    expect(trigger).toHaveClass("dsh-insights-trigger");
  });

  it("covers the terminal rather than the window, at the terminal's own size", async () => {
    const panel = await openTrajectory();
    const overlay = panel.parentElement as HTMLElement;
    expect(overlay).toHaveClass("dsh-trajectory-overlay");
    // The overlay's parent is the box the terminal is drawn in, so `inset: 0` on
    // the overlay is the terminal's size by construction.
    const terminalBox = overlay.parentElement as HTMLElement;
    expect(within(terminalBox).getByTestId("terminal-surface")).toBeInTheDocument();
    expect(terminalBox.style.position).toBe("relative");
    // The old centered dialog was a viewport-fixed backdrop; nothing renders one.
    expect(document.querySelector(".dsh-insights-backdrop")).toBeNull();
  });

  it("reads the session once, from the host both the trigger and the panel share", async () => {
    await openTrajectory();
    const fetches = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "get_dsh_session_history");
    expect(fetches).toHaveLength(1);
  });

  it("opens every view from the terminal header rather than a row of its own", async () => {
    // The panel's tab row is gone: each view opens straight from the meta row,
    // so the panel spends none of its height on a selector.
    const panel = await openTrajectory();
    expect(within(panel).queryByRole("navigation")).not.toBeInTheDocument();

    for (const [view, assertion] of [
      ["Stats", () => within(panel).getByText("Steps")],
      [
        "Produced files",
        () => within(panel).getByText("This session has not produced an openable file."),
      ],
      ["Workflows", () => within(panel).getByText("This session has not run a workflow.")],
      ["Reminders", () => within(panel).getByText("This session has no reminder records.")],
      ["Feedback", () => within(panel).getByRole("button", { name: "Send session feedback" })],
    ] as const) {
      await userEvent.click(screen.getByRole("button", { name: view }));
      expect(assertion()).toBeInTheDocument();
    }
    // And back: the trajectory trigger names the view it opens.
    await userEvent.click(screen.getByRole("button", { name: /^Trajectory/ }));
    expect(within(panel).getByPlaceholderText("Search events")).toBeInTheDocument();
  });

  it("closes on Escape, on the scrim, and on the close button", async () => {
    const panel = await openTrajectory();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    await userEvent.click(screen.getByRole("button", { name: /^Trajectory/ }));
    const reopened = await screen.findByRole("dialog", { name: "DeepSeek Harness trajectory" });
    const scrim = reopened.parentElement?.querySelector(".dsh-trajectory-scrim") as HTMLElement;
    await userEvent.click(scrim);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    await userEvent.click(screen.getByRole("button", { name: /^Trajectory/ }));
    const again = await screen.findByRole("dialog", { name: "DeepSeek Harness trajectory" });
    await userEvent.click(within(again).getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(panel).not.toBeInTheDocument();
  });
});

describe("DSH trajectory ledger", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockHistory(turnEvents());
  });

  it("draws one tagged row per operation, folding a result into its call", async () => {
    const panel = await openTrajectory();
    const rows = ledgerRows(panel);
    // Six events, five rows: the result settles the call it belongs to.
    expect(rows.map((row) => row.dataset.tag)).toEqual([
      "TURN",
      "USER",
      "STEP",
      "ASSISTANT",
      "TOOL",
    ]);
    // The chip carries the tag as data, which is what the colour keys off.
    expect(
      rows.map((row) => row.querySelector(".dsh-ledger-tag")?.getAttribute("data-tag")),
    ).toEqual(["TURN", "USER", "STEP", "ASSISTANT", "TOOL"]);
    expect(rows.map((row) => row.querySelector(".dsh-ledger-tag")?.textContent)).toEqual([
      "TURN",
      "USER",
      "STEP",
      "ASSISTANT",
      "TOOL",
    ]);
    // A call is drawn under the reply that ordered it, and never deeper.
    expect(rows.map((row) => row.dataset.depth)).toEqual(["0", "1", "1", "1", "2"]);
  });

  it("offers the Duration, Turns and Calls controls above the ledger", async () => {
    const panel = await openTrajectory();
    for (const name of [/Duration/, /Turns/, /Calls/]) {
      expect(within(panel).getByRole("button", { name })).toBeInTheDocument();
    }
  });

  it("folds every reply's calls and every turn from those controls", async () => {
    const panel = await openTrajectory();
    expect(ledgerRows(panel)).toHaveLength(5);

    const calls = within(panel).getByRole("button", { name: /Calls/ });
    await userEvent.click(calls);
    expect(ledgerRows(panel)).toHaveLength(4);
    expect(within(panel).queryByRole("button", { name: /Tool: bash/ })).not.toBeInTheDocument();
    await userEvent.click(calls);
    expect(ledgerRows(panel)).toHaveLength(5);

    await userEvent.click(within(panel).getByRole("button", { name: /Turns/ }));
    expect(ledgerRows(panel)).toHaveLength(0);
    // The turn keeps its heading, so a folded session is still navigable.
    const heading = within(panel).getByRole("button", { name: /Turn 1/ });
    expect(heading).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(heading);
    expect(ledgerRows(panel)).toHaveLength(5);
  });

  it("locates a row the overview points at, unfolding whatever hid it", async () => {
    const panel = await openTrajectory();
    await userEvent.click(within(panel).getByRole("button", { name: /Calls/ }));
    expect(within(panel).queryByRole("button", { name: /Tool: bash/ })).not.toBeInTheDocument();

    const track = within(panel).getByLabelText(/Timeline overview/);
    measureTrack(track);
    // The last slot of the equal-width domain is the call.
    clickTrack(track, 0.9);
    const row = within(panel).getByRole("button", { name: /Tool: bash/ });
    expect(row).toHaveAttribute("aria-pressed", "true");
    expect(within(detailColumn(panel)).getByText("#5")).toBeInTheDocument();
  });
});

describe("DSH trajectory detail column", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockHistory(turnEvents());
  });

  it("reads a completed call across every tab the row can fill", async () => {
    const panel = await openTrajectory();
    await userEvent.click(within(panel).getByRole("button", { name: /Tool: bash/ }));
    const details = detailColumn(panel);
    const tabs = detailTabs(panel);
    expect(
      within(tabs)
        .getAllByRole("button")
        .map((button) => button.textContent),
    ).toEqual(["Summary", "Payload", "Result", "Schema", "Timing"]);

    // Summary: what the row is and how it ended.
    expect(within(details).getByText("Completed")).toBeInTheDocument();
    expect(within(details).getByText("T1 / S1")).toBeInTheDocument();
    expect(within(details).getByText("c1")).toBeInTheDocument();

    await userEvent.click(within(tabs).getByRole("button", { name: "Payload" }));
    expect(within(details).getByText(/"command": "ls -1"/)).toBeInTheDocument();

    await userEvent.click(within(tabs).getByRole("button", { name: "Result" }));
    expect(within(details).getByText(/README\.md/)).toBeInTheDocument();

    await userEvent.click(within(tabs).getByRole("button", { name: "Timing" }));
    expect(within(details).getByText("3,728 ms")).toBeInTheDocument();
    expect(within(details).getByText("Session timestamps")).toBeInTheDocument();
  });

  it("walks up the hierarchy to the reply that ordered the call", async () => {
    const panel = await openTrajectory();
    await userEvent.click(within(panel).getByRole("button", { name: /Tool: bash/ }));
    const details = detailColumn(panel);
    const crumbs = details.querySelector(".dsh-detail-crumbs") as HTMLElement;
    expect(
      within(crumbs)
        .getAllByRole("button")
        .map((button) => button.textContent),
    ).toEqual(["turn/start", "Assistant message"]);

    await userEvent.click(within(crumbs).getByRole("button", { name: "Assistant message" }));
    expect(within(detailColumn(panel)).getByText("#4")).toBeInTheDocument();
  });

  it("omits Payload and Result for a row that carries neither", async () => {
    const panel = await openTrajectory();
    await userEvent.click(within(panel).getByRole("button", { name: /step\/start/ }));
    const tabs = detailTabs(panel);
    expect(
      within(tabs)
        .getAllByRole("button")
        .map((button) => button.textContent),
    ).toEqual(["Summary", "Schema", "Timing"]);

    await userEvent.click(within(tabs).getByRole("button", { name: "Schema" }));
    expect(within(detailColumn(panel)).getByText("Schema unavailable")).toBeInTheDocument();
  });

  it("resolves a call's schema from the request header that declared the tool", async () => {
    const panel = await openTrajectory(schemaEvents());
    await userEvent.click(within(panel).getByRole("button", { name: /Tool: bash/ }));
    await userEvent.click(within(detailTabs(panel)).getByRole("button", { name: "Schema" }));
    const details = detailColumn(panel);
    expect(within(details).getByRole("heading", { name: "bash" })).toBeInTheDocument();
    expect(within(details).getByText("Run a bash command in the workspace.")).toBeInTheDocument();
    expect(within(details).getByText(/"The bash command to run\."/)).toBeInTheDocument();
  });

  it("reports a call with no result yet as running and unmeasured", async () => {
    const panel = await openTrajectory(turnEvents().slice(0, 5));
    await userEvent.click(within(panel).getByRole("button", { name: /Tool: bash/ }));
    expect(within(detailColumn(panel)).getByText("Running")).toBeInTheDocument();

    await userEvent.click(within(detailTabs(panel)).getByRole("button", { name: "Timing" }));
    const details = detailColumn(panel);
    expect(within(details).getByText("Not measured")).toBeInTheDocument();
    expect(within(details).getByText("Session timestamps (running)")).toBeInTheDocument();
  });

  it("closes on its own without closing the panel", async () => {
    const panel = await openTrajectory();
    await userEvent.click(within(panel).getByRole("button", { name: /Tool: bash/ }));
    const details = detailColumn(panel);
    await userEvent.click(within(details).getByRole("button", { name: "Close" }));
    expect(
      within(panel).queryByRole("complementary", { name: "Row detail" }),
    ).not.toBeInTheDocument();
    expect(panel).toBeInTheDocument();
  });
});
