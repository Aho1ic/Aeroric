#!/usr/bin/env node
// P3-04 第七片（Action 2/G03/G04）的源守卫：会话层 watcher 的 generation/停机形状。
//
// 为什么是脚本而不是（或不只是）测试：第七片改的是**接线形状**——裸 `thread::spawn`
// 进监督者、`_token` 形同虚设的闭包改成入口真查、Exit 块在关服务之前先静默 watcher。
// 这些用行为测试要造真 AppHandle，本阶段造不出（真实内核端到端属行政 B／P10-05）；
// 而监督者的取消语义与 generation 键表已由 core 的 `tests/runtime_lifecycle.rs` 与
// `registry.rs` 两代测试钉住。本脚本守住的是「明天有人把 `_token` 改回去、把
// `thread::spawn` 加回来、把 supervisor.shutdown() 挪位」这种形状回归——照
// `terminal-invariants.mjs` 的先例（纯函数 + 配套负例 + gate/test 两条 case）。
//
// 双口径（与 D12/补记一百二十二同源并互补）：
// - 调用形状（`thread::spawn(`、`move |_token|`）跑在 `codeOnly()` 上——注释**和
//   字符串**里的提及都不算（terminal-invariants 的教训：注释里的提及不算命中）；
// - 登记名与错误文案住在 `format!`/`eprintln!` 的字符串字面量里，所以那几条判据
//   跑在 `stripComments()`（只剥注释、保留字符串）的语料上；
// - 语料读不到必须报错，不得把"没读到"当成"没有"。

import path from "node:path";
import process from "node:process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { codeOnly } from "./adapter-invariants.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const SESSION = "src-tauri/src/session.rs";
const WATCHER = "src-tauri/src/event_watcher.rs";
const LIB = "src-tauri/src/lib.rs";

const THREAD_SPAWN = /(?<!\w)thread::spawn\s*\(/g;
const UNDER_TOKEN = /move\s+\|_token\|/g;

/** 只剥注释、保留字符串的语料（与 codeOnly 互补；本守卫的名称判据住在字符串里）。 */
export function stripComments(text) {
  const out = text.split("");
  let inString = false;
  let inBlock = 0;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    const next = text[i + 1];
    if (inBlock > 0) {
      if (c === "/" && next === "*") inBlock += 1;
      else if (c === "*" && next === "/") inBlock -= 1;
      if (c !== "\n") out[i] = " ";
      continue;
    }
    if (inString) {
      if (c === "\\") {
        i += 1;
        continue;
      }
      if (c === '"') inString = false;
      continue;
    }
    if (c === '"') {
      inString = true;
      continue;
    }
    if (c === "/" && next === "/") {
      let j = i;
      while (j < text.length && text[j] !== "\n") {
        out[j] = " ";
        j += 1;
      }
      i = j - 1;
      continue;
    }
    if (c === "/" && next === "*") {
      inBlock = 1;
      out[i] = " ";
      out[i + 1] = " ";
      i += 1;
      continue;
    }
  }
  return out.join("");
}

/** 区域断言：从 `from` 到 `to`（含首尾）之间必须出现 `needle` 至少 `min` 次。 */
function regionRule(text, from, to, needle, min, label, problems) {
  const start = text.indexOf(from);
  if (start === -1) {
    problems.push(`${label}: marker not found: ${JSON.stringify(from)}`);
    return;
  }
  const end = text.indexOf(to, start);
  if (end === -1) {
    problems.push(`${label}: region end not found after marker: ${JSON.stringify(to)}`);
    return;
  }
  const region = text.slice(start, end);
  const hits = region.split(needle).length - 1;
  if (hits < min) {
    problems.push(
      `${label}: expected ${min}x ${JSON.stringify(needle)} inside the region, found ${hits} — ` +
        "watcher 的入口/循环 token 检查是 Action 2 的停机形状，不得移除",
    );
  }
}

/** 全部判据的唯一实现。`files` 是 `[{path, text}]`，负例直接喂字符串。 */
export function watcherDisciplineProblems(files) {
  const problems = [];
  const byPath = new Map(files.map((file) => [file.path, file.text]));
  for (const required of [SESSION, WATCHER, LIB]) {
    if (!byPath.has(required)) {
      problems.push(`${required}: guard corpus file was not read — an unread file is not absence`);
    }
  }
  if (problems.length > 0) return problems;

  const session = codeOnly(byPath.get(SESSION));
  const watcher = codeOnly(byPath.get(WATCHER));
  const plainSession = stripComments(byPath.get(SESSION));
  const plainWatcher = stripComments(byPath.get(WATCHER));
  const plainLib = stripComments(byPath.get(LIB));

  // ① 会话层两个文件不得再有裸 thread::spawn（G04 的「唯一真缺」站 + G03 的本站）。
  for (const [label, text] of [
    [SESSION, session],
    [WATCHER, watcher],
  ]) {
    const spawns = [...text.matchAll(THREAD_SPAWN)];
    if (spawns.length > 0) {
      problems.push(
        `${label}: bare thread::spawn returned (${spawns.length}) — per-task/长驻 watcher 必须经 WatcherSupervisor`,
      );
    }
  }

  // ② `_token` 拿而不查的形状不得回来（G04 第二类的机器形状）。
  for (const [label, text] of [
    [SESSION, session],
    [WATCHER, watcher],
  ]) {
    const idle = [...text.matchAll(UNDER_TOKEN)];
    if (idle.length > 0) {
      problems.push(
        `${label}: move |_token| found (${idle.length}) — 要么真检查,要么改回 spawn_unique 并说明自终止依据`,
      );
    }
  }

  // ③ 三个登记名各恰好一处（重复登记＝两个真相）。
  for (const [label, text, needle, expect] of [
    [SESSION, plainSession, "watch:claude-lazy-attach", 1],
    [SESSION, plainSession, "watch:resume-session", 1],
    [WATCHER, plainWatcher, '"watch:hook-events"', 1],
  ]) {
    const hits = text.split(needle).length - 1;
    if (hits !== expect) {
      problems.push(`${label}: ${needle} expected ${expect}x, found ${hits}`);
    }
  }
  regionRule(
    plainWatcher,
    '"watch:hook-events"',
    "fn dispatch",
    "token.is_cancelled()",
    1,
    `${WATCHER}:run_loop`,
    problems,
  );

  // ④ lazy-attach：入口一处（return）+ 等待循环一处（break）都得查。
  regionRule(
    plainSession,
    "watch:claude-lazy-attach",
    "lazy session attach registration failed",
    "token.is_cancelled()",
    2,
    `${SESSION}:lazy-attach`,
    problems,
  );

  // ⑤ resume：入口检查必须在闭包体最前。
  regionRule(
    plainSession,
    "watch:resume-session",
    "session watcher registration failed",
    "token.is_cancelled()",
    1,
    `${SESSION}:resume-watcher`,
    problems,
  );

  // ⑥ Exit 块：supervisor.shutdown() 必须在关 LocalRouter 之前（先静默再关服务）。
  const exitAt = plainLib.indexOf("RunEvent::Exit = event");
  if (exitAt === -1) {
    problems.push(`${LIB}: RunEvent::Exit branch not found — corpus moved, guard blind`);
  } else {
    const exitBlock = plainLib.slice(exitAt, exitAt + 2000);
    const sd = exitBlock.indexOf("supervisor.shutdown()");
    const lr = exitBlock.indexOf("LocalRouterManager");
    if (sd === -1) {
      problems.push(`${LIB}:Exit — supervisor.shutdown() missing；G03 的停机路径又没了`);
    } else if (lr !== -1 && sd > lr) {
      problems.push(`${LIB}:Exit — watcher must be silenced before LocalRouter shutdown`);
    }
  }

  return problems;
}

export function loadWatcherCorpus(root = repoRoot) {
  return [SESSION, WATCHER, LIB].map((rel) => ({
    path: rel,
    text: readFileSync(path.join(root, rel), "utf8"),
  }));
}

function main() {
  let files;
  try {
    files = loadWatcherCorpus();
  } catch (error) {
    console.error(`watcher-discipline: cannot read corpus: ${error.message}`);
    process.exit(1);
  }
  const problems = watcherDisciplineProblems(files);
  if (problems.length > 0) {
    for (const problem of problems) console.error(`  - ${problem}`);
    console.error(`watcher-discipline: ${problems.length} problem(s)`);
    process.exit(1);
  }
  console.log("watcher-discipline: session/event_watcher/lib watcher shapes hold");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
