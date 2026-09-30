#!/usr/bin/env node
// `pnpm native:check <module>` — runs the acceptance cases registered in
// scripts/native/checks.json. argv arrays only (no shell string concatenation),
// real exit codes propagated, and a case that executes zero tests is a failure.
// `all` is the complete gate and only becomes runnable at P10-05.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, rmSync, readFileSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { absenceControlFailure, caseSkipReason, countCases, missingOutputTokens, moduleGateProblem } from "./gate-config.mjs";

// Secrets will start flowing past this point at P7-04 (DBX connection strings),
// P8-02 (provider keys) and P8-03 (pairing material). "It is gitignored" is not a
// control: these logs get pasted into review evidence and CI artefacts.
// Order matters: the scheme-specific rules run before the generic key/value rule, which
// otherwise matches `authorization: Bearer` and redacts only the word `Bearer`, leaving
// the token in place — a leak the self-test below caught on its first run.
const SECRET_VALUE = String.raw`(?:"[^"]*"|'[^']*'|[^\s"',;]+)`;
const KV = new RegExp(
  String.raw`(\b(?:password|passwd|secret|token|api[_-]?key|authorization|access[_-]?key|private[_-]?key|client[_-]?secret)\b\s*[=:]\s*)`
  + `((?!Bearer[\s\\W]|Basic[\s\\W])${SECRET_VALUE})`, "gi");

const REDACTIONS = [
  ["pem", /-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g, () => "[redacted:pem]"],
  ["url-credentials", /\b[a-z][a-z0-9+.-]*:\/\/[^/\s:@]+:[^@\s/]+@/gi, (m) => `${m.slice(0, m.indexOf("://") + 3)}[redacted]@`],
  ["scheme-credential", /\b(Bearer|Basic)[ \t]+[A-Za-z0-9._~+\/-]{12,}={0,}/gi, (m, scheme) => `${scheme} [redacted]`],
  ["api-key", /\bsk-[A-Za-z0-9_-]{16,}\b/g, () => "[redacted:api-key]"],
  ["aws-like", /\bAKIA[0-9A-Z]{12,}\b/g, () => "[redacted:access-key]"],
  ["kv", KV, (m, head) => `${head}[redacted]`],
];

function redact(text) {
  let out = text;
  const hits = {};
  for (const [name, pattern, replace] of REDACTIONS) {
    out = out.replace(pattern, (...args) => {
      hits[name] = (hits[name] ?? 0) + 1;
      return replace(...args);
    });
  }
  return { text: out, hits };
}

// `--redact-selftest` proves the scrubbing rules actually fire. A redactor that only
// ever sees benign logs is an untested redactor.
if (process.argv[2] === "--redact-selftest") {
  const fixtures = [
    ["kv", "password=hunter2supersecret", "hunter2supersecret"],
    ["kv-quoted", 'api_key: "AKIAexamplekey1234"', "AKIAexamplekey1234"],
    ["bearer", "Authorization: Bearer abcdefghijklmnop12345678", "abcdefghijklmnop12345678"],
    ["basic", "curl -H 'Authorization: Basic ZGVmYXVsdDpwYXNzd29yZA=='", "ZGVmYXVsdDpwYXNzd29yZA"],
    ["access-key", "aws-like id AKIAABCDEFGHIJKLMNOP used here", "AKIAABCDEFGHIJKLMNOP"],
    ["url", "postgres://admin:totallysecret@db.internal:5432/app", "totallysecret"],
    ["pem", "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJAAAAAAAAAAAA\n-----END RSA PRIVATE KEY-----", "MIIBOgIBAAJ"],
    ["api-key", "using sk-abcdefghijklmnopqrstuvwxyz1234 now", "sk-abcdefghijklmno"],
  ];
  let leaked = 0;
  for (const [name, input, secret] of fixtures) {
    const { text, hits } = redact(input);
    const clean = !text.includes(secret) && Object.keys(hits).length > 0;
    console.log(`${clean ? "ok  " : "LEAK"}  ${name.padEnd(12)} ${clean ? text : `still contains ${JSON.stringify(secret)} -> ${text}`}`);
    if (!clean) leaked += 1;
  }
  if (leaked) { console.error(`redact-selftest: ${leaked}/${fixtures.length} pattern(s) failed to scrub`); process.exit(1); }
  console.log(`redact-selftest: all ${fixtures.length} patterns scrubbed`);
  process.exit(0);
}

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const checksPath = path.join(repoRoot, "scripts", "native", "checks.json");
const config = JSON.parse(readFileSync(checksPath, "utf8"));
const artifactsDir = path.join(repoRoot, config.runner.artifactsDir);

const module = process.argv[2];
if (!module) {
  console.error("usage: native:check <module|all> | --redact-selftest");
  console.error(`known modules: ${Object.keys(config.modules).sort().join(", ")}`);
  process.exit(2);
}

if (module === "all") {
  const planned = Object.keys(config.plannedModules ?? {}).filter((k) => !k.startsWith("$"));
  const problems = planned.map((m) => moduleGateProblem(m, config.modules[m])).filter(Boolean);
  if (problems.length) {
    console.error(`native:check all — NOT READY: ${problems.length}/${planned.length} planned modules are not gate-ready (${problems.slice(0, 3).join("; ")}…).`);
    console.error("P10-05 owns the first full gate; 'all' must never shrink to whatever happens to exist now.");
    process.exit(3);
  }

  // §3.2 defines `all` as the complete matrix, so the run set is DERIVED from what is
  // registered. Before this, the pre-flight fell through to `config.modules["all"]`, which
  // does not exist, so `all` printed "unknown module" and exited 2 — the command could never
  // run anything, no matter how complete the matrix became. A module literally named "all"
  // is refused by gate-config (it would recurse here), so the only honest source is `modules`.
  const derived = Object.keys(config.modules).filter((name) => name !== "all");
  if (!derived.length) {
    console.error("native:check all — no registered module to run; an empty matrix is not a pass.");
    process.exit(1);
  }

  const self = fileURLToPath(import.meta.url);
  const failed = [];
  for (const name of derived) {
    const cases = config.modules[name].cases ?? [];
    const budget = cases.reduce((sum, c) => sum + (c.timeoutMs ?? config.runner.defaultTimeoutMs), 0) + 60_000;
    process.stdout.write(`\n══ native:check ${name} ══\n`);
    const child = await run([process.execPath, self, name], budget);
    process.stdout.write(redact(`${child.stdout}${child.stderr}`).text);
    if (child.code !== 0) failed.push(`${name} (exit ${child.code})`);
  }
  if (failed.length) {
    console.error(`\nnative:check all: ${failed.length}/${derived.length} module(s) failed: ${failed.join(", ")}`);
    process.exit(1);
  }
  console.log(`\nnative:check all: all ${derived.length} registered module(s) passed`);
  process.exit(0);
}

const entry = config.modules[module];
if (!entry) {
  console.error(`native:check ${module} — unknown module. Registered: ${Object.keys(config.modules).sort().join(", ")}`);
  console.error("Refusing to treat an unmapped module as success.");
  process.exit(2);
}
const entryProblem = moduleGateProblem(module, entry);
if (entryProblem) {
  console.error(`native:check ${entryProblem}`);
  process.exit(1);
}


function run(argv, timeoutMs, env = process.env) {
  return new Promise((resolve) => {
    const [command, ...args] = argv;
    const child = spawn(command, args, { cwd: repoRoot, shell: false, env });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      resolve({ code: "timeout", stdout, stderr: `${stderr}\n<timed out after ${timeoutMs}ms>` });
    }, timeoutMs);
    child.stdout.on("data", (d) => { stdout += d.toString(); });
    child.stderr.on("data", (d) => { stderr += d.toString(); });
    child.on("error", (error) => {
      clearTimeout(timer);
      resolve({ code: "spawn-failed", stdout, stderr: `${stderr}\n${error.message}` });
    });
    child.on("close", (code) => { clearTimeout(timer); resolve({ code, stdout, stderr }); });
  });
}

mkdirSync(artifactsDir, { recursive: true });
let failures = 0;
const summary = [];
const skipped = [];

for (const testCase of entry.cases) {
  const timeoutMs = testCase.timeoutMs ?? config.runner.defaultTimeoutMs;
  process.stdout.write(`▶ ${testCase.name} … `);
  const reason = caseSkipReason(testCase, process.env);
  if (reason) {
    // Opt-in cases are the only skips the runner makes. Saying so on every line, and never
    // counting the skip as a pass, is what keeps §3.2's "a skipped required case fails" true.
    console.log(`SKIP (opt-in): ${reason}`);
    skipped.push({ name: testCase.name, reason });
    continue;
  }
  // `case.env` with the `<scratch>` sentinel gets a fresh temp dir per run
  // (P3-06 / G39): the host suite writes its data root through the single
  // `AERORIC_HOME` override, so the default gate no longer touches the real
  // `~/.aeroric` and the two formerly opt-in cases return to required.
  let scratch = null;
  const childEnv = { ...process.env };
  if (testCase.env && testCase.env.AERORIC_HOME === "<scratch>") {
    scratch = mkdtempSync(path.join(tmpdir(), "aeroric-scratch-"));
    childEnv.AERORIC_HOME = scratch;
  }
  const { code, stdout, stderr } = await run(testCase.argv, timeoutMs, childEnv);
  if (scratch) {
    rmSync(scratch, { recursive: true, force: true });
  }
  const logName = `${module}--${testCase.name}.log`;
  const raw = `$ ${testCase.argv.join(" ")}\nexit=${code}\n--- stdout ---\n${stdout}\n--- stderr ---\n${stderr}`;
  const { text: safe, hits } = redact(raw);
  // Digest of the *unredacted* bytes: proof of what was captured without keeping the
  // secret. A redaction that cannot be attested to invites doubt about itself.
  const digest = createHash("sha256").update(raw).digest("hex");
  writeFileSync(path.join(artifactsDir, logName), `# raw-sha256=${digest}\n# redactions=${JSON.stringify(hits)}\n${safe}`);

  if (testCase.kind === "absent") {
    // "Found nothing" is only a pass if the corpus was actually readable: an ignored
    // or empty directory produces the same exit 1 as a genuinely clean tree.
    let controlProblem = null;
    for (const controlArgv of testCase.controls ?? []) {
      if (controlProblem) break;
      const control = await run(controlArgv, timeoutMs);
      controlProblem = absenceControlFailure(testCase.name, controlArgv, control.code);
    }
    if (controlProblem) { console.log(`FAIL (${controlProblem})`); failures += 1; continue; }
    // `rg`/`grep` convention: exit 1 means "found nothing", which is the pass case.
    // Any other non-zero is a broken scanner and must NOT be read as absence.
    if (code === 1) { console.log("ok (no forbidden matches)"); summary.push({ name: testCase.name, cases: null }); continue; }
    if (code === 0) {
      const shown = stdout.split("\n").filter(Boolean).slice(0, 6).join("\n      ");
      console.log(`FAIL (forbidden matches present)\n      ${shown}`);
      failures += 1; continue;
    }
    console.log(`FAIL (scanner exited ${code}; a broken scan is not absence)`);
    failures += 1; continue;
  }

  if (testCase.kind === "forbid") {
    const haystack = (testCase.stream === "stdout" ? stdout : stdout + stderr).toLowerCase();
    const found = testCase.forbid.filter((token) => haystack.includes(String(token).toLowerCase()));
    if (code !== 0) { console.log(`FAIL (command exit ${code})`); failures += 1; continue; }
    if (found.length) { console.log(`FAIL (forbidden deps: ${found.join(", ")})`); failures += 1; continue; }
    const missing = missingOutputTokens(testCase, haystack);
    if (missing.length) {
      console.log(`FAIL (output lacked positive control: ${missing.join(", ")} — an empty or truncated listing is not purity)`);
      failures += 1; continue;
    }
    console.log("ok (no forbidden deps)");
    summary.push({ name: testCase.name, cases: null });
    continue;
  }

  if (code !== 0) { console.log(`FAIL (exit ${code})`); failures += 1; continue; }

  if (testCase.kind === "test") {
    const executed = countCases(testCase.parse, stdout + stderr);
    const min = testCase.minCases ?? 1;
    if (executed === 0) { console.log("FAIL (runner exited 0 but executed 0 cases)"); failures += 1; continue; }
    if (executed < min) { console.log(`FAIL (${executed} cases < required ${min})`); failures += 1; continue; }
    console.log(`ok (${executed} cases, min ${min})`);
    summary.push({ name: testCase.name, cases: executed });
    continue;
  }

  console.log("ok");
  summary.push({ name: testCase.name, cases: null });
}

const totalCases = summary.reduce((sum, s) => sum + (s.cases ?? 0), 0);
writeFileSync(path.join(artifactsDir, `check-${module}.json`), JSON.stringify({ module, failures, totalCases, skipped, summary }, null, 2));
const skipNote = skipped.length ? ` (${skipped.length} opt-in skip: ${skipped.map((s) => s.name).join(", ")})` : "";
if (failures) {
  console.error(`\nnative:check ${module}: ${failures} case(s) failed, ${totalCases} test cases executed${skipNote}`);
  process.exit(1);
}
// "passed" must never count a skip: the module-level run used to print 12 case(s) passed while
// two of them had just rewritten the user's real credential sidecars.
console.log(`\nnative:check ${module}: ${entry.cases.length - skipped.length} case(s) passed, ${totalCases} test cases executed${skipNote}`);
