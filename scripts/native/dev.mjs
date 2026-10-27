#!/usr/bin/env node
// `pnpm native:dev` — launch a freshly built Debug bundle for interactive use
// (P4-01 Actions 5–7).
//
// Launches the Mach-O directly (NOT via `open`) so an isolated scratch data
// dir can be injected through the environment. Verifies the bundle signature
// first, confirms the process is still alive after 1s (a launch crash fails
// this command), prints PID + data dir, then exits 0 leaving the app running.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const ARTIFACTS = path.join(repoRoot, ".artifacts/native");
const APP = path.join(ARTIFACTS, "DerivedData/Build/Products/Debug/AeroricNative.app");
const EXECUTABLE = path.join(APP, "Contents/MacOS/AeroricNative");
// Outside the repo on purpose — same reason as `ui-test.mjs`: the repo sits in
// ~/Downloads, which is TCC-protected, and an ad-hoc rebuild voids the grant
// every time. The app then blocks on the consent sheet and the host never
// reaches ready. `~/Library/Caches` needs no consent.
const DEV_DATA = path.join(os.homedir(), "Library/Caches/aeroric-native/dev-data");

function fail(message) {
  console.error(`native:dev: ${message}`);
  process.exit(1);
}

if (!existsSync(EXECUTABLE)) {
  console.log("native:dev: app bundle missing — running build first");
  const build = spawnSync("node", [path.join(repoRoot, "scripts/native/build.mjs")], {
    cwd: repoRoot,
    stdio: "inherit",
  });
  if (build.status !== 0) fail(`build chain exited with ${build.status ?? "?"}`);
}

const verify = spawnSync("bash", [path.join(repoRoot, "scripts/native/verify-bundle.sh"), APP], {
  cwd: repoRoot,
  encoding: "utf8",
});
if (verify.status !== 0) {
  process.stdout.write(verify.stdout ?? "");
  process.stderr.write(verify.stderr ?? "");
  fail(`bundle verification failed with ${verify.status}`);
}

rmSync(DEV_DATA, { force: true, recursive: true });
mkdirSync(DEV_DATA, { recursive: true });

console.log(`native:dev: launching ${path.relative(repoRoot, EXECUTABLE)}`);
const child = spawn(EXECUTABLE, [], {
  cwd: repoRoot,
  // Isolation: the dev launch gets its own data dir; nothing under
  // com.aeroric.desktop (the Tauri app's home) is touched.
  env: { ...process.env, AERORIC_DATA_DIR: DEV_DATA, AERORIC_ENV: "dev" },
  detached: true,
  stdio: "ignore",
});
child.unref();

setTimeout(() => {
  try {
    process.kill(child.pid, 0); // existence probe only
  } catch {
    fail(`app exited within 1s of launch (pid ${child.pid}) — run native:ui-test for the automated check`);
  }
  console.log(`native:dev: OK — pid ${child.pid}, data dir: ${DEV_DATA}`);
  process.exit(0);
}, 1000);
