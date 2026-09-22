import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { absenceControlFailure, countCases, missingOutputTokens, moduleGateProblem } from "./gate-config.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const goodCase = { name: "does-something", kind: "gate", argv: ["node", "-e", "0"] };
const committed = JSON.parse(readFileSync(path.join(repoRoot, "scripts", "native", "checks.json"), "utf8"));

/** Run the real runner in a throwaway tree against a module defined on the fly. */
function runGateAgainst(moduleName, definition) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-gate-"));
  try {
    mkdirSync(path.join(dir, "scripts/native"), { recursive: true });
    for (const source of ["check.mjs", "gate-config.mjs"]) {
      cpSync(path.join(repoRoot, "scripts/native", source), path.join(dir, "scripts/native", source));
    }
    const config = JSON.parse(JSON.stringify(committed));
    config.modules[moduleName] = definition;
    writeFileSync(path.join(dir, "scripts/native/checks.json"), JSON.stringify(config, null, 2) + "\n");
    return spawnSync(process.execPath, [path.join(dir, "scripts/native/check.mjs"), moduleName], { encoding: "utf8" });
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
