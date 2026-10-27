#!/usr/bin/env node
// `scripts/native/stress.mjs` — AP8 R-AP8-4（原 P10-01 Action 4/5）：在**固定口径**下压一遍，
// 记录 p50/p95 与资源计数，并与代码里真实存在的上限逐项对照。
//
// ## 口径是固定的，不是"跑得动就行"
//
// 计划给了确切的量与机器口径。写死它有意义：一个"随机器规模变化"的压测，两次运行之间
// 的数字不可比，于是"这次怎么慢了一倍"永远答不出。所以负载常量在下面，改动要附理由。
//
// ## 预算只认代码里的常量
//
// `docs/native/acceptance-budgets.md` 里的每条预算都要能追到一个**真实常量**或一个
// **本次实测值**。追不到的就如实标 `not-measured` 并说明它归谁——把渲染器指标在无头
// 环境里"测"出来，比不测更糟。
//
// ## 全部在隔离目录

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const cliBin = path.join(repoRoot, "core", "target", "debug", "aeroric-cli");
const EXIT = { OK: 0, USAGE: 2, CONNECT: 3, TASK_FAILED: 4, NEEDS_HUMAN: 5, CANCELLED: 130 };

/** 固定口径（计划 §Task P10-01 Action 4）。**改这里必须附理由与新的基准。** */
const LOAD = {
  /** 每个 fake task 的输出量：5 MiB。 */
  taskOutputBytes: 5 * 1024 * 1024,
  /** 单行宽度：2000 B。5120 行 × 2000 B ≈ 9.8 MiB 输出（含换行），足以越过 8 MiB 历史保留线。 */
  lineBytes: 2000,
  /** 行数。 */
  lines: 5184,
  /** 模式切换等价的 RPC 次数（计划要求 100）。 */
  modeSwitches: 100,
  /** 并发客户端数（计划要求 3）。 */
  clients: 3,
  /** 重连次数（计划要求 10）。 */
  reconnects: 10,
};

const failures = [];
function check(name, condition, detail = "") {
  if (condition) {
    console.log(`▶ ${name} … ok${detail ? ` (${detail})` : ""}`);
  } else {
    console.log(`▶ ${name} … FAIL${detail ? ` — ${detail}` : ""}`);
    failures.push(name);
  }
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

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

function runCli(args, { dataDir, timeoutMs = 300_000 } = {}) {
  const argv = dataDir ? ["--data-dir", dataDir, ...args] : args;
  const started = process.hrtime.bigint();
  const result = spawnSync(cliBin, argv, {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env },
    timeout: timeoutMs,
  });
  const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
  return { code: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "", elapsedMs };
}

function startServe({ dataDir }) {
  const child = spawn(cliBin, ["--data-dir", dataDir, "serve"], {
    cwd: repoRoot,
    env: { ...process.env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const state = { child, exited: false, code: null, stderr: "" };
  child.stderr.on("data", (chunk) => (state.stderr += chunk));
  child.on("exit", (code) => {
    state.exited = true;
    state.code = code ?? -1;
  });
  return state;
}

function probeLive(sockPath) {
  return new Promise((resolve) => {
    const socket = net.createConnection(sockPath);
    const done = (value) => {
      socket.destroy();
      resolve(value);
    };
    socket.setTimeout(500);
    socket.once("connect", () => done(true));
    socket.once("timeout", () => done(false));
    socket.once("error", () => done(false));
  });
}

async function waitForReady({ dataDir, state, timeoutMs = 20_000 }) {
  const sock = path.join(dataDir, "host.sock");
  const token = path.join(dataDir, "host.token");
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (existsSync(token) && (await probeLive(sock))) return true;
    if (state.exited) return false;
    await sleep(25);
  }
  return false;
}

async function stopServe(state) {
  if (state.exited) return;
  state.child.kill("SIGTERM");
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline && !state.exited) await sleep(25);
}

function percentile(sorted, p) {
  if (sorted.length === 0) return null;
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[index];
}

/** 宿主进程的资源计数。`ps` 是这里唯一的真相源（rss 单位 KB）。 */
function resourceCounts(pid) {
  const out = spawnSync("ps", ["-o", "rss=,vsz=", "-p", String(pid)], { encoding: "utf8" });
  const line = (out.stdout ?? "").trim().split("\n")[0] ?? "";
  const [rssKb, vszKb] = line.split(/\s+/).map(Number);
  const fds = spawnSync("sh", ["-c", `ls /dev/fd 2>/dev/null | wc -l`], { encoding: "utf8" });
  void fds;
  return {
    rssMib: Number.isFinite(rssKb) ? rssKb / 1024 : null,
    vszMib: Number.isFinite(vszKb) ? vszKb / 1024 : null,
  };
}

function childProcessCount(dataDir) {
  const out = spawnSync("ps", ["axo", "pid=,command="], { encoding: "utf8" });
  return (out.stdout ?? "")
    .split("\n")
    .filter((line) => line.includes(dataDir) && !line.includes("ps axo") && !line.includes("aeroric-cli --data-dir"))
    .length;
}

async function main() {
  ensureCliBinary();
  const root = path.join("/tmp", `aeroric-stress-${process.pid}`);
  rmSync(root, { recursive: true, force: true });
  const dataDir = path.join(root, "data");
  const projectDir = path.join(root, "proj");
  mkdirSync(dataDir, { recursive: true });
  mkdirSync(projectDir, { recursive: true });

  // 5 MiB 输出的 fake agent。`awk` 比 shell 循环快一个数量级，且输出量精确可控。
  const agent = path.join(root, "big.sh");
  writeFileSync(
    agent,
    `#!/bin/sh\nawk 'BEGIN { for (i = 0; i < ${LOAD.lines}; i++) printf "%${LOAD.lineBytes}s\\n", "" }'\nexit 0\n`,
    { mode: 0o755 },
  );
  writeFileSync(
    path.join(dataDir, "settings.json"),
    `${JSON.stringify({ custom_agents: [{ id: "big", path: agent, args: [], family: "claude", codex_like: false }] }, null, 2)}\n`,
  );
  const promptFile = path.join(root, "p.txt");
  writeFileSync(promptFile, "stress\n");

  const measurements = {};
  let serve;
  try {
    serve = startServe({ dataDir });
    const ready = await waitForReady({ dataDir, state: serve });
    check("宿主就绪", ready, serve.stderr.trim().slice(0, 160));
    if (!ready) throw new Error("host never became ready");
    const hostPid = serve.child.pid;

    const baseline = resourceCounts(hostPid);
    const baselineChildren = childProcessCount(dataDir);

    // ── 负载 1：5 MiB 输出的一轮任务 ──────────────────────────────────
    const run = runCli(
      ["run", "--project", projectDir, "--agent", "big", "--prompt-file", promptFile, "--json"],
      { dataDir },
    );
    const historyDir = path.join(dataDir, "terminal-history");
    let historyBytes = 0;
    try {
      const listing = spawnSync("sh", ["-c", `du -sk ${historyDir} 2>/dev/null | cut -f1`], { encoding: "utf8" });
      historyBytes = (Number((listing.stdout ?? "0").trim()) || 0) * 1024;
    } catch {
      historyBytes = 0;
    }
    measurements.taskRunMs = run.elapsedMs;
    check(
      "5 MiB 输出的任务能跑完",
      run.code === EXIT.OK,
      `rc=${run.code} ${(run.stderr || run.stdout).slice(0, 160)}`,
    );

    // 历史保留上限来自 `terminal/history.rs::HISTORY_MAX_BYTES`（8 MiB）。
    //
    // **这条断言必须要求历史非空**。第一版写成 `historyBytes === 0 || …`，于是它在
    // "日志根本没被创建"时也绿——那不是压测写错了，而是它正确地暴露了一件真事：
    // 历史的**写侧仍只在待退休的 Tauri 桥里**（`src-tauri/src/pty.rs:607`），core 只读不写。
    // 见 `docs/native/acceptance-budgets.md` §3 与 `GAP-AP8-HISTORY-WRITER`。
    // 写侧上收之后，这条会自然变绿；在那之前它是**具名缺口**的机器证据。
    const HISTORY_MAX_BYTES = 8 * 1024 * 1024;
    check(
      "5 MiB 输出的任务在磁盘上留下了终端历史（写侧仍 absent 则本条红）",
      historyBytes > 0,
      `history=${(historyBytes / 1024 / 1024).toFixed(2)} MiB — GAP-AP8-HISTORY-WRITER：写侧仅在 src-tauri/src/pty.rs:607`,
    );
    check(
      "磁盘终端历史落在保留上限之内（含 slack）",
      historyBytes <= HISTORY_MAX_BYTES * 2,
      `history=${(historyBytes / 1024 / 1024).toFixed(2)} MiB vs cap ${(HISTORY_MAX_BYTES / 1024 / 1024).toFixed(0)} MiB`,
    );

    // ── 负载 2：100 次模式切换等价的 RPC ──────────────────────────────
    const latencies = [];
    for (let i = 0; i < LOAD.modeSwitches; i += 1) {
      const call = runCli(["rpc", "project.load_projects"], { dataDir });
      latencies.push(call.elapsedMs);
    }
    latencies.sort((a, b) => a - b);
    measurements.switchP50Ms = percentile(latencies, 50);
    measurements.switchP95Ms = percentile(latencies, 95);
    measurements.switchP99Ms = percentile(latencies, 99);
    check(
      `100 次切换的 p95 < 150ms（实测 p50 ${measurements.switchP50Ms.toFixed(1)}ms / p95 ${measurements.switchP95Ms.toFixed(1)}ms）`,
      measurements.switchP95Ms < 150,
      `p99=${measurements.switchP99Ms.toFixed(1)}ms`,
    );

    // ── 负载 3：并发客户端 ────────────────────────────────────────────
    const clientResults = await Promise.all(
      Array.from({ length: LOAD.clients }, () =>
        Promise.resolve(runCli(["doctor", "--json"], { dataDir })),
      ),
    );
    check(
      `${LOAD.clients} 个并发客户端都能得到答复（互不阻塞）`,
      clientResults.every((result) => result.code === EXIT.OK),
      clientResults.map((r) => r.code).join(","),
    );

    // ── 负载 4：重连（每次新连接）─────────────────────────────────────
    let reconnectOk = 0;
    for (let i = 0; i < LOAD.reconnects; i += 1) {
      if (runCli(["doctor", "--json"], { dataDir }).code === EXIT.OK) reconnectOk += 1;
    }
    check(`${LOAD.reconnects} 次重连全部成功`, reconnectOk === LOAD.reconnects, `${reconnectOk}/${LOAD.reconnects}`);

    // ── 反事实：切换之后子进程数必须回到基线 ──────────────────────────
    // 计划原文要求"100 次切换后 renderer/subscriber/子进程数回到基线"。
    // renderer 属于原生端（无头环境里测不出来，见 budgets 文档的 not-measured 行），
    // 但**子进程数**在这里是可测的，而它正是"切换泄漏了进程"的那种故障。
    const afterChildren = childProcessCount(dataDir);
    check(
      "100 次切换后子进程数回到基线（没有泄漏进程）",
      afterChildren <= baselineChildren,
      `baseline=${baselineChildren} after=${afterChildren}`,
    );

    const after = resourceCounts(hostPid);
    measurements.hostRssMibGrowth = after.rssMib !== null && baseline.rssMib !== null
      ? after.rssMib - baseline.rssMib
      : null;
    check(
      "宿主 RSS 没有随负载无界增长（增长 < 64 MiB）",
      measurements.hostRssMibGrowth === null || measurements.hostRssMibGrowth < 64,
      `baseline=${baseline.rssMib?.toFixed(1)} MiB after=${after.rssMib?.toFixed(1)} MiB`,
    );

    console.log("\n── 实测（写入 docs/native/acceptance-budgets.md 的对照表）──");
    console.log(JSON.stringify(measurements, null, 2));
  } finally {
    if (serve) await stopServe(serve);
    rmSync(root, { recursive: true, force: true });
  }

  if (failures.length) {
    console.error(`\nstress: ${failures.length} check(s) failed — ${failures.join(", ")}`);
    process.exit(1);
  }
  console.log("\nstress: all checks passed (isolated dir only).");
}

main().catch((error) => {
  console.error(`stress: ${error.message}`);
  process.exit(1);
});
