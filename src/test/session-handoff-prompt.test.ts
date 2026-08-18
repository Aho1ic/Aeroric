import { describe, expect, it } from "vitest";
import {
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
});
