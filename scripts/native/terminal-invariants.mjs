#!/usr/bin/env node
// P3-03 收尾轮的计数守卫(补记一百零六):`settle_exit` 在宿主必须**恰好被调用一次**。
//
// 为什么需要另一个脚本:`terminal-core` 里那条 `host-exit-settlement-goes-through-core` 是
// `absent` 形状,它只能断"旧形状没了 + 新形状还在",断不了次数。而运行时的"每条来源只结算
// 一次"由 `tests/pty_exit.rs` 的十个真 PTY 用例守着——那防的是行为错,防不了"将来有人再加
// 第二个结算入口"。计数归计数脚本,照 `adapter-invariants.mjs` 的先例(纯函数 + 配套负例 +
// `kind:"gate"`/`kind:"test"` 两条 case)。
//
// 口径与 D12 同源:命中必须先过 `codeOnly()`(注释与字符串里的 `settle_exit(` 不算),且
// 语料读不到时必须报错,不得把"没读到"当成"没有"。

import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { codeOnly, enclosingFunction, rustFiles } from "./adapter-invariants.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const HOST_DIR = "src-tauri/src";
const CORE_EXIT_FILE = "core/crates/aeroric-core/src/terminal/exit.rs";
const THE_SETTLEMENT_FN = "finalize_task_exit";

const CALL = /(?<!\w)settle_exit\s*\(/g;
const DEFINITION = /(?<!\w)fn\s+settle_exit\s*\(/;

/**
 * 结算计数的唯一判定。`hostFiles` / `coreFiles` 都是 `[{path, text}]`——内容调用方给,
 * 这样负例可以直接喂字符串,不需要临时 git 树(照 `adapter-invariants.test.mjs` 的口径)。
 */
export function settlementProblems(hostFiles, coreFiles) {
  const problems = [];

  if (hostFiles.length === 0) {
    return [`${HOST_DIR}: no host .rs file was read — an unread corpus is not absence`];
  }

  const sites = [];
  for (const file of hostFiles) {
    const lines = codeOnly(file.text).split("\n");
    lines.forEach((line, index) => {
      CALL.lastIndex = 0;
      if (!CALL.test(line)) return;
      DEFINITION.lastIndex = 0;
      if (DEFINITION.test(line)) return;
      sites.push({ file: file.path, line: index + 1, fn: enclosingFunction(lines, index) });
    });
  }

  if (sites.length !== 1) {
    const where = sites.map((s) => `${s.file}:${s.line}`).join(", ") || "none";
    problems.push(
      `settle_exit must be called from exactly 1 host site, found ${sites.length} (${where})`
      + (sites.length === 0
        ? " — the host decides exit status by itself again"
        : " — two settlement owners is exactly the double-settle P3-03 closed"),
    );
  } else if (sites[0].fn !== THE_SETTLEMENT_FN) {
    problems.push(
      `the single settle_exit call must live in ${THE_SETTLEMENT_FN}, found it in ${sites[0].fn ?? "(no enclosing fn)"} at ${sites[0].file}:${sites[0].line}`,
    );
  }

  if (coreFiles.length === 0) {
    problems.push(`${CORE_EXIT_FILE}: core settlement source was not read — refusing to certify a count against an empty corpus`);
  } else {
    const definitions = coreFiles
      .filter((file) => { DEFINITION.lastIndex = 0; return DEFINITION.test(codeOnly(file.text)); })
      .map((file) => file.path);
    if (definitions.length !== 1) {
      problems.push(
        `core must define settle_exit exactly once, found ${definitions.length}`
        + ` (${definitions.join(", ") || "none — the decision face left core"})`,
      );
    }
  }

  return problems;
}

export function loadSettlementCorpus(root = repoRoot) {
  const hostFiles = rustFiles(HOST_DIR, root).map((relative) => ({
    path: relative,
    text: readFileSync(path.join(root, relative), "utf8"),
  }));
  const coreFiles = [];
  try {
    coreFiles.push({
      path: CORE_EXIT_FILE,
      text: readFileSync(path.join(root, CORE_EXIT_FILE), "utf8"),
    });
  } catch {
    // 读不到就留空:problems 里会把它报成"未读到语料",绝不静默通过。
  }
  return { hostFiles, coreFiles };
}

function main() {
  const { hostFiles, coreFiles } = loadSettlementCorpus();
  const problems = settlementProblems(hostFiles, coreFiles);
  if (problems.length) {
    console.error(`terminal-invariants: ${problems.length} problem(s)`);
    for (const problem of problems) console.error(`  - ${problem}`);
    process.exit(1);
  }
  const sites = [];
  for (const file of hostFiles) {
    const lines = codeOnly(file.text).split("\n");
    lines.forEach((line, index) => {
      CALL.lastIndex = 0;
      if (CALL.test(line) && !DEFINITION.test(line)) sites.push(`${file.path}:${index + 1}`);
    });
  }
  console.log(
    `terminal-invariants: settle_exit is called from exactly 1 host site (${sites[0]} in ${THE_SETTLEMENT_FN}) `
    + `and defined exactly once in core; ${hostFiles.length} host file(s) read`,
  );
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
