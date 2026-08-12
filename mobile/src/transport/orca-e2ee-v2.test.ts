import { describe, expect, it } from "vitest";
import { x25519 } from "@noble/curves/ed25519.js";
import { OrcaE2EESession, ORCA_E2EE_PROTOCOL, startOrcaE2EEHandshake } from "./orca-e2ee-v2";

function base64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
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
