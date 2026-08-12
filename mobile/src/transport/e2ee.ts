/**
 * 远程连接 E2EE 客户端(协议 v2),与桌面端 src-tauri/src/remote/crypto.rs 严格镜像:
 *
 * - 握手:客户端发明文 hello(临时 X25519 公钥),桌面回 hello_ack(临时公钥 +
 *   confirm 标签)。会话密钥 = HKDF-SHA256(dh(c_eph, s_static) || dh(c_eph, s_eph)),
 *   info = c_eph || s_eph || s_static,salt 固定。confirm 用 s2c 密钥 seq 0 封
 *   "aeroric-e2ee-confirm-v2"——只有持有配对 QR 中 pin 的静态私钥的主机才能算出,
 *   验证失败即中间人/主机换代,必须中止。
 * - 帧:`[kind:u8][seq:u64 BE][ChaCha20-Poly1305 ciphertext]`,kind 作 AAD,
 *   nonce = 4 字节 0 + seq,双向密钥独立、seq 严格递增。
 *
 * 纯 TS + @noble 系(无原生模块),vitest 与 RN(Hermes)同源运行。
 * 随机源可注入:RN 侧由 install-crypto.ts 提供 expo-crypto polyfill。
 */

import { x25519 } from "@noble/curves/ed25519.js";
import { chacha20poly1305 } from "@noble/ciphers/chacha.js";
import { hkdf } from "@noble/hashes/hkdf.js";
import { sha256 } from "@noble/hashes/sha2.js";

export const E2EE_PROTOCOL_VERSION = 2;
export const KIND_CTRL = 1;
export const KIND_TERMINAL = 2;

const HKDF_SALT = utf8("aeroric-remote-e2ee-v2");
const CONFIRM_PLAINTEXT = utf8("aeroric-e2ee-confirm-v2");
const CONFIRM_AAD = utf8("confirm");
const FRAME_HEADER_BYTES = 9;

export type RandomBytes = (length: number) => Uint8Array;

function utf8(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

export function defaultRandomBytes(length: number): Uint8Array {
  const globalCrypto = (
    globalThis as { crypto?: { getRandomValues?: (b: Uint8Array) => Uint8Array } }
  ).crypto;
  if (!globalCrypto?.getRandomValues) {
    throw new Error("No secure random source; import install-crypto before connecting");
  }
  const bytes = new Uint8Array(length);
  globalCrypto.getRandomValues(bytes);
  return bytes;
}

// ── base64url(无填充,匹配 Rust URL_SAFE_NO_PAD) ──────────────────────────

export function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  const b64 = typeof btoa === "function" ? btoa(binary) : Buffer.from(bytes).toString("base64");
  return b64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

export function base64UrlToBytes(text: string): Uint8Array {
  const normalized = text.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
  const binary =
    typeof atob === "function" ? atob(padded) : Buffer.from(padded, "base64").toString("binary");
  return Uint8Array.from(binary, (ch) => ch.charCodeAt(0));
}

// ── 密钥派生与帧加解密 ───────────────────────────────────────────────────────

function isAllZero(bytes: Uint8Array): boolean {
  return bytes.every((b) => b === 0);
}

function deriveKeys(
  dhStatic: Uint8Array,
  dhEphemeral: Uint8Array,
  clientEph: Uint8Array,
  serverEph: Uint8Array,
  serverStatic: Uint8Array,
): { clientToServer: Uint8Array; serverToClient: Uint8Array } {
  const ikm = new Uint8Array(64);
  ikm.set(dhStatic, 0);
  ikm.set(dhEphemeral, 32);
  const info = new Uint8Array(96);
  info.set(clientEph, 0);
  info.set(serverEph, 32);
  info.set(serverStatic, 64);
  const okm = hkdf(sha256, ikm, HKDF_SALT, info, 64);
  return { clientToServer: okm.slice(0, 32), serverToClient: okm.slice(32, 64) };
}

class DirectionCipher {
  private seq = 0n;

  constructor(private readonly key: Uint8Array) {}

  private static nonce(seq: bigint): Uint8Array {
    const bytes = new Uint8Array(12);
    new DataView(bytes.buffer).setBigUint64(4, seq, false);
    return bytes;
  }

  seal(aad: Uint8Array, plain: Uint8Array): { seq: bigint; ciphertext: Uint8Array } {
    const seq = this.seq;
    this.seq += 1n;
    const cipher = chacha20poly1305(this.key, DirectionCipher.nonce(seq), aad);
    return { seq, ciphertext: cipher.encrypt(plain) };
  }

  open(seq: bigint, aad: Uint8Array, ciphertext: Uint8Array): Uint8Array {
    if (seq !== this.seq) {
      throw new Error(`out-of-order frame: got ${seq}, want ${this.seq}`);
    }
    const cipher = chacha20poly1305(this.key, DirectionCipher.nonce(seq), aad);
    const plain = cipher.decrypt(ciphertext);
    this.seq = seq + 1n;
    return plain;
  }
}

export class E2eeSession {
  constructor(
    private readonly send: DirectionCipher,
    private readonly recv: DirectionCipher,
  ) {}

  encryptFrame(kind: number, plain: Uint8Array): Uint8Array {
    const { seq, ciphertext } = this.send.seal(new Uint8Array([kind]), plain);
    const frame = new Uint8Array(FRAME_HEADER_BYTES + ciphertext.length);
    frame[0] = kind;
    new DataView(frame.buffer).setBigUint64(1, seq, false);
    frame.set(ciphertext, FRAME_HEADER_BYTES);
    return frame;
  }

  decryptFrame(frame: Uint8Array): { kind: number; plain: Uint8Array } {
    if (frame.length < FRAME_HEADER_BYTES) {
      throw new Error("frame too short");
    }
    const kind = frame[0];
    const seq = new DataView(frame.buffer, frame.byteOffset).getBigUint64(1, false);
    const plain = this.recv.open(seq, new Uint8Array([kind]), frame.slice(FRAME_HEADER_BYTES));
    return { kind, plain };
  }
}

// ── 握手 ─────────────────────────────────────────────────────────────────────

export interface PendingHandshake {
  /** 发给服务端的明文 hello(WS text 帧)。 */
  helloJson: string;
  /** 用 hello_ack 完成握手;confirm 验证失败抛错(主机身份不符)。 */
  finish(ackJson: string): E2eeSession;
}

export function startHandshake(
  serverStaticPubB64: string,
  randomBytes: RandomBytes = defaultRandomBytes,
): PendingHandshake {
  const serverStatic = base64UrlToBytes(serverStaticPubB64);
  if (serverStatic.length !== 32) {
    throw new Error("主机公钥格式无效,请重新扫码配对");
  }
  const ephSecret = randomBytes(32);
  const ephPub = x25519.getPublicKey(ephSecret);
  const helloJson = JSON.stringify({
    v: E2EE_PROTOCOL_VERSION,
    type: "hello",
    pub: bytesToBase64Url(ephPub),
  });
  return {
    helloJson,
    finish(ackJson: string): E2eeSession {
      const ack = JSON.parse(ackJson) as {
        v?: number;
        type?: string;
        pub?: string;
        confirm?: string;
        error?: string;
      };
      if (ack.type === "hello_error") {
        throw new Error(ack.error ?? "主机拒绝了连接");
      }
      if (
        ack.type !== "hello_ack" ||
        typeof ack.pub !== "string" ||
        typeof ack.confirm !== "string"
      ) {
        throw new Error("握手响应无效");
      }
      const serverEph = base64UrlToBytes(ack.pub);
      if (serverEph.length !== 32) {
        throw new Error("握手响应无效");
      }
      const dhStatic = x25519.getSharedSecret(ephSecret, serverStatic);
      const dhEphemeral = x25519.getSharedSecret(ephSecret, serverEph);
      if (isAllZero(dhStatic) || isAllZero(dhEphemeral)) {
        throw new Error("主机密钥无效");
      }
      const { clientToServer, serverToClient } = deriveKeys(
        dhStatic,
        dhEphemeral,
        ephPub,
        serverEph,
        serverStatic,
      );
      const send = new DirectionCipher(clientToServer);
      const recv = new DirectionCipher(serverToClient);
      let confirmPlain: Uint8Array;
      try {
        confirmPlain = recv.open(0n, CONFIRM_AAD, base64UrlToBytes(ack.confirm));
      } catch {
        throw new Error("主机身份验证失败,请重新扫码配对");
      }
      if (
        confirmPlain.length !== CONFIRM_PLAINTEXT.length ||
        !confirmPlain.every((b, i) => b === CONFIRM_PLAINTEXT[i])
      ) {
        throw new Error("主机身份验证失败,请重新扫码配对");
      }
      return new E2eeSession(send, recv);
    },
  };
}

// ── 服务端侧实现(仅测试:vitest 里模拟桌面端) ─────────────────────────────

export interface TestServerKeys {
  secret: Uint8Array;
  publicB64: string;
}

export function testGenerateServerKeys(
  randomBytes: RandomBytes = defaultRandomBytes,
): TestServerKeys {
  const secret = randomBytes(32);
  return { secret, publicB64: bytesToBase64Url(x25519.getPublicKey(secret)) };
}

export function testRespondHandshake(
  serverKeys: TestServerKeys,
  helloJson: string,
  randomBytes: RandomBytes = defaultRandomBytes,
): { ackJson: string; session: E2eeSession } {
  const hello = JSON.parse(helloJson) as { v?: number; type?: string; pub?: string };
  if (hello.v !== E2EE_PROTOCOL_VERSION || hello.type !== "hello" || !hello.pub) {
    throw new Error("bad hello");
  }
  const clientEph = base64UrlToBytes(hello.pub);
  const ephSecret = randomBytes(32);
  const ephPub = x25519.getPublicKey(ephSecret);
  const serverStaticPub = base64UrlToBytes(serverKeys.publicB64);
  const dhStatic = x25519.getSharedSecret(serverKeys.secret, clientEph);
  const dhEphemeral = x25519.getSharedSecret(ephSecret, clientEph);
  const { clientToServer, serverToClient } = deriveKeys(
    dhStatic,
    dhEphemeral,
    clientEph,
    ephPub,
    serverStaticPub,
  );
  const send = new DirectionCipher(serverToClient);
  const recv = new DirectionCipher(clientToServer);
  const { ciphertext } = send.seal(CONFIRM_AAD, CONFIRM_PLAINTEXT);
  const ackJson = JSON.stringify({
    v: E2EE_PROTOCOL_VERSION,
    type: "hello_ack",
    pub: bytesToBase64Url(ephPub),
    confirm: bytesToBase64Url(ciphertext),
  });
  return { ackJson, session: new E2eeSession(send, recv) };
}
