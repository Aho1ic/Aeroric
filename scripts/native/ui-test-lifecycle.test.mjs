import assert from "node:assert/strict";
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, symlinkSync } from "node:fs";
import os from "node:os";
import { spawnSync } from "node:child_process";
import path from "node:path";
import test from "node:test";
import { assertAttemptComplete, assertQuiescent, createLifecycle, descriptorIO, resources } from "./ui-test-lifecycle.mjs";

const runID = "72e4b8b0-f7fe-4d4d-b623-3d6abeab6ba7";
const clear = () => ({ schemaVersion: 1, errors: [], applications: [], processes: [] });
function fixture(overrides = {}) {
  const files = new Map();
  const directories = new Set(["/", "/fixture", "/fixture/home", "/fixture/artifacts"]);
  const directoryIDs = new Map();
  let nextID = 1;
  const calls = [];
  const io = {
    lstat(p) {
      if (directories.has(p)) {
        if (!directoryIDs.has(p)) directoryIDs.set(p, nextID++);
        return { directory: true, symlink: false, dev: 1, ino: directoryIDs.get(p) };
      }
      return files.has(p) ? { directory: false, symlink: false, dev: 1, ino: 999 } : null;
    },
    mkdir(p, recursive) {
      calls.push(["mkdir", p, recursive]);
      if (directories.has(p)) throw new Error("EEXIST");
      directories.add(p);
    },
    write(p, value) { if (files.has(p)) throw new Error("EEXIST"); files.set(p, value); },
    read: (p) => files.get(p),
    rename: (from, to) => { calls.push(["rename", from, to]); directories.delete(from); directories.add(to); },
    snapshot: async () => clear(),
    warmup: async () => { calls.push(["warmup"]); return { completed: true, causalLaunchReceipt: true, exitClean: true }; },
    ...overrides,
  };
  return { lifecycle: createLifecycle({ home: "/fixture/home", runID, artifacts: "/fixture/artifacts", io }),
    io, calls, files, directories, directoryIDs };
}
test("UUID scratch and warmup are distinct new children; no fixed-root deletion", () => {
  const owned = resources("/fixture/home", runID);
  assert.equal(owned.root, `/fixture/home/Library/Caches/aeroric-native/ui-test-${runID}`);
  assert.equal(owned.scratch, path.join(owned.root, "data"));
  assert.notEqual(owned.warmup, owned.scratch);
  assert.throws(() => resources("/fixture/home", "../escape"));
});
test("lease is exclusive; contention never invokes warmup or cleanup", () => {
  const f = fixture();
  f.directories.add("/fixture/artifacts/ui-test-active");
  assert.throws(() => f.lifecycle.reserve(), /EEXIST/);
  assert.equal(f.calls.filter(([c]) => c === "warmup" || c === "rename").length, 0);
});
for (const name of ["symlink", "file ancestor"]) {
  test(`reject ${name} before directory mutation`, () => {
    const f = fixture({ lstat: () => ({ symlink: name === "symlink", directory: name !== "file ancestor" }) });
    assert.throws(() => f.lifecycle.reserve(), /unsafe/);
    assert.equal(f.calls.length, 0);
  });
}
test("existing run root is never reused", () => {
  const f = fixture();
  f.directories.add(f.lifecycle.root);
  assert.throws(() => f.lifecycle.reserve(), /already exists/);
});
for (const [name, sample] of [
  ["unknown App", { ...clear(), applications: [{ pid: 3, uid: 502 }] }],
  ["CLI holder", { ...clear(), processes: [{ pid: 4, executable: "/wrong/aeroric-cli" }] }],
  ["test runner", { ...clear(), processes: [{ pid: 5, executable: "/fixture/xctest" }] }],
  ["probe error", { ...clear(), errors: [{ code: "unknown" }] }],
  ["missing errors field", { schemaVersion: 1, applications: [], processes: [] }],
]) {
  test(`${name} blocks any next attempt and retains lease`, async () => {
    const f = fixture({ snapshot: async () => sample });
    f.lifecycle.reserve();
    await assert.rejects(() => f.lifecycle.barrier());
    await assert.rejects(() => f.lifecycle.warmup());
    assert.throws(() => f.lifecycle.finish());
    assert.equal(f.calls.filter(([c]) => c === "warmup" || c === "rename").length, 0);
  });
}
for (const name of ["missing callback receipt", "Quit refused", "leaked child", "tool failed"]) {
  test(`${name} poisons lifecycle without signals/deletion/relaunch`, async () => {
    const f = fixture({
      warmup: async () => {
        if (name === "tool failed") throw new Error("fixture tool error");
        return { completed: true, causalLaunchReceipt: name !== "missing callback receipt",
          exitClean: name !== "Quit refused" && name !== "leaked child" };
      },
    });
    f.lifecycle.reserve();
    await assert.rejects(() => f.lifecycle.warmup());
    await assert.rejects(() => f.lifecycle.barrier());
    assert.throws(() => f.lifecycle.finish());
    assert.equal(f.calls.some(([c]) => c === "rename" || c === "kill" || c === "remove"), false);
  });
}
test("successful normal warmup and strict barrier preserve completed owner", async () => {
  const f = fixture();
  f.lifecycle.reserve();
  assert.throws(() => f.lifecycle.finish()); // No observation, no release.
  await f.lifecycle.warmup();
  await f.lifecycle.barrier();
  f.lifecycle.finish();
  assert.equal(f.calls.filter(([c]) => c === "rename").length, 0);
  assert.ok(f.files.has("/fixture/artifacts/ui-test-active/completed.json"));
  assert.ok(f.directories.has("/fixture/artifacts/ui-test-active"));
  assert.throws(() => f.lifecycle.finish()); // Duplicate release is not a new reservation.
});
test("owner replacement cannot release somebody else's lease", async () => {
  const f = fixture();
  f.lifecycle.reserve();
  await f.lifecycle.barrier();
  f.files.set("/fixture/artifacts/ui-test-active/owner.json", "replacement");
  assert.throws(() => f.lifecycle.finish());
});
for (const kind of ["owner", "lease", "data symlink", "warmup symlink"]) {
  test(`${kind} replacement blocks before snapshot or launch`, async () => {
    const f = fixture();
    f.lifecycle.reserve();
    let snapshots = 0;
    f.io.snapshot = async () => { snapshots += 1; return clear(); };
    if (kind === "owner") f.files.set("/fixture/artifacts/ui-test-active/owner.json", "replacement");
    else if (kind === "lease") f.directoryIDs.set("/fixture/artifacts/ui-test-active", -1);
    else {
      const original = f.io.lstat;
      const target = kind.startsWith("data") ? f.lifecycle.scratch : path.join(f.lifecycle.root, "warmup");
      f.io.lstat = (p) => p === target ? { directory: false, symlink: true } : original(p);
    }
    await assert.rejects(() => f.lifecycle.warmup());
    await assert.rejects(() => f.lifecycle.barrier());
    assert.equal(snapshots, 0);
    assert.equal(f.calls.some(([c]) => c === "warmup"), false);
  });
}
test("a concurrent run-root creator is contention, never recursive reuse", () => {
  const f = fixture();
  const original = f.io.mkdir;
  f.io.mkdir = (p, recursive) => {
    if (p === f.lifecycle.root) f.directories.add(p);
    return original(p, recursive);
  };
  assert.throws(() => f.lifecycle.reserve(), /EEXIST/);
  assert.equal(f.calls.some(([c, , recursive]) => c === "mkdir" && recursive), false);
});
test("durable Swift lifecycle failure cannot be washed by an empty later sample", async () => {
  const f = fixture();
  f.lifecycle.reserve();
  await f.lifecycle.barrier();
  f.files.set(f.lifecycle.failure, '{"failed":true}');
  await assert.rejects(() => f.lifecycle.barrier(), /latched/);
  await assert.rejects(() => f.lifecycle.warmup());
  assert.throws(() => f.lifecycle.finish());
});
test("reservation exports original directory and lease identities, never a later baseline", async () => {
  const f = fixture();
  f.lifecycle.reserve();
  const value = f.lifecycle.reservation();
  assert.equal(value.directories.length, 5);
  assert.equal(value.lease, "/fixture/artifacts/ui-test-active");
  f.directoryIDs.set(f.lifecycle.scratch, -1);
  assert.throws(() => f.lifecycle.reservation(), /identity changed/);
});
for (const kind of ["lease", "root"]) {
  test(`remember cannot rebind a ${kind} replaced between its two observations`, () => {
    const f = fixture();
    const target = kind === "lease" ? "/fixture/artifacts/ui-test-active" : f.lifecycle.root;
    const lstat = f.io.lstat;
    const mkdir = f.io.mkdir;
    let created = false;
    let observations = 0;
    f.io.mkdir = (p, recursive) => {
      mkdir(p, recursive);
      const born = lstat(p);
      if (p === target) created = true;
      return { dev: born.dev, ino: born.ino };
    };
    f.io.lstat = (p) => {
      if (p === target && created && ++observations === 2) f.directoryIDs.set(p, -1);
      return lstat(p);
    };
    assert.throws(() => f.lifecycle.reserve(), /identity changed/);
    assert.equal([...f.files.keys()].some((p) => p.startsWith(`${target}/`)), false);
    assert.equal(f.calls.some(([c]) => c === "warmup"), false);
  });
}
test("descriptor-relative mutations reject a replaced/symlink parent before creating anything", () => {
  // This is a fictional temp filesystem only: no host process/App/preferences.
  const root = mkdtempSync(path.join(realpathSync(os.tmpdir()), "aeroric-ui-fs-fixture-"));
  const base = path.join(root, "parent");
  const moved = path.join(root, "retained");
  const target = path.join(root, "target");
  mkdirSync(base); mkdirSync(target);
  const original = lstatSync(base);
  const io = descriptorIO(spawnSync);
  try {
    io.mkdir(path.join(base, "child"), false, [original.dev, original.ino]);
    io.write(path.join(base, "owner.json"), "fixture", [original.dev, original.ino]);
    assert.equal(io.read(path.join(base, "owner.json"), [original.dev, original.ino]), "fixture");
    renameSync(base, moved);
    symlinkSync(target, base);
    assert.throws(() => io.write(path.join(base, "outside.json"), "unsafe", [original.dev, original.ino]));
    assert.throws(() => io.mkdir(path.join(base, "outside-dir"), false, [original.dev, original.ino]));
    assert.throws(() => lstatSync(path.join(target, "outside.json")), /ENOENT/);
    assert.throws(() => lstatSync(path.join(target, "outside-dir")), /ENOENT/);
  } finally { rmSync(root, { recursive: true, force: true }); } // This fixture's exclusive mkdtemp only.
});
test("quiescence requires an explicitly complete empty observation", () => {
  assert.doesNotThrow(() => assertQuiescent(clear()));
  for (const value of [null, {}, { ...clear(), schemaVersion: 0 }]) assert.throws(() => assertQuiescent(value));
});
test("late exit cannot replace a missing/failed Swift completion receipt", () => {
  const receipt = { version: 1, runID, attemptID: "fixture", safeToRetry: true };
  assert.doesNotThrow(() => assertAttemptComplete(receipt, runID, "fixture"));
  for (const value of [null, {}, { ...receipt, safeToRetry: false }, { ...receipt, runID: "wrong" },
    { ...receipt, attemptID: "old" }]) {
    assert.throws(() => assertAttemptComplete(value, runID, "fixture"));
  }
});
test("runner preserves the ordered33 registrations and per-method retry limit", () => {
  const runner = readFileSync(new URL("./ui-test.mjs", import.meta.url), "utf8");
  const entries = [...runner.matchAll(/^\s*"(AeroricNativeUITests\/[^"]+)"/gm)].map((m) => m[1]);
  const expected = [
    ["AppLaunchTests", "testColdLaunchShowsBootstrapIdentifiersThenCloses"],
    ...["testSettingsWindowOpensWithCommandCommaAndClosesWithCommandW", "testShellIdentifiersPresentAtRest",
      "testEveryNavigationRowSwitchesTheDetailSurface", "testShowMainWindowRestoresAfterClose",
      "testSidebarSelectionMovesWithArrowKeys", "testTwoMainWindowSelectionsAreIndependent"].map((m) => ["ShellLifecycleTests", m]),
    ...["testWorkspaceNavigationIsAddressableWithReadableRows", "testSidebarSelectionIsKeyboardReachableAndItsValueIsReadable",
      "testShellPaintsAndKeepsItsMenuSurface"].map((m) => ["AccessibilityTests", m]),
    ["EditorTests", "testAppLaunchesWithEditorPackageMounted"],
    ...["testPresentationVocabularyCoversNativeTerminalAndUnsupported", "testModeSwitchNeverTouchesTaskProtocolMethods"]
      .map((m) => ["ModeSessionIntegrationTests", m]),
    ...["testReviewControlsAreAccessibleAndIsolated", "testReviewAppearanceAndGeometryEvidence"].map((m) => ["NativeVisualFoundationUITests", m]),
    ...["testRoleBindingsAreIndependentAndOffline", "testOrdinaryPhaseAndCandidateGatesDiffer", "testLateFindingKeepsHistoryAndScopesDependencies",
      "testServiceAndToolFailuresRemainDistinct", "testNativeAppearanceKeyboardAndCompactLayout", "testInvalidReviewRequestNeverFallsIntoProduct"]
      .map((m) => ["NativeEvolutionReviewUITests", m]),
    ...["testCandidatesBatchReviewAndDraftStayOffline", "testAutoAdmissionAndFullAccessRemainIndependent",
      "testScopedReferencesRespectSourcesAndCurrentRequirements", "testRevisionsRetirementAndAffectedTasksStayLocal",
      "testNativeAppearanceKeyboardAndCompactLayout", "testInvalidKnowledgeRequestsNeverReachProduct"]
      .map((m) => ["NativeKnowledgeReviewUITests", m]),
    ...["testDraftReviewEditsAndDiscardStayOffline", "testNewSkillConsentAndProjectScopeRemainExplicit",
      "testTaskSelectionRespectsAvailabilityAndSources", "testUpdatesKeepInFlightVersionsWithinSafetyLimits",
      "testNativeAppearanceKeyboardAndCompactLayout", "testInvalidSkillRequestsNeverReachProduct"].map((m) => ["NativeSkillReviewUITests", m]),
  ].map(([suite, method]) => `AeroricNativeUITests/${suite}/${method}`);
  assert.deepEqual(entries, expected);
  assert.match(runner, /attempt < 2 && !passed/);
  assert.match(runner, /"-only-testing:" \+ onlyTesting/);
  assert.doesNotMatch(runner, /process\.kill|rmSync|pgrep/);
});
