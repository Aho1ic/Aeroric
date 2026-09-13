import { describe, expect, it } from "vitest";
import {
  DEFAULT_HANDOFF_COMPLETION_RESERVE_TOKENS,
  DEFAULT_HANDOFF_CONTEXT_WINDOW_TOKENS,
  estimateHandoffTokens,
  formatSessionHandoff,
  hasStructuredSessionTranscript,
  originalTaskPrompt,
} from "../sessionHandoffPrompt";

const task = { prompt: "Fix the pairing flow" };

describe("session handoff prompt", () => {
  it("uses structured messages as the only context source when available", () => {
    const result = formatSessionHandoff(
      task,
      "Claude Code",
      [
        { role: "user", content: [{ type: "text", text: "Inspect the listener" }] },
        {
          role: "assistant",
          content: [
            { type: "thinking", thinking: "Check the bind scope" },
            { type: "tool_use", id: "call-1", name: "shell", input: "cargo test" },
            { type: "tool_result", id: "call-1", output: "ok" },
          ],
        },
      ],
      "spinner\nesc to interrupt\nnoisy terminal redraw",
    );

    expect(result).toContain("USER:\nInspect the listener");
    expect(result).toContain("[thinking]\nCheck the bind scope");
    expect(result).toContain("[tool shell (call-1)]\ncargo test");
    expect(result).toContain("[tool result (call-1)]\nok");
    expect(result).not.toContain("spinner");
    expect(result).not.toContain("noisy terminal redraw");
  });

  it("falls back to sanitized terminal history when the transcript is empty", () => {
    const result = formatSessionHandoff(
      task,
      "Codex",
      [],
      "Working\nesc to interrupt\nBuild passed",
    );

    expect(result).toContain("Previous structured conversation: unavailable");
    expect(result).toContain("Previous terminal fallback");
    expect(result).toContain("Build passed");
    expect(result).not.toContain("Working");
  });

  it("treats empty structured messages as unavailable for terminal fallback", () => {
    const messages = [{ role: "assistant" as const, content: [] }];
    const result = formatSessionHandoff(task, "Claude Code", messages, "Build passed");

    expect(hasStructuredSessionTranscript(messages)).toBe(false);
    expect(result).toContain("Previous terminal fallback");
    expect(result).toContain("Build passed");
  });

  it("treats control-only structured messages as unavailable", () => {
    const messages = [
      { role: "assistant" as const, content: [{ type: "text" as const, text: "\u001b[2K\r" }] },
    ];

    expect(hasStructuredSessionTranscript(messages)).toBe(false);
    expect(formatSessionHandoff(task, "Codex", messages, "Build passed")).toContain("Build passed");
  });

  it("keeps DSH structured events in transcript order", () => {
    const result = formatSessionHandoff(
      task,
      "DeepSeek Harness",
      [
        { role: "user", content: [{ type: "text", text: "Inspect DSH" }] },
        {
          role: "assistant",
          content: [
            { type: "thinking", thinking: "Read JSONL" },
            { type: "tool_use", id: "call-1", name: "bash", input: "pwd" },
          ],
        },
        { role: "user", content: [{ type: "tool_result", id: "call-1", output: "/repo" }] },
        { role: "assistant", content: [{ type: "text", text: "Ready" }] },
      ],
      "terminal fallback must stay hidden",
    );

    const orderedParts = [
      "USER:\nInspect DSH",
      "[thinking]\nRead JSONL",
      "[tool bash (call-1)]\npwd",
      "[tool result (call-1)]\n/repo",
      "ASSISTANT:\nReady",
    ];
    const positions = orderedParts.map((part) => result.indexOf(part));

    expect(result).toContain("started with DeepSeek Harness");
    expect(positions.every((position) => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((left, right) => left - right));
    expect(result).not.toContain("terminal fallback must stay hidden");
  });

  it("unwraps repeated handoff prompts to preserve the original task", () => {
    const once = formatSessionHandoff(task, "Claude Code", [], "Build passed");
    const twice = formatSessionHandoff({ prompt: once }, "Codex", [], "More output");

    expect(originalTaskPrompt(twice)).toBe("Fix the pairing flow");
    expect(twice.match(/\[Aeroric context handoff\]/g)).toHaveLength(1);
    expect(twice).toContain("Original task:\nFix the pairing flow");
    expect(twice).not.toContain("Original task:\n[Aeroric context handoff]");
  });

  it("estimates wide characters as one token each and narrow as two per token", () => {
    expect(estimateHandoffTokens("a".repeat(100))).toBe(50);
    expect(estimateHandoffTokens("中".repeat(100))).toBe(100);
    expect(estimateHandoffTokens("")).toBe(0);
  });

  it("budgets an oversized transcript to head + tail with an omission marker", () => {
    // 60 条 × 4000 个 ASCII 字符 ≈ 每条约 2000 token,总计约 12 万,远超默认预算
    // (128K 窗口 - 64K completion - 16K 余量 = 48K 转录预算)。
    const messages = Array.from({ length: 60 }, (_, index) => ({
      role: "user" as const,
      content: [{ type: "text" as const, text: `m${index}-unique ${"a".repeat(4000)}` }],
    }));

    const result = formatSessionHandoff(task, "sota", messages, "no terminal");

    expect(result).toContain("characters of the structured conversation omitted");
    expect(result).toContain("m0-unique");
    expect(result).toContain("m59-unique");
    expect(result).not.toContain("m30-unique");
    // 估算函数对产物复算:整个交接(去掉原任务等固定段)应在转录预算量级内,
    // 这里只验证没有把整段原样塞回去。
    expect(estimateHandoffTokens(result)).toBeLessThan(60_000);
  });

  it("keeps the full transcript when the target window is large enough", () => {
    const messages = Array.from({ length: 60 }, (_, index) => ({
      role: "user" as const,
      content: [{ type: "text" as const, text: `m${index}-unique ${"a".repeat(4000)}` }],
    }));

    const result = formatSessionHandoff(task, "sota", messages, "no terminal", {
      contextWindowTokens: 262_144,
      completionReserveTokens: 64_000,
    });

    expect(result).not.toContain("characters of the structured conversation omitted");
    expect(result).toContain("m0-unique");
    expect(result).toContain("m30-unique");
    expect(result).toContain("m59-unique");
  });

  it("keeps the tail of a single message that alone exceeds the budget", () => {
    const huge = `${"a".repeat(199_000)} tail-marker-after-the-crash`;
    const messages = [{ role: "user" as const, content: [{ type: "text" as const, text: huge }] }];

    const result = formatSessionHandoff(task, "sota", messages, "");

    expect(result).toContain("tail-marker-after-the-crash");
    expect(result).not.toContain(huge);
    expect(estimateHandoffTokens(result)).toBeLessThan(
      DEFAULT_HANDOFF_CONTEXT_WINDOW_TOKENS - DEFAULT_HANDOFF_COMPLETION_RESERVE_TOKENS,
    );
  });

  it("budgets the terminal fallback too, not just the structured transcript", () => {
    const terminalHistory = `progress ${"终".repeat(70_000)} last-line-marker`;

    const result = formatSessionHandoff(task, "sota", [], terminalHistory);

    expect(result).toContain("terminal history omitted to fit the context window");
    expect(result).toContain("last-line-marker");
  });
});
