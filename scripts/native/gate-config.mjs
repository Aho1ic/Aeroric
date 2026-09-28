// Shared by scripts/native/check.mjs (the gate runner) and its tests.
//
// check.mjs is a script with top-level side effects and cannot be imported without
// running it, so the one rule that decides whether a module counts as a real gate
// lives here instead of being duplicated.
/**
 * One module is "ready" only if it is registered **and** declares at least one
 * runnable case with a usable shape.
 *
 * Measured reason this exists: `native:check protocol` with `"cases": []` used to
 * print "0 case(s) passed" and **exit 0** — a green gate that certified nothing,
 * reachable the moment P2-01 registers its module. `all` had the same hole, and its
 * error text claimed to test for exactly that while only testing whether the module
 * key existed.
 */
/**
 * A scan that "found nothing" is only a pass if it could have found something.
 *
 * Measured reason: with `tauri::AppHandle` sitting in `core/crates/aeroric-core/src/`,
 * adding one line to a `.gitignore` in that same directory made the INV-02 guard exit 1
 * with empty output — i.e. report *no forbidden matches* while the violation was on
 * disk. An empty directory behaves the same way. Each `absent` case therefore declares
 * `controls`: one probe per scanned path that must find content, or the case fails.
 */
export function absenceControlFailure(name, controlArgv, exitCode) {
  if (exitCode === 0) return null;
  const target = controlArgv[controlArgv.length - 1] ?? controlArgv.join(" ");
  return `${name}: control probe on "${target}" found no content (exit ${exitCode}) — refusing to read an unreadable or empty corpus as absence`;
}

/** `forbid`-style purity checks need a positive control too: the output must be the
 *  output of a real resolution, not an empty or truncated one. */
export function missingOutputTokens(testCase, haystack) {
  return (testCase.mustContain ?? []).filter((token) => !String(haystack).includes(String(token)));
}

export function moduleGateProblem(name, entry) {
  // §3.2 lets nobody redefine `all`. The tempting "fix" for the matrix was to register a module
  // literally named "all" with a hand-copied case list — that both drifts from the matrix and
  // recurses (check.mjs spawns itself per module). Derive the run set from `modules` instead.
  if (name === "all") {
    return `all: "all" is derived from the registered matrix, so it must not be a hand-maintained module (§3.2)`;
  }
  if (!entry) return `${name}: not registered in checks.json`;
  if (!Array.isArray(entry.cases)) return `${name}: "cases" is missing or not an array`;
  if (entry.cases.length === 0) return `${name}: registered with zero cases — a gate that runs nothing cannot certify anything`;
  const malformed = entry.cases.filter(
    (c) => !c || typeof c !== "object" || typeof c.name !== "string" || !c.name
      || !Array.isArray(c.argv) || c.argv.length === 0 || typeof c.argv[0] !== "string",
  );
  if (malformed.length) return `${name}: ${malformed.length} case(s) without a name or a non-empty string argv`;
  // A new absence guard that could pass without ever reading content must not be
  // registerable — that is how INV-02's scan got defeated by one ignore file.
  const uncontrolled = entry.cases.filter(
    (c) => c.kind === "absent"
      && (!Array.isArray(c.controls) || c.controls.length === 0
        || c.controls.some((a) => !Array.isArray(a) || a.length === 0 || typeof a[0] !== "string")),
  );
  if (uncontrolled.length) {
    return `${name}: ${uncontrolled.length} "absent" case(s) declare no usable controls (${uncontrolled.map((c) => c.name).join(", ")}) — they cannot prove the corpus was read`;
  }
  const unassertive = entry.cases.filter((c) => c.kind === "forbid" && (!Array.isArray(c.mustContain) || c.mustContain.length === 0));
  if (unassertive.length) {
    return `${name}: ${unassertive.length} "forbid" case(s) declare no mustContain (${unassertive.map((c) => c.name).join(", ")}) — an empty listing would pass them`;
  }
  return null;
}

/**
 * How many cases a test-kind run actually executed, per reporter.
 *
 * This used to live inline in `check.mjs`, where nothing could assert against it —
 * and a wrong count is a wrong floor: SwiftPM 6.4 repeats `Executed N tests` per
 * bundle (summing reads 12 as 36), and a reporter that changes its summary line
 * would silently report zero, which the runner treats as a failure only because
 * `minCases` says so. Keep every branch here pinned by `check-runner.test.mjs`.
 */
export function countCases(kind, raw) {
  // Colours survive a pipe (vitest emits them even when captured), and an escape
  // sequence inside `Tests  17 passed` reads as zero executed cases — which this
  // runner treats as a failure, so the noise has to go before any matching.
  const text = String(raw).replace(/\u001b\[[0-9;]*m/g, "");
  if (kind === "rust") {
    return [...text.matchAll(/^test result: \w+\. (\d+) passed/gm)].reduce((sum, m) => sum + Number(m[1]), 0);
  }
  if (kind === "swift") {
    // Per-case lines are the only exact source; the totals line is a fallback for a
    // reporter that prints only summaries, and then only its maximum is trustworthy.
    const cases = [...text.matchAll(/^Test Case '.*' (passed|failed)/gm)].length;
    if (cases > 0) return cases;
    const totals = [...text.matchAll(/Executed (\d+) tests/g)];
    return totals.length ? Math.max(...totals.map((m) => Number(m[1]))) : 0;
  }
  if (kind === "vitest") {
    // `Tests  17 passed (17)` — and never `Test Files  1 passed (1)`, which would
    // report one case for a suite of hundreds.
    const summary = text.match(/^\s*Tests\s+(\d+) passed/m);
    if (summary) return Number(summary[1]);
    return [...text.matchAll(/^\s*[✓√]\s+\S+.*\((\d+) tests?\)/gm)].reduce((sum, m) => sum + Number(m[1]), 0);
  }
  if (kind === "node-test") {
    const m = text.match(/^# pass (\d+)$/m);
    return m ? Number(m[1]) : [...text.matchAll(/^ok \d+ - /gm)].length;
  }
  return 0;
}
