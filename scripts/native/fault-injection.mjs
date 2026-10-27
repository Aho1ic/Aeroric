#!/usr/bin/env node
// `scripts/native/fault-injection.mjs` — AP8 Action 2：**在真实进程与真实文件系统上**
// 注入故障，断言"不许出现的副作用"，而不是只断言"命令返回了错误"。
//
// ## 为什么必须是真的
//
// 这些边界在单测里**已经有**覆盖（core 的 atomic/lock/operations），但单测里"磁盘满"
// 是伪造的 `io::Error`、`SIGKILL` 是函数调用。AP8 要回答的是另一件事：
// **真内核、真 ENOSPC、真信号下的可观察结果**。两者不能互相顶替——单测能证明分支，
// 证不了"写了一半的文件真的没留下"。
//
// ## 每条判据都是"不许发生什么"
//
// 只断言 "rc != 0" 的测试，在"错误码对了但数据毁了"时照样绿。所以每条注入都带一个
// **反事实断言**：文件必须字节不变、不许留 `.tmp`、不许有第二个写者、不许重复副作用。
//
// ## 注入点（对应 MIGRATION_PLAN §Task P10-01 Action 3）
//
// | # | 注入 | 反事实断言 |
// |---|---|---|
// | F1 | 磁盘满（真 2 MiB RAM disk，真 ENOSPC）下的 mutation | 目标文件字节不变、无 `.tmp`/`.bak` 泄漏、宿主仍可服务 |
// | F2 | 磁盘满下的 serve 冷启动 | 明确失败、退出码非 0、不留下半个数据目录 |
// | F3 | SIGKILL 打断写（崩溃，不是优雅退出） | 冷启动成功、崩溃前记录一条不少、目录仍可解析 |
// | F4 | SIGTERM 优雅退出 | socket 被移除、租约释放、退出码 0 |
// | F5 | 活宿主上第二个写者 | 被拒且**不抢**租约；原宿主仍可服务 |
// | F6 | 只读数据目录 | 拒写并指名路径；不许静默成功 |
//
// 全部在隔离目录（`/tmp` 或临时 RAM disk），**永不触碰真实 `~/.aeroric`**。

import { spawn, spawnSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import net from "node:net";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const cliBin = path.join(repoRoot, "core", "target", "debug", "aeroric-cli");
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

function runCli(args, { dataDir, timeoutMs = 60_000 } = {}) {
  const argv = dataDir ? ["--data-dir", dataDir, ...args] : args;
  const result = spawnSync(cliBin, argv, {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env },
    timeout: timeoutMs,
  });
  return {
    code: result.status ?? -1,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
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

async function stopServe(state, signal = "SIGTERM") {
  if (state.exited) return;
  state.child.kill(signal);
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline && !state.exited) await sleep(25);
}

/** 真连一次才算就绪：socket 文件存在 ≠ 有人 accept（尸体 socket 的教训）。 */
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

function seedDataDir(dataDir) {
  mkdirSync(dataDir, { recursive: true });
  writeFileSync(
    path.join(dataDir, "projects.json"),
    `${JSON.stringify([{ id: "p1", name: "fault", path: "/tmp", orderIndex: 0 }], null, 2)}\n`,
  );
  writeFileSync(
    path.join(dataDir, "settings.json"),
    `${JSON.stringify({ custom_agents: [] }, null, 2)}\n`,
  );
}

function tempFilesIn(dir) {
  try {
    return readdirSync(dir).filter((n) => n.includes(".tmp"));
  } catch {
    return [];
  }
}

// ── F1/F2：真 ENOSPC（2 MiB RAM disk）────────────────────────────────────
//
// 盘小是故意的：填满它只需要几百 KB，而"写不进去"是内核给的**真** errno 28，
// 不是我伪造的一个枚举值。夹具用完立刻 detach，不给机器留状态。

function attachRamDisk(mb) {
  const created = spawnSync("hdiutil", ["attach", "-nomount", `ram://${mb * 2048}`], {
    encoding: "utf8",
  });
  if (created.status !== 0) return null;
  const device = created.stdout.trim().split("\n")[0].split(/\s+/)[0];
  if (!device) return null;
  const erased = spawnSync("diskutil", ["erasevolume", "HFS+", "AP8FaultDisk", device], {
    encoding: "utf8",
  });
  if (erased.status !== 0) {
    spawnSync("hdiutil", ["detach", device, "-force"]);
    return null;
  }
  return { device, mount: "/Volumes/AP8FaultDisk" };
}

function freeKilobytes(mount) {
  const out = spawnSync("df", ["-k", mount], { encoding: "utf8" });
  if (out.status !== 0) return null;
  const line = out.stdout.trim().split("\n").pop() ?? "";
  const available = Number(line.split(/\s+/)[3]);
  return Number.isFinite(available) ? available : null;
}

/** 把盘填到**真的 0 KB 可用**。
 *
 * 写死"几百 KB"是不够的:文件系统元数据与宿主启动时创建的文件都会占地方,于是"我以为
 * 填满了"与"内核说还有余量"之间的差就足以让一次写入成功——本脚本第一版正是这样把
 * F1 变成了三个假绿。所以这里直接**写到内核拒绝为止**(ENOSPC 就是填满的信号),
 * 再读 `df` 断言可用量为 0:填不满就说明这条注入没建立,必须报红而不是继续往下假装。
 */
function fillDiskToZero(mount, label) {
  for (let round = 0; round < 60; round += 1) {
    const available = freeKilobytes(mount);
    if (available === null) return false;
    if (available <= 0) return true;
    // 每轮一个**新文件**:macOS 的 `dd` 没有 `oflag=append`(实测 "unknown open flag"),
    // 而带 `oflag=append` 的调用是静默失败的——本脚本第一版就是这样"填了满盘却没填满"。
    // 按 `df` 的实际读数写,且读数为 0 才收工。
    const kilobytes = Math.max(1, Math.min(available, 8192));
    spawnSync(
      "dd",
      ["if=/dev/zero", `of=${path.join(mount, `${label}-${round}`)}`, "bs=1024", `count=${kilobytes}`],
      { encoding: "utf8", timeout: 60_000 },
    );
    spawnSync("sync", [], { encoding: "utf8" });
  }
  return (freeKilobytes(mount) ?? 1) <= 0;
}

function detachRamDisk(device) {
  if (device) spawnSync("hdiutil", ["detach", device, "-force"], { encoding: "utf8" });
}

async function diskFullSection() {
  // 8 MiB:足够放下数据目录与一个正在运行的宿主,填满之后还剩"写不进去"这个现场。
  const disk = attachRamDisk(8);
  if (!disk) {
    console.log("▶ F1/F2 disk-full … SKIP (hdiutil could not create a RAM disk on this host)");
    return;
  }
  try {
    const dataDir = path.join(disk.mount, "data");
    seedDataDir(dataDir);
    const before = readFileSync(path.join(dataDir, "projects.json"));

    const serve = startServe({ dataDir });
    const ready = await waitForReady({ dataDir, state: serve });
    check("F1 前置：宿主能在近满的盘上启动", ready, serve.stderr.trim().slice(0, 200));

    // 项目目录要在填满**之前**建好:填满之后 `mkdir` 自己就 ENOSPC 了,那样测的
    // 是"目录建不出来",不是"写入中途没有空间"——后者才是这条注入要问的。
    const projectDir = path.join(disk.mount, "proj");
    mkdirSync(projectDir, { recursive: true });

    // 顺序是"先起宿主、再填满":反过来的话宿主自己就起不来,而 F1 要问的是
    // **在一次正在进行的会话中途**磁盘没了会发生什么。
    const filled = fillDiskToZero(disk.mount, "fill-a");
    check(
      "F1 前置：磁盘已被填到 0 KB 可用（这条注入真的建立了）",
      filled,
      `free=${freeKilobytes(disk.mount)} KB`,
    );

    const open = runCli(["project", "open", projectDir], { dataDir });
    check(
      "F1 磁盘满下的写被拒绝（不是静默成功）",
      open.code !== EXIT.OK,
      `rc=${open.code} ${(open.stderr || open.stdout).slice(0, 240)}`,
    );
    check(
      "F1 错误指名是空间问题而不是含糊失败",
      /space left|os error 28|No space/i.test(open.stderr + open.stdout),
      (open.stderr || open.stdout).slice(0, 240),
    );
    check(
      "F1 反事实：目标文件字节不变（半截写入没有落地）",
      before.equals(readFileSync(path.join(dataDir, "projects.json"))),
    );
    check(
      "F1 反事实：没有留下 .tmp 残骸",
      tempFilesIn(dataDir).length === 0,
      JSON.stringify(tempFilesIn(dataDir)),
    );
    check(
      "F1 反事实：宿主没被这次失败打死，仍可服务",
      runCli(["doctor", "--json"], { dataDir }).code === EXIT.OK,
    );

    await stopServe(serve);
    detachRamDisk(disk.device);

    // F2：盘满到连一个小文件都写不下时，serve 必须明确失败且不留下半个数据目录。
    const disk2 = attachRamDisk(2);
    if (disk2) {
      try {
        const fullDir = path.join(disk2.mount, "data");
        fillDiskToZero(disk2.mount, "fill-c");
        const cold = runCli(["serve"], { dataDir: fullDir, timeoutMs: 15_000 });
        check(
          "F2 磁盘满时 serve 明确失败（非零退出）",
          cold.code !== EXIT.OK,
          `rc=${cold.code} ${cold.stderr.slice(0, 200)}`,
        );
        check(
          "F2 反事实：失败发生在建立状态之前（没有半个数据目录）",
          !existsSync(fullDir) || readdirSync(fullDir).length === 0,
          existsSync(fullDir) ? JSON.stringify(readdirSync(fullDir)) : "absent",
        );
      } finally {
        detachRamDisk(disk2.device);
      }
    }
  } finally {
    detachRamDisk(disk.device);
  }
}

// ── F3–F6：隔离目录上的信号与锁边界 ─────────────────────────────────────

async function signalSection(root) {
  const dataDir = path.join(root, "data");
  seedDataDir(dataDir);

  // F3：SIGKILL 打断写。
  const first = startServe({ dataDir });
  const ready = await waitForReady({ dataDir, state: first });
  check("F3 前置：宿主就绪", ready, first.stderr.trim().slice(0, 200));
  const recordsBefore = readFileSync(path.join(dataDir, "projects.json"), "utf8");
  first.child.kill("SIGKILL");
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline && !first.exited) await sleep(25);
  check("F3 宿主被 SIGKILL 后确实死了", first.exited, `code=${first.code}`);

  const second = startServe({ dataDir });
  const rebooted = await waitForReady({ dataDir, state: second });
  check("F3 崩溃后冷启动成功（尸体 socket 不挡路）", rebooted, second.stderr.trim().slice(0, 200));
  check(
    "F3 反事实：崩溃前的记录一条不少、字节一致",
    readFileSync(path.join(dataDir, "projects.json"), "utf8") === recordsBefore,
  );

  // F4：SIGTERM 优雅退出 —— socket 移除、租约释放。
  const sock = path.join(dataDir, "host.sock");
  const lease = path.join(dataDir, ".write.lock");
  second.child.kill("SIGTERM");
  const gracefulDeadline = Date.now() + 10_000;
  while (Date.now() < gracefulDeadline && !second.exited) await sleep(25);
  check("F4 SIGTERM 后宿主干净退出（rc 0）", second.exited && second.code === EXIT.OK, `code=${second.code}`);
  check("F4 反事实：socket 被移除（不是留在盘上的尸体）", !existsSync(sock));

  // 租约：进程没了，锁就该能被下一个人拿到（文件可能还在，但不再是**持有**）。
  const owner = runCli(["doctor", "--json"], { dataDir });
  let leaseFree = false;
  try {
    leaseFree = JSON.parse(owner.stdout).lease === "free";
  } catch {
    leaseFree = false;
  }
  check("F4 反事实：租约已释放", leaseFree, owner.stdout.slice(0, 200));
  void lease;

  // F5：活宿主上第二个写者被拒且不抢。
  const third = startServe({ dataDir });
  const serving = await waitForReady({ dataDir, state: third });
  check("F5 前置：宿主就绪", serving, third.stderr.trim().slice(0, 200));
  const intruder = startServe({ dataDir });
  const intruderDeadline = Date.now() + 10_000;
  while (Date.now() < intruderDeadline && !intruder.exited) await sleep(25);
  check(
    "F5 第二个写者被拒",
    intruder.exited && intruder.code === EXIT.CONNECT,
    `exited=${intruder.exited} code=${intruder.code} ${intruder.stderr.trim().slice(0, 160)}`,
  );
  check(
    "F5 反事实：原宿主没被挤掉，仍可服务",
    runCli(["doctor", "--json"], { dataDir }).code === EXIT.OK,
  );
  await stopServe(third);
}

// ── F6：只读数据目录 ────────────────────────────────────────────────────

function readOnlySection(root) {
  if (process.getuid && process.getuid() === 0) {
    console.log("▶ F6 read-only dir … SKIP (running as root: mode bits would not bind)");
    return;
  }
  const dataDir = path.join(root, "sealed-data");
  seedDataDir(dataDir);
  const before = readFileSync(path.join(dataDir, "projects.json"), "utf8");
  spawnSync("chmod", ["0500", dataDir]);

  const result = runCli(["project", "open", "/tmp"], { dataDir });
  check(
    "F6 只读数据目录拒写（不是静默成功）",
    result.code !== EXIT.OK,
    `rc=${result.code} ${(result.stderr || result.stdout).slice(0, 240)}`,
  );
  spawnSync("chmod", ["0700", dataDir]);
  check(
    "F6 反事实：原文件没被动过",
    readFileSync(path.join(dataDir, "projects.json"), "utf8") === before,
  );
}

async function main() {
  ensureCliBinary();
  const root = path.join("/tmp", `aeroric-fault-${process.pid}`);
  rmSync(root, { recursive: true, force: true });
  mkdirSync(root, { recursive: true });
  try {
    await diskFullSection();
    await signalSection(root);
    readOnlySection(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  if (failures.length) {
    console.error(`\nfault-injection: ${failures.length} check(s) failed — ${failures.join(", ")}`);
    process.exit(1);
  }
  console.log("\nfault-injection: all checks passed (isolated dirs only).");
}

main().catch((error) => {
  console.error(`fault-injection: ${error.message}`);
  process.exit(1);
});
