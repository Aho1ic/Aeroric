import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { I18nProvider } from "../i18n";
import { DshToolCard } from "../components/DshToolCard";
import {
  parseDshToolCallView,
  parseDshToolEventView,
  parseDshToolResultView,
} from "../dshToolViews";

function renderCard(value: unknown) {
  const intent = parseDshToolEventView(value);
  if (intent === undefined) throw new Error("expected a usable render intent");
  return render(
    <I18nProvider>
      <DshToolCard intent={intent} />
    </I18nProvider>,
  );
}

describe("DSH tool render intents", () => {
  it("narrows the three pending-call cards", () => {
    expect(
      parseDshToolCallView({
        card: "terminal",
        title: "pnpm test",
        cwd: "/repo",
        description: "run tests",
      }),
    ).toEqual({ card: "terminal", title: "pnpm test", description: "run tests", cwd: "/repo" });
    expect(
      parseDshToolCallView({
        card: "diff",
        title: "Edit main.ts",
        diffs: [{ path: "src/main.ts", oldText: "a", newText: "b" }],
        locations: [{ path: "src/main.ts", line: 4 }],
      }),
    ).toEqual({
      card: "diff",
      title: "Edit main.ts",
      diffs: [{ path: "src/main.ts", oldText: "a", newText: "b" }],
      locations: [{ path: "src/main.ts", line: 4 }],
    });
    expect(
      parseDshToolCallView({
        card: "generic",
        title: "Grep",
        kind: "search",
        rawInput: { pattern: "x" },
      }),
    ).toEqual({ card: "generic", title: "Grep", kind: "search", rawInput: { pattern: "x" } });
  });

  it("degrades an unusable view so the caller falls back to the raw event", () => {
    // No title, unknown card, a diff card with nothing readable, and an unknown
    // `for` are each unrenderable; the Harness expects the UI to fall back.
    expect(parseDshToolCallView({ card: "terminal" })).toBeUndefined();
    expect(parseDshToolCallView({ card: "sparkline", title: "x" })).toBeUndefined();
    expect(
      parseDshToolCallView({ card: "diff", title: "x", diffs: [{ path: "a" }] }),
    ).toBeUndefined();
    expect(parseDshToolResultView({ card: "search", shape: "histogram" })).toBeUndefined();
    expect(
      parseDshToolResultView({ card: "web", kind: "fetch", url: "https://x" }),
    ).toBeUndefined();
    expect(
      parseDshToolEventView({ for: "stream", view: { card: "generic", title: "x" } }),
    ).toBeUndefined();
    expect(parseDshToolEventView("nope")).toBeUndefined();
  });

  it("reads a terminal result carrying both exitCode and signal as the signal", () => {
    expect(
      parseDshToolResultView({ card: "terminal", output: "boom", exitCode: 1, signal: "SIGKILL" }),
    ).toEqual({ card: "terminal", output: "boom", signal: "SIGKILL" });
  });

  it("defaults search totals to the retained count and read offset to the first line", () => {
    expect(
      parseDshToolResultView({
        card: "search",
        shape: "matches",
        files: [{ path: "a.ts", matches: [{ lineNumber: 2, line: "hit" }, { lineNumber: 9 }] }],
      }),
    ).toEqual({
      card: "search",
      shape: "matches",
      files: [{ path: "a.ts", matches: [{ lineNumber: 2, line: "hit" }] }],
      truncated: false,
      total: 1,
    });
    expect(parseDshToolResultView({ card: "read", path: "a.ts", lines: [] })).toEqual({
      card: "read",
      path: "a.ts",
      offset: 1,
      lines: [],
      totalLines: 0,
    });
  });

  it("renders a terminal result with its output and a failing exit code", () => {
    renderCard({
      for: "result",
      view: { card: "terminal", title: "pnpm lint", output: "2 problems", exitCode: 1 },
    });
    expect(screen.getByText("pnpm lint")).toBeInTheDocument();
    expect(screen.getByText("2 problems")).toBeInTheDocument();
    expect(screen.getByText("exit 1")).toHaveAttribute("data-failed", "true");
  });

  it("renders an edit as added and removed diff rows", () => {
    const { container } = renderCard({
      for: "result",
      view: {
        card: "diff",
        diffs: [{ path: "src/a.ts", oldText: "keep\nold\ntail", newText: "keep\nnew\ntail" }],
      },
    });
    expect(container.querySelectorAll('[data-sign="-"]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-sign="+"]')).toHaveLength(1);
    expect(screen.getByText("-old")).toBeInTheDocument();
    expect(screen.getByText("+new")).toBeInTheDocument();
  });

  it("renders every line of a created file as an addition", () => {
    const { container } = renderCard({
      for: "result",
      view: { card: "diff", diffs: [{ path: "src/new.ts", oldText: null, newText: "one\ntwo" }] },
    });
    expect(container.querySelectorAll('[data-sign="+"]')).toHaveLength(2);
    expect(container.querySelectorAll('[data-sign="-"]')).toHaveLength(0);
    expect(screen.getByText("new file")).toBeInTheDocument();
  });

  it("renders grouped search matches with their file line numbers", () => {
    renderCard({
      for: "result",
      view: {
        card: "search",
        shape: "matches",
        files: [{ path: "src/a.ts", matches: [{ lineNumber: 12, line: "const x = 1;" }] }],
        truncated: false,
        total: 1,
      },
    });
    expect(screen.getByText("src/a.ts")).toBeInTheDocument();
    expect(screen.getByText("12")).toBeInTheDocument();
    expect(screen.getByText("const x = 1;")).toBeInTheDocument();
    expect(screen.getByText("1 matches")).toBeInTheDocument();
  });

  it("renders a read window with the file's own numbering", () => {
    renderCard({
      for: "result",
      view: {
        card: "read",
        path: "src/a.ts",
        offset: 40,
        totalLines: 120,
        lang: "ts",
        lines: [{ number: 40, text: "line forty" }],
      },
    });
    expect(screen.getByText("ts")).toBeInTheDocument();
    expect(screen.getByText("40")).toBeInTheDocument();
    expect(screen.getByText("line forty")).toBeInTheDocument();
    expect(screen.getByText("lines 40+, 1 of 120")).toBeInTheDocument();
  });

  it("renders web search sources as external links", () => {
    renderCard({
      for: "result",
      view: {
        card: "web",
        kind: "search",
        answer: "Yes.",
        truncated: true,
        sources: [{ url: "https://example.com/a", title: "Example A", snippet: "…" }],
      },
    });
    expect(screen.getByText("Yes.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Example A/ })).toHaveAttribute(
      "href",
      "https://example.com/a",
    );
    expect(screen.getByText("1 sources (truncated)")).toBeInTheDocument();
  });

  it("renders a failed web fetch status", () => {
    renderCard({
      for: "result",
      view: { card: "web", kind: "fetch", url: "https://example.com", statusCode: 503 },
    });
    expect(screen.getByText("503")).toHaveAttribute("data-failed", "true");
    expect(screen.getByRole("link")).toHaveAttribute("href", "https://example.com");
  });

  it("renders a pending terminal call with its working directory", () => {
    renderCard({
      for: "call",
      view: { card: "terminal", title: "pnpm build", description: "compile", cwd: "/repo" },
    });
    expect(screen.getByText("pnpm build")).toBeInTheDocument();
    expect(screen.getByText("compile")).toBeInTheDocument();
    expect(screen.getByText("/repo")).toBeInTheDocument();
  });
});
