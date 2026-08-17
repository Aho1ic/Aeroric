import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import {
  dshBasename,
  dshMentionVocabulary,
  linkDshProducedMentions,
  resolveDshProducedMention,
  segmentDshProse,
} from "../dshDeliverables";
import type { DshProducedFile } from "../dshSessionFeatures";
import { DshSessionInsights } from "../components/DshSessionInsights";
import { SessionView } from "../components/SessionView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

function produced(...entries: Array<[string, number]>): DshProducedFile[] {
  return entries.map(([path, seq]) => ({ path, seq }));
}

/** The plain text of a segment list, mentions included as their token. */
function flatten(prose: string, paths: readonly string[]): string {
  return segmentDshProse(prose, paths)
    .map((segment) => (segment.kind === "text" ? segment.text : segment.token))
    .join("");
}

describe("dshBasename", () => {
  it("takes the trailing segment of either separator", () => {
    expect(dshBasename("src/components/App.tsx")).toBe("App.tsx");
    expect(dshBasename("src\\lib\\main.rs")).toBe("main.rs");
    expect(dshBasename("README.md")).toBe("README.md");
  });
});

describe("dshMentionVocabulary", () => {
  it("keeps first-seen order and drops files produced after the message", () => {
    const files = produced(["src/a.ts", 4], ["src/b.ts", 9], ["src/c.ts", 12]);
    expect(dshMentionVocabulary(files, 9)).toEqual(["src/a.ts", "src/b.ts"]);
    expect(dshMentionVocabulary(files)).toEqual(["src/a.ts", "src/b.ts", "src/c.ts"]);
  });

  it("lists a path once even when it was produced twice", () => {
    expect(dshMentionVocabulary(produced(["src/a.ts", 4], ["src/a.ts", 8]))).toEqual(["src/a.ts"]);
  });
});

describe("resolveDshProducedMention", () => {
  const paths = ["out/report.html", "src/lib.rs"];

  it("resolves an exact path and a unique basename", () => {
    expect(resolveDshProducedMention(paths, "out/report.html")).toBe("out/report.html");
    expect(resolveDshProducedMention(paths, "lib.rs")).toBe("src/lib.rs");
  });

  it("leaves a shared basename inert rather than guessing", () => {
    const shared = ["a/index.html", "b/index.html"];
    expect(resolveDshProducedMention(shared, "index.html")).toBeUndefined();
    expect(resolveDshProducedMention(shared, "a/index.html")).toBe("a/index.html");
  });

  it("refuses a partial suffix and an unknown token", () => {
    expect(resolveDshProducedMention(["deep/out/index.html"], "out/index.html")).toBeUndefined();
    expect(resolveDshProducedMention(paths, "pnpm build")).toBeUndefined();
  });
});

describe("segmentDshProse", () => {
  const paths = ["src/lib.rs", "out/report.html"];

  it("links a recognized token and keeps the prose around it", () => {
    expect(segmentDshProse("Wrote `lib.rs` for you.", paths)).toEqual([
      { kind: "text", text: "Wrote " },
      { kind: "mention", token: "lib.rs", path: "src/lib.rs" },
      { kind: "text", text: " for you." },
    ]);
  });

  it("links every recognized token on the line", () => {
    const segments = segmentDshProse("`src/lib.rs` and `out/report.html`", paths);
    expect(segments.filter((segment) => segment.kind === "mention")).toHaveLength(2);
  });

  it("leaves an unrecognized token exactly as written", () => {
    const prose = "Run `pnpm build` first.";
    expect(segmentDshProse(prose, paths)).toEqual([{ kind: "text", text: prose }]);
  });

  it("closes a double-backtick span on its own fence", () => {
    expect(segmentDshProse("see ``lib.rs``", paths)).toEqual([
      { kind: "text", text: "see " },
      { kind: "mention", token: "lib.rs", path: "src/lib.rs" },
    ]);
  });

  it("keeps an unmatched opening run literal", () => {
    // The two-backtick run finds no two-backtick closer, so nothing is a span —
    // consuming the whole run is what stops the trailing tick from opening one.
    const prose = "``lib.rs`";
    expect(segmentDshProse(prose, paths)).toEqual([{ kind: "text", text: prose }]);
  });

  it("never lets a span cross a line break", () => {
    const fence = "```\nsrc/lib.rs\n```";
    expect(segmentDshProse(fence, paths)).toEqual([{ kind: "text", text: fence }]);
  });

  it("drops the padding spaces a token was written with", () => {
    expect(segmentDshProse("` lib.rs `", paths)).toEqual([
      { kind: "mention", token: "lib.rs", path: "src/lib.rs" },
    ]);
  });

  it("returns the prose unchanged when the session produced nothing", () => {
    const prose = "Wrote `lib.rs` for you.";
    expect(segmentDshProse(prose, [])).toEqual([{ kind: "text", text: prose }]);
    expect(flatten(prose, paths)).toBe(prose.replace(/`/g, ""));
  });
});

/** A history page whose turn writes `src/lib.rs` and then names it in prose. */
function history(text: string, locations: Array<{ path: string }>) {
  return {
    events: [
      {
        event: {
          type: "tool/result",
          seq: 11,
          time: 1_700_000_000_000,
          data: { turn: 1, meta: { locations } },
        },
      },
      {
        event: {
          type: "assistant/message",
          seq: 12,
          time: 1_700_000_001_000,
          data: { turn: 1, content: [{ type: "text", text }] },
        },
      },
    ],
    hasMore: false,
    projections: { values: {} },
  };
}

async function openTrajectory(page: ReturnType<typeof history>) {
  invokeMock.mockImplementation((command) =>
    command === "get_dsh_session_history" ? Promise.resolve(page) : Promise.resolve(null),
  );
  render(
    <I18nProvider>
      <DshSessionInsights sessionId="session-1" />
    </I18nProvider>,
  );
  await userEvent.click(screen.getByRole("button", { name: /Session details/ }));
}

describe("DSH trajectory produced-file references", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("opens the produced file a closing message names by basename", async () => {
    await openTrajectory(history("Updated `lib.rs` with the fix.", [{ path: "src/lib.rs" }]));
    const mention = await screen.findByRole("button", { name: "Open src/lib.rs" });
    // The chip stays short while the full path rides the title, as the produced
    // files panel does for the same reason.
    expect(mention).toHaveTextContent("lib.rs");
    expect(mention).toHaveAttribute("title", "src/lib.rs");
    await userEvent.click(mention);
    expect(invokeMock).toHaveBeenCalledWith("open_dsh_host_path", { path: "src/lib.rs" });
  });

  it("leaves a token no produced file answers as plain prose", async () => {
    await openTrajectory(history("Ran `pnpm build` after the edit.", [{ path: "src/lib.rs" }]));
    expect(await screen.findByText(/Ran `pnpm build` after the edit\./)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Open / })).not.toBeInTheDocument();
  });

  it("keeps a shared basename inert so a reference cannot open the wrong file", async () => {
    await openTrajectory(
      history("Both `index.html` files changed.", [
        { path: "a/index.html" },
        { path: "b/index.html" },
      ]),
    );
    expect(await screen.findByText(/Both `index.html` files changed\./)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Open / })).not.toBeInTheDocument();
  });
});

describe("linkDshProducedMentions", () => {
  const label = (path: string) => `Open ${path}`;

  it("leaves the markup untouched when nothing resolves", () => {
    const html = "<p>Ran <code>pnpm build</code>.</p>";
    expect(linkDshProducedMentions(html, ["src/lib.rs"], label)).toBe(html);
    expect(linkDshProducedMentions("<p><code>lib.rs</code></p>", [], label)).toBe(
      "<p><code>lib.rs</code></p>",
    );
  });

  it("wraps a resolved reference in an opener that carries the full path", () => {
    const linked = linkDshProducedMentions(
      "<p>See <code>lib.rs</code>.</p>",
      ["src/lib.rs"],
      label,
    );
    const host = document.createElement("div");
    host.innerHTML = linked;
    const button = host.querySelector("code button");
    expect(button?.textContent).toBe("lib.rs");
    expect(button?.getAttribute("data-dsh-file")).toBe("src/lib.rs");
    expect(button?.getAttribute("aria-label")).toBe("Open src/lib.rs");
  });

  it("skips a listing inside a fenced block", () => {
    const html = "<pre><code>src/lib.rs\n</code></pre>";
    expect(linkDshProducedMentions(html, ["src/lib.rs"], label)).toBe(html);
  });

  it("skips code inside a link, where a button cannot nest", () => {
    const html = '<p><a href="https://example.com"><code>lib.rs</code></a></p>';
    expect(linkDshProducedMentions(html, ["src/lib.rs"], label)).toBe(html);
  });

  it("puts a path carrying markup into the button as text, never as markup", () => {
    const linked = linkDshProducedMentions(
      "<p><code>&lt;img src=x&gt;.ts</code></p>",
      ["evil/<img src=x>.ts"],
      label,
    );
    const host = document.createElement("div");
    host.innerHTML = linked;
    expect(host.querySelector("img")).toBeNull();
    expect(host.querySelector("button")?.getAttribute("data-dsh-file")).toBe("evil/<img src=x>.ts");
  });
});

/** A transcript page whose one assistant message is the given Markdown. */
function transcript(text: string) {
  return {
    messages: [{ role: "assistant" as const, messageId: "m1", content: [{ type: "text", text }] }],
    nextCursor: null,
    hasMore: false,
  };
}

async function renderTranscript(text: string, locations: Array<{ path: string }>) {
  invokeMock.mockImplementation((command) => {
    if (command === "read_session_message_page") return Promise.resolve(transcript(text));
    if (command === "get_dsh_session_history") return Promise.resolve(history(text, locations));
    return Promise.resolve(null);
  });
  render(
    <I18nProvider>
      <SessionView
        sessionPath="/tmp/dsh/session.jsonl"
        projectPath="/tmp/project"
        isCodex={false}
        family="dsh"
        sessionId="session-1"
      />
    </I18nProvider>,
  );
}

describe("SessionView produced-file references", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    invokeMock.mockReset();
  });

  it("opens the produced file the transcript names", async () => {
    await renderTranscript("Updated `lib.rs` with the fix.", [{ path: "src/lib.rs" }]);
    const mention = await screen.findByRole("button", { name: "Open src/lib.rs" });
    expect(mention).toHaveTextContent("lib.rs");
    await userEvent.click(mention);
    expect(invokeMock).toHaveBeenCalledWith("open_dsh_host_path", { path: "src/lib.rs" });
  });

  it("leaves a command token as the inert code the Markdown made it", async () => {
    await renderTranscript("Ran `pnpm build` after the edit.", [{ path: "src/lib.rs" }]);
    expect(await screen.findByText("pnpm build")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Open / })).not.toBeInTheDocument();
  });

  it("links nothing for a non-DSH session, which has no vocabulary to read", async () => {
    invokeMock.mockImplementation((command) =>
      command === "read_session_message_page"
        ? Promise.resolve(transcript("Updated `lib.rs`."))
        : Promise.resolve(null),
    );
    render(
      <I18nProvider>
        <SessionView
          sessionPath="/tmp/claude/session.jsonl"
          projectPath="/tmp/project"
          isCodex={false}
          family="claude"
        />
      </I18nProvider>,
    );
    expect(await screen.findByText("lib.rs")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Open / })).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("get_dsh_session_history", expect.anything());
  });
});
