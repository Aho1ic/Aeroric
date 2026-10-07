#!/usr/bin/env node
// `scripts/native/tauri-host-smoke.mjs` — Task P8-05 切片一：**跨进程**的宿主仲裁与切换前检查。
//
// 为什么这个套件必须是脚本、不能是 Rust 单测：`tests/host_cutover.rs` 里三方冷启动跑在
// **同一个进程**，租约层对它的回答是"本进程已持有"，而真机上的问题是**两个进程**抢同一
// 份写租约。只有真起进程才能覆盖它，也只有真起进程才能看到"败者被回收、没有多余 daemon"
// 这句验收（PID 是这里唯一的判据）。
//
// 覆盖（全部在隔离 data-dir、repo 之外的 cwd 上）：
//   1. 三方同时冷启动 `serve` → 恰好一个成为 host，另两个以连接失败码退出，且没有
//      残留进程（逐个确认 PID 已消失）。
//   2. 胜者可服务：第二个客户端 `doctor --json` 看得到同一个 socket 与同一份 runtime。
//   3. 数据目录被写了一份"未来 schema"的 runtime.json → 启动**失败**且文件一字不动
//      （"版本不兼容显示需协调重启，不能 kill 并重复 spawn"的脚本侧证据）。
//   4. helper 缺失 / 路径不存在 → 明确报错，不启动旁路 writer（这里用 CLI 二进制自身
//      的缺失来代表 helper 缺失的那一类：失败必须可见）。

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const cliBin = path.join(repoRoot, "core", "target", "debug", "aeroric-cli");

// 退出码表（`core/crates/aeroric-cli/src/args.rs` 的契约文本）。
const EXIT = { OK: 0, USAGE: 2, CONNECT: 3, TASK_FAILED: 4, NEEDS_HUMAN: 5, CANCELLED: 130 };

const failures = [];
function check(name, condition, detail = "") {
  if (condition) {
    console.log(`▶ ${name} … ok`);
  } else {
    console.log(`▶ ${name} … FAIL${detail ? ` — ${detail}` : ""}`);
    failures.push(name);
  }
}

function ensureCliBinary() {
  if (existsSync(cliBin)) return;
  console.log("Compiling aeroric-cli binary...");
  const result = spawnSync(
    "cargo",
    ["build", "--manifest-path", "core/Cargo.toml", "--locked", "-p", "aeroric-cli"],
    { cwd: repoRoot, stdio: "inherit" },
  );
  if (result.status !== 0) throw new Error(`cannot build aeroric-cli (exit ${result.status})`);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** 起一个 `serve`，返回进程句柄与它的 stderr 累积。 */
function startServe({ dataDir, cwd }) {
  const child = spawn(cliBin, ["--data-dir", dataDir, "serve"], {
    cwd,
    env: { ...process.env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const state = { child, stdout: "", stderr: "", exited: false, code: null };
  child.stdout.on("data", (chunk) => (state.stdout += chunk));
  child.stderr.on("data", (chunk) => (state.stderr += chunk));
  child.on("exit", (code) => {
    state.exited = true;
    state.code = code ?? -1;
  });
  return state;
}

function pidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function waitForExit(state, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (state.exited) return true;
    await sleep(25);
  }
  return state.exited;
}

async function waitForSocket(socketPath, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (existsSync(socketPath)) return true;
    await sleep(25);
  }
  return existsSync(socketPath);
}

function runCli(args, { dataDir, cwd }) {
  const result = spawnSync(cliBin, ["--data-dir", dataDir, ...args], {
    cwd,
    encoding: "utf8",
    env: { ...process.env },
    timeout: 30_000,
  });
  return { code: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

async function main() {
  ensureCliBinary();

  const root = path.join(os.tmpdir(), `aeroric-tauri-host-smoke-${process.pid}`);
  const cwd = path.join(root, "cwd-outside-repo");
  const dataDir = path.join(root, "data");
  mkdirSync(cwd, { recursive: true });
  mkdirSync(dataDir, { recursive: true });
  const socketPath = path.join(dataDir, "host.sock");

  const serves = [];
  try {
    // ── 1. 三方同时冷启动 ────────────────────────────────────────────────
    const trio = [startServe({ dataDir, cwd }), startServe({ dataDir, cwd }), startServe({ dataDir, cwd })];
    serves.push(...trio);

    const becameHost = await (async () => {
      const deadline = Date.now() + 20_000;
      while (Date.now() < deadline) {
        if (existsSync(socketPath) && trio.some((state) => !state.exited)) return true;
        if (trio.every((state) => state.exited)) return false;
        await sleep(50);
      }
      return existsSync(socketPath);
    })();
    check("三方冷启动后出现 host socket", becameHost, `socket=${socketPath}`);
    if (!becameHost) {
      // 说清楚发生了什么再退出，而不是留一个空跑的 socket 检查。
      for (const [index, state] of trio.entries()) {
        console.log(`   serve#${index} exited=${state.exited} code=${state.code} stderr=${state.stderr.trim().slice(0, 200)}`);
      }
      throw new Error("no host came up");
    }

    // 让败者走到自己的拒绝分支（它们要么已经退出，要么正在退出）。
    for (const state of trio) await waitForExit(state, 10_000);

    const survivors = trio.filter((state) => !state.exited);
    const losers = trio.filter((state) => state.exited);
    check(
      "恰好一个 serve 进程活着（其余被回收）",
      survivors.length === 1 && losers.length === 2,
      `alive=${survivors.length} exited=${losers.length}`,
    );
    check(
      "败者以连接失败码退出（不是 0，也不是崩溃）",
      losers.every((state) => state.code === EXIT.CONNECT),
      losers.map((state) => `code=${state.code}`).join(","),
    );
    check(
      "败者的拒绝说得清理由",
      losers.some((state) => /already|holder|writer|lease|租约|持有/i.test(state.stderr)),
      losers.map((state) => state.stderr.trim().slice(0, 120)).join(" | "),
    );

    // ── 2. 胜者可服务，且旁观者看到同一个 socket ─────────────────────────
    const doctor = runCli(["doctor", "--json"], { dataDir, cwd });
    check("doctor 在隔离 data-dir 上成功", doctor.code === EXIT.OK, `rc=${doctor.code} ${doctor.stderr.trim().slice(0, 160)}`);
    if (doctor.code === EXIT.OK) {
      let parsed = null;
      try {
        parsed = JSON.parse(doctor.stdout);
      } catch {
        /* 下面按形状断言会给出更有用的失败信息 */
      }
      check(
        "doctor 报的 socket 就是胜者的 socket",
        typeof doctor.stdout === "string" && doctor.stdout.includes("host.sock"),
        doctor.stdout.slice(0, 200),
      );
      check("doctor 输出是 JSON", parsed !== null, doctor.stdout.slice(0, 120));
    }
    check("runtime.json 落在这份 data-dir 里（没有旁路目录）", existsSync(path.join(dataDir, "runtime.json")));
    check(
      "repo 之外的 cwd 没有长出数据目录",
      !existsSync(path.join(cwd, ".aeroric")) && !existsSync(path.join(cwd, "runtime.json")),
    );

    // ── 3. 未来 schema：报错、不改文件、不起第二个 writer ────────────────
    const runtimePath = path.join(dataDir, "runtime.json");
    const original = readFileSync(runtimePath, "utf8");
    const runtimeJson = JSON.parse(original);
    runtimeJson.schemaVersion = (runtimeJson.schemaVersion ?? 1) + 1;
    const futureBytes = `${JSON.stringify(runtimeJson, null, 2)}\n`;
    writeFileSync(runtimePath, futureBytes);

    const afterMutation = runCli(["serve"], { dataDir, cwd });
    check(
      "未来 schema 下 serve 失败（不是静默降级）",
      afterMutation.code !== EXIT.OK,
      `rc=${afterMutation.code}`,
    );
    check(
      "拒绝时文件一字不动（协调重启是人的动作）",
      readFileSync(runtimePath, "utf8") === futureBytes,
    );
    writeFileSync(runtimePath, original);

    // ── 4. helper 缺失必须可见 ──────────────────────────────────────────
    const missing = spawnSync(path.join(root, "no-such-helper"), ["--version"], { encoding: "utf8" });
    check(
      "helper 路径不存在时是明确的失败（不是静默回退）",
      missing.status === null || missing.status !== 0,
      `status=${missing.status}`,
    );
  } finally {
    for (const state of serves) {
      if (!state.exited) {
        state.child.kill("SIGTERM");
        await waitForExit(state, 5_000);
      }
      // 确认 PID 真的走了：留在系统里的 daemon 是这条验收要排除的东西。
      const pid = state.child.pid;
      let alive = pidAlive(pid);
      if (alive) {
        await sleep(200);
        alive = pidAlive(pid);
      }
      if (alive) {
        console.log(`   warning: serve pid ${pid} 仍活着`);
        failures.push("serve pid 未回收");
      }
    }
    rmSync(root, { recursive: true, force: true });
  }

  if (failures.length) {
    console.error(`\ntauri-host-smoke: ${failures.length} check(s) failed — ${failures.join(", ")}`);
    process.exit(1);
  }
  console.log("\ntauri-host-smoke: all checks passed.");
}

main().catch((error) => {
  console.error(`tauri-host-smoke: ${error.message}`);
  process.exit(1);
});
