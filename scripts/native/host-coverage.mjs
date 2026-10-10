#!/usr/bin/env node
// `scripts/native/host-coverage.mjs` — P8-05：**命令映射覆盖率**的量表，以及"哪些还没接"的清单。
//
// 计划 Acceptance 里那句"命令映射覆盖率 100%"到今天为止只是一句愿望：台账
// （`docs/native/command-map.json`）给 641 条命令写了 `rpc_method`，而**宿主真的注册了
// 多少**从来没人量过。这个脚本就是那把尺子，两条判据：
//
//   1. **覆盖率**：宿主 `aeroric-host` 实际 `set.register("<method>")` 的方法名 ∩ 台账
//      `rpc_method` 集合 ÷ 台账总数。今日读数与分模块缺口逐条打印。
//   2. **棘轮**：覆盖率**不得低于**冻结值（`--floor=N`，默认读 `checks.json` 里本 case
//      的 `coverageFloor`）。降下来就红——这才是"不再往回退"的机器形状；至于涨到 100%
//      需要补齐哪些宿主方法，清单就在下面，按模块排好。
//
// 为什么不做成"必须 100% 否则红"：那会让今天的门禁立刻红成一片，而红着不修的门禁等于
// 没有门禁（本仓反复栽在这上面）。棘轮＋清单是"既如实又前进"的形状。

import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const args = process.argv.slice(2);
const floorArg = args.find((a) => a.startsWith("--floor="));
const floor = floorArg ? Number(floorArg.slice("--floor=".length)) : null;
const quiet = args.includes("--quiet");

/** 宿主侧真实注册的方法名。扫源码里的 `set.register("<name>"` 与 notebook_services.rs
 * 的 `service!("<name>"`／`service_ctx!("<name>"` 宏调用点（宏在调用点展开为同一个
 * `set.register($method, …)`，方法名是逐调用点的字面量——切片九十前该盲区让 P7-05/06
 * 的 71 条 notebook 注册全部不可见）。 */
function hostMethods() {
  const dir = path.join(repoRoot, "core", "crates", "aeroric-host", "src");
  const methods = new Set();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".rs")) continue;
    const text = readFileSync(path.join(dir, file), "utf8");
    for (const match of text.matchAll(/set\.register\(\s*"([^"]+)"/g)) methods.add(match[1]);
    for (const match of text.matchAll(/service(?:_ctx)?!\(\s*"([^"]+)"/g)) methods.add(match[1]);
  }
  return methods;
}

/** 命名对账表：台账派生名 ↔ 宿主真实名（`docs/native/host-methods.json`）。
 *
 * 台账的 `rpc_method` 是**派生列**（`inventory.mjs` 按命令名推），所以它记的是立项期的
 * 计划命名；宿主实现选了别的拼写，两者只在这张表里对上。三条校验：派生名必须在台账里、
 * 真实名必须在宿主里、一个真实名只能被一条别名占用（否则覆盖率会重复计数）。 */
function aliasTable(ledgerMethods) {
  const file = path.join(repoRoot, "docs/native", "host-methods.json");
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    return { aliases: [], problems: [`cannot read docs/native/host-methods.json: ${error.message}`] };
  }
  const problems = [];
  const seen = new Map();
  for (const row of parsed.aliases ?? []) {
    if (!ledgerMethods.has(row.derived)) problems.push(`alias source absent from the ledger: ${row.derived}`);
    if (seen.has(row.actual)) problems.push(`${row.actual} claimed by both ${seen.get(row.actual)} and ${row.derived}`);
    seen.set(row.actual, row.derived);
    if (!String(row.evidence ?? "").trim()) problems.push(`${row.derived}: alias without evidence`);
  }
  return { aliases: parsed.aliases ?? [], problems };
}

/** Tauri 侧的**转发调用点**：`host_dispatch::dispatch(&app, "<method>", …)`。
 *
 * 唯一真相源是调用点本身（不是一张另抄的表）：有调用点的方法才会在 external 模式下走
 * 宿主。一条调用点指向宿主没注册的方法＝那条命令在 external 下必然报"不提供"，属于接线
 * 错误，所以这里逐条对账。 */
function dispatchCallSites() {
  const dir = path.join(repoRoot, "src-tauri", "src");
  const found = [];
  const walk = (current) => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) {
        walk(full);
        continue;
      }
      if (!entry.name.endsWith(".rs")) continue;
      const text = readFileSync(full, "utf8");
      for (const match of text.matchAll(/host_dispatch::dispatch\(\s*[\s\S]{0,80}?"([a-z_]+\.[a-zA-Z_]+)"/g)) {
        found.push({ method: match[1], file: path.relative(repoRoot, full) });
      }
    }
  };
  walk(dir);
  return found;
}

/** 台账里声明的方法名（`rpc_method` 非空的行）。 */
function ledgerMethods() {
  const map = JSON.parse(readFileSync(path.join(repoRoot, "docs/native/command-map.json"), "utf8"));
  const rows = map.commands
    .filter((command) => command.rpc_method)
    .map((command) => ({
      method: command.rpc_method,
      module: command.check_module ?? "?",
      command: command.command,
    }));
  return rows;
}

function main() {
  const host = hostMethods();
  const ledger = ledgerMethods();
  const byMethod = new Map();
  for (const row of ledger) {
    if (!byMethod.has(row.method)) byMethod.set(row.method, []);
    byMethod.get(row.method).push(row);
  }

  const { aliases, problems: aliasProblems } = aliasTable(new Set(byMethod.keys()));
  const aliasFor = new Map(aliases.map((row) => [row.derived, row.actual]));
  const isCovered = (method) => {
    if (host.has(method)) return true;
    const actual = aliasFor.get(method);
    return Boolean(actual && host.has(actual));
  };
  const covered = [...byMethod.keys()].filter(isCovered).sort();
  const missing = [...byMethod.keys()].filter((method) => !isCovered(method)).sort();
  const brokenAliases = aliases.filter((row) => !host.has(row.actual));
  const total = byMethod.size;
  const percent = total === 0 ? 100 : (covered.length / total) * 100;

  console.log(
    `host-coverage: ${covered.length}/${total} rpc methods served by aeroric-host (${percent.toFixed(1)}%)`,
  );

  for (const problem of aliasProblems) console.error(`host-coverage: ${problem}`);
  for (const row of brokenAliases) {
    console.error(`host-coverage: alias target is not registered by the host: ${row.derived} -> ${row.actual}`);
  }
  if (aliasProblems.length || brokenAliases.length) process.exit(1);

  if (!quiet) {
    console.log(`  命名对账: ${aliases.length} 条别名（台账派生名 ↔ 宿主真实名，逐条带证据）`);
    // 缺口按模块聚合：一个模块里"差多少条"比一整张 600 行的清单更能看出下一步该做谁。
    const gaps = new Map();
    for (const method of missing) {
      for (const row of byMethod.get(method)) {
        const key = row.module;
        gaps.set(key, (gaps.get(key) ?? 0) + 1);
      }
    }
    console.log("  缺口（按模块，降序）:");
    for (const [module, count] of [...gaps.entries()].sort((a, b) => b[1] - a[1])) {
      console.log(`    ${module.padEnd(20)} ${count}`);
    }
    console.log(`  已接（${covered.length}）: ${covered.join(", ")}`);
    // 宿主有、台账没登记的方法：不算缺口，但值得一眼看见（可能是漏登记的新能力）。
    const unledgered = [...host].filter((method) => !byMethod.has(method)).sort();
    console.log(`  宿主注册但台账未登记（${unledgered.length}）: ${unledgered.slice(0, 12).join(", ")}${unledgered.length > 12 ? ", …" : ""}`);
  }

  // 接线表 vs 宿主：映射指向不存在的方法＝那条命令在 external 下必然失败。
  const table = dispatchCallSites();
  const broken = table.filter((row) => !host.has(row.method));
  console.log(
    `  转发调用点: ${table.length} 处，其中 ${table.length - broken.length} 处指向宿主真的服务的方法`,
  );
  if (broken.length) {
    for (const row of broken) console.log(`    ✗ ${row.file}: ${row.method}（宿主未注册）`);
    process.exit(1);
  }

  if (floor !== null && Number.isFinite(floor) && covered.length < floor) {
    console.error(
      `host-coverage: 覆盖率回退了——冻结地板 ${floor}，今天 ${covered.length}（补齐宿主方法或说明为什么允许回退）`,
    );
    process.exit(1);
  }
  if (floor !== null && Number.isFinite(floor)) {
    console.log(`  棘轮: ${covered.length} >= floor ${floor} ✓`);
  }
}

main();
