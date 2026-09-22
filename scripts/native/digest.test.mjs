import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runShards, shardPlanFor } from "./digest.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const SOURCES = ["docs/native/command-map.summary.json", "docs/native/feature-matrix.json"];

function tempCopy() {
  // A throwaway tree: a negative test that reached the real digests would edit
  // the frozen inventory of record.
  const dir = mkdtempSync(path.join(os.tmpdir(), "aeroric-digest-"));
  for (const source of SOURCES) {
    const target = path.join(dir, source);
    mkdirSync(path.dirname(target), { recursive: true });
    cpSync(path.join(repoRoot, source), target);
  }
  const written = runShards({ repoRoot: dir, write: true });
  assert.equal(written.problems.length, 0, written.problems.join("\n"));
  return { dir, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

const readJson = (dir, file) => JSON.parse(readFileSync(path.join(dir, file), "utf8"));
const writeJson = (dir, file, value) => writeFileSync(path.join(dir, file), JSON.stringify(value, null, 2) + "\n");

test("the committed shards match the canonical digests on disk", () => {
  const { problems } = runShards({ repoRoot });
  assert.deepEqual(problems, []);
});

test("a canonical key that no shard covers is refused", () => {
  const spec = { source: "x.json", prefix: "x", parts: { head: ["a"] } };
  const { problems } = shardPlanFor(spec, { a: 1, b: 2 });
  assert.match(problems.join("\n"), /key "b" is in no shard/);
});

test("a key cut in two shards is refused", () => {
  const spec = { source: "x.json", prefix: "x", parts: { head: ["a"], tail: ["a"] } };
  const { problems } = shardPlanFor(spec, { a: 1 });
  assert.match(problems.join("\n"), /assigned to both shards "head" and "tail"/);
});

test("a shard naming a key the source no longer has is refused", () => {
  const spec = { source: "x.json", prefix: "x", parts: { head: ["a", "gone"] } };
  const { problems } = shardPlanFor(spec, { a: 1 });
  assert.match(problems.join("\n"), /names key "gone"/);
});

test("editing the canonical digest without re-cutting fails the check", () => {
  const { dir, cleanup } = tempCopy();
  const matrix = readJson(dir, "docs/native/feature-matrix.json");
  matrix.features.pop();
  writeJson(dir, "docs/native/feature-matrix.json", matrix);
  const { problems } = runShards({ repoRoot: dir });
  assert.match(problems.join("\n"), /feature-matrix-features\.json: stale/);
  cleanup();
});

test("a hand-edited shard payload fails the check", () => {
  const { dir, cleanup } = tempCopy();
  const file = "docs/native/digest/feature-matrix-gaps.json";
  const shard = readJson(dir, file);
  shard.payload.gaps.pop();
  writeJson(dir, file, shard);
  const { problems } = runShards({ repoRoot: dir });
  assert.match(problems.join("\n"), /feature-matrix-gaps\.json: payload does not match/);
  cleanup();
});

test("a missing shard fails rather than passing quietly", () => {
  const { dir, cleanup } = tempCopy();
  rmSync(path.join(dir, "docs/native/digest/command-map-modules.json"));
  const { problems } = runShards({ repoRoot: dir });
  assert.match(problems.join("\n"), /command-map-modules\.json: missing/);
  cleanup();
});

test("a shard over the injection cap is reported, not truncated", () => {
  const { dir, cleanup } = tempCopy();
  const { problems } = runShards({ repoRoot: dir, capBytes: 2048 });
  assert.match(problems.join("\n"), /exceeds the 2048 B injection cap/);
  cleanup();
});
