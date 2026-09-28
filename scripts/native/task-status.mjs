// Task-status carrier: the machine-readable mirror of MIGRATION_PLAN.md §进度总表, plus the
// derivation that turns it into `feature-matrix.json`'s delivery columns.
//
// Why this file exists (F3, closed by Q7 on 2026-09-27): the matrix's `final_state`/`status`
// columns were hand-maintained and never read by any gate, so 26 rows sat on `unstarted` while
// F01/F02/F03 had long since been accepted. `inventory.mjs` only checked that an `F##` id existed
// and counted rows. A generator now owns those columns; §进度总表 and `docs/native/task-status.json`
// cross-check each other, and `--freeze-check` stays a byte-level no-op assertion.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const defaultRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
export const STATUS_VOCABULARY = ["pending", "in_progress", "blocked", "accepted"];
// `blocked` counts as "not delivered" (rank 0) but keeps its own label, because a stalled Task is
// a different fact from an untouched one and the matrix must not blur them.
export const STATE_RANK = { unstarted: 0, blocked: 0, in_progress: 1, accepted: 2 };
export const STATE_OF_STATUS = { pending: "unstarted", blocked: "blocked", in_progress: "in_progress", accepted: "accepted" };
export const NO_TASK_FOR_SIDE = "no-task-declared";
// A row whose `target` does not carry this letter at all. Distinct from `no-task-declared`, which
// means the row *does* claim the side but no Task behind the derivation table feeds it — and that
// case is an input problem `--freeze` refuses to write.
export const NO_SIDE_IN_TARGET = "not-declared";

const TASK_STATUS_PATH = "docs/native/task-status.json";
const MATRIX_PATH = "docs/native/feature-matrix.json";
const PLAN_PATH = "MIGRATION_PLAN.md";

/** Every `### Task P##-##` heading, in document order. The plan itself defines the task universe. */
export function taskUniverse(planText) {
  const found = [...planText.matchAll(/^### Task (P(\d+)-(\d+))\b/gm)];
  return found.map((m) => ({ task: m[1], phase: Number(m[2]), index: Number(m[3]) }));
}

function parseTaskCell(cell) {
  // Returns {tasks, kind} or null when the cell is not a pure task spec (a historical row such as
  // `P2-05（补记三十六～六十六）`, which must not be read as the live status).
  const trimmed = cell.trim();
  const annotated = trimmed.match(/^(P\d+-\d+)（[^）]*）$/);
  if (annotated) return { tasks: [], kind: "historical", note: trimmed };
  if (/^Phase \d+$/.test(trimmed)) return { tasks: [], kind: "phase", phase: Number(trimmed.split(" ")[1]) };
  const range = trimmed.match(/^(P(\d+)-(\d+)) ~ (P(\d+)-(\d+))$/);
  if (range) return { tasks: [], kind: "range", from: Number(range[3]), to: Number(range[5]), phase: Number(range[2]) };
  if (/^P\d+-\d+(、P\d+-\d+)*$/.test(trimmed)) {
    return { tasks: trimmed.split("、"), kind: "list" };
  }
  return null;
}

function statusOfCell(cell) {
  const bare = cell.trim();
  const bold = bare.match(/^\*\*(pending|in_progress|blocked|accepted)\b/);
  if (bold) return bold[1];
  const plain = bare.match(/^(pending|in_progress|blocked|accepted)\b/);
  return plain ? plain[1] : null;
}

/**
 * Read §进度总表 as data. The status cell must *lead* with a status word — that is what makes the
 * prose table mechanically checkable, and it is why the merged `P3-01、P3-02 **accepted**（…）` row
 * was split into one row per Task.
 */
export function parsePlanProgressTable(planText) {
  const problems = [];
  const skipped = [];
  const statuses = new Map();
  const universe = taskUniverse(planText);
  if (!universe.length) throw new Error("no `### Task P#-#` headings found in the plan — cannot build the task universe");
  const lines = planText.split("\n");
  const start = lines.findIndex((l) => l.trim() === "### 进度总表");
  if (start === -1) throw new Error("§进度总表 not found in the plan — refusing to cross-check against a table we could not read");
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i += 1) {
    if (/^### /.test(lines[i])) { end = i; break; }
  }
  for (const line of lines.slice(start, end)) {
    if (!line.startsWith("|")) continue;
    const cells = line.split("|").slice(1, -1);
    if (cells.every((c) => /^:?-{3,}:?$/.test(c.trim()))) continue; // the table's own separator row
    if (cells.length !== 4) {
      if (cells[0]?.trim() === "Task") continue;
      problems.push(`进度总表: row has ${cells.length} cells, expected 4 — ${line.slice(0, 60)}…`);
      continue;
    }
    if (cells[0].trim() === "Task") continue;
    const spec = parseTaskCell(cells[0]);
    if (!spec) {
      problems.push(`进度总表: task cell is not a pure task spec (id list, 'P#-# ~ P#-#', or 'Phase N'): ${cells[0].trim().slice(0, 60)}`);
      continue;
    }
    if (spec.kind === "historical") {
      skipped.push(spec.note);
      continue;
    }
    let tasks = spec.tasks;
    if (spec.kind === "phase") tasks = universe.filter((u) => u.phase === spec.phase).map((u) => u.task);
    if (spec.kind === "range") {
      tasks = universe.filter((u) => u.phase === spec.phase && u.index >= spec.from && u.index <= spec.to).map((u) => u.task);
    }
    if (!tasks.length) {
      problems.push(`进度总表: row ${cells[0].trim()} resolves to no Task in the universe (${spec.kind})`);
      continue;
    }
    const status = statusOfCell(cells[1]);
    if (!status) {
      problems.push(`进度总表: status cell for ${cells[0].trim()} must lead with one of ${STATUS_VOCABULARY.join("|")}`);
      continue;
    }
    for (const task of tasks) {
      if (statuses.has(task)) problems.push(`进度总表: ${task} is given a status twice (${statuses.get(task)} and ${status})`);
      else statuses.set(task, status);
    }
  }
  for (const u of universe) if (!statuses.has(u.task)) problems.push(`进度总表: ${u.task} has no row in the table`);
  for (const task of statuses.keys()) if (!universe.some((u) => u.task === task)) problems.push(`进度总表: ${task} is not a declared Task heading`);
  return { statuses, skipped, problems, universe };
}

/** Load the canonical carrier. Missing or malformed input must throw, never degrade to "no change". */
export function loadTaskStatus({ repoRoot = defaultRoot, read = (p) => readFileSync(p, "utf8") } = {}) {
  const file = path.join(repoRoot, TASK_STATUS_PATH);
  let parsed;
  try {
    parsed = JSON.parse(read(file));
  } catch (error) {
    throw new Error(`${TASK_STATUS_PATH} unreadable: ${error.message}`);
  }
  if (!Array.isArray(parsed.tasks) || !parsed.tasks.length) throw new Error(`${TASK_STATUS_PATH} has no tasks[] — refusing to treat an empty carrier as "no change"`);
  const problems = [];
  const statuses = new Map();
  const reviews = new Map();
  for (const entry of parsed.tasks) {
    const { task, status, accepted_on: acceptedOn, review } = entry ?? {};
    if (!/^P\d+-\d+$/.test(task ?? "")) problems.push(`task-status.json: bad task id ${JSON.stringify(task)}`);
    if (!STATUS_VOCABULARY.includes(status)) problems.push(`${task}: status '${status}' is outside ${STATUS_VOCABULARY.join("|")}`);
    if (status === "accepted" && !acceptedOn) problems.push(`${task}: accepted without accepted_on`);
    if (status !== "accepted" && acceptedOn) problems.push(`${task}: carries accepted_on but is '${status}'`);
    if (statuses.has(task)) problems.push(`${task}: listed twice in task-status.json`);
    statuses.set(task, status);
    reviews.set(task, review ?? null);
    if (review && !existsSync(path.join(repoRoot, "docs/native", review))) problems.push(`${task}: review file does not exist: docs/native/${review}`);
  }
  if (problems.length) throw new Error(`task-status.json invalid:\n  ${problems.join("\n  ")}`);
  // `meta.counts` is written by a person, so it is checked rather than trusted: a summary that has
  // drifted from the entries below it is exactly the "looks maintained, is stale" shape F3 was.
  const counts = parsed.meta?.counts;
  if (counts) {
    const tallied = { tasks: parsed.tasks.length };
    for (const s of STATUS_VOCABULARY) tallied[s] = parsed.tasks.filter((t) => t.status === s).length;
    for (const [key, value] of Object.entries(counts)) {
      if (!(key in tallied)) { problems.push(`meta.counts.${key} is not a countable field (tasks|${STATUS_VOCABULARY.join("|")})`); continue; }
      if (tallied[key] !== value) problems.push(`meta.counts.${key} says ${value} but the entries below tally ${tallied[key]}`);
    }
    if (problems.length) throw new Error(`task-status.json invalid:\n  ${problems.join("\n  ")}`);
  }
  return { meta: parsed.meta ?? {}, statuses, reviews, tasks: parsed.tasks };
}

export function crossCheckTaskStatus({ carrier, table }) {
  const problems = [];
  for (const [task, status] of carrier) {
    if (!table.has(task)) { problems.push(`进度总表 has no row for ${task} (task-status.json says ${status})`); continue; }
    if (table.get(task) !== status) problems.push(`${task}: 进度总表 says '${table.get(task)}' but task-status.json says '${status}' — 任一侧改了另一侧没改`);
  }
  for (const [task, status] of table) if (!carrier.has(task)) problems.push(`${task}: 进度总表 says '${status}' but task-status.json has no entry`);
  return problems;
}

/**
 * Derive the matrix's delivery columns from the carrier. A row's side (C/N/A/K) is fed by the Tasks
 * whose phase belongs to that side; the side takes the least-advanced status among them.
 */
export function deriveFeatureStates({ features, statuses, reviews, sidePhases, universe }) {
  const phaseOf = new Map(universe.map((u) => [u.task, u.phase]));
  const rows = [];
  const findings = [];
  for (const feature of features) {
    const letters = String(feature.target ?? "").split("+").map((s) => s.trim()).filter(Boolean);
    const tasks = String(feature.task_ids ?? "").split(",").map((s) => s.trim()).filter(Boolean);
    const sideStates = {};
    for (const letter of letters) {
      const allowed = sidePhases[letter];
      if (!allowed) { sideStates[letter] = NO_TASK_FOR_SIDE; findings.push(`${feature.id}: target letter '${letter}' has no side table`); continue; }
      const contributing = tasks.filter((t) => allowed.includes(phaseOf.get(t) ?? -1));
      if (!contributing.length) {
        sideStates[letter] = NO_TASK_FOR_SIDE;
        findings.push(`${feature.id}: side '${letter}' has no Task among '${feature.task_ids}' (phases ${allowed.join("/")})`);
        continue;
      }
      const ranked = contributing
        .map((t) => ({ task: t, state: STATE_OF_STATUS[statuses.get(t)] ?? "unstarted" }))
        .sort((a, b) => STATE_RANK[a.state] - STATE_RANK[b.state] || (a.state === "blocked" ? -1 : 0) - (b.state === "blocked" ? -1 : 0));
      sideStates[letter] = ranked[0].state;
    }
    const rowReviews = [...new Set(tasks.map((t) => reviews.get(t)).filter(Boolean))].sort();
    rows.push({
      id: feature.id,
      core_state: sideStates.C ?? NO_SIDE_IN_TARGET,
      native_state: sideStates.N ?? NO_SIDE_IN_TARGET,
      side_states: sideStates,
      reviews: rowReviews,
    });
  }
  return { rows, findings };
}

/** Compare the committed matrix against what the carrier says it should be. */
export function compareMatrixDerived({ matrix, derived }) {
  const problems = [];
  const byId = new Map(derived.rows.map((r) => [r.id, r]));
  for (const feature of matrix.features) {
    const want = byId.get(feature.id);
    if (!want) { problems.push(`feature-matrix: ${feature.id} is not in the derived set`); continue; }
    if ("final_state" in feature) problems.push(`${feature.id}: final_state is a hand-maintained column superseded by core_state/native_state — run \`node scripts/native/inventory.mjs --freeze\``);
    for (const key of ["core_state", "native_state", "side_states", "reviews"]) {
      if (JSON.stringify(feature[key]) !== JSON.stringify(want[key])) {
        problems.push(`${feature.id}.${key}: committed ${JSON.stringify(feature[key] ?? null)}, carrier says ${JSON.stringify(want[key])} — run \`node scripts/native/inventory.mjs --freeze\``);
      }
    }
  }
  return problems;
}

/** `status` stays human-owned, so its vocabulary and its cross-references are validated instead. */
export function validateMatrixStatuses(matrix) {
  const problems = [];
  const allowed = new Set(["planned", "planned-new-capability-no-existing-command"]);
  for (const feature of matrix.features) {
    if (!allowed.has(feature.status)) problems.push(`${feature.id}: status '${feature.status}' is outside the documented vocabulary`);
  }
  const claimed = new Set((matrix.meta?.counts?.features_with_zero_existing_commands ?? []));
  const actually = new Set(matrix.features.filter((f) => f.status === "planned-new-capability-no-existing-command").map((f) => f.id));
  for (const id of claimed) if (!actually.has(id)) problems.push(`meta.counts.features_with_zero_existing_commands lists ${id} but its status says otherwise`);
  for (const id of actually) if (!claimed.has(id)) problems.push(`${id} is marked 'planned-new-capability-no-existing-command' but is absent from meta.counts`);
  return problems;
}

export function freezeFeatureMatrix({  repoRoot = defaultRoot,
  read = (p) => readFileSync(p, "utf8"),
  put = (p, data) => writeFileSync(p, data),
  dryRun = false,
  derived,
} = {}) {
  const file = path.join(repoRoot, MATRIX_PATH);
  const original = read(file);
  const matrix = JSON.parse(original);
  const byId = new Map(derived.rows.map((r) => [r.id, r]));
  let written = 0;
  for (const feature of matrix.features) {
    const row = byId.get(feature.id);
    if (!row) throw new Error(`freeze: no derived state for ${feature.id}`);
    delete feature.final_state;
    feature.core_state = row.core_state;
    feature.native_state = row.native_state;
    feature.side_states = row.side_states;
    feature.reviews = row.reviews;
    written += 1;
  }
  matrix.meta = matrix.meta ?? {};
  matrix.meta.derived_columns = {
    rule: "core_state/native_state/side_states/reviews 由 docs/native/task-status.json 派生（scripts/native/task-status.mjs::deriveFeatureStates），禁止手改；final_state 已删除，它正是 F3 里腐烂的那一列。取值词表：unstarted|blocked|in_progress|accepted（一条边取其所依赖 Task 中最低秩的状态），`not-declared`=该行的 target 里根本没有这一半，`no-task-declared`=行了这一半但派生表里找不到 Task（属输入不一致，--freeze 会拒绝写出）。",
    carrier: TASK_STATUS_PATH,
    plan_table: `${PLAN_PATH} §进度总表`,
    generated_from_rows: derived.rows.length,
    sides_without_declared_task: derived.findings.length,
  };
  const serialized = `${JSON.stringify(matrix, null, 2)}\n`;
  const changed = serialized !== original;
  if (dryRun) {
    return { changed, rows: written, serialized };
  }
  if (changed) put(file, serialized);
  return { changed, rows: written, serialized };
}

/**
 * One call does the whole cross-check bundle: plan table ⇄ carrier ⇄ derived matrix columns.
 * Anything unreadable must throw or become a problem (D12) — "we could not look" must never be
 * reported as "nothing changed".
 *
 * `planText` lets the caller read §进度总表 from the real tree while the frozen artifacts come from
 * a throwaway copy; `inventory.mjs` already keeps those two roots apart (`sourceRoot ?? repoRoot`),
 * and a temp-tree negative test is only real if the table it mutates is the one being read.
 */
export function carrierCrossCheck({ repoRoot = defaultRoot, read = (p) => readFileSync(p, "utf8"), matrix, planText } = {}) {
  const plan = planText ?? read(path.join(repoRoot, PLAN_PATH));
  const carrier = loadTaskStatus({ repoRoot, read });
  const table = parsePlanProgressTable(plan);
  // Two classes of problem, because `--freeze` may only repair the second one: the mirror inputs
  // disagree (nothing safe to derive from), versus the committed columns lag behind a correct
  // derivation (regenerate and review the diff).
  const inputProblems = [
    ...table.problems,
    ...crossCheckTaskStatus({ carrier: carrier.statuses, table: table.statuses }),
  ];
  const universe = taskUniverse(plan);
  const sidePhases = carrier.meta?.side_phases;
  if (!sidePhases) throw new Error(`${TASK_STATUS_PATH} has no meta.side_phases — the derivation table is missing`);
  const derived = deriveFeatureStates({
    features: matrix.features,
    statuses: carrier.statuses,
    reviews: carrier.reviews,
    sidePhases,
    universe,
  });
  // An edge with no Task behind it is not "unstarted", it is unaccounted for: §1.2 forbids closing a
  // row with a vague owner, so the derivation table itself has to be checkable.
  for (const finding of derived.findings) inputProblems.push(`task-status derivation: ${finding}`);
  const problems = [
    ...inputProblems,
    ...compareMatrixDerived({ matrix, derived }),
    ...validateMatrixStatuses(matrix),
  ];
  const accepted = [...carrier.statuses.values()].filter((s) => s === "accepted").length;
  return { problems, inputProblems, findings: derived.findings, derived, carrier, table, stats: { tasks: carrier.statuses.size, accepted, rows: derived.rows.length, historicalRows: table.skipped.length } };
}
