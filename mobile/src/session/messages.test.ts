import { describe, expect, it } from "vitest";
import type { SessionMessage } from "../types";
import { mergeAppended, messageText } from "./messages";

const user = (text: string): SessionMessage => ({
  role: "user",
  content: [{ type: "text", text }],
});
const assistant = (text: string): SessionMessage => ({
  role: "assistant",
  content: [{ type: "text", text }],
});

describe("mergeAppended", () => {
  it("appends messages in order", () => {
    const merged = mergeAppended([user("hi")], [assistant("hello"), user("again")], false);
    expect(merged).toHaveLength(3);
    expect(merged[1].role).toBe("assistant");
    expect(merged[2].role).toBe("user");
  });

  it("merges adjacent assistant batches in codex mode", () => {
    const merged = mergeAppended([assistant("part 1")], [assistant("part 2")], true);
    expect(merged).toHaveLength(1);
    expect(merged[0].content).toHaveLength(2);
    expect(messageText(merged[0])).toBe("part 1\npart 2");
  });

  it("keeps adjacent assistant messages separate in claude mode", () => {
    const merged = mergeAppended([assistant("turn 1")], [assistant("turn 2")], false);
    expect(merged).toHaveLength(2);
  });

  it("does not merge across a user message", () => {
    const merged = mergeAppended([assistant("a")], [user("q"), assistant("b")], true);
    expect(merged).toHaveLength(3);
  });

  it("does not mutate the existing array", () => {
    const existing = [assistant("a")];
    const merged = mergeAppended(existing, [assistant("b")], true);
    expect(existing).toHaveLength(1);
    expect(existing[0].content).toHaveLength(1);
    expect(merged).not.toBe(existing);
  });

  it("drops malformed entries defensively", () => {
    const bad = [
      { role: "system", content: [{ type: "text", text: "x" }] },
      { role: "assistant", content: [] },
      { role: "assistant" },
    ] as unknown as SessionMessage[];
    expect(mergeAppended([], bad, true)).toHaveLength(0);
  });

  it("returns existing array unchanged for empty batch", () => {
    const existing = [user("hi")];
    expect(mergeAppended(existing, [], true)).toBe(existing);
  });
});

describe("messageText", () => {
  it("joins text parts and ignores tool_use/thinking", () => {
    const msg: SessionMessage = {
      role: "assistant",
      content: [
        { type: "thinking", thinking: "hmm" },
        { type: "text", text: "one" },
        { type: "tool_use", id: "t", name: "Bash", input: "{}" },
        { type: "text", text: "two" },
      ],
    };
    expect(messageText(msg)).toBe("one\ntwo");
  });
});
