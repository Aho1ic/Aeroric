#!/usr/bin/env node
// P8-05 移交项(P3-03 Q5,用户 2026-09-27/28 确认照原样):宿主终端控制租约的
// **单通道**计数守卫——「同一时刻只有一个控制者」在宿主侧的机器形状。
//
// 为什么需要另一个脚本:`host-dispatch`/`terminal-core` 的门禁断"形状在不在"
// (absent 谓词),断不了"这个面上到底有几条 write 通道"。而运行时的真 PTY 用例
// (`terminal_lease.rs` 四条)防的是行为错,防不了"将来有人再加第二个无闸写入口"。
// 计数归计数脚本,照 `terminal-invariants.mjs` 的先例(纯函数 + 配套负例 +
// `kind:"gate"`/`kind:"test"` 两条 case)。
//
// 口径与 D12 同源:命中必须先过 `codeOnly()`(注释与字符串里的词不算),语料读不到
// 时必须报错,不得把"没读到"当成"没有"。
//
// 事实基线(2026-10-09 实测):宿主唯一的 PTY 字节写与 resize 路径是
// `CliPtyExecutor::{input, resize}`(`core/crates/aeroric-host/src/executor.rs`),
// 两处都从同一张 per-task `ControlLease` 表 `acquire → write_gate → 变更 → release`;
// `task.input`/`task.resize`(services.rs)是仅有的两个 RPC 入口;
// `kill_run` 读 `master_for` 是为 `killpg`(进程杀路径,不是控制权写,如实记账)。
//
// 范围边界:**legacy Tauri 不在语料内**——`src-tauri/src/pty.rs` 的
// `send_input`/`resize_pty` cutover 前不取租约是 INV-04 的如实记账(文档),
// 本守卫量的是新宿主(host + CLI)两面。

import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { codeOnly, enclosingFunction, rustFiles } from "./adapter-invariants.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const HOST_DIR = "core/crates/aeroric-host/src";
const CLI_DIR = "core/crates/aeroric-cli/src";
const CORE_LEASE_FILE = "core/crates/aeroric-core/src/terminal/lease.rs";
const CHANNEL_FILE = "core/crates/aeroric-host/src/executor.rs";
const SERVICES_FILE = "core/crates/aeroric-host/src/services.rs";
const REMOTE_FILE = "core/crates/aeroric-host/src/remote_headless.rs";
const THE_FUNCTIONS = ["input", "resize"];

const GATE_CALL = /(?<!\w)write_gate\s*\(/g;
const GATE_DEFINITION = /(?<!\w)pub\s+fn\s+write_gate\s*\(/g;
const ACQUIRE = /(?<!\w)lease\.acquire\s*\(/g;
const RELEASE = /(?<!\w)lease\.release\s*\(/g;
const PTY_WRITER_SINK = /(?<!\w)pty_writers\b/g;
const MASTER_SINK = /(?<!\w)master_for\s*\(/g;
const PTY_WRITE = /(?<!\w)write_all\s*\(/g;
const PTY_RESIZE = /(?<!\w)\.resize\s*\(/g;
const EXECUTOR_INPUT = /(?<!\w)executor\.input\s*\(/g;
const EXECUTOR_RESIZE = /(?<!\w)executor\.resize\s*\(/g;

/** 逐行收集命中:返回 `[{line, fn}]`(行号 1 起,fn 为 enclosingFunction 名)。一行多处命中各计一次。 */
function hits(file, regex) {
  const lines = codeOnly(file.text).split("\n");
  const out = [];
  lines.forEach((line, index) => {
    regex.lastIndex = 0;
    let match;
    while ((match = regex.exec(line)) !== null) {
      out.push({ line: index + 1, fn: enclosingFunction(lines, index) });
    }
  });
  return out;
}

const siteLabel = (site) => `${site.file}:${site.line}(${site.fn ?? "(no fn)"})`;

/**
 * 单通道判定的唯一入口。`hostFiles` / `cliFiles` / `coreFiles` 都是
 * `[{path, text}]`——内容调用方给,负例直接喂字符串,不需要临时 git 树
 * (照 `terminal-invariants.test.mjs` 的口径)。
 */
export function leaseChannelProblems(hostFiles, cliFiles, coreFiles) {
  const problems = [];
  const all = [...hostFiles, ...cliFiles];

  if (hostFiles.length === 0) {
    problems.push(`${HOST_DIR}: no host .rs file was read — an unread corpus is not absence`);
  }
  if (cliFiles.length === 0) {
    problems.push(`${CLI_DIR}: no CLI .rs file was read — an unread corpus is not absence`);
  }
  const leaseFile = coreFiles.find((file) => file.path === CORE_LEASE_FILE);
  if (!leaseFile) {
    problems.push(`${CORE_LEASE_FILE}: lease source was not read — refusing to certify a count against an empty corpus`);
  }

  if (all.length === 0) return problems;

  // C1:write_gate 调用点恰 2,都在 executor.rs,enclosing fn = {input, resize}。
  const gateSites = [];
  for (const file of all) {
    const lines = codeOnly(file.text).split("\n");
    lines.forEach((line, index) => {
      GATE_CALL.lastIndex = 0;
      if (!GATE_CALL.test(line)) return;
      GATE_DEFINITION.lastIndex = 0;
      if (GATE_DEFINITION.test(line)) return;
      gateSites.push({ file: file.path, line: index + 1, fn: enclosingFunction(lines, index) });
    });
  }
  if (gateSites.length !== 2) {
    problems.push(
      `the host PTY channel must pass the core gate exactly 2 times (input + resize), found ${gateSites.length} (${gateSites.map(siteLabel).join(", ") || "none — byte writes now bypass the lease"})`,
    );
  } else {
    for (const site of gateSites) {
      if (site.file !== CHANNEL_FILE) {
        problems.push(`the gate sites must both live in ${CHANNEL_FILE}, found one in ${site.file}:${site.line}`);
      }
    }
    const fns = gateSites.map((s) => s.fn).sort();
    if (fns.join(",") !== THE_FUNCTIONS.join(",")) {
      problems.push(`the gate must be called from exactly the channel functions ${THE_FUNCTIONS.join(" and ")} (one each), found ${fns.map((f) => f ?? "(no fn)").join(", ")}`);
    }
  }

  // C2:acquire/release 各恰 2,每通道 fn 各一。
  for (const [name, regex] of [["acquire", ACQUIRE], ["release", RELEASE]]) {
    const siteHits = [];
    for (const file of all) {
      for (const hit of hits(file, regex)) siteHits.push({ file: file.path, ...hit });
    }
    if (siteHits.length !== 2) {
      problems.push(`lease.${name} must appear exactly once per channel fn (2 total), found ${siteHits.length} (${siteHits.map(siteLabel).join(", ") || "none"})`);
    } else {
      const fns = siteHits.map((s) => s.fn).sort();
      if (fns.join(",") !== THE_FUNCTIONS.join(",")) {
        problems.push(`lease.${name} must pair with the same two channel fns, found ${fns.map((f) => f ?? "(no fn)").join(", ")}`);
      }
    }
  }

  // C3:唯一字节写入 sink 与 master 消费者(恰 input 用 pty_writers;resize + kill_run 用 master_for)。
  const writerHits = [];
  const masterHits = [];
  for (const file of all) {
    for (const hit of hits(file, PTY_WRITER_SINK)) writerHits.push({ file: file.path, ...hit });
    for (const hit of hits(file, MASTER_SINK)) masterHits.push({ file: file.path, ...hit });
  }
  if (writerHits.length !== 1 || writerHits[0]?.fn !== "input") {
    problems.push(`pty_writers must be read exactly once, inside input (the one byte sink), found ${writerHits.length} (${writerHits.map(siteLabel).join(", ") || "none"})`);
  }
  const masterFns = masterHits.map((s) => s.fn).filter((f) => f).sort();
  if (masterHits.length !== 2 || masterFns.join(",") !== "kill_run,resize") {
    problems.push(`master_for must be used exactly twice — resize (gated) and kill_run (killpg, not a control write) — found ${masterHits.length} (${masterHits.map(siteLabel).join(", ") || "none"})`);
  }

  // C4:channel 文件内不得有无闸变更——write_all 恰在 input、.resize( 恰在 resize。
  const channel = all.find((file) => file.path === CHANNEL_FILE);
  if (channel) {
    const writeHits = hits(channel, PTY_WRITE);
    if (writeHits.length !== 1 || writeHits[0].fn !== "input") {
      problems.push(`the only pty byte write inside ${CHANNEL_FILE} must be the gated write_all in input, found ${writeHits.length} (${writeHits.map((s) => `${s.line}:${s.fn ?? "(no fn)"}`).join(", ") || "none"})`);
    }
    const resizeHits = hits(channel, PTY_RESIZE);
    if (resizeHits.length !== 1 || resizeHits[0].fn !== "resize") {
      problems.push(`the only master resize inside ${CHANNEL_FILE} must be the gated .resize( in resize, found ${resizeHits.length} (${resizeHits.map((s) => `${s.line}:${s.fn ?? "(no fn)"}`).join(", ") || "none"})`);
    }
  }

  // C5:唯一 RPC 漏斗——任务输入恰好两条(services.rs 的 task.input 处理器、
  // remote_headless.rs 的 remote task_input),resize 恰好一条(services.rs 的
  // task.resize 处理器);全部都汇入同一条 gated channel。
  const funnelInput = [];
  const funnelResize = [];
  for (const file of hostFiles) {
    for (const hit of hits(file, EXECUTOR_INPUT)) funnelInput.push({ file: file.path, ...hit });
    for (const hit of hits(file, EXECUTOR_RESIZE)) funnelResize.push({ file: file.path, ...hit });
  }
  const inputServices = funnelInput.filter((s) => s.file === SERVICES_FILE).length;
  const inputRemote = funnelInput.filter((s) => s.file === REMOTE_FILE).length;
  const inputOther = funnelInput.filter((s) => s.file !== SERVICES_FILE && s.file !== REMOTE_FILE).length;
  const resizeServices = funnelResize.filter((s) => s.file === SERVICES_FILE).length;
  const resizeOther = funnelResize.length - resizeServices;
  if (
    inputServices !== 1 ||
    inputRemote !== 1 ||
    inputOther !== 0 ||
    resizeServices !== 1 ||
    resizeOther !== 0
  ) {
    problems.push(
      `the host funnel must be exactly: executor.input in services.rs (task.input) × 1 + in remote_headless.rs (remote task_input) × 1, executor.resize in services.rs × 1 — found ${inputServices} services-input / ${inputRemote} remote-input / ${inputOther} other-input / ${resizeServices} services-resize / ${resizeOther} other-resize`,
    );
  }

  // C6:闸定义只在 core,且 core 恰定义一次(宿主只有调用面)。
  let hostDefinitions = 0;
  for (const file of all) {
    GATE_DEFINITION.lastIndex = 0;
    if (GATE_DEFINITION.test(codeOnly(file.text))) hostDefinitions += 1;
  }
  if (hostDefinitions !== 0) {
    problems.push(`write_gate must be defined in core only, found ${hostDefinitions} definition(s) outside ${CORE_LEASE_FILE}`);
  }
  if (leaseFile) {
    const coreDefs = hits(leaseFile, GATE_DEFINITION);
    if (coreDefs.length !== 1) {
      problems.push(`core must define pub fn write_gate exactly once, found ${coreDefs.length}`);
    }
  }

  return problems;
}

export function loadLeaseCorpus(root = repoRoot) {
  const hostFiles = rustFiles(HOST_DIR, root).map((relative) => ({
    path: relative,
    text: readFileSync(path.join(root, relative), "utf8"),
  }));
  const cliFiles = rustFiles(CLI_DIR, root).map((relative) => ({
    path: relative,
    text: readFileSync(path.join(root, relative), "utf8"),
  }));
  const coreFiles = [];
  try {
    coreFiles.push({
      path: CORE_LEASE_FILE,
      text: readFileSync(path.join(root, CORE_LEASE_FILE), "utf8"),
    });
  } catch {
    // 读不到就留空:problems 里会把它报成"未读到语料",绝不静默通过。
  }
  return { hostFiles, cliFiles, coreFiles };
}

function main() {
  const { hostFiles, cliFiles, coreFiles } = loadLeaseCorpus();
  const problems = leaseChannelProblems(hostFiles, cliFiles, coreFiles);
  if (problems.length) {
    console.error(`terminal-lease-invariants: ${problems.length} problem(s)`);
    for (const problem of problems) console.error(`  - ${problem}`);
    process.exit(1);
  }
  console.log(`terminal-lease-invariants: single-channel ok (${hostFiles.length} host + ${cliFiles.length} cli files, core lease read)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();