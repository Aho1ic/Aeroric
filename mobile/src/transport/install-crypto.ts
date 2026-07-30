/**
 * RN(Hermes)没有 WebCrypto;expo-crypto 提供 CSPRNG。
 * 在 app/_layout.tsx 顶部 import 本文件,保证 e2ee.ts 的默认随机源可用。
 * vitest / node 环境已有 globalThis.crypto,此处为 no-op。
 */

import * as Crypto from "expo-crypto";

type CryptoLike = { getRandomValues?: (buffer: Uint8Array) => Uint8Array };

const holder = globalThis as { crypto?: CryptoLike };
if (!holder.crypto?.getRandomValues) {
  holder.crypto = {
    ...holder.crypto,
    getRandomValues: (buffer: Uint8Array) => Crypto.getRandomValues(buffer),
  };
}
