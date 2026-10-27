#!/usr/bin/env node
// `pnpm native:ui-test` — run AeroricNativeUITests against a freshly built
// Debug bundle (P4-01 Action 7).
//
// Isolation: a brand-new scratch data directory is injected into the runner
// AND forwarded into the app via xcodebuild's TEST_RUNNER_-prefixed
// environment forwarding (documented mechanism — env, not build settings).
// AppLaunchTests then asserts the injected values reached the app, so every
// green run proves the isolation actually works.
import { spawnSync } from "node:child_process";
import { closeSync, existsSync, lstatSync, openSync, readFileSync, writeSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { assertAttemptComplete, createLifecycle, descriptorIO, nativeProcessNames } from "./ui-test-lifecycle.mjs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
import { assertRealUITestsAllowed, UI_TEST_POLICY_EXIT_CODE } from "./ui-test-policy.mjs";

// Refuse before resource reservation, compilation, App checks, warmup or logs.
try {
  assertRealUITestsAllowed();
} catch (error) {
  console.error(`native:ui-test: ${error.message}`);
  process.exit(UI_TEST_POLICY_EXIT_CODE);
}

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const ARTIFACTS = path.join(repoRoot, ".artifacts/native");
const APP = path.join(ARTIFACTS, "DerivedData/Build/Products/Debug/AeroricNative.app");
// The scratch data dir is deliberately NOT under the repo.
//
// The repo lives in the user's ~/Downloads, which macOS protects with TCC. The
// app's first access to a path inside it raises a consent sheet, and because a
// local build is ad-hoc signed, every rebuild changes the cdhash the grant was
// pinned to — so the grant is voided on each build. The app then sits at
// `connecting` with the sheet up and the host never becomes ready.
//
// Measured 2026-10-14: with the scratch under `.artifacts/native/` the host
// never reached ready and 7 of 13 UI tests failed on missing project rows and
// an unmounted workspace; `test-hook.log` recorded `give up: never ready;
// phase=connecting` every time. Pointing the same build at a data dir outside
// ~/Downloads made it ready immediately. `~/Library/Caches` is the
// conventional macOS location for this and is not TCC-protected.
const RUN_ID = randomUUID();
let SCRATCH;
const LOG = path.join(ARTIFACTS, "ui-test-last.log");
// The approved knowledge-review VM controller owns actual-account preference
// writes. XCTest remains sandboxed and receives only a nonsensitive descriptor.
// This is not a test filter and does not change the ordinary runner lifecycle.
const WITNESS_CONTROLLER = process.env.AERORIC_KNOWLEDGE_WITNESS_CONTROLLER;
function knowledgeWitness(action, argument) {
  if (!WITNESS_CONTROLLER || !path.isAbsolute(WITNESS_CONTROLLER)) {
    throw new Error("knowledge tests require the authorized VM witness controller");
  }
  const result = spawnSync("/usr/bin/python3", ["-I", WITNESS_CONTROLLER, action, argument], {
    cwd: repoRoot, env: process.env, encoding: "utf8", timeout: 45000,
  });
  if (result.error || result.status !== 0) {
    throw new Error(`VM witness ${action} failed: ${result.error?.message ?? result.stderr ?? result.status}`);
  }
  return result.stdout.trim();
}

function fail(message) {
  console.error(`native:ui-test: ${message}`);
  process.exit(1);
}

// A fresh, retained resource root and an exclusive lease. Never delete a prior
// scratch, kill a matching process or use a lock PID as cleanup authority.
const PROCESS_TOOL = path.join(ARTIFACTS, `ui-test-process-${RUN_ID}`);
const resourceIO = descriptorIO(spawnSync);
function processTool(action, owned) {
  const result = spawnSync(PROCESS_TOOL, [action, APP, owned.root, owned.runID], {
    cwd: repoRoot, encoding: "utf8", maxBuffer: 4 * 1024 * 1024,
    env: { ...process.env, AERORIC_UI_TEST_RESOURCE_RECEIPT: JSON.stringify(lifecycle.reservation()) },
  });
  if (result.error || result.status !== 0) throw new Error("native UI identity/normal-Quit barrier failed; resources retained");
  return JSON.parse(result.stdout);
}
const lifecycle = createLifecycle({
  home: os.homedir(), runID: RUN_ID, artifacts: ARTIFACTS,
  io: {
    lstat(candidate) {
      try { const info = lstatSync(candidate); return { directory: info.isDirectory(), symlink: info.isSymbolicLink(),
        dev: info.dev, ino: info.ino }; }
      catch (error) { if (error.code === "ENOENT") return null; throw error; }
    },
    ...resourceIO,
    snapshot: async (owned) => processTool("snapshot", owned),
    warmup: async (owned) => processTool("prewarm", owned),
  },
});
try {
  // The exclusive/path gate precedes even compiler/build output writes.
  lifecycle.reserve();
  // Read-only bootstrap inventory before the identity tool itself is compiled.
  // Do not print unrelated process paths or use names as termination authority.
  const baseline = spawnSync("/bin/ps", ["-axo", "pid=,comm="], { encoding: "utf8", maxBuffer: 4 * 1024 * 1024 });
  const lines = baseline.stdout?.trim().split("\n") ?? [];
  if (baseline.error || baseline.status !== 0 || !lines.length || lines.some((line) => {
    const row = line.match(/^\s*([0-9]+)\s+(.+)$/);
    return !row || nativeProcessNames.has(path.basename(row[2].trim()));
  })) throw new Error("native resources occupied or bootstrap observation incomplete; no build authorized");
  const compile = spawnSync("xcrun", ["swiftc", path.join(repoRoot, "scripts/native/ui-test-process.swift"),
    "-o", PROCESS_TOOL], { cwd: repoRoot, encoding: "utf8" });
  if (compile.error || compile.status !== 0) throw new Error("cannot compile native UI identity tool");
  await lifecycle.barrier();
  if (!existsSync(APP)) {
    console.log("native:ui-test: app bundle missing — running build first");
    const build = spawnSync("node", [path.join(repoRoot, "scripts/native/build.mjs")], {
      cwd: repoRoot, stdio: "inherit",
    });
    if (build.status !== 0) throw new Error(`build chain exited with ${build.status ?? "?"}`);
    await lifecycle.barrier();
  }
  await lifecycle.warmup();
} catch (error) { fail(String(error)); }
SCRATCH = lifecycle.scratch;

const environment = {
  ...process.env,
  TEST_RUNNER_AERORIC_DATA_DIR: SCRATCH,
  TEST_RUNNER_AERORIC_ENV: "ui-test",
  TEST_RUNNER_AERORIC_UI_TEST_RUN_ROOT: lifecycle.root,
  TEST_RUNNER_AERORIC_UI_TEST_RUN_ID: RUN_ID,
  TEST_RUNNER_AERORIC_UI_TEST_APP_EXECUTABLE: path.join(APP, "Contents/MacOS/AeroricNative"),
  TEST_RUNNER_AERORIC_UI_TEST_RECEIPT_ROOT: lifecycle.receipts,
  TEST_RUNNER_AERORIC_UI_TEST_RESOURCE_RECEIPT: JSON.stringify(lifecycle.reservation()),
};

// One xcodebuild invocation PER TEST. Measured over 2026-10-01/02 (G41):
// SwiftUI scene assembly under a long-lived testmanagerd session degrades
// with cumulative launches inside one invocation — a 6-test run ended with
// wholesale zero-window launches while a single-test invocation (the canary)
// always painted. Isolating each test in its own invocation resets that
// state; the acceptance command and its pass/fail semantics are unchanged.
const TESTS = [
  "AeroricNativeUITests/AppLaunchTests/testColdLaunchShowsBootstrapIdentifiersThenCloses",
  "AeroricNativeUITests/ShellLifecycleTests/testSettingsWindowOpensWithCommandCommaAndClosesWithCommandW",
  "AeroricNativeUITests/ShellLifecycleTests/testShellIdentifiersPresentAtRest",
  "AeroricNativeUITests/ShellLifecycleTests/testEveryNavigationRowSwitchesTheDetailSurface",
  "AeroricNativeUITests/ShellLifecycleTests/testShowMainWindowRestoresAfterClose",
  "AeroricNativeUITests/ShellLifecycleTests/testSidebarSelectionMovesWithArrowKeys",
  "AeroricNativeUITests/ShellLifecycleTests/testTwoMainWindowSelectionsAreIndependent",
  // AP8 R-AP8-5. This list is hand-maintained (`-only-testing:` per case, one
  // fresh xcodebuild session each — G41), so a new UI test file is **not** run
  // until it is named here. That is how `ModeSessionIntegrationTests` and
  // `EditorTests` came to exist, never compile, and never be noticed.
  "AeroricNativeUITests/AccessibilityTests/testWorkspaceNavigationIsAddressableWithReadableRows",
  "AeroricNativeUITests/AccessibilityTests/testSidebarSelectionIsKeyboardReachableAndItsValueIsReadable",
  "AeroricNativeUITests/AccessibilityTests/testShellPaintsAndKeepsItsMenuSurface",
  // These two were in the tree but never listed — the guard below found them.
  "AeroricNativeUITests/EditorTests/testAppLaunchesWithEditorPackageMounted",
  "AeroricNativeUITests/ModeSessionIntegrationTests/testPresentationVocabularyCoversNativeTerminalAndUnsupported",
  "AeroricNativeUITests/ModeSessionIntegrationTests/testModeSwitchNeverTouchesTaskProtocolMethods",
  // Stage 01: DEBUG-only native review fixture; same isolation/warmup runner.
  "AeroricNativeUITests/NativeVisualFoundationUITests/testReviewControlsAreAccessibleAndIsolated",
  "AeroricNativeUITests/NativeVisualFoundationUITests/testReviewAppearanceAndGeometryEvidence",
  // Q26 task-evolution review sample (e3836e62:docs/native/task-evolution-review-stage-01.md):
  // DEBUG-only, explicitly enabled, offline in-memory sample. Registered per
  // method because the runner's file guard cannot see a method that is missing
  // from this list.
  "AeroricNativeUITests/NativeEvolutionReviewUITests/testRoleBindingsAreIndependentAndOffline",
  "AeroricNativeUITests/NativeEvolutionReviewUITests/testOrdinaryPhaseAndCandidateGatesDiffer",
  "AeroricNativeUITests/NativeEvolutionReviewUITests/testLateFindingKeepsHistoryAndScopesDependencies",
  "AeroricNativeUITests/NativeEvolutionReviewUITests/testServiceAndToolFailuresRemainDistinct",
  "AeroricNativeUITests/NativeEvolutionReviewUITests/testNativeAppearanceKeyboardAndCompactLayout",
  "AeroricNativeUITests/NativeEvolutionReviewUITests/testInvalidReviewRequestNeverFallsIntoProduct",
  // Q45: offline project-knowledge review; keep each real-window method explicit.
  "AeroricNativeUITests/NativeKnowledgeReviewUITests/testCandidatesBatchReviewAndDraftStayOffline",
  "AeroricNativeUITests/NativeKnowledgeReviewUITests/testAutoAdmissionAndFullAccessRemainIndependent",
  "AeroricNativeUITests/NativeKnowledgeReviewUITests/testScopedReferencesRespectSourcesAndCurrentRequirements",
  "AeroricNativeUITests/NativeKnowledgeReviewUITests/testRevisionsRetirementAndAffectedTasksStayLocal",
  "AeroricNativeUITests/NativeKnowledgeReviewUITests/testNativeAppearanceKeyboardAndCompactLayout",
  "AeroricNativeUITests/NativeKnowledgeReviewUITests/testInvalidKnowledgeRequestsNeverReachProduct",
  "AeroricNativeUITests/NativeSkillReviewUITests/testDraftReviewEditsAndDiscardStayOffline",
  "AeroricNativeUITests/NativeSkillReviewUITests/testNewSkillConsentAndProjectScopeRemainExplicit",
  "AeroricNativeUITests/NativeSkillReviewUITests/testTaskSelectionRespectsAvailabilityAndSources",
  "AeroricNativeUITests/NativeSkillReviewUITests/testUpdatesKeepInFlightVersionsWithinSafetyLimits",
  "AeroricNativeUITests/NativeSkillReviewUITests/testNativeAppearanceKeyboardAndCompactLayout",
  "AeroricNativeUITests/NativeSkillReviewUITests/testInvalidSkillRequestsNeverReachProduct",
];

// `-only-testing:` names one case, so `TESTS` above is the whole run set. A UI
// test file that is not named there **never runs** — and because the file still
// gets compiled, the failure mode is "green run, untested code" rather than an
// error. Measured 2026-10-13: `ModeSessionIntegrationTests` had been in the tree
// since P5-04 (`87220d5c`, 2026-10-02) referencing `ModeSwitchStep` with no
// `import AeroricUI` and no package link — `pnpm native:ui-test` could not even
// compile, and nobody saw it because the suite was never registered as a gate
// case. Refuse to run with an unlisted file instead of silently ignoring it.
{
  const { readdirSync } = await import("node:fs");
  const dir = path.join(repoRoot, "macos/AeroricUITests");
  const listed = new Set(TESTS.map((entry) => entry.split("/")[1]));
  const onDisk = readdirSync(dir)
    .filter((name) => name.endsWith("Tests.swift"))
    .map((name) => name.replace(/\.swift$/, ""));
  const missing = onDisk.filter((name) => !listed.has(name));
  if (missing.length > 0) {
    console.error(
      `native:ui-test: ${missing.join(", ")} declare(s) tests but no case is listed in TESTS — ` +
        "a file that is never named there never runs (add its cases, or delete the file)",
    );
    process.exit(1);
  }
}

const logHandle = openSync(LOG, "w");
const failed = [];
const witnessFailed = new Set();
for (const [index, onlyTesting] of TESTS.entries()) {
  console.log(`native:ui-test: [${index + 1}/${TESTS.length}] ${onlyTesting}`);
  let passed = false;
  // One retry per test in a FRESH xcodebuild session: the last tests of the
  // suite hit the degraded end of the G41 scene-assembly state, and a new
  // invocation resets it (same reason the runner is per-test at all).
  for (let attempt = 0; attempt < 2 && !passed; attempt += 1) {
    if (attempt > 0) {
      console.log(`native:ui-test: retrying ${onlyTesting} in a fresh session`);
    }
    // Even an ordinary assertion retry must pass strict process custody first.
    // A leaked App/CLI or observation error stops the complete chain, not a
    // forced cleanup followed by a misleading later success.
    try { await lifecycle.barrier(); }
    catch (error) { closeSync(logHandle); fail(String(error)); }
    let descriptor;
    let expectedWitness;
    let witnessFailure;
    const isKnowledge = onlyTesting.startsWith("AeroricNativeUITests/NativeKnowledgeReviewUITests/");
    const requiresOrdinaryCustody = /\/(?:AppLaunchTests|ShellLifecycleTests|AccessibilityTests|EditorTests)\//.test(onlyTesting)
      || onlyTesting === "AeroricNativeUITests/ModeSessionIntegrationTests/testPresentationVocabularyCoversNativeTerminalAndUnsupported";
    const attemptID = randomUUID();
    const attemptEnvironment = { ...environment };
    attemptEnvironment.TEST_RUNNER_AERORIC_UI_TEST_ATTEMPT_ID = attemptID;
    try {
      if (isKnowledge) {
        descriptor = knowledgeWitness("prepare", onlyTesting);
        expectedWitness = JSON.parse(descriptor);
        if (expectedWitness.version !== 1 || expectedWitness.method !== onlyTesting) {
          throw new Error("VM controller returned a mismatched witness descriptor");
        }
        attemptEnvironment.TEST_RUNNER_AERORIC_KNOWLEDGE_PREFERENCE_WITNESS = descriptor;
        delete attemptEnvironment.AERORIC_KNOWLEDGE_PREFERENCE_WITNESS;
      }
    } catch (error) {
      witnessFailure = error;
    }
    let test;
    try {
      test = witnessFailure ? { status: 1, stderr: String(witnessFailure) } : spawnSync(
      "xcodebuild",
      [
        "test",
        "-project",
        path.join(repoRoot, "macos/AeroricNative.xcodeproj"),
        "-scheme",
        "AeroricNative",
        "-configuration",
        "Debug",
        "-destination",
        "platform=macOS",
        "-derivedDataPath",
        path.join(ARTIFACTS, "DerivedData"),
        "-only-testing:" + onlyTesting,
      ],
      // Preserve verbose real-window evidence beyond Node's 1MiB default.
      { cwd: repoRoot, env: attemptEnvironment, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }
      );
    } finally {
      // Collect even after a failed launch/test or synchronous spawn exception.
      // A missing or changed marker is never restored to make a retry green.
      if (descriptor) {
        try {
          const receipt = JSON.parse(knowledgeWitness("collect", descriptor));
          if (receipt.attemptID !== expectedWitness?.attemptID
              || receipt.preserved !== true || receipt.collected !== true) {
            throw new Error("VM controller returned an unverified collection receipt");
          }
          writeSync(logHandle, `\nVM witness collection: ${JSON.stringify(receipt)}\n`);
        } catch (error) {
          witnessFailure = error;
          writeSync(logHandle, `\nVM witness failure: ${error}\n`);
        }
      }
      // Also sweep receipts when preparation returned no descriptor (e.g. an
      // interrupted writer). Only the marked VM controller may recover them.
      if (isKnowledge) {
        try {
          const receipt = JSON.parse(knowledgeWitness("reconcile", onlyTesting));
          if (receipt.unresolved !== 0) throw new Error("VM witness receipts remain unresolved");
          writeSync(logHandle, `\nVM witness reconciliation: ${JSON.stringify(receipt)}\n`);
        } catch (error) {
          witnessFailure = error;
          writeSync(logHandle, `\nVM witness reconciliation failure: ${error}\n`);
        }
      }
      if (witnessFailure) witnessFailed.add(onlyTesting);
    }
    const status = witnessFailure ? 1 : test.status;
    if (test.stdout) writeSync(logHandle, test.stdout);
    if (test.stderr) writeSync(logHandle, test.stderr);
    if (test.status === 86) {
      closeSync(logHandle);
      fail("private evidence facility failed; retain first failure, no retry or next test");
    }
    // Swift records safety failures durably and emits this nonsensitive token.
    // Even a late natural exit / XCTest cleanup may not erase that failure.
    if (`${test.stdout ?? ""}\n${test.stderr ?? ""}`.includes("AERORIC_UI_LIFECYCLE_FAILURE_V1")) {
      closeSync(logHandle);
      fail("Swift lifecycle failure is latched; no retry or next method");
    }
    writeSync(logHandle, `\n=== ${onlyTesting} (attempt ${attempt + 1}) exit status: ${status} ===\n`);
    const tail = `${test.stdout ?? ""}\n${test.stderr ?? ""}`.trimEnd().split("\n").slice(-8);
    for (const line of tail) console.log(line);
    if (test.error) fail(`failed to spawn xcodebuild: ${test.error.message}`);
    if (witnessFailure) console.error(String(witnessFailure));
    try { await lifecycle.barrier(); }
    catch (error) {
      writeSync(logHandle, `\nLifecycle barrier failed: ${error}\n`);
      closeSync(logHandle);
      fail(String(error));
    }
    if (requiresOrdinaryCustody) {
      try {
        const receiptPath = path.join(lifecycle.receipts, `lifecycle-complete-${attemptID}.json`);
        const parent = lifecycle.reservation().directories.find((item) => item.path === lifecycle.receipts);
        assertAttemptComplete(JSON.parse(resourceIO.read(receiptPath, [parent.dev, parent.ino])), RUN_ID, attemptID);
      } catch {
        closeSync(logHandle);
        fail("ordinary Swift lifecycle completion missing/invalid; retain lease, no retry");
      }
    }
    passed = status === 0;
  }
  if (!passed || witnessFailed.has(onlyTesting)) failed.push(onlyTesting);
}
closeSync(logHandle);
try { await lifecycle.barrier(); lifecycle.finish(); }
catch (error) { fail(String(error)); }

if (failed.length) {
  console.error(`\nnative:ui-test: ${failed.length}/${TESTS.length} test(s) failed:`);
  for (const name of failed) console.error(`  - ${name}`);
  fail(`full log: ${path.relative(repoRoot, LOG)}`);
}

console.log(`\nnative:ui-test: OK — all ${TESTS.length} test(s) passed — log: ${path.relative(repoRoot, LOG)}`);
