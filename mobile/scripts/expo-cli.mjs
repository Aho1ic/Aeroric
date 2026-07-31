#!/usr/bin/env node

// Expo CLI 包装器：在 LAN 模式下先让用户确认手机可访问的电脑 IPv4 地址，
// 再把它通过 REACT_NATIVE_PACKAGER_HOSTNAME 交给 expo，避免 Expo 自动选中
// Clash/Surge TUN 的 198.18.0.1 之类 fake-IP 导致 Expo Go 扫码超时。

import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { isIP } from "node:net";
import { networkInterfaces } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline/promises";

import { candidateLabel, rankCandidates, shouldPromptForLanAddress } from "./lan-address.mjs";

const require = createRequire(import.meta.url);
const expoCli = require.resolve("expo/bin/cli");
const args = process.argv.slice(2);
const projectRoot = process.cwd();
const preferenceFile = join(projectRoot, ".expo", "aeroric-lan.json");

function readRememberedAddress() {
  try {
    const parsed = JSON.parse(readFileSync(preferenceFile, "utf8"));
    return typeof parsed?.address === "string" && isIP(parsed.address) === 4
      ? parsed.address
      : null;
  } catch {
    return null;
  }
}

function rememberAddress(address) {
  try {
    writeFileSync(preferenceFile, `${JSON.stringify({ address }, null, 2)}\n`);
  } catch {
    // .expo 目录可能还不存在或不可写，记不住不影响启动。
  }
}

async function readManualAddress(readline) {
  while (true) {
    const address = (await readline.question("请输入手机可访问的电脑 IPv4 地址: ")).trim();
    if (isIP(address) === 4) return address;
    console.error("请输入有效的 IPv4 地址，例如 192.168.0.121。");
  }
}

async function chooseLanAddress() {
  const configured = process.env.AERORIC_EXPO_LAN_IP?.trim();
  if (configured) {
    if (isIP(configured) !== 4) {
      throw new Error(`AERORIC_EXPO_LAN_IP 不是有效的 IPv4 地址: ${configured}`);
    }
    console.log(`使用环境变量 AERORIC_EXPO_LAN_IP 指定的地址: ${configured}`);
    return { address: configured, persist: false };
  }

  const candidates = rankCandidates(networkInterfaces());
  const remembered = readRememberedAddress();
  const rememberedIndex = candidates.findIndex((candidate) => candidate.address === remembered);
  // 上次选择仍然存在时作为默认值，否则用排序后的首选项。
  const defaultIndex = rememberedIndex >= 0 ? rememberedIndex : 0;

  if (candidates.length === 0) {
    console.warn("未检测到可用的 IPv4 网络接口，交给 Expo 自动检测。\n");
    return { address: null, persist: false };
  }

  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    const selected = candidates[defaultIndex].address;
    console.log(`非交互环境，自动选择 Expo LAN 地址: ${selected}`);
    return { address: selected, persist: false };
  }

  console.log("\n请选择 Expo Go 扫码使用的电脑 LAN 地址（手机需与该地址同一网络）:");
  candidates.forEach((candidate, index) => {
    // “推荐”始终跟随排序结果，避免上次误选的 fake-IP 被标成推荐。
    const suffix = candidateLabel(candidate, index === 0);
    const lastUsed = candidate.address === remembered ? " (上次使用)" : "";
    console.log(`  ${index + 1}. ${candidate.address} (${candidate.name})${suffix}${lastUsed}`);
  });

  const manualOption = candidates.length + 1;
  const autoOption = candidates.length + 2;
  console.log(`  ${manualOption}. 手动输入 IPv4 地址`);
  console.log(`  ${autoOption}. 交给 Expo 自动检测`);

  const readline = createInterface({ input: process.stdin, output: process.stdout });
  try {
    while (true) {
      const defaultChoice = String(defaultIndex + 1);
      const answer = (await readline.question(`请选择 [${defaultChoice}]: `)).trim();
      const raw = answer || defaultChoice;
      const choice = Number(raw);

      if (Number.isInteger(choice) && choice >= 1 && choice <= candidates.length) {
        return { address: candidates[choice - 1].address, persist: true };
      }
      if (choice === manualOption) {
        return { address: await readManualAddress(readline), persist: true };
      }
      if (choice === autoOption) return { address: null, persist: false };
      if (isIP(raw) === 4) return { address: raw, persist: true };
      console.error("请选择列表中的编号，或直接输入有效的 IPv4 地址。");
    }
  } finally {
    readline.close();
  }
}

async function run() {
  const env = { ...process.env };

  if (shouldPromptForLanAddress(args)) {
    const { address, persist } = await chooseLanAddress();
    // 这两个变量会覆盖 Expo 生成的 URL，代理环境里常常是超时的根因。
    delete env.EXPO_PACKAGER_PROXY_URL;

    if (address) {
      env.REACT_NATIVE_PACKAGER_HOSTNAME = address;
      if (persist) rememberAddress(address);
      console.log(`Expo Go 将使用 exp://${address}:<Metro 端口>\n`);
    } else {
      delete env.REACT_NATIVE_PACKAGER_HOSTNAME;
      console.log("Expo Go 将使用 Expo 自动检测的地址。\n");
    }
  }

  const child = spawn(process.execPath, [expoCli, ...args], {
    cwd: projectRoot,
    env,
    stdio: "inherit",
  });

  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (signal === "SIGINT") resolve(130);
      else if (signal === "SIGTERM") resolve(143);
      else resolve(code ?? 1);
    });
  });

  process.exitCode = exitCode;
}

run().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
