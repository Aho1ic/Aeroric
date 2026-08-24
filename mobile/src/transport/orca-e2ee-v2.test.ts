import { describe, expect, it } from "vitest";
import { x25519 } from "@noble/curves/ed25519.js";
import { xsalsa20poly1305 } from "@noble/ciphers/salsa.js";
import { OrcaE2EESession, ORCA_E2EE_PROTOCOL, startOrcaE2EEHandshake } from "./orca-e2ee-v2";

function base64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

const SESSION_ID_BYTES = 32;
const HEADER_BYTES = SESSION_ID_BYTES + 1 + 1 + 8;
const NONCE_BYTES = 24;
/** 桌面端 send 方向(orca_crypto.rs:266 用 desktop_to_client + direction=1)。 */
const DESKTOP_SEND_DIRECTION = 1;

/**
 * 镜像 src-tauri/src/remote/orca_crypto.rs 的 `DirectionCipher::seal`。
 * 手机侧只有对着桌面真实的封帧规则才能证明接收方向没写反 —— 自己 seal 再自己
 * open 会同时用错方向而互相抵消,测不出问题。
 */
function desktopSeal(options: {
  key: Uint8Array;
  sessionId: Uint8Array;
  kind: number;
  counter: bigint;
  payload: Uint8Array;
  direction?: number;
}): Uint8Array {
  const { key, sessionId, kind, counter, payload } = options;
  const direction = options.direction ?? DESKTOP_SEND_DIRECTION;

  const nonce = new Uint8Array(NONCE_BYTES);
  nonce.set(sessionId.slice(0, 12));
  nonce[12] = 2;
  nonce[13] = direction;
  nonce[14] = kind;
  nonce[15] = 0;
  new DataView(nonce.buffer).setBigUint64(16, counter, false);

  const header = new Uint8Array(HEADER_BYTES);
  header.set(sessionId, 0);
  header[SESSION_ID_BYTES] = direction;
  header[SESSION_ID_BYTES + 1] = kind;
  new DataView(header.buffer).setBigUint64(SESSION_ID_BYTES + 2, counter, false);

  const plaintext = new Uint8Array(header.length + payload.length);
  plaintext.set(header, 0);
  plaintext.set(payload, header.length);

  const ciphertext = xsalsa20poly1305(key, nonce).encrypt(plaintext);
  const frame = new Uint8Array(nonce.length + ciphertext.length);
  frame.set(nonce, 0);
  frame.set(ciphertext, nonce.length);
  return frame;
}

function base64url(bytes: Uint8Array): string {
  return base64(bytes).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

describe("Orca E2EE v2 compatibility", () => {
  it("emits the exact hello contract and binds the ready transcript", () => {
    const serverSecret = new Uint8Array(32).fill(3);
    const serverPublic = x25519.getPublicKey(serverSecret);
    let randomCall = 0;
    const pending = startOrcaE2EEHandshake(
      base64(serverPublic),
      {
        protocol: ORCA_E2EE_PROTOCOL,
        initiator: "mobile",
        responder: "desktop",
        transport: "direct",
      },
      (length) => new Uint8Array(length).fill(randomCall++ === 0 ? 7 : 9),
    );
    const hello = JSON.parse(pending.helloJson) as Record<string, unknown>;
    expect(Object.keys(hello).sort()).toEqual([
      "capabilities",
      "clientNonceB64",
      "clientPublicKeyB64",
      "context",
      "type",
      "v",
    ]);
    expect(hello.type).toBe("e2ee_hello");
    expect(hello.v).toBe(2);

    const ready = JSON.stringify({
      type: "e2ee_ready",
      v: 2,
      desktopPublicKeyB64: base64(serverPublic),
      clientNonceB64: hello.clientNonceB64,
      desktopNonceB64: base64(new Uint8Array(32).fill(3)),
      selection: { framing: 2, payloadKinds: ["text", "binary"] },
      context: hello.context,
    });
    const session = pending.finish(ready);
    expect(session.transcriptHashB64).toHaveLength(44);
    expect(session.encryptText('{"id":"1"}')).toMatch(/^[A-Za-z0-9+/]+=*$/);
  });

  it("accepts Aeroric's pinned unpadded base64url server key", () => {
    const serverSecret = new Uint8Array(32).fill(3);
    const pending = startOrcaE2EEHandshake(
      base64url(x25519.getPublicKey(serverSecret)),
      {
        protocol: ORCA_E2EE_PROTOCOL,
        initiator: "mobile",
        responder: "desktop",
        transport: "direct",
      },
      (length) => new Uint8Array(length).fill(7),
    );
    expect(JSON.parse(pending.helloJson).type).toBe("e2ee_hello");
  });

  it("rejects a ready frame with extra fields or a changed context", () => {
    const serverSecret = new Uint8Array(32).fill(3);
    const serverPublic = x25519.getPublicKey(serverSecret);
    const pending = startOrcaE2EEHandshake(
      base64(serverPublic),
      {
        protocol: ORCA_E2EE_PROTOCOL,
        initiator: "mobile",
        responder: "desktop",
        transport: "direct",
      },
      (length) => new Uint8Array(length).fill(7),
    );
    const hello = JSON.parse(pending.helloJson) as Record<string, unknown>;
    const ready = {
      type: "e2ee_ready",
      v: 2,
      desktopPublicKeyB64: base64(serverPublic),
      clientNonceB64: hello.clientNonceB64,
      desktopNonceB64: base64(new Uint8Array(32).fill(3)),
      selection: { framing: 2, payloadKinds: ["text", "binary"] },
      context: { ...(hello.context as Record<string, unknown>), transport: "relay" },
      extra: true,
    };
    expect(() => pending.finish(JSON.stringify(ready))).toThrow();
  });

  // 回归:接收方向曾错用手机→桌面的密钥与发送计数器,桌面封的帧一律
  // authentication failed;手机先发过消息后还会退化成 nonce mismatch。
  describe("receive direction matches the desktop sealer", () => {
    const mobileToDesktopKey = new Uint8Array(32).fill(1); // schedule[0..32]
    const desktopToMobileKey = new Uint8Array(32).fill(2); // schedule[32..64]
    const sessionId = new Uint8Array(32).fill(4); // schedule[64..96]
    const newSession = () =>
      new OrcaE2EESession(
        mobileToDesktopKey,
        desktopToMobileKey,
        sessionId,
        base64(new Uint8Array(32).fill(8)),
      );

    it("opens a desktop-sealed text frame", () => {
      const session = newSession();
      const frame = desktopSeal({
        key: desktopToMobileKey,
        sessionId,
        kind: 0,
        counter: 0n,
        payload: new TextEncoder().encode('{"id":"rpc-1","ok":true}'),
      });
      expect(session.decryptText(base64(frame))).toBe('{"id":"rpc-1","ok":true}');
    });

    it("opens a desktop-sealed binary frame", () => {
      const session = newSession();
      const frame = desktopSeal({
        key: desktopToMobileKey,
        sessionId,
        kind: 1,
        counter: 0n,
        payload: new Uint8Array([7, 8, 9]),
      });
      expect([...session.decryptBinary(frame)]).toEqual([7, 8, 9]);
    });

    it("keeps the receive counter independent from the send counter", () => {
      const session = newSession();
      session.encryptText("a");
      session.encryptText("b");
      session.encryptBinary(new Uint8Array([1]));
      // 桌面首个回复始终带自己方向的 counter 0,不受手机已发条数影响。
      const frame = desktopSeal({
        key: desktopToMobileKey,
        sessionId,
        kind: 0,
        counter: 0n,
        payload: new TextEncoder().encode("first-reply"),
      });
      expect(session.decryptText(base64(frame))).toBe("first-reply");
    });

    it("advances the receive counter monotonically and rejects a replay", () => {
      const session = newSession();
      const reply = (counter: bigint, body: string) =>
        base64(
          desktopSeal({
            key: desktopToMobileKey,
            sessionId,
            kind: 0,
            counter,
            payload: new TextEncoder().encode(body),
          }),
        );
      expect(session.decryptText(reply(0n, "one"))).toBe("one");
      expect(session.decryptText(reply(1n, "two"))).toBe("two");
      // 重放第 0 号帧:nonce 已经不匹配当前接收计数器。
      expect(() => session.decryptText(reply(0n, "one"))).toThrow(/nonce mismatch/);
    });

    it("rejects a frame sealed with the mobile-to-desktop key", () => {
      const session = newSession();
      const frame = desktopSeal({
        key: mobileToDesktopKey,
        sessionId,
        kind: 0,
        counter: 0n,
        payload: new TextEncoder().encode("wrong-key"),
      });
      expect(() => session.decryptText(base64(frame))).toThrow(/authentication failed/);
    });

    it("rejects a desktop frame that claims the mobile-to-desktop direction", () => {
      const session = newSession();
      const frame = desktopSeal({
        key: desktopToMobileKey,
        sessionId,
        kind: 0,
        counter: 0n,
        payload: new TextEncoder().encode("wrong-direction"),
        direction: 0,
      });
      expect(() => session.decryptText(base64(frame))).toThrow(/nonce mismatch/);
    });
  });

  it("binds text and binary frames to distinct payload kinds", () => {
    const session = new OrcaE2EESession(
      new Uint8Array(32).fill(1),
      new Uint8Array(32).fill(2),
      new Uint8Array(32).fill(4),
      base64(new Uint8Array(32).fill(8)),
    );
    const textFrame = session.encryptText('{"id":"rpc-1"}');
    const binaryFrame = session.encryptBinary(new Uint8Array([1, 2, 3]));
    expect(textFrame).not.toBe(binaryFrame.toString());
    expect(textFrame).toMatch(/^[A-Za-z0-9+/]+=*$/);
    expect(binaryFrame[14]).toBe(1);
  });
});
