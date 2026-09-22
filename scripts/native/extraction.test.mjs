import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  expandModules, hazardsFor, renderRow, splitPublicSymbols, tableDrift,
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
  const row = { source: "a.rs", destination: "b.rs", commands: 1, testItems: 4, hazards: [], symbols: Array.from({ length: 17 }, (_, i) => `f${i}()`) };
  const line = renderRow(3, row);
  assert.match(line, /^\| 3 \| `a\.rs` \| `b\.rs` \| 1 \| /);
  assert.match(line, /\+2 more \(4 test items excluded\)/);
});

test("tableDrift catches a stale row, a moved destination and a changed wrapper count", () => {
  const rows = [{ source: "x.rs", destination: "core/x.rs", commands: 2, testItems: 1, symbols: ["a()"], hazards: [] }];
  const good = "| # | source | destination | commands | moved symbols (top-level public) | per-file hazards to verify |\n|---|---|---|---|---|---|\n| 1 | `x.rs` | `core/x.rs` | 2 | a() (1 test items excluded) | — |";
  assert.deepEqual(tableDrift({ committedText: good, rows }), []);
  assert.match(tableDrift({ committedText: good.replace("| 2 | a()", "| 3 | a()"), rows }).join("\n"), /command wrappers differ/);
  assert.match(tableDrift({ committedText: good.replace("core/x.rs", "core/moved.rs"), rows }).join("\n"), /destination differs/);
  assert.match(tableDrift({ committedText: good.replace("| 1 | `x.rs`", "| 1 | `gone.rs`"), rows }).join("\n"), /not in the current source tree/);
  assert.match(tableDrift({ committedText: good.split("\n").slice(0, 2).join("\n"), rows }).join("\n"), /row count differs|no §5\.1 table rows/);
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
