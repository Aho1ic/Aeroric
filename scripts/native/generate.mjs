#!/usr/bin/env node
// `pnpm native:generate` — regenerate macos/AeroricNative.xcodeproj from
// macos/project.yml with the pinned XcodeGen version (P4-01 Action 5).
//
// macos/AeroricNative.xcodeproj is a derived artifact and .gitignore-covered;
// the committed sources are project.yml + Config/*.xcconfig. Hand-editing a
// .pbxproj is forbidden (docs/cookbook/build-and-verify.md); this script is the only
// sanctioned path to (re)generate it.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const SPEC = path.join(repoRoot, "macos/project.yml");
const PROJECT = path.join(repoRoot, "macos/AeroricNative.xcodeproj");
// XcodeGen 2.46.0 的 `-p` 是「父目录」语义（本地实测：`-p sub` → sub/<SpecName>.xcodeproj）；
// 传 `<dir>/X.xcodeproj` 会被当成目录再拼接 Spec 名，copy 直接 ENOENT 失败。
// 所以这里传已存在的 macos/ 目录，产物 = macos/<SpecName>.xcodeproj。
const PROJECT_PARENT = path.join(repoRoot, "macos");
// Same pin as doctor.mjs and docs/cookbook/build-and-verify.md; xcodegen 2.46.0 was
// installed and verified on this machine on 2026-10-01.
const PINNED = "2.46.0";

function fail(message) {
  console.error(`native:generate: ${message}`);
  process.exit(1);
}

if (!existsSync(SPEC)) {
  fail(`missing ${path.relative(repoRoot, SPEC)} — XcodeGen spec is the committed source`);
}

const version = spawnSync("xcodegen", ["--version"], { encoding: "utf8" });
const versionOutput = `${version.stdout ?? ""}${version.stderr ?? ""}`;
if (version.error || version.status !== 0 || !/2\.46\.0/.test(versionOutput)) {
  fail(
    `pinned XcodeGen ${PINNED} not usable (error=${version.error?.message ?? "none"}, ` +
      `status=${version.status}, output=${versionOutput.trim().slice(0, 200)}) — ` +
      "see docs/cookbook/build-and-verify.md for the install command"
  );
}

const generate = spawnSync(
  "xcodegen",
  ["generate", "-s", SPEC, "-p", PROJECT_PARENT],
  { cwd: repoRoot, encoding: "utf8" }
);
if (generate.stdout) process.stdout.write(generate.stdout);
if (generate.stderr) process.stderr.write(generate.stderr);
if (generate.error) fail(`failed to spawn xcodegen: ${generate.error.message}`);
if (generate.status !== 0) fail(`xcodegen exited with ${generate.status}`);
if (!existsSync(PROJECT)) fail(`xcodegen reported success but ${path.relative(repoRoot, PROJECT)} is missing`);

console.log(`native:generate: wrote ${path.relative(repoRoot, PROJECT)} (XcodeGen ${PINNED})`);
