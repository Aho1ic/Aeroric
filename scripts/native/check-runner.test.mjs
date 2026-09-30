import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { absenceControlFailure, caseSkipReason, countCases, missingOutputTokens, moduleGateProblem } from "./gate-config.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const goodCase = { name: "does-something", kind: "gate", argv: ["node", "-e", "0"] };
const committed = JSON.parse(readFileSync(path.join(repoRoot, "scripts", "native", "checks.json"), "utf8"));

/** Run the real runner in a throwaway tree against a module defined on the fly. */
function runGateAgainst(moduleName, definition, extraEnv) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-gate-"));
  try {
    mkdirSync(path.join(dir, "scripts/native"), { recursive: true });
    for (const source of ["check.mjs", "gate-config.mjs"]) {
      cpSync(path.join(repoRoot, "scripts/native", source), path.join(dir, "scripts/native", source));
    }
    const config = JSON.parse(JSON.stringify(committed));
    config.modules[moduleName] = definition;
    writeFileSync(path.join(dir, "scripts/native/checks.json"), JSON.stringify(config, null, 2) + "\n");
    return spawnSync(process.execPath, [path.join(dir, "scripts/native/check.mjs"), moduleName], {
      encoding: "utf8",
      env: extraEnv ? { ...process.env, ...extraEnv } : process.env,
    });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a registered module with real cases is accepted", () => {
  assert.equal(moduleGateProblem("skeleton", { cases: [goodCase] }), null);
});

test("a module registered with zero cases is refused", () => {
  // The measured hole: this used to print "0 case(s) passed" and exit 0.
  assert.match(moduleGateProblem("protocol", { cases: [] }), /zero cases/);
});

test("a module whose cases field is missing or wrong-typed is refused", () => {
  assert.match(moduleGateProblem("storage", {}), /missing or not an array/);
  assert.match(moduleGateProblem("storage", { cases: "inventory" }), /missing or not an array/);
});

test("an unregistered planned module is refused", () => {
  assert.match(moduleGateProblem("remote", undefined), /not registered/);
});

test("a case without a name or a usable argv is refused", () => {
  assert.match(moduleGateProblem("x", { cases: [{ argv: [] }] }), /1 case\(s\) without a name/);
  assert.match(moduleGateProblem("x", { cases: [{ name: "n", argv: [] }] }), /1 case\(s\) without a name/);
  assert.match(moduleGateProblem("x", { cases: [{ name: "n", argv: ["ok" ] }, { name: "m", argv: [7] }] }), /1 case\(s\) without a name/);
});

test("every registered module in the committed checks.json is gate-ready", () => {
  for (const [name, entry] of Object.entries(committed.modules)) {
    assert.equal(moduleGateProblem(name, entry), null, `${name} is not gate-ready`);
  }
  assert.ok(Object.keys(committed.modules).length >= 1, "no registered module means the gate checks nothing");
});

test("'all' still refuses until every planned module has cases", () => {
  const planned = Object.keys(committed.plannedModules ?? {}).filter((k) => !k.startsWith("$"));
  const notReady = planned.filter((m) => moduleGateProblem(m, committed.modules[m]));
  assert.ok(notReady.length > 0, "'all' would pass while most modules are still empty");
  assert.ok(!notReady.includes("skeleton"), "skeleton must be ready");
});

test("a failing case propagates the child's exit code instead of passing", () => {
  // Everything the gate certifies rests on this: a red child must make the gate red.
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-gate-fail-"));
  try {
    mkdirSync(path.join(dir, "scripts/native"), { recursive: true });
    for (const source of ["check.mjs", "gate-config.mjs"]) {
      cpSync(path.join(repoRoot, "scripts/native", source), path.join(dir, "scripts/native", source));
    }
    const config = JSON.parse(JSON.stringify(committed));
    config.modules.failing = { registeredBy: "test", cases: [{ name: "exits-four", kind: "gate", argv: [process.execPath, "-e", "process.exit(4)"] }] };
    writeFileSync(path.join(dir, "scripts/native/checks.json"), JSON.stringify(config, null, 2) + "\n");
    const result = spawnSync(process.execPath, [path.join(dir, "scripts/native/check.mjs"), "failing"], { encoding: "utf8" });
    assert.equal(result.status, 1, `a red case must fail the gate, got ${result.status}`);
    assert.match(result.stdout + result.stderr, /FAIL \(exit 4\)/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the runner really exits non-zero on a zero-case module", () => {
  // Regression guard for the hole, run as a subprocess so the exit code is the
  // runner's own and not a re-implementation of its logic.
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-gate-"));
  try {
    mkdirSync(path.join(dir, "scripts/native"), { recursive: true });
    for (const source of ["check.mjs", "gate-config.mjs"]) {
      cpSync(path.join(repoRoot, "scripts/native", source), path.join(dir, "scripts/native", source));
    }
    const config = JSON.parse(JSON.stringify(committed));
    config.modules.protocol = { registeredBy: "P2-01", cases: [] };
    writeFileSync(path.join(dir, "scripts/native/checks.json"), JSON.stringify(config, null, 2) + "\n");
    const result = spawnSync(process.execPath, [path.join(dir, "scripts/native/check.mjs"), "protocol"], { encoding: "utf8" });
    assert.notEqual(result.status, 0, "a module that runs nothing must not exit 0");
    assert.match(result.stderr, /zero cases/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
test("a control probe that finds content is not a failure", () => {
  assert.equal(absenceControlFailure("x", ["rg", "-q", "pub", "src"], 0), null);
});

test("a control probe that finds nothing is reported with its target", () => {
  assert.match(absenceControlFailure("x", ["rg", "-q", "pub", "core/src"], 1), /core\/src/);
  assert.match(absenceControlFailure("x", ["rg", "-q", "pub", "core/src"], 2), /exit 2/);
});

test("an absent case without controls cannot be registered", () => {
  const problem = moduleGateProblem("m", { cases: [{ name: "scan", kind: "absent", argv: ["rg", "x", "y"] }] });
  assert.match(problem, /no usable controls/);
});

test("malformed controls are refused", () => {
  assert.match(moduleGateProblem("m", { cases: [{ name: "s", kind: "absent", argv: ["rg"], controls: [] }] }), /no usable controls/);
  assert.match(moduleGateProblem("m", { cases: [{ name: "s", kind: "absent", argv: ["rg"], controls: [[]] }] }), /no usable controls/);
});

test("a forbid case without a positive output assertion cannot be registered", () => {
  assert.match(moduleGateProblem("m", { cases: [{ name: "purity", kind: "forbid", argv: ["cargo", "tree"], forbid: ["tauri"] }] }), /no mustContain/);
});

test("missingOutputTokens names the tokens an output lacked", () => {
  assert.deepEqual(missingOutputTokens({ mustContain: ["aeroric-core", "aeroric-ghost"] }, "tree\naeroric-core v0.1.0"), ["aeroric-ghost"]);
});

test("an absent case fails the gate when its corpus was never read", () => {
  // Main scan says "found nothing" (exit 1) and the control also finds nothing:
  // exactly the state a renamed or emptied scanned tree produces.
  const result = runGateAgainst("probe_empty", {
    cases: [{
      name: "would-pass-vacuously", kind: "absent",
      argv: [process.execPath, "-e", "process.exit(1)"],
      controls: [[process.execPath, "-e", "process.exit(1)"]],
    }],
  });
  assert.equal(result.status, 1, `an unreadable corpus must not pass, got ${result.status}`);
  assert.match(result.stdout, /found no content/);
});

test("the same absent case passes once its control proves the corpus was read", () => {
  const result = runGateAgainst("probe_real", {
    cases: [{
      name: "genuine-absence", kind: "absent",
      argv: [process.execPath, "-e", "process.exit(1)"],
      controls: [[process.execPath, "-e", "process.exit(0)"]],
    }],
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test("a forbid case with an empty output fails its positive control", () => {
  const result = runGateAgainst("probe_forbid", {
    cases: [{
      name: "purity-empty-output", kind: "forbid",
      argv: [process.execPath, "-e", "console.log('')"],
      stream: "stdout", forbid: ["tauri"], mustContain: ["aeroric-core"],
    }],
  });
  assert.equal(result.status, 1, "an empty dependency listing must not certify purity");
  assert.match(result.stdout, /positive control/);
});

// The floor is only as honest as the counter. Every branch below pins a reporter
// quirk that has already produced a wrong number once in this repository.
test("countCases sums rust results across targets without double-counting", () => {
  assert.equal(countCases("rust", "test result: ok. 28 passed; 0 failed\ntest result: ok. 10 passed; 0 failed"), 38);
  assert.equal(countCases("rust", "test result: FAILED. 1 passed; 2 failed"), 1);
  assert.equal(countCases("rust", ""), 0);
});

test("countCases reads swift per-case lines instead of the repeated totals", () => {
  const perCase = "Test Case '-[a.B testOne]' passed (0.1)\nTest Case '-[a.B testTwo]' failed (0.1)\n";
  assert.equal(countCases("swift", `${perCase}\t Executed 2 tests, with 1 failure`), 2);
  // SwiftPM prints one `Executed N tests` block per bundle: summing 12+12+12 read 36.
  assert.equal(countCases("swift", "Executed 12 tests\nExecuted 4 tests\nExecuted 4 tests"), 12);
  assert.equal(countCases("swift", "nothing matched"), 0);
});

test("countCases takes the vitest summary and never the file count", () => {
  const output = " Test Files  1 passed (1)\n      Tests  17 passed (17)";
  assert.equal(countCases("vitest", output), 17);
  assert.equal(countCases("vitest", " ✓ src/test/a.test.ts (12 tests) 3ms\n ✓ src/test/b.test.ts (5 tests) 1ms"), 17);
  assert.equal(countCases("vitest", " Test Files  0 passed (0)"), 0);
});

test("colour codes never turn a passing suite into zero executed cases", () => {
  const coloured = "\u001b[2m      Tests \u001b[22m \u001b[1m\u001b[32m17 passed\u001b[39m\u001b[22m\u001b[90m (1)\u001b[39m";
  assert.equal(countCases("vitest", coloured), 17, "this is the byte stream a piped vitest emits");
  assert.equal(countCases("vitest", "\u001b[2m Test Files \u001b[22m \u001b[1m\u001b[32m1 passed\u001b[39m"), 0);
});

test("an unknown reporter counts zero rather than guessing", () => {
  assert.equal(countCases("vitest-ish", " Tests  17 passed (17)"), 0);
  assert.equal(countCases(undefined, " Tests  17 passed (17)"), 0);
});

test("every registered test case declares a reporter the runner can read", () => {
  for (const [name, entry] of Object.entries(committed.modules)) {
    for (const testCase of entry.cases) {
      if (testCase.kind !== "test") continue;
      assert.ok(["rust", "swift", "vitest", "node-test"].includes(testCase.parse),
        `${name}/${testCase.name}: kind=test without a known parse reporter`);
      const floor = committed.runner.floors[`${testCase.name}@${name}`];
      assert.equal(floor, testCase.minCases,
        `${name}/${testCase.name}: minCases ${testCase.minCases} != floor ${floor}`);
    }
  }
});

// ---- F1 (MIGRATION_PLAN 补记一百零六): `all` must run the derived matrix, not fall through.
// Measured before the fix: with every planned module ready, `native:check all` printed
// "unknown module" and exited 2, because the pre-flight fell through to config.modules["all"].

function runAllAgainst(modules, plannedModules) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-gate-all-"));
  try {
    mkdirSync(path.join(dir, "scripts/native"), { recursive: true });
    for (const source of ["check.mjs", "gate-config.mjs"]) {
      cpSync(path.join(repoRoot, "scripts/native", source), path.join(dir, "scripts/native", source));
    }
    const config = JSON.parse(JSON.stringify(committed));
    config.modules = modules;
    config.plannedModules = plannedModules ?? { $comment: "probe" };
    writeFileSync(path.join(dir, "scripts/native/checks.json"), JSON.stringify(config, null, 2) + "\n");
    return spawnSync(process.execPath, [path.join(dir, "scripts/native/check.mjs"), "all"], { encoding: "utf8" });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const greenModule = { registeredBy: "test", cases: [{ name: "green", kind: "gate", argv: ["node", "-e", "0"] }] };
const redModule = { registeredBy: "test", cases: [{ name: "red", kind: "gate", argv: ["node", "-e", "process.exit(1)"] }] };

test("'all' runs every registered module once the matrix is complete", () => {
  const result = runAllAgainst({ alpha: greenModule, beta: greenModule });
  assert.equal(result.status, 0, `expected a full-matrix pass, got:\n${result.stdout}${result.stderr}`);
  assert.match(result.stdout, /native:check alpha/);
  assert.match(result.stdout, /native:check beta/);
  assert.match(result.stdout, /all 2 registered module\(s\) passed/);
});

test("a red child module makes 'all' red and names it", () => {
  const result = runAllAgainst({ alpha: greenModule, beta: redModule });
  assert.notEqual(result.status, 0, "a failing module must not pass the full gate");
  assert.match(result.stderr + result.stdout, /beta \(exit 1\)/);
  assert.match(result.stderr + result.stdout, /1\/2 module\(s\) failed/);
});

test("'all' refuses an empty derived matrix instead of passing vacuously", () => {
  const result = runAllAgainst({});
  assert.equal(result.status, 1);
  assert.match(result.stderr, /no registered module to run/);
});

test("a hand-registered module named 'all' is refused (§3.2 forbids the shortcut)", () => {
  assert.match(moduleGateProblem("all", { cases: [goodCase] }), /must not be a hand-maintained module/);
});

test("'all' still refuses while the committed matrix is incomplete", () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-gate-notready-"));
  try {
    mkdirSync(path.join(dir, "scripts/native"), { recursive: true });
    for (const source of ["check.mjs", "gate-config.mjs", "checks.json"]) {
      cpSync(path.join(repoRoot, "scripts/native", source), path.join(dir, "scripts/native", source));
    }
    const result = spawnSync(process.execPath, [path.join(dir, "scripts/native/check.mjs"), "all"], { encoding: "utf8" });
    assert.equal(result.status, 3, `committed matrix is incomplete, so 'all' must stay NOT READY:\n${result.stdout}${result.stderr}`);
    assert.match(result.stderr, /NOT READY/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// Opt-in skips. Measured hole (2026-09-29, ledger G19/G20/G24): the module-level
// `native:check tauri-adapter` ran `src-tauri-lib-tests` + `adapter-acceptance-real-kernel`,
// which rewrote three real `~/.aeroric/agent-credentials/*` sidecars and appended forged
// `invite-created` rows to the real remote-access audit log. The gate must be able to say
// "this case only runs on a scratch home" without ever turning a skip into a pass.
const optedIn = (marker) => ({
  name: "opt-in-writer", kind: "gate",
  argv: [process.execPath, "-e", `require("node:fs").writeFileSync(${JSON.stringify(marker)},"ran")`],
  optInEnv: "AERORIC_ALLOW_REAL_HOME_TESTS",
  optInReason: "writes the real ~/.aeroric",
});

test("an opt-in case is skipped while its switch is unset, and the skip is disclosed", () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-optin-"));
  const marker = path.join(dir, "ran");
  try {
    const result = runGateAgainst("optin", { cases: [goodCase, optedIn(marker)] });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.match(result.stdout, /SKIP \(opt-in\): opt-in-writer/);
    assert.match(result.stdout, /set AERORIC_ALLOW_REAL_HOME_TESTS=1 to run/);
    assert.match(result.stdout, /writes the real ~\/\.aeroric/);
    assert.ok(!existsSync(marker), "the opt-in case executed without its switch");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a skipped opt-in case is never counted among the passed ones", () => {
  // Pins the regression this replaces: the summary line printed `entry.cases.length`, so a
  // module could report "12 case(s) passed" while two of them rewrote the user's home.
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-optin2-"));
  const marker = path.join(dir, "ran");
  try {
    const result = runGateAgainst("optin_count", { cases: [goodCase, optedIn(marker)] });
    assert.match(result.stdout, /native:check optin_count: 1 case\(s\) passed/);
    assert.doesNotMatch(result.stdout, /2 case\(s\) passed/);
    assert.match(result.stdout, /1 opt-in skip: opt-in-writer/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("setting the switch to 1 runs the opt-in case for real", () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-optin3-"));
  const marker = path.join(dir, "ran");
  try {
    const result = runGateAgainst("optin_on", { cases: [optedIn(marker)] }, { AERORIC_ALLOW_REAL_HOME_TESTS: "1" });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.doesNotMatch(result.stdout, /SKIP/);
    assert.ok(existsSync(marker), "the switch did not actually enable the case");
    assert.match(result.stdout, /native:check optin_on: 1 case\(s\) passed/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("only the literal 1 opts in: 0/true/empty/yes all keep the case skipped", () => {
  // `AERORIC_ALLOW_REAL_HOME_TESTS=0` is truthy to a bare `if (env.VAR)`, and `true` is what
  // someone types when they mean yes. Neither may reach into the real home.
  for (const value of ["0", "true", "", "yes"]) {
    const reason = caseSkipReason({ name: "n", optInEnv: "E", optInReason: "r" }, { E: value });
    assert.match(reason ?? "null", /opt-in only/, `value ${JSON.stringify(value)} must not opt in`);
  }
  assert.equal(caseSkipReason({ name: "n", optInEnv: "E", optInReason: "r" }, { E: "1" }), null);
  assert.equal(caseSkipReason({ name: "n" }, {}), null);
});

test("an opt-in case that cannot say why is refused, as is a malformed variable name", () => {
  assert.match(
    moduleGateProblem("m", { cases: [{ name: "n", argv: ["node", "-e", "0"], optInEnv: "AERORIC_ALLOW_REAL_HOME_TESTS" }] }),
    /no optInReason/,
  );
  assert.match(
    moduleGateProblem("m", { cases: [{ name: "n", argv: ["node", "-e", "0"], optInEnv: "lowercase", optInReason: "r" }] }),
    /malformed optInEnv/,
  );
});

test("a red required case still fails the module even when an opt-in case is skipped", () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-optin4-"));
  const marker = path.join(dir, "ran");
  try {
    const result = runGateAgainst("optin_red", {
      cases: [{ name: "must-fail", kind: "gate", argv: [process.execPath, "-e", "process.exit(1)"] }, optedIn(marker)],
    });
    assert.equal(result.status, 1, "a skip must never mask a failure");
    // The failure summary goes to stderr (console.error); the per-case skip line goes to stdout.
    assert.match(`${result.stdout}${result.stderr}`, /1 case\(s\) failed/);
    assert.match(result.stdout, /SKIP \(opt-in\)/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("in the committed gate no case is opt-in, and the two former real-home writers run by default on a scratch root", () => {
  // G39 收口（P3-06，2026-09-30）：止血期的 opt-in 形态已被根覆盖变量取代——
  // `storage::aeroric_dir()` 认测试 SLOT > `AERORIC_HOME` > 真实家，runner 对
  // 这两条注入 `AERORIC_HOME=<scratch>`（每轮 mkdtemp、跑后清理）。
  // pin 的是集合与形状：新增 opt-in 或丢掉 scratch 注入都必须在这里红。
  const flagged = [];
  for (const [module, entry] of Object.entries(committed.modules)) {
    for (const testCase of entry.cases ?? []) {
      if (testCase.optInEnv) flagged.push(`${testCase.name}@${module}`);
    }
  }
  assert.deepEqual(flagged, [], "opt-in 通道已退役：任何 case 再挂 optInEnv 即门禁失牙");
  for (const name of ["src-tauri-lib-tests", "adapter-acceptance-real-kernel"]) {
    const testCase = committed.modules["tauri-adapter"].cases.find((c) => c.name === name);
    assert.equal(testCase.env?.AERORIC_HOME, "<scratch>", `${name} 必须带 scratch 注入`);
    assert.ok(committed.runner.floors[`${name}@tauri-adapter`] >= testCase.minCases, `${name} floor must stay declared`);
  }
});
;
