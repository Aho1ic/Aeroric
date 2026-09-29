import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  GENERATED_END, GENERATED_START, actionTargetsFor, expandBraced, expandModules, hazardsFor, parseTaskActions,
  refreshRegister, registerDrift, renderActionCell, renderGeneratedBlock, renderRiskRegister, renderRow, spliceGenerated,
  splitPublicSymbols, tableDrift,
} from "./extraction.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const FILE = [
  "use std::sync::Mutex;",
  "pub struct Config { pub inner: u32 }",
  "pub(crate) enum Kind { A }",
  "impl Config {",
  "    pub fn method_in_impl(&self) -> u32 { self.inner }",
  "}",
  "pub fn top() {}",
  "pub const LIMIT: u32 = 3;",
  "pub mod wiring;",
  "",
  "#[cfg(test)]",
  "mod tests {",
  "    use super::*;",
  "    #[test]",
  "    fn one() {}",
  "    #[tokio::test(flavor = \"multi_thread\")]",
  "    async fn two() {}",
  "}",
].join("\n");

test("test items are counted as the #[test] attributes that exist", () => {
  const { testItems } = splitPublicSymbols(FILE);
  assert.equal(testItems, 2, "one bare #[test] plus one parameterised #[tokio::test(...)]");
});

test("only column-zero declarations count as moved surface", () => {
  const { symbols } = splitPublicSymbols(FILE);
  assert.deepEqual(symbols, ["Const LIMIT", "Enum Kind", "Mod wiring", "Struct Config", "top()"]);
  assert.ok(!symbols.some((s) => s.includes("method_in_impl")), "impl methods are not exported surface");
});

test("a field-level #[cfg(test)] must not open a test module", () => {
  const text = ["pub struct S {", "    #[cfg(test)]", "    fail: Option<String>,", "}", "pub fn real() {}"].join("\n");
  const { symbols, testItems } = splitPublicSymbols(text);
  assert.deepEqual(symbols, ["Struct S", "real()"]);
  assert.equal(testItems, 0);
});

test("hazards name what §5.1 step 5 requires verifying", () => {
  const hazards = hazardsFor("use tauri::AppHandle;\ninclude_str!(\"x.sql\")\ntokio::spawn(async {});\n#[cfg(windows)]\nfn keychain() {}", 3);
  for (const expected of ["command-layer", "AppHandle/State", "include_str! path rewrite", "spawns task/thread (needs a cancellation token)", "cfg(windows) branch", "credential owner"]) {
    assert.ok(hazards.includes(expected), `missing hazard ${expected}`);
  }
  assert.deepEqual(hazardsFor("fn plain() {}", 0), []);
});

test("renderRow truncates long symbol lists and keeps the test count", () => {
  const row = {
    source: "a.rs", destination: "b.rs", commands: 1, testItems: 4, hazards: [],
    symbols: Array.from({ length: 17 }, (_, i) => `f${i}()`),
    action: actionTargetsFor("a.rs", ["core/metrics/a.rs"]),
  };
  const line = renderRow(3, row);
  assert.match(line, /^\| 3 \| `a\.rs` \| `b\.rs` \| 1 \| /);
  assert.match(line, /\+2 more \(4 test items excluded\) \| `core\/metrics\/a\.rs` \| — \|$/);
});

test("tableDrift catches a stale row, a moved destination and a changed wrapper count", () => {
  const rows = [{
    source: "x.rs", destination: "core/x.rs", commands: 2, testItems: 1, symbols: ["a()"], hazards: [],
    action: actionTargetsFor("x.rs", ["core/scope/x.rs"]),
  }];
  const good = "| # | source | destination | commands | moved symbols (top-level public) | Action 给定的落点 | per-file hazards to verify |\n|---|---|---|---|---|---|---|\n| 1 | `x.rs` | `core/x.rs` | 2 | a() (1 test items excluded) | `core/scope/x.rs` | — |";
  assert.deepEqual(tableDrift({ committedText: good, rows }), []);
  assert.match(tableDrift({ committedText: good.replace("| 2 | a()", "| 3 | a()"), rows }).join("\n"), /command wrappers differ/);
  assert.match(tableDrift({ committedText: good.replace("core/x.rs", "core/moved.rs"), rows }).join("\n"), /destination differs/);
  assert.match(tableDrift({ committedText: good.replace("| 1 | `x.rs`", "| 1 | `gone.rs`"), rows }).join("\n"), /not in the current source tree/);
  assert.match(tableDrift({ committedText: good.split("\n").slice(0, 2).join("\n"), rows }).join("\n"), /row count differs|no §5\.1 table rows/);
  // The ② column is compared too: a hand-edited landing point must not pass as the Action's word.
  assert.match(tableDrift({ committedText: good.replace("`core/scope/x.rs`", "`core/hand/wave.rs`"), rows }).join("\n"), /Action-target cell differs/);
  // …and the quoted Action block: the banner is the only place the plan's own words live, so
  // rewriting the plan must be able to make `--check` red without touching a single table cell.
  const withQuote = `${good}\n\n> **Task P3-04 — Action 原文（机械抽取）**\n> 1. 新建 \`core/scope/{x}.rs\`。\n`;
  assert.deepEqual(tableDrift({ committedText: withQuote, rows, actionDocs: [{ task: "P3-04", verbatim: ["1. 新建 `core/scope/{x}.rs`。"], targets: ["core/scope/x.rs"] }] }), []);
  assert.match(
    tableDrift({ committedText: withQuote, rows, actionDocs: [{ task: "P3-04", verbatim: ["1. 新建 `core/scope/{y}.rs`。"], targets: [] }] }).join("\n"),
    /no longer quotes Action line/,
  );
});

test("a committed table with no rows fails instead of passing quietly", () => {
  assert.match(tableDrift({ committedText: "# nothing here\n", rows: [] }).join("\n"), /no §5\.1 table rows/);
});

test("expanding a module that yields no files is an error, not an empty document", () => {
  assert.throws(
    () => expandModules({ modules: ["definitely_not_a_module"] }),
    /no \.rs files found/,
    "an empty expansion would silently certify a module nobody looked at",
  );
});

test("the CLI refuses to write when wrapper counts disagree with command-map.json", () => {
  const result = spawnSync(process.execPath, [
    path.join(repoRoot, "scripts/native/extraction.mjs"),
    "--module=aeroric_cli_not_a_dir", "--write", "/tmp/extraction-should-not-exist.md",
  ], { encoding: "utf8" });
  assert.notEqual(result.status, 0, `bad module must fail, got ${result.status}`);
  assert.match(result.stderr + result.stdout, /refusing to write|no \.rs files found/);
});

// The ② machinery (表态项 14, answered 2026-09-29): the plan's Action block becomes the only
// authority for landing points, and `--write` must stop eating hand-written prose.
test("expandBraced turns one Action brace group into real paths", () => {
  assert.deepEqual(
    expandBraced("core/crates/aeroric-core/src/sessions/{mod,export}.rs"),
    ["core/crates/aeroric-core/src/sessions/mod.rs", "core/crates/aeroric-core/src/sessions/export.rs"],
  );
  assert.deepEqual(expandBraced("plain/path.rs"), ["plain/path.rs"]);
  // A malformed candidate (no closing brace) must come back unchanged, not silently empty.
  assert.deepEqual(expandBraced("broken/{a,b"), ["broken/{a,b"]);
});

test("parseTaskActions reads only the Action block, from the real plan text", () => {
  const plan = [
    "### Task P9-01 — probe",
    "- **Input**：`session.rs`、`session/export.rs`。",
    "- **Action**：",
    "  1. 新建 `core/x/{a,b}.rs`，迁移行为。",
    "  2. 引用 `metrics/usage.rs` 与 `tests/never_a_target.rs`。",
    "     - 子条 `core/x/c.rs`。",
    "- **Acceptance Criteria**：",
    "  - 新建 `core/x/acceptance_leak.rs`。",
    "### Task P9-02 — next",
    "- **Action**：",
    "  1. 别的 Task `core/other.rs`。",
  ].join("\n");
  const [doc] = parseTaskActions(plan, ["P9-01"]);
  assert.deepEqual(doc.targets, ["core/x/a.rs", "core/x/b.rs", "core/x/c.rs", "metrics/usage.rs"],
    "Input paths (host sources), Acceptance paths and tests/*.rs must never become landing points");
  assert.equal(doc.verbatim.length, 3, "only the Action steps are quoted");
  assert.doesNotMatch(doc.verbatim.join("\n"), /Acceptance Criteria|Input/);
  assert.equal(parseTaskActions(plan, ["P9-99"])[0].verbatim.length, 0, "a Task that is not in the plan yields nothing, and the CLI must then refuse");
});

test("the plan itself is the source: P3-04's Action names sessions/export.rs and not session.rs", () => {
  const planText = readFileSync(path.join(repoRoot, "MIGRATION_PLAN.md"), "utf8");
  const [doc] = parseTaskActions(planText, ["P3-04"]);
  assert.ok(doc.verbatim.length >= 5, `P3-04 Action should quote several steps, got ${doc.verbatim.length}`);
  assert.ok(doc.targets.includes("core/crates/aeroric-core/src/sessions/export.rs"), doc.targets.join(" "));
  assert.ok(!doc.targets.some((t) => t === "session.rs"), "the host source must not be its own destination");
  assert.ok(!doc.targets.some((t) => t.startsWith("tests/")), "test suites are not landing points");
  const named = actionTargetsFor("src-tauri/src/session/export.rs", doc.targets);
  assert.equal(named.kind, "named");
  assert.deepEqual(named.paths, ["core/crates/aeroric-core/src/sessions/export.rs"]);
  // ② refuses to guess: `session.rs` splits into several responsibility files, so it stays unmapped.
  assert.equal(actionTargetsFor("src-tauri/src/session.rs", doc.targets).kind, "unmapped");
  assert.match(renderActionCell(actionTargetsFor("src-tauri/src/session.rs", doc.targets)), /^unmapped/);
});

test("spliceGenerated rewrites only the marked region and refuses an unmarked document", () => {
  const doc = `# title\n\n> preamble\n\n${GENERATED_START}\nold table\n${GENERATED_END}\n\n## hand section\n\nkeep me\n`;
  const merged = spliceGenerated(doc, "new table");
  assert.ok(merged.includes("keep me"), "hand-written prose must survive");
  assert.ok(merged.includes("new table"));
  assert.ok(!merged.includes("old table"));
  assert.equal(spliceGenerated("# title\n\n| # | source |\n", "x"), null, "no markers means no safe splice");
});

test("the CLI refuses to overwrite an unmarked expansion document", () => {
  // Measured 2026-09-29: an unmarked --write destroyed four hand sections of sessions.md
  // (169 lines / 25 883 B -> 30 lines). The refusal is the regression test for that.
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-extraction-"));
  const target = path.join(dir, "unmarked.md");
  writeFileSync(target, "# title\n\n## 手写约束\n\nprecious\n");
  const result = spawnSync(process.execPath, [
    path.join(repoRoot, "scripts/native/extraction.mjs"),
    "--module=clock", "--write", target,
  ], { encoding: "utf8", cwd: repoRoot });
  assert.notEqual(result.status, 0, `an unmarked document must not be overwritten, got ${result.status}`);
  assert.match(`${result.stderr}${result.stdout}`, /refusing to overwrite/);
  assert.equal(readFileSync(target, "utf8"), "# title\n\n## 手写约束\n\nprecious\n", "the refusal must leave the file byte-identical");
  rmSync(dir, { recursive: true, force: true });
});

test("the generated block says the destination column is a mirror, not a landing point", () => {
  const rows = [{ source: "a.rs", destination: "core/crates/aeroric-core/m/a-root.rs", commands: 0, testItems: 0, symbols: [], hazards: [], action: { kind: "unmapped", paths: [] } }];
  const block = renderGeneratedBlock({ rows, actionDocs: [{ task: "P9-01", verbatim: ["1. step."], targets: [] }] });
  assert.match(block, /表态项 14 定为 ②/);
  assert.match(block, /> \*\*Task P9-01 — Action 原文（机械抽取）\*\*/);
  assert.match(block, /^> 1\. step\.$/m);
  assert.match(block, /unmapped（Action 未给同名文件/);
  // ② keeps the mirror exactly as it was: no src/ insertion, or the column would look authoritative.
  assert.doesNotMatch(block, /aeroric-core\/src\/m\//);
});

// The aggregate risk register is generated data. Two of the four compared documents were caught
// carrying stale counts on 2026-09-29 (`lines: **9323**` vs a 9349 tree, `**9977**` vs 9962) while
// `--check` stayed green, because nothing ever compared that section.
const REGISTER_ROWS = [
  { source: "a.rs", destination: "core/m/a.rs", commands: 2, testItems: 3, symbols: ["one()", "two()"], hazards: ["command-layer", "credential owner"], lines: 40, action: { kind: "unmapped", paths: [] } },
  { source: "b.rs", destination: "core/m/b.rs", commands: 1, testItems: 1, symbols: [], hazards: ["emits events"], lines: 60, action: { kind: "unmapped", paths: [] } },
];
const registerDoc = () => `# t\n\n### Aggregate risk register for this expansion\n\n${renderRiskRegister(REGISTER_ROWS, 3).slice(2).join("\n")}\n\n## hand notes\n\nkeep\n`;

test("registerDrift catches a stale count in the bullet form", () => {
  const doc = registerDoc();
  assert.deepEqual(registerDrift(doc, REGISTER_ROWS, 3), []);
  assert.match(registerDrift(doc.replace("lines: **100**", "lines: **99**"), REGISTER_ROWS, 3).join("\n"), /risk register line stale or missing/);
  // A register with the right row count but a wrong hazard tally must not pass either.
  assert.match(registerDrift(doc.replace("files touching credentials: **1**", "files touching credentials: **9**"), REGISTER_ROWS, 3).join("\n"), /credential/);
});

test("refreshRegister rewrites the bullets and preserves the prose around them", () => {
  const stale = registerDoc().replace("lines: **100**", "lines: **2**").replace("keep", "keep");
  const out = refreshRegister(stale, REGISTER_ROWS, 3);
  assert.match(out, /lines: \*\*100\*\*/);
  assert.match(out, /## hand notes\n\nkeep\n$/);
  // A hand-shaped JSON register (database-notebook) must come back untouched.
  const jsonDoc = "# t\n\n### Aggregate risk register for this expansion\n\n```json\n{\"files\": 75}\n```\n\nnotes\n";
  assert.equal(refreshRegister(jsonDoc, REGISTER_ROWS, 3), jsonDoc);
});

test("registerDrift verifies the JSON register's scalars and catches a wrong one", () => {
  const rows = [{ source: "a.rs", destination: "d", commands: 2, testItems: 3, symbols: ["x()", "y()"], hazards: [], lines: 40, action: { kind: "unmapped", paths: [] } }];
  const good = "```json\n{ \"files\": 1, \"commands\": 2, \"testOnlyItemsExcluded\": 3, \"pubSymbolsTotal\": 2 }\n```";
  assert.deepEqual(registerDrift(good, rows, 2), []);
  assert.match(registerDrift(good.replace('\"pubSymbolsTotal\": 2', '\"pubSymbolsTotal\": 650'), rows, 2).join("\n"), /pubSymbolsTotal differs \(committed 650, generated 2\)/);
});

test("tableDrift always returns an array, including when the register is the only drift", () => {
  // Regression: one edit left `tableDrift` without a `return`, so the CLI crashed on
  // `problems.length` of undefined and every extraction gate case died with a TypeError.
  const rows = [{ source: "a.rs", destination: "core/m/a.rs", commands: 0, testItems: 0, symbols: [], hazards: [], lines: 10, action: { kind: "unmapped", paths: [] } }];
  const doc = `| # | source | destination | commands | moved symbols (top-level public) | Action 给定的落点 | per-file hazards to verify |\n|---|---|---|---|---|---|---|\n${renderRow(1, rows[0])}\n\n### Aggregate risk register for this expansion\n\n- files: **99**, lines: **99**, command wrappers: **99**, test items excluded from the symbol lists: **99**\n`;
  const problems = tableDrift({ committedText: doc, rows, actionDocs: [], totalCommands: 0 });
  assert.ok(Array.isArray(problems), "tableDrift must return an array");
  assert.match(problems.join("\n"), /register|files: \*\*99\*\*/);
});
