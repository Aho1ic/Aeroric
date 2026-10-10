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
  const file = "docs/native/digest/feature-matrix-gaps-1.json";
  const shard = readJson(dir, file);
  shard.payload.gaps.pop();
  writeJson(dir, file, shard);
  const { problems } = runShards({ repoRoot: dir });
  assert.match(problems.join("\n"), /feature-matrix-gaps-1\.json: payload does not match/);
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

test("a key added to the canonical digest lands in the catch-all head shard", () => {
  const spec = { source: "x.json", prefix: "x", parts: { head: ["*"], big: ["blob"] } };
  const canonical = { blob: 1, a: 2, later_added: 3 };
  const { problems, shards } = shardPlanFor(spec, canonical);
  assert.deepEqual(problems, []);
  const head = shards.find((s2) => s2.name === "x-head.json");
  assert.deepEqual(Object.keys(head.payload).sort(), ["a", "later_added"], "new key must be carried, not dropped");
});

test("the catch-all never double-claims a key an explicit shard owns", () => {
  const spec = { source: "x.json", prefix: "x", parts: { head: ["*"], big: ["blob"] } };
  const { problems, shards } = shardPlanFor(spec, { blob: 1, other: 2 });
  assert.deepEqual(problems, []);
  assert.deepEqual(Object.keys(shards.find((s2) => s2.name === "x-head.json").payload), ["other"]);
});
test("a chunked part is cut into ordered shards that reconstruct the canonical array", () => {
  const spec = { source: "x.json", prefix: "x", parts: { gaps: ["gaps"] }, chunks: { gaps: 3 } };
  const rows = Array.from({ length: 9 }, (_, i) => ({ id: i, body: "x".repeat(50) }));
  const { problems, shards } = shardPlanFor(spec, { gaps: rows });
  assert.deepEqual(problems, []);
  assert.deepEqual(shards.map((s2) => s2.name), ["x-gaps-1.json", "x-gaps-2.json", "x-gaps-3.json"]);
  const back = shards.flatMap((s2) => s2.payload.gaps);
  assert.deepEqual(back, rows, "chunks must concatenate back to the canonical array, in order");
  assert.ok(shards.every((s2) => s2.payload.gaps.length > 0), "no shard may be empty");
});

test("more chunks than rows is refused rather than silently producing empty shards", () => {
  const spec = { source: "x.json", prefix: "x", parts: { gaps: ["gaps"] }, chunks: { gaps: 3 } };
  const { problems, shards } = shardPlanFor(spec, { gaps: [1, 2] });
  assert.match(problems.join("\n"), /cannot split 2 row\(s\) of "gaps" into 3 shards/);
  assert.deepEqual(shards.map((s2) => s2.name), ["x-gaps.json"], "the unfulfillable split must not half-apply");
});

test("chunking a non-array key is refused", () => {
  const spec = { source: "x.json", prefix: "x", parts: { gaps: ["gaps"] }, chunks: { gaps: 2 } };
  const { problems } = shardPlanFor(spec, { gaps: { not: "an array" } });
  assert.match(problems.join("\n"), /canonical key "gaps" is not an array/);
});

test("chunking a part that claims two keys is refused", () => {
  const spec = { source: "x.json", prefix: "x", parts: { mixed: ["a", "b"] }, chunks: { mixed: 2 } };
  const { problems } = shardPlanFor(spec, { a: [1], b: [2] });
  assert.match(problems.join("\n"), /declared chunked but claims 2 keys/);
});

test("a chunk that loses a row fails the reconstruction assertion, not just the payload compare", () => {
  const { dir, cleanup } = tempCopy();
  const file = "docs/native/digest/feature-matrix-gaps-2.json";
  const shard = readJson(dir, file);
  shard.payload.gaps.pop();
  writeJson(dir, file, shard);
  const { problems } = runShards({ repoRoot: dir });
  const all = problems.join("\n");
  assert.match(all, /feature-matrix-gaps-2\.json: payload does not match/);
  assert.match(all, /the shard files on disk do not reconstruct the canonical digest/);
  cleanup();
});

test("the gaps array ships in ten shards, all with injection headroom", () => {
  const { problems, rows, index } = runShards({ repoRoot });
  assert.deepEqual(problems, []);
  const names = rows.map((r) => r.file);
  for (let shard = 1; shard <= 10; shard += 1) {
    assert.ok(names.includes(`docs/native/digest/feature-matrix-gaps-${shard}.json`), names.join(" "));
  }
  assert.ok(!names.includes("docs/native/digest/feature-matrix-gaps.json"), "the unsplit shard must be gone, not left behind as a stale copy");
  assert.ok(!names.includes("docs/native/digest/feature-matrix-gaps-11.json"), "an 11th shard means the constant moved without this expectation");
  for (const row of rows.filter((r) => /gaps/.test(r.file))) {
    assert.ok(row.bytes < 32768 * 0.8, `${row.file} is at ${row.bytes} B — headroom is the point of chunking`);
  }
  const fm = index.sources.find((s2) => s2.path === "docs/native/feature-matrix.json");
  assert.equal(fm.shards.filter((s2) => /gaps/.test(s2.file)).length, 10);
});
test("a shard the plan no longer emits is reported instead of lingering", () => {
  const { dir, cleanup } = tempCopy();
  writeJson(dir, "docs/native/digest/feature-matrix-gaps-11.json", { shard: "x", payload: { gaps: [] } });
  const { problems } = runShards({ repoRoot: dir });
  assert.match(problems.join("\n"), /feature-matrix-gaps-11\.json: stale shard/);
  cleanup();
});
