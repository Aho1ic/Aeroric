#!/usr/bin/env node
// `pnpm native:doctor` — read-only environment/license/tool/DBX-pin inspection.
// Never installs anything, never accepts a license, never uses sudo (P1-01 Action 2).
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

function run(argv, cwd = repoRoot) {
  const res = spawnSync(argv[0], argv.slice(1), { cwd, encoding: "utf8", shell: false, timeout: 120000 });
  return { code: res.status ?? -1, out: (res.stdout ?? "") + (res.stderr ?? "") };
}

const checks = [];
function check(name, argv, { expect = 0, cwd, match } = {}) {
  const { code, out } = run(argv, cwd ?? repoRoot);
  const detail = out.split("\n").find((l) => l.trim()) ?? "";
  const ok = expect === 0 ? code === 0 : code !== 0;
  const matched = match ? match.test(out) : true;
  checks.push({ name, argv: argv.join(" "), exit: code, ok: ok && matched, detail: detail.slice(0, 120) });
}

check("macOS version", ["sw_vers", "-productVersion"]);
check("architecture", ["uname", "-m"]);
check("Xcode select path", ["xcode-select", "-p"]);
check("Xcode license accepted (blocks cc, python3, swift, ld)", ["xcrun", "swift", "--version"]);
check("SDK path resolvable", ["xcrun", "--sdk", "macosx", "--show-sdk-path"]);
check("Rust toolchain", ["rustc", "--version"]);
check("Cargo", ["cargo", "--version"]);
check("Node", ["node", "--version"]);
check("pnpm", ["pnpm", "--version"]);
check("python3 for Trellis scripts", ["python3", "--version"]);
check("Trellis CLI", ["trellis", "--version"]);
check(
  "XcodeGen (needed by native:generate, version pinned in docs/native/tooling.md §5)",
  ["xcodegen", "--version"],
  // Three documents promise P4-01 must use this exact generator version; without the match this check
  // accepted any exit-0, so a brew upgrade would silently change what native:generate emits.
  { match: /2\.46\.0/ },
);

const dbxPinPath = path.join(repoRoot, "scripts", "dbx-ref.txt");
if (existsSync(dbxPinPath)) {
  const pin = readFileSync(dbxPinPath, "utf8").trim();
  const head = run(["git", "-C", "../dbx", "rev-parse", "HEAD"]);
  const dirty = run(["git", "-C", "../dbx", "status", "--short"]);
  const actual = head.out.trim();
  checks.push({
    name: "DBX checkout matches pin",
    argv: `git -C ../dbx rev-parse HEAD == ${pin}`,
    exit: 0,
    ok: actual === pin,
    detail: `HEAD=${actual.slice(0, 12)} dirtyFiles=${dirty.out.split("\n").filter(Boolean).length} (dirty files are the user's; doctor never resets them)`,
  });
} else {
  checks.push({ name: "DBX pin file", argv: "scripts/dbx-ref.txt", exit: -1, ok: false, detail: "missing" });
}

for (const probe of [
  ["core/Cargo.toml", "core workspace manifest"],
  ["core/Cargo.lock", "core lock file (required before --locked is meaningful)"],
  ["macos/AeroricKit/Package.swift", "Swift package manifest"],
  ["docs/native/command-map.json", "frozen command inventory"],
  ["docs/native/feature-matrix.json", "feature matrix"],
]) {
  checks.push({ name: probe[1], argv: probe[0], exit: 0, ok: existsSync(path.join(repoRoot, probe[0])), detail: "" });
}

const failed = checks.filter((c) => !c.ok);
for (const c of checks) {
  console.log(`${c.ok ? "ok  " : "MISS"}  ${c.name.padEnd(52)} ${c.detail}`);
}
if (failed.length) {
  console.error(`\ndoctor: ${failed.length}/${checks.length} required item(s) missing. Nothing was installed or changed; resolve these before building.`);
  console.error("Agent must not accept the Xcode license, disable SIP/Gatekeeper or use sudo on the user's behalf.");
  process.exit(1);
}
console.log(`\ndoctor: all ${checks.length} checks satisfied`);
