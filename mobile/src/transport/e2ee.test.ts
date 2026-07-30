import { describe, expect, it } from "vitest";
import {
  base64UrlToBytes,
  bytesToBase64Url,
  KIND_CTRL,
  KIND_TERMINAL,
  startHandshake,
  testGenerateServerKeys,
  testRespondHandshake,
} from "./e2ee";

function utf8(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function establish() {
  const server = testGenerateServerKeys();
  const client = startHandshake(server.publicB64);
  const { ackJson, session: serverSession } = testRespondHandshake(server, client.helloJson);
  const clientSession = client.finish(ackJson);
  return { clientSession, serverSession };
}

describe("e2ee", () => {
  it("base64url roundtrips without padding", () => {
    const bytes = new Uint8Array([0, 1, 250, 251, 252, 253, 254, 255, 62, 63]);
    const encoded = bytesToBase64Url(bytes);
    expect(encoded).not.toMatch(/[+/=]/);
    expect(Array.from(base64UrlToBytes(encoded))).toEqual(Array.from(bytes));
  });

  it("handshake establishes matching sessions in both directions", () => {
    const { clientSession, serverSession } = establish();

    const c2s = clientSession.encryptFrame(KIND_CTRL, utf8('{"v":2,"id":1}'));
    const opened = serverSession.decryptFrame(c2s);
    expect(opened.kind).toBe(KIND_CTRL);
    expect(new TextDecoder().decode(opened.plain)).toBe('{"v":2,"id":1}');

    const s2c = serverSession.encryptFrame(KIND_TERMINAL, new Uint8Array([9, 8, 7]));
    const openedBack = clientSession.decryptFrame(s2c);
    expect(openedBack.kind).toBe(KIND_TERMINAL);
    expect(Array.from(openedBack.plain)).toEqual([9, 8, 7]);
  });

  it("seq increases and replay is rejected", () => {
    const { clientSession, serverSession } = establish();
    const first = clientSession.encryptFrame(KIND_CTRL, utf8("one"));
    const second = clientSession.encryptFrame(KIND_CTRL, utf8("two"));
    serverSession.decryptFrame(first);
    expect(() => serverSession.decryptFrame(first)).toThrow(/out-of-order/);
    // 重放被拒后合法顺序帧仍可解
    expect(new TextDecoder().decode(serverSession.decryptFrame(second).plain)).toBe("two");
    // 跳帧(丢中间帧)被拒:严格递增
    clientSession.encryptFrame(KIND_CTRL, utf8("dropped"));
    const fourth = clientSession.encryptFrame(KIND_CTRL, utf8("four"));
    expect(() => serverSession.decryptFrame(fourth)).toThrow(/out-of-order/);
  });

  it("tampered payload or kind fails authentication", () => {
    const { clientSession, serverSession } = establish();
    const frame = clientSession.encryptFrame(KIND_CTRL, utf8("payload"));
    const tampered = frame.slice();
    tampered[tampered.length - 1] ^= 1;
    expect(() => serverSession.decryptFrame(tampered)).toThrow();

    const kindFlipped = clientSession.encryptFrame(KIND_CTRL, utf8("payload"));
    kindFlipped[0] = KIND_TERMINAL;
    expect(() => serverSession.decryptFrame(kindFlipped)).toThrow();
  });

  it("rejects an imposter host that does not own the pinned key", () => {
    const real = testGenerateServerKeys();
    const imposter = testGenerateServerKeys();
    const client = startHandshake(real.publicB64);
    const { ackJson } = testRespondHandshake(imposter, client.helloJson);
    expect(() => client.finish(ackJson)).toThrow(/主机身份验证失败/);
  });

  it("surfaces hello_error from the server", () => {
    const server = testGenerateServerKeys();
    const client = startHandshake(server.publicB64);
    expect(() =>
      client.finish(JSON.stringify({ v: 2, type: "hello_error", error: "Unsupported protocol" })),
    ).toThrow("Unsupported protocol");
  });
});
