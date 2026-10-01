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
import { existsSync, mkdirSync, openSync, writeSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const ARTIFACTS = path.join(repoRoot, ".artifacts/native");
const APP = path.join(ARTIFACTS, "DerivedData/Build/Products/Debug/AeroricNative.app");
const SCRATCH = path.join(ARTIFACTS, "ui-test-data");
const LOG = path.join(ARTIFACTS, "ui-test-last.log");

function fail(message) {
  console.error(`native:ui-test: ${message}`);
  process.exit(1);
}

if (!existsSync(APP)) {
  console.log("native:ui-test: app bundle missing — running build first");
  const build = spawnSync("node", [path.join(repoRoot, "scripts/native/build.mjs")], {
    cwd: repoRoot,
    stdio: "inherit",
  });
  if (build.status !== 0) fail(`build chain exited with ${build.status ?? "?"}`);
}

// Fresh scratch dir on every run — never touches production data
// (com.aeroric.desktop TCC does not apply to this bundle id either).
await (async () => {
  const { rmSync } = await import("node:fs");
  rmSync(SCRATCH, { force: true, recursive: true });
})();
mkdirSync(SCRATCH, { recursive: true });

// Kill any serve orphaned by a hard-killed app instance of a PREVIOUS run
// (an alive orphan holds the scratch's write lock and live socket, so every
// supervisor attempt in this run would die on lock acquisition — measured
// 2026-10-01). Scoped to this scratch's data-dir argument, never broader.
{
  const { execFileSync } = await import("node:child_process");
  try {
    const out = execFileSync("pgrep", ["-f", `aeroric-cli --data-dir ${SCRATCH}`], { encoding: "utf8" });
    const pids = out.split("\n").filter((x) => /^\d+$/.test(x.trim()));
    for (const pid of pids) {
      try { process.kill(Number(pid), "SIGTERM"); } catch { /* already gone */ }
    }
    if (pids.length) console.log(`native:ui-test: terminated ${pids.length} orphaned serve(s) of the scratch`);
  } catch { /* pgrep found nothing */ }
}

const environment = {
  ...process.env,
  TEST_RUNNER_AERORIC_DATA_DIR: SCRATCH,
  TEST_RUNNER_AERORIC_ENV: "ui-test",
};

console.log(`native:ui-test: running AeroricNativeUITests (AppLaunchTests + ShellLifecycleTests; scratch data dir: ${SCRATCH})`);
const test = spawnSync(
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
  ],
  { cwd: repoRoot, env: environment, encoding: "utf8" }
);

// Keep the full output for post-mortems; also stream a tail to the console.
const logHandle = openSync(LOG, "w");
if (test.stdout) writeSync(logHandle, test.stdout);
if (test.stderr) writeSync(logHandle, test.stderr);
writeSync(logHandle, `\nexit status: ${test.status}\n`);
const tail = `${test.stdout ?? ""}\n${test.stderr ?? ""}`.trimEnd().split("\n").slice(-40);
for (const line of tail) console.log(line);

if (test.error) fail(`failed to spawn xcodebuild: ${test.error.message}`);
if (test.status !== 0) {
  fail(`xcodebuild test exited with ${test.status} — full log: ${path.relative(repoRoot, LOG)}`);
}

console.log(`native:ui-test: OK — log: ${path.relative(repoRoot, LOG)}`);
