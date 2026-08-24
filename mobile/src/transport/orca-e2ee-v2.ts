/**
 * Orca mobile E2EE v2 compatibility channel.
 *
 * This mirrors Orca's mobile-e2ee-v2-contract, key schedule, and framing
 * modules. It is intentionally separate from Aeroric's legacy ChaCha channel
 * so a connection can negotiate the correct protocol without silent crypto
 * downgrade.
 */

import { x25519 } from "@noble/curves/ed25519.js";
import { xsalsa20poly1305 } from "@noble/ciphers/salsa.js";
import { hkdf } from "@noble/hashes/hkdf.js";
import { sha256 } from "@noble/hashes/sha2.js";

export const ORCA_E2EE_PROTOCOL = "orca-mobile-e2ee" as const;
export const ORCA_E2EE_VERSION = 2 as const;
export const ORCA_TEXT_KIND = 0 as const;
export const ORCA_BINARY_KIND = 1 as const;

export type OrcaE2EETransport = "direct" | "relay";

export type OrcaE2EEContext = {
  protocol: typeof ORCA_E2EE_PROTOCOL;
  initiator: "mobile";
  responder: "desktop";
  transport: OrcaE2EETransport;
  relayHostId?: string;
};

export type RandomBytes = (length: number) => Uint8Array;

export type OrcaE2EEPendingHandshake = {
  helloJson: string;
  finish: (readyJson: string) => OrcaE2EESession;
};

type OrcaHello = {
  type: "e2ee_hello";
  v: 2;
  clientPublicKeyB64: string;
  clientNonceB64: string;
  capabilities: { framing: [2]; payloadKinds: ["text", "binary"] };
  context: OrcaE2EEContext;
};

const NONCE_BYTES = 24;
const SESSION_ID_BYTES = 32;
const HEADER_BYTES = SESSION_ID_BYTES + 1 + 1 + 8;
const TAG_BYTES = 16;
const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/**
 * 帧头/nonce 里的方向字节。手机是 initiator:发出的帧恒为
 * `DIRECTION_MOBILE_TO_DESKTOP`,收到的帧恒为 `DIRECTION_DESKTOP_TO_MOBILE`
 * (桌面端 orca_crypto.rs 的 `send` 用 direction=1、`recv` 用 direction=0)。
 */
const DIRECTION_MOBILE_TO_DESKTOP = 0;
const DIRECTION_DESKTOP_TO_MOBILE = 1;

export class OrcaE2EESession {
  /** 发送计数器:只随手机→桌面的帧递增。 */
  private sendCounter = 0n;
  /** 接收计数器:只随桌面→手机的帧递增,与发送计数器互不影响。 */
  private receiveCounter = 0n;

  constructor(
    private readonly mobileToDesktopKey: Uint8Array,
    private readonly desktopToMobileKey: Uint8Array,
    private readonly sessionId: Uint8Array,
    readonly transcriptHashB64: string,
  ) {
    requireLength(mobileToDesktopKey, 32, "mobile-to-desktop key");
    requireLength(desktopToMobileKey, 32, "desktop-to-mobile key");
    requireLength(sessionId, SESSION_ID_BYTES, "session id");
  }

  encryptText(plaintext: string): string {
    return bytesToBase64(this.seal(textEncoder.encode(plaintext), ORCA_TEXT_KIND));
  }

  decryptText(frameB64: string): string {
    return textDecoder.decode(this.open(bytesFromCanonicalBase64(frameB64), ORCA_TEXT_KIND));
  }

  encryptBinary(plaintext: Uint8Array): Uint8Array {
    return this.seal(plaintext, ORCA_BINARY_KIND);
  }

  decryptBinary(frame: Uint8Array): Uint8Array {
    return this.open(frame, ORCA_BINARY_KIND);
  }

  private seal(payload: Uint8Array, kind: number): Uint8Array {
    const counter = this.sendCounter;
    if (counter > MAX_UINT64) throw new Error("Orca E2EE counter exhausted");
    const direction = DIRECTION_MOBILE_TO_DESKTOP;
    const header = encodeHeader(this.sessionId, direction, kind, counter);
    const nonce = encodeNonce(this.sessionId, direction, kind, counter);
    const plaintext = concatBytes([header, payload]);
    const ciphertext = xsalsa20poly1305(this.mobileToDesktopKey, nonce).encrypt(plaintext);
    this.sendCounter = counter + 1n;
    return concatBytes([nonce, ciphertext]);
  }

  private open(frame: Uint8Array, kind: number): Uint8Array {
    if (frame.length < NONCE_BYTES + HEADER_BYTES + TAG_BYTES) {
      throw new Error("Orca E2EE frame too short");
    }
    const counter = this.receiveCounter;
    if (counter > MAX_UINT64) throw new Error("Orca E2EE counter exhausted");
    const direction = DIRECTION_DESKTOP_TO_MOBILE;
    const nonce = encodeNonce(this.sessionId, direction, kind, counter);
    if (!equalBytes(frame.subarray(0, NONCE_BYTES), nonce)) {
      throw new Error("Orca E2EE nonce mismatch");
    }
    let plaintext: Uint8Array;
    try {
      plaintext = xsalsa20poly1305(this.desktopToMobileKey, nonce).decrypt(
        frame.subarray(NONCE_BYTES),
      );
    } catch {
      throw new Error("Orca E2EE authentication failed");
    }
    const header = encodeHeader(this.sessionId, direction, kind, counter);
    if (!equalBytes(plaintext.subarray(0, HEADER_BYTES), header)) {
      throw new Error("Orca E2EE header mismatch");
    }
    this.receiveCounter = counter + 1n;
    return plaintext.slice(HEADER_BYTES);
  }
}

export function startOrcaE2EEHandshake(
  serverPublicKeyB64: string,
  context: OrcaE2EEContext,
  randomBytes: RandomBytes = defaultRandomBytes,
): OrcaE2EEPendingHandshake {
  validateContext(context);
  const serverPublicKey = decodeServerPublicKey(serverPublicKeyB64);
  const clientSecret = randomBytes(32);
  const clientPublicKey = x25519.getPublicKey(clientSecret);
  const clientNonce = randomBytes(32);
  const hello: OrcaHello = {
    type: "e2ee_hello" as const,
    v: ORCA_E2EE_VERSION,
    clientPublicKeyB64: bytesToBase64(clientPublicKey),
    clientNonceB64: bytesToBase64(clientNonce),
    capabilities: { framing: [2] as const, payloadKinds: ["text", "binary"] as const },
    context,
  };
  const helloJson = JSON.stringify(hello);

  return {
    helloJson,
    finish(readyJson: string): OrcaE2EESession {
      const ready = parseReady(readyJson, hello, context);
      const desktopPublicKey = bytesFromCanonicalBase64(ready.desktopPublicKeyB64);
      const desktopNonce = bytesFromCanonicalBase64(ready.desktopNonceB64);
      if (desktopPublicKey.length !== 32 || desktopNonce.length !== 32) {
        throw new Error("Invalid Orca E2EE ready key material");
      }
      if (!equalBytes(bytesFromCanonicalBase64(ready.clientNonceB64), clientNonce)) {
        throw new Error("Orca E2EE client nonce mismatch");
      }
      if (!equalBytes(desktopPublicKey, serverPublicKey)) {
        throw new Error("Orca E2EE desktop identity mismatch");
      }
      const sharedSecret = x25519.getSharedSecret(clientSecret, desktopPublicKey);
      if (sharedSecret.every((byte) => byte === 0)) {
        throw new Error("Invalid Orca E2EE desktop key");
      }
      const transcript = encodeTranscript(
        hello,
        ready,
        clientPublicKey,
        desktopPublicKey,
        clientNonce,
        desktopNonce,
      );
      const transcriptHash = sha256(transcript);
      const schedule = deriveSchedule(sharedSecret, transcript, clientNonce, desktopNonce);
      return new OrcaE2EESession(
        schedule.slice(0, 32),
        schedule.slice(32, 64),
        schedule.slice(64, 96),
        bytesToBase64(transcriptHash),
      );
    },
  };
}

function parseReady(
  readyJson: string,
  hello: OrcaHello,
  expectedContext: OrcaE2EEContext,
): {
  type: "e2ee_ready";
  v: 2;
  desktopPublicKeyB64: string;
  clientNonceB64: string;
  desktopNonceB64: string;
  selection: { framing: 2; payloadKinds: ["text", "binary"] };
  context: OrcaE2EEContext;
} {
  let ready: unknown;
  try {
    ready = JSON.parse(readyJson);
  } catch {
    throw new Error("Malformed Orca E2EE ready");
  }
  if (
    !isRecord(ready) ||
    !hasExactKeys(ready, [
      "type",
      "v",
      "desktopPublicKeyB64",
      "clientNonceB64",
      "desktopNonceB64",
      "selection",
      "context",
    ])
  ) {
    throw new Error("Invalid Orca E2EE ready fields");
  }
  if (
    ready.type !== "e2ee_ready" ||
    ready.v !== 2 ||
    ready.clientNonceB64 !== hello.clientNonceB64
  ) {
    throw new Error("Invalid Orca E2EE ready");
  }
  if (
    !isRecord(ready.selection) ||
    !hasExactKeys(ready.selection, ["framing", "payloadKinds"]) ||
    ready.selection.framing !== 2 ||
    !arrayEquals(ready.selection.payloadKinds, ["text", "binary"])
  ) {
    throw new Error("Unsupported Orca E2EE selection");
  }
  if (!isRecord(ready.context) || !contextsEqual(ready.context, expectedContext)) {
    throw new Error("Orca E2EE context mismatch");
  }
  if (typeof ready.desktopPublicKeyB64 !== "string" || typeof ready.desktopNonceB64 !== "string") {
    throw new Error("Invalid Orca E2EE ready key material");
  }
  const desktopPublicKey = bytesFromCanonicalBase64(ready.desktopPublicKeyB64);
  const desktopNonce = bytesFromCanonicalBase64(ready.desktopNonceB64);
  if (desktopPublicKey.length !== 32 || desktopNonce.length !== 32) {
    throw new Error("Invalid Orca E2EE ready key material");
  }
  return ready as typeof ready & {
    type: "e2ee_ready";
    v: 2;
    desktopPublicKeyB64: string;
    clientNonceB64: string;
    desktopNonceB64: string;
    selection: { framing: 2; payloadKinds: ["text", "binary"] };
    context: OrcaE2EEContext;
  };
}

function validateContext(context: OrcaE2EEContext): void {
  if (
    context.protocol !== ORCA_E2EE_PROTOCOL ||
    context.initiator !== "mobile" ||
    context.responder !== "desktop"
  ) {
    throw new Error("Invalid Orca E2EE context");
  }
  if (context.transport === "direct") {
    if (Object.keys(context).length !== 4) throw new Error("Invalid direct Orca E2EE context");
  } else if (context.transport === "relay") {
    if (
      Object.keys(context).length !== 5 ||
      !context.relayHostId ||
      !isBase64Url16(context.relayHostId)
    ) {
      throw new Error("Invalid relay Orca E2EE context");
    }
  } else {
    throw new Error("Invalid Orca E2EE transport");
  }
}

function contextsEqual(left: Record<string, unknown>, right: OrcaE2EEContext): boolean {
  return (
    left.protocol === right.protocol &&
    left.initiator === right.initiator &&
    left.responder === right.responder &&
    left.transport === right.transport &&
    left.relayHostId === right.relayHostId
  );
}

function encodeTranscript(
  hello: OrcaHello,
  ready: { context: OrcaE2EEContext },
  clientPublicKey: Uint8Array,
  desktopPublicKey: Uint8Array,
  clientNonce: Uint8Array,
  desktopNonce: Uint8Array,
): Uint8Array {
  // The field order and length-prefix encoding are part of Orca's wire
  // contract; do not replace this with JSON.stringify.
  const context = hello.context;
  const fields: [string, Uint8Array][] = [
    ["domain", textEncoder.encode("orca-mobile-e2ee/v2/transcript")],
    ["mobile-to-desktop.type", textEncoder.encode("e2ee_hello")],
    ["mobile-to-desktop.version", u32(2)],
    ["mobile-to-desktop.client-public-key", clientPublicKey],
    ["mobile-to-desktop.client-nonce", clientNonce],
    ["mobile-to-desktop.capabilities.framing", numberList([2])],
    ["mobile-to-desktop.capabilities.payload-kinds", stringList(["text", "binary"])],
    ["mobile-to-desktop.context.protocol", textEncoder.encode(context.protocol)],
    ["mobile-to-desktop.context.initiator", textEncoder.encode(context.initiator)],
    ["mobile-to-desktop.context.responder", textEncoder.encode(context.responder)],
    ["mobile-to-desktop.context.transport", textEncoder.encode(context.transport)],
    ["mobile-to-desktop.context.relay-host-id", textEncoder.encode(context.relayHostId ?? "")],
    ["desktop-to-mobile.type", textEncoder.encode("e2ee_ready")],
    ["desktop-to-mobile.version", u32(2)],
    ["desktop-to-mobile.desktop-public-key", desktopPublicKey],
    ["desktop-to-mobile.client-nonce-echo", clientNonce],
    ["desktop-to-mobile.desktop-nonce", desktopNonce],
    ["desktop-to-mobile.selection.framing", u32(2)],
    ["desktop-to-mobile.selection.payload-kinds", stringList(["text", "binary"])],
    ["desktop-to-mobile.context.protocol", textEncoder.encode(ready.context.protocol)],
    ["desktop-to-mobile.context.initiator", textEncoder.encode(ready.context.initiator)],
    ["desktop-to-mobile.context.responder", textEncoder.encode(ready.context.responder)],
    ["desktop-to-mobile.context.transport", textEncoder.encode(ready.context.transport)],
    [
      "desktop-to-mobile.context.relay-host-id",
      textEncoder.encode(ready.context.relayHostId ?? ""),
    ],
  ];
  return concatBytes(
    fields.flatMap(([name, value]) => [
      u32(textEncoder.encode(name).length),
      textEncoder.encode(name),
      u32(value.length),
      value,
    ]),
  );
}

function deriveSchedule(
  sharedSecret: Uint8Array,
  transcript: Uint8Array,
  clientNonce: Uint8Array,
  desktopNonce: Uint8Array,
): Uint8Array {
  const transcriptHash = sha256(transcript);
  const salt = sha256(
    concatBytes([textEncoder.encode("orca-mobile-e2ee/v2/salt\0"), clientNonce, desktopNonce]),
  );
  const info = concatBytes([textEncoder.encode("orca-mobile-e2ee/v2/session\0"), transcriptHash]);
  return hkdf(sha256, sharedSecret, salt, info, 96);
}

function encodeHeader(
  sessionId: Uint8Array,
  direction: number,
  kind: number,
  counter: bigint,
): Uint8Array {
  return concatBytes([sessionId, new Uint8Array([direction, kind]), u64(counter)]);
}

function encodeNonce(
  sessionId: Uint8Array,
  direction: number,
  kind: number,
  counter: bigint,
): Uint8Array {
  const nonce = new Uint8Array(NONCE_BYTES);
  nonce.set(sessionId.slice(0, 12));
  nonce[12] = 2;
  nonce[13] = direction;
  nonce[14] = kind;
  nonce[15] = 0;
  nonce.set(u64(counter), 16);
  return nonce;
}

function decodeServerPublicKey(value: string): Uint8Array {
  try {
    const bytes = bytesFromCanonicalBase64(value);
    if (bytes.length === 32) return bytes;
  } catch {
    // Aeroric pairing offers use unpadded base64url while Orca's v2 contract
    // uses padded standard base64; accept both representations at this trust
    // boundary and still require exactly 32 bytes.
  }
  try {
    const bytes = bytesFromBase64Url(value);
    if (bytes.length === 32) return bytes;
  } catch {
    // fall through to one stable validation error
  }
  throw new Error("Invalid Orca E2EE server key");
}

function bytesFromCanonicalBase64(value: string): Uint8Array {
  if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value))
    throw new Error("Invalid canonical base64");
  const normalized =
    typeof atob === "function" ? atob(value) : Buffer.from(value, "base64").toString("binary");
  const bytes = Uint8Array.from(normalized, (character) => character.charCodeAt(0));
  if (bytesToBase64(bytes) !== value) throw new Error("Non-canonical base64");
  return bytes;
}

function bytesFromBase64Url(value: string): Uint8Array {
  const padded =
    value.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (value.length % 4)) % 4);
  return bytesFromCanonicalBase64(padded);
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return typeof btoa === "function" ? btoa(binary) : Buffer.from(bytes).toString("base64");
}

function defaultRandomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  const crypto = globalThis.crypto;
  if (!crypto?.getRandomValues) throw new Error("Secure random source unavailable");
  crypto.getRandomValues(bytes);
  return bytes;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const actual = Object.keys(value).sort();
  return (
    actual.length === keys.length && actual.every((key, index) => key === [...keys].sort()[index])
  );
}

function arrayEquals(left: unknown, right: readonly unknown[]): boolean {
  return (
    Array.isArray(left) &&
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function isBase64Url16(value: string): boolean {
  return /^[A-Za-z0-9_-]{16}$/.test(value);
}

function requireLength(value: Uint8Array, length: number, label: string): void {
  if (value.length !== length) throw new Error(`Invalid ${label}`);
}

function u32(value: number): Uint8Array {
  const result = new Uint8Array(4);
  new DataView(result.buffer).setUint32(0, value, false);
  return result;
}

function u64(value: bigint): Uint8Array {
  const result = new Uint8Array(8);
  new DataView(result.buffer).setBigUint64(0, value, false);
  return result;
}

function numberList(values: readonly number[]): Uint8Array {
  return concatBytes([u32(values.length), ...values.map(u32)]);
}

function stringList(values: readonly string[]): Uint8Array {
  return concatBytes([
    u32(values.length),
    ...values.flatMap((value) => {
      const bytes = textEncoder.encode(value);
      return [u32(bytes.length), bytes];
    }),
  ]);
}

function concatBytes(parts: readonly Uint8Array[]): Uint8Array {
  const result = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    result.set(part, offset);
    offset += part.length;
  }
  return result;
}

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
}

const MAX_UINT64 = (1n << 64n) - 1n;
