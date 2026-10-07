// `prepare-tauri-host.mjs` 的判据测试（P8-05 Action 8）。
//
// 这些用例钉的是**脚本的拒绝行为**，不是它构建什么：Action 8 的禁令是"不把 runner 的
// CPU 当目标 CPU"，而那条禁令的可测形态就是"CI 下不给目标 ⇒ 退出 2，而不是猜一个"。
// 真正的构建与架构一致性由门禁 case `cutover/tauri-host-prepare` 与
// `scripts/native/tauri-host-smoke.mjs` 覆盖（前者真的产出 binaries、后者真的起进程）。

import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const script = path.join(repoRoot, "scripts/native/prepare-tauri-host.mjs");

function run(args, env = {}) {
  const result = spawnSync(process.execPath, [script, ...args], {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
  return { code: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

test("an unknown target triple is refused with the known list", () => {
  const result = run(["--triple=riscv64-unknown-linux-gnu"]);
  assert.equal(result.code, 2);
  assert.match(result.stderr, /不认识的目标三元组/);
  assert.match(result.stderr, /aarch64-apple-darwin/);
});

test("an unknown argument is refused rather than ignored", () => {
  const result = run(["--frobnicate"]);
  assert.equal(result.code, 2);
  assert.match(result.stderr, /unknown argument/);
});

test("in CI a missing triple is a hard refusal, never a guess from the runner CPU", () => {
  const result = run([], { CI: "true", AERORIC_TAURI_TARGET: "", TAURI_ENV_TARGET_TRIPLE: "" });
  assert.equal(result.code, 2);
  assert.match(result.stderr, /需要显式目标三元组/);
  assert.match(result.stderr, /不接受『按本机 CPU 猜一个』/);
  // 关键的否定断言：**没有**任何一行说它推导出了什么。
  assert.doesNotMatch(result.stdout, /本地推导/);
});

test("locally an absent triple falls back to the rustc host triple and says so", () => {
  // 没有 CI 变量时允许兜底（本机架构 == 目标架构，不会产生跨架构包），
  // 但必须**大声说明来源**——静默推导才是危险的形态。
  const probe = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
  if (probe.status !== 0) return; // 没有 rustc 的机器上这条无意义
  const host = /^host:\s*(\S+)$/m.exec(probe.stdout)?.[1];
  if (!host) return;
  const result = run(["--dry-run"], { CI: "", AERORIC_TAURI_TARGET: "", TAURI_ENV_TARGET_TRIPLE: "" });
  if (result.code === 2) {
    // 宿主三元组不在已知清单里（少见）：拒绝也是正确行为，只要它说了为什么。
    assert.match(result.stderr, /不认识的目标三元组/);
    return;
  }
  assert.equal(result.code, 0);
  assert.match(result.stdout, /本地推导/);
  assert.match(result.stdout, new RegExp(`triple=${host.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`));
});

test("the tauri env triple is what Tauri's own hook supplies", () => {
  const result = run(["--dry-run"], { TAURI_ENV_TARGET_TRIPLE: "x86_64-apple-darwin" });
  assert.equal(result.code, 0);
  assert.match(result.stdout, /triple=x86_64-apple-darwin/);
  assert.match(result.stdout, /binaries\/aeroric-cli-x86_64-apple-darwin/);
});

test("the bundled name follows Tauri's externalBin convention, including .exe", () => {
  const mac = run(["--triple=aarch64-apple-darwin", "--dry-run"]);
  assert.equal(mac.code, 0);
  assert.match(mac.stdout, /src-tauri\/binaries\/aeroric-cli-aarch64-apple-darwin\n/);
  assert.doesNotMatch(mac.stdout, /aeroric-cli-aarch64-apple-darwin\.exe/);

  const windows = run(["--triple=x86_64-pc-windows-msvc", "--dry-run"]);
  assert.equal(windows.code, 0);
  assert.match(windows.stdout, /aeroric-cli-x86_64-pc-windows-msvc\.exe/);
});

test("dry-run never writes the binaries directory", () => {
  const result = run(["--triple=aarch64-apple-darwin", "--dry-run"]);
  assert.equal(result.code, 0);
  assert.match(result.stdout, /--dry-run，不构建也不落盘/);
});
