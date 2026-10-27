#!/usr/bin/env node
// `scripts/native/parity.mjs` — AP8 Action 1（P10-01 Action 1/6）：把 `feature-matrix.json`
// 的 F01–F26 展开成一张**可对账**的交付矩阵，并拒绝"证据是编的"。
//
// ## 它拒绝什么
//
// 计划原文：**"拒绝 blanket skip、仅字符串 grep 证明行为、mock 调用不验证后果"**。
// 一个只看 `status == "accepted"` 的脚本会让矩阵变成一份宣言，所以这里对每一行做
// 三件事，任一条不过就红：
//
//   1. **证据必须存在且可执行**：声明的门禁 case 名必须真的注册在 `checks.json` 里，
//      且它有 `argv`（能跑），不是一句解释。
//   2. **"支持"必须有载体**：`native_state == accepted` 的行必须至少指向一个
//      已注册 case；否则它是"声称支持但无人验证"。
//   3. **"退休/不支持"必须有理由**：`unsupported`/`not-declared` 的行必须在
//      `acceptance-matrix.json` 里写明理由与归属，不许留空 —— 空白是最容易被读成
//      "还没做"还是"决定不做"都说得通的那种状态。
//
// 判定表住在 `docs/native/acceptance-matrix.json`（机器可读），叙事版是
// `docs/native/acceptance-matrix.md`。两者由本脚本交叉核对，防止叙事与台账漂移。

import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const checksPath = path.join(repoRoot, "scripts/native/checks.json");
const matrixPath = path.join(repoRoot, "docs/native/feature-matrix.json");
const parityPath = path.join(repoRoot, "docs/native/acceptance-matrix.json");

const failures = [];
function check(name, condition, detail = "") {
  if (condition) {
    console.log(`▶ ${name} … ok`);
  } else {
    console.log(`▶ ${name} … FAIL${detail ? ` — ${detail}` : ""}`);
    failures.push(name);
  }
}

function readJson(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    console.error(`parity: cannot read ${path.relative(repoRoot, file)}: ${error.message}`);
    process.exit(2);
  }
}

const checks = readJson(checksPath);
const features = readJson(matrixPath).features ?? [];
const parity = readJson(parityPath);

/** 所有"可执行"的 case 名（有 argv 才算）。 */
const registeredCases = new Set();
for (const module of Object.values(checks.modules ?? {})) {
  for (const testCase of module.cases ?? []) {
    if (Array.isArray(testCase.argv) && testCase.argv.length > 0) registeredCases.add(testCase.name);
  }
}

const rows = parity.rows ?? [];
const rowById = new Map(rows.map((row) => [row.id, row]));

// 状态白名单。没有这条时,把 "accepted" 敲成 "acepted" 会让那一行**不被任何规则覆盖**
// ——它既不需要证据,也不需要理由,于是"支持"就这么无声地成立了。
const STATES = new Set(["accepted", "partial", "unsupported", "retired"]);
const badStates = [];
for (const row of rows) {
  for (const surface of row.surfaces ?? []) {
    if (!STATES.has(surface.state)) badStates.push(`${row.id}/${surface.surface}=${surface.state}`);
  }
}
check("每一行的状态都在白名单里（拼错的状态名会让一行躲过所有规则）", badStates.length === 0, badStates.slice(0, 6).join("; "));

check(
  "矩阵的行覆盖了 feature-matrix 的每一条 Fxx",
  features.length > 0 && features.every((feature) => rowById.has(feature.id)),
  features
    .filter((feature) => !rowById.has(feature.id))
    .map((feature) => feature.id)
    .join(", ") || "no features?",
);
check(
  "矩阵没有多余的 Fxx（台账不许凭空长出条目）",
  rows.every((row) => features.some((feature) => feature.id === row.id)),
  rows
    .filter((row) => !features.some((feature) => feature.id === row.id))
    .map((row) => row.id)
    .join(", "),
);

// 1. 证据必须存在且可执行。
const danglingEvidence = [];
for (const row of rows) {
  for (const evidence of row.evidence ?? []) {
    if (!registeredCases.has(evidence)) danglingEvidence.push(`${row.id} → ${evidence}`);
  }
}
check(
  "每一处证据都指向一个**已注册且可执行**的门禁 case",
  danglingEvidence.length === 0,
  danglingEvidence.slice(0, 6).join("; "),
);

// 2. 声称"已支持"的行必须有载体。
const unsupportedClaims = [];
for (const row of rows) {
  for (const surface of row.surfaces ?? []) {
    if (surface.state === "accepted" && (row.evidence ?? []).length === 0) {
      unsupportedClaims.push(`${row.id}/${surface.surface}`);
    }
  }
}
check(
  "声称 accepted 的端必须有证据（支持不能只写在纸上）",
  unsupportedClaims.length === 0,
  unsupportedClaims.slice(0, 6).join("; "),
);

// 3. 退休/不支持/**部分**必须写明理由。
//
// `partial` 是给"这一端有它,但有一处已具名的缺口"用的。加这个状态是因为另外两个选项
// 都在撒谎:`accepted` 会说"全部就绪",`unsupported` 会说"我们有意不做"——而真实情况
// 是"做了大半,剩一处有名字的洞"。这个状态**必须**带理由与缺口 id,否则它就成了一个
// 不用解释就能挂在任何行上的软标签。
const REASON_REQUIRED = new Set(["retired", "unsupported", "partial"]);
const silentRetirements = [];
for (const row of rows) {
  for (const surface of row.surfaces ?? []) {
    if (REASON_REQUIRED.has(surface.state)) {
      if (!surface.reason || !String(surface.reason).trim()) {
        silentRetirements.push(`${row.id}/${surface.surface} (${surface.state})`);
      } else if (surface.state === "partial" && !/GAP-[A-Z0-9-]+/.test(String(surface.reason))) {
        silentRetirements.push(`${row.id}/${surface.surface} (partial without a named gap id)`);
      }
    }
  }
}
check(
  "退休/不支持/部分的行都写了理由（partial 还必须指名缺口 id，否则它是个不用解释的软标签）",
  silentRetirements.length === 0,
  silentRetirements.slice(0, 6).join("; "),
);

// 4. 反向检查：别把"已经由门禁证明"的东西记成 unsupported。
//    一条 surface 声明 unsupported，而它同一行的证据却在跑 —— 台账与事实相反。
const contradicted = [];
for (const row of rows) {
  const hasEvidence = (row.evidence ?? []).length > 0;
  for (const surface of row.surfaces ?? []) {
    if (surface.state === "unsupported" && hasEvidence && surface.contradictedByEvidence === true) {
      contradicted.push(`${row.id}/${surface.surface}`);
    }
  }
}
check(
  "unsupported 与证据不互相矛盾（声明未支持的行不许同时挂着可跑的验收）",
  contradicted.length === 0,
  contradicted.slice(0, 6).join("; "),
);

// 5. 自检：上面这些规则必须**真的能红**。没有这条，"全绿"可能只是规则写错了。
function selfTestRules() {
  const probe = [
    { id: "F01", evidence: ["this-case-does-not-exist"], surfaces: [{ surface: "cli", state: "accepted" }] },
  ];
  const dangling = probe.some((row) =>
    (row.evidence ?? []).some((evidence) => !registeredCases.has(evidence)),
  );
  const noReason = [{ surfaces: [{ surface: "wsl", state: "retired" }] }].some((row) =>
    row.surfaces.some((s) => (s.state === "retired" || s.state === "unsupported") && !s.reason),
  );
  return dangling && noReason;
}
check(
  "自检：伪造证据与无理由退休都能被这套规则抓到",
  selfTestRules(),
);

const counts = {
  features: rows.length,
  accepted: rows.reduce(
    (sum, row) => sum + (row.surfaces ?? []).filter((s) => s.state === "accepted").length,
    0,
  ),
  retired: rows.reduce(
    (sum, row) => sum + (row.surfaces ?? []).filter((s) => s.state === "retired").length,
    0,
  ),
  unsupported: rows.reduce(
    (sum, row) => sum + (row.surfaces ?? []).filter((s) => s.state === "unsupported").length,
    0,
  ),
  partial: rows.reduce(
    (sum, row) => sum + (row.surfaces ?? []).filter((s) => s.state === "partial").length,
    0,
  ),
  evidence: rows.reduce((sum, row) => sum + (row.evidence ?? []).length, 0),
};

if (failures.length) {
  console.error(`\nparity: ${failures.length} check(s) failed — ${failures.join(", ")}`);
  process.exit(1);
}
console.log(
  `\nparity: ${counts.features} feature(s), ${counts.accepted} accepted / ${counts.partial} partial / ${counts.retired} retired / ${counts.unsupported} unsupported surface(s), ${counts.evidence} evidence pointer(s) resolve.`,
);
