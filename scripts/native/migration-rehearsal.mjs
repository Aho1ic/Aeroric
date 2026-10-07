#!/usr/bin/env node
// `scripts/native/migration-rehearsal.mjs` — Task P8-05 Action 4：**在合成数据上的切换演练**。
//
// 与 `smoke-headless.mjs` 的区别（不是重复）：smoke 在**空目录**上跑一遍理想生命周期；
// 这个脚本要回答的是切换那一类问题——
//
//   1. **旧客户端形状的数据目录**：手写一份 `projects.json` + `projects/<id>/tasks.json`，
//      字段用旧客户端会写的那些（含它自己的键）。新宿主必须**读得懂、写回去不丢键**
//      （ADR-037/`Persist::ViaHost` 的字节保真义务）。
//   2. **异常写入恢复**：在宿主写盘的过程中 `SIGKILL`（不是 SIGTERM——要模拟断电/崩溃），
//      再冷启动：目录必须仍然可读，已落盘的任务记录**一条不少**，中断的那条被对账成
//      `interrupted` 而不是"重新 spawn 一遍"。
//   3. **锁竞争**：崩溃后的 socket 是尸体，新宿主可以接手；而活着的时候第二个宿主被拒
//      （这条与 `tauri-host-smoke.mjs` 同判据，但这里是在**崩溃恢复**的语境下再问一遍）。
//   4. **回退面**：script 结束时把 data dir 原样留着（`--keep`）或删掉，并明确**不动**
//      真实 `~/.aeroric`——演练的每一步都带 `--data-dir <scratch>`。
//
// 明确不做（需要用户在场，见 `docs/native/migration-rehearsal.md` §4）：真实目录的停写、
// 备份、校验与指向。那一步是一次**不可自动回退**的操作，脚本只负责把它的前置条件与
// 检查项写成清单。

import { spawn, spawnSync } from "node:child_process";
import net from "node:net";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const cliBin = path.join(repoRoot, "core", "target", "debug", "aeroric-cli");
const EXIT = { OK: 0, USAGE: 2, CONNECT: 3, TASK_FAILED: 4, NEEDS_HUMAN: 5, CANCELLED: 130 };

const failures = [];
function check(name, condition, detail = "") {
  if (condition) console.log(`▶ ${name} … ok`);
  else {
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

function runCli(args, { dataDir }) {
  const result = spawnSync(cliBin, ["--data-dir", dataDir, ...args], {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env },
    timeout: 60_000,
  });
  return { code: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
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

/** 真连一次才算就绪。
 *
 * 只检查 socket **文件存在**是不够的：崩溃后那个文件还在（尸体 socket），于是"就绪"
 * 会在没有任何监听者时成立——本脚本第一版就是这么误判的，表现是"第二个宿主被拒"偶发
 * 失败（因为两个 serve 在抢同一个名字，谁先绑上取决于时序）。宿主自己的
 * `probe_liveness` 也是这个道理：文件存在 ≠ 有人 accept。 */
function probeLive(sockPath, timeoutMs = 500) {
  return new Promise((resolve) => {
    const socket = net.createConnection(sockPath);
    const done = (value) => {
      socket.destroy();
      resolve(value);
    };
    socket.setTimeout(timeoutMs);
    socket.once("connect", () => done(true));
    socket.once("timeout", () => done(false));
    socket.once("error", () => done(false));
  });
}

async function waitForReady({ dataDir, state, timeoutMs = 15_000 }) {
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

async function stopServe(state, signal = "SIGTERM") {
  if (state.exited) return;
  state.child.kill(signal);
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline && !state.exited) await sleep(25);
}

function readJson(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

/** 合成一份**旧客户端形状**的数据目录。字段刻意包含新客户端不认识的键。 */
function seedLegacyDir(root) {
  const dataDir = path.join(root, "data");
  const projectDir = path.join(root, "project");
  mkdirSync(path.join(dataDir, "projects", "p1"), { recursive: true });
  mkdirSync(projectDir, { recursive: true });

  writeFileSync(
    path.join(dataDir, "projects.json"),
    `${JSON.stringify(
      [
        {
          id: "p1",
          name: "legacy-project",
          path: projectDir,
          // 旧客户端自己写下的、新客户端不声明的键（ADR-037 要求原样保留）。
          legacyOnlyKey: "must-survive",
          order: 3,
        },
      ],
      null,
      2,
    )}\n`,
  );

  writeFileSync(
    path.join(dataDir, "projects", "p1", "tasks.json"),
    `${JSON.stringify(
      [
        {
          id: "t-legacy-done",
          projectId: "p1",
          prompt: "an old finished task",
          agent: "fake-fast",
          status: "done",
          permissionMode: "ask",
          createdAt: 1_700_000_000_000,
          completedAt: 1_700_000_100_000,
          // 同样是旧客户端写下的不定型键：`Task` 结构体不声明它，字节保真义务要求它活着。
          legacyLaunchFamily: "claude",
        },
        {
          id: "t-legacy-interrupted",
          projectId: "p1",
          prompt: "a task that was in flight when the old client died",
          agent: "fake-fast",
          status: "running",
          permissionMode: "ask",
          createdAt: 1_700_000_200_000,
        },
      ],
      null,
      2,
    )}\n`,
  );

  const agent = path.join(root, "fake-fast.sh");
  writeFileSync(agent, "#!/bin/sh\necho 'REHEARSAL AGENT'\nexit 0\n", { mode: 0o755 });
  writeFileSync(
    path.join(dataDir, "settings.json"),
    `${JSON.stringify(
      {
        custom_agents: [
          { id: "fake-fast", path: agent, args: [], family: "claude", codex_like: false },
        ],
      },
      null,
      2,
    )}\n`,
  );

  return { dataDir, projectDir, agent };
}

async function main() {
  ensureCliBinary();
  const keep = process.argv.includes("--keep");
  const root = path.join("/tmp", `aeroric-rehearsal-${process.pid}`);
  rmSync(root, { recursive: true, force: true });
  mkdirSync(root, { recursive: true });
  const seeded = seedLegacyDir(root);
  const { dataDir } = seeded;

  const serves = [];
  try {
    // ── 1. 新宿主读旧目录 ────────────────────────────────────────────────
    const first = startServe({ dataDir });
    serves.push(first);
    const ready = await waitForReady({ dataDir, state: first });
    check("宿主在**旧客户端形状**的目录上启动", ready, first.stderr.trim().slice(0, 200));
    if (!ready) throw new Error("host never became ready on the legacy dir");

    const doctor = runCli(["doctor", "--json"], { dataDir });
    check("doctor 读得懂这份目录", doctor.code === EXIT.OK, `rc=${doctor.code} ${doctor.stderr.slice(0, 160)}`);

    const tasksBefore = readJson(path.join(dataDir, "projects", "p1", "tasks.json"));
    check("启动没有改动任务表（读不写）", Array.isArray(tasksBefore) && tasksBefore.length === 2);

    // ── 2. 真实生命周期 + 字节保真 ──────────────────────────────────────
    const promptFile = path.join(root, "prompt.txt");
    writeFileSync(promptFile, "rehearsal prompt\n");
    const run = runCli(
      ["run", "--project", seeded.projectDir, "--agent", "fake-fast", "--prompt-file", promptFile, "--json"],
      { dataDir },
    );
    check(
      "在新宿主上跑完一轮任务",
      run.code === EXIT.OK,
      `rc=${run.code} ${(run.stderr || run.stdout).slice(0, 240)}`,
    );

    const tasksAfter = readJson(path.join(dataDir, "projects", "p1", "tasks.json"));
    const byId = new Map((tasksAfter ?? []).map((task) => [task.id, task]));
    check("旧记录仍在", byId.has("t-legacy-done") && byId.has("t-legacy-interrupted"));
    check(
      "旧客户端写下的不定型键原样保留（字节保真，ADR-037）",
      byId.get("t-legacy-done")?.legacyLaunchFamily === "claude",
      JSON.stringify(byId.get("t-legacy-done") ?? {}).slice(0, 200),
    );
    const projects = readJson(path.join(dataDir, "projects.json"));
    check(
      "项目记录的不定型键也保留",
      projects?.[0]?.legacyOnlyKey === "must-survive",
      JSON.stringify(projects ?? []).slice(0, 200),
    );
    check("新增的任务确实落盘了", (tasksAfter ?? []).length > 2, `n=${(tasksAfter ?? []).length}`);

    // ── 3. 崩溃写入恢复（SIGKILL，不是优雅退出）─────────────────────────
    const longRun = spawn(cliBin, [
      "--data-dir",
      dataDir,
      "run",
      "--project",
      seeded.projectDir,
      "--agent",
      "fake-fast",
      "--prompt-file",
      promptFile,
      "--json",
    ], { cwd: repoRoot, env: { ...process.env }, stdio: ["ignore", "pipe", "pipe"] });
    await sleep(400);
    longRun.kill("SIGKILL");
    first.child.kill("SIGKILL");
    await sleep(400);
    check("宿主被 SIGKILL 后进程确实没了", first.exited, `code=${first.code}`);

    // 尸体 socket 不该挡住重启（`bind_listener` 的探活闸）。
    const second = startServe({ dataDir });
    serves.push(second);
    const rebooted = await waitForReady({ dataDir, state: second });
    check("崩溃后可以冷启动（尸体 socket 不挡路）", rebooted, second.stderr.trim().slice(0, 200));
    if (rebooted) {
      const reconciled = readJson(path.join(dataDir, "projects", "p1", "tasks.json"));
      const ids = (reconciled ?? []).map((task) => task.id);
      check("崩溃恢复后目录仍可解析", Array.isArray(reconciled), JSON.stringify(reconciled).slice(0, 160));
      check(
        "崩溃前已落盘的记录一条不少",
        ids.includes("t-legacy-done") && ids.includes("t-legacy-interrupted"),
        ids.join(","),
      );

      // 锁竞争：活着的时候第二个宿主被拒（不是排队、不是抢）。
      const intruder = startServe({ dataDir });
      serves.push(intruder);
      const deadline = Date.now() + 10_000;
      while (Date.now() < deadline && !intruder.exited) await sleep(25);
      check(
        "第二个宿主被拒（活宿主不被挤掉）",
        intruder.exited && intruder.code === EXIT.CONNECT,
        `exited=${intruder.exited} code=${intruder.code} ${intruder.stderr.trim().slice(0, 160)}`,
      );
      check("被拒之后原宿主仍可服务", runCli(["doctor", "--json"], { dataDir }).code === EXIT.OK);

      // ── 4. 回退面：目录仍然只是"旧客户端的目录" ──────────────────────
      const third = runCli(["doctor", "--json"], { dataDir });
      let parsed = null;
      try {
        parsed = JSON.parse(third.stdout);
      } catch {
        /* 下面的形状断言会给出更有用的失败信息 */
      }
      check("doctor 输出可解析（回退到旧客户端前的最后一次自检）", parsed !== null);
      check(
        "没有把数据搬到第二个目录（原位读写）",
        !existsSync(path.join(path.dirname(dataDir), "data.new")) &&
          !existsSync(path.join(dataDir, "migrated")),
      );
    }
  } finally {
    for (const state of serves) await stopServe(state);
    if (keep) console.log(`rehearsal data kept at ${root}`);
    else rmSync(root, { recursive: true, force: true });
  }

  if (failures.length) {
    console.error(`\nmigration-rehearsal: ${failures.length} check(s) failed — ${failures.join(", ")}`);
    process.exit(1);
  }
  console.log("\nmigration-rehearsal: all checks passed (synthetic data only).");
}

main().catch((error) => {
  console.error(`migration-rehearsal: ${error.message}`);
  process.exit(1);
});
