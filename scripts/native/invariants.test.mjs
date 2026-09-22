import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { baselineProblems } from "./check-signing-baseline.mjs";
import { parsePatchTables, patchProblems } from "./check-patches.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const read = (rel) => readFileSync(path.join(repoRoot, rel), "utf8");

const CONFIG = JSON.stringify({ bundle: { macOS: { signingIdentity: "-", hardenedRuntime: false } } });
const WORKFLOW = "jq -n '{ bundle: { macOS: { hardenedRuntime: true } } }' > tauri.macos-signed.json\ntauri build --config tauri.macos-signed.json\n";
const clean = () => baselineProblems(CONFIG, null, WORKFLOW);

// ---- INV-09: the ad-hoc signing baseline ----

test("the committed signing baseline and release workflow satisfy INV-09", () => {
  assert.deepEqual(
    baselineProblems(read("src-tauri/tauri.conf.json"), null, read(".github/workflows/release-desktop.yml")),
    [],
    "if this fails, the baseline itself moved — that is an INV-09 change, not a test bug",
  );
});

test("flipping hardenedRuntime in the baseline is refused", () => {
  const flipped = JSON.stringify({ bundle: { macOS: { signingIdentity: "-", hardenedRuntime: true } } });
  assert.match(baselineProblems(flipped, null, WORKFLOW).join("\n"), /requires false/);
});

test("deleting hardenedRuntime is refused, because Tauri defaults it to true", () => {
  // A value diff shows nothing here: absence is the silent change.
  const absent = JSON.stringify({ bundle: { macOS: { signingIdentity: "-" } } });
  assert.match(baselineProblems(absent, null, WORKFLOW).join("\n"), /absent — Tauri defaults it to true/);
});

test("replacing the ad-hoc identity with a Developer ID is refused", () => {
  const real = JSON.stringify({ bundle: { macOS: { signingIdentity: "Developer ID Application: X (T)", hardenedRuntime: false } } });
  assert.match(baselineProblems(real, null, WORKFLOW).join("\n"), /requires the ad-hoc/);
});

test("a config without bundle.macOS fails instead of passing quietly", () => {
  const problems = baselineProblems(JSON.stringify({ bundle: {} }), null, WORKFLOW);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /baseline shape moved/);
});

test("an unparseable tauri config is a failure, not an absence of evidence", () => {
  assert.match(baselineProblems("{ not json", null, WORKFLOW).join("\n"), /unparseable/);
});

test("the signed CI path must keep re-enabling hardened runtime", () => {
  assert.match(baselineProblems(CONFIG, null, "tauri build\n").join("\n"), /no longer writes hardenedRuntime=true/);
  const noOverlayFlag = "jq -n '{ bundle: { macOS: { hardenedRuntime: true } } }' > tauri.macos-signed.json\ntauri build\n";
  assert.match(baselineProblems(CONFIG, null, noOverlayFlag).join("\n"), /no longer passes --config/);
  // A committed overlay, if one ever appears, has to actually set the flag.
  assert.match(baselineProblems(CONFIG, JSON.stringify({ bundle: { macOS: { hardenedRuntime: false } } }), "").join("\n"), /does not set hardenedRuntime=true/);
});

// ---- patch table parity ----

const PATCH_HEAD = "[package]\nname = \"x\"\n\n[patch.crates-io]\n";

test("two identical non-empty patch tables agree", () => {
  // git-kind entries only: a path entry is additionally checked for a real
  // Cargo.toml on disk, which a synthetic fixture cannot supply.
  const text = PATCH_HEAD + "rustyline = { git = \"https://g/r.git\", rev = \"aaa\" }\nreedline = { git = \"https://g/l.git\", rev = \"bbb\" }\n";
  assert.deepEqual(patchProblems(parsePatchTables(text, "src-tauri", repoRoot), parsePatchTables(text, "core", repoRoot), 2), []);
});

test("a path patch whose target has no Cargo.toml is refused", () => {
  const text = PATCH_HEAD + "wry = { path = \"vendor/definitely-not-here\" }\n";
  const tables = parsePatchTables(text, "src-tauri", repoRoot);
  assert.match(patchProblems(tables, parsePatchTables(text, "src-tauri", repoRoot), 1).join("\n"), /has no readable Cargo.toml/);
});

test("two empty patch tables are a mismatch, not a pass", () => {
  // Measured: the old checker printed "0 patch entries identical" and exited 0.
  const problems = patchProblems(parsePatchTables("[package]\n", "src-tauri", repoRoot), parsePatchTables("[workspace]\n", "core", repoRoot), 9);
  assert.match(problems.join("\n"), /src-tauri: no \[patch\.\*\] entries parsed/);
  assert.match(problems.join("\n"), /core: no \[patch\.\*\] entries parsed/);
});

test("a patch dropped from one side is named", () => {
  const desktop = parsePatchTables(PATCH_HEAD + "wry = { path = \"vendor/wry\" }\nroot = { path = \"vendor/root\" }\n", "src-tauri", repoRoot);
  const core = parsePatchTables(PATCH_HEAD + "wry = { path = \"vendor/wry\" }\n", "core", repoRoot);
  const problems = patchProblems(desktop, core, 1);
  assert.match(problems.join("\n"), /patch count differs: src-tauri 2 vs core 1/);
  assert.match(problems.join("\n"), /crates-io\|root: patched in src-tauri but missing from core/);
});

test("a rev or source-kind drift on the same entry is caught", () => {
  const desktop = parsePatchTables(PATCH_HEAD + "rustyline = { git = \"https://g/r.git\", rev = \"aaa\" }\n", "src-tauri", repoRoot);
  const core = parsePatchTables(PATCH_HEAD + "rustyline = { git = \"https://g/r.git\", rev = \"bbb\" }\n", "core", repoRoot);
  assert.match(patchProblems(desktop, core, 1).join("\n"), /rev differs/);

  const forked = parsePatchTables(PATCH_HEAD + "rustyline = { path = \"vendor/rustyline\" }\n", "core", repoRoot);
  assert.match(patchProblems(desktop, forked, 1).join("\n"), /source kind or rev differs/);
});

test("quoted registry tables are parsed rather than skipped", () => {
  const text = "[patch.\"https://github.com/o/r.git\"]\nfoo = { git = \"https://github.com/o/r.git\", rev = \"abc\" }\n";
  const table = parsePatchTables(text, "core", repoRoot);
  assert.equal(table.size, 1, "a quoted registry must not yield an empty comparison");
  assert.equal([...table.values()][0].rev, "abc");
});
