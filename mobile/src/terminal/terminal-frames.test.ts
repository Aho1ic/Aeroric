import { describe, expect, it } from "vitest";
import {
  decodeTerminalFrame,
  encodeTerminalFrame,
  jsonPayload,
  parseJsonPayload,
  payloadText,
  TerminalOpcode,
  textPayload,
} from "./terminal-frames";

describe("terminal frame codec", () => {
  it("round-trips frames with unicode payload and large seq", () => {
    const frame = {
      opcode: TerminalOpcode.Output,
      streamId: 0xdeadbeef,
      seq: 2 ** 40 + 123,
      payload: textPayload("你好 wörld\x1b[31m"),
    };
    const decoded = decodeTerminalFrame(encodeTerminalFrame(frame));
    expect(decoded).not.toBeNull();
    expect(decoded!.opcode).toBe(TerminalOpcode.Output);
    expect(decoded!.streamId).toBe(0xdeadbeef);
    expect(decoded!.seq).toBe(2 ** 40 + 123);
    expect(payloadText(decoded!.payload)).toBe("你好 wörld\x1b[31m");
  });

  it("matches the rust wire layout byte-for-byte", () => {
    const bytes = encodeTerminalFrame({
      opcode: TerminalOpcode.Subscribe,
      streamId: 7,
      seq: 0,
      payload: textPayload("x"),
    });
    expect(Array.from(bytes.slice(0, 16))).toEqual([
      0x74, 1, 9, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    expect(bytes[16]).toBe("x".charCodeAt(0));
  });

  it("rejects foreign frames", () => {
    expect(decodeTerminalFrame(new Uint8Array(8))).toBeNull();
    const wrongKind = encodeTerminalFrame({
      opcode: TerminalOpcode.Output,
      streamId: 1,
      seq: 1,
      payload: new Uint8Array(0),
    });
    wrongKind[0] = 0x00;
    expect(decodeTerminalFrame(wrongKind)).toBeNull();
    const wrongOpcode = encodeTerminalFrame({
      opcode: TerminalOpcode.Output,
      streamId: 1,
      seq: 1,
      payload: new Uint8Array(0),
    });
    wrongOpcode[2] = 250;
    expect(decodeTerminalFrame(wrongOpcode)).toBeNull();
  });

  it("json payload helpers round-trip", () => {
    const payload = jsonPayload({ taskId: "t1", cols: 92 });
    expect(parseJsonPayload<{ taskId: string; cols: number }>(payload)).toEqual({
      taskId: "t1",
      cols: 92,
    });
    expect(parseJsonPayload(textPayload("not json"))).toBeNull();
  });
});
