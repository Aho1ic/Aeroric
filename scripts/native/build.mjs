#!/usr/bin/env node
// `pnpm native:build` — full no-account build chain for the native preview
// app (P4-01 Actions 2–7):
//   1. regenerate the Xcode project from project.yml (kills staleness);
//   2. cargo build the aeroric-cli host binary (Debug);
//   3. xcodebuild the app bundle (Debug, CODE_SIGNING_ALLOWED=NO — the
//      in-linker signature is not an app signature);
//   4. copy the helper binary into the bundle;
//   5. sign inside-out with sign-local.sh (no account, ad-hoc);
//   6. verify with verify-bundle.sh;
//   7. print the final .app path.
// Every step propagates a non-zero exit; a failing step fails the whole chain.
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const ARTIFACTS = path.join(repoRoot, ".artifacts/native");
const APP = path.join(ARTIFACTS, "DerivedData/Build/Products/Debug/AeroricNative.app");
const CLI_BINARY = path.join(repoRoot, "core/target/debug/aeroric-cli");

function run(label, command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
    ...options,
  });
  if (result.error) {
    console.error(`native:build: ${label}: failed to spawn ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(`native:build: ${label}: ${command} exited with ${result.status}`);
    process.exit(result.status ?? 1);
  }
  return result;
}

console.log("native:build: [1/6] regenerating Xcode project");
run("generate", "node", [path.join(repoRoot, "scripts/native/generate.mjs")]);

console.log("native:build: [2/6] cargo build (aeroric-cli, Debug)");
run("cargo", "cargo", [
  "build",
  "--manifest-path",
  path.join(repoRoot, "core/Cargo.toml"),
  "--locked",
  "-p",
  "aeroric-cli",
]);

console.log("native:build: [3/6] xcodebuild (AeroricNative, Debug, no account signing)");
run("xcodebuild", "xcodebuild", [
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
  "CODE_SIGNING_ALLOWED=NO",
  "build",
]);

if (!existsSync(APP)) {
  console.error(`native:build: expected app bundle missing after xcodebuild: ${APP}`);
  process.exit(1);
}
if (!existsSync(CLI_BINARY)) {
  console.error(`native:build: cargo helper binary missing: ${CLI_BINARY}`);
  process.exit(1);
}

console.log("native:build: [4/6] copying aeroric-cli into bundle helpers");
mkdirSync(path.join(APP, "Contents/MacOS"), { recursive: true });
copyFileSync(CLI_BINARY, path.join(APP, "Contents/MacOS/aeroric-cli"));

console.log("native:build: [5/6] inside-out ad-hoc signing");
run("sign", "bash", [path.join(repoRoot, "scripts/native/sign-local.sh"), APP]);

console.log("native:build: [6/6] bundle verification");
run("verify", "bash", [path.join(repoRoot, "scripts/native/verify-bundle.sh"), APP]);

console.log(`native:build: OK — ${path.relative(repoRoot, APP)}`);
