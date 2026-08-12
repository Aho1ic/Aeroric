/**
 * 配对深链解析:`aeroric://pair?code=<base64url(JSON)>`。
 * 与桌面端 remote/mod.rs 的 remote_create_invite 输出严格对应。
 * 纯 TS 无 RN 依赖,便于 vitest 覆盖。
 */

import type { PairingOffer } from "../types";

export const SUPPORTED_OFFER_VERSION = 2;
const MAX_CODE_LENGTH = 64 * 1024;

function base64UrlDecode(code: string): string {
  const normalized = code.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
  // atob 在 RN(Hermes)与 node 20+ 均可用
  const binary =
    typeof atob === "function" ? atob(padded) : Buffer.from(padded, "base64").toString("binary");
  // UTF-8 还原(offer 含主机名,可能有非 ASCII)
  const bytes = Uint8Array.from(binary, (ch) => ch.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

export function extractPairingCode(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed || trimmed.length > MAX_CODE_LENGTH) return null;
  if (trimmed.startsWith("aeroric://")) {
    // RN 环境无 URL.searchParams 完整实现,手动截取更稳
    const match = /^aeroric:\/\/pair\/?\?code=([A-Za-z0-9_-]+)/.exec(trimmed);
    return match ? match[1] : null;
  }
  // 允许直接粘贴裸 code
  return /^[A-Za-z0-9_-]+$/.test(trimmed) ? trimmed : null;
}

export function parsePairingOffer(input: string): PairingOffer {
  const code = extractPairingCode(input);
  if (!code) {
    throw new Error("不是有效的 Aeroric 配对码");
  }
  let raw: unknown;
  try {
    raw = JSON.parse(base64UrlDecode(code));
  } catch {
    throw new Error("配对码内容无法解析");
  }
  const offer = raw as Partial<PairingOffer>;
  if (offer.v !== SUPPORTED_OFFER_VERSION) {
    throw new Error(
      offer.v === 1
        ? "配对码来自旧版桌面端(不支持加密),请升级电脑上的 Aeroric 后重试"
        : `配对码版本不支持(${String(offer.v)})`,
    );
  }
  if (!Array.isArray(offer.endpoints) || offer.endpoints.length === 0) {
    throw new Error("配对码缺少连接地址");
  }
  const endpoints = offer.endpoints.filter(
    (e): e is string => typeof e === "string" && /^wss?:\/\//.test(e),
  );
  if (endpoints.length === 0) {
    throw new Error("配对码中的连接地址无效");
  }
  if (typeof offer.invite !== "string" || offer.invite.length === 0) {
    throw new Error("配对码缺少邀请令牌");
  }
  // v2 起 E2EE 强制:静态公钥是握手信任根,缺失即无法安全连接
  if (typeof offer.publicKey !== "string" || offer.publicKey.length === 0) {
    throw new Error("配对码缺少主机公钥,请在电脑上重新生成二维码");
  }
  return {
    v: offer.v,
    endpoints,
    invite: offer.invite,
    hostName: typeof offer.hostName === "string" && offer.hostName ? offer.hostName : "Aeroric",
    hostId: typeof offer.hostId === "string" && offer.hostId ? offer.hostId : undefined,
    publicKey: offer.publicKey,
  };
}
