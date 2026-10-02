// Negative tests for the F3/Q7 carrier (docs/native/task-status.json) and the derived
// feature-matrix columns. Every guard here is asserted through the branch's own message text, and
// the discipline being proved is D12: "we could not read it" must never look like "nothing changed".
import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { check, freeze } from "./inventory.mjs";
import {
  carrierCrossCheck,
  freezeFeatureMatrix,
  loadTaskStatus,
  parsePlanProgressTable,
  taskUniverse,
} from "./task-status.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const CARRIER = "docs/native/task-status.json";
const MATRIX = "docs/native/feature-matrix.json";
const COPIED = [
  "src-tauri/src/lib.rs",
  "docs/native/command-map.json",
  MATRIX,
  CARRIER,
  "scripts/native/checks.json",
];

function mirrorTree(mutate = () => {}) {
  // A throwaway copy of the frozen artifacts; the plan table is still read from the real tree, the
  // same split `inventory.test.mjs` relies on (frozen files from repoRoot, sources from sourceRoot).
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-task-status-"));
  for (const sub of ["docs/native", "scripts/native", "src-tauri/src"]) {
    mkdirSync(path.join(dir, sub), { recursive: true });
  }
  for (const file of COPIED) cpSync(path.join(repoRoot, file), path.join(dir, file));
  cpSync(path.join(repoRoot, "docs/native/reviews"), path.join(dir, "docs/native/reviews"), { recursive: true });
  mutate(dir);
  return { dir, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

const readJson = (dir, rel) => JSON.parse(readFileSync(path.join(dir, rel), "utf8"));
const writeJson = (dir, rel, doc) => writeFileSync(path.join(dir, rel), `${JSON.stringify(doc, null, 2)}\n`);
const problemText = (problems) => problems.join("\n");

function crossCheckIn(dir) {
  return carrierCrossCheck({
    repoRoot: dir,
    read: (p) => readFileSync(p, "utf8"),
    matrix: readJson(dir, MATRIX),
    planText: readFileSync(path.join(repoRoot, "MIGRATION_PLAN.md"), "utf8"),
  });
}

test("the committed carrier, the plan table and the derived columns all agree today", () => {
  // Positive control: this only proves something if the reader really found the rows.
  const bundle = crossCheckIn(repoRoot);
  assert.deepEqual(bundle.problems, [], problemText(bundle.problems));
  assert.equal(bundle.stats.tasks, 51);
  // `accepted` 是进度变量——每个合法验收都会 +1，绝对值断言会在验收日把门禁钉红
  // （2026-09-30 P3-04 accepted 当日实测：14 !== 13，补记二百四十八）。正控的正确形状：
  // 独立重算载体的 tally 与 reader 的统计比对（证明读者真找到了行），再加下界（防止 0）。
  assert.equal(
    bundle.stats.accepted,
    readJson(repoRoot, CARRIER).tasks.filter((t) => t.status === "accepted").length,
  );
  assert.ok(bundle.stats.accepted >= 14, "the reader must be finding the accepted rows");
  assert.equal(bundle.stats.rows, 26);
  assert.equal(bundle.findings.length, 0, `every declared side must have a Task: ${bundle.findings.join("; ")}`);
});

test("the derived columns are what a finished core Task looks like", () => {
  // F3 was exactly this contradiction: F01-F03 accepted while the matrix said `unstarted`.
  const matrix = readJson(repoRoot, MATRIX);
  const byId = new Map(matrix.features.map((f) => [f.id, f]));
  assert.equal(byId.get("F01").core_state, "accepted");
  assert.equal(byId.get("F04").core_state, "accepted");
  // P5-01 开工后 F04 的原生面进入 in_progress（补记三百一十）。
  assert.equal(byId.get("F04").native_state, "in_progress");
  assert.deepEqual(byId.get("F04").reviews, ["reviews/terminal-core.md"]);
  assert.equal(matrix.features.some((f) => "final_state" in f), false, "the hand-written column is gone");
  assert.equal(byId.get("F24").core_state, "not-declared", "F24's target is N only");
});

test("inventory --check really runs the cross-check, not just the unit tests", () => {
  const result = check({ repoRoot });
  assert.equal(result.ok, true, problemText(result.problems.slice(0, 5)));
  assert.equal(result.taskStates.tasks, 51);
  assert.equal(
    result.taskStates.accepted,
    readJson(repoRoot, CARRIER).tasks.filter((t) => t.status === "accepted").length,
  );
  assert.ok(result.taskStates.accepted >= 14);
});

test("a missing carrier must be an error, never 'nothing changed'", () => {
  const { dir, cleanup } = mirrorTree((d) => unlinkSync(path.join(d, CARRIER)));
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false, "an unreadable carrier may not pass");
    assert.match(problemText(result.problems), /task-status carrier unreadable/);
  } finally { cleanup(); }
});

test("an empty tasks array is refused rather than read as an empty mirror", () => {
  const { dir, cleanup } = mirrorTree((d) => writeJson(d, CARRIER, { meta: {}, tasks: [] }));
  try {
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false);
    assert.match(problemText(result.problems), /refusing to treat an empty carrier/);
  } finally { cleanup(); }
});

test("a status flipped on one side of the mirror names both sides", () => {
  const { dir, cleanup } = mirrorTree((d) => {
    const carrier = readJson(d, CARRIER);
    const row = carrier.tasks.find((t) => t.task === "P2-01");
    row.status = "pending";
    delete row.accepted_on;
    carrier.meta.counts.accepted -= 1;
    carrier.meta.counts.pending += 1;
    writeJson(d, CARRIER, carrier);
  });
  try {
    // Through the gate path, not just the module: `--check` is what P2–P10 actually run. The match is
    // the assertion; `ok===false` alone would be vacuous here, since a temp tree also trips the
    // stale-source-path rules.
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.match(problemText(result.problems), /P2-01: 进度总表 says 'accepted' but task-status.json says 'pending'/);
  } finally { cleanup(); }
});

test("a carrier entry outside the status vocabulary is rejected", () => {
  const { dir, cleanup } = mirrorTree((d) => {
    const carrier = readJson(d, CARRIER);
    carrier.tasks.find((t) => t.task === "P3-03").status = "done";
    writeJson(d, CARRIER, carrier);
  });
  try {
    assert.throws(() => loadTaskStatus({ repoRoot: dir }), /status 'done' is outside pending\|in_progress\|blocked\|accepted/);
  } finally { cleanup(); }
});

test("`accepted` without a date, and a date without `accepted`, are both refused", () => {
  const base = readJson(repoRoot, CARRIER);
  const missing = { ...base.tasks.find((t) => t.task === "P2-04") };
  delete missing.accepted_on;
  assert.throws(
    () => loadTaskStatus({ repoRoot, read: () => JSON.stringify({ meta: base.meta, tasks: [...base.tasks.filter((t) => t.task !== "P2-04"), missing] }) }),
    /P2-04: accepted without accepted_on/,
  );
  // 第二段曾钉 P3-04（in_progress 现势）——P3-04 结案后该载体失效（补记二百四十八）。
  // 正确形状：动态取第一个非 accepted 的 Task 做「提前日期」突变。
  const live = base.tasks.find((t) => t.status !== "accepted");
  const premature = base.tasks.map((t) => (t.task === live.task ? { ...t, accepted_on: "2026-09-29" } : t));
  assert.throws(
    () => loadTaskStatus({ repoRoot, read: () => JSON.stringify({ meta: base.meta, tasks: premature }) }),
    new RegExp(live.task + ": carries accepted_on but is '" + live.status + "'"),
  );
});

test("a review path that does not exist is refused instead of cited as evidence", () => {
  assert.throws(
    () => loadTaskStatus({
      repoRoot,
      read: () => JSON.stringify({ meta: {}, tasks: [{ task: "P1-04", status: "accepted", accepted_on: "2026-09-21", review: "reviews/never-written.md" }] }),
    }),
    /review file does not exist: docs\/native\/reviews\/never-written\.md/,
  );
});

test("a hand-edited derived column is reported as drift against the carrier", () => {
  const { dir, cleanup } = mirrorTree((d) => {
    const matrix = readJson(d, MATRIX);
    matrix.features.find((f) => f.id === "F01").core_state = "unstarted";
    writeJson(d, MATRIX, matrix);
  });
  try {
    // The F3 failure mode was precisely this column being written by hand and read by nobody, so the
    // assertion has to run through the gate that now reads it.
    const result = check({ repoRoot: dir, sourceRoot: repoRoot });
    assert.equal(result.ok, false, "a hand-written delivery column may not pass");
    assert.match(problemText(result.problems), /F01\.core_state: committed "unstarted", carrier says "accepted"/);
  } finally { cleanup(); }
});

test("re-adding the hand-maintained final_state column fails", () => {
  const { dir, cleanup } = mirrorTree((d) => {
    const matrix = readJson(d, MATRIX);
    matrix.features[0].final_state = "done";
    writeJson(d, MATRIX, matrix);
  });
  try {
    const bundle = crossCheckIn(dir);
    assert.match(problemText(bundle.problems), /final_state is a hand-maintained column superseded by core_state\/native_state/);
  } finally { cleanup(); }
});

test("a status word outside the documented matrix vocabulary is refused", () => {
  const { dir, cleanup } = mirrorTree((d) => {
    const matrix = readJson(d, MATRIX);
    matrix.features.find((f) => f.id === "F26").status = "shipped";
    writeJson(d, MATRIX, matrix);
  });
  try {
    const bundle = crossCheckIn(dir);
    assert.match(problemText(bundle.problems), /F26: status 'shipped' is outside the documented vocabulary/);
  } finally { cleanup(); }
});

test("a derivation table that stops covering a phase names the rows that lost their Task", () => {
  const { dir, cleanup } = mirrorTree((d) => {
    const carrier = readJson(d, CARRIER);
    carrier.meta.side_phases.C = carrier.meta.side_phases.C.filter((p) => p !== 6);
    writeJson(d, CARRIER, carrier);
  });
  try {
    const bundle = crossCheckIn(dir);
    assert.match(problemText(bundle.problems), /task-status derivation: F07: side 'C' has no Task among/);
    assert.equal(check({ repoRoot: dir, sourceRoot: repoRoot }).problems.filter((p) => p.includes("side 'C' has no Task among")).length, 4, "the same drift must reach --check (F07–F10)");
    // The refusal has to bind `--freeze` too, or a drifted table would silently rewrite the artifact.
    assert.equal(bundle.inputProblems.length > 0, true);
    const map = readJson(dir, "docs/native/command-map.json");
    map.commands = []; // this test only needs the carrier path of --freeze, not 660 signature reads
    writeJson(dir, "docs/native/command-map.json", map);
    assert.throws(
      () => freeze({
        repoRoot: dir,
        sourceRoot: repoRoot,
        live: { events: [], panels: [], unresolved: { dynamic_emitters: [], dynamic_listeners: [] } },
      }),
      /refusing to freeze while the mirror inputs disagree/,
    );
  } finally { cleanup(); }
});

test("a hand-edited meta.counts summary is caught against the entries", () => {
  assert.throws(
    () => loadTaskStatus({
      repoRoot,
      read: () => {
        const base = readJson(repoRoot, CARRIER);
        return JSON.stringify({ meta: { ...base.meta, counts: { ...base.meta.counts, accepted: 99 } }, tasks: base.tasks });
      },
    }),
    // tally 数字同样是进度现势——由载体独立计数构造期望串（补记二百四十八）。
    new RegExp(
      "meta\\.counts\\.accepted says 99 but the entries below tally " +
        readJson(repoRoot, CARRIER).tasks.filter((t) => t.status === "accepted").length,
    ),
  );
});

test("a Task with a heading but no row in §进度总表 is a problem, not an absence", () => {
  const plan = readFileSync(path.join(repoRoot, "MIGRATION_PLAN.md"), "utf8");
  const universe = taskUniverse(plan);
  assert.ok(universe.length >= 51, `the plan defines the task universe, found ${universe.length}`);
  const withoutP205 = plan.replace(/^\| P2-05 \|.*$/m, "");
  const table = parsePlanProgressTable(withoutP205);
  assert.match(problemText(table.problems), /进度总表: P2-05 has no row in the table/);
});

test("the historical annotated row is not read as the live status", () => {
  const plan = readFileSync(path.join(repoRoot, "MIGRATION_PLAN.md"), "utf8");
  const table = parsePlanProgressTable(plan);
  assert.deepEqual(table.skipped, ["P2-05（补记三十六～六十六）"], "only the recorded historical row may be skipped");
  assert.equal(table.statuses.get("P2-05"), "accepted");
  assert.equal(table.problems.length, 0, problemText(table.problems));
});

test("a second row for the same Task is refused rather than last-one-wins", () => {
  const plan = readFileSync(path.join(repoRoot, "MIGRATION_PLAN.md"), "utf8");
  const row = plan.split("\n").find((l) => l.startsWith("| P1-01 |"));
  assert.ok(row, "the P1-01 row must exist to be duplicated");
  const table = parsePlanProgressTable(plan.replace(row, () => `${row}\n${row}`));
  assert.match(problemText(table.problems), /P1-01 is given a status twice \(accepted and accepted\)/);
});

test("freezing the committed artifacts is a byte-level noop", () => {
  const bundle = crossCheckIn(repoRoot);
  const result = freezeFeatureMatrix({ repoRoot, dryRun: true, derived: bundle.derived });
  assert.equal(result.changed, false, "the committed matrix already equals its derivation");
  assert.equal(result.rows, 26);
});
