#!/usr/bin/env node
// `scripts/native/prepare-tauri-host.mjs` — Task P8-05 Action 7/8：把宿主 helper 做成
// Tauri 可以随包分发的形状。
//
// 为什么需要它：`external` 模式的**前置条件是 Tauri 自带一个能启动的 helper**，不允许
// 依赖"用户另外装了 Native Preview"。Tauri 的 macOS `bundle.externalBin` 约定是
// `binaries/<name>-<target-triple>`，bundler 会在打包时把它放进 `Contents/MacOS/`
// 并（在签名构建里）一起签。
//
// 三条纪律（照 Action 8 原文）：
//
// 1. **显式 target**：只认入参或已校验的环境变量，**绝不把 runner 的 CPU 当目标 CPU**
//    （CI 的 macOS runner 可能是 arm64 或 x86_64 的任一种）。
// 2. **一致性检查**：拷完之后核对架构与版本——helper 与主程序不是同一个架构时，
//    打包会成功而运行时 `spawn` 立刻失败，那是最难查的一类故障。
// 3. **生成物不进 git**：`src-tauri/binaries/` 是派生产物（与 `dist`、`core/target`
//    同一类），由 `.gitignore` 挡住；机器二进制不提交。
//
// 用法：
//   node scripts/native/prepare-tauri-host.mjs --triple=aarch64-apple-darwin
//   AERORIC_TAURI_TARGET=aarch64-apple-darwin pnpm native:prepare-tauri-host
//   pnpm native:prepare-tauri-host --dry-run     # 只打印计划，不落盘

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, copyFileSync, statSync, chmodSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const binariesDir = path.join(repoRoot, "src-tauri", "binaries");
const profile = process.env.AERORIC_TAURI_PROFILE ?? "release";

// Tauri 打包时要用的三元组。**只认显式来源**。
const ALLOWED_TRIPLES = new Set([
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
]);

function parseArgs(argv) {
  const args = { triple: null, dryRun: false, profile: null };
  for (const raw of argv) {
    if (raw === "--dry-run") args.dryRun = true;
    else if (raw.startsWith("--triple=")) args.triple = raw.slice("--triple=".length);
    else if (raw.startsWith("--profile=")) args.profile = raw.slice("--profile=".length);
    else {
      console.error(`prepare-tauri-host: unknown argument ${raw}`);
      process.exit(2);
    }
  }
  return args;
}

/** 本地推导：只在**不是 CI** 时兜底，且大声说明来源。
 *
 * Action 8 的禁令是"不把 runner 的 CPU 当目标 CPU"——那条禁令针对的是 CI（那里
 * `--target` 由矩阵给出，猜错会打出错的包）。本机开发时 Tauri 会传
 * `TAURI_ENV_TARGET_TRIPLE`；万一某个路径没传，用 `rustc -vV` 的宿主三元组是**同一个
 * 架构**，不会产生跨架构包，比"直接失败让 dev 起不来"更合适。 */
function hostTriple() {
  const result = run("rustc", ["-vV"], { capture: true });
  const match = /^host:\s*(\S+)$/m.exec(result.stdout);
  return match ? match[1] : null;
}

function resolveTriple(args) {
  const explicit = args.triple ?? process.env.AERORIC_TAURI_TARGET ?? process.env.TAURI_ENV_TARGET_TRIPLE;
  if (!explicit) {
    const inCi = Boolean(process.env.CI);
    const derived = inCi ? null : hostTriple();
    if (!derived) {
      console.error(
        [
          "prepare-tauri-host: 需要显式目标三元组。",
          "  用 --triple=<triple>，或设 AERORIC_TAURI_TARGET / TAURI_ENV_TARGET_TRIPLE。",
          "  不接受『按本机 CPU 猜一个』：CI 的 runner 架构与发布目标不是一回事。",
        ].join("\n"),
      );
      process.exit(2);
    }
    console.log(`prepare-tauri-host: 未给三元组，本地推导为 ${derived}（CI 下无此兜底）`);
    return derived;
  }
  if (!ALLOWED_TRIPLES.has(explicit)) {
    console.error(`prepare-tauri-host: 不认识的目标三元组 ${explicit}`);
    console.error(`  已知：${[...ALLOWED_TRIPLES].join(", ")}`);
    process.exit(2);
  }
  return explicit;
}

function fail(message) {
  console.error(`prepare-tauri-host: ${message}`);
  process.exit(1);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
    env: { ...process.env },
  });
  return { code: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

/** helper 源文件的落点（`cargo build --target <triple>` 的输出）。 */
function builtHelperPath(triple, profileName) {
  const exeName = triple.includes("windows") ? "aeroric-cli.exe" : "aeroric-cli";
  return path.join(repoRoot, "core", "target", triple, profileName, exeName);
}

/** Tauri 约定的目标文件名：`binaries/aeroric-cli-<triple>[.exe]`。 */
function bundledHelperPath(triple) {
  const suffix = triple.includes("windows") ? ".exe" : "";
  return path.join(binariesDir, `aeroric-cli-${triple}${suffix}`);
}

/** 读一个 Mach-O 的架构（用它自己的头，不依赖 `file` 命令的输出格式）。 */
function machoArchitectures(file) {
  const result = run("/usr/bin/lipo", ["-archs", file], { capture: true });
  if (result.code !== 0) return null;
  return result.stdout.trim().split(/\s+/).filter(Boolean);
}

function tripleArch(triple) {
  if (triple.startsWith("aarch64")) return "arm64";
  if (triple.startsWith("x86_64")) return "x86_64";
  return null;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const triple = resolveTriple(args);
  const profileName = args.profile ?? profile;

  const source = builtHelperPath(triple, profileName);
  const target = bundledHelperPath(triple);

  console.log(`prepare-tauri-host: triple=${triple} profile=${profileName}`);
  console.log(`  source: ${path.relative(repoRoot, source)}`);
  console.log(`  target: ${path.relative(repoRoot, target)}`);

  if (args.dryRun) {
    console.log("prepare-tauri-host: --dry-run，不构建也不落盘。");
    return;
  }

  if (!existsSync(source)) {
    const build = run(
      "cargo",
      ["build", "--manifest-path", "core/Cargo.toml", "--locked", "-p", "aeroric-cli", "--target", triple, ...(profileName === "release" ? ["--release"] : [])],
    );
    if (build.code !== 0) fail(`构建 helper 失败（exit ${build.code}）`);
  }
  if (!existsSync(source)) fail(`构建之后仍然找不到 ${source}`);

  // 架构一致性：不一致时打包会成功、运行时 spawn 立刻失败——最难查的一类。
  const expectedArch = tripleArch(triple);
  if (expectedArch && triple.includes("darwin")) {
    const architectures = machoArchitectures(source);
    if (!architectures) fail(`无法读取 ${source} 的架构（lipo 失败）`);
    if (!architectures.includes(expectedArch)) {
      fail(
        `helper 架构与目标不符：目标 ${triple} 要 ${expectedArch}，实际 ${architectures.join(",")}`,
      );
    }
    console.log(`  arch check: ok (${architectures.join(",")})`);
  }

  mkdirSync(binariesDir, { recursive: true });
  copyFileSync(source, target);
  chmodSync(target, 0o755);
  const bytes = statSync(target).size;

  // 版本一致性：helper 与主程序读同一个 workspace 版本。
  const version = run(source, ["--version"], { capture: true });
  const cliVersion = (version.stdout || "").trim();
  console.log(`  bytes: ${bytes}`);
  console.log(`  version: ${cliVersion || "(unknown)"}`);
  console.log("prepare-tauri-host: 完成（生成物不进 git，由 .gitignore 挡住）。");
}

main();
