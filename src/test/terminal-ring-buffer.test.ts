import { describe, expect, it } from "vitest";
import {
  TERMINAL_BUFFER_MAX_BYTES,
  TERMINAL_BUFFER_MAX_CHUNKS,
  createTerminalRingBuffer,
  joinTerminalBuffer,
  joinTerminalBufferFrom,
  pushTerminalChunk,
  terminalBufferAbsLength,
} from "../terminalRingBuffer";

describe("terminalRingBuffer", () => {
  it("starts empty", () => {
    const buffer = createTerminalRingBuffer();
    expect(buffer.chunks).toEqual([]);
    expect(terminalBufferAbsLength(buffer)).toBe(0);
    expect(joinTerminalBuffer(buffer)).toBe("");
    expect(joinTerminalBufferFrom(buffer, 0)).toBe("");
  });

  it("accumulates chunks and tracks absolute length", () => {
    const buffer = createTerminalRingBuffer();
    pushTerminalChunk(buffer, "abc");
    pushTerminalChunk(buffer, "de");
    expect(joinTerminalBuffer(buffer)).toBe("abcde");
    expect(terminalBufferAbsLength(buffer)).toBe(5);
    expect(buffer.droppedLen).toBe(0);
  });

  it("compacts the chunk array once it exceeds the chunk cap", () => {
    const buffer = createTerminalRingBuffer();
    for (let i = 0; i < TERMINAL_BUFFER_MAX_CHUNKS + 1; i++) {
      pushTerminalChunk(buffer, "x");
    }
    expect(buffer.chunks).toHaveLength(1);
    expect(buffer.chunks[0]).toHaveLength(TERMINAL_BUFFER_MAX_CHUNKS + 1);
    expect(terminalBufferAbsLength(buffer)).toBe(TERMINAL_BUFFER_MAX_CHUNKS + 1);
  });

  it("drops from the head once the byte cap is exceeded", () => {
    const buffer = createTerminalRingBuffer();
    const chunk = "y".repeat(TERMINAL_BUFFER_MAX_BYTES / 2);
    pushTerminalChunk(buffer, chunk);
    pushTerminalChunk(buffer, chunk);
    expect(buffer.droppedLen).toBe(0);
    expect(buffer.totalLen).toBe(TERMINAL_BUFFER_MAX_BYTES);

    pushTerminalChunk(buffer, "z");
    expect(buffer.droppedLen).toBe(chunk.length);
    expect(buffer.totalLen).toBe(chunk.length + 1);
    // 绝对长度必须包含被裁掉的部分，否则外部持有的偏移会整体错位。
    expect(terminalBufferAbsLength(buffer)).toBe(TERMINAL_BUFFER_MAX_BYTES + 1);
    expect(joinTerminalBuffer(buffer)).toBe(`${chunk}z`);
  });

  it("keeps a single oversized chunk rather than emptying the buffer", () => {
    const buffer = createTerminalRingBuffer();
    const oversized = "o".repeat(TERMINAL_BUFFER_MAX_BYTES + 10);
    pushTerminalChunk(buffer, oversized);
    expect(joinTerminalBuffer(buffer)).toBe(oversized);
    expect(buffer.droppedLen).toBe(0);

    // 下一块到达时那块超限内容就会被正常挤出去。
    pushTerminalChunk(buffer, "next");
    expect(buffer.droppedLen).toBe(oversized.length);
    expect(joinTerminalBuffer(buffer)).toBe("next");
    expect(terminalBufferAbsLength(buffer)).toBe(oversized.length + 4);
  });

  describe("joinTerminalBufferFrom", () => {
    it("returns the tail starting at an absolute offset", () => {
      const buffer = createTerminalRingBuffer();
      pushTerminalChunk(buffer, "abc");
      pushTerminalChunk(buffer, "def");
      pushTerminalChunk(buffer, "ghi");
      expect(joinTerminalBufferFrom(buffer, 0)).toBe("abcdefghi");
      expect(joinTerminalBufferFrom(buffer, 2)).toBe("cdefghi");
      expect(joinTerminalBufferFrom(buffer, 3)).toBe("defghi");
      expect(joinTerminalBufferFrom(buffer, 7)).toBe("hi");
    });

    it("returns nothing when the offset is at or past the end", () => {
      const buffer = createTerminalRingBuffer();
      pushTerminalChunk(buffer, "abc");
      expect(joinTerminalBufferFrom(buffer, 3)).toBe("");
      expect(joinTerminalBufferFrom(buffer, 99)).toBe("");
    });

    it("accounts for dropped bytes when resolving the offset", () => {
      const buffer = createTerminalRingBuffer();
      const half = "y".repeat(TERMINAL_BUFFER_MAX_BYTES / 2);
      pushTerminalChunk(buffer, half);
      pushTerminalChunk(buffer, half);
      pushTerminalChunk(buffer, "tail");
      expect(buffer.droppedLen).toBe(half.length);

      // 偏移落在仍留存的区间：正常取尾部。
      const absEnd = terminalBufferAbsLength(buffer);
      expect(joinTerminalBufferFrom(buffer, absEnd - 2)).toBe("il");
      expect(joinTerminalBufferFrom(buffer, half.length)).toBe(`${half}tail`);
    });

    it("falls back to everything retained when the offset was already dropped", () => {
      const buffer = createTerminalRingBuffer();
      const half = "y".repeat(TERMINAL_BUFFER_MAX_BYTES / 2);
      pushTerminalChunk(buffer, half);
      pushTerminalChunk(buffer, half);
      pushTerminalChunk(buffer, "tail");

      // offset 0 已经被裁掉，只能从还留着的位置继续重放。
      expect(joinTerminalBufferFrom(buffer, 0)).toBe(joinTerminalBuffer(buffer));
      expect(joinTerminalBufferFrom(buffer, half.length - 1)).toBe(joinTerminalBuffer(buffer));
    });
  });
});
