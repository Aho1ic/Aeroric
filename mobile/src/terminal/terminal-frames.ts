/**
 * 终端流二进制帧编解码(TS 侧),与桌面端
 * src-tauri/src/remote/terminal_frames.rs 严格对齐:
 * 16 字节头(kind/version/opcode/pad + u32 streamId LE + u64 seq LE)+ payload。
 * seq 以 Number 表示(< 2^53,按每帧一条计数远不会溢出)。
 */

export const TERMINAL_FRAME_KIND = 0x74;
export const TERMINAL_FRAME_VERSION = 1;
export const HEADER_BYTES = 16;

export enum TerminalOpcode {
  Output = 1,
  SnapshotStart = 2,
  SnapshotChunk = 3,
  SnapshotEnd = 4,
  Resized = 5,
  Error = 6,
  Input = 7,
  Resize = 8,
  Subscribe = 9,
  Unsubscribe = 10,
}

const KNOWN_OPCODES = new Set<number>([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

export interface TerminalFrame {
  opcode: TerminalOpcode;
  streamId: number;
  seq: number;
  payload: Uint8Array;
}

export function encodeTerminalFrame(frame: TerminalFrame): Uint8Array {
  const out = new Uint8Array(HEADER_BYTES + frame.payload.length);
  const view = new DataView(out.buffer);
  view.setUint8(0, TERMINAL_FRAME_KIND);
  view.setUint8(1, TERMINAL_FRAME_VERSION);
  view.setUint8(2, frame.opcode);
  view.setUint8(3, 0);
  view.setUint32(4, frame.streamId, true);
  const seq = Math.max(0, Math.floor(frame.seq));
  view.setUint32(8, seq >>> 0, true);
  view.setUint32(12, Math.floor(seq / 0x1_0000_0000), true);
  out.set(frame.payload, HEADER_BYTES);
  return out;
}

export function decodeTerminalFrame(bytes: Uint8Array): TerminalFrame | null {
  if (bytes.length < HEADER_BYTES) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint8(0) !== TERMINAL_FRAME_KIND || view.getUint8(1) !== TERMINAL_FRAME_VERSION) {
    return null;
  }
  const opcode = view.getUint8(2);
  if (!KNOWN_OPCODES.has(opcode)) return null;
  const low = view.getUint32(8, true);
  const high = view.getUint32(12, true);
  return {
    opcode,
    streamId: view.getUint32(4, true),
    seq: high * 0x1_0000_0000 + low,
    payload: bytes.slice(HEADER_BYTES),
  };
}

export function textPayload(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

export function payloadText(payload: Uint8Array): string {
  return new TextDecoder().decode(payload);
}

export function jsonPayload(value: unknown): Uint8Array {
  return textPayload(JSON.stringify(value));
}

export function parseJsonPayload<T>(payload: Uint8Array): T | null {
  try {
    return JSON.parse(payloadText(payload)) as T;
  } catch {
    return null;
  }
}
