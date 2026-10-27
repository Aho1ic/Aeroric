import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  assertRealUITestsAllowed, assertVMProvisioningAllowed, readUITestPolicy, UI_TEST_POLICY_EXIT_CODE,
} from "./ui-test-policy.mjs";

const expectedPolicy = {
  version: 1,
  date: "2026-10-19",
  realUITests: "paused",
  vmProvisioning: "forbidden",
  resumesOnlyBy: "explicit new user authorization and reviewed policy update",
};
const policyModule = new URL("./ui-test-policy.mjs", import.meta.url);
const runner = fileURLToPath(new URL("./ui-test.mjs", import.meta.url));
const artifacts = fileURLToPath(new URL("../../.artifacts/native/", import.meta.url));
const trapToken = "AERORIC_UI_POLICY_UNEXPECTED_SIDE_EFFECT";

function fixture(t, contents = JSON.stringify(expectedPolicy)) {
  const root = mkdtempSync(path.join(realpathSync(os.tmpdir()), "aeroric-ui-policy-fixture-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const home = path.join(root, "private-fixture-home");
  const bin = path.join(root, "bin");
  const trapFile = path.join(root, "unexpected-tool");
  const preload = path.join(root, "deny-side-effects.cjs");
  mkdirSync(bin);
  for (const name of ["node", "pnpm", "xcrun", "swiftc", "xcodebuild", "xcodegen", "open", "osascript", "tart", "ps", "python3"]) {
    writeFileSync(path.join(bin, name),
      `#!/bin/sh\nprintf '${trapToken}\\n' >&2\nprintf 'unexpected\\n' > "$AERORIC_UI_POLICY_TRAP_FILE"\nexit 99\n`,
      { mode: 0o700 });
  }
  // Catch absolute subprocess paths too, and prevent any accidental runner write
  // even if the gate regresses. Only this temporary fixture is ever writable.
  writeFileSync(preload, `
const fs = require("node:fs");
const cp = require("node:child_process");
const { syncBuiltinESMExports } = require("node:module");
function deny() {
  process.stderr.write("${trapToken}\\n");
  throw new Error("${trapToken}");
}
for (const name of ["spawn", "spawnSync", "exec", "execSync", "execFile", "execFileSync", "fork"]) cp[name] = deny;
for (const name of ["mkdir", "mkdirSync", "mkdtemp", "mkdtempSync", "writeFile", "writeFileSync",
  "appendFile", "appendFileSync", "write", "writeSync", "rename", "renameSync", "rm", "rmSync",
  "unlink", "unlinkSync", "symlink", "symlinkSync", "copyFile", "copyFileSync", "truncate", "truncateSync"]) fs[name] = deny;
const openSync = fs.openSync;
fs.openSync = function(candidate, flags, ...rest) {
  if (typeof flags === "string" ? !["r", "rs", "sr"].includes(flags)
    : (flags & (fs.constants.O_WRONLY | fs.constants.O_RDWR | fs.constants.O_CREAT | fs.constants.O_TRUNC | fs.constants.O_APPEND))) deny();
  return openSync(candidate, flags, ...rest);
};
syncBuiltinESMExports();
`);
  const modulePath = path.join(root, "ui-test-policy.mjs");
  copyFileSync(policyModule, modulePath);
  if (contents !== null) writeFileSync(path.join(root, "ui-test-policy.json"), contents);
  return {
    root, home, trapFile, preload, modulePath,
    env: { HOME: home, PATH: bin, TMPDIR: root, AERORIC_UI_POLICY_TRAP_FILE: trapFile },
  };
}

function refused(result, f) {
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  assert.equal(result.status, 78);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /PAUSED/);
  assert.match(result.stderr, /NOT EXECUTED \/ NOT PASSED/);
  assert.match(result.stderr, /VM provisioning FORBIDDEN/);
  assert.match(result.stderr, /2026-10-19-ui-automation-pause\.md/);
  assert.equal(result.stderr.includes(trapToken), false);
  for (const secret of [f.root, f.home, "private-policy-fixture-value"]) {
    assert.equal(result.stderr.includes(secret), false);
  }
  assert.equal(existsSync(f.home), false, "runner must not create HOME or UI resource roots");
  assert.equal(existsSync(f.trapFile), false, "no PATH stub may run");
}

function spawnAssertion(f, name, overrides = {}, args = []) {
  const source = `
import { ${name} } from ${JSON.stringify(pathToFileURL(f.modulePath).href)};
try { ${name}(); } catch (error) {
  if (error.code !== "ERR_AERORIC_UI_TEST_POLICY" || error.exitCode !== 78) throw error;
  console.error(error.message);
  process.exit(error.exitCode);
}
throw new Error("policy unexpectedly allowed execution");
`;
  return spawnSync(process.execPath, ["--require", f.preload, "--input-type=module", "--eval", source, "--", ...args], {
    cwd: f.root, env: { ...f.env, ...overrides }, encoding: "utf8", timeout: 10000,
  });
}

function artifactState() {
  if (!existsSync(artifacts)) return null;
  return readdirSync(artifacts).filter((name) => name.startsWith("ui-test-")).sort().map((name) => {
    const info = lstatSync(path.join(artifacts, name));
    return [name, info.dev, info.ino, info.size, info.mtimeMs, info.ctimeMs];
  });
}

test("shipped v1 policy is read-only, paused and forbidden; both assertions refuse", () => {
  assert.deepEqual(readUITestPolicy(), expectedPolicy);
  assert.equal(Object.isFrozen(readUITestPolicy()), true);
  assert.equal(UI_TEST_POLICY_EXIT_CODE, 78);
  for (const assertion of [assertRealUITestsAllowed, assertVMProvisioningAllowed]) {
    assert.throws(assertion, {
      code: "ERR_AERORIC_UI_TEST_POLICY", exitCode: 78,
      message: /PAUSED: real UI tests NOT EXECUTED \/ NOT PASSED; VM provisioning FORBIDDEN/,
    });
  }
});

test("official UI entry refuses with 78 before HOME, original logs, resources or tools", (t) => {
  const f = fixture(t);
  const before = artifactState();
  const result = spawnSync(process.execPath, ["--require", f.preload, runner,
    "--force", "--allow-real-ui-tests", "--allow-vm-provisioning", "--review-token=fixture"], {
    cwd: f.root,
    env: {
      ...f.env,
      AERORIC_ALLOW_REAL_UI_TESTS: "1",
      AERORIC_UI_TESTS_ALLOWED: "1",
      AERORIC_UI_TEST_POLICY: "allowed",
      AERORIC_VM_PROVISIONING: "allowed",
      AERORIC_UI_TEST_RESUME_TOKEN: "reviewed",
    },
    encoding: "utf8", timeout: 10000,
  });
  refused(result, f);
  assert.deepEqual(artifactState(), before, "original UI logs and resources must remain unchanged");
});

test("fixture assertions ignore env and argv attempts to authorize UI or VM work", (t) => {
  const f = fixture(t);
  for (const name of ["assertRealUITestsAllowed", "assertVMProvisioningAllowed"]) {
    refused(spawnAssertion(f, name, {
      AERORIC_ALLOW_REAL_UI_TESTS: "1",
      AERORIC_VM_PROVISIONING: "allowed",
      AERORIC_UI_TEST_POLICY: "allowed",
      AERORIC_UI_TEST_RESUME_TOKEN: "reviewed",
    }, ["--force", "--allow-real-ui-tests", "--allow-vm-provisioning", "--review-token=fixture"]), f);
  }
});

for (const [name, contents] of [
  ["missing JSON", null],
  ["malformed JSON", '{"private-policy-fixture-value":'],
  ["null root", "null"],
  ["array root", "[]"],
  ["missing field", JSON.stringify({ ...expectedPolicy, resumesOnlyBy: undefined })],
  ["wrong field type", JSON.stringify({ ...expectedPolicy, version: "1" })],
  ["unknown version", JSON.stringify({ ...expectedPolicy, version: 2 })],
  ["unreviewed date", JSON.stringify({ ...expectedPolicy, date: "2026-10-20" })],
  ["unknown UI state", JSON.stringify({ ...expectedPolicy, realUITests: "private-policy-fixture-value" })],
  ["unknown VM state", JSON.stringify({ ...expectedPolicy, vmProvisioning: "unknown" })],
  ["UI allowed without source review", JSON.stringify({ ...expectedPolicy, realUITests: "allowed" })],
  ["VM allowed without source review", JSON.stringify({ ...expectedPolicy, vmProvisioning: "allowed" })],
  ["altered resumption condition", JSON.stringify({ ...expectedPolicy, resumesOnlyBy: "environment" })],
  ["injected review token", JSON.stringify({ ...expectedPolicy, realUITests: "allowed", vmProvisioning: "allowed", reviewToken: "reviewed" })],
]) {
  test(`${name} fails closed for UI and VM using only copied policy module and JSON`, (t) => {
    const f = fixture(t, contents);
    for (const assertion of ["assertRealUITestsAllowed", "assertVMProvisioningAllowed"]) {
      refused(spawnAssertion(f, assertion), f);
    }
  });
}

test("policy is re-read, not cached after a valid read", async (t) => {
  const f = fixture(t);
  const policy = await import(pathToFileURL(f.modulePath).href);
  assert.deepEqual(policy.readUITestPolicy(), expectedPolicy);
  writeFileSync(path.join(f.root, "ui-test-policy.json"), "{}");
  assert.throws(policy.readUITestPolicy, /Unsupported policy schema or state/);
  for (const assertion of [policy.assertRealUITestsAllowed, policy.assertVMProvisioningAllowed]) {
    assert.throws(assertion, { code: "ERR_AERORIC_UI_TEST_POLICY", exitCode: 78 });
  }
});

test("gate precedes existing runner work and retains all 33 method registrations", () => {
  const source = readFileSync(runner, "utf8");
  const gate = source.indexOf("\n  assertRealUITestsAllowed();");
  assert.ok(gate > 0);
  for (const marker of ["const repoRoot =", "const RUN_ID =", "lifecycle.reserve();",
    "const compile =", "if (!existsSync(APP))", "await lifecycle.warmup();", "const logHandle = openSync(LOG"]) {
    assert.ok(source.indexOf(marker) > gate, `${marker} must follow the pause gate`);
  }
  assert.equal([...source.matchAll(/^\s*"(AeroricNativeUITests\/[^"]+)"/gm)].length, 33);
  assert.match(source, /attempt < 2 && !passed/);
  assert.match(source, /if \(missing\.length > 0\)/);
});
