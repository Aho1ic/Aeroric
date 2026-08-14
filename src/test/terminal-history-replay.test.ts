import { describe, expect, it, vi } from "vitest";
import {
  TERMINAL_FRAME_WRITE_BUDGET,
  createSmartWriter,
  splitTerminalWriteChunk,
} from "../components/terminalShared";
import type { Terminal } from "@xterm/xterm";

function fakeTerminal() {
  const writes: string[] = [];
  const term = {
    write: vi.fn((data: string, callback?: () => void) => {
      writes.push(data);
      callback?.();
    }),
    buffer: { active: { cursorY: 0 } },
    refresh: vi.fn(),
  };
  return { term: term as unknown as Terminal, writes };
}

describe("terminal history replay", () => {
  // 逐帧写入是给实时输出留渲染时间的；用在恢复历史上会变成用户肉眼可见的
  // "从中间一路滚到底部"动画（8 MB 缓冲 / 32 KB 每帧 ≈ 250 帧）。
  const history = "x".repeat(TERMINAL_FRAME_WRITE_BUDGET * 4);

  it("flushes the whole history in one batch instead of one frame budget at a time", () => {
    const { term, writes } = fakeTerminal();
    const writer = createSmartWriter(term, () => "dark");
    const done = vi.fn();

    writer.writeImmediate(history, done);

    expect(writes.join("")).toBe(history);
    expect(writes).toHaveLength(splitTerminalWriteChunk(history).length);
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("still throttles live output to the per-frame budget", () => {
    const { term, writes } = fakeTerminal();
    const writer = createSmartWriter(term, () => "dark");

    writer.write(history);

    // 同步阶段只写到帧预算为止，剩余部分留给后续帧。
    const flushed = writes.join("").length;
    expect(flushed).toBeGreaterThan(0);
    expect(flushed).toBeLessThan(history.length);
  });

  it("invokes the callback immediately for an empty replay", () => {
    const { term, writes } = fakeTerminal();
    const writer = createSmartWriter(term, () => "dark");
    const done = vi.fn();

    writer.writeImmediate("", done);

    expect(writes).toHaveLength(0);
    expect(done).toHaveBeenCalledTimes(1);
  });
});
